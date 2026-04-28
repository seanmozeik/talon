---
title: Talon — scope CLI follow-up + remaining work
date: 2026-04-28
status: handoff
supersedes:
  - docs/2026-04-27-dogfood-findings.md
  - docs/2026-04-27-plan-vs-implementation.md
---

# What this doc is

A snapshot of where Talon stands after the scope-CLI work and the mechanical
follow-ups landed on 2026-04-28. The two prior audit docs covered a moment
where `--scope` was unwired and per-result envelopes were stripped down; both
have since shifted enough that pulling findings from them in isolation will
mislead. This doc is the current source of truth.

---

# What's now in main

Across two commits — `87c7312` (scope CLI wiring) and the follow-up batch
landing today — the following is shipped:

## Scope CLI surface

- `-s` / `--scope NAME` — repeatable, additive opt-in to a scope on top of
  the default pool. Required to surface any scope with `default = false`.
- `--scope-only NAME` — repeatable, exclusive search.
- `--scope-all` — every configured scope, overriding `default = false`.

The three are mutually exclusive on a single invocation. Unknown scope names
error fast with the configured-name list. `meta.scope_set` echoes the
resolved active scope names on every response.

Honored across `search`, `recall`, `related`, `meta`, `changes`, and `lint`.
`read` is path-targeted, no scope flags.

## Option B `default = false`

Scopes flagged `default = false` are now **excluded entirely** from default
queries. The previous behaviour (rank with a `Buried` 0.05× multiplier and
let lexical hits leak through) is gone. Re-include via `--scope NAME`,
`--scope-only NAME`, or `--scope-all`.

This decision is intentional and replaces the spec §6.4/§6.5 ambiguity
between filter semantics and ranker semantics.

## Per-scope `lint` config + global `[lint].ignore`

Each `[scopes.<name>]` table now accepts a `lint = bool` field (default
`true`). Top-level `[lint]` table accepts `ignore = ["glob", ...]`.
`talon lint` filters findings whose `from_path` is either:

- in a scope with `lint = false`, or
- matched by any glob in `lint.ignore`.

Excluded paths are **still indexed and used for link resolution** — a
broken link in a wiki note that targets a `daily/` file is still detected;
the broken-link finding emits if and only if the *source* file
(`from_path`) is included.

Karpathy preset defaults: `daily`, `archive`, `private` ship with
`lint = false`. The dogfood report's "20-of-22 orphans are `daily/`" noise
is gone.

## ScopeFilter (config-aware glob matching)

`talon_core::ScopeFilter` resolves scope names against configured glob
patterns rather than naive prefix matching. Replaces four duplicated
`passes_scope_filter` helpers that silently broke when scope names differed
from directory prefixes (`scopes.notebook.glob = "personal/notes/**"` would
have worked accidentally before; works correctly now).

Lives at `crates/talon-core/src/config/scope_filter.rs`. Public API:
`from_args`, `default_for`, `accepts(path)`, `resolved_set()`. 14 unit
tests cover `Default`, `Only`, `All`, mutual exclusivity, unknown names,
unscoped fallback, and `resolved_set` in all modes.

## Result envelope enrichments

All canonical per-result types (`SearchResult`, `RelatedResult`,
`MetaEntry`) now carry:

- `scope: Option<String>` — resolved scope name (skip-if-none, so unscoped
  paths just omit the field).
- `mtime: Option<String>` — file modification time in system local TZ.
  `"HH:MM"` for edits within the last 24h, `"YYYY-MM-DD"` otherwise. Recent
  edits get instantly-readable wall-clock time; older edits collapse to
  date. SQLite handles both the TZ conversion (`'localtime'`) and the 24h
  branch (`CASE WHEN ms / 1000 >= strftime('%s','now') - 86400 ...`) at
  query time, dodging the multi-threaded fallibility of
  `time::UtcOffset::current_local_offset()`.

Compact agent types (`AgentSearchHit`, etc. in
`crates/talon-cli/src/output/json/agent.rs`) deliberately do **not** carry
these fields — token budget over freshness signal for agent-mode
consumers. Recall's `prompt-xml` output likewise stays compact. Full
`--json` mode and `--verbose` see the new fields.

`ChangeEntry.indexed_at` and `TombstoneEntry.deleted_at` changed from
`u64` ms-epoch to ISO 8601 strings.

