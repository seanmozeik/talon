# Search & Retrieval Improvements — Implementation Plan (PRD)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Each user story (`US-XXX`) is one focused task. Acceptance-criteria boxes (`- [ ]`) are the tracking unit.

**Goal:** Port battle-tested retrieval and ranking techniques from `obsidian-hybrid-search` (OHS) and `qmd` into talon, fix one quality regression in `--limit`, tighten one cosmetic correctness gap in the cosine path, and switch vector storage to native int8.

**Architecture:** Changes are localized to `crates/talon-core/src/search/**`, `crates/talon-core/src/embed/**`, `crates/talon-core/src/vec_ext.rs`, and the talon config schema, plus a small CLI/MCP surface for new flags. The local SQLite database will be nuked and re-created with new schemas as needed (FTS5 tokenizer reconfig in US-008, int8 vector storage in US-022) — talon is pre-release with a single in-container consumer, so no migration code is written.

**Tech Stack:** Rust, `rusqlite`, SQLite FTS5, `sqlite-vec`, talon's existing inference sidecar (TEI-compatible: `/embed`, `/rerank`).

**Reference repos** (read-only context for porting math, formulas, and constants):
- `/home/yolo/.opensrc/repos/github.com/flowing-abyss/obsidian-hybrid-search/master` (OHS — **TypeScript**, MIT license)
- `/home/yolo/.opensrc/repos/github.com/tobi/qmd/main` (qmd — TypeScript/Bun)

**Porting policy — don't reinvent OHS math, but don't blindly regress where we're already better.** Where this plan cites OHS line numbers for scoring weights, sigmoid blending, trigram overlap, RRF weights, top-rank bonuses, and similar math, the Rust implementation defaults to **the exact same constants, formulas, and edge-case handling** as OHS. The OHS implementation is benchmark-validated; independently redesigning scoring math is out of scope.

**Exception: where talon already does something genuinely better, keep it.** Examples already identified:
- `tokenx-rs` for token estimation (proper CJK ranges, Cyrillic handling, etc.) is more sophisticated than OHS's `if any(non_ascii) { 1 char/token } else { 0.25 token/char }` heuristic — keep it.
- `text-splitter` crate's overlap semantics are different from OHS's hand-rolled formula — keep unless benchmarked as worse.

When talon intentionally diverges from OHS:
- Inline comment: `// Intentional divergence from OHS searcher.ts:NNN — reason: <X>` near the relevant code.
- Note in the US's commit body explaining why.

When porting OHS verbatim:
- Inline comment: `// Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:NNN-MMM`.

Aggregate attribution lives in `LICENSE-3RD-PARTY.md` (created in US-000).

If a value in OHS conflicts with a value already in talon's `search/constants.rs`, the audit US (US-026) is responsible for deciding case-by-case: align to OHS, or document an intentional divergence.

---

## 1. Introduction / Overview

talon's search currently caps retrieval at the user's `--limit` value, runs `--where` / `--since` / scope filtering on that capped pool, and only then truncates. With `--limit 10 --where status:active`, BM25 returns the top 10 by rank and filters can leave 0–10 results even when the vault holds hundreds of matching `active` notes. The same shape constrains rerank quality: the rerank pool is at most `limit` wide rather than the standard 40.

While auditing the retrieval path against OHS and qmd, we identified ~20 additional, mostly small, retrieval/ranking improvements those projects have hardened over time — RRF tie-breakers, position-aware rerank blending, sigmoid logit conversion, FTS tokenizer fixes (`C++`, `C#`), trigram-overlap² fuzzy scoring, NFD normalization, exact-alias bypass for short tokens, and others. We also confirmed talon is using cosine correctly but has one cosmetic issue where the query embedding is not L2-normalized at search time.

This document bundles all of those changes — high, medium, and low priority — into one ordered plan.

## 2. Goals

- **G-1** — `--limit N` returns up to `N` results across the vault, not "up to N out of the top-N retrieval pool". Filter recall improves materially.
- **G-2** — Rerank quality improves by always seeing a fixed-size candidate pool (default 40), independent of `--limit`.
- **G-3** — Hybrid score blending after rerank is monotonic and well-bounded ([0,1] × [0,1]) instead of mixing a hybrid score with raw logits.
- **G-4** — Tokenizer recall improves on tech tokens (`C++`, `C#`, `gpt-4`, `multi-agent`, `DEC-0054`) without requiring re-tokenization of the query at every site.
- **G-5** — Unicode/NFD handling is uniform across query, alias, tag, and scope paths so macOS NFC vs. NFD vault differences disappear.
- **G-6** — Cosine path is exactly correct (not just scale-invariantly correct) — query and stored vectors both unit-norm.
- **G-7** — Cross-process cache coherency works without polling once MCP and CLI share an index.
- **G-8** — All changes are landed under feature-isolated commits with passing `just check` at each step. No regressions in `tests/ranking_regression/**` golden files.

## 3. Non-Goals

- **NG-1** — No migration code. The local SQLite DB will be deleted and recreated when schema changes land (talon is pre-release with one in-container consumer). Schema files updated in place.
- **NG-2** — No new search modes. No changes to `SearchMode` enum.
- **NG-3** — Sidecar API extensions are limited to the optional `intent` field on `/rerank` and `/expand` (US-019). No other endpoint changes.
- **NG-4** — No CLI output-format changes beyond the new flags introduced explicitly here.
- **NG-5** — No changes to linter config (per CLAUDE.md). All findings fixed via refactor or flagged.
- **NG-6** — OHS's two-pass BM25 anchor lookup is **deferred-conditional** (US-024) on a UI consumer existing. Existing `search/anchor.rs` covers the CLI/MCP cases.
- **NG-7** — No reinvention of OHS scoring math. See "Porting policy" above.

## 4. Functional Requirements

- **FR-1** — Each retriever (`search_bm25`, `search_vector`, `search_fuzzy_title`) is called with a `pool_size`, not the user `limit`. `pool_size` is computed by a shared helper; defaults below.
- **FR-2** — A new constant `CANDIDATE_FLOOR: u32 = 40` decouples the rerank pool size from `--limit`.
- **FR-3** — A new CLI/MCP flag `--candidate-limit <N>` (default `CANDIDATE_FLOOR`) overrides the post-RRF candidate pool size.
- **FR-4** — Post-filters (`apply_where_filter`, `apply_since_filter`, `apply_scope_priority`) run on the full retrieval pool, then `truncate(limit)` is the final step.
- **FR-5** — `total` in `SearchResponse` reports post-filter, pre-truncate count (unchanged semantics, but now a meaningful number rather than "≤ limit").
- **FR-6** — Strong-signal probe gates expansion AND rerank skip on `top_score >= 0.85 && (top_score - second_score) >= 0.15` (qmd-style tightening).
- **FR-7** — Rerank score blending uses position-aware weights: rank 0–9 → 0.75 hybrid + 0.25 rerank, rank 10–19 → 0.60/0.40, rank 20+ → 0.40/0.60. Rerank logits pass through sigmoid before blending.
- **FR-8** — Fuzzy title BM25 rank is multiplied by `trigram_overlap_ratio²` (Jaccard-like).
- **FR-9** — Exact-alias lookup uses NFD + lowercase at the Rust layer (bypassing FTS5 trigram tokenizer for tokens < 3 chars).
- **FR-10** — Original query gets 2× weight in cross-variant RRF fusion.
- **FR-11** — RRF results get a flat additive bonus: +0.05 for rank 1, +0.02 for rank 2 and 3 (post-fusion tiebreaker).
- **FR-12** — `notes_fts_bm25` is rebuilt with `tokenchars '+#'` so `C++` and `C#` are single tokens.
- **FR-13** — `to_fts_query` rewrites hyphenated tokens (`gpt-4`, `multi-agent`) into quoted phrases (`"gpt 4"`, `"multi agent"`) for the porter tokenizer.
- **FR-14** — All query/alias/tag/scope normalization paths use NFD via a single `normalize_text_nfd` helper.
- **FR-15** — A `-term` prefix in the query becomes an FTS5 `NOT term` clause, provided at least one positive term remains.
- **FR-16** — Query embedding is L2-normalized in `search_vector` before being bound to `vec_f32(?)`.
- **FR-17** — `embed/persist.rs:58` comment is corrected to reflect that L2 normalization is for score-stability, not correctness.
- **FR-18** — A new `db_version` row in `metadata` is bumped on every index-mutating commit; an in-memory `LruCache` (cap 100) for `(query, mode, options) → SearchResponse` consults `db_version` on every hit.
- **FR-19** — The rerank cache keys on `(chunk_text_hash, query_text)`, not `(path, query)`.
- **FR-20** — A `SearchHooks` struct of optional callbacks (`on_expand_start/end`, `on_embed_batch`, `on_rerank_start/end`) is wired into `run_hybrid_pipeline`. Default impl is no-op.
- **FR-21** — When combining FTS5 `MATCH` with `JOIN notes WHERE …`, the BM25 query uses a `WITH fts_matches AS (…)` CTE barrier (only applied if benchmark in US-011 shows a regression).
- **FR-22** — Reranker request batch size capped at 4; per-request max sequence length capped at 128 tokens.
- **FR-23** — `resolve_snippet_heading` falls back to a content-walk when `chunks.heading_path` is NULL (covers pre-reindex notes).

## 5. Tech Stack & Files

