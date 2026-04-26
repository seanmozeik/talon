//! Full hybrid search pipeline orchestrator.
//!
//! Wires together a lexical probe, optional LLM expansion (US-001),
//! per-variant hybrid retrieval (US-003), cross-variant RRF fusion, and
//! cross-encoder reranking (US-004).
//!
//! Ports `services/talon/search/hybrid-pipeline.ts`.

use rusqlite::Connection;

use crate::expansion::client::ExpansionClient;
use crate::inference::InferenceClient;

use super::bm25::search_bm25;
use super::cache::dedupe_query_variants;
use super::constants::{
    DEFAULT_SNIPPET_LENGTH, HYBRID_PROBE_LEXICAL_LIMIT, HYBRID_PROBE_TITLE_LIMIT, RERANK_TOP_K,
};
use super::fuse::{estimate_strong_signal, fuse_hybrid_result_lists};
use super::fuzzy_title::search_title_parts;
use super::hybrid_single::{HybridSingleResult, run_hybrid_single};
use super::rerank_pipeline::rerank_candidates;
use super::rrf::{RrfInputs, RrfList, RrfScoreAccumulator, normalize_and_merge_rrf_results};
use super::types::{HybridScoreData, RawSearchResult, SearchScores};

/// Default number of LLM expansion variants to request per query.
const EXPANSION_N_VARIANTS: u8 = 3;

/// Options for [`run_hybrid_pipeline`].
#[derive(Debug, Clone)]
pub struct HybridPipelineOptions {
    /// Maximum results to return.
    pub limit: u32,
    /// Skip LLM expansion and cross-encoder reranking when true.
    pub fast: bool,
    /// Pre-supplied query variants (bypass LLM call when non-empty).
    pub queries: Vec<String>,
}

/// Runs the full hybrid search pipeline:
///   probe → optional LLM expansion → per-variant retrieval → fusion → rerank.
///
/// **Short-circuit rules:**
/// - `fast=true` or a decisive BM25 probe (`estimate_strong_signal`) skips
///   both LLM expansion and reranking.
/// - An exact-alias hit during the title probe also skips LLM expansion
///   (the alias is already a confident match).
/// - `options.queries` non-empty bypasses the LLM and uses the supplied
///   variants directly.
///
/// **Graceful degradation:** embedding failures produce empty vector buckets;
/// expansion failures fall back to the original query; rerank failures return
/// hybrid-scored results unchanged.
#[must_use]
pub fn run_hybrid_pipeline(
    conn: &Connection,
    inference: &InferenceClient,
    expansion: Option<&ExpansionClient>,
    query: &str,
    options: &HybridPipelineOptions,
) -> Vec<RawSearchResult> {
    // Lexical-only probe to detect high-confidence matches before paying for
    // the embedding + expansion + rerank round-trips.
    let bm25_probe = search_bm25(
        conn,
        query,
        HYBRID_PROBE_LEXICAL_LIMIT,
        DEFAULT_SNIPPET_LENGTH,
    );
    let title_probe = search_title_parts(conn, query, HYBRID_PROBE_TITLE_LIMIT);

    let has_supplied = !options.queries.is_empty();
    let has_exact_alias = !title_probe.exact_alias.is_empty();
    let probe_decisive = estimate_strong_signal(&bm25_probe);

    // A decisive probe or fast mode skips both expansion and reranking.
    let skip_expensive = options.fast || probe_decisive;
    // An exact alias hit additionally skips LLM expansion (not reranking).
    let skip_llm = skip_expensive || has_exact_alias;

    // Resolve variants: supplied → deduped supplied; bypass → []; else → LLM.
    let variants: Vec<String> = if has_supplied {
        dedupe_query_variants(&options.queries)
    } else if skip_llm {
        vec![]
    } else if let Some(exp) = expansion {
        exp.expand(query, EXPANSION_N_VARIANTS).unwrap_or_default()
    } else {
        vec![]
    };

    // Build the final query list.
    let queries_to_search: Vec<String> = if has_supplied {
        if variants.is_empty() {
            vec![query.to_owned()]
        } else {
            variants
        }
    } else if variants.is_empty() {
        vec![query.to_owned()]
    } else {
        let mut v = vec![query.to_owned()];
        v.extend(variants);
        v
    };

    // Per-variant: embed → retrieve (BM25 + fuzzy + vector) → intra-variant RRF.
    let per_variant: Vec<Vec<RawSearchResult>> = queries_to_search
        .iter()
        .map(|q| {
            let embedding = inference
                .embed(std::slice::from_ref(q))
                .ok()
                .and_then(|mut vecs| vecs.pop());
            let single = run_hybrid_single(conn, q, embedding.as_deref(), options.limit);
            single_to_raw_list(&single, options.limit as usize)
        })
        .collect();

    // Cross-variant RRF fusion.
    let list_refs: Vec<&[RawSearchResult]> = per_variant.iter().map(Vec::as_slice).collect();
    let fused = fuse_hybrid_result_lists(&list_refs, options.limit as usize);

    // Rerank unless the probe gave us high confidence or fast mode is active.
    if skip_expensive {
        fused
    } else {
        rerank_candidates(inference, query, fused, RERANK_TOP_K)
    }
}

