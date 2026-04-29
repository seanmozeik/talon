---
name: talon
description: Agent-facing contract for Obsidian vault search, read, sync, recall, related, meta, changes, lint, and status.
---

# Talon

Use Talon to search and inspect an indexed Obsidian vault. Talon is optimized for agents: start with natural-language search, inspect compact navigation metadata, follow the graph when useful, then read exact notes or sections.

Always pass `--agent` on every command. The examples below are the pattern to follow.

## Default Search

Use normal search first:

```bash
talon --agent search "<natural language query>"
```

Search defaults to hybrid retrieval. It combines lexical matching, semantic/vector search, title and alias matching, query expansion, reranking, and scope-aware ranking when the configured sidecar is available. A good natural-language query is usually the highest-value call.

Only switch modes when there is a reason:

- `--mode fulltext` for exact wording, command names, IDs, or when semantic search is over-broad.
- `--mode title` when you are looking for a note by title or alias.
- `--mode semantic` when wording may differ strongly from the user's phrasing.
- `--fast` only when you explicitly need lexical-only speed and can skip embeddings/rerank.

## Optional Query Narrowing

Do not overuse tag and heading syntax. Add it only when the user gives an explicit tag/section constraint, or when an initial natural-language search is too broad.

- `#tag` or `tag:name` restricts results to notes with that tag.
- `heading:name` or `h:name` restricts results to notes with a matching indexed heading path.
- Scope is not query syntax; use scope flags.

```bash
talon --agent search "<natural language query> #<tag>"
talon --agent search "<natural language query> heading:<section>"
```

## Reading

Use `read` after search when you need source text, exact wording, or the body of a result.

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
- Use `related` for graph traversal from a known note.

```bash
talon --agent related "<vault/path.md>" --direction both --depth 1
talon --agent related "<vault/path.md>" --direction outgoing --depth 2
talon --agent related "<vault/path.md>" --direction backlinks --depth 1
```

## Recall

Use `recall` when you are about to answer a user and want Talon to supply compact vault context for that user request.

Pass the actual current user request, not a generic meta-prompt about what context might be useful.

```bash
talon --agent recall "<current user request>"
```

Recall returns active notes plus graph-neighborhood linked context when evidence is strong enough.

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

- `talon --agent sync`: full index and embedding sync. Use before judging semantic search quality.
- `talon --agent sync --fast`: lexical/index refresh only. Do not use when testing embeddings.
- `talon --agent meta --where <field><op><value> --select <field>`: inspect frontmatter metadata.
- `talon --agent meta --tag-counts`: inspect tag distribution.
- `talon --agent changes --since 7d`: inspect recent added/modified/deleted notes. `--since` accepts relative durations such as `7d`/`3h`, ISO 8601 timestamps, dates, or epoch milliseconds.
- `talon --agent lint broken-links`: inspect graph health.
- `talon --agent status`: inspect index readiness.

## Result Contract

Search hits include `path`, `title`, `snippet`, and `score`. They may also include:

- `isIndex`: index/overview page signal.
- `citations`: resolved notes from `sources:` frontmatter.
- `links`: outgoing resolved Obsidian links.
- `backlinks`: incoming resolved Obsidian links.
- `tags`: indexed tags.
- `aliases`: indexed aliases.

Read results include `path`, `title`, `content`, `links`, `backlinks`, `tags`, and `aliases`. Heading reads also include `section`.

Related results include `path`, `title`, `relation`, and `linkText`.
