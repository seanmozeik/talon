---
title: Talon plan vs implementation — drift audit
date: 2026-04-27
author: synthesis-pass
status: draft
inputs:
  - 2026-04-25-talon-extraction-design.md (the original spec, 1024 lines, "stale" per maintainer)
  - prd.json (31 user stories)
  - 2026-04-27-dogfood-findings.md (sibling document; surface-level CLI quality)
  - 2026-04-27-memory-landscape-research.md (sibling document; recall redesign research)
sibling_inputs:
  - /tmp/talon-comparison-prd.md (PRD audit, 31 stories)
  - /tmp/talon-comparison-plan-part1.md (spec §3-§13 surface audit, 247 requirements)
  - /tmp/talon-comparison-plan-decisions.md (spec §22 resolved decisions, 21 items)
---

# Executive summary

Three audit passes (PRD stories, spec §3–§13 surface, spec §22 resolved decisions) read against the current codebase. Headline counts:

| Source                       | Items | Match | Drift / Partial | Missing | N/A (ultraclaw) |
|------------------------------|-------|-------|-----------------|---------|-----------------|
| PRD user stories             | 31    | 28    | 2               | 1       | —               |
| Spec §3–§13 surface          | 247   | 201   | 22 (19P + 3D)   | 12      | —               |
| Spec §22 resolved decisions  | 21    | 15    | 1 (deliberate)  | 0       | 3               |

**The maintainer's standing note:** *"there's lots of decisions made that are deliberate and improvements over the spec."* So this document **flags differences without asserting they're regressions**. Each item lists what the spec said, what the impl does, and a one-line read on whether the difference looks deliberate or accidental — final judgment is the maintainer's.

The five highest-leverage items, regardless of category:

1. **Scope iteration order is alphabetical (BTreeMap), not declaration order.** Spec §6.3 explicitly relies on declaration order to encode specificity ("put narrower scopes above broader ones"). BTreeMap sorts by scope name. Looks accidental — the alphabetical-vs-insertion semantics is a Rust container choice that has user-visible consequences when scope globs overlap.
2. **`--scope` / `--scope-only` CLI flags are unwired.** The Rust types carry the fields, every command builder hardcodes them empty, and `cli.rs` has zero scope-related arguments. Spec §5 and §6.4 require both flags. Looks accidental.
3. **Tombstone persistence has no SQL table.** Tracking is in-memory; spec §10.3 specified persistence. `changes --since` returns deletions in the response struct, but they may not survive sync runs.
4. **Output envelope is flattened in `--agent` mode** — confirmed deliberate. `--json` emits the full Decision 8 envelope; `--agent` strips it to raw `data` for token efficiency. Spec §10.5 / Decision 8 should be amended so `--agent` is documented as a third output mode rather than implicit drift.
5. **Boost-multiplier "shouts" empirically** even though spec §6.5 reasoning predicted it shouldn't. The mechanism is correct; the spec's prediction relied on the reranker producing wide score gaps that don't seem to materialize in practice. Detail in dogfood findings 3.2.

The rest of this document is the per-item layout grouped by likely category (deliberate / accidental / ambiguous), plus a section on places the implementation grew beyond the spec.

---

# 1. Where the implementation looks deliberately different from the spec

These are differences where the impl seems to have made a considered choice. Listed for explicit confirmation, not as concerns.

## 1.1 `--agent` mode flattens the response envelope

- **Spec (§10.5, Decision 8):** every JSON response uses `{action, version, ok, data, meta}` for success, `{action, version, ok: false, error}` for failures. Decision 8 is "locked."
- **Impl:** `crates/talon-core/src/contracts/mod.rs:211` defines `TalonEnvelope` correctly; `--json` mode emits the full envelope. `crates/talon-cli/src/output/json/agent.rs:12-35` flattens for `--agent`, emitting only the `data` payload.
- **Read:** **confirmed deliberate (per maintainer 2026-04-27)** — agent-mode flattening is an intentional improvement over Decision 8 for token efficiency. Full envelope remains available via `--json` for tooling, observability, and human inspection. The recall redesign should adopt the same convention. Spec §10.5 / Decision 8 should be amended to document `--agent` as the third explicit output mode with a defined flat shape.

