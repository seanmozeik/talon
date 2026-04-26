# Talon Extraction Part 1 — Standalone Rust Binary

Port the Talon Obsidian vault search engine from TypeScript (`/tmp/talon-scaffold-imports/ultra/edge/src/services/talon/`) to a standalone Rust binary, per the design spec `2026-04-25-talon-extraction-design.md`. Phase 1 only: the Rust project itself. Phase 2 (ultraclaw integration) is separate.

## Reference Code

**Primary reference: `/tmp/talon-scaffold-imports/ultra/edge/src/services/talon/`** — the full TypeScript Talon implementation (~107 files, ~10.6K LOC). Every algorithm has a concrete TS source to translate.

### Key reference files (by module)

**Chunker** (port exactly):
- `shared/chunker.ts` (297 lines) — block-based segmentation, 900-token chunks, 15% overlap, heading-aware, embedding text construction
- `shared/chunker-types.ts` — `TalonMarkdownBlock`, `TalonNoteChunk`, `ChunkContext`
- `shared/text.ts` — line splitting, fence/heading detection, token estimation, wikilink parsing

**Search algorithms** (port exactly):
- `search/bm25.ts` (91 lines) — BM25 FTS with OHS weights (title=10, alias=5, content=1)
- `search/text-fts.ts` (59 lines) — FTS query building, trigrams, BM25 score normalization (`buildBm25Score`)
- `search/rrf.ts` (106 lines) — Reciprocal Rank Fusion (k=60, weights: bm25=2, exactAlias=2, fuzzy=0.5, semantic=1)
- `search/hybrid-pipeline.ts` (68 lines) — probe → expansion → multi-query → fuse → rerank → filter
- `search/hybrid-single.ts` (42 lines) — single-query hybrid retrieval (semantic + bm25 + title parts)
- `search/hybrid-variants.ts` (21 lines) — query expansion variant resolution
- `search/hybrid-expand.ts` (82 lines) — LLM-based query expansion with caching
- `search/rerank-pipeline.ts` (114 lines) — cross-encoder reranking with weighted blending
- `search/fuse.ts` (139 lines) — result fusion, strong-signal detection, rerank blending
- `search/fuzzy-title.ts` (82 lines) — trigram-based fuzzy title/alias matching
- `search/vector.ts` (157 lines) — sqlite-vec cosine distance search
- `search/llm-cache.ts` (214 lines) — LRU expansion cache + rerank cache
- `search/constants.ts` (31 lines) — all magic numbers

**Indexer** (port logic):
- `indexer/index.ts` (31 lines) — main entry, composes store + config + vault path
- `indexer/wiring.ts` (156 lines) — per-note indexing: parse → chunk → upsert note/chunks/links/aliases/tags/fm
- `indexer/wiring-scan.ts` (151 lines) — full vault scan loop with include/ignore filters
- `indexer/wiring-factory.ts` (88 lines) — build indexer shape (deleteNote, indexFullScan, reconcile)
- `indexer/wiring-helpers.ts` (83 lines) — row narrowing, chunk row mapping
- `indexer/prelude.ts` (108 lines) — hashFileContent, matchesInclude/IgnorePatterns, scanVaultMarkdownRelPaths, extractTitle
- `indexer/note-upsert.ts` (150 lines) — upsert note row (insert or update)
- `indexer/chunk-upsert.ts` (137 lines) — upsert chunks (compare chunk_hash for dedup)
- `indexer/note-meta.ts` (127 lines) — upsert links, aliases, tags, frontmatter fields; performNoteDeletion
- `indexer/migrations.ts` (233 lines) — full SQLite schema + triggers

**Store**:
- `store.ts` — Database open/close, sqlite-vec loading, migration execution
- `sync/sync-lock.ts` — advisory file lock with PID stale detection

**Frontmatter & links** (partially ported — verify parity):
- `shared/frontmatter.ts` — YAML parsing, Obsidian scalar quoting, wikilink/tag extraction
- `shared/links.ts` — wiki link resolution against note references

