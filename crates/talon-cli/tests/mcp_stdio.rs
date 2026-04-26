use std::io::Write as _;
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, bail};
use serde_json::Value;

#[test]
fn mcp_stdio_process_round_trips_lifecycle_and_tool_call() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_talon"))
        .arg("--mcp")
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
            "talon --mcp exited with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let responses = parse_json_lines(&output.stdout)?;
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "talon");
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["result"]["tools"][0]["name"], "talon");
    let actions: Vec<&str> =
        responses[1]["result"]["tools"][0]["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("action enum should be an array"))
            .iter()
            .map(|v| v.as_str().unwrap_or(""))
            .collect();
    assert_eq!(
        actions.len(),
        8,
        "tools/list should advertise exactly 8 actions"
    );
    // Decision 10: embed is not a public MCP action; it runs inside talon sync.
    assert!(
        !actions.contains(&"embed"),
        "tools/list must not advertise an embed action (Decision 10)"
    );
    for expected in &[
        "search", "read", "sync", "status", "related", "meta", "changes", "lint",
    ] {
        assert!(
            actions.contains(expected),
            "tools/list missing expected action: {expected}"
        );
    }
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(
        responses[2]["result"]["structuredContent"]["action"],
        "status"
    );
    assert_eq!(responses[2]["result"]["structuredContent"]["ok"], true);
    assert_eq!(responses[3]["id"], 4);
    assert_eq!(responses[3]["result"], Value::Null);
    Ok(())
}

fn parse_json_lines(output: &[u8]) -> Result<Vec<Value>> {
    let stdout = std::str::from_utf8(output)?;
    stdout
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}
