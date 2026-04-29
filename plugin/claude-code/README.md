# Talon Claude Code Plugin

Automatically injects relevant vault notes as context before each Claude Code turn.

## Installation

1. **Install talon** and ensure `talon mcp` works:
   ```
   talon status
   ```

2. **Add MCP server** — copy or merge `mcp.json` into your project or user `.mcp.json`:
   ```
   cp plugin/claude-code/mcp.json .mcp.json
   ```

3. **Add hooks** — copy or merge `hooks.json` into your Claude Code settings:
   ```
   # Project-level (recommended):
   mkdir -p .claude
   cp plugin/claude-code/hooks.json .claude/hooks.json

   # Or user-level:
   cp plugin/claude-code/hooks.json ~/.claude/hooks.json
   ```

4. **Add skill** (optional) — generate the agent contract from the binary into your project `.claude/`:
   ```
   mkdir -p .claude
   talon --skill > .claude/SKILL.md
   ```
   This writes the canonical skill directly from the installed binary — no static copy to go stale.

5. **Restart Claude Code**. On the next session start, `talon mcp` will launch and
   vault context will be injected automatically before each prompt.

## Verification

After restarting, run a prompt that relates to content in your vault. Check the
`SessionStart` hook output (visible in Claude Code verbose mode) — it should show
`talon_hook_session_start` succeeded.

## Notes

- Recall is silent — injected context appears in Claude's context window but is not
  shown as a separate message.
- Duplicate context from prior turns is suppressed automatically.
- The `talon_search`, `talon_read`, and `talon_related` tools are available for
  explicit vault queries if auto-recall is insufficient.
