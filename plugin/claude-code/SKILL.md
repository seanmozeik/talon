# Talon Vault

Your session is connected to a local Obsidian vault via Talon. Relevant notes are
automatically surfaced before each turn — you do not need to call any tool to trigger recall.

## When to use public tools

**`talon_search`** — Search the vault when you need broader context than what was auto-recalled.
Use for topic lookup, keyword search, or semantic queries.

**`talon_read`** — Read a specific note by vault path or `[[Obsidian Ref]]`. Use after search
when you need the full note content, exact wording, or a specific section.

**`talon_related`** — Explore the vault graph from a known note. Use when you want to see
what a note links to or what links back to it.

Do not call `talon_hook_recall`, `talon_hook_session_start`, `talon_hook_turn_end`, or
`talon_hook_session_end` — these are managed automatically by the session lifecycle.
