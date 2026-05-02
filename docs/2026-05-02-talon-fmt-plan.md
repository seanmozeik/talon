# Talon fmt Plan

## Summary

`talon fmt` should be Talon's vault-wide Obsidian Markdown formatter. The command
formats files immediately by default; `talon fmt --check` is the non-writing mode
that reports which files would change. This is intentionally different from many
developer formatters where a bare command only checks or prints. Talon's audience
is someone with an Obsidian vault who wants the vault normalized at scale.

The main upstream reference is Obsidian Linter:

- https://github.com/platers/obsidian-linter
- Local source mirror used during planning:
  `/home/yolo/.opensrc/repos/github.com/platers/obsidian-linter/master`

The goal is not to embed or shell out to the TypeScript plugin. The goal is to
port the stable formatting rules to Rust, keep the behavior deterministic and
idempotent, and make it fast enough for large vaults.

## Product Position

Talon already treats Obsidian vaults as its native domain: indexing, search,
recall, link analysis, metadata, changes, and lint all operate over vault notes.
`talon fmt` should complete that maintenance loop by writing normalized Markdown
back to the vault.

Primary promise:

```text
talon fmt
```

turns a messy Obsidian vault into a consistently formatted Obsidian vault.

Secondary promise:

```text
talon fmt --check
```

is CI- and agent-friendly. It exits non-zero when formatting would change files
and prints the paths/rules that would be applied.

## Command Semantics

### Default mode writes

`talon fmt` writes changes in place.

This is deliberate. The formatter is an action command, not a passive linter.
Users who want a dry run must ask for it with `--check` or `--diff`.

### Check mode does not write

`talon fmt --check` must:

- traverse the same file set as write mode,
- run the exact same formatter pipeline,
- not modify any file,
- print each file that would change,
- include enough summary information for humans and agents,
- exit `0` when no file would change,
- exit non-zero when one or more files would change.

### Diff mode is preview-focused

`talon fmt --diff` should not write. It should print unified diffs, plus the same
summary as `--check`. `--diff` and `--check` may be allowed together, but `--diff`
already implies no writes.

### Scope and path selection

The command should support the same mental model as other Talon commands:

```bash
talon fmt
talon fmt wiki/
talon fmt "wiki/Stock Theory.md"
talon fmt --scope wiki
talon fmt --scope-only projects
talon fmt --scope-all
```

Path arguments narrow the file set. Scope arguments narrow the file set using
the existing scope configuration.

### Ignore handling

`talon fmt` must respect existing Talon ignore behavior.

At minimum:

- top-level `ignore_patterns` are excluded,
- default ignores from `talon init` are excluded,
- non-Markdown files are excluded,
- `.obsidian/**`, `.git/**`, and other configured non-note paths are never
  formatted unless the user explicitly changes config.

The command should also have formatter-specific ignores:

```toml
[fmt]
ignore = ["templates/**", "archive/raw-imports/**"]
```

`fmt.ignore` should be additive with `ignore_patterns`, not a replacement.

Open decision: whether `[lint].ignore` should apply to `fmt`. The safer default
is no. Lint exclusions often mean "do not report graph findings," while format
exclusions mean "do not write files." Writing and reporting are different risk
classes, so `fmt.ignore` should be explicit.

## CLI Surface

Initial target:

```bash
talon fmt [PATH ...]
    --check
    --diff
    --rules <LIST>
    --skip-rules <LIST>
    --jobs <N|auto>
    --scope <NAME>...
    --scope-only <NAME>...
    --scope-all
    --agent
    --json
```

Proposed behavior:

| Invocation | Writes? | Output |
| --- | --- | --- |
| `talon fmt` | yes | concise changed/unchanged/error summary |
| `talon fmt --check` | no | files that would change; non-zero if any |
| `talon fmt --diff` | no | unified diffs and summary |
| `talon --agent fmt --check` | no | compact JSON intended for agents |
| `talon --json fmt` | yes | full Talon envelope with format report |

`--rules` and `--skip-rules` should accept stable category names first:

- `spacing`
- `yaml`
- `links`
- `quotes`
- `headings`
- `lists`
- `footnotes`
- `prose`

Later they can accept individual Obsidian Linter-compatible rule aliases.

## Configuration

Add a dedicated `[fmt]` table to `TalonConfig`.

Sketch:

```toml
[fmt]
jobs = "auto"
ignore = []
line_endings = "preserve" # preserve | lf
final_newline = true

[fmt.rules]
spacing = true
yaml = true
links = true
quotes = true
headings = true
lists = true
footnotes = true
prose = true
```

The first implementation can ship with hard-coded rule defaults and only add the
table once users need persistence. The command line should still be designed so
the config table can slot in without breaking flags.

## Architecture

Put the formatter engine in `talon-core`, with CLI plumbing in `talon-cli`.

Suggested layout:

