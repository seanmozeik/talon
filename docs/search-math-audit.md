# Search Math Audit

Date: 2026-04-27

Scope: the retrieval constants and score transforms exercised by US-026.

| Area | talon | Reference | Status |
|---|---|---|---|
| BM25 column weights | `crates/talon-core/src/search/constants.rs:84-88` | OHS `searcher.ts:237` | aligned |
| BM25 score normalization | `crates/talon-core/src/search/text_fts.rs:235-238` | OHS `searcher.ts:260` | aligned |
| RRF k | `crates/talon-core/src/search/constants.rs:33` | OHS `searcher.ts:721` | aligned |
| RRF per-list weights | `crates/talon-core/src/search/constants.rs:49-54` | OHS `searcher.ts:1390-1392` | aligned via US-007 |
| RRF normalization cap | `crates/talon-core/src/search/fuse.rs:110-160` | OHS `searcher.ts:751-758` | aligned |
| FTS query construction | `crates/talon-core/src/search/text_fts.rs:119-211` | OHS `searcher.ts:209-214` | aligned with the same `OR`-join and sanitization shape |
| Trigram overlap math | `crates/talon-core/src/search/text_fts.rs:42-68` and `crates/talon-core/src/search/fuzzy_title.rs:56-139` | OHS `searcher.ts:368-410` | aligned; talon keeps the same overlap-squared penalty |
| Exact alias bypass for short tokens | `crates/talon-core/src/search/bm25.rs:111-157` | OHS `searcher.ts:336-366` | aligned and covered by integration test |
| Cosine distance ceiling | `crates/talon-core/src/search/constants.rs:107` and `crates/talon-core/src/search/vector.rs:20-26` | OHS `searcher.ts:691` | intentional metric-source divergence, equivalent score range |

## Notes

- `distance_to_score` uses `1 - distance / COSINE_DISTANCE_MAX` with `COSINE_DISTANCE_MAX = 2.0`. sqlite-vec returns cosine distance directly, so this is the correct talon-side form of the OHS similarity transform.
- `to_fts_query` strips the same FTS5-sensitive punctuation the current test suite already exercises, and it preserves `OR` joining for title search.
- `build_trigram_or_query` returns a quoted literal for inputs shorter than three characters so the fuzzy path stays non-erroring even when there are no trigrams to expand.
- `search_title_parts` keeps the exact-alias path ahead of fuzzy scoring, which is why the short-token integration test now passes for `A`, `Go`, and `C#`.

## Verification

- [x] COSINE_DISTANCE_MAX divergence comment added in code.
- [x] FTS query construction audited.
- [x] Trigram query construction and overlap math audited.
- [x] RRF normalization audited.
- [x] Short-token alias integration coverage added.
