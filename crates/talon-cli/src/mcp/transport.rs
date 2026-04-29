use std::io::{BufRead, Write};
use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr};
use serde_json::json;

use crate::mcp::protocol::{
    JsonRpcRequest, MethodDisposition, handle_request, handle_request_with_state, parse_error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportOutcome {
    Eof,
    Shutdown,
}

/// Runs a line-delimited JSON-RPC 2.0 loop over the supplied streams.
///
/// Each non-empty input line is one complete JSON-RPC frame. Responses are
/// written as one JSON object per line; notifications produce no output.
///
/// # Errors
///
/// Returns an error if reading from the input stream or writing to the output
/// stream fails.
pub fn run_jsonrpc_loop<R, W>(mut reader: R, mut writer: W) -> Result<TransportOutcome>
where
    R: BufRead,
    W: Write,
{
    let mut frame = String::new();

    loop {
        frame.clear();
        let bytes_read = reader
            .read_line(&mut frame)
            .wrap_err("failed to read MCP frame")?;
        if bytes_read == 0 {
            return Ok(TransportOutcome::Eof);
        }

        let trimmed = frame.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(request) => {
                let (response, disposition) = handle_request(request);
                if let Some(response) = response {
                    write_response(&mut writer, &response)?;
                }
                if disposition == MethodDisposition::Shutdown {
                    return Ok(TransportOutcome::Shutdown);
                }
                continue;
            }
            Err(error) => parse_error(json!({ "message": error.to_string() })),
        };

        write_response(&mut writer, &response)?;
    }
}

/// Runs a line-delimited JSON-RPC 2.0 loop with access to process-local server
/// state, enabling hook-only tools to read and update session state.
///
/// For `tools/call` requests the state-aware handler is used; all other
/// methods behave identically to [`run_jsonrpc_loop`].
///
/// # Errors
///
/// Returns an error if reading from the input stream or writing to the output
/// stream fails. See [`run_jsonrpc_loop`] for the full error contract.
pub fn run_jsonrpc_loop_with_state<R, W>(
    mut reader: R,
    mut writer: W,
    state: &Arc<crate::mcp::state::McpServerState>,
) -> Result<TransportOutcome>
where
    R: BufRead,
    W: Write,
{
    let mut frame = String::new();

    loop {
        frame.clear();
        let bytes_read = reader
            .read_line(&mut frame)
            .wrap_err("failed to read MCP frame")?;
        if bytes_read == 0 {
            return Ok(TransportOutcome::Eof);
        }

        let trimmed = frame.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(request) => {
                let (response, disposition) = handle_request_with_state(request, state);
                if let Some(response) = response {
                    write_response(&mut writer, &response)?;
                }
                if disposition == MethodDisposition::Shutdown {
                    return Ok(TransportOutcome::Shutdown);
                }
                continue;
            }
            Err(error) => parse_error(json!({ "message": error.to_string() })),
        };

        write_response(&mut writer, &response)?;
    }
}

fn write_response<W, T>(writer: &mut W, response: &T) -> Result<()>
where
    W: Write,
    T: serde::Serialize,
{
    serde_json::to_writer(&mut *writer, response).wrap_err("failed to encode MCP response")?;
    writer
        .write_all(b"\n")
        .wrap_err("failed to write MCP response")?;
    writer.flush().wrap_err("failed to flush MCP response")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use color_eyre::eyre::Result;
    use serde_json::Value;

    use super::{TransportOutcome, run_jsonrpc_loop};

    #[test]
    fn run_jsonrpc_loop_round_trips_handshake_and_shutdown() -> Result<()> {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":"tools","method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#,
            "\n",
        );
        let mut output = Vec::new();

        let outcome = run_jsonrpc_loop(Cursor::new(input), &mut output)?;

        let responses = parse_output_lines(&output)?;
        assert_eq!(outcome, TransportOutcome::Shutdown);
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], "talon");
        assert_eq!(responses[1]["id"], "tools");
        let tool_names: Vec<&str> = responses[1]["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("tools should be an array"))
            .iter()
            .map(|tool| tool["name"].as_str().unwrap_or(""))
            .collect();
        assert!(tool_names.contains(&"talon_search"));
        assert!(tool_names.contains(&"talon_read"));
        assert!(tool_names.contains(&"talon_related"));
        assert!(!tool_names.contains(&"talon"));
        assert_eq!(responses[2]["id"], 2);
        Ok(())
    }

    #[test]
    fn run_jsonrpc_loop_returns_parse_error_for_malformed_frame() -> Result<()> {
        let input = "{not-json}\n";
        let mut output = Vec::new();

        let outcome = run_jsonrpc_loop(Cursor::new(input), &mut output)?;

        let responses = parse_output_lines(&output)?;
        assert_eq!(outcome, TransportOutcome::Eof);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["id"], Value::Null);
        assert_eq!(responses[0]["error"]["code"], -32700);
        Ok(())
    }

    fn parse_output_lines(output: &[u8]) -> Result<Vec<Value>> {
        let text = std::str::from_utf8(output)?;
        text.lines()
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
    }
}
