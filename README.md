# Talon

Talon is a standalone Rust binary for Obsidian vault search, read, sync, related-note traversal, status, and MCP-over-stdio integration.

This repository is currently scaffolded from `2026-04-25-talon-extraction-design.md`. The major architecture questions in that design are resolved; implementation is intentionally skeletal while the Rust port is built out.

## Layout

- `crates/talon-core`: typed contracts, config, constants, and future core search/indexing modules.
- `crates/talon-cli`: `bpaf` CLI, `--agent`, `--skill`, banner/spinner scaffolding, `--mcp` entry point, config loading, and process wiring.
- `skill/SKILL.md`: agent-facing contract copied from the reference implementation.
- `ts/`: thin npm wrapper skeleton for binary resolution and MCP child specs.

## CLI UX Direction

Human output is a product surface. The scaffold uses `anstyle`/`anstream` for terminal styling, `rattles` for indeterminate spinners, and reserves `indicatif` for future sync/embed progress bars. Tables/result cards are intentionally deferred until real search output lands.

## Development

```bash
just verify
cargo run -p talon-cli -- --help
cargo run -p talon-cli -- init
cargo run -p talon-cli -- --skill
```

## Standalone Embedder

Set `inference.base_url` in `~/.config/talon/config.toml` to a local HTTP TEI-compatible endpoint. Good defaults are Hugging Face `text-embeddings-inference`, Infinity, or ultraclaw's local sidecar if you are running one. Talon only expects `/embed`, `/embed-chunked`, and `/rerank` endpoints with the shapes described in the design doc.

## Scopes

Scopes partition the vault by role and let queries opt in or out of each partition. Declare them in `~/.config/talon/config.toml`:

```toml
[scopes.wiki]
glob = "wiki/**"
priority = "boosted"
default = true
lint = true

[scopes.daily]
glob = "daily/**"
priority = "muted"
default = true
lint = false        # daily notes still indexed; just not reported by `talon lint`

[scopes.private]
glob = "private/**"
priority = "buried"
default = false     # excluded from queries unless --scope private / --scope-all
lint = false
```

Scope iteration follows TOML declaration order — narrower or more sensitive scopes declared above broader ones win when their globs overlap.

By default, queries (`search`, `recall`, `related`, `meta`, `changes`, `lint`) cover only scopes with `default = true`. Scopes with `default = false` are **excluded** entirely — not just down-ranked.

To include a `default = false` scope, opt in explicitly:

- `--scope NAME` (repeatable, additive, short form `-s`): adds the named scope to the default pool.
- `--scope-only NAME` (repeatable): searches only the named scope(s).
- `--scope-all`: searches every configured scope, overriding the `default` flag.

The three are mutually exclusive on a single invocation. Unknown scope names error with the list of configured names. The response's `meta.scope_set` echoes the resolved active scope names. See `examples/config.toml` for a Karpathy-style preset.

### Lint exclusion

Per-scope `lint = false` skips a scope's files in `talon lint` findings — useful for ephemeral journals (`daily/`), closed work (`archive/`), or sensitive material (`private/`). Excluded files are still indexed and continue to satisfy link-target resolution, so a wiki note linking to a `daily/` file isn't reported as broken.

For globs that don't fit cleanly into a scope, set a global ignore list:

```toml
[lint]
ignore = ["**/_drafts/**", "**/scratch.md"]
```

Global `lint.ignore` takes precedence over per-scope `lint = true`.

## Per-result fields

`search`, `related`, and `meta` results carry these fields in `--json` mode (skipped in `--agent` mode for token efficiency):

- `scope`: resolved scope name. Omitted for paths that match no scope.
- `mtime`: file modification time in the system local timezone. `"HH:MM"` (e.g. `"15:42"`) for edits within the last 24 hours, `"YYYY-MM-DD"` (e.g. `"2026-04-25"`) otherwise. Recent edits get instantly-readable wall-clock time; older edits collapse to date. For sub-day precision on indexing/deletion events, see `changes`.
- `count` (`related` only): number of distinct link rows between source and target — a rough edge-strength signal.

`changes.indexed_at` and `tombstones.deleted_at` use full RFC 3339 UTC (`"2026-04-25T10:23:00Z"`) since `--since` consumers compare exact timestamps.

## Chunking

Markdown chunking is configured under `[indexer]` in `talon.toml`:

```toml
[indexer]
chunk_tokens = 512
chunk_overlap = 64
chunk_min_tokens = 16
```

The text-splitter/tokenx chunker strips frontmatter from BM25 and embedding text. Existing indexes will be re-chunked on the next `talon sync`, which also queues the new chunks for embedding. On a large vault, the first post-upgrade sync can spend 30+ minutes re-embedding against a real sidecar.

## Integrations

- [`integrations/hermes-talon-recall/`](integrations/hermes-talon-recall/README.md) — Hermes Agent Memory Provider plugin that wraps `talon recall --format prompt-xml` to inject vault-native context on every agent turn. Recall-only; agent host handles vault writes.
