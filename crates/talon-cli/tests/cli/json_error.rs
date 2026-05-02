use super::assert_error_envelope;

#[test]
fn json_error_envelope_inspect_config_missing() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .args([
            "inspect",
            "orphans",
            "--json",
            "--config",
            "/nonexistent/path/config.toml",
        ])
        .output()
        .unwrap_or_else(|e| panic!("spawn talon: {e}"));
    assert!(!out.status.success(), "should exit nonzero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v = assert_error_envelope(&stdout, "inspect");
    assert!(
        v["error"]["message"].is_string(),
        "error.message should be a string"
    );
}

#[test]
fn json_error_envelope_search_config_missing() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .args([
            "search",
            "hello",
            "--fast",
            "--json",
            "--config",
            "/nonexistent/path/config.toml",
        ])
        .output()
        .unwrap_or_else(|e| panic!("spawn talon: {e}"));
    assert!(!out.status.success(), "should exit nonzero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v = assert_error_envelope(&stdout, "search");
    assert!(
        v["error"]["message"].is_string(),
        "error.message should be a string"
    );
}

#[test]
fn agent_error_output_wins_over_json_flag() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .args([
            "search",
            "hello",
            "--fast",
            "--agent",
            "--json",
            "--config",
            "/nonexistent/path/config.toml",
        ])
        .output()
        .unwrap_or_else(|e| panic!("spawn talon: {e}"));
    assert!(!out.status.success(), "should exit nonzero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\n') || stdout.ends_with('\n') && !stdout.trim_end().contains('\n'),
        "--agent output should be compact single-line JSON, got: {stdout}"
    );
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(v["code"].is_string(), "agent error should include code");
    assert!(
        v["message"].is_string(),
        "agent error should include message"
    );
    assert!(v.get("action").is_none(), "agent error should omit action");
    assert!(
        v.get("version").is_none(),
        "agent error should omit version"
    );
    assert!(v.get("ok").is_none(), "agent error should omit ok");
}
