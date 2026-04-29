mod dispatch;
mod error;
mod public;
mod schema;
mod status;
mod sync;

#[cfg(test)]
mod tests;

use serde::Deserialize;
use serde_json::{Value, json};
use talon_core::{ErrorCode, TalonEnvelope, TalonInput};

use self::error::ToolError;

const TOOL_NAME: &str = "talon";
// DEPRECATED: use talon_search, talon_read, or talon_related instead.
const TOOL_DESCRIPTION: &str = "DEPRECATED: use talon_search, talon_read, or talon_related instead. Run one stateless Talon action against the configured vault.";

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// Returns the MCP `tools/list` payload.
#[must_use]
pub fn tools_list_result() -> Value {
    let mut tools = vec![json!({
        "name": TOOL_NAME,
        "description": TOOL_DESCRIPTION,
        "inputSchema": schema::input_schema()
    })];
    tools.extend(public::tools_list_entries());
    json!({ "tools": tools })
}

/// Executes one MCP `tools/call` request.
#[must_use]
pub fn tools_call_result(params: Option<Value>) -> Value {
    // Parse params enough to extract name and arguments.
    let (name, arguments) = match parse_name_and_arguments(params) {
        Ok(pair) => pair,
        Err(error) => return content_result(&error.envelope()),
    };

    // Try named tools first.
    if let Some(result) = public::dispatch_named(&name, arguments.clone()) {
        let envelope = result.unwrap_or_else(ToolError::envelope);
        return public::named_content_result(&envelope);
    }

    // Fall through to legacy action-union tool.
    if name != TOOL_NAME {
        let error = ToolError::with_detail(
            "talon",
            ErrorCode::Internal,
            format!("unknown tool '{name}'"),
            json!({ "expected": TOOL_NAME }),
        );
        return content_result(&error.envelope());
    }

    let envelope = match dispatch_arguments(arguments) {
        Ok(envelope) => envelope,
        Err(error) => error.envelope(),
    };
    content_result(&envelope)
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

fn dispatch_arguments(arguments: Value) -> Result<TalonEnvelope, ToolError> {
    let action = action_from_arguments(&arguments);
    let input: TalonInput = serde_json::from_value(arguments).map_err(|error| {
        ToolError::with_detail(
            action.unwrap_or("talon"),
            ErrorCode::Internal,
            "invalid talon tool arguments",
            json!({ "message": error.to_string() }),
        )
    })?;
    let action = action_name(&input);
    dispatch::dispatch_input(input)
        .map_err(|error| ToolError::new(action, ErrorCode::Internal, format!("{error:#}")))
}

fn action_from_arguments(arguments: &Value) -> Option<&'static str> {
    let action = arguments.get("action")?.as_str()?;
    match action {
        "search" => Some("search"),
        "read" => Some("read"),
        "sync" => Some("sync"),
        "status" => Some("status"),
        "related" => Some("related"),
        "meta" => Some("meta"),
        "changes" => Some("changes"),
        "lint" => Some("lint"),
        "recall" => Some("recall"),
        _ => Some("talon"),
    }
}

const fn action_name(input: &TalonInput) -> &'static str {
    match input {
        TalonInput::Search(_) => "search",
        TalonInput::Read(_) => "read",
        TalonInput::Sync(_) => "sync",
        TalonInput::Status(_) => "status",
        TalonInput::Related(_) => "related",
        TalonInput::Meta(_) => "meta",
        TalonInput::Changes(_) => "changes",
        TalonInput::Lint(_) => "lint",
        TalonInput::Recall(_) => "recall",
    }
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
