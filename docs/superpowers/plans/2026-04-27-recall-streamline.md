# Recall Streamline & Hermes Plugin Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Strip `talon recall` to `active_notes` + `linked_context` with `mtime` on notes and one-line headline excerpts; rewrite the Hermes plugin with synchronous prefetch (20s timeout), no exposed tools, and cache-safe empty returns.

**Architecture:** Rust side deletes three output sections (`frontmatter`, `recent_edits`, `fuzzy_anchors`), their scoring signals, and their CLI plumbing; adds `mtime` and headline truncation. Python plugin drops `queue_prefetch` for always-current synchronous `prefetch()` with `TimeoutExpired` caught silently.

**Tech Stack:** Rust workspace crates `talon-core` / `talon-cli`, SQLite `strftime` for date formatting (no chrono needed), Python 3.11+ pytest.

---

## File Map

### talon-core (modified)
- `crates/talon-core/src/query/output.rs` — `NoteExcerpt` gains `mtime: String`; `VaultRecall` loses 3 section vecs; `FrontmatterFact`, `EditedNote`, `FuzzyAnchor` deleted
- `crates/talon-core/src/query/input.rs` — `RecallInput` loses `since`, `recency_half_life_days`
- `crates/talon-core/src/query/recall_scoring.rs` — `EvidenceInputs` loses `frontmatter_match_indicator`; weights renormalized
- `crates/talon-core/src/query/recall/sections.rs` — remove 3 section builders; update `to_note_excerpts` (add `conn`, `mtime`, headline); add `to_headline` and `mtime_date` helpers
- `crates/talon-core/src/query/recall/mod.rs` — remove 3 section calls; update `EvidenceInputs` literal; update callers
- `crates/talon-core/src/query/recall/budget.rs` — 2-section only
- `crates/talon-core/src/query/recall/tests.rs` — fix dead imports and updated signatures

### talon-cli (modified)
- `crates/talon-cli/src/cli.rs` — remove `recency_half_life_days` from `RecallArgs`
- `crates/talon-cli/src/command/recall.rs` — remove `since`, `recency_half_life_days`; new defaults: budget→500, min_confidence→0.4
- `crates/talon-cli/src/output/recall.rs` — prompt-xml: add `mtime` attr + inline body; remove 3 dead sections; human: same
- `crates/talon-cli/src/mcp/tool/schema.rs` — remove `recencyHalfLifeDays`; update defaults
- `crates/talon-cli/src/mcp/tool/dispatch.rs` — `since: None` in recall `ResponseMeta`

### Python plugin (modified)
- `integrations/hermes-talon-recall/hermes_talon_recall/provider.py` — full rewrite
- `integrations/hermes-talon-recall/plugin.yaml` — remove `hooks:` block
- `integrations/hermes-talon-recall/pyproject.toml` — add pip entry point
- `integrations/hermes-talon-recall/tests/test_provider.py` — add timeout test; update sync_turn and config tests

---

## Task 1: Update core data types — `output.rs` and `input.rs`

**Files:**
- Modify: `crates/talon-core/src/query/output.rs`
- Modify: `crates/talon-core/src/query/input.rs`

This change will produce compile errors in every file that references the deleted types. That is expected and fine — later tasks fix each site. Work through them with `cargo check` rather than `cargo build` to stay fast.

- [ ] **Step 1: Add `mtime` to `NoteExcerpt` and strip `VaultRecall` to two sections**

In `output.rs`, replace the `NoteExcerpt` struct:

```rust
pub struct NoteExcerpt {
    pub vault_path: VaultPath,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub rank: u32,
    pub mtime: String, // "YYYY-MM-DD", empty when unavailable
}
```

Replace `VaultRecall`:

```rust
pub struct VaultRecall {
    pub active_notes: Vec<NoteExcerpt>,
    pub linked_context: Vec<LinkedNote>,
}
```

Delete the three struct definitions entirely:

```
pub struct FrontmatterFact { ... }
pub struct EditedNote { ... }
pub struct FuzzyAnchor { ... }
```

- [ ] **Step 2: Remove `since` and `recency_half_life_days` from `RecallInput`**

In `input.rs`, the new `RecallInput`:

```rust
pub struct RecallInput {
    pub message: String,
    pub prior_messages: Vec<String>,
    pub budget_tokens: u32,
    pub exclude: Vec<String>,
    pub scope: Vec<String>,
    pub scope_only: Vec<String>,
    pub format: RecallFormat,
    pub depth: u8,
    pub min_confidence: f64,
    pub fast: bool,
}
```

