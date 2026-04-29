use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::tool;

pub const JSONRPC_VERSION: &str = "2.0";

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    #[must_use]
    pub const fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodDisposition {
    Continue,
    Shutdown,
}

#[must_use]
pub fn parse_error(data: Value) -> JsonRpcResponse {
    error_response(Value::Null, PARSE_ERROR, "parse error", Some(data))
}

#[must_use]
pub fn handle_request(request: JsonRpcRequest) -> (Option<JsonRpcResponse>, MethodDisposition) {
    if request.jsonrpc != JSONRPC_VERSION {
        let id = request.id.unwrap_or(Value::Null);
        return (
            Some(error_response(id, INVALID_REQUEST, "invalid request", None)),
            MethodDisposition::Continue,
        );
    }

    match request.method.as_str() {
        "initialize" => {
            respond_to_request(request, initialize_result(), MethodDisposition::Continue)
        }
        "notifications/initialized" | "initialized" => (None, MethodDisposition::Continue),
        "tools/list" => respond_to_request(
            request,
            tool::tools_list_result(),
            MethodDisposition::Continue,
        ),
        "tools/call" => {
            let params = request.params.clone();
            respond_to_request(
                request,
                tool::tools_call_result(params),
                MethodDisposition::Continue,
            )
        }
        "shutdown" => respond_to_request(request, Value::Null, MethodDisposition::Shutdown),
        _ => {
            if request.is_notification() {
                (None, MethodDisposition::Continue)
            } else {
                let id = request.id.unwrap_or(Value::Null);
                (
                    Some(error_response(
                        id,
                        METHOD_NOT_FOUND,
                        "method not found",
                        None,
                    )),
                    MethodDisposition::Continue,
                )
            }
        }
    }
}

/// State-aware variant of [`handle_request`].
///
/// For `tools/call` requests, delegates to
/// [`tool::tools_call_result_with_state`] so that hook tools can access
/// session state.  All other methods behave identically to [`handle_request`].
#[must_use]
pub fn handle_request_with_state(
    request: JsonRpcRequest,
    state: &std::sync::Arc<crate::mcp::state::McpServerState>,
) -> (Option<JsonRpcResponse>, MethodDisposition) {
    if request.jsonrpc != JSONRPC_VERSION {
        let id = request.id.unwrap_or(Value::Null);
        return (
            Some(error_response(id, INVALID_REQUEST, "invalid request", None)),
            MethodDisposition::Continue,
        );
    }

    match request.method.as_str() {
        "initialize" => {
            respond_to_request(request, initialize_result(), MethodDisposition::Continue)
        }
        "notifications/initialized" | "initialized" => (None, MethodDisposition::Continue),
        "tools/list" => respond_to_request(
            request,
            tool::tools_list_result(),
            MethodDisposition::Continue,
        ),
        "tools/call" => {
            let params = request.params.clone();
            respond_to_request(
                request,
                tool::tools_call_result_with_state(params, state),
                MethodDisposition::Continue,
            )
        }
        "shutdown" => respond_to_request(request, Value::Null, MethodDisposition::Shutdown),
        _ => {
            if request.is_notification() {
                (None, MethodDisposition::Continue)
            } else {
                let id = request.id.unwrap_or(Value::Null);
                (
                    Some(error_response(
                        id,
                        METHOD_NOT_FOUND,
                        "method not found",
                        None,
                    )),
                    MethodDisposition::Continue,
                )
            }
        }
    }
}

fn respond_to_request(
    request: JsonRpcRequest,
    result: Value,
    disposition: MethodDisposition,
) -> (Option<JsonRpcResponse>, MethodDisposition) {
    if request.is_notification() {
        (None, disposition)
    } else {
        (
            Some(JsonRpcResponse {
                jsonrpc: JSONRPC_VERSION,
                id: request.id.unwrap_or(Value::Null),
                result: Some(result),
                error: None,
            }),
            disposition,
        )
    }
}

fn error_response(id: Value, code: i32, message: &str, data: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION,
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_owned(),
            data,
        }),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "talon",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{JsonRpcRequest, MethodDisposition, handle_request};
    use color_eyre::eyre::Result;
    use serde_json::{Value, json};

    #[test]
    fn handle_request_returns_initialize_response_when_request_has_id() -> Result<()> {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }))?;

        let (response, disposition) = handle_request(request);
        let response = serde_json::to_value(response)?;

        assert_eq!(disposition, MethodDisposition::Continue);
        assert_eq!(response["result"]["serverInfo"]["name"], "talon");
        assert_eq!(response["id"], 1);
        Ok(())
    }

    #[test]
    fn handle_request_suppresses_response_for_initialized_notification() -> Result<()> {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))?;

        let (response, disposition) = handle_request(request);

        assert!(response.is_none());
        assert_eq!(disposition, MethodDisposition::Continue);
        Ok(())
    }

    #[test]
    fn handle_request_marks_shutdown_after_response() -> Result<()> {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": "stop",
            "method": "shutdown"
        }))?;

        let (response, disposition) = handle_request(request);
        let response = serde_json::to_value(response)?;

        assert_eq!(disposition, MethodDisposition::Shutdown);
        assert_eq!(response["id"], Value::String("stop".to_owned()));
        Ok(())
    }

    #[test]
    fn handle_request_rejects_generic_talon_tool_call() -> Result<()> {
        let request: JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": "call",
            "method": "tools/call",
            "params": {
                "name": "talon",
                "arguments": { "action": "status" }
            }
        }))?;

        let (response, disposition) = handle_request(request);
        let response = serde_json::to_value(response)?;

        assert_eq!(disposition, MethodDisposition::Continue);
        assert_eq!(response["id"], "call");
        assert_eq!(response["result"]["structuredContent"]["action"], "talon");
        assert_eq!(response["result"]["structuredContent"]["ok"], false);
        assert_eq!(response["result"]["isError"], true);
        Ok(())
    }
}