## 1.2 The `recall` command exists beyond §5's binary surface

- **Spec (§5):** lists `search`, `read`, `sync`, `related`, `status`, `meta`, `changes`, `lint`. No `recall`.
- **Impl:** `crates/talon-cli/src/command/recall.rs` ships a full recall pipeline (PRD US-025), with `prompt-xml` format, evidence_score gate, multi-section response. The Hermes plugin (`integrations/hermes-talon-recall/`, US-026) is built around it.
- **Read:** clearly deliberate — recall is the agent-memory use case the maintainer is now redesigning. Spec was written before that path crystallized.

## 1.3 `--intent` flag added beyond spec

- **Spec:** no `--intent` flag.
- **Impl:** `cli.rs:181` exposes `--intent=STRING` ("disambiguating context for the query"). Steers expansion, rerank, and chunk selection.
- **Read:** likely deliberate — solves the real problem of one-word queries lacking context. Worth documenting.

## 1.4 `--anchors` flag (US-022a)

- **Spec:** anchors mentioned in passing as future-looking.
- **Impl:** `cli.rs:212` exposes `--anchors`; rich match anchors with char offsets and heading breadcrumbs (US-022a) shipped. `crates/talon-core/src/search/anchor.rs:32-70`.
- **Read:** deliberate, opt-in, useful.

## 1.5 Single workspace structure (no separate `talon-mcp` crate)

- **Spec (§3):** lists the layout neutrally.
- **Impl:** MCP lives inside `talon-cli` rather than a separate crate. Cleaner; one fewer dependency edge.
- **Read:** deliberate simplification.

## 1.6 `vector_metadata` table beyond spec

- **Spec (§10):** schema discussion doesn't mention vector dim tracking.
- **Impl:** schema includes `vector_metadata` table tracking embedding dims per chunk.
- **Read:** deliberate — supports embed-model migrations safely.

---

# 2. Where the difference looks accidental (or at minimum, asks for an explicit decision)

These are differences with user-visible consequences that don't have an obvious "we chose this on purpose" answer.

## 2.1 Scope iteration uses BTreeMap (alphabetical), not declaration order

- **Spec (§6.3):** "Walk the configured scopes **in declaration order** and assign the file to the **first matching scope**. Specificity is encoded by config order: put narrower or more sensitive scopes (`private`, `archive`) above broader ones (`wiki`)."
- **Impl:** `crates/talon-core/src/config.rs:85` declares `ScopesConfig = BTreeMap<String, Scope>`. BTreeMap iterates keys in lexicographic order, so the user's TOML order is discarded. `crates/talon-core/src/config.rs:149 resolve_scope` walks `scopes.values()` in BTreeMap order.
- **Practical consequence:** if the Karpathy preset has `[scopes.archive]`, `[scopes.private]`, and `[scopes.wiki]`, BTreeMap iterates `archive`, `private`, `wiki` regardless of TOML declaration order. As long as scope globs are non-overlapping (each file matches exactly one scope) the bug is invisible. The moment globs overlap (e.g. someone adds a `wiki/private/**` exclusion idea, or has a file in `archive/wiki/foo.md`), specificity ordering breaks.
- **Read:** looks accidental. BTreeMap is the natural Rust-stable choice if you're not thinking about iteration order; switching to `IndexMap` (or `Vec<(String, Scope)>`) preserves insertion order at a small dep cost.

## 2.2 `--scope` and `--scope-only` CLI flags are unwired

