# Talon Refactoring — Line-Count Compliance

**Goal:** Every `.rs` file in `crates/` must be ≤ 350 lines (`just rust-line-counts` passes).  
**Rule:** Move a file to its new location first, then split it if it still exceeds 350 lines — both steps in the same task.  
**No parallel subagents.** One at a time. Verify `cargo check` passes after each task.  
**Subagents can use haiku for mechanical tasks.**  

---

## Baseline (22 violations before any work)

```
1642  crates/talon-core/src/tool.rs               ← Task 1 (IN PROGRESS)
1388  crates/talon-core/tests/fixture_vault.rs     ← Task 14
 953  crates/talon-core/src/indexer/upsert.rs      ← Task 3
 733  crates/talon-core/src/frontmatter.rs         ← Task 2
 717  crates/talon-cli/src/command.rs              ← Task 11
 704  crates/talon-cli/src/output.rs               ← Task 12
 658  crates/talon-core/src/query/recall.rs        ← Task 4
 649  crates/talon-cli/src/mcp/tool.rs             ← Task 13
 606  crates/talon-core/src/chunker.rs             ← Task 2
 600  crates/talon-core/src/text.rs                ← Task 2
 556  crates/talon-core/src/search/hybrid_pipeline.rs ← Task 5
 551  crates/talon-core/src/embed/runner.rs        ← Task 6
 477  crates/talon-core/src/change_tracking.rs     ← Task 3
 476  crates/talon-core/tests/search_integration.rs ← Task 15
 445  crates/talon-core/src/migrations.rs          ← Task 3
 440  crates/talon-core/tests/ranking_regression.rs ← Task 16
 429  crates/talon-cli/tests/cli.rs                ← Task 17
 418  crates/talon-core/src/links.rs               ← Task 7
 411  crates/talon-core/tests/eval/mod.rs          ← Task 18
 409  crates/talon-core/src/search/fuse.rs         ← Task 8
 407  crates/talon-core/src/query/meta.rs          ← Task 9
 374  crates/talon-core/src/search/match_text.rs   ← Task 10
```

---

## Tasks

### Task 1 — Split tool.rs into domain modules [x]

**Status:** New files created but existing files not wired up. `tool.rs` still exists.

#### What was already created (correct, do not recreate):
- `crates/talon-core/src/contracts/mod.rs` (321 lines)
- `crates/talon-core/src/contracts/accessors.rs` (99 lines)
- `crates/talon-core/src/contracts/envelope_tests.rs` (326 lines)
- `crates/talon-core/src/indexing/mod.rs` (10 lines)
- `crates/talon-core/src/indexing/input.rs` (58 lines)
- `crates/talon-core/src/indexing/output.rs` (144 lines)
- `crates/talon-core/src/query/input.rs` (169 lines)
- `crates/talon-core/src/query/output.rs` (218 lines)
- `crates/talon-core/src/search/input.rs` (~182 lines)
- `crates/talon-core/src/search/output.rs` (120 lines)

#### What still needs to happen:

**1. Update `crates/talon-core/src/search/mod.rs`**  
Add after the existing `pub mod vector;` line:
```rust
pub mod input;
pub mod output;
```
Add re-exports (after existing ones):
```rust
pub use input::{Direction, FrontmatterFilter, SearchInput, SearchMode, WhereClause, WhereOperator};
pub use output::{AnchorKind, MatchAnchor, MatchKind, SearchResponse, SearchResult};
```

**2. Update `crates/talon-core/src/query/mod.rs`**  
Add:
```rust
pub mod input;
pub mod output;
```
Add re-exports:
```rust
pub use input::{ChangesInput, MetaInput, ReadInput, RecallFormat, RecallInput};
pub use output::{
    ChangeEntry, ChangesResponse, EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote,
    MetaEntry, MetaResponse, NoteExcerpt, ReadResponse, ReadResult, RecallResponse,
    VaultRecall,
};
```
Also add related type re-exports (after query/related.rs is updated):
```rust
pub use related::{RelatedInput, RelatedResponse, RelatedResult, RelationKind};
```

