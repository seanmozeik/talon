//! Contract tests for the Talon CLI binary.

use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

#[path = "cli/json_error.rs"]
mod json_error;
#[path = "cli/json_success.rs"]
mod json_success;
#[path = "cli/smoke.rs"]
mod smoke;

const BIN: &str = "talon";

/// Minimal on-disk vault synced with `--fast` so all commands have a live DB.
struct TempVault {
    dir: PathBuf,
    config_path: PathBuf,
}

impl TempVault {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("talon-cli-contract-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("create temp dir {}: {e}", dir.display()));

        let vault_dir = dir.join("vault");
        std::fs::create_dir_all(vault_dir.join("notes"))
            .unwrap_or_else(|e| panic!("create vault/notes: {e}"));
        std::fs::write(
            vault_dir.join("notes").join("hello.md"),
            "# Hello\n\nThis is a test note about hello world.\n",
        )
        .unwrap_or_else(|e| panic!("write note: {e}"));

        let db_path = dir.join("index.sqlite");
        let config_path = dir.join("config.toml");
        let config_content = format!(
            r#"vault_path = "{vault}"
db_path = "{db}"
include_patterns = ["**/*.md"]
ignore_patterns = []

[indexer]
chunk_tokens = 512
chunk_overlap = 64
chunk_min_tokens = 16

[inference]
base_url = "http://localhost:8080"

[inference.models]
query_embedding = "embed"
document_embedding = "embed"
chunk_embedding = "embed_chunked"
reranker = "rerank"

[expansion]
provider = "openai-compatible"
base_url = "http://localhost:1234/v1"
model = "gemma-smol"
"#,
            vault = vault_dir.display(),
            db = db_path.display(),
        );
        std::fs::write(&config_path, &config_content)
            .unwrap_or_else(|e| panic!("write config: {e}"));

        let status = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
            .args(["sync", "--fast", "--agent", "--config"])
            .arg(&config_path)
            .status()
            .unwrap_or_else(|e| panic!("spawn talon sync: {e}"));
        assert!(
            status.success(),
            "initial talon sync --fast failed during TempVault setup"
        );

        Self { dir, config_path }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
            .args(args)
            .arg("--json")
            .arg("--config")
            .arg(&self.config_path)
            .output()
            .unwrap_or_else(|e| panic!("spawn talon: {e}"))
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn success_keys() -> HashSet<&'static str> {
    ["action", "version", "ok", "data", "meta"]
        .iter()
        .copied()
        .collect()
}

fn error_keys() -> HashSet<&'static str> {
    ["action", "version", "ok", "error"]
        .iter()
        .copied()
        .collect()
}

fn assert_success_envelope(stdout: &str, expected_action: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).unwrap_or_else(|e| {
        panic!("invalid JSON for action={expected_action}: {e}\nstdout: {stdout}")
    });
    assert_eq!(v["action"], expected_action, "action mismatch");
    assert_eq!(
        v["ok"], true,
        "ok should be true for action={expected_action}"
    );
    assert!(v["version"].is_string(), "version should be a string");
    assert!(
        !v["data"].is_null(),
        "data should be present on success for action={expected_action}"
    );
    assert!(
        !v["meta"].is_null(),
        "meta should be present on success for action={expected_action}"
    );
    let keys: HashSet<&str> = v
        .as_object()
        .unwrap_or_else(|| panic!("expected JSON object for action={expected_action}"))
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        success_keys(),
        "unexpected top-level keys for action={expected_action}"
    );
    v
}

fn assert_error_envelope(stdout: &str, expected_action: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).unwrap_or_else(|e| {
        panic!("invalid JSON for action={expected_action}: {e}\nstdout: {stdout}")
    });
    assert_eq!(v["action"], expected_action, "action mismatch");
    assert_eq!(
        v["ok"], false,
        "ok should be false for action={expected_action}"
    );
    assert!(v["version"].is_string(), "version should be a string");
    assert!(
        !v["error"].is_null(),
        "error should be present on failure for action={expected_action}"
    );
    let keys: HashSet<&str> = v
        .as_object()
        .unwrap_or_else(|| panic!("expected JSON object for action={expected_action}"))
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        error_keys(),
        "unexpected top-level keys for action={expected_action}"
    );
    v
}