Remove the `since: Option<String>` and `recency_half_life_days: u8` fields. Also remove `default_since_7d` if it is only referenced by those fields (check with `cargo check`).

- [ ] **Step 3: Verify compile errors are only at expected sites**

```bash
cargo check -p talon-core 2>&1 | grep "^error" | head -30
```

Expected errors: references to `FrontmatterFact`, `EditedNote`, `FuzzyAnchor`, `.since`, `.recency_half_life_days`, `frontmatter_match_indicator`. Nothing else.

- [ ] **Step 4: Commit**

```bash
git add crates/talon-core/src/query/output.rs crates/talon-core/src/query/input.rs
git commit -m "refactor(core): strip VaultRecall to active_notes + linked_context, add mtime to NoteExcerpt"
```

---

## Task 2: Update evidence scoring — `recall_scoring.rs`

**Files:**
- Modify: `crates/talon-core/src/query/recall_scoring.rs`

- [ ] **Step 1: Write updated tests first**

Replace the three existing scoring tests (zero_result, rerank_skipped, all_signals_strong) in the `#[cfg(test)]` block at the bottom of `recall_scoring.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_approx(got: f64, want: f64, label: &str) {
        assert!(
            (got - want).abs() < 1e-9,
            "{label}: got {got:.10}, want {want:.10}"
        );
    }

    #[test]
    fn zero_result_returns_zero() {
        let inputs = EvidenceInputs {
            top_rerank_score: 0.0,
            top_lexical_indicator: 0.0,
            top_result_link_count: 0,
            days_since_top_result_modified: 0.0,
        };
        assert_approx(compute_evidence_score(&inputs), 0.0, "zero result");
    }

    #[test]
    fn rerank_only_signal() {
        // 0.50*0.8 + 0.20*0 + 0.20*0 + 0.10*exp(0) = 0.40 + 0.10 = 0.50
        let inputs = EvidenceInputs {
            top_rerank_score: 0.8,
            top_lexical_indicator: 0.0,
            top_result_link_count: 0,
            days_since_top_result_modified: 0.0,
        };
        let expected = 0.50 * 0.8 + 0.10 * (-0.0_f64 / 14.0).exp();
        assert_approx(compute_evidence_score(&inputs), expected, "rerank only");
    }

    #[test]
    fn all_signals_strong_returns_one() {
        // 0.50 + 0.20 + 0.20*(10/5 capped 1.0) + 0.10*exp(0) = 0.50+0.20+0.20+0.10 = 1.0
        let inputs = EvidenceInputs {
            top_rerank_score: 1.0,
            top_lexical_indicator: 1.0,
            top_result_link_count: 10,
            days_since_top_result_modified: 0.0,
        };
        assert_approx(compute_evidence_score(&inputs), 1.0, "all strong");
    }
}
```