- **Spec (§5, §6.4):** both flags required; multi-valued, additive vs exclusive, mutually exclusive on a single invocation, error on invalid name.
- **Impl:** `Vec<String>` fields exist in every input type (`SearchInput`, `RecallInput`, `ChangesInput`, `LintInput`, `MetaInput`, `ReadInput`). All seven CLI command builders hardcode `scope: Vec::new()` and `scope_only: Vec::new()`. `grep "scope" cli.rs` returns zero hits.
- **MCP path is wired:** the action discriminator carries `scope`/`scope_only` through `mcp/tool/dispatch.rs`. So the MCP surface honors the spec; only the CLI doesn't.
- **Read:** looks accidental — no rationale to leave a Vec field on every CLI input type if the design intent was to omit the flag. Plausibly the CLI plumbing got skipped in a hurry and never returned to. This is the dogfood report's biggest finding too.

## 2.3 Tombstone persistence — no SQL table

- **Spec (§10.3):** "tombstones table for deleted files" with `(path, deleted_at)`. Pruned older than 90 days.
- **Impl:** `crates/talon-core/src/sync/change_tracking.rs` defines a `TombstoneEntry` struct and an in-memory model. `query/changes.rs` populates `deleted` in the response. **No `CREATE TABLE tombstones` in any migration.**
- **Practical consequence:** depends on what the in-memory tracking actually does. If tombstones are reconstructed from notes-table-row-disappearance during sync, they may work for "what was deleted in this run." For "what was deleted between two arbitrary times" — the spec's `changes --since` semantics — they probably don't survive a process restart.
- **Read:** ambiguous — could be "we'll do this when needed" or could be an oversight. Worth a one-line confirmation.

## 2.4 Lint roots exclusion is not in config

- **Spec (§10.4):** orphans should exclude paths in a `lint.roots` config field, defaulting to `index.md`, `README.md`, `_meta/index.md`.
- **Impl:** no `lint.roots` field in `TalonConfig`. Orphan check fires on every file in graph-rooted scopes regardless.
- **Practical consequence:** in the dogfood vault, `_meta/VAULT_INDEX.md` is the index file; nothing links *into* it. So in a strict reading it'd flag as an orphan (it didn't, because `_meta/` files happen to have backlinks from each other in the chef vault). On a different vault shape this would surface noise.
- **Read:** likely accidental — small config field that was easy to skip. Pairs with dogfood report 3.3 (orphan-on-daily noise).

## 2.5 Frontmatter values stored as TEXT; no `value_type` column

- **Spec (§10.1):** `value_type ∈ {string, number, bool, date, list}` stored alongside the value.
- **Impl:** schema is `note_frontmatter_fields(note_id, field, value, value_norm)`. All values are TEXT. `--where` operators compare strings.
- **Practical consequence:** for `--where date < 2026-04-01` to work, the parser needs to coerce both sides to dates at query time. Not having a stored type means every comparison does parse-on-read. Numeric `<` / `>` ops may not work intuitively for fields a user thinks of as numbers.
- **Read:** ambiguous — could be a deliberate "keep the schema simple, parse at query time" call, or could be an oversight. Worth confirming.

## 2.6 Link table uses paths, not IDs; no `link_type` column

- **Spec (§10.2):** `links(source_file_id, target_file_id_or_null, target_text, link_type)` with `link_type ∈ {wikilink, markdown}`.
- **Impl:** schema is `links(from_path, to_path, raw_target, heading, alias)`. Path-based, not ID-based; no link-type discrimination.
- **Practical consequence:** path-based isn't broken (paths are unique enough as identifiers in Obsidian) but it'll be slower and trickier on rename. No link-type means `lint broken-links` and `related` treat all link forms identically, which is probably fine in an Obsidian-native vault but could matter if the codebase is later asked to handle other markdown flavors.
- **Read:** ambiguous. Possibly deliberate ("Obsidian uses wikilinks; we don't need to discriminate"). Path-vs-ID likely deliberate too (avoids a join). Flagging so it's a confirmed call.

## 2.7 `meta.scope_set` is hardcoded to `None` for most commands

- **Spec (§10.5):** `meta.scope_set` is "resolved active set, where applicable."
- **Impl:** `command/{changes,lint,meta,read,related}.rs` all set `scope_set: None`. Only `recall.rs:116` populates it (from default scope names). Search in particular: the dogfood report surfaced empty `scope_set` despite scope multipliers being applied internally.
- **Read:** accidental — the field exists, the data exists, the wiring is incomplete. Trivial to fix once `--scope` flags are real, since the resolved set is what gets fed to the multiplier anyway.