## `RelatedResult.count`

Each related entry now carries `count: u32` — number of distinct link rows
between source and target. A note linked once vs three times via different
aliases scores 1 vs 3, a rough proxy for edge strength. Implementation:
`SELECT to_path, MIN(...), COUNT(*) ... GROUP BY to_path` (was `SELECT
DISTINCT`).

## `meta --tag-counts` no longer truncates entries silently

When `--tag-counts` is set and the user didn't pass `--limit`, the entries
list defaults to `u16::MAX` instead of the global `DEFAULT_LIMIT = 10`.
The tag-count dictionary is the headline; entries are incidental and
shouldn't be a 10-of-N silent slice. Explicit `--limit` still wins.

## `ScopesConfig` declaration order

Type alias changed from `BTreeMap<String, Scope>` to
`indexmap::IndexMap<String, Scope>`. Iteration follows TOML declaration
order, matching spec §6.3 ("put narrower scopes above broader ones").
Currently invisible (the Karpathy preset has non-overlapping globs) but
correct as soon as anyone adds an overlapping scope.

## Code organisation

`crates/talon-core/src/config.rs` was getting too long; split into
sibling modules: `config/chunker.rs`, `config/endpoints.rs`,
`config/scope_filter.rs`. Public API unchanged via re-exports.

Search retrieval extracted to `query/search_retrieval.rs` to keep
`query/search.rs` under the 350-line per-file budget.

`query/mtime.rs` is a small new utility module: `format_iso8601(u64)` and
`iso8601_for_path(conn, vault_path)`.

---

# What's still left

## §3.2 — boost-multiplier "shouts" (the real product question)

This is the one substantive thread that didn't get pulled today. The
problem: on `"fermented hot sauce"` against the dogfood vault,
`wiki/Sauce Mothers` (Boosted 3.0×, partially relevant) outranks
`projects/Fermented Hot Sauce Line` (Elevated 1.5×, *the actual subject*)
because the multiplier fires at full strength on a low-relevance match.

Why it isn't a one-knob fix:

- Pipeline calibration is OHS-derived (`obsidian-hybrid-search`) and was
  set for a flat-vault world. Title weight 10× over content weight 1×;
  position-weighted blend gives rerank only 25% weight at rank 0–9;
  cross-encoder input asymmetry (often-short BM25 windows vs full ~900-
  token semantic chunks). All in `crates/talon-core/src/search/`.
- The 3.0× multiplier sits *downstream* of all that. Changing the
  multiplier in isolation can move the failure mode rather than fix it.
- Research-report rec #7 proposes
  `score = priority × max(0, relevance − floor)`. With `floor ≈ 0.4`,
  Boosted stops firing on hybrid scores below 0.4. This is the cleanest
  cut and is independent of the OHS calibration.

To pull this thread you want: an eval suite (the apparatus exists at
`tests/eval/`, run with `cargo nextest run --test eval_suite -p
talon-core --no-capture`), a labeled query set with gold rankings (the
Karpathy chef-vault gives plausible queries; gold labels need creating),
and patience to read agent-mode output across `floor ∈ {0.0, 0.2, 0.4,
0.6}` to see what breaks. Half-day of code, half-day of evals.

Pre-reading: §3.2 in the (now-superseded) plan-vs-implementation doc has
the deep-dive on the OHS pipeline shape — read it before tuning anything.

## Tombstone persistence (plan-vs-impl §2.3)

`crates/talon-core/src/sync/change_tracking.rs` has an in-memory
`TombstoneEntry` model; the SQL schema doesn't have a tombstones table.
`changes --since` returns deletions in the response struct, but they may
not survive a process restart. Spec §10.3 specified persistence.

Decision needed: do tombstones need to survive across runs? If yes, add a
`CREATE TABLE tombstones (path, deleted_at, last_indexed_at)` migration
and a small write path on file deletion. If no, document that "deleted"
in `changes --since` is a current-process view.

## Cross-build / npm release (plan-vs-impl §2.8)

`optionalDependencies` in `ts/package.json` are declared but
`.github/workflows/release.yml` doesn't run `cargo zigbuild` for the five
target triples. The npm meta-package would resolve to empty platform
subpackages on a release tag.

