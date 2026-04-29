# Talon Vault

Your session is connected to a local Obsidian vault via Talon. Relevant notes are
automatically surfaced before each turn via hooks — you do not need to call any tool to trigger recall.

## Automatic Recall

Vault recall is **automatic**: Claude Code injects relevant vault context before every prompt invisibly via the `UserPromptSubmit` hook. The injected context is:

- **Deduplicated across turns**: The same context is not injected twice in adjacent turns.
- **Enriched by prior responses**: Your previous assistant response is used to enrich the next recall query, so vault context stays relevant through conversation turns.

Calling recall manually is unnecessary and may produce duplicate context.

## When to use public tools

Use the three public tools for explicit queries beyond what auto-recall already surfaces:

**`talon_search`** — Search the vault when you need broader context than what was auto-recalled.
Use for topic lookup, keyword search, or semantic queries beyond the scope of the automatic injection.

**`talon_read`** — Read a specific note by vault path or `[[Obsidian Ref]]`. Use after search
when you need the full note content, exact wording, or a specific section body.

**`talon_related`** — Explore the vault graph from a known note. Use when you want to see
what a note links to or what links back to it.

## Hook-only tools

Do not call `talon_hook_recall`, `talon_hook_session_start`, `talon_hook_turn_end`, or
`talon_hook_session_end` — these are managed automatically by the session lifecycle and called by Claude Code hooks.