```
crates/talon-core/src/search/
├── bm25.rs                  # FR-1, FR-12 (tokenizer constant flow), FR-13, FR-15, FR-21
├── constants.rs             # FR-2, FR-22 (constants only)
├── fuzzy_title.rs           # FR-1, FR-8, FR-9
├── hybrid_pipeline.rs       # FR-6, FR-10, FR-20 (hooks)
├── hybrid_single.rs         # FR-1 (pool sizing pass-through)
├── pool.rs                  # NEW — FR-1 helper
├── rerank_pipeline.rs       # FR-7, FR-19, FR-22
├── rrf.rs                   # FR-10, FR-11
├── strong_signal.rs (or part of fuse.rs) # FR-6
├── text_fts.rs              # FR-13, FR-15
├── vector.rs                # FR-1, FR-16
└── where_filter.rs          # unchanged

crates/talon-core/src/embed/
├── persist.rs               # FR-17 (comment only)

crates/talon-core/src/text/
├── nfd.rs                   # NEW — FR-14
└── frontmatter.rs           # FR-14 (re-export / update normalize_keyword)

crates/talon-core/src/
├── vec_ext.rs               # FR-12 (CREATE TABLE tokenchars)
├── indexing/migrations.rs   # FR-12 (recreate notes_fts_bm25 with new tokenchars)
└── cache/
    ├── mod.rs               # NEW — FR-18 (LRU + db_version)
    └── rerank.rs            # NEW — FR-19

crates/talon-cli/src/
├── cli.rs                   # FR-3 (--candidate-limit)
└── mcp/tool/schema.rs       # FR-3 (mirror in MCP schema)
```

---

## 6. User Stories

User stories are grouped by tier — execute in this order. Within a tier, stories are independent unless noted.

---

### Tier 0 — Attribution scaffolding

#### US-000: Add `LICENSE-3RD-PARTY.md` and OHS attribution policy

**Description:** As the project maintainer, I want a single attribution file that satisfies OHS's MIT license and qmd's license terms before any code lands that ports their math.

**Files:**
- Create: `LICENSE-3RD-PARTY.md` (repo root)
- Modify: `CLAUDE.md` (one-paragraph note about the porting policy)

**Acceptance Criteria:**
- [ ] `LICENSE-3RD-PARTY.md` includes the full text of OHS's MIT LICENSE under a heading `## obsidian-hybrid-search` with the upstream URL `https://github.com/flowing-abyss/obsidian-hybrid-search`.
- [ ] If qmd's license terms differ from MIT, include them under a heading `## qmd` (verify by reading `/home/yolo/.opensrc/repos/github.com/tobi/qmd/main/LICENSE`).
- [ ] `CLAUDE.md` gains a paragraph: "Math/algorithm ports from third-party repos must cite the source file:line in an inline comment. Aggregate attribution in `LICENSE-3RD-PARTY.md`. Do not reinvent ported scoring math; copy verbatim."
- [ ] `just check` passes.

**Commit:** `chore: add 3rd-party attribution and porting policy`

---

### Tier 1 — Limit fix and rerank quality (high leverage)

#### US-001: Introduce `pool.rs` with retriever-specific over-fetch

**Description:** As a search engineer, I want a single helper that decides how wide each retriever should search, so that retrievers stop being capped at `--limit` and post-filters work correctly.

**Files:**
- Create: `crates/talon-core/src/search/pool.rs`
- Modify: `crates/talon-core/src/search/mod.rs` (`pub mod pool;`)

**Acceptance Criteria:**
- [ ] `pool::bm25_pool(limit, candidate_floor)` returns `max(limit * 2, max(candidate_floor, 50))`.
- [ ] `pool::vector_pool(limit, candidate_floor)` returns `max(limit * 5, max(candidate_floor * 2, 100))` (vector dedups by note, so wider pool needed).
- [ ] `pool::fuzzy_pool(limit, candidate_floor)` returns `max(limit * 2, max(candidate_floor, 50))`.
- [ ] `pool::rrf_pool(limit, candidate_floor)` returns `max(limit, candidate_floor)` (final pre-rerank pool size).
- [ ] All four functions accept `u32` and return `u32`. No panics, no `unwrap()`.
- [ ] Unit tests cover: tiny limit (1), default limit (10), giant limit (1000), with `candidate_floor` = 40 (default) and 100 (override).
- [ ] `cargo test -p talon-core search::pool` passes.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:1377` (`max(limit, 20)`), qmd `store.ts:3035-3038` (`max(50, limit*2)`, `limit*10` with collection filter), OHS `vector.rs:53` (`limit*5`).

**Commit:** `feat(search): add pool sizing helpers`

---

#### US-002: Add `CANDIDATE_FLOOR` and thread it through `SearchInput`

**Description:** As a search engineer, I want a `candidate_limit` field on `SearchInput` defaulting to `CANDIDATE_FLOOR=40`, so that the rerank/RRF pool is decoupled from the user's `--limit`.

**Files:**
- Modify: `crates/talon-core/src/search/constants.rs` (add `pub const CANDIDATE_FLOOR: u32 = 40;`)
- Modify: `crates/talon-core/src/search/input.rs` (add `pub candidate_limit: PositiveCount,` defaulted from `CANDIDATE_FLOOR`)
- Modify: `crates/talon-core/src/search/mod.rs` (re-export `CANDIDATE_FLOOR`)

**Acceptance Criteria:**
- [ ] `SearchInput::default()` sets `candidate_limit = PositiveCount::from_const(CANDIDATE_FLOOR)`.
- [ ] `SearchInput::from_*` builders accept an optional `candidate_limit: Option<u16>` parameter (final positional or builder method, mirror existing `limit` plumbing).
- [ ] All call sites in `crates/talon-core/tests/**` and `crates/talon-cli/src/**` compile after the field add (use field-init shorthand or `..Default::default()`).
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** qmd `--candidate-limit` flag (`store.ts:4125`).

**Commit:** `feat(search): introduce CANDIDATE_FLOOR and SearchInput.candidate_limit`

---

#### US-003: Apply pool sizing in `run_search` and `run_hybrid_pipeline`

**Description:** As a user running `--limit 10 --where status:active`, I want all 200 active matching notes to be considered, not just the top 10 ranked by BM25 before filtering.

**Files:**
- Modify: `crates/talon-core/src/query/search.rs:54-100`
- Modify: `crates/talon-core/src/search/hybrid_pipeline.rs:55-130`
- Modify: `crates/talon-core/src/search/hybrid_single.rs` (accept `pool_size` rather than `limit`)

**Acceptance Criteria:**
- [ ] In `run_search`, retriever calls become:
  - `search_bm25(conn, &query, pool::bm25_pool(limit, candidate_floor), DEFAULT_SNIPPET_LENGTH)` (Hybrid-fast and Fulltext branches).
  - `search_vector(conn, embedding, pool::vector_pool(limit, candidate_floor))` (Semantic branch).
  - `search_fuzzy_title(conn, &query, pool::fuzzy_pool(limit, candidate_floor))` (Title branch).
  - `run_hybrid_pipeline` receives both `limit` and `candidate_floor` via a widened `HybridPipelineOptions`.
- [ ] `HybridPipelineOptions` gains `candidate_limit: u32`.
- [ ] In `run_hybrid_pipeline`, `single_to_raw_list(... pool::rrf_pool(limit, candidate_limit) as usize)` and `fuse_hybrid_result_lists(&list_refs, pool::rrf_pool(limit, candidate_limit) as usize)`.
- [ ] In `run_search`, the order is:
  1. retrieve (wide pool)
  2. `apply_where_filter` (no truncation inside)
  3. `apply_since_filter` (no truncation inside)
  4. `apply_scope_priority`
  5. `total = scored.len() as u32` (post-filter, pre-truncate)
  6. `scored.truncate(limit as usize)` ← final trim, last operation
- [ ] Existing tests in `crates/talon-core/tests/search_integration/**` pass without modification, except where they explicitly assert `total <= limit` (those become `total >= results.len()`).
- [ ] New regression test `tests/search_integration/limit_with_filter.rs` proves the bug fix: 50 notes match the query, 30 also match `--where status:active`, `--limit 10` returns 10 active results (not <10).
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.
- [ ] No regression in `tests/ranking_regression/**` golden files.

**Reference:** OHS `searcher.ts:1180-1191`, qmd `store.ts:4156-4281`.

**Commit:** `fix(search): apply --limit as final output trim, not retrieval cap`

---

#### US-004: Expose `--candidate-limit` flag (CLI + MCP)

**Description:** As a power user, I want to override the rerank pool size for tricky queries where 40 isn't enough.

**Files:**
- Modify: `crates/talon-cli/src/cli.rs:33-209` (add `pub candidate_limit: Option<u16>,` and `bpaf` derive)
- Modify: `crates/talon-cli/src/command/search.rs:24-39`
- Modify: `crates/talon-cli/src/mcp/tool/schema.rs:17` (`"candidate_limit": { "type": "integer", "minimum": 1 }`)
- Modify: `crates/talon-cli/src/mcp/tool/dispatch.rs` (or wherever search-tool args are unpacked)
- Modify: `crates/talon-cli/tests/cli/json_success.rs` (add a `--candidate-limit 80` invocation case)

**Acceptance Criteria:**
- [ ] `talon search "foo" --candidate-limit 80` runs and returns up to 80 candidates into the rerank stage.
- [ ] `talon search --help` shows the flag with description "Rerank pool size (default 40, configurable in talon.toml)".
- [ ] MCP schema accepts `candidate_limit: integer minimum 1`.
- [ ] `--candidate-limit 0` is rejected at parse time with a clear error ("must be positive"). Implemented via `PositiveCount::new`.
- [ ] When omitted, default falls back to `[search].candidate_limit` from `talon.toml` (US-025), then `CANDIDATE_FLOOR = 40` if no config.
- [ ] CLI integration test asserts the response `total` for a known fixture exceeds 10 when `--candidate-limit 80` and `--limit 10` are both set (proves the candidate pool widens beyond `--limit`).
- [ ] `cargo test -p talon-cli` passes.
- [ ] `just check` passes.

**Commit:** `feat(cli): add --candidate-limit flag to override rerank pool size`

---

#### US-005: Sigmoid + position-aware rerank score blending

**Description:** As a user, I want top-ranked retrieval results to retain priority unless the reranker strongly disagrees, and I want rerank logits to be on the same scale as hybrid scores before blending.

**Files:**
- Modify: `crates/talon-core/src/search/rerank_pipeline.rs:34-61` (`rerank_candidates`)
- Modify (or create helper): `crates/talon-core/src/search/rerank_pipeline.rs::blend_rerank_candidates` (currently delegated; add weights)

**Acceptance Criteria:**
- [ ] New private fn `sigmoid(logit: f64) -> f64` returning `1.0 / (1.0 + (-logit).exp())`. Unit-tested.
- [ ] New private fn `position_weights(rank_index: usize) -> (f64 /* hybrid */, f64 /* rerank */)`:
  - `0..=9` → `(0.75, 0.25)`
  - `10..=19` → `(0.60, 0.40)`
  - `20..` → `(0.40, 0.60)`
  - Unit-tested on boundary values 0, 9, 10, 19, 20.
- [ ] `blend_rerank_candidates` (or in-place in `rerank_candidates`) applies: `final_score = w_h * hybrid_score + w_r * sigmoid(rerank_logit)` where `(w_h, w_r) = position_weights(pre_rerank_rank)`.
- [ ] When `rerank_score` is `None` (rerank failed for that candidate), `final_score = hybrid_score` unchanged.
- [ ] `RawSearchResult.scores.rerank` stores the sigmoid'd value (0..1), not the raw logit (document this inline).
- [ ] After blending, results are re-sorted by `final_score` desc.
- [ ] Update `tests/ranking_regression/golden.rs` with regenerated goldens; diff inspected and committed as a single update commit.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:1299-1325` (blending), `searcher.ts:1319` (sigmoid).