Out of scope per the user direction "we'll defer release flow, github
workflows, npm, pip etc etc." Listed for completeness.

## `meta --tag-counts` typed-as-int keys (dogfood §2.2)

Output of `talon --agent meta --tag-counts` includes entries like
`"4":1` — almost certainly a YAML integer (`tags: [4]`) coerced to a
string and counted. One-line filter in the tag aggregation pass at
`crates/talon-core/src/query/meta.rs:73-83` (skip non-string values).
Trivial, unblocked.

## `changes` ordering + default limit (dogfood §3.4)

`talon changes --since 7d` and `--since 30d` return the same 10 paths
because: results are limited to `DEFAULT_LIMIT = 10`, and the in-memory
ordering ends up alphabetic-first. With `lint = false` on
`daily/archive/private` the *lint* noise is fixed, but `changes` is a
separate path — it sorts by ISO 8601 string ASC (which is
chronologically correct now that timestamps are strings), but the cap
hits before the most-recent items appear. Two ways to fix:

- order by `indexed_at` DESC and keep the cap (most-recent-first slice)
- raise the default to a much higher cap (or `usize::MAX`) since the
  consumer is usually an agent doing a "what changed" pass

Recommend: order DESC and raise the default to ~100. Lives in
`crates/talon-core/src/query/changes.rs:146-153`.

## Frontmatter typed values (plan-vs-impl §2.5)

Spec §10.1 had `value_type ∈ {string, number, bool, date, list}` stored
alongside the value. Implementation stores everything as TEXT, with
parse-on-read at query time. Practical consequence: `--where date <
2026-04-01` parses both sides each comparison; `--where priority > 3`
may not work intuitively if the user thinks of priority as a number.

Decision needed: does numeric/date `--where` need to work the way users
expect? If yes, add `value_type` column and persist parsed type at
indexing time. If "TEXT-only is fine, document the limitation," ship
the documentation note.

## Link table shape (plan-vs-impl §2.6)

Spec §10.2 specified `(source_file_id, target_file_id_or_null,
target_text, link_type)` with `link_type ∈ {wikilink, markdown}`.
Implementation uses path-based join (`from_path, to_path, raw_target,
heading, alias`) and no `link_type` discrimination.

Decision needed: do we need to distinguish wikilinks from markdown
links for any future use case (lint differentiation, rendering, alt
flavours)? Path-based join is fine for Obsidian-native vaults; if Talon
is ever asked to handle non-Obsidian markdown vaults the missing
discriminator could matter.

## `lint.roots` default vs `[lint].ignore` (plan-vs-impl §2.4)

Spec specified a `lint.roots` field with default
`["index.md", "README.md", "_meta/index.md"]` to skip from orphan
checks. We shipped a more general `[lint].ignore` with no defaults.
Functionally equivalent if the user adds those globs to ignore, but the
out-of-box behaviour differs — a fresh vault's `README.md` would flag
as orphan today.

Easy follow-up: ship sensible default ignore globs in `init`'s emitted
config, OR special-case those paths in core. Not blocking.

## Per-subcommand help text (plan-vs-impl §5.2)

`talon search --help` returns the same global help as `talon --help`.
bpaf supports per-subcommand help groups; we haven't wired them. Minor
ergonomics, no blocking effect.

## `--scope all` vs `--scope-all` consistency

We picked `--scope-all` (subcommand-namespaced) per user direction. The
spec §6.4 used `--scope all` (treating "all" as a magic scope name).
There's no follow-up needed unless someone wants to align the spec text;
the implementation is clear.

---

# Open questions

## When to write a tombstones table?

See above. The choice frames whether `changes --since` is a durable
question or a within-process question.

## Should `mtime` be in `--agent` mode after all?

Today `mtime` is in canonical types only. The format is now date-only
(`"2026-04-25"`, ~10 chars vs. ~20 for RFC 3339), so the token cost of
adding it to agent mode is roughly halved from the original concern.
Argument for adding it: freshness is high-signal information, especially
for recall. Argument against: agents can re-derive freshness from
`recall`'s built-in `mtime` per excerpt. Decision deferred until we see
whether agents actually struggle without it.

## Should `--scope-all` also bypass the lint exclusion?

