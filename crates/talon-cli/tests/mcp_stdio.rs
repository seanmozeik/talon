use std::io::Write as _;
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, bail};
use serde_json::Value;

#[test]
fn mcp_stdio_process_round_trips_lifecycle_and_tool_call() -> Result<()> {
    let config = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml");
    let mut child = Command::new(env!("CARGO_BIN_EXE_talon"))
        .args(["--config", config, "mcp"])
        // TALON_CONFIG_FILE lets MCP tool handlers resolve the same config.
        .env("TALON_CONFIG_FILE", config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let Some(mut stdin) = child.stdin.take() else {
        bail!("failed to open talon stdin");
    };
    stdin.write_all(
        concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"talon","arguments":{"action":"status","json":true}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#,
            "\n",
        )
        .as_bytes(),
    )?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "talon mcp exited with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let responses = parse_json_lines(&output.stdout)?;
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "talon");
    assert_eq!(responses[1]["id"], 2);
    let tool_names: Vec<&str> = responses[1]["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools should be an array"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or(""))
        .collect();
    for expected in &["talon_search", "talon_read", "talon_related"] {
        assert!(
            tool_names.contains(expected),
            "tools/list missing expected tool: {expected}"
        );
    }
    assert!(!tool_names.contains(&"talon"));
    assert!(!tool_names.contains(&"talon_ask"));
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(
        responses[2]["result"]["structuredContent"]["action"],
        "talon"
    );
    assert_eq!(responses[2]["result"]["structuredContent"]["ok"], false);
    assert_eq!(responses[2]["result"]["isError"], true);
    assert_eq!(responses[3]["id"], 4);
    assert_eq!(responses[3]["result"], Value::Null);
    Ok(())
}

/// Regression test: `talon_hook_recall` must not panic with "Cannot drop a runtime
/// in a context where blocking is not allowed" when `InferenceClient` is created
/// and dropped inside the synchronous JSON-RPC loop (which runs inside tokio).
#[test]
fn hook_recall_does_not_panic_inside_mcp_loop() -> Result<()> {
    let config = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/config.toml");
    let mut child = Command::new(env!("CARGO_BIN_EXE_talon"))
        .args(["--config", config, "mcp"])
        .env("TALON_CONFIG_FILE", config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let Some(mut stdin) = child.stdin.take() else {
        bail!("failed to open talon stdin");
    };
    stdin.write_all(
        concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#, "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"talon_hook_session_start","arguments":{"host":"claude-code","sessionId":"test","cwd":"/"}}}"#, "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"talon_hook_recall","arguments":{"host":"claude-code","sessionId":"test","turnId":"test:t1","message":"fermented hot sauce co-packer","budgetTokens":200,"format":"hook-json"}}}"#, "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#, "\n",
        )
        .as_bytes(),
    )?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "talon mcp exited with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let responses = parse_json_lines(&output.stdout)?;
    assert_eq!(
        responses.len(),
        4,
        "expected init + session_start + recall + shutdown responses"
    );
    // recall response must be present and contain hookSpecificOutput
    let recall_text = responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    let recall_output: Value = serde_json::from_str(recall_text)
        .unwrap_or_else(|e| panic!("recall result not valid JSON: {e}\n{recall_text}"));
    assert!(
        recall_output.get("hookSpecificOutput").is_some(),
        "recall result should contain hookSpecificOutput"
    );
    Ok(())
}

fn parse_json_lines(output: &[u8]) -> Result<Vec<Value>> {
    let stdout = std::str::from_utf8(output)?;
    stdout
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}
