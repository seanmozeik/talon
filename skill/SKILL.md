---
name: talon
description: Obsidian vault search, read, sync (index + embed), recall, related, meta, changes, lint, and status for agents and CLI users.
---

# Talon

Use Talon when you need to search or inspect an indexed Obsidian vault. It is built for agent workflows: search first, follow links/backlinks when the graph matters, then read the exact note or section you need.

Default output is human-readable. Use `--json` for the full JSON envelope and `--agent` for compact token-efficient JSON without the envelope.

## Core Commands

- **`sync`**: Scan the vault, update the SQLite index, and run embeddings. Use this before judging semantic search quality.
- **`sync --fast`**: Lexical/index pass only; skips embeddings. Do not use this when testing embedding retrieval.
- **`search "query"`**: Hybrid search. Modes: `hybrid` (default), `semantic`, `fulltext`, `title`.
- **`search --fast "query"`**: Lexical-only search; skips expansion, embeddings, and rerank.
- **`recall "message"`**: Retrieve compact context for an agent turn, including active notes and graph-neighborhood linked context.
- **`read <path-or-obsidian-ref>`**: Return note body with frontmatter stripped, plus links, backlinks, tags, and aliases.
- **`related <path>`**: Traverse resolved Obsidian wikilinks and backlinks. Use `--depth 1-3` and `--direction outgoing|backlinks|both`.
- **`meta`**: Filter/project frontmatter metadata and tag counts.
- **`changes --since <timestamp>`**: Return added, modified, and deleted notes.
- **`lint [check]`**: Surface graph health issues: `all`, `orphans`, `broken-links`, `dangling-refs`, `unreferenced`.
- **`status`**: Report index readiness, active note count, chunk count, vector dimensions, and scope summary.

## Search Syntax

Search accepts normal text plus lightweight Obsidian-native filters inside the query string:

- `#fermentation` or `tag:fermentation` restricts results to notes with that tag.
- `heading:Targets` or `h:Targets` restricts results to notes with a matching indexed heading path.
- Scope selection is not query syntax; use the scope flags below.

Examples:

```bash
talon --agent search "#fermentation hot sauce"
talon --agent search "tag:fermentation heading:Targets hot sauce"
talon --agent search "hot sauce" --mode fulltext
```

## Reading Notes

`read` accepts vault paths and Obsidian-style references:

- `talon --agent read wiki/Hot Sauce Formulation.md`
- `talon --agent read "[[Hot Sauce Formulation]]"`
- `talon --agent read "[[Hot Sauce Formulation#Search Boundaries]]"`
- `talon --agent read "Hot Sauce Formulation#Search Boundaries"`

When a heading is provided, Talon returns only that section. The result includes `section.heading`, `section.fromLine`, `section.toLine`, and `section.obsidianRef`.

Use `--raw` to keep frontmatter. Use `--from-line N --max-lines M` for line slicing.

## Scope Flags

`search`, `recall`, `related`, `meta`, `changes`, and `lint` honor the shared scope-selection surface. Scopes are named in `~/.config/talon/config.toml` under `[scopes.<name>]` and have `default = true|false`.

- **No flag**: query covers only scopes with `default = true`. Scopes with `default = false` are excluded entirely, not merely down-ranked.
- **`-s NAME` / `--scope NAME`**: re-include a named scope on top of the default pool. This is required to surface a `default = false` scope.
- **`--scope-only NAME`**: replace the pool with the named scope or scopes.
- **`--scope-all`**: cover every configured scope, overriding `default`.

These forms are mutually exclusive. Unknown scope names error with the configured-name list.

## Agent Output

Use `--agent` for compact JSON.

Search hits include:

- `path`, `title`, `snippet`, `score`
- `isIndex` for index/overview pages
- `citations` from resolved `sources:` frontmatter
- `links` for outgoing resolved Obsidian links
- `backlinks` for incoming resolved Obsidian links
- `tags` and `aliases` when present

Read results include:

- `path`, `title`, `content`
- `section` when reading a heading
- `links`, `backlinks`, `tags`, `aliases`

Related results include `path`, `title`, `relation`, and `linkText`.

Use `--json` when you need the full envelope metadata such as `meta.scope_set`, result `scope`, `mtime`, and related-result `count`.

## Useful Patterns

```bash
talon sync                                      # full index + embeddings
talon sync --fast                               # lexical-only refresh
talon --agent search "zettelkasten atomic notes"
talon --agent search "#fermentation hot sauce"
talon --agent search "heading:Targets hot sauce"
talon --agent search "lease renewal" --scope private
talon --agent search "fermented hot sauce" --scope-only wiki
talon --agent read "[[Hot Sauce Formulation#Search Boundaries]]"
talon --agent related "wiki/Hot Sauce Formulation.md" --depth 2 --direction both
talon --agent recall "what should I know before answering this?"
talon --json meta --where status=archived --select title --select status
talon --json meta --tag-counts
talon --json changes --since 2024-01-01T00:00:00Z
talon lint broken-links
talon status --json
talon --skill
```
