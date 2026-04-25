---
name: talon
description: Obsidian vault search, read, sync (index + embed), and status for agents and CLI users.
---

# Talon

Use Talon for Obsidian vault **search**, **read**, **`sync`** (lexical index + embeddings), and **`status`**.

## Modes

- **`sync`**: Scan the vault, update the lexical/SQLite index, then run embeddings unless skipped.
- **`--fast`** / **`fast: true`** on **`sync`**: lexical/index pass only; **no** embeddings.
- **`--fast`** on **`search`**: lexical-only search; no expansion, no rerank.
- **`read`** returns note content or an excerpt.
- **`related`** walks wikilinks and backlinks from a starting note.
- **`status`** reports readiness.

## Examples

```bash
talon sync
talon sync --fast
talon search "zettelkasten atomic notes"
talon search "zettelkasten atomic notes" --fast
talon read notes/pkm/zettelkasten.md --raw
talon related notes/pkm/zettelkasten.md --depth 2 --direction both
talon status --json
talon --skill
```

