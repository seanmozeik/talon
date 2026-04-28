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
| `crates/talon-cli/Cargo.toml` | Replace `bpaf` with `clap = { version = "4.6", features = ["derive", "color", "error-context"] }` |
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
| `Cargo.toml` (workspace) | Replace `bpaf` dep with `clap = { version = "4.6", features = ["derive", "color", "error-context"] }` |

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

---

## Shell Completions (Initial Scope)

All shells — bash, zsh, fish, powershell, **nushell**.

```toml
# Cargo.toml
clap = { version = "4.6", features = ["derive", "color", "error-context"] }
clap_complete = "4.6"
clap_complete_nushell = "4.6"
```

```rust
// crates/talon-cli/src/completion.rs
use clap::CommandFactory;
use clap_complete::{Generator, generate_to, shells::*};
use clap_complete_nushell::Nushell; // separate crate, same version

pub fn generate_completions() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Cli::command();
    let out_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("talon")
        .join("completions");
    std::fs::create_dir_all(&out_dir)?;

    for shell in [Bash, Zsh, Fish, PowerShell] {
        generate_to(shell, &mut cmd, "talon", &out_dir)?;
    }
    // Nushell: separate function, not Generator trait
    generate_to(Nushell, &mut cmd, "talon", &out_dir)?;

    Ok(())
}
```

Install hook in `main()` or expose as `talon completions install` subcommand.

---

## Banner + Styling (Initial Scope)

The ASCII banner already exists in `crates/talon-cli/src/banner.rs` with gradient coloring (`anstyle`). Plumb it into help output on first run.

### Help template customization

Override the default clap help template to inject the banner:

```rust
// crates/talon-cli/src/cli/styling.rs
use clap::builder::{AnsiColor, Styles};

pub fn talon_styles() -> Styles {
    // Matches the existing banner gradient: cyan → blue → purple
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())   // help heading groups
        .usage(AnsiColor::Cyan.on_default().bold())      // Usage: line
        .literal(AnsiColor::Green.on_default().bold())   // flag names (--json)
        .valid(AnsiColor::Green.on_default())            // possible values
        .invalid(AnsiColor::Red.on_default())            // error values
}
```

Apply to root struct:

```rust
#[derive(Parser)]
#[command(
    name = "talon",
    about = "Obsidian vault search, indexing, and MCP server.",
    styles = talon_styles(),
    after_help = r#"Examples:
  talon search "project setup" --mode hybrid
  talon read src/main.rs --from-line 10 --max-lines 20
  talon related src/lib.rs --depth 2 --direction both
  talon sync --force

Use 'talon <command> --help' for per-command help."#
)]
struct Cli { /* ... */ }
```

### Banner on first use

Reuse existing `banner::eprint_fancy_prelude_for_run()` — it already checks agent/json/mcp/TTY conditions. After migration, pass the new typed args instead of `CliArgs`.

No separate banner-in-help needed — the existing banner prints to stderr before output, which is the right behavior. The help template styling (above) makes `--help` look polished without needing a custom printer.

---

## Migration Steps

### Step 1: Setup (0.5 day)

- [ ] Add `clap`, `clap_complete`, `clap_complete_nushell` to workspace `Cargo.toml`
- [ ] Remove `bpaf` from both Cargo.tomls
- [ ] Create `cli/` directory structure with new files
- [ ] Create `cli/styling.rs` — define `talon_styles()` matching banner gradient
- [ ] Define `ValueEnum` derives for `SearchMode` and `Direction` in `talon-core` (or keep local to cli crate)

### Step 2: Root CLI struct (0.5 day)

- [ ] Create `cli/mod.rs` with root `Cli` struct
- [ ] Create `Commands` enum with all subcommands
- [ ] Create `global_args.rs` with shared flags as `#[derive(Args)]`
- [ ] Add `styles = talon_styles()`, `after_help` examples to root struct
- [ ] Replace `parse_or_exit()` with `Cli::parse()` or `Cli::try_parse().unwrap_or_else(|e| e.exit())`

### Step 2.5: Shell completions module (0.5 day)

- [ ] Create `completion.rs` — generate bash/zsh/fish/powershell/nushell completions
- [ ] Wire into build or expose as `talon completions install` subcommand
- [ ] Use `ValueHint::FilePath` on path args for better tab-completion

### Step 3: Command structs (1 day)

