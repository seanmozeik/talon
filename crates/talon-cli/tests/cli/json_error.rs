use super::assert_error_envelope;

#[test]
fn json_error_envelope_lint_config_missing() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .args([
            "lint",
            "orphans",
            "--json",
            "--config",
            "/nonexistent/path/config.toml",
        ])
        .output()
        .unwrap_or_else(|e| panic!("spawn talon: {e}"));
    assert!(!out.status.success(), "should exit nonzero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v = assert_error_envelope(&stdout, "lint");
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
    assert_error_envelope(&stdout, "search");
}
