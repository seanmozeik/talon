---
name: talon
description: Obsidian vault search, read, sync (index + embed), related, meta, changes, lint, and status for agents and CLI users.
---

# Talon

Use Talon for Obsidian vault **search**, **read**, **sync** (lexical index + embeddings), **related**, **meta**, **changes**, **lint**, and **status**.

All commands emit `{action, version, ok, data, meta}` JSON on success; `{action, version, ok: false, error}` on failure.

## Modes

- **`sync`**: Scan the vault, update the lexical/SQLite index, then run embeddings unless `--fast` is set.
- **`sync --fast`**: Lexical/index pass only; no embeddings.
- **`search`**: Hybrid search across the indexed vault. Modes: `hybrid` (default), `semantic`, `fulltext`, `title`.
- **`search --fast`**: Lexical-only search; no expansion, no rerank.
- **`search --where KEY OP VALUE`**: Post-filter results by frontmatter (repeatable). Operators: `=`, `!=`, `<`, `<=`, `>`, `>=`, `contains`, `exists`.
- **`search --since <timestamp>`**: Post-filter to notes modified at or after the timestamp.
- **`read <path>`**: Return note body with frontmatter stripped and a heading tree. Use `--raw` to keep frontmatter.
- **`read --from-line N --max-lines M`**: Return a slice of the body.
- **`related <path>`**: Traverse wikilinks and backlinks. Use `--depth 1-3` and `--direction outgoing|backlinks|both`.
- **`meta`**: Filter notes by frontmatter and project fields.
  - `--where KEY OP VALUE` (repeatable) — filter by frontmatter field.
  - `--select FIELD` (repeatable) — project specific frontmatter fields onto each result.
  - `--tag-counts` — emit a `{tag, count}` aggregation from `note_tags`.
  - `--sources PATH` — resolve reverse-source references for a path.
  - `--since <timestamp>` — restrict to notes indexed since this time.
- **`changes --since <timestamp>`**: Return `{added, modified, deleted}` note lists from the event log.
- **`lint [check]`**: Surface graph health issues. Default/checks: `all`, `orphans`, `broken-links`, `dangling-refs`, `unreferenced`.
- **`status`**: Report active note count, chunk count, vector dimensions, scope summary, and readiness state.

## Scope flags

`search`, `recall`, `related`, `meta`, `changes`, and `lint` honor a shared scope-selection surface. Scopes are named in `~/.config/talon/config.toml` under `[scopes.<name>]` and have a `default = true|false` flag.

- **No flag**: query covers only scopes with `default = true`. Scopes with `default = false` (e.g. `private`, `archive`) are excluded from results entirely — they are not just down-ranked.
- **`-s NAME` / `--scope NAME`** (repeatable, additive): re-includes the named scope on top of the default pool. Required to surface any `default = false` scope.
- **`--scope-only NAME`** (repeatable): replaces the pool with the named scope(s); ignores `default`.
- **`--scope-all`**: covers every configured scope, overriding `default`.

The three forms are mutually exclusive on a single invocation. Unknown scope names error with the configured-name list. Each response's `meta.scope_set` reports the resolved active scope names so the caller can verify what was searched.

## Output flags

- `--json`: Pretty-printed JSON envelope.
- `--agent`: Compact JSON for token-efficient agent consumption.

## Examples

```bash
talon sync
talon sync --fast
talon search "zettelkasten atomic notes"
talon search "zettelkasten atomic notes" --fast
talon search "project alpha" --where status=active --since 2024-01-01
talon search "lease renewal" --scope private        # opt in to a default=false scope
talon search "fermented hot sauce" --scope-only wiki # search only the wiki scope
talon search "agenda" --scope-all                    # every configured scope
talon read notes/pkm/zettelkasten.md --raw
talon related notes/pkm/zettelkasten.md --depth 2 --direction both
talon meta --where status=archived --select title --select status
talon meta --tag-counts
talon changes --since 2024-01-01T00:00:00Z
talon lint
talon lint orphans
talon lint broken-links
talon status --json
talon --skill
```