- [ ] Create `search_args.rs` — move search-relevant fields from old CliArgs, add `long_about`
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
- [ ] Wire existing `banner::eprint_fancy_prelude_for_run()` through new args type
- [ ] Remove `flag_type!` macro and all 10 flag enum types
- [ ] Remove `normalize_cli_args()` function (agent/verbose precedence can be handled in run() or via clap's `conflicts_with`)

### Step 6: Cleanup (0.5 day)

- [ ] Delete old `cli.rs` and verify no references remain
- [ ] Run `just check` for formatting/linting
- [ ] Delete old `cli/scope.rs` (or convert to shared scope struct)
- [ ] Update documentation/comments referencing bpaf
- [ ] Run `just check` for formatting/linting
- [ ] Verify all commands work with their flags

---

---

## Research — clap 4.6.1 (latest as of 2026-04) — Updated 2026-04-29

Clap 4.6.1 is the latest stable version. Source fetched via opensrc at `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/`.

### New / Important APIs Since Draft

#### 1. `ArgAction::Count` for verbose-like flags
Use `u8` with `action = clap::ArgAction::Count` instead of a custom flag enum or bool. Enables `-v`, `-vv`, `-vvv` patterns:

```rust
#[arg(short, long, action = clap::ArgAction::Count)]
verbose: u8,
```

This is cleaner than `bool` for flags that might want future verbosity levels. For talon, stick with `bool` since we don't need multiple levels.

#### 2. Help Template System (powerful customization)
Clap uses a template language with these placeholders:
- `{before-help}`, `{about-with-newline}`, `{usage-heading} {usage}`
- `{all-args}`, `{options}`, `{positionals}`, `{subcommands}`
- `{after-help}`, `{tab}`

Default template:
```
{before-help}{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
```

You can override via `#[command(help_template = "...")]` on the struct or via builder API.

#### 3. Styling System (`Styles`)
Full control over terminal colors:
```rust
use clap::builder::{AnsiColor, Styles};

let styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::White.on_default());
```

Available styles: `header`, `error`, `usage`, `literal`, `placeholder`, `valid`, `invalid`, `context`, `context_value`.

Apply via builder:
```rust
#[derive(Parser)]
#[command(styles = custom_styles())]
struct Cli { /* ... */ }
```

Or on individual args with raw attributes:
```rust
#[arg(help_heading = "OUTPUT")]
json: bool,
```

#### 4. Error Formatting — `RichFormatter`
Clap 4.6 has two error formatters (feature-gated):
- **`KindFormatter`** — basic: just the error kind string
- **`RichFormatter`** (default, feature `error-context`) — rustc-style diagnostics with:
  - Context lines showing where the error occurred
  - "Did you mean?" suggestions for subcommands and arguments
  - Colored output following rustc diagnostic style guide

No custom implementation needed — just enable the `error-context` feature (it's default).

#### 5. `flatten_help = true`
From the git-derive example — shows nested subcommand help inline:
```rust
#[derive(Args)]
#[command(flatten_help = true)]
struct StashArgs {
    #[command(subcommand)]
    command: Option<StashCommands>,
}
```
This makes `talon stash --help` show subcommand options inline instead of just listing them.

#### 6. `external_subcommand`
For commands that accept arbitrary extra args:
```rust
#[derive(Subcommand)]
enum Commands {
    // ... known commands ...
    #[command(external_subcommand)]
    External(Vec<String>),
}
```

### Inspiration Patterns from Modern CLIs

#### Pattern A: Minimal global flags, rich per-command help (zoxide, fd)
Global flags are kept to absolute essentials (`--config`, `--verbose`). Everything else is command-specific. This keeps `talon --help` clean.

For talon: `--mcp`, `--skill`, `--version`, `--config` go global. `--json`, `--agent`, `--fast`, `--raw`, `--force`, `--verbose` could be command-specific or stay global depending on usage patterns.

#### Pattern B: Help headings group related flags (find, ripgrep)
Instead of a flat list, flags are grouped:
```
OPTIONS:
  -v, --verbose    Enable verbose output
  -c, --config     Config file path

SEARCH MODES:
      --mode       hybrid | semantic | fulltext | title
      --limit      Result count

OUTPUT:
      --json       JSON output
      --raw        Raw content
```

This is exactly what `next_help_heading` provides.

#### Pattern C: Rich `--help` with long descriptions (bat, fd)
The short help (`-h`) shows one-liners. The long help (`--help`) shows full documentation:
```rust
#[command(
    about = "Search your Obsidian vault",
    long_about = r#"Search your Obsidian vault using hybrid ranking.

Combines BM25 fulltext scoring with semantic vector similarity.
The query is expanded using LLM-based context before ranking."#
)]
struct SearchArgs { /* ... */ }
```

#### Pattern D: Subcommand-specific help templates (git)
Each subcommand can have its own `about`, `long_about`, and even `help_template`. This lets `talon search --help` look different from `talon sync --help`.

### "Sexy" CLI Design Ideas for Talon

#### Idea 1: Custom help template with banner
Add a brief ASCII banner or tagline before help:
```rust
#[command(
    about = "Obsidian vault search, indexing, and MCP server.",
    before_help = "  ╔══════════════════════════════════╗\n  ║       talon — vault intelligence        ║\n  ╚══════════════════════════════════╝",
    long_about = None
)]
```

#### Idea 2: Custom styling — brand colors
Use a consistent color scheme:
```rust
fn talon_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default().bold())
        .usage(AnsiColor::Cyan.on_default().bold())
        .literal(AnsiColor::Green.on_default().bold())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
}
```

#### Idea 3: `--help` shows example commands
Use `after_help` to show common usage patterns:
```rust
#[command(
    after_help = r#"Examples:
  talon search "project setup" --mode hybrid
  talon read src/main.rs --from-line 10 --max-lines 20
  talon related src/lib.rs --depth 2 --direction both
  talon sync --force
  talon meta --tag-counts"#
)]
```

#### Idea 4: Per-command `after_help` with examples
Each command struct can have its own examples section, similar to `git commit --help` showing usage patterns.

#### Idea 5: Use `value_hint` for path arguments
Clap supports value hints for shell completion:
```rust
#[arg(value_hint = clap::ValueHint::FilePath)]
config_file: Option<PathBuf>,
```
This enables better tab-completion in shells that support it (via clap_complete).

#### Idea 6: `ArgGroup` for mutually exclusive options
Use groups to enforce "pick one" semantics:
```rust
#[command(group = ArgGroup::new("format")
    .required(true)
    .args(["json", "raw"]))]
struct ReadArgs {
    #[arg(long)]
    json: bool,
    #[arg(long)]
    raw: bool,
}
```

#### Idea 7: `conflicts_with` and `requires` for flag relationships
Enforce semantic constraints at parse time:
```rust
#[arg(long, conflicts_with = "verbose")]
agent: bool,  // agent mode suppresses verbose output
```
This makes the CLI self-documenting — invalid flag combinations get clear errors before any code runs.

### Updated Cargo.toml Dependencies

```toml
# Workspace Cargo.toml
clap = { version = "4.6", features = ["derive", "color", "error-context"] }
clap_complete = "4.6"
clap_complete_nushell = "4.6"
```

Note: `error-context` and `color` are default features but being explicit is better. No `clap_mangen` — man pages are out of scope.

### Updated Help Heading Plan (refined)

Based on inspiration from find(1), ripgrep, and git:

| Command | Headings | Notes |
|---|---|---|
| `search` | MODE, SCOPE, FILTERS, OUTPUT | "MODE" covers --mode, --limit, --candidate-limit, --intent |
| `read` | POSITION, FORMAT, OUTPUT | "POSITION" for --from-line, --max-lines |
| `sync` | OPTIONS, OUTPUT | Minimal command |
| `related` | TRAVERSAL, SCOPE, OUTPUT | "TRAVERSAL" for --depth, --direction |
| `meta` | QUERY, SCOPE, FILTERS, OUTPUT | "QUERY" for --select, --tag-counts, --sources |
| `changes` | TIME RANGE, SCOPE, OUTPUT | "TIME RANGE" for --since |
| `lint` | OPTIONS, OUTPUT | Minimal command |
| `recall` | CONTEXT, SCOPE, FILTERS, OUTPUT | "CONTEXT" for format, budget, confidence, prior messages |

Global flags (on root `Cli`): `--mcp`, `--skill`, `--version`, `--config`, `--json`, `--agent`, `--fast`, `--raw`, `--verbose`

### Updated Migration Steps

Add to Step 1:
- [ ] Enable `error-context` feature on clap (explicit, not relying on default)
- [ ] Decide on custom `Styles` — define in `cli/styling.rs` or inline

Add to Step 2:
- [ ] Add `after_help` with usage examples to root `Cli`
- [ ] Use `conflicts_with("verbose")` on `--agent` flag

Add to Step 3:
- [ ] Add `long_about` descriptions to each command struct
- [ ] Add `after_help` with examples per command
- [ ] Use `value_hint = ValueHint::FilePath` for path arguments
- [ ] Consider `ArgGroup` for mutually exclusive format flags in `read`

---

## Reference Resources

### Documentation

- **clap derive guide:** https://docs.rs/clap/latest/clap/_derive/
- **clap derive tutorial:** https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
- **clap Cookbook:** https://docs.rs/clap/latest/clap/_cookbook/index.html
- **Command API (help templates, error formatting):** https://docs.rs/clap/latest/clap/struct.Command.html
- **Error types:** https://docs.rs/clap/latest/clap/struct.Error.html

### Helper crates

- **clap_complete** — shell completions (bash, zsh, fish, powershell)
  - https://docs.rs/clap_complete
- **clap_complete_nushell** — nushell completions (separate crate, same version as clap)
  - https://crates.io/crates/clap_complete_nushell
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
- **ripgrep** (grep replacement): `--help` is a masterclass in grouping and examples
  - https://github.com/BurntSushi/ripgrep
- **git** (reference): the gold standard for subcommand structure and per-command help
  - `git status --help`, `git log --help` each show only relevant flags
  - clap cookbook git-derive example: `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/examples/git-derive.rs`
- **find(1)** (Unix): master class in help headings (TESTS, OPERATORS, ACTIONS)
  - clap cookbook find example: `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/examples/find.rs`

### Clap 4.6 Source (local cache)

- Help template: `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/clap_builder/src/output/help_template.rs`
- Error formatting: `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/clap_builder/src/error/format.rs`
- Styling system: `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/clap_builder/src/builder/styling.rs`
- Git derive example: `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/examples/git-derive.rs`
- Find example (help headings): `/home/yolo/.opensrc/repos/github.com/clap-rs/clap/4.6.1/examples/find.rs`

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
| Keep `flag_type!` macro removal for migration step | Eliminates 10 enum types, 90+ lines of boilerplate. Simple bool → true win. |
| Move `SearchMode` and `Direction` ValueEnum to cli crate | They're CLI-facing concerns, not core domain. Keeps talon-core clean. |
| Use `ignore_case = true` on enums | Matches current bpaf behavior (case-insensitive matching via manual parsing) |
| Preserve exact flag names and short options | Zero behavioral change for users |
| Enable `error-context` feature explicitly | RichFormatter gives rustc-style error diagnostics with "did you mean" suggestions |
| Use `conflicts_with("verbose")` on `--agent` | Enforce agent/verbose precedence at parse time, not in normalize_cli_args |
| Add `value_hint = ValueHint::FilePath` for path args | Better shell completion via clap_complete |
| Custom `Styles` with brand colors | Initial scope — yellow headers, cyan usage, green literals. Matches existing banner gradient aesthetic |
| Shell completions for all shells + nushell | Initial scope — clap_complete + clap_complete_nushell |
| `after_help` with usage examples on root Cli | Immediate value — users see real commands without reading docs |
| Reuse existing banner module | Already exists with gradient coloring; just wire new args type through |
| No clap-help crate | Default clap help with custom Styles is sufficient; no need for external table printer |
| No man pages | Out of scope for this migration

---

## Success Criteria

1. `talon --help` shows grouped, categorized flags with custom styling (yellow headers, cyan usage)
2. `talon search --help` shows only search-relevant flags (no sync or read flags)
3. `talon <command> --help` shows usage examples in after_help
4. All existing flags work exactly as before (same names, same positions, same semantics)
5. No custom flag enum types remain — all flags are `bool` fields
6. Enum validation errors show valid options and "did you mean" suggestions (RichFormatter)
7. Shell completions generated for bash, zsh, fish, powershell, nushell
8. Existing ASCII banner still prints on TTY runs (wired through new args type)
9. `just check` passes with no formatting/linting issues
10. All command dispatch paths work (search, read, sync, related, meta, changes, lint, recall, status, init)
11. MCP mode still works (--mcp flag)
12. Agent mode precedence over verbose still enforced (via conflicts_with or runtime check)
13. No regression in existing tests