- [ ] **Step 2: Run tests — expect compile failure (field doesn't exist yet)**

```bash
cargo test -p talon-core recall_scoring 2>&1 | tail -20
```

Expected: compile error referencing `frontmatter_match_indicator`.

- [ ] **Step 3: Update `EvidenceInputs` and `compute_evidence_score`**

Replace the struct:

```rust
pub struct EvidenceInputs {
    pub top_rerank_score: f64,
    pub top_lexical_indicator: f64,
    pub top_result_link_count: u32,
    pub days_since_top_result_modified: f64,
}
```

Replace the weight computation inside `compute_evidence_score` (keep the early-exit guard and graph_density local):

```rust
let graph_density = (f64::from(inputs.top_result_link_count) / 5.0).min(1.0);
let recency = (-inputs.days_since_top_result_modified / 14.0).exp();

0.50 * inputs.top_rerank_score.clamp(0.0, 1.0)
    + 0.20 * inputs.top_lexical_indicator.clamp(0.0, 1.0)
    + 0.20 * graph_density.clamp(0.0, 1.0)
    + 0.10 * recency.clamp(0.0, 1.0)
```

- [ ] **Step 4: Run tests — expect pass**

```bash
cargo test -p talon-core recall_scoring 2>&1 | tail -10
```

Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/talon-core/src/query/recall_scoring.rs
git commit -m "refactor(core): remove frontmatter signal from evidence scoring, renormalize weights 0.50/0.20/0.20/0.10"
```

---

## Task 3: Simplify sections — `sections.rs`

**Files:**
- Modify: `crates/talon-core/src/query/recall/sections.rs`

- [ ] **Step 1: Write unit tests for `to_headline` and `mtime_date` first**

Add at the bottom of `sections.rs` inside a `#[cfg(test)]` block (or extend the existing one if present):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use crate::indexing::migrations::run_migrations;

    #[test]
    fn headline_takes_first_nonempty_line() {
        assert_eq!(to_headline("line one\nline two"), "line one");
    }

    #[test]
    fn headline_skips_blank_lines() {
        assert_eq!(to_headline("\n\n  content  \n"), "content");
    }

    #[test]
    fn headline_truncates_long_line_at_sentence() {
        let s = "A ".repeat(40) + "end.";   // >80 chars, ends with period
        let result = to_headline(&s);
        assert!(result.ends_with('.'), "should end at sentence boundary");
        assert!(result.len() <= 120);
    }

    #[test]
    fn headline_hard_truncates_with_ellipsis_when_no_sentence() {
        let s = "x".repeat(200);
        let result = to_headline(&s);
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 121); // 117 ascii + ellipsis (3 bytes, 1 char)
    }

    #[test]
    fn mtime_date_returns_empty_for_unknown_path() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        assert_eq!(mtime_date(&conn, "does/not/exist.md"), "");
    }

    #[test]
    fn mtime_date_formats_unix_ms_as_yyyy_mm_dd() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        // 2026-04-15 00:00:00 UTC = 1776124800 seconds = 1776124800000 ms
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, frontmatter, \
             mtime_ms, size_bytes, hash, docid, active) \
             VALUES ('test.md', 'Test', '[]', '[]', '', '{}', 1776124800000, 0, 'h', 1, 1)",
            [],
        ).unwrap();
        assert_eq!(mtime_date(&conn, "test.md"), "2026-04-15");
    }
}
```

- [ ] **Step 2: Run tests — expect compile failure (functions not defined yet)**

```bash
cargo test -p talon-core to_headline 2>&1 | tail -10
cargo test -p talon-core mtime_date 2>&1 | tail -10
```

Expected: compile errors (functions don't exist).

- [ ] **Step 3: Add `to_headline` and `mtime_date` helpers**

Add near the top of `sections.rs` (before `to_note_excerpts`):

```rust
fn to_headline(snippet: &str) -> String {
    let first = snippet
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if first.len() <= 120 {
        return first.to_owned();
    }
    first[..120]
        .rfind(|c: char| c == '.' || c == '!' || c == '?')
        .map(|i| first[..=i].to_owned())
        .unwrap_or_else(|| format!("{}…", &first[..117]))
}

