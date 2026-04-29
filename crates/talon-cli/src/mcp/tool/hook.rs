use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use talon_core::RecallInput;

use crate::mcp::session::ledger::TurnLedger;
use crate::mcp::state::{HostKind, McpServerState, SessionKey, SessionState};

/// Returns the MCP `tools/list` entries for all hook-only tools.
///
/// These tools are intended for Claude Code hook use only and must not be
/// called by the model directly.
#[must_use]
pub fn hook_tools_list_entries() -> Vec<Value> {
    vec![
        json!({
            "name": "talon_hook_session_start",
            "description": "hook-only — not for model use. Called by Claude Code hooks when a new agent session begins. Registers the session in the MCP server state.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host":           { "type": "string" },
                    "sessionId":      { "type": "string" },
                    "cwd":            { "type": "string" },
                    "transcriptPath": { "type": "string" }
                },
                "required": []
            }
        }),
        json!({
            "name": "talon_hook_recall",
            "description": "hook-only — not for model use. Called by Claude Code hooks at UserPromptSubmit to inject vault recall context into the conversation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host":         { "type": "string" },
                    "sessionId":    { "type": "string" },
                    "turnId":       { "type": "string" },
                    "message":      { "type": "string" },
                    "budgetTokens": { "type": "integer" },
                    "format":       { "type": "string" },
                    "scope":        { "type": "array", "items": { "type": "string" } }
                },
                "required": []
            }
        }),
        json!({
            "name": "talon_hook_turn_end",
            "description": "hook-only — not for model use. Called by Claude Code hooks when a conversation turn completes. Updates session bookkeeping.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host":                 { "type": "string" },
                    "sessionId":            { "type": "string" },
                    "turnId":               { "type": "string" },
                    "outcome":              { "type": "string" },
                    "lastUserMessage":      { "type": "string" },
                    "lastAssistantMessage": { "type": "string" }
                },
                "required": []
            }
        }),
        json!({
            "name": "talon_hook_session_end",
            "description": "hook-only — not for model use. Called by Claude Code hooks when a session ends. Marks the session last-seen timestamp for TTL eviction.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host":      { "type": "string" },
                    "sessionId": { "type": "string" }
                },
                "required": []
            }
        }),
    ]
}