## 2.8 US-018 missing — no cross-build pipeline

- **Spec / PRD US-018:** `cargo zigbuild` produces stripped release binaries for darwin-arm64, darwin-x64, linux-x64, linux-arm64, win32-x64. `optionalDependencies` in `ts/package.json` carries them.
- **Impl:** `optionalDependencies` are declared (US-017 ✓), `.github/workflows/release.yml` exists, but no zigbuild step. **The npm meta-package would resolve to empty platform subpackages.**
- **Read:** clearly accidental in the sense that the npm package can't actually ship binaries on a release tag without this. Probably "deferred until first real release" — needs to land before US-024 (final verification) can be run end-to-end.

---

# 3. Where the spec was probably wrong (or impl revealed something the spec didn't anticipate)

## 3.1 §6.5 reasoning relies on a reranker score-gap assumption that doesn't hold

- **Spec §6.5:** "A `boosted` candidate above a `normal` one only wins if they were close in raw relevance; a wildly more relevant `normal` result still wins. A `buried` result needs to be ~60× more relevant than a `normal` one to surface."
- **Impl:** mechanism is correct (`config.rs:34 ScopePriority::multiplier` returns the right values; `query/search.rs:318` applies them post-rerank). But empirically, on `"fermented hot sauce"`, `wiki/Sauce Mothers` (3.0× boosted, partially relevant) lands at score 2.5 while `projects/Fermented Hot Sauce Line` (1.5× elevated, *the actual subject*) lands at 1.39. The reranker did not produce a wide-enough gap for the multiplier to behave as the spec predicted.
- **Reranker input is `title\n\nsnippet`** (`crates/talon-core/src/search/rerank_pipeline.rs:55-60`) — the cross-encoder sees the title plus the same 300-char heading-anchored snippet that gets returned to the agent. Not the full chunk; not the whole note.
- **Position-weighted blend** further damps the rerank contribution at the top of the list (`rerank_pipeline.rs:45-53`):
  - rank 0–9 → `(0.75 hybrid, 0.25 rerank)`
  - rank 10–19 → `(0.60, 0.40)`
  - rank 20+ → trust rerank more
  So even when the cross-encoder produces a wide gap on title+snippet, only 25% of that gap reaches the final score for top candidates — then the 3.0× boost multiplier hits.
- **Read:** the spec's §6.5 reasoning was optimistic on two counts: (1) it assumed strong reranker discrimination, but discrimination is bounded by the size of the title+snippet input window — when titles share words ("Sauce Mothers" vs "Fermented Hot Sauce Line") and snippets cover similar vocabulary, the cross-encoder may genuinely score them close; (2) the position-weighted blend only gives the rerank score 25% of the weight at the top of the list. Research-report rec #7 (priority × relevance with a floor) addresses this by making the boost multiplier *not fire* below a relevance threshold. This is a **spec-level gap**, not an implementation gap — the impl honors the spec; the spec's reasoning needs revision.

## 3.2 How the snippet / rerank pipeline actually works (and why it matters for scopes)

The §3.1 finding is structural, not a one-off scoring quirk. The pipeline that feeds the cross-encoder was inherited from **obsidian-hybrid-search (OHS)** — a flat-vault search tool — and the constants were copied verbatim. None of it was tuned for a scope-priority world. This subsection collects what the pipeline actually does so the calibration trade-offs are visible.

### Two snippets: one for the reranker, one for the agent

There are **two distinct snippets** in the pipeline. The reranker sees one; the agent sees a different (post-processed) one. They're never the same value. This was a non-obvious finding from re-reading the code.

**Stage 1 — the raw snippet (built during retrieval, fed to reranker):**