fn mtime_date(conn: &Connection, path: &str) -> String {
    conn.query_row(
        "SELECT strftime('%Y-%m-%d', mtime_ms / 1000, 'unixepoch') \
         FROM notes WHERE vault_path = ?1 AND active = 1",
        rusqlite::params![path],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .unwrap_or_default()
}
```

- [ ] **Step 4: Update `to_note_excerpts` to use both helpers**

Replace the function signature and body:

```rust
pub fn to_note_excerpts(conn: &Connection, pipeline_results: &[RawSearchResult]) -> Vec<NoteExcerpt> {
    pipeline_results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let vault_path = VaultPath::parse(&r.path).ok()?;
            Some(NoteExcerpt {
                vault_path,
                title: r.title.clone(),
                snippet: to_headline(&r.snippet),
                score: r.score,
                rank: (i + 1) as u32,
                mtime: mtime_date(conn, &r.path),
            })
        })
        .collect()
}
```

- [ ] **Step 5: Delete the three dead section builders**

Remove these functions entirely from `sections.rs`:
- `pub fn collect_frontmatter(...) -> Vec<FrontmatterFact>`
- `pub fn collect_recent_edits(...) -> Vec<EditedNote>`
- `pub fn collect_fuzzy_anchors(...) -> Vec<FuzzyAnchor>`
- `fn parse_since(...)` if it is only used by `collect_recent_edits`
- `fn default_since_7d(...)` if it is only used by `collect_recent_edits`

Check with `cargo check -p talon-core` that no other callers reference them.

- [ ] **Step 6: Run sections tests — expect pass**

```bash
cargo test -p talon-core to_headline mtime_date 2>&1 | tail -10
```

Expected: `test result: ok. 6 passed`.

- [ ] **Step 7: Commit**

```bash
git add crates/talon-core/src/query/recall/sections.rs
git commit -m "feat(core): add mtime_date + to_headline to NoteExcerpt; drop frontmatter/recent_edits/fuzzy_anchors section builders"
```

---

## Task 4: Simplify the recall pipeline — `mod.rs` and `budget.rs`

**Files:**
- Modify: `crates/talon-core/src/query/recall/mod.rs`
- Modify: `crates/talon-core/src/query/recall/budget.rs`
- Modify: `crates/talon-core/src/query/recall/tests.rs`

- [ ] **Step 1: Update `budget.rs` — two sections only**

Replace `estimate_payload_tokens`:

```rust
pub fn estimate_payload_tokens(
    active_notes: &[NoteExcerpt],
    linked_context: &[LinkedNote],
) -> usize {
    let active: usize = active_notes
        .iter()
        .map(|n| tokenx_rs::estimate_token_count(&n.snippet) + 10)
        .sum();
    let linked: usize = linked_context
        .iter()
        .map(|n| tokenx_rs::estimate_token_count(&n.title) + 8)
        .sum();
    active + linked
}
```

Replace `trim_to_budget`:

```rust
pub fn trim_to_budget(
    budget: usize,
    active_notes: &mut Vec<NoteExcerpt>,
    linked_context: &mut Vec<LinkedNote>,
    excluded_by_budget: &mut Vec<String>,
) {
    let budget_with_slack = budget + budget / 50;
    loop {
        if estimate_payload_tokens(active_notes, linked_context) <= budget_with_slack {
            break;
        }
        // linked_context trimmed first (lower priority)
        if let Some(dropped) = linked_context.pop() {
            excluded_by_budget.push(dropped.vault_path.as_str().to_owned());
        } else if let Some(dropped) = active_notes.pop() {
            excluded_by_budget.push(dropped.vault_path.as_str().to_owned());
        } else {
            break;
        }
    }
}
```

- [ ] **Step 2: Update `tests.rs` — fix dead imports and budget test**

At the top of `tests.rs`, remove `FrontmatterFact`, `EditedNote`, `FuzzyAnchor` from the import:

```rust
use crate::query::{LinkedNote, NoteExcerpt};
```

Replace the `budget_enforcement_populates_excluded_by_budget` test:

```rust
#[test]
fn budget_enforcement_populates_excluded_by_budget() {
    let make_note = |path: &str, rank: u32| NoteExcerpt {
        vault_path: VaultPath::parse(path).unwrap(),
        title: path.to_string(),
        snippet: "a".repeat(50),
        score: 1.0,
        rank,
        mtime: String::new(),
    };
    let mut active = vec![make_note("A.md", 1), make_note("B.md", 2)];
    let mut linked: Vec<LinkedNote> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();

    trim_to_budget(1, &mut active, &mut linked, &mut dropped);

    assert!(
        !dropped.is_empty(),
        "budget trimmer must populate excluded_by_budget"
    );
}
```

- [ ] **Step 3: Update `mod.rs` — remove 3 section calls and fix EvidenceInputs**

In `run_recall`:

1. Remove imports for `collect_frontmatter`, `collect_recent_edits`, `collect_fuzzy_anchors`.

2. Remove these lines:
```rust
let frontmatter_facts = collect_frontmatter(conn, &pipeline_results, &excluded_set);
let frontmatter_match_indicator = if frontmatter_facts.is_empty() { 0.0 } else { 1.0 };
let since_str = input.since.clone().unwrap_or_else(default_since_7d);
let active_paths: Vec<String> = pipeline_results.iter().map(|r| r.path.clone()).collect();
let recent_edits = collect_recent_edits(conn, &since_str, &active_paths, &excluded_set, input.recency_half_life_days);
let fuzzy_anchors = collect_fuzzy_anchors(conn, &query, top_rerank_score, &excluded_set);
```

3. Update `EvidenceInputs` literal — remove `frontmatter_match_indicator`:
```rust
let evidence_score = compute_evidence_score(&EvidenceInputs {
    top_rerank_score,
    top_lexical_indicator,
    top_result_link_count: top_link_count,
    days_since_top_result_modified: top_days,
});
```

4. Update `to_note_excerpts` call — add `conn`:
```rust
let mut active_notes = to_note_excerpts(conn, &pipeline_results);
```

5. Remove `frontmatter_facts_mut`, `recent_edits_mut`, `fuzzy_anchors_mut` variables.

6. Update `trim_to_budget` call:
```rust
trim_to_budget(
    input.budget_tokens as usize,
    &mut active_notes,
    &mut linked_notes_mut,
    &mut excluded_by_budget,
);
```

7. Update `VaultRecall` literal:
```rust
vault_recall: Some(VaultRecall {
    active_notes,
    linked_context: linked_notes_mut,
}),
```

- [ ] **Step 4: Run core tests — expect pass**

```bash
cargo test -p talon-core 2>&1 | tail -15
```

Expected: all tests pass. If there are fixture vault test failures, check that `RecallInput::default()` still compiles (it should, since the removed fields had defaults and the struct still derives `Default`).

- [ ] **Step 5: Check formatting**

```bash
just check
```

- [ ] **Step 6: Commit**

```bash
git add crates/talon-core/src/query/recall/
git commit -m "refactor(core): simplify recall pipeline to active_notes + linked_context; update budget and tests"
```

---

## Task 5: Update CLI plumbing — args, command, schema, dispatch

**Files:**
- Modify: `crates/talon-cli/src/cli.rs`
- Modify: `crates/talon-cli/src/command/recall.rs`
- Modify: `crates/talon-cli/src/mcp/tool/schema.rs`
- Modify: `crates/talon-cli/src/mcp/tool/dispatch.rs`

- [ ] **Step 1: Remove `recency_half_life_days` from `RecallArgs` in `cli.rs`**

In the `RecallArgs` struct, delete the field:
```rust
pub recency_half_life_days: Option<u8>,
```

In the recall argument parser block (around line 310–342), delete the parser entry for `--recency-half-life-days`.

- [ ] **Step 2: Update `RecallInput` construction in `command/recall.rs`**

Remove these two lines from the `RecallInput { ... }` literal:
```rust
since: args.since.clone(),
recency_half_life_days: args.recall.recency_half_life_days.unwrap_or(7),
```

Change the budget and confidence defaults:
```rust
budget_tokens: args.recall.budget_tokens.unwrap_or(500),
min_confidence: args.recall.min_confidence.unwrap_or(0.4),
```

- [ ] **Step 3: Update `schema.rs` — remove `recencyHalfLifeDays`, update defaults**

In the recall tool JSON schema properties, delete the `recencyHalfLifeDays` entry entirely.

Update the descriptions for `budgetTokens` and `minConfidence` to reflect the new defaults (500 and 0.4).

- [ ] **Step 4: Fix `dispatch.rs` — remove `since` from `RecallResponse` meta**

In `dispatch_recall`, change:
```rust
since: input.since.clone(),
```
to:
```rust
since: None,
```

- [ ] **Step 5: Build and verify**

```bash
cargo build -p talon-cli 2>&1 | grep "^error" | head -20
```

Expected: no errors.

- [ ] **Step 6: Run full test suite**

```bash
cargo test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 7: Check formatting**