/// Runs per-signal weighted RRF on the three [`HybridSingleResult`] buckets
/// and converts the output to [`RawSearchResult`] for cross-variant fusion.
fn single_to_raw_list(single: &HybridSingleResult, limit: usize) -> Vec<RawSearchResult> {
    let mut acc = RrfScoreAccumulator::new();
    acc.accumulate(&single.vector, RrfList::Semantic);
    acc.accumulate(&single.bm25, RrfList::Bm25);
    acc.accumulate(&single.fuzzy_title_parts.exact_alias, RrfList::ExactAlias);
    acc.accumulate(&single.fuzzy_title_parts.fuzzy, RrfList::Fuzzy);

    let inputs = RrfInputs {
        semantic: &single.vector,
        bm25: &single.bm25,
        exact_alias: &single.fuzzy_title_parts.exact_alias,
        fuzzy: &single.fuzzy_title_parts.fuzzy,
    };

    normalize_and_merge_rrf_results(&acc, &inputs, limit)
        .iter()
        .map(hybrid_data_to_raw)
        .collect()
}

/// Converts [`HybridScoreData`] (post-RRF) to [`RawSearchResult`].
fn hybrid_data_to_raw(h: &HybridScoreData) -> RawSearchResult {
    RawSearchResult {
        path: h.path.clone(),
        title: h.title.clone(),
        tags: h.tags.clone(),
        aliases: h.aliases.clone(),
        snippet: h.snippet.clone(),
        score: h.hybrid_before_norm.unwrap_or(0.0),
        scores: SearchScores {
            bm25: h.bm25,
            fuzzy_title: h.fuzzy_title,
            hybrid: h.hybrid_before_norm,
            semantic: h.semantic,
            rerank: None,
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::expansion::client::ExpansionClient;
    use crate::inference::InferenceClient;
    use crate::store::open_database;
    use rusqlite::params;
    use serde_json::json;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_db_path() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-hybrid-pipeline-test-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn insert_note(conn: &Connection, vault_path: &str, title: &str, content: &str) {
        conn.execute(
            "INSERT INTO notes \
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active) \
             VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, content],
        )
        .unwrap();
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn dummy_embed_response() -> serde_json::Value {
        // 3-dim vector — search_vector returns empty on empty vec_chunks regardless.
        json!([[0.1_f32, 0.2_f32, 0.3_f32]])
    }

    // ── Test 1: full pipeline end-to-end ────────────────────────────────────

    #[test]
    fn full_pipeline_calls_embed_expand_and_rerank() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());

        // /embed: returns a dummy vector for each query call.
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
                .mount(&server),
        );

        // /chat/completions: returns two expansion variants.
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {
                            "content": "{\"queries\":[\"atomic ideas\",\"note taking systems\"]}"
                        }
                    }]
                })))
                .mount(&server),
        );

        // /rerank: boosts the target note to rank 0 with high score.
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    {"index": 0, "score": 0.95}
                ])))
                .mount(&server),
        );

        let db_path = unique_db_path();
        let conn = open_database(&db_path).unwrap();

        // Seed: a few background notes + one target.
        insert_note(
            &conn,
            "unrelated-a.md",
            "Chemistry Notes",
            "periodic table elements",
        );
        insert_note(
            &conn,
            "unrelated-b.md",
            "History Notes",
            "ancient civilizations events",
        );
        insert_note(
            &conn,
            "target.md",
            "Zettelkasten Method",
            "atomic notes for thinking and learning",
        );

        let inference = InferenceClient::new(server.uri()).unwrap();
        let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

        let opts = HybridPipelineOptions {
            limit: 10,
            fast: false,
            queries: vec![],
        };

        let results =
            run_hybrid_pipeline(&conn, &inference, Some(&expansion), "atomic notes", &opts);

        assert!(
            !results.is_empty(),
            "pipeline must return at least one result"
        );
        assert!(
            results.iter().any(|r| r.path == "target.md"),
            "target.md must appear in results"
        );

        drop(conn);
        cleanup(&db_path);
    }

    // ── Test 2: strong-signal probe skips expansion and rerank ───────────────

    #[test]
    fn strong_signal_probe_skips_expansion_and_rerank() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());

        // Only /embed is mocked; /chat/completions and /rerank are NOT registered.
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
                .mount(&server),
        );

        let db_path = unique_db_path();
        let conn = open_database(&db_path).unwrap();

        // Insert 100 dummy notes to raise IDF for the unique query term,
        // then the one target note with "crystallophosphene" in its title.
        // High IDF + title weight=10 → BM25 score >= 0.85 → strong signal.
        for i in 0..100 {
            insert_note(
                &conn,
                &format!("dummy-{i}.md"),
                &format!("Unrelated Topic {i}"),
                &format!("content about something completely different topic number {i}"),
            );
        }
        insert_note(
            &conn,
            "signal.md",
            "crystallophosphene Research",
            "unique term found nowhere else",
        );

        let inference = InferenceClient::new(server.uri()).unwrap();
        let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

        let opts = HybridPipelineOptions {
            limit: 10,
            fast: false,
            queries: vec![],
        };

        let results = run_hybrid_pipeline(
            &conn,
            &inference,
            Some(&expansion),
            "crystallophosphene",
            &opts,
        );

        // The probe should detect a strong signal and skip expansion + rerank.
        let received = rt.block_on(server.received_requests()).unwrap_or_default();
        let expansion_count = received
            .iter()
            .filter(|r| r.url.path() == "/chat/completions")
            .count();
        let rerank_count = received
            .iter()
            .filter(|r| r.url.path() == "/rerank")
            .count();

        assert!(
            expansion_count == 0,
            "expansion must not be called when probe is decisive; \
             got {expansion_count} calls to /chat/completions"
        );
        assert!(
            rerank_count == 0,
            "rerank must not be called when probe is decisive; \
             got {rerank_count} calls to /rerank"
        );

        assert!(
            results.iter().any(|r| r.path == "signal.md"),
            "signal.md must appear in results even when short-circuited"
        );

        drop(conn);
        cleanup(&db_path);
    }

    // ── Test 3: fast flag skips expansion and rerank ─────────────────────────

    #[test]
    fn fast_flag_skips_expansion_and_rerank() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());

        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
                .mount(&server),
        );

        let db_path = unique_db_path();
        let conn = open_database(&db_path).unwrap();
        insert_note(
            &conn,
            "note.md",
            "Fast Search Note",
            "fast lexical search content",
        );

        let inference = InferenceClient::new(server.uri()).unwrap();
        let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

        let opts = HybridPipelineOptions {
            limit: 10,
            fast: true,
            queries: vec![],
        };

        let results = run_hybrid_pipeline(&conn, &inference, Some(&expansion), "fast", &opts);

        let received = rt.block_on(server.received_requests()).unwrap_or_default();
        assert!(
            !received.iter().any(|r| r.url.path() == "/chat/completions"),
            "fast mode must not call expansion"
        );
        assert!(
            !received.iter().any(|r| r.url.path() == "/rerank"),
            "fast mode must not call rerank"
        );
        assert!(!results.is_empty(), "fast mode must still return results");

        drop(conn);
        cleanup(&db_path);
    }

    // ── Test 4: no expansion client still returns results ────────────────────

    #[test]
    fn no_expansion_client_returns_results() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());

        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
                .mount(&server),
        );
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
                .mount(&server),
        );

        let db_path = unique_db_path();
        let conn = open_database(&db_path).unwrap();
        insert_note(
            &conn,
            "note.md",
            "Knowledge Base",
            "knowledge management and note taking",
        );

        let inference = InferenceClient::new(server.uri()).unwrap();

        let opts = HybridPipelineOptions {
            limit: 10,
            fast: false,
            queries: vec![],
        };

        // expansion=None: pipeline must degrade gracefully (no LLM call).
        let results = run_hybrid_pipeline(&conn, &inference, None, "knowledge management", &opts);

        assert!(
            !results.is_empty(),
            "pipeline must return results without expansion client"
        );

        drop(conn);
        cleanup(&db_path);
    }

    // ── Test 5: pre-supplied queries bypass LLM ──────────────────────────────

    #[test]
    fn pre_supplied_queries_bypass_llm_expansion() {
        let rt = runtime();
        let server = rt.block_on(MockServer::start());

        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/embed"))
                .respond_with(ResponseTemplate::new(200).set_body_json(dummy_embed_response()))
                .mount(&server),
        );
        rt.block_on(
            Mock::given(method("POST"))
                .and(path("/rerank"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
                .mount(&server),
        );
        // /chat/completions deliberately NOT mocked.

        let db_path = unique_db_path();
        let conn = open_database(&db_path).unwrap();
        insert_note(
            &conn,
            "spaced.md",
            "Spaced Repetition",
            "spaced repetition system for memory",
        );
        insert_note(
            &conn,
            "anki.md",
            "Anki Flashcards",
            "flashcard review system anki",
        );

        let inference = InferenceClient::new(server.uri()).unwrap();
        let expansion = ExpansionClient::new(server.uri(), "test-model").unwrap();

        let opts = HybridPipelineOptions {
            limit: 10,
            fast: false,
            queries: vec!["anki flashcards".to_owned()],
        };

        let results =
            run_hybrid_pipeline(&conn, &inference, Some(&expansion), "memory systems", &opts);

        let received = rt.block_on(server.received_requests()).unwrap_or_default();
        assert!(
            !received.iter().any(|r| r.url.path() == "/chat/completions"),
            "pre-supplied queries must bypass LLM expansion"
        );
        assert!(
            !results.is_empty(),
            "must return results with pre-supplied queries"
        );

        drop(conn);
        cleanup(&db_path);
    }
}
