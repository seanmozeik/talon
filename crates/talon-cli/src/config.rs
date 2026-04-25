//! Config loading and initialization for the CLI process boundary.

use eyre::{Context, Result, bail};
use std::path::{Path, PathBuf};
use talon_core::TalonConfig;

/// Default config filename.
pub const CONFIG_FILE_NAME: &str = "config.toml";

const CONFIG_TEMPLATE: &str = r#"# Talon configuration.

vault_path = "/Users/you/path/to/obsidian"
db_path = "~/.local/share/talon/index.sqlite"
index_on_start = true
watch = true
embedding_schedule = ["03:00", "15:00"]
include_patterns = ["**/*.md"]
ignore_patterns = [".obsidian/**", ".git/**", "templates/**", "*.canvas"]

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
"#;

/// Loads a JSON or TOML config file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_config_file(path: &Path) -> Result<TalonConfig> {
    let raw = fs_err::read_to_string(path)
        .wrap_err_with(|| format!("failed to read config file {}", path.display()))?;

    let extension = path.extension().and_then(std::ffi::OsStr::to_str);
    match extension {
        Some("json") => serde_json::from_str(&raw)
            .wrap_err_with(|| format!("failed to parse JSON config {}", path.display())),
        _ => toml::from_str(&raw)
            .wrap_err_with(|| format!("failed to parse TOML config {}", path.display())),
    }
}

/// Returns Talon's default config path.
///
/// # Errors
///
/// Returns an error when `$HOME` is not available.
pub fn default_config_path() -> Result<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        bail!("HOME is not set; pass --config <path> explicitly");
    };
    Ok(PathBuf::from(home)
        .join(".config")
        .join("talon")
        .join(CONFIG_FILE_NAME))
}

/// Creates the default config directory and template file if missing.
///
/// # Errors
///
/// Returns an error when the config path cannot be resolved or written.
pub fn init_default_config() -> Result<InitConfigResult> {
    let path = default_config_path()?;
    let Some(parent) = path.parent() else {
        bail!("invalid config path {}", path.display());
    };

    fs_err::create_dir_all(parent)
        .wrap_err_with(|| format!("failed to create config directory {}", parent.display()))?;
    set_private_dir_permissions(parent)?;

    if path.exists() {
        return Ok(InitConfigResult {
            path,
            created: false,
        });
    }

    fs_err::write(&path, CONFIG_TEMPLATE)
        .wrap_err_with(|| format!("failed to write config file {}", path.display()))?;
    set_private_file_permissions(&path)?;

    Ok(InitConfigResult {
        path,
        created: true,
    })
}

/// Result of `talon init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitConfigResult {
    /// Config path.
    pub path: PathBuf,
    /// Whether the file was created by this invocation.
    pub created: bool,
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o700);
    fs_err::set_permissions(path, permissions)
        .wrap_err_with(|| format!("failed to chmod 0700 {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o600);
    fs_err::set_permissions(path, permissions)
        .wrap_err_with(|| format!("failed to chmod 0600 {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
