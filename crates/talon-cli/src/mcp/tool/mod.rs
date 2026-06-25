mod dispatch;
mod error;
pub(super) mod hook;
pub(crate) mod hook_recall;
mod public;
mod status;
mod sync;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use talon_core::{ErrorCode, TalonEnvelope};

use self::error::ToolError;

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// Returns the MCP `tools/list` payload.
#[must_use]
pub fn tools_list_result() -> Value {
    let mut tools = public::tools_list_entries();
    tools.extend(hook::hook_tools_list_entries());
    json!({ "tools": tools })
}

/// Executes one MCP `tools/call` request, with access to session state for
/// hook-only tools.
#[must_use]
pub fn tools_call_result_with_state(
    params: Option<Value>,
    state: &Arc<crate::mcp::state::McpServerState>,
) -> Value {
    let (name, arguments) = match parse_name_and_arguments(params) {
        Ok(pair) => pair,
        Err(error) => return content_result(&error.envelope()),
    };

    // Try hook tools first — they require state and produce hook-formatted output.
    if let Some(result) = hook::dispatch_hook(&name, &arguments, state) {
        return result;
    }

    // Try named public tools next.
    if let Some(result) = public::dispatch_named(&name, arguments) {
        let envelope = result.unwrap_or_else(ToolError::envelope);
        return public::named_content_result(&envelope);
    }

    content_result(&unknown_tool_error(&name).envelope())
}

/// Executes one MCP `tools/call` request.
#[must_use]
pub fn tools_call_result(params: Option<Value>) -> Value {
    // Parse params enough to extract name and arguments.
    let (name, arguments) = match parse_name_and_arguments(params) {
        Ok(pair) => pair,
        Err(error) => return content_result(&error.envelope()),
    };

    // Try named tools.
    if let Some(result) = public::dispatch_named(&name, arguments) {
        let envelope = result.unwrap_or_else(ToolError::envelope);
        return public::named_content_result(&envelope);
    }

    content_result(&unknown_tool_error(&name).envelope())
}

fn parse_name_and_arguments(params: Option<Value>) -> Result<(String, Value), ToolError> {
    let params = params.ok_or_else(|| {
        ToolError::new(
            "talon",
            ErrorCode::Internal,
            "tools/call requires params with name and arguments",
        )
    })?;
    let call: ToolCallParams = serde_json::from_value(params).map_err(|error| {
        ToolError::with_detail(
            "talon",
            ErrorCode::Internal,
            "invalid tools/call params",
            json!({ "message": error.to_string() }),
        )
    })?;
    Ok((call.name, call.arguments))
}

fn unknown_tool_error(name: &str) -> ToolError {
    ToolError::with_detail(
        "talon",
        ErrorCode::Internal,
        format!("unknown tool '{name}'"),
        json!({ "expected": ["talon_search", "talon_read", "talon_related"] }),
    )
}

fn content_result(envelope: &TalonEnvelope) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string(envelope).unwrap_or_else(|_| "{}".to_owned())
            }
        ],
        "isError": !envelope.ok,
        "structuredContent": envelope
    })
}

#[must_use]
pub fn panic_tool_result() -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": "{\"error\":\"talon MCP tool handler panicked; see talon status for diagnostics\"}"
            }
        ],
        "isError": true
    })
}
