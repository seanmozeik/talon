# CLI Migration: bpaf → clap derive

**Status:** Draft
**Created:** 2026-04-28
**Owner:** @yolo

---

## Objectives

1. **Replace `bpaf` with `clap` derive** for argument parsing in `talon-cli`.
2. **Improve CLI UX** — per-command help, grouped flags, colored errors, shell completions.
3. **Reduce boilerplate** — eliminate 9 custom flag enum types, manual parse closures.
4. **Zero behavioral change** — same flags, same positions, same exit codes. Users should not notice a difference in what works, only in how help looks and errors read.

---

## Current State

### Files using bpaf

| File | Lines | Purpose |
|---|---|---|
| `crates/talon-cli/src/cli.rs` | ~290 | Flat `CliArgs` struct + all parser combinators |
| `crates/talon-cli/src/cli/scope.rs` | ~45 | Shared `--scope` flags |

### Architecture today

```
main() → run() → cli::parse_or_exit() → CliArgs (flat)
                                    → command::run(&CliArgs)
                                        → dispatch on args.positionals[0]
                                            → search::emit(args, rest)
                                            → read::emit(args, rest)
                                            → ...
```

- One flat `CliArgs` with ~30 fields covering all commands.
- Commands identified by first positional string (`"search"`, `"read"`, etc.).
- Remaining positionals joined into query/path strings per command.
- 9 custom flag enums via `flag_type!` macro: `McpFlag`, `SkillFlag`, `VersionFlag`, `AgentFlag`, `JsonFlag`, `RawFlag`, `FastFlag`, `ForceFlag`, `VerboseFlag`, `AnchorsFlag`.
- Manual enum parsing: `parse_search_mode()`, `parse_direction()` with match arms.
- Scope flags in separate module, shared across query commands.

### What bpaf does well

- Fast compile times (no proc macros on the main parser).
- Combinator API is composable and testable.
- Already has shell completion support (`bpaf_cacom`).

### What bpaf does poorly for this project

- No help heading groups — flat flag list in `--help`.
- No subcommand structure — all flags available everywhere.
- Manual enum parsing = runtime errors, not parse-time validation.
- Custom flag enums are boilerplate with no semantic value.
- Help template is hardcoded — limited customization.
- Error messages lack suggestions ("did you mean?").

---

## Target Architecture

```
main() → run() → cli::parse_or_exit() → Cli (root struct)
                                    → Commands enum (subcommands)
                                        → Commands::Search(SearchArgs)
                                        → Commands::Read(ReadArgs)
                                        → ...
```

### Structured per-command args

Each command gets its own `#[derive(Args)]` struct. Global flags live on the root `Cli` struct.

```rust
#[derive(Parser)]
#[command(name = "talon", about = "Obsidian vault search, indexing, and MCP server.")]
#[command(header = "Commands: init, sync, status, search, read, related, meta, changes, lint, recall.")]
struct Cli {
    // Global flags (available before any subcommand)
    #[arg(long)]
    mcp: bool,

    #[arg(long)]
    skill: bool,

    #[arg(short, long)]
    version: bool,

    #[arg(long)]
    agent: bool,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    raw: bool,

    #[arg(long)]
    fast: bool,

    #[arg(long)]
    force: bool,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Search(SearchArgs),
    Read(ReadArgs),
    Sync(SyncArgs),
    Related(RelatedArgs),
    Meta(MetaArgs),
    Changes(ChangesArgs),
    Lint(LintArgs),
    Recall(RecallArgs),
    Status,
    Init,
}
```

### Flag types eliminated

All 10 flag enums (`McpFlag`, `SkillFlag`, etc.) become `bool` fields. The `flag_type!` macro is removed entirely. Call sites change from `args.mcp.enabled()` to `args.mcp`.

### Enums validated at parse time

```rust
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Hybrid,
    Semantic,
    Fulltext,
    Title,
}

// In SearchArgs:
#[arg(long, value_enum, ignore_case = true)]
mode: Option<SearchMode>,
```

Same pattern for `Direction`. Manual `parse_search_mode()` and `parse_direction()` closures are removed.

---

## File Changes

### New files

