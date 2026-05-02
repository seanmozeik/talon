//! End-to-end graph checks against the bundled Calle Sur example vault.

use serde_json::Value;
use std::process::Command;

#[test]
fn examples_config_sync_builds_graph_tables() {
    let output = Command::new(env!("CARGO_BIN_EXE_talon"))
        .args([
            "sync",
            "--fast",
            "--agent",
            "--config",
            "../../examples/config.toml",
        ])
        .output()
        .unwrap_or_else(|err| panic!("spawn talon sync: {err}"));
    assert!(
        output.status.success(),
        "example sync failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("parse sync JSON: {err}"));
    let graph = json
        .get("graph")
        .unwrap_or_else(|| panic!("sync output missing graph stats: {json}"));

    assert!(graph["nodeCount"].as_u64().unwrap_or(0) >= 70);
    assert!(graph["edgeCount"].as_u64().unwrap_or(0) >= 300);
    assert!(graph["sourceCount"].as_u64().unwrap_or(0) >= 70);

    let output = Command::new(env!("CARGO_BIN_EXE_talon"))
        .args([
            "related",
            "wiki/Sauce Mothers.md",
            "--direction",
            "both",
            "--json",
            "--config",
            "../../examples/config.toml",
        ])
        .output()
        .unwrap_or_else(|err| panic!("spawn talon related: {err}"));
    assert!(
        output.status.success(),
        "example related failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("parse related JSON: {err}"));
    let results = json["data"]["results"]
        .as_array()
        .unwrap_or_else(|| panic!("related output missing results: {json}"));

    assert_eq!(results[0]["vaultPath"], "wiki/Salt Acid Fat Heat.md");
    assert!(results[0]["score"].as_f64().unwrap_or(0.0) > 0.0);
    assert!(results[0]["signals"]["directOut"].as_f64().unwrap_or(0.0) > 0.0);
    assert_eq!(results[0]["signals"]["typeAffinity"], 1.0);
}
