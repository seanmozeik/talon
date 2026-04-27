---
title: Talon dogfood findings — chef vault, agent-mode CLI surface
date: 2026-04-27
author: dogfood-pass
status: draft
talon_version: 0.1.0
vault: /tmp/talon-dogfood-vault (73 markdown files, 8 directories, Karpathy-style)
config: ~/.config/talon/config.toml (Karpathy preset; original at .bak)
---

# Executive summary

- **`--scope` is unimplemented at the CLI layer.** The `Vec<String>` fields exist in `talon-core` types but every CLI command file hardcodes `scope: Vec::new()` and `scope_only: Vec::new()`. The Karpathy preset documented in `config.toml`'s comments is therefore half-functional: priority multipliers affect ranking, but agents have no way to filter to or exclude specific scopes from queries. **This is the single highest-leverage fix.**
- **`default = false` is rank-only, not exclusion.** A query touching `private/` content surfaces it in default search at low rank rather than gating it behind explicit opt-in. The Karpathy plan's intent ("reference existence only, read when explicitly asked") is not enforced.
- **Boost multiplier "shouts" on partially-relevant wiki articles in default search,** not just recall. `wiki/Sauce Mothers` (score 2.5) outranks `projects/Fermented Hot Sauce Line` (1.39) for the query "fermented hot sauce." Validates research-report rec #7 (priority × relevance) at the search layer too.
- **`changes --since` returns a misleading slice** — `DEFAULT_LIMIT=10` plus alphabetic ordering surfaces only `_meta`/`archive`/`private` paths regardless of `--since 7d` vs `--since 30d`.
- **No `mtime` on any agent JSON output.** Agents must rely on frontmatter `last_updated`, which lies (haiku-generated frontmatter has stale dates). Filesystem mtime would be ground truth.

This document captures findings only. No fixes are made.

---

# Setup

The dogfood vault is generated to mirror Karpathy's LLM Wiki structure: a single Obsidian vault tended over ~14 months by a fictional chef-restaurateur (Marco Reyes / Calle Sur). 73 markdown files distributed across 8 directories:

| Directory     | Files | Karpathy role                        | Talon scope priority | `default` |
|---------------|-------|---------------------------------------|----------------------|-----------|
| `wiki/`       | 15    | Compiled, agent-curated knowledge     | `boosted` (3.0×)     | `true`    |
| `projects/`   | 13    | Active workspaces                     | `elevated` (1.5×)    | `true`    |
| `artifacts/`  | 4     | Agent outputs for the user            | `normal` (1.0×)      | `true`    |
| `daily/`      | 20    | Ephemeral daily notes                 | `muted` (0.3×)       | `false`   |
| `raw/`        | 10    | Untreated source material             | `muted` (0.3×)       | `false`   |
| `archive/`    | 3     | Completed/closed projects             | `buried` (0.05×)     | `false`   |
| `private/`    | 4     | Sensitive (lease, payroll, financial) | `buried` (0.05×)     | `false`   |
| `_meta/`      | 4     | Vault infrastructure                  | `buried` (0.05×)     | `false`   |

Indexed in 37s; 73/73 embedded; 183 chunks; 1024-dim embeddings. Status `Ready` per `talon status`.

The vault was generated with strict cross-linking: every wiki article links to 4–7 manifest files; daily notes reference 2–5; projects reference wiki and raw; raw clips reference target wiki articles. Frontmatter is consistent (type-specific keys, plausible `compiled` / `last_updated` / `archived` dates).

All testing used `--agent` mode (compact JSON, UI art disabled).

---

# 1. What works well

These are the surfaces that already feel solid for an agent harness.

**Consistent JSON envelope.** `search`, `related`, `meta`, `changes`, `lint`, `recall` all return `{vault, ...}` with per-command payload underneath. Agents can build a unified parser.

**Heading-anchored snippets.** Search snippets return with breadcrumbs like `Vault Index > Cooking & Technique` rather than first-N chars. Significantly better signal density than naïve windowing — the agent gets context for free.