```text
crates/talon-core/src/fmt/
  mod.rs
  config.rs
  engine.rs
  file_set.rs
  protected.rs
  report.rs
  rules/
    spacing.rs
    yaml.rs
    links.rs
    quotes.rs
    headings.rs
    lists.rs
    footnotes.rs
    prose.rs

crates/talon-cli/src/cli/fmt_args.rs
crates/talon-cli/src/command/fmt.rs
```

Core responsibilities:

- discover candidate Markdown files,
- apply config/scope/path/ignore filtering,
- parse protected ranges,
- run ordered formatting rules,
- compare original vs formatted bytes,
- return structured reports,
- write files only when requested by the caller.

CLI responsibilities:

- parse flags,
- load config,
- choose output mode,
- call `talon-core`,
- render human, JSON, and agent output.

## Parallel Execution

Use file-level parallelism. Each Markdown file can be formatted independently.
Rayon is a good fit because the work is CPU/string heavy and the implementation
can stay synchronous.

Pipeline:

1. Walk the vault and collect candidate `.md` files.
2. Apply include, ignore, path, and scope filters.
3. Sort paths for deterministic reporting.
4. Process files with `rayon::par_iter`.
5. Read each file.
6. Run the formatter pipeline.
7. If unchanged, return an unchanged result.
8. If changed and write mode is enabled, write via temporary file and atomic
   rename where possible.
9. Return a per-file result.
10. Sort results before rendering output.

Determinism matters. Parallel execution must not produce nondeterministic output
ordering.

`--jobs auto` should use Rayon defaults. `--jobs N` should use a local thread
pool rather than mutating a global pool that could surprise future callers.

## Formatter Engine

The formatter should be rule-ordered and idempotent.

Basic shape:

```rust
pub struct FormatInput<'a> {
    pub path: &'a Path,
    pub vault_relative_path: &'a str,
    pub original: &'a str,
    pub options: &'a FormatOptions,
}

pub struct FormatOutput {
    pub text: String,
    pub applied_rules: Vec<AppliedRule>,
}
```

Rule interface:

```rust
pub trait FormatRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn category(&self) -> FormatCategory;
    fn apply(&self, doc: &mut MarkdownDoc) -> Result<RuleChange>;
}
```

It is acceptable to start simpler with ordered pure functions over `String`.
Do not prematurely build a full Markdown AST if protected ranges and line-based
rewrites are enough for the first categories.

## Protected Ranges

A shared protected-range scanner is the most important safety primitive.

Rules need to know when not to touch content inside:

- YAML frontmatter,
- fenced code blocks,
- inline code spans,
- math blocks,
- Obsidian comments,
- HTML blocks where appropriate,
- wikilinks and Markdown links where appropriate.

Different rules need different protections. For example, spacing around code
fences must see fence lines, but quote normalization should avoid fenced code
content entirely.

Build one scanner and let each rule request the protection set it needs.

## Rule Porting Strategy

Use Obsidian Linter as the behavior reference, but port rule categories in
coherent batches.

When translating logic closely from upstream, cite the source file and line in
an inline Rust comment and aggregate attribution in `LICENSE-3RD-PARTY.md`.
The upstream project is MIT licensed, so porting is viable, but attribution must
be explicit.

### Phase 1: engine and spacing rules

Implement command plumbing, file traversal, reports, and the high-value rules
that mostly operate on line layout:

- trailing spaces,
- final newline,
- consecutive blank lines,
- paragraph blank lines,
- heading blank lines,
- empty line around code fences,
- empty line around math blocks,
- empty line around tables,
- empty line around horizontal rules,
- empty line around blockquotes,
- space after list markers,
- remove link spacing.

Acceptance criteria:

- `talon fmt` writes by default.
- `talon fmt --check` does not write and exits non-zero when changes are needed.
- configured ignore patterns are honored.
- output is deterministic under parallel execution.
- every rule is idempotent.

### Phase 2: YAML/frontmatter rules

Port frontmatter rules:

- add blank line after YAML,
- compact YAML,
- format YAML arrays,
- format tags in YAML,
- sort YAML array values,
- dedupe YAML array values,
- YAML key sort,
- YAML title,
- YAML title alias,
- insert YAML attributes,
- remove YAML keys,
- escape YAML special characters,
- force YAML escape.

This phase should lean on structured YAML parsing/serialization where possible.
Avoid ad hoc string manipulation unless preserving comments or Obsidian-specific
syntax requires it.

### Phase 3: links, quotes, and prose style

Port content normalization rules:

- no bare URLs,
- quote style,
- proper ellipsis,
- emphasis style,
- strong style,
- blockquote style,
- remove multiple spaces,
- remove space around characters,
- remove space before or after characters,
- CJK/English/number spacing,
- default language for code fences.

These rules have higher risk because they rewrite prose. That risk is acceptable
for `talon fmt`, but they still need strong fixtures and protected ranges.