**MCP tools**:
- `mcp/tools/talon.ts` — tool registration pattern
- `mcp/tools/talon-search.ts`, `talon-read.ts`, `talon-sync.ts`, `talon-status.ts`, `talon-search-related.ts` — input schemas

**Tests** (port as Rust integration tests):
- `tests/fixtures/talon/fixture-vault.ts` — 21 fixture notes (Atlas/*, Graph/*, Search/*, Filters/*, Lifecycle/*)
- `tests/fixtures/talon/chunking-note.md` — chunker test fixture
- `tests/fixtures/talon/parser-note.md` — parser test fixture
- `tests/services/talon/hybrid-search.test.ts` — search algorithm tests
- `tests/services/talon/indexer.test.ts` — indexing tests
- `tests/services/talon/fixture-vault.integration.test.ts` — integration test fixture
- `tests/services/talon/contract.test.ts` — API contract tests
- `tests/services/talon/ranking-regression.test.ts` — ranking regression tests
- `tests/services/talon/query.test.ts` — query tests
- `tests/services/talon/cli.test.ts` — CLI tests

### Design spec reference

- `2026-04-25-talon-extraction-design.md` — full architectural spec
- Key decisions: stateless binary, no background work, post-rerank scope multipliers, single `talon` MCP tool, `~/.config/talon/config.toml` config location

## What's already scaffolded (DO NOT REWRITE)

The following already exists and is correct. **Do not delete or rewrite these.** The new work builds on them:

- `talon-core/src/constants.rs` — magic numbers as `const`
- `talon-core/src/error.rs` — `ErrorCode` enum + `TalonError` types
- `talon-core/src/config.rs` — scope model, priority multipliers, `TalonConfig` struct
- `talon-core/src/tool.rs` — all input/output types, response envelope
- `talon-core/src/frontmatter.rs` — YAML parsing, wikilink extraction, tag extraction, reverse indexes (with tests)
- `talon-core/src/links.rs` — wiki link resolution, backlinks, graph stats (with tests)
- `talon-core/src/change_tracking.rs` — file state, tombstones, change feed, `--since` parser (with tests)
- `talon-cli/src/cli.rs` — bpaf argument parsing
- `talon-cli/src/command.rs` — command dispatch (currently all stubs)
- `talon-cli/src/output.rs` — human/JSON/agent output formatters
- `talon-cli/src/config.rs` — config loading, `talon init`, Karpathy preset scopes
- `talon-cli/src/banner.rs`, `spinner.rs` — CLI polish

## Goals

Implement the full Talon Rust binary: chunker, SQLite store, search algorithms, indexer, query layer, MCP server, CLI wiring, TS wrapper.

## Quality Gates (must pass at every milestone)

### Build gates
- `just check` — runs `cargo check` (fast compile check)
- `cargo clippy --workspace -- -D warnings` — strict linting, zero warnings
- `cargo build --release` — release build succeeds
- `just test` — runs all unit and integration tests

### Test gates (red-green TDD discipline)
- **Write test first, then implementation.** Every new module/function starts with a failing test.
- Unit tests for pure functions (chunker, BM25 scoring, RRF fusion, scope resolution, where-parser).
- Integration tests against the fixture vault from `/tmp/talon-scaffold-imports/ultra/edge/src/tests/fixtures/talon/`.
- Parity tests: Rust output must match TS implementation output on the same input (paths match, scores within tolerance).
- `just test` must pass before any phase is marked complete.

### Code quality gates
- All public APIs documented with `///` docs
- `missing_debug_implementations = "warn"` — all types implement Debug
- `unsafe_code = "deny"` — no unsafe blocks
- `unused_must_use = "deny"` — Result/Option must_use enforced
- `expect_used = "warn"` — no `.expect()` in production code
- `unwrap_used = "deny"` — no `.unwrap()` in production code
- `todo = "warn"` — no TODOs in committed code
- `print_stdout = "warn"` — no print! in production code (use tracing or output module)

## Checklist

### Phase 0: Foundation ✓ (already done — skip)
- [x] 0.1 Workspace Cargo.toml with all dependencies
- [x] 0.2 Constants module (§5 magic numbers)
- [x] 0.3 Error types with full error code enum (§10.5)
- [x] 0.4 Config model — TalonConfig with scopes (§7.2)
- [x] 0.5 Tool input/output contracts — all actions (§11)
- [x] 0.6 Output envelope — unified shape (§10.5)
- [x] 0.7 `justfile` with check/test/clippy/build targets
- [x] 0.8 **Quality gate:** `just check` passes, `cargo clippy --workspace -- -D warnings` clean

### Phase 1: Chunker (pure functions, no DB dependency)
**Dependency: none. Foundation for everything downstream.**

- [ ] 1.1 `text.rs` — line splitting, fence/heading detection, token estimation, wikilink parsing, keyword/path normalization
  - Reference: `shared/text.ts` (100 lines)
  - Functions: `split_lines()`, `is_fence_line()`, `is_heading_line()`, `strip_heading_text()`, `estimate_tokens()`, `normalize_keyword()`, `normalize_vault_path()`, `parse_wikilink()`
  - Constants: `TOKEN_CHAR_RATIO=4`, `LF_LENGTH=1`, `HEADING_PATTERN`, `FENCE_PATTERN`
  - Tests: line splitting on CRLF/LF, fence detection, heading detection, wikilink parsing with alias/heading

- [ ] 1.2 `chunker.rs` — block-based markdown segmentation, chunk creation, overlap logic
  - Reference: `shared/chunker.ts` (297 lines)
  - Types: `MarkdownBlock` (fence/heading/paragraph), `NoteChunk` (hash, embedding text, heading path, token estimate)
  - Functions: `collect_blocks()`, `chunk_blocks()`, `chunk_markdown()`, `build_heading_path()`, `build_embedding_text()`, `make_chunk_hash()`
  - Constants: `CHUNK_TOKENS=900`, `OVERLAP_RATIO=0.15`, `OVERLAP_TOKENS=135`, `MAX_CHUNK_CHARS=3600`, `OVERLAP_CHARS=540`
  - Tests: chunking on fixture note (chunking-note.md), fence handling, heading-aware chunking, overlap boundaries

- [ ] 1.3 **Quality gate:** `just check` passes, `just test` passes with chunker tests

### Phase 2: SQLite store & migrations
**Dependency: Phase 0 (types, config, error). No Phase 1 dependency — store is independent infrastructure.**

- [ ] 2.1 `store.rs` — Database open/close, WAL mode, busy timeout, foreign keys, migration execution
  - Reference: `store.ts` (~100 lines)
  - Functions: `open_database()`, `run_migrations()`
  - PRAGMAs: `journal_mode=WAL`, `busy_timeout=10000`, `foreign_keys=ON`
  - Tests: open/close database, migrations create all tables

- [ ] 2.2 `migrations.rs` — full SQLite schema from TS reference
  - Reference: `indexer/migrations.ts` (233 lines)
  - Tables: notes, chunks, links, note_aliases, note_tags, note_frontmatter_fields, settings, event_log, llm_cache, vector_metadata
  - FTS virtual tables: `notes_fts_bm25` (unicode61 tokenizer), `notes_fts_fuzzy` (trigram tokenizer)
  - Triggers: FTS sync on INSERT/UPDATE/DELETE of notes
  - Indexes: idx_links_to, idx_chunks_note_chunk_index, idx_note_aliases_alias_norm, idx_note_tags_tag_norm, idx_fm_field_value_norm, idx_notes_active_path, idx_notes_hash, idx_notes_docid, idx_chunks_hash
  - Tests: schema creates all tables, triggers fire correctly

- [ ] 2.3 `sqlite-vec.rs` — sqlite-vec extension loading
  - Reference: `sqlite-vec.ts` (loadSqliteVecInto pattern)
  - Function: register sqlite-vec functions with rusqlite connection
  - Tests: sqlite-vec MATCH query works

- [ ] 2.4 Note/chunk upsert helpers
  - Reference: `indexer/note-upsert.ts` (150 lines), `indexer/chunk-upsert.ts` (137 lines)
  - Functions: `upsert_note()` (compare mtime/size for up-to-date), `upsert_chunks()` (compare chunk_hash for dedup)
  - Tests: insert new note, update existing note, chunk dedup by hash

- [ ] 2.5 Meta upsert helpers
  - Reference: `indexer/note-meta.ts` (127 lines)
  - Functions: `upsert_links()`, `upsert_aliases()`, `upsert_tags()`, `upsert_frontmatter_fields()`, `perform_note_deletion()`
  - Tests: aliases normalized, frontmatter flattened, note deletion soft-deletes

- [ ] 2.6 **Quality gate:** `just check` passes, `just test` passes with store tests

### Phase 3: Search algorithms
**Dependency: Phase 2 (store for DB queries). Pure algorithm functions first, then DB integration.**

- [ ] 3.1 `search/bm25.rs` — BM25 FTS query via SQLite FTS5
  - Reference: `search/bm25.ts` (91 lines)
  - SQL: `bm25(notes_fts_bm25, 10.0, 5.0, 1.0)` — OHS weights title=10, alias=5, content=1
  - Functions: `search_bm25()`, `search_by_alias_exact()`
  - Snippet: `snippet(notes_fts_bm25, 2, '', '', '...', ?)`
  - Tests: BM25 scoring on fixture vault, exact alias match

- [ ] 3.2 `search/text_fts.rs` — FTS query building, trigrams, score normalization
  - Reference: `search/text-fts.ts` (59 lines)
  - Functions: `sanitize_fts_query()`, `to_fts_query()`, `build_trigram_or_query()`, `build_bm25_score()`, `get_trigrams()`, `calculate_trigram_overlap()`
  - Constants: `TRIGRAM_LEN=3`, `BM25_MIN_TOKENS=10`, `BM25_TOKENS_PER_CHAR_DIV=4`
  - Tests: trigram generation, FTS query sanitization, BM25 score normalization (abs(raw)/(1+abs(raw)))

- [ ] 3.3 `search/fuzzy_title.rs` — trigram-based fuzzy title/alias matching
  - Reference: `search/fuzzy-title.ts` (82 lines)
  - Functions: `search_title_parts()`, `search_fuzzy_title()`, `max_alias_overlap()`, `map_fuzzy_fts_row()`
  - Logic: exact alias via FTS5, fuzzy via trigram overlap × BM25 score
  - Tests: fuzzy match on fixture vault, exact alias separation

- [ ] 3.4 `search/vector.rs` — sqlite-vec cosine distance search
  - Reference: `search/vector.ts` (157 lines)
  - SQL: `MATCH vec_f32(?) AND k = ?` two-step: get chunk_ids + distances, then fetch chunk metadata
  - Functions: `search_vector()`, `distance_to_score()` (max(0, 1 - distance / COSINE_DISTANCE_MAX))
  - Constants: `COSINE_DISTANCE_MAX=2`
  - Tests: vector search returns correct chunks, distance-to-score mapping

- [ ] 3.5 `search/rrf.rs` — Reciprocal Rank Fusion
  - Reference: `search/rrf.ts` (106 lines)
  - Functions: `build_rrf_accumulator()`, `accumulate_rrf_scores()`, `normalize_and_merge_rrf_results()`
  - Constants: `RRF_K=60`, `RRF_WEIGHTS={bm25:2, exactAlias:2, fuzzy:0.5, semantic:1}`
  - Normalization: `hybridScore = rawScore / maxPossibleScore`, clamped to [0, 1]
  - Tests: RRF fusion of multiple ranked lists, weight application, normalization

- [ ] 3.6 `search/fuse.rs` — result fusion, strong-signal detection, rerank blending
  - Reference: `search/fuse.ts` (139 lines)
  - Functions: `estimate_strong_signal()`, `fuse_hybrid_result_lists()`, `blend_rerank_candidates()`, `sigmoid()`, `clamp01()`, `rerank_weight_for_rank()`
  - Strong signal: top score ≥ 0.85 AND top - second ≥ 0.15
  - Rerank blending: weighted hybrid + sigmoid(rerank_score), weights by rank (top=0.75, mid=0.6, low=0.4)
  - Tests: strong signal detection, fuse with/without rerank, sigmoid normalization

- [ ] 3.7 `search/cache.rs` — LRU caches for expansion and rerank
  - Reference: `search/llm-cache.ts` (214 lines)
  - Types: `LruCache<K, V>` with configurable capacity
  - Functions: `build_expansion_cache_key()`, `build_rerank_cache_key()`, `dedupe_query_variants()`
  - Constants: `LLM_CACHE_LIMIT=1000`, `GLOBAL_HYBRID_CACHE_SIZE=100`
  - Tests: LRU eviction, cache key construction, variant deduplication

- [ ] 3.8 **Quality gate:** `just check` passes, `just test` passes with search algorithm tests

### Phase 4: Indexer & sync
**Dependency: Phase 2 (store) + Phase 1 (chunker) + Phase 3 (search for FTS triggers).**

- [ ] 4.1 `indexer/prelude.rs` — file utilities
  - Reference: `indexer/prelude.ts` (108 lines)
  - Functions: `hash_file_content()`, `matches_ignore_patterns()`, `matches_include_patterns()`, `try_stat_sync()`, `load_notes_for_linking()`, `merge_current_path_for_linking()`, `extract_title()`, `scan_vault_markdown()`
  - Tests: pattern matching, title extraction, vault scanning

- [ ] 4.2 `indexer/wiring.rs` — per-note indexing pipeline
  - Reference: `indexer/wiring.ts` (156 lines)
  - Pipeline: parse markdown → extract frontmatter → chunk → upsert note → upsert chunks → upsert links → upsert aliases → upsert tags → upsert frontmatter fields
  - Functions: `index_one_note()`, `index_path()`
  - Tests: full index of a single fixture note

- [ ] 4.3 `indexer/wiring_scan.rs` — full vault scan loop
  - Reference: `indexer/wiring-scan.ts` (151 lines)
  - Functions: `run_full_scan()`, `process_one_markdown()`
  - Logic: scan all .md files, skip unchanged (mtime+size check), apply include/ignore filters
  - Tests: full scan on fixture vault, skip unchanged files, apply filters

- [ ] 4.4 `indexer/wiring_factory.rs` — build indexer shape
  - Reference: `indexer/wiring-factory.ts` (88 lines)
  - Functions: `build_indexer()` with `delete_note()`, `index_full_scan()`, `reconcile()`
  - Tests: indexer shape has all methods

- [ ] 4.5 `sync.rs` — sync orchestration with lock
  - Reference: `sync/sync-lock.ts` (~150 lines), `indexer/wiring-scan.ts`
  - Functions: `acquire_sync_lock()`, `release_sync_lock()`, `run_sync()`, `detect_deleted_files()`, `create_tombstones()`, `prune_old_tombstones()`
  - Lock file: `{pid, startedAt}`, stale detection via `kill(pid, 0)`
  - Tombstones: 90-day retention, `TOMBSTONE_RETENTION_MS`
  - `--fast`: lexical pass only (no embeddings)
  - `--force`: reset vector state
  - Tests: sync lock acquisition/release, tombstone creation, fast sync

- [ ] 4.6 Embedding pass (one-shot via TEI HTTP endpoint)
  - Reference: `embed/chunks-and-write.ts`, `embed/chunks-run.ts`
  - Functions: `embed_chunks()`, `embed_chunked()`, `write_vectors()`
  - HTTP: POST `/embed` (batch), POST `/embed-chunked` (chunked)
  - Tests: embedding pass on indexed chunks, vector write to vec_chunks table

- [ ] 4.7 **Quality gate:** `just check` passes, `just test` passes with indexer tests

### Phase 5: Query layer
**Dependency: Phase 3 (search) + Phase 4 (indexer).**

- [ ] 5.1 `query/search.rs` — search query handler
  - Reference: `search/hybrid-pipeline.ts` (68 lines), `search/hybrid-single.ts` (42 lines)
  - Modes: hybrid, semantic, fulltext, title
  - Hybrid pipeline: probe (bm25 + title) → expansion (LLM) → multi-query → fuse → rerank → filter → scope multiplier
  - Functions: `run_search()`, `run_hybrid_pipeline()`, `run_semantic_search()`, `run_fulltext_search()`, `run_title_search()`
  - Scope awareness: default scopes, `--scope`, `--scope-only`, post-rerank multiplier application
  - Tests: search on fixture vault, scope filtering, mode variants

- [ ] 5.2 `query/read.rs` — read handler
  - Reference: `query/read.ts`
  - Functions: `read_note()`, `read_raw()`, `read_line_range()`
  - Tests: read fixture notes, line ranges, raw mode

- [ ] 5.3 `query/related.rs` — related notes handler
  - Reference: `query/related-run.ts`, `query/related-links.ts`, `query/related-map.ts`
  - Functions: `find_related()`, `traverse_graph()`, `build_related_map()`
  - Direction: outgoing, backlinks, both
  - Depth: 1-3
  - Tests: related traversal on fixture vault graph

- [ ] 5.4 `query/meta.rs` — frontmatter query handler
  - Reference: `query/index.ts` (meta portion)
  - Functions: `query_frontmatter()`, `apply_where_filter()`, `get_tag_counts()`, `reverse_source_lookup()`
  - `--where`: key OP value with =, !=, <, <=, >, >=, contains, exists
  - `--select`: specific frontmatter fields
  - `--tag-counts`: aggregate tag counts
  - `--sources`: reverse-source index lookup
  - Tests: where filter on fixture vault, tag counts, source lookup

- [ ] 5.5 `query/changes.rs` — change feed handler
  - Reference: `change_tracking.rs` (already implemented)
  - Functions: `get_changes_since()`, `classify_changes()` (added vs modified)
  - Tests: change feed on fixture vault with simulated changes

- [ ] 5.6 `query/lint.rs` — lint check handler
  - Reference: `query/graph.ts`
  - Checks: orphans, broken-links, dangling-refs, unreferenced
  - Functions: `check_orphans()`, `check_broken_links()`, `check_dangling_refs()`, `check_unreferenced()`
  - Tests: lint on fixture vault (Graph/Grandchild is orphan, nonexistent links are broken)

- [ ] 5.7 `query/status.rs` — status handler
  - Reference: `query/index.ts` (status portion)
  - Functions: `get_status()`, `compute_index_stats()`, `get_scope_report()`
  - Tests: status on indexed fixture vault

- [ ] 5.8 **Quality gate:** `just check` passes, `just test` passes with query tests

### Phase 6: MCP server
**Dependency: Phase 5 (query layer).**

- [ ] 6.1 `mcp/server.rs` — MCP-over-stdio server
  - Reference: `mcp/tools/talon.ts` (tool registration pattern)
  - Protocol: JSON-RPC 2.0 over stdio
  - Methods: `initialize`, `tools/list`, `tools/call`, `notifications/initialized`
  - Functions: `run_mcp_server()`, `handle_initialize()`, `handle_tools_list()`, `handle_tools_call()`, `handle_notification()`
  - Tool schema: full JSON Schema for all actions (§11)
  - Tests: MCP handshake, tools/list returns correct schema, tools/call dispatches correctly

- [ ] 6.2 `mcp/protocol.rs` — JSON-RPC frame reading/writing
  - Functions: `read_frame()`, `write_frame()`, `parse_request()`, `build_response()`, `build_error_response()`
  - Tests: frame serialization/deserialization, error responses

- [ ] 6.3 **Quality gate:** `just check` passes, `just test` passes with MCP tests

### Phase 7: CLI wiring & polish
**Dependency: Everything above.**

- [ ] 7.1 Wire all CLI subcommands to real handlers
  - Reference: `command.rs` (currently all stubs)
  - Commands: `search`, `read`, `sync`, `related`, `status`, `meta`, `changes`, `lint`, `init`
  - Replace stubs with real handler calls
  - Tests: CLI end-to-end on indexed fixture vault

- [ ] 7.2 `talon init` — write config with Karpathy preset scopes
  - Reference: `config.rs` (already implemented)
  - Tests: `talon init` creates config, config parses correctly

- [ ] 7.3 Human CLI UX polish
  - Reference: `output.rs` (already has formatters)
  - Enhance: terminal-width-aware wrapping, colored headings, compact result cards
  - Progress: spinner for sync, progress bar for long operations
  - Tests: human output renders correctly

- [ ] 7.4 **Quality gate:** `just check` passes, `just test` passes, `talon --help` renders

### Phase 8: TypeScript wrapper & distribution
**Dependency: Phase 7 (binary is built).**

- [ ] 8.1 `ts/src/binary.ts` — platform-matched binary resolution via optionalDependencies
  - Reference: npm pattern from esbuild/biome/oxc
  - Subpackages: `talon-darwin-arm64`, `talon-darwin-x64`, `talon-linux-x64`, `talon-linux-arm64`
  - Tests: binary resolution works (requires npm available)

- [ ] 8.2 `ts/src/child.ts` — `mcpChildSpec()` returns `{ command, args: ['--mcp'], env: {} }`
  - Reference: spec §9

- [ ] 8.3 npm package.json with optionalDependencies
  - Reference: spec §13

- [ ] 8.4 **Quality gate:** `npm run build` succeeds (if npm available)

### Phase 9: Final verification
- [ ] 9.1 `just check` passes (cargo check)
- [ ] 9.2 `cargo clippy --workspace -- -D warnings` clean
- [ ] 9.3 `cargo build --release` succeeds
- [ ] 9.4 `just test` passes (all unit + integration tests)
- [ ] 9.5 `talon --help` renders
- [ ] 9.6 `talon --skill` prints SKILL.md
- [ ] 9.7 `talon --version` works
- [ ] 9.8 `talon init` creates config template
- [ ] 9.9 `talon search --help` works
- [ ] 9.10 Parity test: Rust search output matches TS output on fixture vault (paths match, scores within tolerance)
- [ ] 9.11 **Quality gate:** All quality gates pass, parity test passes

## Verification

Every checklist item records verification evidence (commands run, test outputs, parity comparisons).

`just check` must pass before any item is marked complete.
`cargo clippy --workspace -- -D warnings` must pass before any item is marked complete.
`just test` must pass before any phase is marked complete.
Parity tests compare Rust output against TS reference output on the same fixture vault.

## Notes

- Spec references: §3 (repo layout), §4 (process model), §5 (binary surface), §6 (scopes), §7 (config), §8 (inference), §9 (TS wrapper), §10 (frontmatter/link graph/change tracking), §11 (MCP tool surface), §12 (what to copy), §13 (distribution)
- Decisions baked in: stateless binary, no background work, post-rerank scope multipliers, single `talon` MCP tool, `~/.config/talon/config.toml` config location
- Reference code lives in `/tmp/talon-scaffold-imports/ultra/edge/src/services/talon/` — this is the actual TS implementation to port
- Existing scaffold: workspace, constants, basic CLI parsing, banner, spinner, output module, config template, error types, tool contracts, frontmatter parsing, link resolution, change tracking — all in `crates/` and `ts/`