```bash
just check
```

- [ ] **Step 8: Commit**

```bash
git add crates/talon-cli/src/cli.rs \
        crates/talon-cli/src/command/recall.rs \
        crates/talon-cli/src/mcp/tool/schema.rs \
        crates/talon-cli/src/mcp/tool/dispatch.rs
git commit -m "refactor(cli): remove recency_half_life_days, set budget default 500 and min_confidence default 0.4"
```

---

## Task 6: Update output formatters — `output/recall.rs`

**Files:**
- Modify: `crates/talon-cli/src/output/recall.rs`

- [ ] **Step 1: Update the prompt-xml formatter**

In `format_recall_prompt_xml`, the `<active_notes>` block currently writes a multi-line per-note body. Replace it with an inline single-line body and `mtime` attribute:

```rust
writeln!(w, "  <active_notes>")?;
for note in &recall.active_notes {
    writeln!(
        w,
        "    <note path=\"{}\" title=\"{}\" mtime=\"{}\" score=\"{:.4}\">{}</note>",
        xml_escape(note.vault_path.as_str()),
        xml_escape(&note.title),
        note.mtime,
        note.score,
        xml_escape(&note.snippet),
    )?;
}
writeln!(w, "  </active_notes>")?;
```

Delete the `<frontmatter>`, `<recent_edits>`, and `<fuzzy_anchors>` blocks entirely from `format_recall_prompt_xml`.

- [ ] **Step 2: Update the human-readable formatter**

In `format_recall_human`, remove the sections that print frontmatter, recent edits, and fuzzy anchors. Add `mtime` to the active notes display:

```rust
// active notes line — show mtime when available
let mtime_str = if note.mtime.is_empty() { String::new() } else { format!("  ({})", note.mtime) };
writeln!(w, "    [{}] {}{} ({:.3})", note.rank, note.vault_path, mtime_str, note.score)?;
```

- [ ] **Step 3: Build and smoke-test the formatter**

```bash
cargo build -p talon-cli 2>&1 | grep "^error"
```