### Phase 4: headings and lists

Port structural-but-common rules:

- capitalize headings,
- header increment,
- headings start line,
- remove trailing punctuation in heading,
- unordered list style,
- ordered list style,
- convert bullet list markers,
- remove consecutive list markers,
- remove empty list markers,
- remove empty lines between list markers and checklists.

Heading capitalization needs careful defaults. It can be useful and visible, but
it is more subjective than blank-line cleanup.

### Phase 5: footnotes and heavier structural rewrites

Port heavier rules:

- footnote after punctuation,
- re-index footnotes,
- move footnotes to the bottom,
- move math block indicators to own line,
- move tags to YAML,
- file-name heading.

These should be later because they can move content across the file.

### Paste-only rules

Obsidian Linter has paste-only rules. `talon fmt` is file-oriented, so do not
port paste rules directly unless the behavior makes sense for a whole file.
If ported, put them behind normal file-rule names rather than preserving
paste-specific semantics.

## Reporting

Human output should be concise:

```text
Formatted 42 files, unchanged 318, skipped 12, errors 0
```

With `--check`:

```text
Would format 42 files
  wiki/Stock Theory.md
  daily/2026-04-24.md
```

Agent output should be structured:

```json
{
  "changed": 42,
  "unchanged": 318,
  "skipped": 12,
  "errors": [],
  "files": [
    {
      "path": "wiki/Stock Theory.md",
      "changed": true,
      "rules": ["heading-blank-lines", "trailing-spaces"]
    }
  ]
}
```

In write mode, `changed=true` means the file was modified. In check mode,
`changed=true` means the file would be modified.

## Failure Handling

Formatting a vault should be robust across thousands of files.

Rules:

- A read/write error for one file should not stop the entire run unless the
  error prevents traversal.
- The final exit code should be non-zero if any file errors.
- In write mode, write through a temp file in the same directory, then rename.
- Preserve original permissions where practical.
- Do not write unchanged files.
- Do not rewrite files that are not valid UTF-8 unless a future binary-safe mode
  is explicitly designed.

## Testing Strategy

### Unit tests

Each rule gets focused before/after tests, including idempotence:

```text
format(format(input)) == format(input)
```

### Golden fixtures

Create fixture directories:

```text
crates/talon-core/tests/fixtures/fmt/
  spacing/
  yaml/
  links/
  quotes/
  headings/
  footnotes/
```

Each fixture should have:

- `input.md`
- `expected.md`
- optional `options.toml`

Where a fixture is copied or directly adapted from Obsidian Linter's test vault,
cite the source in the test comment and in `LICENSE-3RD-PARTY.md`.

### CLI tests

Cover:

- bare `talon fmt` writes,
- `--check` does not write,
- `--check` exits non-zero when changes are pending,
- `--diff` prints a diff and does not write,
- ignore patterns are respected,
- path arguments narrow formatting,
- scope arguments narrow formatting,
- `--agent` emits compact JSON.

### Parallel tests

Have at least one test that formats many files with `--jobs 2` and verifies:

- deterministic result ordering,
- only expected files changed,
- ignored files are untouched.

## Implementation Order

1. Add formatter plan and attribution note.
2. Add `FmtArgs` and `Commands::Fmt`.
3. Add `talon-core::fmt` module with report types and no-op formatter.
4. Add file discovery that respects vault path, Markdown extension, configured
   ignore patterns, path args, and scope args.
5. Add write/check/diff behavior with no-op rules.
6. Add parallel execution with deterministic result rendering.
7. Add Phase 1 spacing rules.
8. Add CLI tests for default write, check, diff, and ignores.
9. Add YAML/frontmatter rule batch.
10. Add style/content rule batches.
11. Add structural and footnote batches.

This order gets the risky semantics right before the long rule-porting work.

## Open Questions

- Should `fmt.ignore` exist from day one, or should initial behavior rely only
  on `ignore_patterns`?
- Should `fmt` respect per-scope `lint = false`? Recommendation: no. Add a
  future `fmt = false` per-scope field only if users need it.
- Should quote normalization and heading capitalization be on by default in the
  first stable release? The product direction says aggressive, but these are
  more subjective than spacing/YAML/link cleanup.
- Should `talon fmt` refresh the index after writing? Recommendation: do not
  implicitly run `sync` in the first version. Print a hint that search results
  may need `talon sync` after large formatting runs.
- Should file modification times be preserved? Recommendation: no by default.
  Formatting is a real edit.

## Done Definition

`talon fmt` is ready when:

- a bare `talon fmt` formats files in place,
- `talon fmt --check` reports pending changes without writing,
- configured ignore patterns are honored,
- formatting is deterministic and idempotent,
- the engine processes files in parallel without nondeterministic reports,
- the initial stable rule set is ported with attribution,
- `just check` passes,
- user-facing docs describe the command clearly.
