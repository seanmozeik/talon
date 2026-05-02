use super::{TempVault, assert_success_envelope};

#[test]
fn json_envelope_status_success() {
    let vault = TempVault::new("status");
    let out = vault.run(&["status"]);
    assert!(out.status.success(), "talon status should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v = assert_success_envelope(&stdout, "status");
    assert_eq!(v["data"]["state"], "ready");
}

#[test]
fn json_envelope_sync_success() {
    let vault = TempVault::new("sync");
    let out = vault.run(&["sync", "--fast"]);
    assert!(out.status.success(), "talon sync should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "sync");
}

#[test]
fn agent_sync_omits_envelope_metadata() {
    let vault = TempVault::new("agent-sync");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .args(["sync", "--fast", "--agent", "--config"])
        .arg(&vault.config_path)
        .output()
        .unwrap_or_else(|e| panic!("spawn talon: {e}"));
    assert!(out.status.success(), "talon sync should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(
        v.get("indexed").is_some(),
        "agent sync should include indexed count"
    );
    assert!(v.get("action").is_none(), "agent sync should omit action");
    assert!(v.get("version").is_none(), "agent sync should omit version");
    assert!(v.get("ok").is_none(), "agent sync should omit ok");
    assert!(v.get("meta").is_none(), "agent sync should omit meta");
    assert!(
        v.get("durationMs").is_none(),
        "agent sync should omit duration metadata"
    );
}

#[test]
fn agent_outputs_omit_envelope_metadata_for_query_commands() {
    let vault = TempVault::new("agent-commands");
    for args in [
        vec!["status"],
        vec!["search", "hello", "--fast"],
        vec!["read", "notes/hello.md"],
        vec!["related", "notes/hello.md"],
        vec!["meta"],
        vec!["changes", "--since", "2020-01-01T00:00:00Z"],
        vec!["inspect"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
            .args(&args)
            .arg("--agent")
            .arg("--config")
            .arg(&vault.config_path)
            .output()
            .unwrap_or_else(|e| panic!("spawn talon {args:?}: {e}"));
        assert!(out.status.success(), "talon {args:?} should exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: serde_json::Value =
            serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
        assert!(v.get("action").is_none(), "{args:?} should omit action");
        assert!(v.get("version").is_none(), "{args:?} should omit version");
        assert!(v.get("ok").is_none(), "{args:?} should omit ok");
        assert!(v.get("meta").is_none(), "{args:?} should omit meta");
    }
}

#[test]
fn agent_lint_groups_findings_by_check() {
    let vault = TempVault::new("agent-inspect");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .args(["inspect", "--agent", "--config"])
        .arg(&vault.config_path)
        .output()
        .unwrap_or_else(|e| panic!("spawn talon inspect: {e}"));
    assert!(out.status.success(), "talon inspect should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert_eq!(v["total"], 3);
    assert_eq!(v["checks"]["orphans"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        v["checks"]["unreferenced"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(v["checks"]["graph"].as_array().map(Vec::len), Some(1));
    assert!(v.get("meta").is_none());
}

#[test]
fn json_envelope_search_success() {
    let vault = TempVault::new("search");
    let out = vault.run(&["search", "hello", "--fast"]);
    assert!(out.status.success(), "talon search should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "search");
}

#[test]
fn search_accepts_options_after_query() {
    let vault = TempVault::new("search-trailing-options");
    let out = vault.run(&[
        "search",
        "hello",
        "--fast",
        "--mode",
        "fulltext",
        "-n",
        "1",
        "--anchors",
    ]);
    assert!(out.status.success(), "talon search should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v = assert_success_envelope(&stdout, "search");
    assert_eq!(v["data"]["fast"], true);
    assert_eq!(v["data"]["mode"], "fulltext");
    assert_eq!(v["data"]["results"].as_array().map(Vec::len), Some(1));
}

#[test]
fn json_envelope_read_success() {
    let vault = TempVault::new("read");
    let out = vault.run(&["read", "notes/hello.md"]);
    assert!(out.status.success(), "talon read should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "read");
}

#[test]
fn json_envelope_related_success() {
    let vault = TempVault::new("related");
    let out = vault.run(&["related", "notes/hello.md"]);
    assert!(out.status.success(), "talon related should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "related");
}

#[test]
fn json_envelope_meta_success() {
    let vault = TempVault::new("meta");
    let out = vault.run(&["meta"]);
    assert!(out.status.success(), "talon meta should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "meta");
}

#[test]
fn json_envelope_changes_success() {
    let vault = TempVault::new("changes");
    let out = vault.run(&["changes", "--since", "2020-01-01T00:00:00Z"]);
    assert!(out.status.success(), "talon changes should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "changes");
}

#[test]
fn json_envelope_lint_success() {
    let vault = TempVault::new("inspect");
    let out = vault.run(&["inspect", "orphans"]);
    assert!(out.status.success(), "talon inspect should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "inspect");
}

#[test]
fn json_envelope_lint_defaults_to_all() {
    let vault = TempVault::new("inspect-all");
    let out = vault.run(&["inspect"]);
    assert!(out.status.success(), "talon inspect should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let envelope = assert_success_envelope(&stdout, "inspect");
    assert_eq!(envelope["data"]["check"], "all");
}

#[test]
fn search_candidate_limit_flag_accepted() {
    let vault = TempVault::new("search-candidate-limit");
    let out = vault.run(&[
        "search",
        "hello",
        "--fast",
        "--candidate-limit",
        "80",
        "--limit",
        "10",
    ]);
    assert!(
        out.status.success(),
        "talon search --candidate-limit 80 should exit 0"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "search");
}

#[test]
fn search_candidate_limit_zero_rejected() {
    let vault = TempVault::new("search-candidate-limit-zero");
    let out = vault.run(&["search", "hello", "--fast", "--candidate-limit", "0"]);
    assert!(
        !out.status.success(),
        "--candidate-limit 0 should exit nonzero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v = super::assert_error_envelope(&stdout, "search");
    let msg = v["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("must be") || msg.contains("positive") || msg.contains("zero"),
        "error message should explain the constraint, got: {msg}"
    );
}