| Branch         | `raw.snippet` source                                                              | Includes matched text? | File:line |
|----------------|-----------------------------------------------------------------------------------|------------------------|-----------|
| **BM25 (FTS)** | SQLite FTS5 `snippet(notes_fts_bm25, 2, '', '', '...', N)` over the content column. FTS5 picks the window centred on matched terms and ellipsises the rest. The size target is roughly 75 tokens (`DEFAULT_SNIPPET_LENGTH/4` = 75 from `BM25_TOKENS_PER_CHAR_DIV`). May come back **shorter** than 300 chars when the document is short or the match is near a boundary. | **Yes — by construction.** | `crates/talon-core/src/search/bm25.rs:65` |
| **Semantic (vector)** | `raw.snippet = c.text` — the whole 900-token chunk text, no windowing. | The chunk that semantically matched is the snippet. | `vector.rs:102` |
| **Alias-exact** | `raw.snippet = String::new()` — empty. | N/A (only a title/alias matched). | `bm25.rs:142`, `fuzzy_title.rs:122` |

**The reranker is fed `${title}\n\n${raw.snippet}`** (`rerank_pipeline.rs:55-60`). No heading breadcrumb, no expansion of short BM25 snippets, no post-processing. So the cross-encoder sees three asymmetric input shapes:

- BM25 candidates: title + a possibly-short FTS5 window (sometimes well under 300 chars).
- Semantic candidates: title + a full ~900-token chunk.
- Alias-exact candidates: title + empty string.

These three shapes go into the same cross-encoder call on the same score scale, with no normalization. A 900-token chunk and a 100-char snippet are scored against the query as-is.

**Stage 2 — the displayed snippet (what the agent / human actually sees):**

After reranking and after the scope multiplier, `raw_to_search_result` (`crates/talon-core/src/query/search.rs:181-224`) transforms `raw.snippet` into the snippet shown in the response. Three things happen, in order:

1. **BM25 short-snippet fallback** (`search.rs:202-210`). If `raw.snippet.chars().count() * 2 < DEFAULT_SNIPPET_LENGTH` and the candidate has a BM25 score, `maybe_expand_bm25_snippet` re-queries the FTS index for a longer window centred on the match. So a snippet that came back at 80 chars from the original BM25 retrieval may grow to ~300 chars here. **The reranker never saw this expanded version.**
2. **Heading breadcrumb prepended** (`search.rs:213-218`). `resolve_snippet_heading` looks up which chunk's `heading_path` contains the snippet's matched text and joins it as `"H1 > H2 > H3"`. Then the snippet becomes `format!("{breadcrumb}\n{snippet}")`. **The reranker never saw the breadcrumb either.**
3. **Truncation to `DEFAULT_SNIPPET_LENGTH` chars** (`search.rs:221-224`). `.chars().take(300).collect()`. So if the breadcrumb is long, less of the actual snippet body fits in the 300-char budget.

Comment in source at `search.rs:220`: *"Algorithm ported verbatim from obsidian-hybrid-search (MIT) — searcher.ts:1209."* So this is OHS-derived display formatting, applied after the OHS-derived rerank.

**Net consequences for calibration:**

- The cross-encoder's discrimination is bounded by what's in `raw.snippet`. For BM25 hits with short windows, the reranker may have very little to work on — sometimes 50-150 chars. The agent then sees a much richer 300-char snippet with breadcrumb because of the expansion + prepend. So the agent and the reranker are looking at *different* evidence.
- For semantic hits, the reranker sees ~900 tokens. The agent sees the same chunk text truncated to 300 chars (with breadcrumb prepended). So semantic candidates get *more* context at the rerank stage and *less* in display, while BM25 candidates get the reverse.
- The breadcrumb counts against the 300-char display budget. A deep heading path eats budget the agent could otherwise spend on content.

### The position-weighted blend

`rerank_pipeline.rs:45-53`:

| Pre-rerank rank index | (hybrid weight, rerank weight) |
|----------------------:|--------------------------------|
| 0–9                   | `(0.75, 0.25)` |
| 10–19                 | `(0.60, 0.40)` |
| 20+                   | `(0.50, 0.50)` *(deeper trusts rerank more)* |

Comment in source: *"Top results trust hybrid more; deeper results trust rerank more."*