Currently `--scope-all` includes every scope in the *search* candidate
pool, but `lint_excluded` still applies to lint findings. So
`talon lint --scope-all` would still skip `daily/` because daily has
`lint = false` per scope config. Plausible interpretation either way:
"scope-all means I want everything everywhere" vs "scope-all is a search
flag, lint exclusions are independent." Not blocking; flag for explicit
decision when lint scope behaviour gets pulled again.

---

# Where to read the code

| Concern | File |
|---------|------|
| Scope filter | `crates/talon-core/src/config/scope_filter.rs` |
| Lint exclusion helper | `crates/talon-core/src/config.rs:200` |
| Lint config | `crates/talon-core/src/config.rs:255` |
| Scope CLI flags | `crates/talon-cli/src/cli/scope.rs` |
| Per-command scope wiring | `crates/talon-cli/src/command/{search,recall,changes,lint,meta,related}.rs` |
| Search filter step | `crates/talon-core/src/query/search.rs:148` (`apply_scope_filter`) |
| Lint post-filter | `crates/talon-core/src/query/lint.rs:24` |
| Related edge count | `crates/talon-core/src/query/related.rs:177-225` |
| mtime helpers | `crates/talon-core/src/query/mtime.rs` |
| Karpathy preset (with `lint = false` defaults) | `crates/talon-cli/src/config.rs:266-329` |
| Example user config | `examples/config.toml` (note: has uncommitted local edits) |
| Skill doc | `skill/SKILL.md` |
| Repo README | `README.md` |

---

# Test coverage

`just verify` is clean as of this doc. 539 tests pass. New coverage:

- 14 `ScopeFilter` unit tests (config-aware glob matching, mutual
  exclusivity, unknown names, resolved-set).
- 3 `format_iso8601` unit tests in `query/mtime.rs`.
- Two pre-existing prefix-matching integration tests in `lint`/`meta`
  converted to use real configs (`test_scope_filter_limits_orphan_findings`,
  `scope_only_filters_by_configured_scope`).

What's not tested:

- `lint_excluded` helper directly. Covered transitively by `query_lint`
  but a direct unit test would be cheap and worth adding next time
  someone touches the lint path.
- `RelatedResult.count` with multi-alias links. Build a test fixture
  that inserts the same link three times under different aliases and
  assert `count == 3`.
- Agent vs JSON gating for the new `scope` and `mtime` fields. The split
  is structural (different types) but a snapshot test confirming agent
  output stays compact would be insurance.

---

# Why these particular decisions

- **Option B over rank-only**: dogfood §3.1 surfaced `private/Lease Notes`
  leaking into a default search via lexical hits. Ranker-only semantics
  is purely advisory; agents in Mode B navigation expect "default search"
  to be a curated, safe view.
- **`lint = bool` not `lint: ["check1", "check2"]`**: keep simple per
  user direction. If per-check granularity becomes important, the field
  can be expanded to an enum (`All(bool)` vs `Checks(Vec<LintCheck>)`)
  without breaking existing configs (`bool` deserializes to `All(...)`).
- **Karpathy preset ships `lint = false` for daily/archive/private**:
  matches the dogfood-flagged noise sources directly. Users with
  different vault shapes override in their config.
- **`--scope-all` not `--all`**: keeps the scope-flag namespace clean.
  Reserves `--all` for any future general operation.
- **Agent types stay compact, canonical types get scope+mtime**: token
  efficiency. Agents reasoning about scope mix can use `meta.scope_set`
  to know what was searched without paying for per-result echo.

---

# Reproduction commands

If you're picking up this work cold:

```bash
# Confirm clean state
just verify

# See the new flags
cargo run -q -p talon-cli -- --help

# Run with the dogfood vault (if still around at /tmp/talon-dogfood-vault)
cargo run -q -p talon-cli -- search "fermented hot sauce" --scope-only wiki
cargo run -q -p talon-cli -- search "lease renewal" --scope private
cargo run -q -p talon-cli -- meta --tag-counts | jq '.entries | length'  # was 10, now ≈73
cargo run -q -p talon-cli -- related "wiki/Sauce Mothers.md"  # check `count` field
cargo run -q -p talon-cli -- lint                              # daily/* findings should be gone
```

For the boost-shouts thread when you pick it up:

```bash
cargo nextest run --test eval_suite -p talon-core --no-capture
```
