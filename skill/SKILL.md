---
name: talon
description: Agent-facing contract for Obsidian vault search, ask, read, sync, recall, related, meta, changes, lint, and status.
---

# Talon

Use Talon to search and inspect an indexed Obsidian vault. Talon is optimized for agents: start with natural-language search, inspect compact navigation metadata, follow the graph when useful, then read exact notes or sections.

> **Claude Code**: vault recall is injected automatically before each turn — no recall tool call needed. Use the MCP tools below for explicit queries only.
>
> **MCP** exposes `talon_search`, `talon_read`, and `talon_related`. It intentionally does not expose `talon_ask`; agents should use search/read and synthesize with their own model. If you truly need Talon's built-in synthesis, call the CLI.

Always pass `--agent` on every command (unless you specifically need metadata that only `--json` provides). The examples below are the pattern to follow.

## Default Search

Use normal search first:

```bash
talon --agent search "<natural language query>"
```

Search defaults to hybrid retrieval. It combines lexical matching, semantic/vector search, title and alias matching, query expansion, reranking, graph-refined ranking, and scope-aware ranking when the configured sidecar is available. A good natural-language query is usually the highest-value call.

Only switch modes when there is a reason:

- `--mode fulltext` for exact wording, command names, IDs, or when semantic search is over-broad.
- `--mode title` when you are looking for a note by title or alias.
- `--mode semantic` when wording may differ strongly from the user's phrasing.
- `--fast` only when you explicitly need lexical-only speed and can skip embeddings/rerank.
- `-n <count>` to cap result count.

## Optional Query Narrowing

Do not overuse tag and heading syntax. Add it only when the user gives an explicit tag/section constraint, or when an initial natural-language search is too broad.

- `#tag` or `tag:name` restricts results to notes with that tag.
- `heading:name` or `h:name` restricts results to notes with a matching indexed heading path.
- Scope is not query syntax; use scope flags.

```bash
talon --agent search "<natural language query> #<tag>"
talon --agent search "<natural language query> heading:<section>"
```

## Ask (CLI Fallback)

Use `ask` sparingly. It is mainly a human CLI convenience for quick vault-grounded answers:

```bash
talon --agent ask "<broad vault question>"
```

For agents, prefer `search` plus `read`: you can plan searches and synthesize better with your own model. `ask` is useful only when you need Talon to do the synthesis in one CLI call.

## Reading

Use `read` after search when you need source text, exact wording, or the body of a result.
You may also read the files directly with your file read tool.

`read` accepts vault paths and Obsidian references:

```bash
talon --agent read "<vault/path.md>"
talon --agent read "[[Note Title]]"
talon --agent read "[[Note Title#Heading]]"
talon --agent read "Note Title#Heading"
```

When a heading is provided, Talon returns only that section. The result includes `section.heading`, `section.fromLine`, `section.toLine`, and `section.obsidianRef`.

Use `--raw` only when frontmatter must be preserved. Use `--from-line N --max-lines M` for line slicing.

## Graph Navigation

Search and read results expose resolved Obsidian graph metadata. Use it instead of scraping markdown links.

- Search hits may include `citations`, `links`, `backlinks`, `tags`, and `aliases`.
- Read results include `links`, `backlinks`, `tags`, and `aliases`.
- Use `related` for ranked graph/provenance exploration from a known note. Related results use links, backlinks, shared sources, common neighbors, communities, and bridge signals; they are not raw traversal order.

```bash
talon --agent related "<vault/path.md>" --direction both --depth 1
talon --agent related "<vault/path.md>" --direction outgoing --depth 2
talon --agent related "<vault/path.md>" --direction backlinks --depth 1
```

## Recall (CLI Use Only)

In CLI contexts, use `recall` when you are about to answer a user and want Talon to supply compact vault context for that user request.

Pass the actual current user request, not a generic meta-prompt about what context might be useful.

```bash
talon --agent recall "<current user request>"
```

Recall returns active notes plus community-diverse graph-ranked linked context when evidence is strong enough.