If a talon index is available locally, run a quick manual check:
```bash
cargo run --bin talon -- recall "test query" --format prompt-xml 2>/dev/null | head -20
```

Expected output shape:
```xml
<vault_recall source="talon" vault="..." evidence_score="0.XXXX">
  <active_notes>
    <note path="..." title="..." mtime="YYYY-MM-DD" score="X.XXXX">One line headline.</note>
  </active_notes>
  <linked_context>
    <note path="..." title="..." relation="Outgoing" hops="1"/>
  </linked_context>
</vault_recall>
```

- [ ] **Step 4: Run full test suite and check**

```bash
cargo test 2>&1 | tail -10
just check
```

- [ ] **Step 5: Commit**

```bash
git add crates/talon-cli/src/output/recall.rs
git commit -m "feat(cli): prompt-xml adds mtime attr and inline headline; removes frontmatter/recent_edits/fuzzy_anchors sections"
```

---

## Task 7: Rewrite the Python plugin — `provider.py`

**Files:**
- Modify: `integrations/hermes-talon-recall/hermes_talon_recall/provider.py`

- [ ] **Step 1: Write the failing timeout test first**

Add this test to `tests/test_provider.py` (near the existing subprocess tests):

```python
def test_prefetch_timeout_returns_empty(monkeypatch):
    """When talon takes longer than 20s, prefetch returns '' without raising."""
    p = _make_provider(monkeypatch)

    with patch(
        "hermes_talon_recall.provider.subprocess.run",
        side_effect=subprocess.TimeoutExpired(cmd=["talon"], timeout=20),
    ):
        result = p.prefetch("slow query")

    assert result == ""
```

Also add an import at the top of the test file if not already present:
```python
import subprocess
```

- [ ] **Step 2: Run new test — expect fail**

```bash
cd integrations/hermes-talon-recall && python -m pytest tests/test_provider.py::test_prefetch_timeout_returns_empty -v
```