**Commit:** `feat(rerank): position-aware score blending with sigmoid`

---

#### US-006: Tighten strong-signal probe with score-gap check

**Description:** As a search engineer, I want the "skip expensive ops" probe to also require a clear gap between the top BM25 hit and the second hit, not just a high top score, to avoid skipping rerank when many docs are similarly relevant.

**Files:**
- Modify: `crates/talon-core/src/search/fuse.rs::estimate_strong_signal` (or wherever `estimate_strong_signal` lives; locate via `rg estimate_strong_signal`)

**Acceptance Criteria:**
- [ ] `estimate_strong_signal(probe: &[RawSearchResult]) -> bool` returns `true` iff:
  - `probe.len() >= 1` AND `probe[0].score >= 0.85` AND
  - (`probe.len() < 2` OR `probe[0].score - probe[1].score >= 0.15`).
- [ ] Threshold constants `STRONG_SIGNAL_TOP: f64 = 0.85` and `STRONG_SIGNAL_GAP: f64 = 0.15` live in `search/constants.rs` with comments citing qmd `store.ts:309-315`.
- [ ] Unit tests cover: empty probe, single high-score result, two close high scores (no skip), high score + low second (skip), borderline values.
- [ ] If existing tests assert old behavior, update goldens with explanation.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** qmd `store.ts:309-315, 4024-4036`.

**Commit:** `fix(search): require score gap for strong-signal probe`

---

#### US-007: Fix RRF per-list weights + original-query 2× weight + top-rank bonus

**Description:** As a user, I want the RRF fusion math to match OHS's benchmark-validated weights. **talon currently diverges from OHS on three of four per-list RRF weights** (`crates/talon-core/src/search/constants.rs:49-54`):

| List | talon (current) | OHS (`searcher.ts:1390-1392`) | Action |
|---|---|---|---|
| bm25 | 2.0 | **1.5** | fix |
| exact_alias | 2.0 | 2.0 | keep |
| fuzzy (partial) | 0.5 | **0.25** | fix |
| semantic | 1.0 | **1.5** | fix |

Plus add: original query gets 2× weight in cross-variant fusion (qmd), and top-rank bonus (qmd).

**Files:**
- Modify: `crates/talon-core/src/search/constants.rs:49-54` (`RRF_WEIGHTS`)
- Modify: `crates/talon-core/src/search/rrf.rs` (`normalize_and_merge_rrf_results`, `RrfScoreAccumulator`)
- Modify: `crates/talon-core/src/search/hybrid_pipeline.rs:108-122` (cross-variant fusion call with per-query weights)