**3. Update `crates/talon-core/src/query/related.rs`**  
Add type definitions at the top (before the existing `find_related` function).  
Copy from original `tool.rs` (available via `git show HEAD:crates/talon-core/src/tool.rs`):
- `RelationKind` enum
- `RelatedResult` struct
- `RelatedInput` struct
- `RelatedResponse` struct
These types reference `VaultPath` (from `crate::contracts`) and `Direction` (from `crate::search`).

**4. Update `crates/talon-core/src/lib.rs`**
- Remove `pub mod tool;`
- Add `pub mod contracts;` and `pub mod indexing;`
- Replace the entire `pub use tool::{...}` block (lines 73–82) with:
```rust
pub use contracts::{
    ContainerPath, ErrorEnvelope, PositiveCount, ResponseMeta, TalonEnvelope, TalonInput,
    TalonResponseData, TalonResponseTrait, VaultPath,
};
pub use indexing::{
    IndexStats, LintCheck, LintFinding, LintInput, LintResponse, ScopeReport, StatusInput,
    StatusResponse, StatusState, SyncInput, SyncResponse, SyncStatus,
};
pub use query::{
    ChangeEntry, ChangesInput, ChangesResponse, EditedNote, FrontmatterFact, FuzzyAnchor,
    LinkedNote, MetaEntry, MetaInput, MetaResponse, NoteExcerpt, ReadInput, ReadResponse,
    ReadResult, RecallFormat, RecallInput, RecallResponse, RelatedInput, RelatedResponse,
    RelatedResult, RelationKind, VaultRecall,
};
pub use search::{
    AnchorKind, Direction, FrontmatterFilter, MatchAnchor, MatchKind, SearchInput, SearchMode,
    SearchResponse, SearchResult, WhereClause, WhereOperator,
};
```
Note: `FrontmatterValue` and `FrontmatterValueType` stay exported from `frontmatter::` (already in lib.rs line 40–42) — do NOT add them to the search re-export.

**5. Delete `crates/talon-core/src/tool.rs`**

**6. Verify:** `cargo check --workspace --all-targets --all-features --locked` must pass, then `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`

#### Key design decisions (do not change):
- `FrontmatterValue` + `FrontmatterValueType`: NOT redefined in search/input.rs. Instead, `search/input.rs` does `pub use crate::frontmatter::{FrontmatterValue, FrontmatterValueType};`. lib.rs continues exporting them from `frontmatter::`.
- `PositiveCount` inner field: **private**. `pub(crate) const fn from_const(value: u16) -> Self` added for Default impls in search/input.rs and query/input.rs.
- `contracts/` is a directory (mod.rs + accessors.rs + envelope_tests.rs), not a single file.

---

### Task 2 — Create text/ module: move + split chunker.rs, frontmatter.rs, text.rs [x]

Create `crates/talon-core/src/text/` directory.  
Move and split:
- `chunker.rs` (606 lines) → `text/chunker.rs` + split to ≤350
- `frontmatter.rs` (733 lines) → `text/frontmatter.rs` + split to ≤350
- `text.rs` (600 lines) → `text/processing.rs` + split to ≤350

Create `text/mod.rs` re-exporting everything.  
Update `lib.rs`: `pub mod text;` already exists — since text/ directory replaces text.rs, update re-exports to pull from `text::` (same names, same API).  
`cargo check` must pass.

---

### Task 3 — Create indexing/ module: move + split change_tracking.rs, migrations.rs, indexer/upsert.rs [x]

`crates/talon-core/src/indexing/` already exists (created in Task 1 with input.rs/output.rs).  
Move and split into that directory:
- `change_tracking.rs` (477 lines) → `indexing/change_tracking.rs` + split if needed
- `migrations.rs` (445 lines) → `indexing/migrations.rs` + split if needed
- `indexer/upsert.rs` (953 lines) → `indexing/upsert/` subdir + split to ≤350