Expected: `FAILED` (provider doesn't catch `TimeoutExpired` yet).

- [ ] **Step 3: Rewrite `provider.py`**

Replace the entire file:

```python
"""TalonRecallProvider: Hermes MemoryProvider backed by `talon recall`.

Talon is recall-only and stateless per call. This plugin:
  - Implements prefetch() synchronously — always uses the current query.
  - Returns "" on timeout (20s), non-zero exit, empty output, or skipped response.
  - Buffers recent user turns via sync_turn() so --prior-message widens the query.
  - Never writes to the vault.
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
from collections import deque
from pathlib import Path
from typing import Any

from agent.memory_provider import MemoryProvider

logger = logging.getLogger(__name__)

_INSTALL_HINT = (
    "Install Talon: https://github.com/seanmozeik/talon  "
    "or set TALON_BIN to the absolute binary path."
)
_NO_RECALL = ""
_SKIPPED_PREFIX = '<vault_recall skipped="true"'
_TIMEOUT = 20  # seconds


class TalonRecallProvider(MemoryProvider):
    """Hermes MemoryProvider — vault-native context via talon recall --format prompt-xml."""

    @property
    def name(self) -> str:
        return "talon-recall"

    def __init__(self) -> None:
        self._binary: str | None = None
        self._vault_path: str | None = None
        self._budget_tokens: int = 500
        self._min_confidence: float = 0.4
        self._fast: bool = False
        self._prior_message_count: int = 2
        # Stores user message strings only — assistant content not needed for BM25 expansion.
        self._turn_history: deque[str] = deque(maxlen=8)

    # ── MemoryProvider ABC ────────────────────────────────────────────────────

    def is_available(self) -> bool:
        return self._resolve_binary() is not None

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        binary = self._resolve_binary()
        if binary is None:
            raise RuntimeError(f"talon binary not found. {_INSTALL_HINT}")
        self._binary = binary
        self._load_config(kwargs.get("hermes_home", ""))
        if vault_env := os.environ.get("TALON_VAULT"):
            self._vault_path = vault_env

    def system_prompt_block(self) -> str:
        return (
            "# Talon Vault\n"
            "Relevant vault notes are auto-injected as <vault_recall> before each turn. "
            "Use `talon read` or `talon search` via the shell to look up notes directly."
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        """Run talon recall synchronously. Returns vault_recall XML or empty string.

        Empty string is returned (cache-safe, no injection) on:
          - 20s timeout
          - non-zero exit code
          - empty stdout
          - skipped=true confidence-gate response
        """
        if self._binary is None:
            return _NO_RECALL
        try:
            result = subprocess.run(
                self._build_command(query),
                capture_output=True,
                text=True,
                timeout=_TIMEOUT,
                env=self._build_env(),
            )
        except subprocess.TimeoutExpired:
            logger.debug("talon-recall: prefetch timed out after %ds", _TIMEOUT)
            return _NO_RECALL
        except Exception as exc:
            logger.warning("talon-recall: subprocess error: %s", exc)
            return _NO_RECALL

        if result.returncode != 0:
            logger.warning(
                "talon-recall: exited %d: %s", result.returncode, result.stderr[:200]
            )
            return _NO_RECALL

        stdout = result.stdout.strip()
        if not stdout or stdout.startswith(_SKIPPED_PREFIX):
            return _NO_RECALL

        return stdout

    def sync_turn(
        self, user_content: str, assistant_content: str, *, session_id: str = ""
    ) -> None:
        """Buffer user message for --prior-message expansion on the next prefetch."""
        if user_content.strip():
            self._turn_history.append(user_content)

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return []

    def shutdown(self) -> None:
        pass

    # ── config ────────────────────────────────────────────────────────────────

    def get_config_schema(self) -> list[dict[str, Any]]:
        return [
            {
                "key": "vault_path",
                "description": "Absolute path to your Obsidian vault directory",
                "required": False,
                "env_var": "TALON_VAULT",
            },
            {
                "key": "budget_tokens",
                "description": "Token budget for the recall context block (default 500)",
                "default": 500,
            },
            {
                "key": "min_confidence",
                "description": "Minimum evidence score 0.0–1.0 (default 0.4)",
                "default": 0.4,
            },
            {
                "key": "fast",
                "description": "Skip LLM expansion and reranking (default false)",
                "default": False,
            },
            {
                "key": "prior_message_count",
                "description": "Recent user turns fed via --prior-message (default 2)",
                "default": 2,
            },
        ]

    def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
        Path(hermes_home).joinpath("talon-recall.json").write_text(
            json.dumps(values, indent=2)
        )

    # ── private helpers ───────────────────────────────────────────────────────

    def _resolve_binary(self) -> str | None:
        if bin_env := os.environ.get("TALON_BIN"):
            if os.path.isfile(bin_env) and os.access(bin_env, os.X_OK):
                return bin_env
            return None
        return shutil.which("talon")

    def _load_config(self, hermes_home: str) -> None:
        if not hermes_home:
            return
        config_path = Path(hermes_home) / "talon-recall.json"
        if not config_path.exists():
            return
        try:
            cfg: dict[str, Any] = json.loads(config_path.read_text())
        except Exception as exc:
            logger.warning("talon-recall: failed to read config: %s", exc)
            return
        self._vault_path = cfg.get("vault_path") or self._vault_path
        self._budget_tokens = int(cfg.get("budget_tokens", self._budget_tokens))
        self._min_confidence = float(cfg.get("min_confidence", self._min_confidence))
        self._fast = bool(cfg.get("fast", self._fast))
        self._prior_message_count = int(
            cfg.get("prior_message_count", self._prior_message_count)
        )

    def _build_command(self, query: str) -> list[str]:
        assert self._binary is not None
        cmd = [
            self._binary, "recall", query,
            "--format", "prompt-xml",
            "--budget-tokens", str(self._budget_tokens),
            "--min-confidence", str(self._min_confidence),
        ]
        for msg in list(self._turn_history)[-self._prior_message_count:]:
            cmd += ["--prior-message", msg]
        if self._fast:
            cmd.append("--fast")
        return cmd

    def _build_env(self) -> dict[str, str]:
        env = os.environ.copy()
        if self._vault_path:
            env["TALON_VAULT"] = self._vault_path
        return env


def register(ctx) -> None:
    ctx.register_memory_provider(TalonRecallProvider())
```

- [ ] **Step 4: Run all Python tests**

```bash
cd integrations/hermes-talon-recall && python -m pytest tests/ -v
```

Expected: all tests pass. If `test_prior_messages_passed_to_talon` fails, it's because the old test called `sync_turn` with tuples but the new `_turn_history` stores only strings. Fix the test call:

```python
# old:
p.sync_turn("What is a knowledge graph?", "It's a graph…")
# new: same call signature — sync_turn still accepts (user, assistant) — only user is stored
```

No change needed to the test call itself — `sync_turn` signature is unchanged. The test assertions check `--prior-message` presence, which still works.

If `test_save_and_load_config` fails because it sets `recency_half_life_days`, update that test:

```python
def test_save_and_load_config(tmp_path, monkeypatch):
    hermes_home = str(tmp_path)
    values = {
        "vault_path": "/my/vault",
        "budget_tokens": 1500,
        "min_confidence": 0.5,
        "fast": True,
        "prior_message_count": 3,
    }
    writer = TalonRecallProvider()
    writer.save_config(values, hermes_home)

    reader = TalonRecallProvider()
    monkeypatch.setattr("hermes_talon_recall.provider.shutil.which", lambda _: "/usr/bin/talon")
    reader.initialize(session_id="x", hermes_home=hermes_home)

    assert reader._vault_path == "/my/vault"
    assert reader._budget_tokens == 1500
    assert reader._min_confidence == 0.5
    assert reader._fast is True
    assert reader._prior_message_count == 3
```

- [ ] **Step 5: Commit**

```bash
cd integrations/hermes-talon-recall
git add hermes_talon_recall/provider.py tests/test_provider.py
git commit -m "feat(plugin): rewrite TalonRecallProvider — synchronous prefetch, 20s timeout, no tools"
```

---

## Task 8: Update `plugin.yaml` and add pip entry point

**Files:**
- Modify: `integrations/hermes-talon-recall/plugin.yaml`
- Modify: `integrations/hermes-talon-recall/pyproject.toml`

- [ ] **Step 1: Clean up `plugin.yaml`**

Replace the file content:

```yaml
name: talon-recall
version: 0.2.0
description: "Vault-native context recall for Hermes Agent — injects relevant Obsidian notes as <vault_recall> before each turn."
author: seanmozeik
homepage: https://github.com/seanmozeik/talon
```

No `hooks:` block — `queue_prefetch` is not implemented.

- [ ] **Step 2: Add pip entry point to `pyproject.toml`**

Add an `[project.entry-points]` section:

```toml
[project.entry-points."hermes_agent.plugins"]
talon-recall = "hermes_talon_recall"
```

The full updated `pyproject.toml`:

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "hermes-talon-recall"
version = "0.2.0"
description = "Hermes Agent Memory Provider: vault-native context recall via talon recall"
license = { text = "MIT" }
authors = [{ name = "seanmozeik", email = "xtremium2002@gmail.com" }]
readme = "README.md"
requires-python = ">=3.11"
dependencies = []

[project.urls]
Homepage = "https://github.com/seanmozeik/talon"

[project.entry-points."hermes_agent.plugins"]
talon-recall = "hermes_talon_recall"

[tool.pytest.ini_options]
testpaths = ["tests"]
```

- [ ] **Step 3: Verify `__init__.py` exports `register`**

Check `integrations/hermes-talon-recall/hermes_talon_recall/__init__.py`. It must re-export `register` so the entry point resolves:

```python
from hermes_talon_recall.provider import register  # noqa: F401
```

If this export is missing, add it.

- [ ] **Step 4: Run tests one final time**

```bash
cd integrations/hermes-talon-recall && python -m pytest tests/ -v
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
cd integrations/hermes-talon-recall
git add plugin.yaml pyproject.toml hermes_talon_recall/__init__.py
git commit -m "feat(plugin): bump to 0.2.0, add hermes_agent.plugins entry point, clean plugin.yaml"
```

---

## Self-Review

**Spec coverage:**

| Requirement | Task |
|---|---|
| Drop `recent_edits`, `fuzzy_anchors`, `frontmatter` sections | Tasks 1, 3, 4, 6 |
| Add `mtime="YYYY-MM-DD"` to `<note>` in prompt-xml | Tasks 3, 6 |
| One-line headline excerpt (not multi-line snippet) | Task 3 |
| Budget default 500 tokens | Task 5 |
| `min_confidence` default 0.4 | Task 5 |
| Synchronous `prefetch()` with 20s timeout | Task 7 |
| Return `""` on timeout (cache-safe) | Task 7 |
| Return `""` on zero/skipped results | Task 7 |
| No `queue_prefetch` | Task 7 |
| No tools exposed | Task 7 |
| Tiny stable `system_prompt_block` | Task 7 |
| `sync_turn` buffers user messages only | Task 7 |
| pip entry point | Task 8 |
| `plugin.yaml` no hooks block | Task 8 |

**No gaps found.**

**Type consistency check:** `NoteExcerpt.mtime: String` defined in Task 1, populated in Task 3 (`to_note_excerpts`), rendered in Task 6 (formatter), tested in Tasks 3 and 4. `trim_to_budget` new signature defined and called consistently in Tasks 4 and 4. `EvidenceInputs` without `frontmatter_match_indicator` defined in Task 2, constructed in Task 4. All consistent.