/// Dispatches a hook tool call.
///
/// Returns `Some(MCP tool result Value)` when `name` matches a hook tool,
/// `None` when it does not (allowing the caller to fall through to the
/// stateless public-tool dispatch).
pub fn dispatch_hook(name: &str, arguments: &Value, state: &Arc<McpServerState>) -> Option<Value> {
    match name {
        "talon_hook_session_start" => Some(handle_session_start(arguments, state)),
        "talon_hook_recall" => Some(handle_recall(arguments, state)),
        "talon_hook_turn_end" => Some(handle_turn_end(arguments, state)),
        "talon_hook_session_end" => Some(handle_session_end(arguments, state)),
        _ => None,
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

fn handle_session_start(arguments: &Value, state: &Arc<McpServerState>) -> Value {
    let host = string_field(arguments, "host").unwrap_or_else(|| "unknown".to_owned());
    let session_id = string_field(arguments, "sessionId").unwrap_or_default();

    let now_ms = now_ms();
    let key = SessionKey {
        host: parse_host_kind(&host),
        session_id,
    };
    let session = SessionState {
        created_at_ms: now_ms,
        last_seen_at_ms: now_ms,
        ledger: TurnLedger::new(),
        suppression_decay: crate::mcp::session::suppression::DEFAULT_DECAY,
        last_agent_response: None,
    };

    {
        let mut store = state
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.sessions.insert(key, session);
    }

    json!({ "content": [{ "type": "text", "text": "ok" }] })
}

fn handle_recall(arguments: &Value, state: &Arc<McpServerState>) -> Value {
    let host = string_field(arguments, "host").unwrap_or_else(|| "unknown".to_owned());
    let session_id = string_field(arguments, "sessionId").unwrap_or_default();
    let turn_id = string_field(arguments, "turnId").unwrap_or_else(|| "unknown".to_owned());
    let message = string_field(arguments, "message").unwrap_or_default();
    let budget_tokens = arguments
        .get("budgetTokens")
        .and_then(Value::as_u64)
        .map_or(500, |v| u32::try_from(v).unwrap_or(u32::MAX));
    let format = string_field(arguments, "format").unwrap_or_else(|| "hook-json".to_owned());
    let scope: Vec<String> = arguments
        .get("scope")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    // Enrich recall with the agent's last response so the query captures
    // conversation context, not just the current user message.
    let prior_messages = {
        let store = state
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store
            .sessions
            .get(&SessionKey {
                host: parse_host_kind(&host),
                session_id: session_id.clone(),
            })
            .and_then(|s| s.last_agent_response.clone())
            .into_iter()
            .collect::<Vec<_>>()
    };

    let input = RecallInput {
        message: message.clone(),
        prior_messages,
        budget_tokens,
        exclude: Vec::new(),
        scope,
        scope_only: Vec::new(),
        scope_all: false,
        format: talon_core::RecallFormat::default(),
        depth: 1,
        min_confidence: 0.0,
        fast: false,
    };

    let config = &state.config.config;
    let vault = config.vault_path.to_string_lossy().to_string();
    let result = super::hook_recall::dispatch_recall_for_hook(&input, config);

    let key = SessionKey {
        host: parse_host_kind(&host),
        session_id,
    };

    match result {
        Ok(recall_response) => {
            let filtered = super::hook_recall::apply_recall_suppression(
                recall_response,
                state,
                &key,
                &message,
                turn_id,
            );
            super::hook_recall::build_recall_output(&filtered, &format, &vault)
        }
        Err(err) => {
            touch_session(state, &key);
            let text = format!("{{\"error\":{err:?}}}");
            json!({ "content": [{ "type": "text", "text": text }] })
        }
    }
}

fn handle_turn_end(arguments: &Value, state: &Arc<McpServerState>) -> Value {
    let host = string_field(arguments, "host").unwrap_or_else(|| "unknown".to_owned());
    let session_id = string_field(arguments, "sessionId").unwrap_or_default();
    let last_assistant = string_field(arguments, "lastAssistantMessage");

    let key = SessionKey {
        host: parse_host_kind(&host),
        session_id,
    };

    {
        let mut store = state
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = store.sessions.get_mut(&key) {
            let now = now_ms();
            session.last_seen_at_ms = now;
            // Store the agent's response so the next recall call can use it
            // as prior context, enriching the query beyond the user message alone.
            if last_assistant.is_some() {
                session.last_agent_response = last_assistant;
            }
        }
    }

    json!({ "content": [{ "type": "text", "text": "{\"ok\":true}" }] })
}

fn handle_session_end(arguments: &Value, state: &Arc<McpServerState>) -> Value {
    let host = string_field(arguments, "host").unwrap_or_else(|| "unknown".to_owned());
    let session_id = string_field(arguments, "sessionId").unwrap_or_default();

    let key = SessionKey {
        host: parse_host_kind(&host),
        session_id,
    };
    touch_session(state, &key);

    json!({ "content": [{ "type": "text", "text": "ok" }] })
}

// ── Shared utilities ──────────────────────────────────────────────────────────

fn string_field(arguments: &Value, key: &str) -> Option<String> {
    arguments.get(key)?.as_str().map(str::to_owned)
}

fn parse_host_kind(host: &str) -> HostKind {
    match host {
        "claude-code" | "claudecode" | "ClaudeCode" => HostKind::ClaudeCode,
        "hermes" | "Hermes" => HostKind::Hermes,
        other => HostKind::Unknown(other.to_owned()),
    }
}

fn touch_session(state: &Arc<McpServerState>, key: &SessionKey) {
    let now = now_ms();
    let mut store = state
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(session) = store.sessions.get_mut(key) {
        session.last_seen_at_ms = now;
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::hook_tools_list_entries;

    #[test]
    fn hook_tools_list_entries_returns_four_hook_tools() {
        let entries = hook_tools_list_entries();
        assert_eq!(entries.len(), 4);
        for entry in &entries {
            let name = entry["name"].as_str().unwrap_or("");
            assert!(
                name.starts_with("talon_hook_"),
                "expected name to start with 'talon_hook_', got '{name}'"
            );
        }
    }

    #[test]
    fn hook_tool_descriptions_mention_hook_only() {
        let entries = hook_tools_list_entries();
        for entry in &entries {
            let desc = entry["description"].as_str().unwrap_or("");
            assert!(
                desc.contains("hook-only"),
                "expected description to contain 'hook-only', got: {desc:?}"
            );
        }
    }
}