```
"snippet":"Vault Index > Cooking & Technique\n## Current Projects\n\n- [[Spring 2026 Menu]] — ..."
```

**`related` separates outgoing vs backlink and includes `linkText`.**

```json
{"path":"wiki/Sauce Mothers.md","title":"Sauce Mothers",
 "relation":"backlink","linkText":"Lacto-Fermentation"}
```

The `linkText` is what disambiguates ambiguous backlinks (e.g. wiki link via alias vs canonical name). Useful, and not present in many vault tools.

**Lint catches real broken wikilinks.** The chef-vault haikus produced realistic mistakes — wikilinks like `[[Charred Spring Onion]]` against the actual file `Dish - Charred Spring Onion.md`. `talon lint --agent` correctly flagged 22 such cases. This is the single highest-value curation surface for an agent that maintains a vault.

```json
{"path":"daily/2026-03-23.md",
 "message":"broken link: Charred Spring Onion → Charred Spring Onion (not found)"}
```

**Frontmatter is fully serialized in `meta`.** No second `read` required to inspect properties.

```json
{"path":"artifacts/Spring 2026 Costed Menu.md",
 "frontmatter":{"artifact_kind":"costed-menu","generated":"2026-04-24",
  "generated_by":"agent","sources":["Spring 2026 Menu","Dish - ..."],
  "tags":["artifact","menu","costing","spring2026"],"title":"...","type":"artifact"}}
```

**`--agent` flag does its job.** Compact JSON, no human ornamentation, no spinners visible in stdout.

---

# 2. Bugs

## 2.1 `--scope` flag is unimplemented at the CLI

**Severity:** high — it's the single feature most needed for the agent-driven Mode B navigation use case described in the memory-landscape research report.

**Evidence:** All `talon-core` query input types carry scope fields (`SearchInput`, `RecallInput`, `ChangesInput`, `LintInput`, `MetaInput`, `ReadInput` all have `scope: Vec<String>` and `scope_only: Vec<String>`). The CLI command builders hardcode them to empty:

| File                                                  | Line | Code                          |
|-------------------------------------------------------|------|-------------------------------|
| `crates/talon-cli/src/command/changes.rs`             | 23   | `scope: Vec::new(),`          |
| `crates/talon-cli/src/command/changes.rs`             | 24   | `scope_only: Vec::new(),`     |
| `crates/talon-cli/src/command/lint.rs`                | 32   | `scope: Vec::new(),`          |
| `crates/talon-cli/src/command/lint.rs`                | 33   | `scope_only: Vec::new(),`     |
| `crates/talon-cli/src/command/meta.rs`                | 25   | `scope: vec![],`              |
| `crates/talon-cli/src/command/meta.rs`                | 26   | `scope_only: vec![],`         |
| `crates/talon-cli/src/command/read.rs`                | 56   | `scope_set: None,`            |

`grep -n "scope" crates/talon-cli/src/cli.rs` returns zero matches — the CLI definition has no scope arguments at all.

**Reproduction:**

```
$ talon search "fermented hot sauce" --scope wiki
Error: no such flag: `--scope`, did you mean `--mcp`?
```

**Implications:**
- The Karpathy preset advertised in the `config.toml` comments ("uncomment and edit") is half-functional. Priorities apply, but `--scope wiki`, `--scope projects`, `--scope all`, `--scope personal` cannot be invoked.
- Every other finding in this report becomes easier to address once scope filtering exists. It's the prerequisite.

**Fix shape:** add bpaf bindings for `--scope` and `--scope-only` (repeatable), validate against the configured scope names, plumb through to each command's input struct. ~50 lines. Probably worth a small PR independent of any other work.

## 2.2 `meta --tag-counts` emits typed-as-int keys

**Severity:** low — produces malformed data but doesn't crash.

**Evidence:** Output of `talon --agent meta --tag-counts` includes:

```json
"tagCounts":{"4":1,"Diana-Henry":1,"archive":3, ...}
```