**Acceptance Criteria:**
- [ ] **Per-list weights aligned with OHS.** `RRF_WEIGHTS` becomes `{ bm25: 1.5, exact_alias: 2.0, fuzzy: 0.25, semantic: 1.5 }`. Inline source comment: `// Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:1390-1392`.
- [ ] **Cross-variant 2× weight (qmd).** `fuse_hybrid_result_lists` accepts a per-query weight slice. The original query's results get weight 2.0; expansion variants get 1.0. `hybrid_pipeline.rs` constructs `[2.0, 1.0, 1.0, …]` matching `queries_to_search` order. Source comment: `// Algorithm ported verbatim from qmd — store.ts:4122`.
- [ ] **Top-rank bonus (qmd).** After RRF score accumulation, additive bonus: rank 0 → +0.05, rank 1 → +0.02, rank 2 → +0.02. Constant `RRF_TOP_RANK_BONUS: [f64; 3] = [0.05, 0.02, 0.02]` in `search/constants.rs`. Source comment: `// Algorithm ported verbatim from qmd — store.ts:3377-3384`.
- [ ] **Bonus interaction.** Bonus is added *after* `min(1.0, rrf_score / max_possible_score)` normalization, so a rank-0 result can score up to 1.05 (OK — final ranking-sort uses these scores; downstream consumers don't depend on a strict [0,1] cap, but this is documented).
- [ ] Unit tests:
  - `RRF_WEIGHTS` literal values match the new constants.
  - Fused list of 5 results with identical base RRF scores: original-query winner ranks first because of 2× variant weight.
  - Top-rank bonus tiebreaker: two results with identical base score, ranks 0 and 5 → rank-0 wins by 0.05.
- [ ] **Goldens regeneration required.** `tests/ranking_regression/**` will shift because semantic and fuzzy contribute differently. Regenerate goldens, manually review the diff (no all-zeros, no all-equal scores, ordering shifts make sense). Document the MRR@10 delta in commit body — expected to improve since the weights are OHS's benchmark-tuned values.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference + Attribution:**
- OHS `searcher.ts:715-760` (RRF function), `searcher.ts:1390-1392` (per-list weights `[1.5, 1.5, 2.0, 0.25]`).
- qmd `store.ts:4122` (2× original-query weight), `store.ts:3377-3384, 4446-4451` (top-rank bonus).

**Commit:** `fix(rrf): align per-list weights with OHS, add 2x original-query and top-rank bonuses`

---

### Tier 2 — Indexing and tokenization (recall wins)

#### US-008: FTS5 `tokenchars '+#'` + reindex of `notes_fts_bm25`

**Description:** As a user searching `C++`, `C#`, or `F#`, I want results that match those exact tokens, not BM25-scattered partial matches.

**Files:**
- Modify: `crates/talon-core/src/indexing/migrations.rs` (locate `notes_fts_bm25` CREATE; change tokenizer to `tokenize='unicode61 tokenchars ''+#'''`)

**Acceptance Criteria:**
- [ ] `notes_fts_bm25` CREATE statement uses `tokenize='unicode61 tokenchars ''+#'''`.
- [ ] No migration code added. The local DB is deleted and re-indexed by the user (per NG-1: pre-release, single in-container consumer).
- [ ] Test setup helpers that create fresh DBs for tests inherit the new schema automatically.
- [ ] Integration test: insert a note titled `C++ Programming Notes`, search `"C++"`, get the note as result 1. Same for `C#`.
- [ ] `cargo test -p talon-core` passes (after wiping any pre-existing test DB cache).
- [ ] `just check` passes.

**Reference:** OHS `db.ts:374-382`.

**Commit:** `feat(fts): include + and # as tokenchars`

---

#### US-009: Hyphenated-token phrase rewriting in `to_fts_query`

**Description:** As a user searching `gpt-4` or `multi-agent`, I want the porter tokenizer to find documents containing those terms even though it splits on hyphens.

**Files:**
- Modify: `crates/talon-core/src/search/text_fts.rs::to_fts_query`

**Acceptance Criteria:**
- [ ] `to_fts_query` detects bare tokens matching `\b[a-zA-Z][a-zA-Z0-9]*(-[a-zA-Z0-9]+)+\b` and rewrites them as quoted phrases: `gpt-4` → `"gpt 4"`, `multi-agent` → `"multi agent"`, `DEC-0054` → `"DEC 0054"`.
- [ ] Quoted phrases inside the user's original query (already wrapped in `"…"`) are preserved as-is and not re-rewritten.
- [ ] Rewriting happens before `FtsOperator::Or` joining.
- [ ] Unit tests cover: bare hyphenated, multi-segment hyphen (`a-b-c-d`), already-quoted, mixed (`foo gpt-4 "bar baz"`), edge case (`-prefix` and `suffix-`).
- [ ] No regression in existing `text_fts` tests.
- [ ] `cargo test -p talon-core search::text_fts` passes.
- [ ] `just check` passes.

**Reference:** qmd `store.ts:2959-2971`.

**Commit:** `feat(fts): rewrite hyphenated tokens as porter-friendly phrases`

---

#### US-010: Centralize NFD normalization in `text/nfd.rs`

**Description:** As a user on macOS with NFC-encoded filenames matching against NFD-encoded vault content (or vice versa on Linux), I want my queries, aliases, tags, and scope filters to all match.

**Files:**
- Create: `crates/talon-core/src/text/nfd.rs`
- Modify: `crates/talon-core/src/text/frontmatter.rs::normalize_keyword` (delegate to `nfd::normalize`)
- Modify: `crates/talon-core/src/lib.rs` (re-export `normalize_text_nfd`)

**Acceptance Criteria:**
- [ ] `nfd::normalize(input: &str) -> String` returns the NFD-normalized form using the `unicode-normalization` crate (already a transitive dep of `tantivy` or comparable; if not, add to `Cargo.toml`).
- [ ] `normalize_keyword` calls `nfd::normalize(input).to_lowercase()` (preserves existing trim/lowercase semantics).
- [ ] All call sites that previously called `to_lowercase()` directly on user input or vault data are routed through `nfd::normalize` — locate via `rg "to_lowercase\(\)" --type rust` and audit.
- [ ] Unit tests: NFC `"é"` (`é`) and NFD `"é"` both normalize to the same string and compare equal. Cyrillic example (`"Кириллица"`) round-trips.
- [ ] `cargo test -p talon-core text::nfd` passes.
- [ ] No regression in alias/tag exact-match tests.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:115`, `db.ts:13`.

**Commit:** `feat(text): centralize NFD normalization for unicode parity`

---

#### US-011: Benchmark FTS+JOIN latency, add CTE barrier if needed

**Description:** As a search engineer, I want to confirm whether SQLite's planner falls back to a table scan when combining `notes_fts_bm25 MATCH ?` with `JOIN notes WHERE …`, and if so, force the FTS path with a CTE barrier.

**Files:**
- Modify: `crates/talon-core/src/search/bm25.rs:43-55` (only if benchmark shows regression)
- Add: `crates/talon-core/benches/bm25_with_filter.rs` (criterion-based)

**Acceptance Criteria:**
- [ ] Bench measures `search_bm25` over a fixture vault of ≥ 1000 notes with and without a `--where` clause active. Captures p50 and p99 latencies.
- [ ] If the filtered case is > 5× slower than the unfiltered case at the same result count, the BM25 SQL is rewritten:
  ```sql
  WITH fts_matches AS (
      SELECT rowid, snippet(...), bm25(...)
      FROM notes_fts_bm25
      WHERE notes_fts_bm25 MATCH ?
      ORDER BY bm25 LIMIT ?
  )
  SELECT n.vault_path, n.title, ... FROM fts_matches
  JOIN notes n ON n.id = fts_matches.rowid
  WHERE n.active = 1
  ```
- [ ] If the bench shows < 5× regression, this US closes with a doc note: "no CTE needed at current vault scale".
- [ ] Bench results captured in commit message body.
- [ ] `just check` passes.

**Reference:** qmd `store.ts:3028-3046`.

**Commit:** `perf(bm25): add CTE barrier for FTS+JOIN planner` *(or `docs: bench shows no CTE needed`)*

---

#### US-012: Trigram-overlap² fuzzy title scoring

**Description:** As a user, I want fuzzy-title results to be penalized when the query and title share few trigrams, so single-word matches don't dominate.

**Files:**
- Modify: `crates/talon-core/src/search/fuzzy_title.rs`

**Acceptance Criteria:**
- [ ] New helper `trigrams(s: &str) -> HashSet<[char; 3]>` (or `String`-trigram). Unit-tested for short strings (`""`, `"a"`, `"ab"`, `"abc"`, `"abcd"`).
- [ ] New helper `trigram_overlap_ratio(query: &str, title: &str) -> f64` returns `|query_trigrams ∩ title_trigrams| / max(|query_trigrams|, 1)`. Range [0,1].
- [ ] In `search_fuzzy_title` (and `search_title_parts`), the BM25 rank `score` is multiplied by `overlap_ratio.powi(2)` after retrieval.
- [ ] Unit test: title `"Atomic Notes"` vs. query `"atom"` produces a higher score than title `"Notes on Atomic Habits"` vs. query `"atom"` (because the former has higher trigram overlap relative to query length).
- [ ] No regression in `tests/fixture_vault/anchors.rs` exact-alias tests.
- [ ] `cargo test -p talon-core search::fuzzy_title` passes.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:318-329, 398-409`.

**Commit:** `feat(fuzzy): multiply title score by trigram-overlap²`

---

#### US-013: Exact-alias bypass for short tokens — **CLOSED, already implemented**

**Status:** Already implemented in `crates/talon-core/src/search/bm25.rs:100` (`search_by_alias_exact`) and called unconditionally from `crates/talon-core/src/search/fuzzy_title.rs:57` via `search_title_parts`. The lookup goes directly to `note_aliases.alias_norm` (NFD-normalized + lowercased via `normalize_keyword`) and bypasses FTS, so any token length works — not just queries ≥ 3 chars.

**Verification AC** (run as part of US-026):
- [ ] Audit confirms `search_by_alias_exact` handles `query.len() < 3` correctly. No FTS dependency, no minimum-length filter.
- [ ] Add an integration test if missing: vault with aliases `"A"`, `"Go"`, `"C#"` returns those notes for the corresponding short queries.

**Commit:** none — closed without code change. If integration test is added, fold into US-026's commit.

---

### Tier 3 — Caching (latency wins)

#### US-014: Add `db_version` row + bump on every index mutation

**Description:** As a developer running multiple talon processes (CLI + MCP) against the same SQLite file, I want any process's mutation to invalidate every other process's caches without polling.

**Files:**
- Modify: `crates/talon-core/src/indexing/migrations.rs` (add `db_meta` table if it doesn't exist; ensure `db_version` row defaults to `0`)
- Modify: `crates/talon-core/src/indexing/upsert/**` (every commit point bumps `db_version`)
- Modify: `crates/talon-core/src/indexing/change_tracking.rs` (delete path bumps too)

**Acceptance Criteria:**
- [ ] `db_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)` table exists with row `('db_version', '0')` after migration.
- [ ] Helper `bump_db_version(conn: &Connection) -> Result<u64, TalonError>` increments and returns the new value atomically inside the calling transaction.
- [ ] Helper `read_db_version(conn: &Connection) -> u64` reads (with default `0` on missing row).
- [ ] Every `upsert_note`, `upsert_chunks`, `delete_note`, and bulk-reindex code path calls `bump_db_version` exactly once per logical mutation (not per row).
- [ ] Unit test: two writes produce monotonically increasing `db_version` values.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:929-983`.

**Commit:** `feat(index): track db_version for cross-process cache invalidation`

---

#### US-015: LRU search-response cache keyed on `(query, mode, options, db_version)`

**Description:** As a CLI user re-running the same query (e.g. via shell-up-arrow) against an unchanged index, I want the second invocation to skip embedding + retrieval entirely.

**Files:**
- Create: `crates/talon-core/src/cache/mod.rs`
- Create: `crates/talon-core/src/cache/search.rs`
- Modify: `crates/talon-core/src/query/search.rs::run_search`
- Modify: `crates/talon-core/Cargo.toml` (add `lru` crate)

**Acceptance Criteria:**
- [ ] `SearchCache::new(capacity: usize)` returns a cache wrapping `lru::LruCache<CacheKey, CacheEntry>`.
- [ ] `CacheKey` is a hash of `(normalized_query, mode, fast_flag, sorted_where_clauses, since_iso, limit, candidate_limit)`.
- [ ] `CacheEntry` carries the cached `SearchResponse` and the `db_version` it was computed against.
- [ ] On lookup: if `db_version` (from `read_db_version`) matches, return cached response; else evict and miss.
- [ ] Default capacity is 100; configurable via `TALON_SEARCH_CACHE_SIZE` env var (parsed at process start).
- [ ] `run_search` consults the cache at function entry, populates on exit.
- [ ] Cache lookup is short-circuited (skipped) when `inference` is `None` — empty-input returns aren't cached.
- [ ] Unit test: two identical calls produce one inference invocation; a third call after `bump_db_version` produces a second.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:929-983`.

**Commit:** `feat(cache): add LRU search-response cache invalidated by db_version`

---

#### US-016: Rerank cache keyed on `(chunk_text_hash, query_text)`

**Description:** As a search engineer, I want repeated rerank calls for the same `(query, snippet)` pair to skip the cross-encoder, which is expensive.

**Files:**
- Create: `crates/talon-core/src/cache/rerank.rs`
- Modify: `crates/talon-core/src/search/rerank_pipeline.rs::rerank_candidates`

**Acceptance Criteria:**
- [ ] `RerankCache::new(capacity: usize)` (default 1000) wraps `LruCache<(u64 /* xxhash of chunk_text */, u64 /* xxhash of query */), f64 /* sigmoid'd score */>`.
- [ ] Before invoking `inference.rerank`, check the cache per candidate; populate `scores` directly for cache hits, batch-rerank only the misses.
- [ ] After `inference.rerank` succeeds, populate the cache for the misses.
- [ ] Cache is invalidated when `db_version` changes (same mechanism as search cache; share the invalidation hook).
- [ ] Unit test: two consecutive `rerank_candidates` calls with same query + same chunk produce one `InferenceClient::rerank` invocation (mock the client).
- [ ] `cargo test -p talon-core search::rerank_pipeline` passes.
- [ ] `just check` passes.

**Reference:** qmd `store.ts:3304-3318`.

**Commit:** `feat(cache): per-snippet rerank cache to amortize cross-encoder cost`

---

### Tier 4 — Query syntax & ergonomics

#### US-017: `-term` negation in FTS query construction

**Description:** As a user, I want to write `rust -async` and exclude documents containing "async" from the result.

**Files:**
- Modify: `crates/talon-core/src/search/text_fts.rs::to_fts_query`

**Acceptance Criteria:**
- [ ] `to_fts_query` recognizes tokens prefixed with `-` (and not inside a quoted phrase) and emits FTS5 `NOT` syntax: `rust -async` → `rust NOT async`.
- [ ] If all tokens are negative (e.g. `-async`), the function returns `""` (the calling code already treats empty queries as "no FTS match"), preventing FTS5 errors.
- [ ] Multiple negatives are AND-NOT'd: `rust -async -tokio` → `rust NOT async NOT tokio`.
- [ ] Unit tests cover: single positive + single negative, multi-negative, all-negative, negation inside quotes (`"hello -world"` is a literal phrase, no NOT applied).
- [ ] `cargo test -p talon-core search::text_fts` passes.
- [ ] `just check` passes.

**Reference:** qmd `store.ts:2904-2918, 2987-2996`.

**Commit:** `feat(fts): support -term negation in user queries`

---

#### US-018: Search progress hooks

**Description:** As a CLI/MCP author, I want callbacks at key search-pipeline stages so I can show progress, instrument latency, and write tests that assert short-circuits fired.

**Files:**
- Create: `crates/talon-core/src/search/hooks.rs`
- Modify: `crates/talon-core/src/search/hybrid_pipeline.rs`
- Modify: `crates/talon-core/src/search/rerank_pipeline.rs`

**Acceptance Criteria:**
- [ ] `pub struct SearchHooks { pub on_expand_start, on_expand_end, on_embed_batch, on_rerank_start, on_rerank_end: Option<Box<dyn Fn(...) + Send + Sync>> }` with sensible event payloads (e.g. `on_rerank_start(candidate_count: usize)`, `on_rerank_end(elapsed_ms: u64)`).
- [ ] `HybridPipelineOptions` gains `pub hooks: SearchHooks` (default: all `None`).
- [ ] All callbacks fire at the correct stage, with timing measured via `Instant::now()`.
- [ ] CLI doesn't wire hooks yet (just plumbed); MCP doesn't either. Wiring is a future task.
- [ ] Unit test: a test hook records call order and timestamps for a happy-path hybrid run; asserts `on_rerank_start` fires after `on_expand_end`.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** qmd `store.ts:4015, 4039-4045, 4215-4217, 4407-4410`.

**Commit:** `feat(search): add SearchHooks for stage instrumentation`

---

#### US-019: Intent pipeline — full port from qmd

**Description:** As a user, I want to pass `--intent "<text>"` alongside my query to disambiguate ambiguous searches (e.g. `qmd search "performance" --intent "web page load times"` steers results toward page speed, not athletics). Intent affects four pipeline stages: query expansion, strong-signal gate, chunk selection, and rerank prompt.

**Transport contract:** **NO sidecar / LLM API changes.** Expansion is `POST /chat/completions` against the user's configured LLM (`crates/talon-core/src/expansion/client.rs:129`); rerank is the TEI-compatible `/rerank` endpoint. Both take a single user query string. Talon prepends intent into that string client-side. Neither expansion nor rerank wire formats change.

**Files:**
- Modify: `crates/talon-core/src/search/input.rs` (add `pub intent: Option<String>`)
- Modify: `crates/talon-core/src/search/hybrid_pipeline.rs` (`HybridPipelineOptions` gains `intent`; the call to `exp.expand(query, ...)` at line ~87 swaps to `exp.expand(&intent::prefix_query(intent, query), ...)`; gate conditional on intent)
- Modify: `crates/talon-core/src/search/rerank_pipeline.rs` (rerank-query construction + per-doc chunk selection)
- Modify: `crates/talon-cli/src/cli.rs` (add `--intent` flag)
- Modify: `crates/talon-cli/src/mcp/tool/schema.rs` (`"intent": { "type": "string" }`)
- Create: `crates/talon-core/src/search/intent.rs` (term extraction + query-prefix builder)

`ExpansionClient` and `InferenceClient` signatures are unchanged. The only producers of "intent-aware" query strings live in `hybrid_pipeline.rs` and `rerank_pipeline.rs`.

**Acceptance Criteria:**
- [ ] `SearchInput::intent: Option<String>` is plumbed end-to-end. Default `None`. Trimmed on input; empty string normalized to `None`.
- [ ] CLI flag: `--intent <STRING>` (no shorthand). Help text: "Disambiguating context for the query (steers expansion, rerank, and chunk selection)."
- [ ] MCP schema: `"intent": { "type": "string" }` optional.
- [ ] `intent::extract_terms(intent: &str) -> Vec<String>` ported verbatim from qmd `store.ts:3820-3845`:
  - NFD-normalize + lowercase, split on whitespace + punctuation.
  - Stop-word filter: 2–4 char function words removed (use qmd's exact list at `store.ts:3820-3835`).
  - Short domain terms preserved: tokens ≥ 2 chars after stripping, even if 2 chars (so `API`, `SQL`, `CPU`, `LLM`, `Go` survive).
  - Inline `// Algorithm ported verbatim from qmd — store.ts:3820-3845` comment.
- [ ] `intent::prefix_query(intent: Option<&str>, query: &str) -> String` returns `format!("{trimmed}\n\n{query}")` when intent is `Some` and non-empty after trim, else returns `query.to_owned()`. Unit tests cover empty/whitespace/`Some("real")`.
- [ ] **Effect 1 — Expansion.** At `hybrid_pipeline.rs` line ~87, change the call from `exp.expand(query, EXPANSION_N_VARIANTS)` to `exp.expand(&intent::prefix_query(intent, query), EXPANSION_N_VARIANTS)`. The configured LLM (`/chat/completions`) sees the prefixed user message; system prompt is unchanged. If the expansion module has its own cache, it already hashes the input string, so distinct intents produce distinct cache entries automatically.
- [ ] **Effect 2 — Strong-signal gate.** `estimate_strong_signal` stays pure (signature unchanged). The caller in `hybrid_pipeline.rs` gates: `let strong = intent.is_none() && estimate_strong_signal(&bm25_probe);`. Math comment: `// Algorithm ported verbatim from qmd — store.ts:4025-4034`.
- [ ] **Effect 3 — Chunk selection.** When picking the best chunk per `note_id` for rerank input, score = `1.0 * query_term_hits + 0.5 * intent_term_hits`. Term hits are membership tests against `intent::extract_terms`. Highest-scoring chunk wins; ties broken by lower `chunk_index`. Math comment: `// Algorithm ported verbatim from qmd — store.ts:4140-4151`.
- [ ] **Effect 4 — Rerank query.** The query string passed to `inference.rerank(query, &texts, false)` is `intent::prefix_query(intent, query)`. The reranker sees a single string; sidecar API unchanged.
- [ ] **Cache key.** US-016's `RerankCache` key already hashes the rerank-query string; since we now pass the prefix-built string, intent automatically differentiates cache entries. No separate `intent_text_hash` field needed; verify via test.
- [ ] Unit tests:
  - `extract_terms("the API for SQL queries")` → `["api", "sql", "queries"]`.
  - `prefix_query(Some("foo"), "bar")` → `"foo\n\nbar"`. `prefix_query(None, "bar")` → `"bar"`. `prefix_query(Some("  "), "bar")` → `"bar"`.
  - Strong-signal gate returns false when intent is `Some`, true when intent is `None` and probe is decisive.
  - Chunk selection: chunk A has 2 query hits + 0 intent, chunk B has 1 query hit + 3 intent hits → B wins (1.0 + 1.5 > 2.0 + 0.0).
  - Rerank cache: same `query` + same chunk + different intents produce two distinct cache hits (because the prefix-built rerank query differs).
- [ ] Integration test: `talon search "performance" --intent "web page load"` over a fixture vault containing `notes/sports-perf.md` and `notes/web-perf.md` ranks the latter first.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference + Attribution:**
- qmd `store.ts:3260, 3278` (expansion intent plumbing).
- qmd `store.ts:4025-4034` (strong-signal gate disable).
- qmd `store.ts:4140-4151` (chunk selection 1.0/0.5).
- qmd `store.ts:3299, 3310` (rerank prompt + cache key).
- qmd `store.ts:3820-3845` (term extraction).

**Commit:** `feat(search): port qmd intent pipeline (expansion + gate + chunks + rerank)`

---

### Tier 5 — Small wins

#### US-020: Heading breadcrumb fallback to content scan

**Description:** As a user with notes indexed before the `chunks.heading_path` column existed, I want snippet breadcrumbs to still resolve via a content walk.

**Files:**
- Modify: `crates/talon-core/src/search/anchor.rs::resolve_snippet_heading`

**Acceptance Criteria:**
- [ ] When `chunks.heading_path` is `NULL` for a result, `resolve_snippet_heading` reads the note's content (already accessible via `notes` join) and walks backward from `chunk.char_start` to find the most recent `# `, `## `, `### ` heading line, returning the full breadcrumb (e.g. `H1 > H2 > H3`).
- [ ] Walk depth capped at 4 levels (H1–H4); deeper headings are folded.
- [ ] If `char_start` is also `NULL`, returns `None` (no fallback).
- [ ] Unit test: synthetic note with `# A\n## B\n### C\nbody`, `char_start` pointing into `body`, returns `"A > B > C"`.
- [ ] `cargo test -p talon-core search::anchor` passes.
- [ ] `just check` passes.

**Reference:** OHS `searcher.ts:469-515, 627-642`.

**Commit:** `fix(anchor): fall back to content scan for breadcrumbs without heading_path`

---

#### US-021: Reranker batch=4, max_len=128 caps

**Description:** As a search engineer running on memory-constrained hardware, I want the reranker to batch in chunks of 4 with a 128-token sequence cap, since attention is O(batch × seq²) and our snippets are already short.

**Files:**
- Modify: `crates/talon-core/src/search/constants.rs` (add `RERANK_BATCH_SIZE: usize = 4`)
- Modify: `crates/talon-core/src/inference/client.rs::rerank` (chunk inputs into batches)

**Acceptance Criteria:**
- [ ] `rerank` chunks the input candidates into batches of `RERANK_BATCH_SIZE=4` and serially POSTs them, concatenating results in order.
- [ ] No `max_length` field is sent — sidecar does not support it (confirmed). The sidecar's truncation default applies; this is acceptable because chunk text is already ≤ ~300 chars / ~80 tokens (well under typical 512-token defaults).
- [ ] A note added in `inference/client.rs::rerank` doc-comment: "Sequence-length truncation is enforced server-side using the sidecar's default max_length. talon's chunker keeps text small enough that this is non-binding."
- [ ] Unit test (mocking the HTTP client): 10 candidates produce 3 POSTs (4+4+2), aggregated correctly. Order preserved.
- [ ] No latency regression in `tests/ranking_regression/**` (these tests use real fixtures; if they get slower, document expected wall-time and re-baseline).
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:** OHS `reranker.ts:44-47, 128-131`.

**Commit:** `perf(rerank): cap batch size to 4 and max_len to 128`

---

### Embedding correctness

#### US-022: Normalize query embedding in `search_vector` (+ comment fix)

**Description:** As a search engineer, I want both stored and query vectors at unit norm so `1 - distance/2` exactly equals cosine similarity, not just monotonically tracks it.

**Files:**
- Modify: `crates/talon-core/src/search/vector.rs:40-103` (`search_vector`)
- Modify: `crates/talon-core/src/embed/persist.rs:55-67` (comment correction only)

**Acceptance Criteria:**
- [ ] In `search_vector`, before serializing `embedding` to JSON, compute `norm = sqrt(sum(x²))` and `normalized = embedding.iter().map(|x| x / norm).collect()` when `norm > 0.0`. When `norm == 0.0`, return empty results immediately (degenerate input).
- [ ] Reuse a private helper `normalize_unit(v: &[f32]) -> Vec<f32>` (move from `persist.rs` or re-implement inline; if reused, place in `crates/talon-core/src/embed/mod.rs` as `pub(crate) fn normalize_unit`).
- [ ] `embed/persist.rs:58` comment changed from "sqlite-vec uses distance_metric=cosine which assumes ‖v‖ = 1" to "Normalizing to unit length lets us use the L2-distance-from-cosine identity (`distance² = 2·(1−cos_sim)`) and keeps `COSINE_DISTANCE_MAX = 2.0` exact. sqlite-vec's cosine itself is scale-invariant, so this is for score-stability, not correctness."
- [ ] Unit test: stored embedding `[3, 4, 0]` and query embedding `[6, 8, 0]` produce a `distance_to_score` of `1.0` (perfect match) — proves both sides are normalized.
- [ ] `cargo test -p talon-core search::vector` passes.
- [ ] `just check` passes.

**Reference:** Existing `embed/persist.rs:55-67` for the storage-side pattern.

**Commit:** `fix(vector): normalize query embedding to unit length`

---

#### US-023: Native int8 vector storage

**Description:** As a vault owner, I want embeddings stored as `int8[N]` rather than `float[N]` to cut the `vec_chunks` table size by ~4× with no recall loss, since pplx-embed is natively int8-quantized. talon is pre-release with one local consumer, so the existing DB will be deleted and re-indexed.

**Files:**
- Modify: `crates/talon-core/src/vec_ext.rs` (CREATE TABLE uses `int8[N] distance_metric=cosine`)
- Modify: `crates/talon-core/src/embed/persist.rs` (quantize f32 → i8 before insert, bind with `vec_int8(?)`)
- Modify: `crates/talon-core/src/search/vector.rs` (quantize query embedding f32 → i8 before binding `vec_int8(?)`)
- Create: `crates/talon-core/src/embed/quantize.rs` (`f32_to_i8_normalized`, `i8_dot`)
- Modify: `crates/talon-core/src/inference/client.rs` (note: sidecar continues returning `Vec<Vec<f32>>`; talon quantizes locally)
- Modify: schema rebuild logic to detect old `float[N]` schema and force a full re-index (drop + recreate `vec_chunks`, mark all chunks `embedding_status = 'pending'`)

**Acceptance Criteria:**
- [ ] `vec_chunks` is created as `embedding int8[{dim}] distance_metric=cosine`. The dimension parser in `vec_ext.rs:get_vec_chunks_dimensions` is updated to recognize `int8[…]` in addition to `float[…]`.
- [ ] `quantize::f32_to_i8_normalized(v: &[f32]) -> Vec<i8>`:
  1. Compute `norm = (v.iter().map(|x| x * x).sum::<f32>()).sqrt()`.
  2. If `norm > 0.0`, multiply each component by `127.0 / norm`, then `.round().clamp(-127.0, 127.0) as i8`. (Yields a unit-vector quantization where each int8 component is `round(127 * x_normalized)`.)
  3. If `norm == 0.0`, return `vec![0i8; v.len()]`.
- [ ] `embed/persist.rs` calls `f32_to_i8_normalized` and binds via `vec_int8(?)` (sqlite-vec helper); unit test confirms round-trip via `vec_to_json(embedding)` returns the i8 vector.
- [ ] `search/vector.rs::search_vector` quantizes the query embedding identically before binding `vec_int8(?)`. Both stored and query vectors are unit-norm before quantization, so cosine distance over int8 is well-defined and `COSINE_DISTANCE_MAX = 2.0` still holds.
- [ ] `distance_to_score` is unchanged.
- [ ] **Schema cutover.** No migration code. The user nukes the DB and re-indexes (per NG-1). `vec_ext.rs::ensure_vec_chunks` already rebuilds when stored dimension differs; extend the comparison to include storage type so a `float[N]` table is also rebuilt as `int8[N]`. The rebuild marks active chunks `embedding_status = 'pending'` (existing logic).
- [ ] Unit tests:
  - `f32_to_i8_normalized([3.0, 4.0, 0.0])` → values consistent with normalized `[0.6, 0.8, 0.0] * 127` rounded → `[76, 102, 0]` (within ±1 from rounding).
  - Empty vector produces empty result.
  - Zero vector produces all-zero i8.
- [ ] Integration test: ingest a fixture vault, run a vector search, get the same top-1 result before and after the i8 cutover (use a small recall-stable fixture to allow recall@1 == 1).
- [ ] Bench: vector-search latency before vs. after on the eval fixture vault. Capture in the commit message body. Expect ≤ same latency (sqlite-vec int8 is typically faster).
- [ ] Storage check: `SELECT COUNT(*) FROM vec_chunks` and total `length(embedding)` per row before vs. after — bytes-per-row drops ≈4×.
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference:**
- Perplexity docs: pplx-embed is natively unnormalized int8; "compare via cosine similarity".
- sqlite-vec `int8` cosine implementation handles norm internally; same scale-invariance as f32 cosine.

**Commit:** `feat(vector): native int8 storage for pplx embeddings (4x size reduction)`

---

#### US-024: BM25 anchor lookup (deferred-conditional)

**Description:** As a future UI consumer (Obsidian plugin / VS Code / web client) wanting "click result, scroll editor to exact match", I want BM25 hits to carry `char_start`/`char_end` anchors via OHS's two-pass lookup (snippet-key against chunk rows, content-scan fallback).

**Files (when un-deferred):**
- Modify: `crates/talon-core/src/search/anchor.rs::build_anchors` (add BM25 path)
- Modify: `crates/talon-core/src/search/bm25.rs` (return raw match snippet positions if available)

**Acceptance Criteria:**
- [ ] **Conditional.** If no UI consumer exists at the time this US is reached, this US closes deferred with a link to the OHS algorithm at `searcher.ts:524-642` and a note "un-defer when a UI client wanting deep-link anchors lands."
- [ ] When un-deferred, the implementation ports OHS verbatim:
  - First 60 chars of FTS5 `snippet()` (after stripping `...` markers) becomes a "snippet key".
  - Match the snippet key against `chunks` rows for the result's note: if a chunk's `text` contains the snippet key, the anchor uses that chunk's `char_start`/`char_end`.
  - Fallback: read the note's content, locate the snippet key by substring, walk backward for the heading breadcrumb. Cap walk at the most recent 4 heading levels (from US-020).
- [ ] Anchor dedup by `match_text` (semantic anchor wins ties).
- [ ] `primary_anchor_index` is `0` (semantic) when both anchors exist.
- [ ] Tests cover: chunk-match path, content-scan fallback, dedup when both anchors point at the same block.

**Reference + Attribution:** OHS `searcher.ts:40-46` (anchor shape), `searcher.ts:524-642` (two-pass lookup), `searcher.ts:1219-1235` (gating + dedup).

**Commit (when un-deferred):** `feat(anchor): two-pass BM25 anchor lookup for editor deep-links`

---

#### US-026: Final scoring-constant audit + close-out verifications

**Description:** Most scoring math has already been audited during plan authoring (BM25 weights, BM25 score normalization, RRF_K, snippet token budget, fuzzy implementation, exact-alias bypass — all verified to match OHS or to be intentionally better). This US is the final sweep covering the remaining items that haven't been touched by an earlier US, and it captures the audit results as a single artifact.

**Pre-verified (no work needed in this US):**
| Item | talon | OHS | Status |
|---|---|---|---|
| BM25 column weights | 10/5/1 (`constants.rs:84-88`) | 10/5/1 (`searcher.ts:237`) | ✓ aligned |
| BM25 score normalization | `abs/(1+abs)` (`text_fts.rs:132`) | same (`searcher.ts:260`) | ✓ aligned |
| BM25 snippet token budget | `max(MIN, ceil(chars/4))` (`bm25.rs:41`) | same (`searcher.ts:232`) | ✓ aligned |
| RRF k | 60 (`constants.rs:33`) | 60 (`searcher.ts:721`) | ✓ aligned |
| RRF per-list weights | fixed by US-007 | OHS values | ✓ via US-007 |
| RRF normalization | `sum(active_w)/(k+1)` (`fuse.rs:116`) | same (`searcher.ts:751`) | ✓ aligned |
| Strong-signal thresholds | 0.85/0.15 (US-006) | 0.85/0.15 (qmd) | ✓ via US-006 |
| Position-aware rerank weights | 0.75/0.60/0.40 (US-005) | same (`searcher.ts:1299`) | ✓ via US-005 |
| Top-rank bonus | 0.05/0.02/0.02 (US-007) | qmd values | ✓ via US-007 |
| Fuzzy implementation | `bm25 × overlap²` + typed buckets | same approach (`searcher.ts:368-410`) | ✓ aligned, talon's API cleaner |
| Exact-alias for short tokens | `search_by_alias_exact` (`bm25.rs:100`) | `searcher.ts:336-366` | ✓ aligned (US-013 closed) |
| Token estimation | `tokenx-rs` (Unicode-aware) | simple any-non-ASCII heuristic | ✓ talon better, divergence noted via US-027 |
| Cosine distance ceiling | 2.0 with `1 - dist/2` from sqlite-vec | L2-from-cosine `1 - L2²/2` | ✓ different metric source, equivalent end result |

**This US covers (work + verification):**
- [ ] **`COSINE_DISTANCE_MAX` divergence comment.** Add an inline comment at `crates/talon-core/src/search/vector.rs:19-20` explaining: "OHS uses L2 distance with `1 - L2²/2` because their vector store returns L2; sqlite-vec's `distance_metric=cosine` returns cosine distance directly, so `1 - distance/2` is the equivalent talon-side formula. Both yield similarity in [0,1]." Cite OHS `searcher.ts:691`.
- [ ] **FTS query construction audit.** Compare `crates/talon-core/src/search/text_fts.rs::to_fts_query` against OHS `searcher.ts:209-214` (operator choice, sanitization). talon already uses OR; verify sanitize behavior (which special chars are stripped) matches OHS `sanitizeFtsQuery`. Document any differences.
- [ ] **`buildTrigramOrQuery` audit.** Compare talon's `build_trigram_or_query` against OHS's. Verify both decompose the query into trigrams + OR-join, and both handle short queries (< 3 chars) by returning a sentinel that triggers no FTS hits but doesn't error.
- [ ] **`maxPossibleScore` normalization audit.** Read `crates/talon-core/src/search/fuse.rs:116`. Verify the formula matches OHS `searcher.ts:751-752` (`sum(active_weights)/(k+1)`) and the per-result divisor + `min(1, …)` clamp match `searcher.ts:758`.
- [ ] **One-time integration test for short-token aliases** (folded from closed US-013): vault with aliases `"A"`, `"Go"`, `"C#"` returns those notes for the corresponding short queries.
- [ ] **Audit summary** committed as a table at the top of `LICENSE-3RD-PARTY.md` or as a new `docs/search-math-audit.md` (one-page; check off each row above).
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Commit:** `chore(search): final scoring-constant audit and close-outs`

---

#### US-027: Verify chunker behavior — keep tokenx-rs, sanity-check overlap

**Description:** talon's chunker is **likely already better than OHS's** in two places. This US verifies that and only changes what genuinely needs changing.

- **Token estimation:** talon uses `tokenx-rs` (`text/chunker.rs:14, 23`) — a Rust port of the `tokenx` heuristic with proper CJK / Hangul / Cyrillic / fullwidth handling per Unicode block. OHS uses a much simpler `if any(non_ascii) { 1 char/token } else { 0.25 token/char }` heuristic. tokenx-rs is more accurate. **Keep talon's; do not regress to OHS.**
- **Chunk overlap:** talon delegates to the `text-splitter` crate's `with_overlap(N)` (where `N = config.chunk_overlap`, default 64 tokens). OHS uses a hand-rolled `max(chunk - step, ceil(chunk/2))` formula. The two approaches produce different boundaries; OHS's is not necessarily better.
- **Min-token filter:** talon's `CHUNK_MIN_TOKENS_DEFAULT = 16` vs OHS's `chunkMinLength = 50` (chars, ≈12 tokens). Different units; close enough.

**Files:**
- Read-only verification: `crates/talon-core/src/text/chunker.rs`
- Modify if needed: `crates/talon-core/src/search/constants.rs` (if `CHUNK_MIN_TOKENS_DEFAULT` needs tuning)

**Acceptance Criteria:**
- [ ] **Confirm tokenx-rs handles representative inputs.** Add unit tests:
  - English ASCII: `estimate_token_count("the quick brown fox")` returns ≥ 4 and ≤ 6.
  - CJK: `estimate_token_count("漢字テスト")` returns ≥ 5 (one per char minimum).
  - Cyrillic: `estimate_token_count("привет мир")` returns ≥ 8 and ≤ 12.
  - Mixed: `estimate_token_count("hello мир 漢字")` reflects all three components.
- [ ] **Annotate divergence.** Add a comment near `text/chunker.rs:14` (the tokenx-rs import):
  ```rust
  // Intentional divergence from OHS chunker.ts:23-35: tokenx-rs (port of johannschopplich/tokenx)
  // is more accurate than OHS's any(non_ascii) heuristic — proper Unicode block handling for
  // CJK / Hangul / Cyrillic / fullwidth. See tokenx-rs/src/estimator.rs.
  ```
- [ ] **Sanity-check overlap.** Index the existing fixture vault, capture chunk counts and boundary offsets, and assert determinism across a second indexing pass. No formula change unless an empirical regression is found.
- [ ] **Min-token filter.** `CHUNK_MIN_TOKENS_DEFAULT` stays at 16 unless audit shows a clear quality issue (very short chunks dominating retrieval). If changed, document the new value's basis in commit body.
- [ ] **DB nuke.** Not required for this US (no algorithm changes). Required only if min-token default is bumped.
- [ ] `cargo test -p talon-core text::chunker` passes.
- [ ] `just check` passes.

**Reference:** OHS `chunker.ts:23-35` (token estimator we deliberately don't port), `chunker.ts:110` (overlap formula we don't port), `config.ts:41` (min-length, units differ).

**Commit:** `chore(chunker): document intentional divergence from OHS for tokenx-rs and text-splitter`

---

#### US-028: Embedding API retry with exponential backoff

**Description:** As a user running large-vault indexing against a flaky embedding endpoint, I want transient failures (429, 502, 503, 5xx, network errors) to retry with exponential backoff rather than aborting the indexing pass.

**Files:**
- Modify: `crates/talon-core/src/inference/client.rs::embed` (and `embed_chunked` if it exists)

**Acceptance Criteria:**
- [ ] On HTTP status `429`, `502`, `503`, any `5xx`, or transport error (`reqwest::Error::is_connect()` / `is_timeout()` / `is_request()`), retry up to **2 times** with delays of `2^attempt seconds` (= 2s, then 4s) before propagating error. Algorithm ported from OHS `embedder.ts:384-395`.
- [ ] Non-transient HTTP statuses (`4xx` except 429) propagate immediately without retry.
- [ ] On batch endpoint failure (`/embed-chunked`), fall back to per-item retry on the items in the failed batch (OHS `embedder.ts` behavior).
- [ ] Inline `// Algorithm ported verbatim from obsidian-hybrid-search (MIT) — embedder.ts:384-395` comment.
- [ ] Unit test (mocking the HTTP client): a `503` followed by a `200` retries once and succeeds; three consecutive `503`s give up and return error.
- [ ] Unit test: a `400` (bad request) returns immediately without retry.
- [ ] Integration test note: skip — too flaky to test against a real sidecar.
- [ ] `cargo test -p talon-core inference` passes.
- [ ] `just check` passes.

**Reference + Attribution:** OHS `embedder.ts:384-395`.

**Commit:** `feat(inference): retry embed requests on transient failures with exponential backoff`

---

#### US-029: Verify reranker label extraction is model-agnostic

**Description:** As a user who may swap reranker models (BGE 1-label vs 2-label), I want the rerank logit extraction to work for both. OHS picks the **last label index** (`LABEL_1`) which works for both regression and classification heads.

**Files:**
- Modify: `crates/talon-core/src/inference/client.rs::rerank` (response parsing)
- Modify: `crates/talon-core/src/inference/types.rs` (rerank response type, if needed)

**Acceptance Criteria:**
- [ ] Audit talon's rerank response parsing. If it picks a fixed-index label (e.g. always `scores[0]`), align to OHS: pick the last element of the `scores` array per item. Single-element arrays (1-label models) trivially return `scores[0]`; multi-element arrays (2-label) return `scores[scores.len() - 1]`.
- [ ] If TEI returns a flat `score: f64` field (single value, not array), no change needed — note this in commit body.
- [ ] Unit test (mocking a 2-label response): score returned is the last element.
- [ ] Unit test (mocking a 1-label response): score returned is the only element.
- [ ] Inline `// Algorithm ported from obsidian-hybrid-search (MIT) — reranker.ts:63, 143` comment if changes are made.
- [ ] `cargo test -p talon-core inference` passes.
- [ ] `just check` passes.

**Reference + Attribution:** OHS `reranker.ts:63, 143`.

**Commit:** `chore(rerank): verify model-agnostic label extraction matches OHS` *(or `fix(rerank): use last-label score for 2-head model compatibility`)*

---

#### US-030: Snippet polish — fallback retrieval + final char truncation

**Description:** As a user, I want snippets that are reliably ≤ `DEFAULT_SNIPPET_LENGTH` characters and never empty when content is available. Two tweaks port directly from OHS.

**Files:**
- Modify: `crates/talon-core/src/search/anchor.rs` (or wherever snippet enrichment lives)
- Modify: `crates/talon-core/src/query/search.rs::raw_to_search_result` (final truncation step)

**Acceptance Criteria:**
- [ ] **Final character truncation.** After heading-breadcrumb prepending, truncate `snippet` to `DEFAULT_SNIPPET_LENGTH` chars. Use char-boundary-safe truncation (`snippet.chars().take(N).collect()`). Cite OHS `searcher.ts:1209`.
- [ ] **Fallback fetch.** If the BM25-derived snippet is shorter than 50% of `DEFAULT_SNIPPET_LENGTH` and the result has a known `note_id`, fetch a fallback snippet via a secondary FTS query against the full body and keep the longer of the two. Cite OHS `searcher.ts:1195-1208`.
- [ ] Both behaviors gated on `mode == Hybrid || mode == Fulltext` (vector hits already use `chunk_text` directly).
- [ ] Unit test: a synthesized result with a 30-char BM25 snippet + a 200-char body returns the body-derived snippet.
- [ ] Unit test: char-boundary truncation preserves multibyte UTF-8 (no panics on emoji or accented chars).
- [ ] `cargo test -p talon-core` passes.
- [ ] `just check` passes.

**Reference + Attribution:** OHS `searcher.ts:1195-1209`.

**Commit:** `feat(snippet): fallback retrieval and char-boundary final truncation`

---

#### US-025: Config defaults for search tunables (`talon.toml` `[search]` section)

**Description:** As a vault owner with strong opinions on retrieval, I want to override `candidate_limit`, search cache size, rerank batch size, and other tunables in `talon.toml` so I don't have to pass flags every invocation.

**Files:**
- Modify: `crates/talon-core/src/config/**` (locate config types via `rg "TalonConfig" -l`; add a `[search]` subsection)
- Modify: `crates/talon-cli/src/cli.rs` (CLI flags override config values; config overrides hardcoded defaults)
- Modify: `crates/talon-core/src/search/input.rs::SearchInput::from_*` (accept config and use its defaults for `candidate_limit`, `limit`)
- Modify: example `talon.toml` (or `example-config.toml` if one exists; otherwise add a generated stub via `talon config init` if that command exists, else just document in `README.md` / config docs)

**Acceptance Criteria:**
- [ ] `TalonConfig` gains a `pub search: SearchConfig` field with:
  ```rust
  #[derive(Debug, Clone, Deserialize)]
  pub struct SearchConfig {
      #[serde(default = "default_candidate_limit")]
      pub candidate_limit: u16,           // default CANDIDATE_FLOOR = 40
      #[serde(default = "default_limit")]
      pub limit: u16,                     // default DEFAULT_LIMIT = 10
      #[serde(default = "default_search_cache_size")]
      pub cache_size: usize,              // default 100 (US-015)
      #[serde(default = "default_rerank_cache_size")]
      pub rerank_cache_size: usize,       // default 1000 (US-016)
      #[serde(default = "default_rerank_batch_size")]
      pub rerank_batch_size: usize,       // default RERANK_BATCH_SIZE = 4
      #[serde(default = "default_rerank_max_tokens")]
      pub rerank_max_tokens: u32,         // default RERANK_MAX_TOKENS = 128
  }
  ```
- [ ] Each `default_*` fn returns the constant from `search/constants.rs`. Single source of truth.
- [ ] Precedence: **CLI flag > config file > hardcoded default**.
- [ ] `SearchInput::from_*` builders accept a `&SearchConfig` (or `Option<&SearchConfig>` if config can be absent at process start) and use it to fill defaults. Existing call sites must be updated.
- [ ] `cache_size`, `rerank_cache_size`, `rerank_batch_size`, `rerank_max_tokens` are read at process start and used to construct caches/clients (one-shot read; no live reload).
- [ ] Example `talon.toml` snippet documented:
  ```toml
  [search]
  candidate_limit = 60
  limit = 10
  cache_size = 200
  rerank_cache_size = 2000
  rerank_batch_size = 4
  rerank_max_tokens = 128
  ```
- [ ] Unit test: config file with `candidate_limit = 60` produces `SearchInput.candidate_limit = 60` when no `--candidate-limit` flag is passed; flag `--candidate-limit 80` overrides to 80.
- [ ] `cargo test -p talon-core` passes.
- [ ] `cargo test -p talon-cli` passes.
- [ ] `just check` passes.

**Commit:** `feat(config): expose [search] section for tunable defaults`

---

## 7. Design Considerations

- **No UI surface changes.** All work is in CLI / MCP / library code.
- **Existing patterns:** every retriever already accepts a `u32` limit; the pool helpers replace the user-`limit` call with a pool-helper call. No new abstractions.
- **Constants live in `search/constants.rs`.** Don't sprinkle magic numbers.
- **NFD normalization is a hot path.** If profiling later shows it dominates, cache normalized strings in `notes` table at indexing time. Not in scope here.

## 8. Technical Considerations

- **Linter config:** unchanged. If a refactor surfaces a clippy warning, refactor; flag to user only if suppression is unavoidable (per CLAUDE.md).
- **Conventional commits:** every commit message uses the prefixes shown in each US.
- **`just check` runs after every US.** Failed lint blocks merge.
- **Ranking-regression goldens:** US-005, US-007 will modify them. Update in the same commit as the behavior change; review the diff manually for sanity (no all-zeros, no all-results-tied, ordering shifts make sense).
- **Cargo deps added:** `lru` (US-015), `unicode-normalization` (US-010 — if not already present transitively), `xxhash-rust` or similar (US-016 — confirm what's already in the tree).
- **Bench harness:** US-011 may be the first criterion bench in the repo. If `criterion` isn't a dev-dep, add it under `[dev-dependencies]` and create `benches/` directory.
- **Feature flag for cache (optional):** US-015 / US-016 are pure perf wins. If risk-averse, wire them behind `TALON_DISABLE_SEARCH_CACHE=1` env var. Default on.

## 9. Success Metrics

- **M-1 (correctness):** `--limit 10 --where status:active` returns 10 results when ≥ 10 active matches exist (US-003 regression test).
- **M-2 (recall):** `tests/ranking_regression/golden.rs` MRR@10 improves or stays equal after US-005 + US-007 + US-012. Document the delta in commit messages.
- **M-3 (recall — tokens):** new fixture queries `C++`, `C#`, `gpt-4`, `multi-agent`, `A`, `Go` all return their target notes as top-3 results (US-008, US-009, US-013).
- **M-4 (latency):** repeat query latency (cache hit) drops to < 5ms for non-trivial queries (US-015 bench).
- **M-5 (latency):** rerank wall-time on a 40-candidate pool drops by ≥ 30% with the per-snippet cache warm (US-016 bench).
- **M-6 (no regression):** `just check` and the full `cargo test --workspace` pass at every US boundary.

## 10. Open Questions

- **Q-1:** Does sqlite-vec's `int8[N] distance_metric=cosine` give recall identical to `float[N]` on our eval suite? US-023's bench will answer; expectation is yes within ±1 result on top-10.

**Resolved (closed before implementation):**
- ~~`max_length` on `/rerank`~~: confirmed unsupported. US-021 drops the request-side `max_length` field; the sidecar's truncation default applies and OHS's batch=4 is the only knob we set.
- ~~`unicode-normalization` dep~~: already in `crates/talon-core/Cargo.toml` and `Cargo.lock`. US-010 uses it directly.
- ~~Read paths bumping metadata~~: audited — talon's search path is pure SELECT, embedding only runs via `talon index`. `db_version` (US-014) only ever increments during explicit index runs, so cache invalidation is never spurious.
- ~~`--candidate-limit 0`~~: rejected as invalid (consistent with `PositiveCount`). US-004 enforces.

---

## 11. Execution Order Summary

```
Tier 0 (attribution scaffolding):
  US-000

Tier 1 (limit + rerank quality):
  US-001 → US-002 → US-003 → US-004 → US-005 → US-006 → US-007

Tier 2 (indexing + tokenization):
  US-008 → US-009 → US-010 → US-011 → US-012 → US-013

Tier 3 (caching):
  US-014 → US-015 → US-016

Tier 4 (query syntax + ergonomics):
  US-017 → US-018 → US-019 (intent — full port)

Tier 5 (small wins):
  US-020 → US-021

Embedding correctness + storage:
  US-022 (query normalize) → US-023 (int8 storage)

Audit-and-align (math parity with OHS):
  US-026 (audit constants) → US-027 (chunker math) → US-028 (embed retry)
  → US-029 (rerank label) → US-030 (snippet polish)

Conditional / config:
  US-024 (BM25 anchor lookup — deferred-conditional on UI consumer)
  US-025 (talon.toml [search] section — runs LAST)
```

**Dependencies that cross tiers:**
- US-003 depends on US-001 + US-002.
- US-015 depends on US-014.
- US-016 depends on US-014 + US-019 (for the intent dimension in cache key).
- US-019 depends on US-006 (it disables the gate that US-006 tightened) and US-016 (cache key shape).
- US-023 depends on US-022 (both store and query are unit-norm before quantization).
- US-026 should land after US-005, US-006, US-007 so the new math is in place to audit; it's a verification/parity pass, not a separate implementation.
- US-027 changes chunk shapes — DB nuke required after this US (per NG-1). Schedule before US-022/US-023 if both are landing in the same release window so users only re-index once.
- US-025 should land after US-002, US-015, US-016, US-021 so all the constants it exposes exist; recommend running it last.

---

## 12. Self-Review

- **Spec coverage:** every conversation-inventory item maps to a US, plus a fresh OHS audit added US-026 through US-030 (math parity, chunker formula, embed retry, rerank label, snippet polish).
- **Placeholder scan:** every "TBD" / "later" / "as appropriate" is replaced with a concrete value or acceptance criterion. US-011 and US-024 are conditional but spell out both branches.
- **Type consistency:** `pool::*_pool` signatures match across US-001 and US-003. `CANDIDATE_FLOOR` typed `u32` everywhere; `SearchConfig.candidate_limit` is `u16` (matching `--limit`'s `Option<u16>` shape) and converts at the `SearchInput` boundary. `SearchHooks` uses `Option<Box<dyn Fn(...)>>` consistently. `db_version` is `u64`.
- **Math attribution:** every US that ports OHS or qmd math (US-005, US-006, US-007, US-009, US-012, US-013, US-019, US-020, US-027, US-028, US-029, US-030) carries a `Reference + Attribution` block citing exact `searcher.ts` / `store.ts` / `chunker.ts` / `embedder.ts` / `reranker.ts` line numbers, and the AC requires an inline source comment in the Rust code. US-026 is the explicit verification pass that confirms parity for already-existing math (`build_bm25_score`, `RRF_K`, etc.).

---

## 13. Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-27-search-retrieval-improvements.md`.

Two execution options:

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per US, review between US, fast iteration. Required sub-skill: `superpowers:subagent-driven-development`.

**2. Inline Execution** — execute US-by-US in this session with checkpoints. Required sub-skill: `superpowers:executing-plans`.

Worktree recommended for either option (Tier 1 + Tier 2 alone touch ~15 files and modify goldens). Ask before creating it.