That means at rank 0, the cross-encoder's score contributes only 25% of the blended value. The hybrid (RRF-fused BM25 + semantic + fuzzy-title) carries 75%. Whatever differentiation the cross-encoder produces gets *quartered* before the scope multiplier hits.

### OHS-derived constants

The retrieval pipeline imports a calibrated set of magic numbers from OHS. Locations:

| Constant | Value | Source comment |
|----------|------:|----------------|
| `BM25_FTS_SCORES.title`   | 10.0 | "Default BM25 OHS weights: title=10, alias=5, content=1" |
| `BM25_FTS_SCORES.alias`   | 5.0  | (same) |
| `BM25_FTS_SCORES.content` | 1.0  | (same) |
| `DEFAULT_SNIPPET_LENGTH`  | 300  | OHS default |
| `BM25_TOKENS_PER_CHAR_DIV`| 4    | OHS default |
| `RERANK_TOP_K`            | 40   | "Mirrors `RERANK_CANDIDATE_LIMIT` from the root constants and the TS reference" |
| `CANDIDATE_FLOOR`         | 40   | per-retriever over-fetch |
| RRF k                     | 60   | OHS / standard RRF default |

All from `crates/talon-core/src/search/constants.rs`. The TS reference Talon was forked from also inherited these from OHS.

### Why this matters for scopes

OHS calibration was done in a flat-vault world: every match competes on relevance only. The scope-priority multiplier (3.0× / 1.5× / 1.0× / 0.3× / 0.05×) was added on top, post-rerank, *after* the OHS-tuned blend. None of the upstream weights know about scopes.

Concrete consequences:

- **Title weight 10× vs content weight 1×** is an OHS judgment that "title matches matter much more." That's reasonable when content matches are noisy. With scopes, the question is whether a wiki article with a strong content match still beats a project file with a strong title match — currently, the BM25 ranking is dominated by title weight before the scope multiplier even sees the candidate.
- **Cross-encoder input asymmetry** (often-short BM25 window for keyword hits vs full ~900-token chunk for semantic hits, vs title-only for alias-exact) means the discrimination quality varies by how the candidate was retrieved. In a flat vault that's invisible; with scope priorities, a wiki article retrieved via semantic match may get a more confident rerank score than the same article retrieved via BM25 — and that confidence interacts with the multiplier downstream. The retrieval branch a candidate came in through quietly affects what scope-priority effectively means for it.
- **Reranker and agent see different snippets.** The post-rerank expansion + breadcrumb prepend means the agent's view of a result is richer than what the cross-encoder used to score it. Tuning the boost multiplier against agent-visible snippets (e.g. by reading agent output and judging "this should rank higher") can be misleading because the rerank score that fed the multiplier was based on different content.
- **The 25% rerank weight at the top of the list** was a sensible OHS choice when the *rerank itself* was the differentiation tool. With a scope multiplier sitting downstream of the blend, the multiplier — not the rerank — becomes the primary differentiator at rank 0–9. That's a *different* dynamic than OHS designed for.
- **Semantic-branch full-chunk snippets vs BM25-branch 300-char snippets** create a presentation asymmetry too: agents see longer snippets for semantic-retrieved hits and shorter ones for keyword-retrieved hits, regardless of which is more relevant.

These are not bugs against the spec. The spec inherited the OHS calibration. They're places where the **calibration regime changed** when scopes landed, and the constants didn't get re-examined. Worth thinking through before tuning anything: changing one knob in isolation (e.g. lowering boost from 3.0× to 2.0×) without revisiting the OHS-baseline assumptions could move the failure mode rather than fix it.

### Possible threads to pull (informational, not prescriptive)