The `"4":1` entry is almost certainly the indexer coercing a YAML integer (`tags: [4]` somewhere in a frontmatter block) to a string and counting it as a tag. Cleanest fix: skip non-string values during tag aggregation, optionally emit a warning surfaced via `meta` diagnostics.

**Fix shape:** one-line filter in the tag aggregation pass.

---

# 3. Design gaps vs the Karpathy plan

These are not bugs in the strict sense — the code does what the code says. But the user-visible behavior diverges from what the plan committed in `config.toml` advertises.

## 3.1 `default = false` is a ranking bias, not a filter

**Severity:** high — privacy leak risk on the `private/` scope.

**Evidence:** Query `talon --agent search "lease renewal landlord"` (no explicit scope opt-in) returns:

| Rank | Path                                  | Score | Default scope? |
|------|---------------------------------------|-------|----------------|
| 1    | `private/Lease Notes 2026.md`         | 0.05  | **false**      |
| 2    | `private/Financial Projections 2026`  | 0.04  | **false**      |
| 3    | `projects/Tasting Counter`            | 1.28  | true           |
| ...  |                                       |       |                |
| 9    | `daily/2026-04-24.md`                 | 0.20  | **false**      |
| 10   | `raw/Voice Memo - Foraging walk ...`  | 0.20  | **false**      |

The `Buried` (0.05×) multiplier lowers the score, but the result is still in the response. Any lexical hit on private content surfaces it. The Karpathy plan states for `private/`: "Reference existence only, read when explicitly asked." Talon's current semantics is "rank low," not "exclude unless asked."

**Why this matters:** in the agent-driven Mode B use case (the agent searches the vault deliberately), the agent expects "default search" to be a curated, safe view. If `private/` content can leak via lexical hits the agent didn't intend, the boundary is purely advisory.

**Decision required, not just a fix:**
- Option A — keep current rank-only semantics, rename `default` to something like `included_in_default_priority` to match.
- Option B — make `default = false` an actual filter (excluded from default search; returned only via `--scope <name>` or `--include-non-default`).
- Option C — split into two fields: `priority: ...` (existing) and `auto_search: bool` (new; controls whether the scope is in the default candidate pool).

Recommend C — preserves backward compatibility with the priority knob and gives a clean filter signal.

## 3.2 Boost multiplier "shouts" on partial-relevance wiki articles

**Severity:** medium — produces visibly wrong rankings on real queries.

**Evidence:** Query `talon --agent search "fermented hot sauce"` returns top three:

| Rank | Path                                                        | Score | Boost?          |
|------|-------------------------------------------------------------|-------|-----------------|
| 1    | `projects/Fermented Hot Sauce Line/Fermented Hot Sauce Line.md` | 1.39  | elevated 1.5×   |
| 2    | `raw/Email - Distributor Quote Hot Sauce Co-Pack.md`        | 0.27  | muted 0.3×      |
| 3    | `wiki/Sauce Mothers.md`                                     | **2.50** | **boosted 3.0×** |