| File | Purpose |
|---|---|
| `crates/talon-cli/src/cli/mod.rs` | Root `Cli` struct + `Commands` enum entry point |
| `crates/talon-cli/src/cli/global_args.rs` | Shared global flags (`--mcp`, `--agent`, etc.) as `#[derive(Args)]` |
| `crates/talon-cli/src/cli/search_args.rs` | `SearchArgs` struct |
| `crates/talon-cli/src/cli/read_args.rs` | `ReadArgs` struct |
| `crates/talon-cli/src/cli/sync_args.rs` | `SyncArgs` struct |
| `crates/talon-cli/src/cli/related_args.rs` | `RelatedArgs` struct |
| `crates/talon-cli/src/cli/meta_args.rs` | `MetaArgs` struct |
| `crates/talon-cli/src/cli/changes_args.rs` | `ChangesArgs` struct |
| `crates/talon-cli/src/cli/lint_args.rs` | `LintArgs` struct |
| `crates/talon-cli/src/cli/recall_args.rs` | `RecallArgs` struct |
| `crates/talon-cli/src/cli/status_args.rs` | `StatusArgs` struct (empty or minimal) |
| `crates/talon-cli/src/cli/init_args.rs` | `InitArgs` struct (empty or minimal) |

### Deleted files

- `crates/talon-cli/src/cli.rs` — replaced by new structure above
- `crates/talon-cli/src/cli/scope.rs` — scope flags inlined into query command structs via `#[command(flatten)]` or repeated per-command

### Modified files

| File | Change |
|---|---|
| `crates/talon-cli/Cargo.toml` | Replace `bpaf` with `clap = { version = "4", features = ["derive"] }` |
| `crates/talon-cli/src/command/mod.rs` | Dispatch on `Commands` enum instead of string matching positionals |
| `crates/talon-cli/src/command/search.rs` | Accept `&SearchArgs` instead of `&CliArgs + rest` |
| `crates/talon-cli/src/command/read.rs` | Accept `&ReadArgs` instead of `&CliArgs + rest` |
| `crates/talon-cli/src/command/sync.rs` | Accept typed args |
| `crates/talon-cli/src/command/related.rs` | Accept typed args |
| `crates/talon-cli/src/command/meta.rs` | Accept typed args |
| `crates/talon-cli/src/command/changes.rs` | Accept typed args |
| `crates/talon-cli/src/command/lint.rs` | Accept typed args + rest for query |
| `crates/talon-cli/src/command/recall.rs` | Accept typed args |
| `crates/talon-cli/src/lib.rs` | Update `run()` to work with new CLI structure |
| `Cargo.toml` (workspace) | Replace `bpaf` dep with `clap` |

### Unchanged files

- `crates/talon-cli/src/cli/where_clause.rs` — data parsing, not CLI parsing
- All MCP modules — use `TalonInput` from `talon-core`, not CLI args
- All output modules — work on response types, not CLI args
- `talon-core` crate — no changes

---

## Shared Scope Flags Strategy

Scope flags (`--scope`, `--scope-only`, `--scope-all`) are shared across 6 commands: `search`, `read`, `related`, `meta`, `changes`, `recall`. Three options:

### Option A: Flatten into each command (duplication)

```rust
#[derive(Args)]
struct SearchArgs {
    query: Vec<String>,
    #[command(flatten)]
    scope: SharedScopeArgs,
    // ... other fields
}
```

**Pros:** Clean struct per command, no cross-cutting complexity.
**Cons:** 6 copies of the same 3 fields.

### Option B: Separate module, shared type reference

Define `SharedScopeArgs` in one file, import and flatten into each command struct.

**Pros:** Single source of truth for scope flags.
**Cons:** Slightly more file organization overhead.

### Option C: Global scope flags on root Cli

Make them available everywhere like `--verbose`.

**Cons:** Pollutes global help with scope flags that only matter for 6 commands. Not ideal UX.

**Decision: Option B.** Define in `cli/scope.rs`, flatten into each query command struct. Keeps scope as a first-class concept without duplicating field definitions.

---

## Help Grouping Plan

Each command struct uses `next_help_heading` to group related flags:

### SearchArgs example

```rust
#[derive(Args)]
struct SearchArgs {
    /// Search query (space-separated words).
    query: Vec<String>,

    #[command(next_help_heading = "SEARCH MODE")]
    #[arg(long, value_enum)]
    mode: Option<SearchMode>,

    #[arg(short, long)]
    limit: Option<u16>,

    #[arg(long)]
    candidate_limit: Option<u16>,

    #[arg(long)]
    intent: Option<String>,

    #[command(next_help_heading = "SCOPE")]
    #[command(flatten)]
    scope: SharedScopeArgs,

    #[command(next_help_heading = "FILTERS")]
    #[arg(long)]
    where_: Vec<String>,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    anchors: bool,

    #[command(next_help_heading = "OUTPUT")]
    #[arg(long)]
    fast: bool,
}
```

### Help headings per command

