//! Contract tests for the Talon CLI binary.
//!
//! Organized into groups:
//!   A. Flag smoke tests (`--help`, `--version`) — no vault needed
//!   B. Error class exit codes — unknown command, missing required args
//!   C. JSON envelope shape (Decision 8) — success path, all 8 commands
//!   D. JSON error envelopes — failure path with `--json` emits `ok:false` envelope

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

const BIN: &str = "talon";

// ── A. Flag smoke tests ───────────────────────────────────────────────────────

#[test]
fn help_flag_exits_zero_and_mentions_talon() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("talon"));
}

#[test]
fn version_flag_exits_zero_and_prints_semver() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^\d+\.\d+\.\d+\n$").unwrap_or_else(|e| panic!("{e}")));
}

// ── A2. talon init ────────────────────────────────────────────────────────────

#[test]
fn init_creates_config_toml_in_xdg_config_home() {
    let tmp = std::env::temp_dir().join(format!("talon-init-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .arg("init")
        .env("HOME", &tmp)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .output()
        .unwrap_or_else(|e| panic!("spawn talon init: {e}"));
    assert!(out.status.success(), "talon init should exit 0");

    let config_path = tmp.join("config").join("talon").join("config.toml");
    assert!(
        config_path.exists(),
        "config.toml not created at {}",
        config_path.display()
    );
    let content =
        std::fs::read_to_string(&config_path).unwrap_or_else(|e| panic!("read config.toml: {e}"));
    assert!(
        content.contains("vault_path"),
        "config.toml missing vault_path"
    );
    assert!(
        content.contains("base_url"),
        "config.toml missing base_url (inference endpoint)"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn init_does_not_overwrite_existing_config() {
    let tmp = std::env::temp_dir().join(format!("talon-init-existing-{}", std::process::id()));
    let config_dir = tmp.join("config").join("talon");
    std::fs::create_dir_all(&config_dir).unwrap_or_else(|e| panic!("create config dir: {e}"));
    let config_path = config_dir.join("config.toml");
    std::fs::write(&config_path, "# sentinel\n")
        .unwrap_or_else(|e| panic!("write sentinel config: {e}"));

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .arg("init")
        .env("HOME", &tmp)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .output()
        .unwrap_or_else(|e| panic!("spawn talon init: {e}"));
    assert!(
        out.status.success(),
        "talon init should exit 0 when file exists"
    );

    let content = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("read sentinel config: {e}"));
    assert_eq!(
        content, "# sentinel\n",
        "talon init must not overwrite an existing config"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ── B. Error class exit codes ────────────────────────────────────────────────

#[test]
fn unknown_command_exits_nonzero() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("nonexistent-command")
        .assert()
        .failure();
}

#[test]
fn search_without_query_exits_nonzero() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("search")
        .assert()
        .failure();
}

#[test]
fn lint_without_check_type_exits_nonzero() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("lint")
        .assert()
        .failure();
}

#[test]
fn read_without_path_exits_nonzero() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("read")
        .assert()
        .failure();
}

#[test]
fn related_without_path_exits_nonzero() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("related")
        .assert()
        .failure();
}

// ── C. JSON envelope shape (Decision 8) ──────────────────────────────────────

/// Minimal on-disk vault synced with `--fast` so all 8 commands have a live DB.
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

        // Initialize the DB with a lexical-only sync.
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

    /// Run `talon <args> --json --config <config_path>` and return the output.
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

// ── D. JSON error envelopes (Decision 8 failure path) ────────────────────────
//
// When `--json` is passed, a failed command must emit `{action, version, ok:
// false, error}` on stdout rather than printing plain text to stderr.

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