Update `indexing/mod.rs` to re-export from the new files.  
Update `lib.rs`: replace `pub mod change_tracking;` + `pub mod migrations;` + `pub mod indexer;` with references through `indexing`.  
`cargo check` must pass.

---

### Task 4 — Move + split query/recall.rs [x]

Move `crates/talon-core/src/query/recall.rs` (658 lines) into `query/recall/` subdirectory.  
Split to ≤350 lines per file.  
Update `query/mod.rs` re-exports.  
`cargo check` must pass.

---

### Task 5 — Split search/hybrid_pipeline.rs (556 lines) [x]

Split within `search/`.  
Update `search/mod.rs`.  
`cargo check` must pass.

---

### Task 6 — Split embed/runner.rs (551 lines) [x]

Split within `embed/`.  
Update `embed/mod.rs`.  
`cargo check` must pass.

---

### Task 7 — Split links.rs (418 lines) [x]

Split into submodules within talon-core/src/.  
Update `lib.rs` re-exports.  
`cargo check` must pass.

---

### Task 8 — Split search/fuse.rs (409 lines) [x]

Split within `search/`.  
Update `search/mod.rs`.  
`cargo check` must pass.

---

### Task 9 — Split query/meta.rs (407 lines) [x]

Split within `query/`.  
Update `query/mod.rs`.  
`cargo check` must pass.

---

### Task 10 — Split search/match_text.rs (374 lines) [x]

Split within `search/`.  
Update `search/mod.rs`.  
`cargo check` must pass.

---

### Task 11 — Split CLI command.rs into command/ subdir (717 lines) [x]

Split `crates/talon-cli/src/command.rs` into `command/` subdirectory, one file per subcommand.  
Update `talon-cli/src/lib.rs` or wherever command is declared.  
`cargo check` must pass.

---

### Task 12 — Split CLI output.rs into output/ subdir (704 lines) [x]

Split `crates/talon-cli/src/output.rs` into `output/` subdirectory.  
`cargo check` must pass.

---

### Task 13 — Split CLI mcp/tool.rs (649 lines) [x]

Split `crates/talon-cli/src/mcp/tool.rs` within `mcp/`.  
Update `mcp/mod.rs`.  
`cargo check` must pass.

---

### Task 14 — Split tests/fixture_vault.rs (1388 lines) [ ]

Split `crates/talon-core/tests/fixture_vault.rs` into focused test helper modules.  
`cargo check --all-targets` must pass.

---

### Task 15 — Split tests/search_integration.rs (476 lines) [ ]

Split `crates/talon-core/tests/search_integration.rs`.  
`cargo check --all-targets` must pass.

---

### Task 16 — Split tests/ranking_regression.rs (440 lines) [ ]

Split `crates/talon-core/tests/ranking_regression.rs`.  
`cargo check --all-targets` must pass.

---

### Task 17 — Split talon-cli/tests/cli.rs (429 lines) [ ]

Split `crates/talon-cli/tests/cli.rs`.  
`cargo check --all-targets` must pass.

---

### Task 18 — Split tests/eval/mod.rs (411 lines) [ ]

Split `crates/talon-core/tests/eval/mod.rs` within `tests/eval/`.  
`cargo check --all-targets` must pass.

---

### Task 19 — Final verification [ ]

```
just check    # fmt + cargo check + clippy + rust-line-counts — must be zero violations
cargo nextest run --workspace --all-targets --locked
```

Both must pass clean.

---

## How to start a new session

Read this file, then run:
```
just rust-line-counts 2>&1 | head -5   # see remaining violations
cargo check --workspace --all-targets --all-features --locked 2>&1 | tail -3
git status --short                       # see what's staged/untracked
```

Task 1 is in progress with new files created but existing files not yet wired. Start there.
