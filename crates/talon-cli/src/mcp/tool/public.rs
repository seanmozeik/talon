use serde_json::{Value, json};
use talon_core::{ErrorCode, TalonEnvelope, TalonInput};

use crate::agent_contract;
use crate::output;

use super::dispatch;
use super::error::ToolError;

/// Returns `tools/list` entries for all named public tools.
pub(super) fn tools_list_entries() -> Vec<Value> {
    vec![
        json!({
            "name": agent_contract::SEARCH.name,
            "description": agent_contract::SEARCH.description,
            "inputSchema": search_input_schema()
        }),
        json!({
            "name": agent_contract::READ.name,
            "description": agent_contract::READ.description,
            "inputSchema": read_input_schema()
        }),
        json!({
            "name": agent_contract::RELATED.name,
            "description": agent_contract::RELATED.description,
            "inputSchema": related_input_schema()
        }),
    ]
}

/// Dispatches to a named tool if `name` matches; returns `None` if unknown.
pub(super) fn dispatch_named(
    name: &str,
    arguments: Value,
) -> Option<Result<TalonEnvelope, ToolError>> {
    match name {
        n if n == agent_contract::SEARCH.name => Some(dispatch_search(arguments)),
        n if n == agent_contract::READ.name => Some(dispatch_read(arguments)),
        n if n == agent_contract::RELATED.name => Some(dispatch_related(arguments)),
        _ => None,
    }
}

fn dispatch_search(arguments: Value) -> Result<TalonEnvelope, ToolError> {
    // Map named tool fields to action-union shape
    let mut args = arguments;
    inject_action(&mut args, "search");
    dispatch_input(agent_contract::SEARCH.name, args)
}

fn dispatch_read(arguments: Value) -> Result<TalonEnvelope, ToolError> {
    let mut args = arguments;
    inject_action(&mut args, "read");
    dispatch_input(agent_contract::READ.name, args)
}

fn dispatch_related(arguments: Value) -> Result<TalonEnvelope, ToolError> {
    let mut args = arguments;
    inject_action(&mut args, "related");
    dispatch_input(agent_contract::RELATED.name, args)
}

fn inject_action(arguments: &mut Value, action: &'static str) {
    if let Some(obj) = arguments.as_object_mut() {
        obj.insert("action".to_owned(), Value::String(action.to_owned()));
    }
}

fn dispatch_input(tool_name: &'static str, arguments: Value) -> Result<TalonEnvelope, ToolError> {
    let input: TalonInput = serde_json::from_value(arguments).map_err(|e| {
        ToolError::with_detail(
            tool_name,
            ErrorCode::Internal,
            "invalid tool arguments",
            json!({ "message": e.to_string() }),
        )
    })?;
    dispatch::dispatch_input(input)
        .map_err(|e| ToolError::new(tool_name, ErrorCode::Internal, format!("{e:#}")))
}

/// Builds the MCP `tools/call` result for a named tool envelope.
pub(super) fn named_content_result(envelope: &TalonEnvelope) -> Value {
    let text = output::json::agent::to_agent_value(envelope)
        .and_then(|v| serde_json::to_string(&v).ok())
        .unwrap_or_else(|| serde_json::to_string(envelope).unwrap_or_default());
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "isError": !envelope.ok,
        "structuredContent": envelope
    })
}

// ── Input schemas ─────────────────────────────────────────────────────────────

fn search_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "scope": { "type": "array", "items": { "type": "string" } },
            "mode": { "type": "string", "enum": ["hybrid", "semantic", "fulltext", "title"] },
            "limit": { "type": "integer", "default": 10 },
            "includeSnippets": { "type": "boolean", "default": true }
        },
        "required": ["query"]
    })
}

fn read_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" },
            "fromLine": { "type": "integer" },
            "maxLines": { "type": "integer" }
        },
        "required": ["path"]
    })
}

fn related_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" },
            "direction": { "type": "string", "enum": ["outgoing", "backlinks", "both"] },
            "depth": { "type": "integer", "default": 1 },
            "limit": { "type": "integer", "default": 10 }
        },
        "required": ["path"]
    })
}