- **Calibrate the boost multiplier against the OHS blend.** What multiplier value, given the 25% rerank weight at rank 0, would actually let "a wildly more relevant normal candidate" beat a Boosted candidate when the spec said it should?
- **Normalize cross-encoder input.** If the rerank input shape varies wildly (300 chars vs 900 tokens), is the cross-encoder's score comparable across them?
- **Reconsider the per-position blend with scopes in the mix.** If the multiplier already encodes "trust this scope," does the position-weighted blend still want to dampen the rerank?
- **Snippet length asymmetry.** The 300-char BM25 default and the full-chunk semantic snippet behave differently for agents and for the reranker. There's an argument either could change; mainly worth flagging that they're *different* by accident of retrieval branch, not by design.

None of this is a recommendation. Listed here so the calibration surface is in one place when you're ready to think it through.

---

## 3.3 `default = true|false` semantics are spec-ambiguous

- **Spec §6.4:** "talon search ... → searches every scope where `default = true`." Reads as a *filter*.
- **Spec §6.5:** describes priority multipliers but doesn't address what `default` means for the candidate pool.
- **Impl:** `default` is rank-only; private/archive content is in the pool but down-weighted by Buried (0.05×). On lexical hits the down-weight is enough that ranking effectively buries it, but it remains visible.
- **Read:** the spec uses both filter language ("searches every scope where") and ranker language (the multipliers) without resolving which is canonical. Implementation chose ranker. Both have merit; the choice should be made explicit. Dogfood report 3.1 lays out three options (rename, filter, split into two fields).

---

# 4. Spec items that are honored as written

For completeness; brief.

