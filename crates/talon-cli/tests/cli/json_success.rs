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
        "--limit",
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
    let vault = TempVault::new("lint");
    let out = vault.run(&["lint", "orphans"]);
    assert!(out.status.success(), "talon lint should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_success_envelope(&stdout, "lint");
}