`wiki/Sauce Mothers` is at most tangentially relevant (it discusses sauce theory; it doesn't mention hot sauce, fermentation, or any concrete topic in the query). Yet it scores higher than the project file that *is* the subject of the query, because the 3.0× Boosted multiplier fires on a low-relevance match.

**This is the "memory shouts" failure mode the research report flagged for `recall` showing up in `search`.** It's not a recall-only problem.

**Fix shape:** apply scope priority as `priority × max(0, relevance − floor)` rather than `priority × relevance` directly. With `floor = 0.4`, the Boosted multiplier doesn't fire on hybrid scores below 0.4 — Sauce Mothers stops shouting; the project file ranks correctly. Research-report rec #7 in `2026-04-27-memory-landscape-research.md` proposes this exact mechanic; it should apply to `search` as well as `recall`.

## 3.3 `lint` orphan check is noise-heavy on `daily/`

**Severity:** medium — drowns the actionable findings.

**Evidence:** Output of `talon --agent lint` returned 44 total issues:

| Issue type      | Count | Useful to agent? |
|-----------------|-------|------------------|
| Orphans         | 22    | Mostly no — 20 of 22 are `daily/*.md` |
| Broken links    | 22    | Yes — every one is actionable curation feedback |

Daily notes are written-from, not written-to — they should not have incoming wikilinks by design. Yet they all flag as orphans, and they outnumber the broken-link findings 1:1, halving the signal density of the response.

**Fix shape:** add a scope-aware exclusion. Either:
- A `--ignore-priority muted,buried` CLI flag.
- A per-scope `lint_orphans: bool` config field defaulting to `true` for `boosted`/`elevated`/`normal` and `false` for `muted`/`buried`.
- A heuristic: skip orphan check for files whose containing directory is named in a known "ephemeral" pattern (`daily/`, `journal/`, `inbox/`).

The first is the cleanest CLI handle; the second is the cleanest config story. Likely both.

## 3.4 `changes --since` defaults `--limit` to 10 and orders alphabetically

**Severity:** medium — agent calling "what changed in the vault" gets a misleading slice.

**Evidence:** `talon --agent changes --since 7d` and `talon --agent changes --since 30d` both return **the same 10 paths**, all from `default = false` scopes:

```
_meta/lint-report.md
_meta/last-garden-pass.md
_meta/VAULT_INDEX.md
_meta/schema.md
archive/Bread Program (Closed)/Bread Program.md
archive/Winter 2025 Menu/Dish - Cassoulet.md
archive/Winter 2025 Menu/Winter 2025 Menu.md
private/Financial Projections 2026.md
private/Staff Issue April Conflict.md
private/Payroll Considerations Q2.md
```

`_meta`, `archive`, `private` are alphabetically the first three top-level directories. `DEFAULT_LIMIT = 10` (`crates/talon-core/src/constants.rs:7`) caps the response. The 60+ files in `wiki`/`projects`/`daily`/`raw`/`artifacts` are absent.

**Why this matters:** an agent asking "what's changed lately?" expects most-recent-first across the whole vault, not alphabetic-first capped at 10.

**Fix shape:** order by mtime DESC, raise default limit (or accept the current default and document it loudly). Could optionally accept `--limit` to override, which already exists per `cli.rs` but is treated as DEFAULT when omitted.

---

# 4. Quality nits

Smaller surface improvements. Each is a one-or-two-line change at the serializer/aggregator level.

## 4.1 No `mtime` field in any agent JSON response

`search`, `related`, `meta`, `changes`, `recall` all omit a freshness signal per result. The agent has to do one of:
- Call `read` on each path and parse `last_updated` from frontmatter (slow, and `last_updated` is unreliable — haiku-generated frontmatter in this very vault has plausible-but-static dates that don't reflect actual filesystem mtime).
- Trust `recall`'s `recent_edits` section, which is recall-specific.

**Fix:** include `mtime` (ISO 8601) on every per-path response object across all commands. The research report recommends Zep's inline `(YYYY-MM-DD - present)` style for recall specifically; for the other commands a plain `mtime` field is enough.

## 4.2 `meta --tag-counts` truncates per-path entries to ~10 by default

The tag-count dictionary is global (correct), but the per-path `entries: [...]` list is capped at `DEFAULT_LIMIT = 10`. Agent gets a 10/73 slice with no indication it was truncated. Either:
- Default the limit to `usize::MAX` for `meta --tag-counts` (the tag dict is the headline; the per-path list is incidental).
- Or always include a `truncated: true` flag and `total_count: 73` in the envelope when a default limit hits.

## 4.3 `related` has no edge strength

A note linked once is treated identically to one referenced from 3 headings.

```json
{"path":"_meta/VAULT_INDEX.md","title":"VAULT_INDEX","relation":"backlink","linkText":"Lacto-Fermentation"}
```

There's no `count` or `weight` field. Agent navigating the link graph (Mode B) can't tell central vs peripheral connections.

**Fix:** add `count: int` (number of times the link appears in the source document) per related entry; optionally `headings: [...]` for which sections contained the link. Indexer already has this data — it's a serialization gap.

## 4.4 Agent JSON does not surface the scope a result came from

Agent receives `path` and `score`; it can infer scope by re-applying the glob from config, but that's brittle. Adding `scope: "wiki"` to each result lets the agent reason about scope mix in its own way (e.g., "ignore artifacts when answering technique questions"). Trivial serializer addition.

---

# 5. Findings dependency graph

Some of the above findings unblock others. Suggested ordering when they're picked up:

```
2.1 (--scope CLI flag)
  ├── enables 3.1 fix (default-as-filter requires --scope to opt back in)
  ├── enables 3.3 fix (--ignore-priority is one form of scope filter)
  └── enables 3.4 fix (changes --scope <name> is the ergonomic shape)

3.2 (priority × relevance gate)
  └── independent — apply at the rerank stage in talon-core; affects search and recall

4.1 (mtime everywhere)
  └── independent — serializer-only; one-day fix; unblocks recall redesign

4.3, 4.4 (edge strength, scope-on-result)
  └── independent — serializer-only

2.2 (typed-as-int tag keys)
  └── independent — one-line indexer filter
```

If forced to pick one, **2.1 first.** Almost every other gap becomes more tractable after `--scope` is real.

---

# 6. Open questions

- **What's the right default value of `default` for new scopes?** Currently the user has to opt in; that means every Karpathy-style scope ships with `default = false` for the muted/buried tiers. Is that the right ergonomics, or should `default` derive from `priority` (e.g. `muted`/`buried` ⇒ `default = false` automatically unless overridden)?
- **Should `meta`/`lint`/`changes` honor the scope priority multiplier at all,** or treat all matched files equally? Today they pull from the index without applying scope — but `lint`'s noise problem suggests at minimum `--ignore-priority` should be available.
- **Recall-specific findings are deliberately omitted** from this report — the research report `2026-04-27-memory-landscape-research.md` covers the recall redesign separately. Some search/recall findings may move during that redesign (e.g. the `min_confidence` gate may tighten); this report is search/lint/related/meta/changes-focused.
- **Should `--scope all` be a thing** (override default-search filter, include every scope at full multiplier)? Karpathy plan suggests yes; current Talon has no equivalent.

---

# 7. Reproduction commands

For someone walking through these findings cold:

```bash
# Build the dogfood vault and index (already done, capture for posterity)
ls /tmp/talon-dogfood-vault/  # 8 directories, 73 markdown files
cargo run -q --bin talon -- status

# 2.1 --scope unimplemented
cargo run -q --bin talon -- search "fermented hot sauce" --scope wiki  # errors

# 2.2 typed-as-int tag keys
cargo run -q --bin talon -- --agent meta --tag-counts | jq '.entries[0].frontmatter.tags, .tagCounts | keys'

# 3.1 default = false leaks via lexical hits
cargo run -q --bin talon -- --agent search "lease renewal landlord" | jq '.results[].path'

# 3.2 boost shouts
cargo run -q --bin talon -- --agent search "fermented hot sauce" | jq '.results[] | {path, score}'

# 3.3 lint orphan noise
cargo run -q --bin talon -- --agent lint | jq '[.checks.orphans[] | select(.path | startswith("daily/"))] | length'

# 3.4 changes alphabetic + capped
cargo run -q --bin talon -- --agent changes --since 30d | jq '.added'
```

---

# 8. Source citations

- Talon CLI: `/home/yolo/talon/crates/talon-cli/`
- Talon core types: `/home/yolo/talon/crates/talon-core/src/query/{input,output}.rs`
- Default limit: `crates/talon-core/src/constants.rs:7`
- Hardcoded empty scope vectors: see § 2.1 table.
- Karpathy plan: original conversation that motivated this dogfooding pass.
- Research report (sibling document): `docs/2026-04-27-memory-landscape-research.md`.