| Item | Verdict |
|------|---------|
| Decision 1 (stateless process model) | Honored. No watcher, no scheduler, no clock-driven work. |
| Decision 2 (opaque scope labels) | Honored. `BTreeMap<String, Scope>` — no enums of scope semantics. |
| Decision 3 (scope shape: glob/priority/default) | Honored. Plus the unscoped fallback to `(normal, true)`. |
| Decision 4 (multiplier values) | Honored. Exact values, hardcoded const. |
| Decision 5 (lint scope: 4 checks only) | Honored. `LintCheck` enum has the four; nothing else. |
| Decision 6 (meta + `--where` operators) | Honored. All 8 operators implemented. |
| Decision 7 (changes returns added/modified/deleted) | Honored at struct level. (Tombstone persistence open per 2.3.) |
| Decision 9 (single config at `~/.config/talon/config.toml`) | Honored. |
| Decision 10 (single MCP tool, action union) | Honored. All 9 actions wired. |
| Decision 11 (`talon init` doesn't overwrite) | Honored. |
| Decision 12 (`skill/SKILL.md` at repo root, `--skill` flag) | Honored. |
| Decision 13 (TEI-compatible inference docs) | Honored. README mentions TEI / Infinity. |
| Decision 14 (npm package name) | Honored. `@seanmozeik/talon`. |
| Decision 16 (MCP parity + new actions) | Honored. All 8 spec'd + recall. |
| Decision 18 (JSON Schema in tools/list) | Honored. |
| Decision 20 (sync lock inside Rust binary) | Honored. PID-aware advisory lock with RAII guard. |
| §3 repo layout | Matches spec, plus extras. |
| §5 binary surface (commands + most flags) | Matches spec; `--scope` family is the only gap. |
| §7 config schema (every field) | Matches spec exactly. |
| §8 inference abstraction | Matches spec; no OpenAI-specific assumptions in the inference client. |
| §9 TS wrapper | Matches spec. |
| §11 MCP tool surface | Matches spec. |

`scope_set: None` and the BTreeMap iteration aside, scope plumbing through MCP works end-to-end (it's only the CLI that's missing the flags).

Three decisions are N/A for this audit — they're about ultraclaw, not Talon:
- **Decision 17** — Doctor probes. Spec'd a single thin probe wrapping `talon status --json` from ultraclaw's doctor surface. The probe lives in ultraclaw, not Talon.
- **Decision 19** — Container shim. Spec'd that the in-container `talon` command routes through edge's host CLI shim (like `gog`/`remi`/`bird`). Edge's concern.
- **Decision 21** — Ultraclaw config split. Reduces `EdgeConfig.talon` in ultraclaw to operational caller knobs only (`enabled`, `watch`, `vaultPath`, `syncTimeoutMs`); implementation knobs move to `~/.config/talon/config.toml`. Talon's side of this is honored — its config carries the implementation knobs. Ultraclaw's side is out of this audit.

(The §22 decisions audit summary said "5 N/A" — that was an arithmetic error in the audit; the body listed only Decisions 17, 19, 21.)

---

# 5. Cross-cutting observations

A few things came out of the audits that don't fit cleanly into "spec said X, impl does Y."

## 5.1 The PRD and the spec disagree slightly on whether `recall` is an in-scope feature

- The spec (§5) lists 8 commands; recall isn't one of them.
- The PRD (US-025, US-026) treats recall and the Hermes plugin as committed deliverables.
- The codebase ships them.
- Suggests the spec was written before recall crystallized; PRD updated; spec didn't catch up. Standard "stale spec" symptom — flagged for awareness.

## 5.2 Help-text completeness

`talon --help` is comprehensive but flag descriptions are flat — every flag at top level. There's no per-subcommand help (`talon search --help` returns the same global help). Spec didn't constrain this; mentioning because it interacts with the dogfood report's "agents need clear help text" implicit assumption.

## 5.3 The audit found ~12 deferred verifications

These are items where the audit couldn't verify in the time given (e.g., specific BM25/RRF formulas, sync-lock file format, CI workflow steps). Listed under "deferred" totals; not gaps, just stuff to confirm if you want absolute coverage. Sub-spec details, mostly.

## 5.4 What's covered in dogfood, here, and the research report — and what isn't

| Concern                                  | Dogfood | This doc | Research report |
|------------------------------------------|---------|----------|-----------------|
| CLI surface usability                    | yes     | partial  | no              |
| Spec-vs-impl drift in code               | minimal | yes      | no              |
| Recall command behaviour                 | yes (briefly) | no | yes (whole report) |
| Memory landscape & injection mechanics   | no      | no       | yes             |
| Scope CLI flag                           | yes     | yes      | no              |
| Tombstones / schema details              | no      | yes      | no              |
| Boost-shouts / multiplier behaviour      | yes     | yes (brief) | yes (rec #7)  |

So: the three reports together cover surface, structure, and direction. They overlap intentionally on a few hot items (`--scope`, boost-shouts, envelope) so each report is independently readable.

---

# 6. Suggested next moves (not prescriptive)

Pick whichever subset matters; nothing here is auto-blocking.

- **Decisions to make explicit, regardless of code change.** §1.1 (envelope flattening), §2.5 (frontmatter value type), §2.6 (link table shape), §3.2 (default = filter or ranker). Each could be a 5-line spec amendment.
- **Concrete code items that look like oversights.** §2.1 (BTreeMap → IndexMap), §2.2 (`--scope` CLI flag), §2.7 (`scope_set: None`). All small, mechanical fixes.
- **Concrete code items that are deferred deliberate work.** §2.3 (tombstone table), §2.8 (zigbuild cross-build). Both probably "do before v0.1.0 release."
- **Spec-level gap:** §3.1 (boost shouts) and §3.2 (default semantics) are open product questions. The recall research report's rec #7 proposes a mechanism that addresses both.

---

# 7. Sources

- Original spec: `/home/yolo/talon/2026-04-25-talon-extraction-design.md` (1024 lines, "stale" per maintainer)
- PRD: `/home/yolo/talon/prd.json` (31 user stories)
- PRD audit: `/tmp/talon-comparison-prd.md` (118 lines)
- §3–§13 surface audit: `/tmp/talon-comparison-plan-part1.md` (511 lines, 247 requirements)
- §22 decisions audit: `/tmp/talon-comparison-plan-decisions.md` (124 lines, 21 decisions)
- Sibling: `/home/yolo/talon/docs/2026-04-27-dogfood-findings.md`
- Sibling: `/home/yolo/talon/docs/2026-04-27-memory-landscape-research.md`

The three audit artifacts in `/tmp/` are intermediate; if useful for future reference, copy them into `docs/` or commit them under a `docs/audits/` subfolder. Otherwise they can be discarded.