**Note:** MCP users get automatic recall injection before each turn — no manual call needed.

## Scope Flags

Configured scopes decide which parts of the vault are searched by default. Scopes with `default = false` are excluded entirely unless explicitly included.

- No scope flag: search only default scopes.
- `--scope NAME`: include one additional named scope.
- `--scope-only NAME`: search only the named scope or scopes.
- `--scope-all`: include every configured scope.

Use scope flags when the user explicitly asks for a private/archive/raw area or when a default-scope search misses something you have reason to believe is outside the default pool.

```bash
talon --agent search "<query>" --scope <scope>
talon --agent search "<query>" --scope-only <scope>
talon --agent search "<query>" --scope-all
```

## Other Commands

- `talon --agent sync`: incremental index refresh, stale path cleanup, and pending/changed embeddings. Picks up changed files, deletes, moves, renames, and changed links in edited files.
- `talon --agent sync --fast`: same index refresh and stale path cleanup, with no embedding pass. Use for quick lexical freshness checks.
- `talon --agent sync --force`: incremental index refresh, then rebuild embeddings for every active chunk.
- `talon --agent sync --rebuild`: delete and recreate the SQLite index, then index the vault from scratch. Add global `--fast` for a lexical-only rebuild.
- `talon --agent meta --tag-counts`: inspect tag distribution.
- `talon --agent changes --since 7d`: inspect recent added/modified/deleted notes. `--since` accepts relative durations such as `7d`/`3h`, ISO 8601 timestamps, dates, or epoch milliseconds.
- `talon --agent lint`: inspect broken links, orphan/unreferenced notes, graph health, and read-only missing-link opportunities.
- `talon --agent lint graph`: inspect only graph-health and read-only missing-link findings; it does not edit files.
- `talon --agent status`: inspect index readiness.

Note: when MCP is active, the vault syncs in the background - no action needed.

### `--where` Filters (meta and search)

Filter by frontmatter fields or the note's vault path. Operators glue to the key — no spaces required.

| Operator          | Syntax                   | Meaning                            |
| ----------------- | ------------------------ | ---------------------------------- |
| `=`               | `status=archived`        | Exact match                        |
| `!=`              | `type!=method`           | Not equal                          |
| `<` `<=` `>` `>=` | `score>0.5`              | Ordered (dates, numbers)           |
| `contains`        | `tags contains workflow` | Substring in any value             |
| `exists`          | `source exists`          | Field is present                   |
| `^=`              | `path^=Templates/`       | Prefix / starts-with               |
| `~=`              | `path~=Patients/*`       | Glob pattern (full globset syntax) |

Prefix and glob work on any frontmatter field **and** the special `path` key:

```bash
# Show all notes under a directory subtree
talon --agent meta --where 'path^=Projects/' --select path

# Glob: all markdown files in artifacts/
talon --agent meta --where 'path~=artifacts/*.md' --select path

# Prefix on frontmatter field
talon --agent meta --where 'title^=Weekly' --select path,title

# Glob on frontmatter field
talon --agent meta --where 'tags~=workflow*' --select path,tags
```

Glob uses the [`globset`](https://docs.rs/globset) crate: `*` matches any chars (including `/`), `**` matches zero or more directories.

## Result Contract

Search hits with `--agent` include `path`, `title`, `snippet`, and `score`.
To see more metadata, pass `--json` instead:

- `isIndex`: index/overview page signal.
- `citations`: resolved notes from `sources:` frontmatter.
- `links`: outgoing resolved Obsidian links.
- `backlinks`: incoming resolved Obsidian links.
- `tags`: indexed tags.
- `aliases`: indexed aliases.

Read results include `path`, `title`, `content`, `links`, `backlinks`, `tags`, and `aliases`. Heading reads also include `section`.

Related results include `path`, `title`, `relation`, `linkText`, rounded `score`, and compact `reasons`.

Ask results include `answer`, `queries`, and `sources`. Sources use plain vault paths; they are not Obsidian wikilinks.