| Command | Headings |
|---|---|
| `search` | SEARCH MODE, SCOPE, FILTERS, OUTPUT |
| `read` | POSITION, FORMAT, OUTPUT |
| `sync` | OPTIONS, OUTPUT |
| `related` | TRAVERSAL, SCOPE, OUTPUT |
| `meta` | QUERY, SCOPE, FILTERS, OUTPUT |
| `changes` | TIME RANGE, SCOPE, OUTPUT |
| `lint` | OPTIONS, OUTPUT |
| `recall` | CONTEXT, SCOPE, FILTERS, OUTPUT |

---

## Error Handling Changes

### Current (bpaf)

```
error: unexpected value 'invalid_mode' found during parsing of '--mode'
```

### Target (clap)

```
error: invalid value 'invalid_mode' for '--mode <MODE>'
  [possible values: hybrid, semantic, fulltext, title]

For more information, try '--help'.
```

clap auto-generates "did you mean" suggestions for typos. ValueEnum validation happens at parse time with clear error messages listing valid options.

---

## Additional UX Features (Phase 2)

These are not required for the initial migration but should be considered:

### Shell completions

Add `clap_complete` dependency. Generate completions for bash, zsh, fish, powershell.

```toml
# Cargo.toml
clap = { version = "4", features = ["derive", "cargo"] }
clap_complete = "4"
```

```rust
// In a separate binary or build script
use clap_complete::{generate, shells::*};

fn generate_completions() {
    let mut cmd = Cli::command();
    generate(Bash, &mut cmd, "talon", &mut std::io::stdout());
}
```

### Man pages

```toml
clap_mangen = "0.2"
```

Generate man pages as part of the build or install process.

### Custom help printer (optional)

`clap-help` crate provides table-formatted help with rounded borders and customizable skin. Could replace default clap help output for a more polished look.

```rust
use clap_help::Printer;

// In --help handler:
let mut printer = Printer::new(Cli::command())
    .with("options", TEMPLATE_OPTIONS_MERGED_VALUE);
let skin = printer.skin_mut();
skin.headers[0].compound_style.set_fg(ansi(202));  // orange headers
skin.table_border_chars = ROUNDED_TABLE_BORDER_CHARS;
printer.print_help();
```

**Decision:** Defer to Phase 2. Default clap help is already significantly better than bpaf's flat list.

---

## Migration Steps

### Step 1: Setup (0.5 day)

- [ ] Add `clap` dependency to workspace `Cargo.toml` and `talon-cli/Cargo.toml`
- [ ] Remove `bpaf` from both Cargo.tomls
- [ ] Create `cli/` directory structure with new files
- [ ] Define `ValueEnum` derives for `SearchMode` and `Direction` in `talon-core` (or keep local to cli crate)

### Step 2: Root CLI struct (0.5 day)

- [ ] Create `cli/mod.rs` with root `Cli` struct
- [ ] Create `Commands` enum with all subcommands
- [ ] Create `global_args.rs` with shared flags as `#[derive(Args)]`
- [ ] Replace `parse_or_exit()` with `Cli::parse()` or `Cli::try_parse().unwrap_or_else(|e| e.exit())`

### Step 3: Command structs (1 day)

- [ ] Create `search_args.rs` — move search-relevant fields from old CliArgs
- [ ] Create `read_args.rs` — from_line, max_lines, raw
- [ ] Create `sync_args.rs` — force
- [ ] Create `related_args.rs` — depth, direction
- [ ] Create `meta_args.rs` — select, tag_counts, sources
- [ ] Create `changes_args.rs` — since
- [ ] Create `lint_args.rs` — (check what lint needs)
- [ ] Create `recall_args.rs` — format, budget_tokens, min_confidence, prior_messages, exclude
- [ ] Create `status_args.rs` and `init_args.rs` — empty or minimal
- [ ] Define `SharedScopeArgs` in `scope.rs`, flatten into query commands

### Step 4: Command dispatch (0.5 day)

- [ ] Update `command/mod.rs` to match on `Commands` enum instead of string positionals
- [ ] Each command handler receives its typed args struct instead of `&CliArgs + rest`
- [ ] Remove positional query joining logic — queries are now typed fields in command structs

### Step 5: Call site updates (0.5 day)

