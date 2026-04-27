//! Config loading and initialization for the CLI process boundary.

use eyre::{Result, WrapErr as _, bail};
use fs_err as fs;
use std::path::{Component, Path, PathBuf};
use talon_core::{InferenceConfig, InferenceModels, Scope, ScopePriority, TalonConfig};

/// Default config filename.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Default config directory.
pub const CONFIG_DIR_NAME: &str = "talon";

/// Default config path: `~/.config/talon/config.toml`.
#[must_use]
pub fn default_config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });
    path.push(CONFIG_DIR_NAME);
    path.push(CONFIG_FILE_NAME);
    path
}

/// Default `SQLite` index path.
#[must_use]
pub fn default_db_path() -> PathBuf {
    default_db_path_for_workspace("default")
}

/// Default `SQLite` index path for a workspace.
#[must_use]
pub fn default_db_path_for_workspace(workspace: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".talon")
        .join(format!("{}.db", sanitize_workspace_name(workspace)))
}

/// Config template written by `talon init`.
pub const CONFIG_TEMPLATE: &str = r#"# Talon configuration.
# Location: ~/.config/talon/config.toml

vault_path = "/Users/you/path/to/obsidian"
# Convention: ~/.talon/{workspace}.db. Update this if you rename the vault.
db_path = "~/.talon/obsidian.db"
include_patterns = ["**/*.md"]
ignore_patterns = [".obsidian/**", ".git/**", "templates/**", "*.canvas"]

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
# Optional total completion cap. Leave unset for thinking models.
# max_tokens = 768

# ── Scopes ─────────────────────────────────────────────────────────────────
# Named vault partitions with priority-based ranking.
# See docs/CONFIG.md for full reference.
# Uncomment and edit the Karpathy preset below.
#
# [scopes.wiki]
# glob     = ["wiki/**", "concepts/**"]
# priority = "boosted"
# default  = true
#
# ... additional scopes ...
"#;

/// Loads a config file from the given path.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_config_file(path: &Path) -> Result<TalonConfig> {
    let content = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read config file: {}", path.display()))?;

    let mut config: TalonConfig = toml::from_str(&content)
        .wrap_err_with(|| format!("failed to parse config file: {}", path.display()))?;
    resolve_config_paths(&mut config, path)?;
    if let Err(message) = config.chunker.validate() {
        bail!("{message}");
    }

    Ok(config)
}

/// Loads config from the default path or an explicit path.
///
/// # Errors
///
/// Returns an error if the config file cannot be found or parsed.
pub fn load_config(explicit_path: Option<&Path>) -> Result<TalonConfig> {
    let path = explicit_path
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::var("TALON_CONFIG_FILE").ok().map(PathBuf::from))
        .unwrap_or_else(default_config_path);

    if !path.exists() {
        bail!(
            "config not found at {}, run `talon init` first",
            path.display()
        );
    }

    let mut config = load_config_file(&path)?;

    // TALON_VAULT overrides vault_path so callers (e.g. Hermes plugin) can
    // target a specific vault without modifying the config file.
    if let Ok(vault_override) = std::env::var("TALON_VAULT") {
        config.vault_path =
            absolutize_path(PathBuf::from(vault_override), &std::env::current_dir()?);
    }

    Ok(config)
}

/// Initializes the config file at the default path.
///
/// Creates the directory if it doesn't exist. Does not overwrite an existing file.
///
/// # Errors
///
/// Returns an error if the config directory cannot be created or the file cannot be written.
pub fn init_config() -> Result<bool> {
    let path = default_config_path();

    if path.exists() {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create config directory: {}", parent.display()))?;
    }

    fs::write(&path, CONFIG_TEMPLATE)
        .wrap_err_with(|| format!("failed to write config file: {}", path.display()))?;

    Ok(true)
}

/// Builds a default config from a vault path.
#[must_use]
pub fn default_config_for_vault(vault_path: PathBuf) -> TalonConfig {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vault_path = absolutize_path(vault_path, &cwd);
    let db_path = default_db_path_for_workspace(&workspace_name_for_vault(&vault_path));

    TalonConfig {
        vault_path,
        db_path,
        include_patterns: vec!["**/*.md".to_string()],
        ignore_patterns: vec![
            ".obsidian/**".to_string(),
            ".git/**".to_string(),
            "templates/**".to_string(),
            "*.canvas".to_string(),
        ],
        inference: InferenceConfig {
            base_url: "http://localhost:8080".to_string(),
            models: InferenceModels {
                query_embedding: "embed".to_string(),
                document_embedding: "embed".to_string(),
                chunk_embedding: "embed_chunked".to_string(),
                reranker: "rerank".to_string(),
            },
        },
        expansion: talon_core::ExpansionConfig {
            provider: "openai-compatible".to_string(),
            base_url: "http://localhost:1234/v1".to_string(),
            model: "gemma-smol".to_string(),
            max_tokens: None,
        },
        scopes: default_karpathy_scopes(),
        chunker: talon_core::ChunkerConfig::default(),
    }
}

fn workspace_name_for_vault(vault_path: &Path) -> String {
    vault_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("default")
        .to_string()
}

fn sanitize_workspace_name(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

fn resolve_config_paths(config: &mut TalonConfig, config_path: &Path) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let config_path = absolutize_path(config_path.to_path_buf(), &cwd);
    let config_dir = config_path.parent().unwrap_or(&cwd);

    config.vault_path = absolutize_path(config.vault_path.clone(), config_dir);
    config.db_path = absolutize_path(config.db_path.clone(), config_dir);
    Ok(())
}

fn absolutize_path(path: PathBuf, base: &Path) -> PathBuf {
    let path = expand_tilde(path);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return path;
    };
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(component)) if component == "~" => home.join(components.as_path()),
        _ => path,
    }
}

/// Builds the Karpathy-shaped preset scopes.
fn default_karpathy_scopes() -> std::collections::BTreeMap<String, Scope> {
    use talon_core::ScopeGlob;
    let mut scopes = std::collections::BTreeMap::new();

    scopes.insert(
        "wiki".to_string(),
        Scope {
            glob: ScopeGlob::Multiple(vec!["wiki/**".to_string(), "concepts/**".to_string()]),
            priority: ScopePriority::Boosted,
            default: true,
        },
    );
    scopes.insert(
        "projects".to_string(),
        Scope {
            glob: ScopeGlob::Single("projects/**".to_string()),
            priority: ScopePriority::Elevated,
            default: true,
        },
    );
    scopes.insert(
        "artifacts".to_string(),
        Scope {
            glob: ScopeGlob::Single("artifacts/**".to_string()),
            priority: ScopePriority::Normal,
            default: true,
        },
    );
    scopes.insert(
        "raw".to_string(),
        Scope {
            glob: ScopeGlob::Single("raw/**".to_string()),
            priority: ScopePriority::Muted,
            default: true,
        },
    );
    scopes.insert(
        "daily".to_string(),
        Scope {
            glob: ScopeGlob::Single("daily/**".to_string()),
            priority: ScopePriority::Muted,
            default: true,
        },
    );
    scopes.insert(
        "archive".to_string(),
        Scope {
            glob: ScopeGlob::Single("archive/**".to_string()),
            priority: ScopePriority::Buried,
            default: true,
        },
    );
    scopes.insert(
        "private".to_string(),
        Scope {
            glob: ScopeGlob::Single("private/**".to_string()),
            priority: ScopePriority::Normal,
            default: false,
        },
    );

    scopes
}

#[cfg(test)]
mod tests;
