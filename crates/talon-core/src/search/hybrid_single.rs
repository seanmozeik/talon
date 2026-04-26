//! Single-query hybrid retrieval: runs BM25, fuzzy-title, and vector in one
//! call and returns the three result buckets for downstream RRF fusion.
//!
//! Ports `services/talon/search/hybrid-single.ts`. This is a pure composition
//! layer — all scoring logic lives in the underlying retriever modules.

use rusqlite::Connection;

use super::bm25::search_bm25;
use super::constants::DEFAULT_SNIPPET_LENGTH;
use super::fuzzy_title::{TitleSearchParts, search_title_parts};
use super::types::RawSearchResult;
use super::vector::search_vector;

/// The three result buckets produced by a single hybrid retrieval pass.
///
/// Kept separate so the RRF layer can apply independent weights:
/// `bm25` (weight 2.0), `fuzzy_title_parts.exact_alias` (weight 2.0),
/// `fuzzy_title_parts.fuzzy` (weight 0.5), `vector` (weight 1.0).
#[derive(Debug, Default, Clone)]
pub struct HybridSingleResult {
    pub bm25: Vec<RawSearchResult>,
    pub fuzzy_title_parts: TitleSearchParts,
    pub vector: Vec<RawSearchResult>,
}

/// Runs BM25, fuzzy-title, and (optionally) vector retrieval for `query`,
/// returning the three result buckets.
///
/// `embedding` should be `None` when no vector sidecar call was made (e.g.
/// fulltext-only mode). In that case the `vector` bucket is always empty.
///
/// `limit` is forwarded to each retriever unchanged.
#[must_use]
pub fn run_hybrid_single(
    conn: &Connection,
    query: &str,
    embedding: Option<&[f32]>,
    limit: u32,
) -> HybridSingleResult {
    let bm25 = search_bm25(conn, query, limit, DEFAULT_SNIPPET_LENGTH);
    let fuzzy_title_parts = search_title_parts(conn, query, limit);
    let vector = embedding
        .map(|emb| search_vector(conn, emb, limit))
        .unwrap_or_default();

    HybridSingleResult {
        bm25,
        fuzzy_title_parts,
        vector,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::frontmatter::normalize_keyword;
    use crate::store::open_database;
    use rusqlite::params;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir().join(format!("talon-hybrid-single-test-{pid}-{n}.sqlite"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }

    fn insert_note(conn: &Connection, vault_path: &str, title: &str, content: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, content],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_alias(conn: &Connection, note_id: i64, alias: &str) {
        let norm = normalize_keyword(alias);
        conn.execute(
            "INSERT INTO note_aliases (note_id, alias, alias_norm) VALUES (?, ?, ?)",
            params![note_id, alias, norm],
        )
        .unwrap();
    }

    #[test]
    fn bm25_bucket_populated_when_content_matches() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(
            &conn,
            "a.md",
            "Zettelkasten",
            "atomic notes are the foundation",
        );
        insert_note(&conn, "b.md", "Unrelated", "completely different text here");

        let result = run_hybrid_single(&conn, "atomic notes", None, 10);
        assert!(!result.bm25.is_empty(), "bm25 should find content match");
        assert!(result.bm25.iter().any(|r| r.path == "a.md"));
        assert!(result.bm25[0].scores.bm25.is_some());
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn fuzzy_title_bucket_populated_when_title_matches() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        let id = insert_note(&conn, "a.md", "Zettelkasten Method", "body text");
        insert_alias(&conn, id, "Zettelkasten");

        let result = run_hybrid_single(&conn, "zettelkasten", None, 10);

        // exact_alias must fire because we inserted the alias
        assert!(
            !result.fuzzy_title_parts.exact_alias.is_empty(),
            "exact_alias bucket should contain the alias match"
        );
        assert_eq!(result.fuzzy_title_parts.exact_alias[0].path, "a.md");
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn vector_bucket_empty_when_embedding_is_none() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "a.md", "Any Note", "any content");

        let result = run_hybrid_single(&conn, "any", None, 10);
        assert!(
            result.vector.is_empty(),
            "vector bucket must be empty when embedding is None"
        );
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn vector_bucket_empty_when_extension_unavailable() {
        // The test DB opened via open_database bundles sqlite-vec, but if a
        // non-trivial embedding is passed it should still return empty results
        // gracefully when the vec_chunks table is empty (no vectors seeded).
        let path = unique_path();
        let conn = open_database(&path).unwrap();
        insert_note(&conn, "a.md", "Note", "content");

        let emb = vec![0.1_f32; 768];
        let result = run_hybrid_single(&conn, "note", Some(&emb), 10);
        // vec_chunks is empty — vector bucket is empty, no panic.
        assert!(
            result.vector.is_empty(),
            "empty vec_chunks should yield empty vector bucket"
        );
        drop(conn);
        cleanup(&path);
    }

    #[test]
    fn all_buckets_independent_for_disjoint_notes() {
        let path = unique_path();
        let conn = open_database(&path).unwrap();

        // bm25 content match only
        insert_note(
            &conn,
            "bm25_only.md",
            "Random Title A",
            "quantum entanglement physics",
        );

        // fuzzy title match only
        let id = insert_note(
            &conn,
            "fuzzy_only.md",
            "Photosynthesis Process",
            "unrelated body",
        );
        insert_alias(&conn, id, "Photosynthesis");

        let bm25_result = run_hybrid_single(&conn, "quantum entanglement", None, 10);
        assert!(bm25_result.bm25.iter().any(|r| r.path == "bm25_only.md"));
        assert!(
            bm25_result
                .fuzzy_title_parts
                .exact_alias
                .iter()
                .all(|r| r.path != "bm25_only.md")
        );

        let fuzzy_result = run_hybrid_single(&conn, "photosynthesis", None, 10);
        assert!(
            !fuzzy_result.fuzzy_title_parts.exact_alias.is_empty(),
            "fuzzy_only note should appear in exact_alias bucket"
        );
        assert_eq!(
            fuzzy_result.fuzzy_title_parts.exact_alias[0].path,
            "fuzzy_only.md"
        );

        drop(conn);
        cleanup(&path);
    }
}