- [ ] Update all command modules to access fields from their specific args struct
- [ ] Replace `.enabled()` calls on flag types with direct `bool` checks
- [ ] Remove manual parse closures — use `value_enum` instead
- [ ] Update `lib.rs::run()` to work with new CLI structure
- [ ] Remove `flag_type!` macro and all 10 flag enum types
- [ ] Remove `normalize_cli_args()` function (agent/verbose precedence can be handled in run() or via clap's `conflicts_with`)

### Step 6: Cleanup (0.5 day)

- [ ] Delete old `cli.rs` and verify no references remain
- [ ] Delete old `cli/scope.rs` (or convert to shared scope struct)
- [ ] Update documentation/comments referencing bpaf
- [ ] Run `just check` for formatting/linting
- [ ] Verify all commands work with their flags

---

## Reference Resources

### Documentation

- **clap derive guide:** https://docs.rs/clap/latest/clap/_derive/
- **clap derive tutorial:** https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
- **clap Cookbook:** https://docs.rs/clap/latest/clap/_cookbook/index.html
- **Command API (help templates, error formatting):** https://docs.rs/clap/latest/clap/struct.Command.html
- **Error types:** https://docs.rs/clap/latest/clap/struct.Error.html

### Helper crates

- **clap_complete** — shell completions (bash, zsh, fish, powershell, elvish)
  - https://docs.rs/clap_complete
- **clap_mangen** — man page generation
  - https://docs.rs/clap_mangen
- **clap-help** — table-formatted help with customizable skin (Phase 2)
  - https://docs.rs/clap-help
- **clap-verbosity-flag** — reusable `--verbose`/`-v` flag struct (optional, could use instead of manual verbose field)
  - https://crates.io/crates/clap-verbosity-flag

### Inspiration from other Rust CLIs

- **eza** (ls replacement): grouped help headings, colored output, per-subcommand help
  - https://github.com/eza-community/eza
- **bat** (cat replacement): syntax-highlighted output, clear error messages
  - https://github.com/sharkdp/bat
- **zoxide** (cd replacement): minimal flags, clean help
  - https://github.com/ajeetdsouza/zoxide
- **fd** (find replacement): simple interface, excellent help text
  - https://github.com/sharkdp/fd
- **git** (reference): the gold standard for subcommand structure and per-command help
  - `git status --help`, `git log --help` each show only relevant flags

### Design comparisons

- **clap vs bpaf comparison:** https://www.libhunt.com/posts/890161-design-comparison-of-clap-and-bpaf-arg-parsers
- **Rust CLI recommendations:** https://rust-cli-recommendations.sunshowers.io/cli-parser.html (recommends clap)

---

## Risk Assessment

| Risk | Severity | Mitigation |
|---|---|---|
| Compile time increase from derive macros | Low — small crate, ~100ms impact | Monitor, not a blocker |
| Breaking existing scripts that parse `--help` output | Medium — help format changes | Document the change; script parsers are fragile by nature |
| Subcommand flag scoping breaks user workflows | High — if global flags become command-scoped | Keep all current flags as global or properly flatten into each command |
| MCP mode handling | Low — MCP uses TalonInput, not CLI args directly | Minimal changes needed |
| Test breakage | Medium — existing tests may reference old struct fields | Update test fixtures to use new typed structs |

### Key risk: flag scoping

The biggest behavioral risk is making flags that were previously global (available with every command) become scoped to specific commands. For example, `--json` was available with `talon search --json` and `talon read --json`. After migration, it must remain available everywhere.

**Solution:** Put all cross-cutting flags (`--json`, `--agent`, `--verbose`, `--fast`, `--raw`) on the root `Cli` struct or in a flattened global args struct. Command-specific flags go on command structs.

---

## Decision Log

| Decision | Rationale |
|---|---|
| Use clap derive, not builder API | Derive is cleaner for this codebase size; less boilerplate overall |
| Keep `--where` as `Vec<String>` with manual parsing | `where_clause.rs` parses data format (KEY OP VALUE), not CLI syntax. No change needed. |
| Scope flags in shared struct, flattened into commands | Single source of truth, no duplication, clean per-command help |
| Defer clap-help crate to Phase 2 | Default clap help is already a win; table formatting is polish |
| Defer shell completions and man pages to Phase 2 | Not required for UX parity; can be added post-migration |
| Keep `flag_type!` macro removal for migration step | Eliminates 10 enum types, 90+ lines of boilerplate. Simple bool → true win. |
| Move `SearchMode` and `Direction` ValueEnum to cli crate | They're CLI-facing concerns, not core domain. Keeps talon-core clean. |
| Use `ignore_case = true` on enums | Matches current bpaf behavior (case-insensitive matching via manual parsing) |
| Preserve exact flag names and short options | Zero behavioral change for users |

---

## Success Criteria

1. `talon --help` shows grouped, categorized flags with clear headings
2. `talon search --help` shows only search-relevant flags (no sync or read flags)
3. All existing flags work exactly as before (same names, same positions, same semantics)
4. No custom flag enum types remain — all flags are `bool` fields
5. Enum validation errors show valid options and "did you mean" suggestions
6. `just check` passes with no formatting/linting issues
7. All command dispatch paths work (search, read, sync, related, meta, changes, lint, recall, status, init)
8. MCP mode still works (--mcp flag)
9. Agent mode precedence over verbose still enforced
10. No regression in existing tests
