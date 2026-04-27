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
        .stdout(predicate::str::is_match(r"^\d+\.\d+\.\d+\n$").unwrap_or_else(|e| panic!("{e}")));
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
    let expected_db = tmp.join(".talon").join("obsidian.db").display().to_string();
    assert!(
        content.contains(&format!("db_path = \"{expected_db}\"")),
        "config.toml should contain absolute db_path under ~/.talon; got:\n{content}"
    );
    assert!(
        tmp.join(".talon").exists(),
        "talon init should create the database directory"
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
