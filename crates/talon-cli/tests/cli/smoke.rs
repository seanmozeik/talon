use assert_cmd::Command;
use predicates::prelude::*;

use super::BIN;

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
        .stdout(
            predicate::str::is_match(r"^talon \d+\.\d+\.\d+\n$").unwrap_or_else(|e| panic!("{e}")),
        );
}

#[test]
fn short_version_flag_exits_zero_and_prints_semver() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("-V")
        .assert()
        .success()
        .stdout(
            predicate::str::is_match(r"^talon \d+\.\d+\.\d+\n$").unwrap_or_else(|e| panic!("{e}")),
        );
}

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
    assert!(
        content.contains("db_path = \"~/.talon/obsidian.db\""),
        "config.toml should contain the workspace db convention; got:\n{content}"
    );
    assert!(
        !tmp.join(".talon").exists(),
        "talon init should not create the database directory before vault configuration"
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

#[test]
fn sync_rebuild_recreates_existing_index_database() {
    let tmp = std::env::temp_dir().join(format!("talon-sync-rebuild-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let vault_path = tmp.join("vault");
    let db_path = tmp.join("index.sqlite");
    let config_path = tmp.join("config.toml");
    std::fs::create_dir_all(&vault_path).unwrap_or_else(|e| panic!("create vault: {e}"));
    std::fs::write(vault_path.join("note.md"), "# Note\n\nBody")
        .unwrap_or_else(|e| panic!("write note: {e}"));
    std::fs::write(
        &config_path,
        format!(
            r#"vault_path = "{vault}"
db_path = "{db}"
include_patterns = ["**/*.md"]
ignore_patterns = []

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
            vault = vault_path.display(),
            db = db_path.display(),
        ),
    )
    .unwrap_or_else(|e| panic!("write config: {e}"));

    let conn = talon_core::open_database(&db_path).unwrap_or_else(|e| panic!("open db: {e}"));
    conn.execute("CREATE TABLE rebuild_marker (id INTEGER)", [])
        .unwrap_or_else(|e| panic!("create marker: {e}"));
    drop(conn);

    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("--json")
        .arg("--config")
        .arg(&config_path)
        .arg("--fast")
        .arg("sync")
        .arg("--rebuild")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""rebuild": true"#));

    let conn = talon_core::open_database_read_only(&db_path)
        .unwrap_or_else(|e| panic!("open rebuilt db: {e}"));
    assert!(
        conn.prepare("SELECT COUNT(*) FROM rebuild_marker").is_err(),
        "rebuild should discard stale tables from the old index"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn status_ignores_empty_talon_config_file_env() {
    let tmp = std::env::temp_dir().join(format!("talon-empty-config-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let config_dir = tmp.join("config").join("talon");
    let vault_path = tmp.join("vault");
    let db_path = tmp.join("index.sqlite");
    std::fs::create_dir_all(&config_dir).unwrap_or_else(|e| panic!("create config dir: {e}"));
    std::fs::create_dir_all(&vault_path).unwrap_or_else(|e| panic!("create vault dir: {e}"));
    let config_path = config_dir.join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"vault_path = "{vault}"
db_path = "{db}"

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
            vault = vault_path.display(),
            db = db_path.display(),
        ),
    )
    .unwrap_or_else(|e| panic!("write config: {e}"));

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .arg("--agent")
        .arg("status")
        .env("HOME", &tmp)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .env("TALON_CONFIG_FILE", "")
        .output()
        .unwrap_or_else(|e| panic!("spawn talon status: {e}"));
    assert!(out.status.success(), "talon status should exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap_or_else(|e| panic!("utf8 stdout: {e}"));
    assert!(
        stdout.contains(&format!(r#""configPath":"{}""#, config_path.display())),
        "status should load the default config path when TALON_CONFIG_FILE is empty; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("config not found at "),
        "empty TALON_CONFIG_FILE must not be treated as an explicit empty path; got:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn status_ignores_empty_talon_vault_env() {
    let tmp = std::env::temp_dir().join(format!("talon-empty-vault-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let config_dir = tmp.join("config").join("talon");
    let vault_path = tmp.join("vault");
    let db_path = tmp.join("index.sqlite");
    std::fs::create_dir_all(&config_dir).unwrap_or_else(|e| panic!("create config dir: {e}"));
    std::fs::create_dir_all(&vault_path).unwrap_or_else(|e| panic!("create vault dir: {e}"));
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"vault_path = "{vault}"
db_path = "{db}"

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
            vault = vault_path.display(),
            db = db_path.display(),
        ),
    )
    .unwrap_or_else(|e| panic!("write config: {e}"));

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_talon"))
        .arg("--agent")
        .arg("status")
        .env("HOME", &tmp)
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .env("TALON_VAULT", "")
        .output()
        .unwrap_or_else(|e| panic!("spawn talon status: {e}"));
    assert!(out.status.success(), "talon status should exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap_or_else(|e| panic!("utf8 stdout: {e}"));
    assert!(
        stdout.contains(&format!(r#""vaultPath":"{}""#, vault_path.display())),
        "empty TALON_VAULT should not override the configured vault path; got:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

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
fn lint_unknown_check_type_exits_nonzero() {
    Command::cargo_bin(BIN)
        .unwrap_or_else(|e| panic!("cargo_bin: {e}"))
        .arg("inspect")
        .arg("not-a-check")
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
