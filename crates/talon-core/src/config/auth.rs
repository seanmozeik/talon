//! Credential resolution for HTTP endpoint configuration.

use std::collections::BTreeMap;
use std::env;

use serde::{Deserialize, Serialize};

use crate::error::TalonError;

/// Named API credential referenced by capability blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialEntry {
    /// Inline API key (discouraged; prefer `api_key_env`).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Environment variable holding the API key.
    #[serde(default)]
    pub api_key_env: Option<String>,
}

/// Named credential table from `[credentials.*]` config sections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CredentialsConfig {
    #[serde(flatten)]
    pub entries: BTreeMap<String, CredentialEntry>,
}

/// Resolved authentication material for an HTTP client.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedAuth {
    /// Bearer token, when configured.
    pub api_key: Option<String>,
    /// Provider-specific headers (for example `OpenRouter` attribution).
    pub extra_headers: BTreeMap<String, String>,
}

/// Shared transport/auth fields for any HTTP capability block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EndpointAuthConfig {
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
}

impl EndpointAuthConfig {
    /// Resolves the API key and merges extra headers for this endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::Config`] when a referenced credential or env var
    /// is missing.
    pub fn resolve(&self, credentials: &CredentialsConfig) -> Result<ResolvedAuth, TalonError> {
        let api_key = resolve_api_key(credentials, self)?;
        Ok(ResolvedAuth {
            api_key,
            extra_headers: self.extra_headers.clone(),
        })
    }
}

/// Resolves an API key from inline fields and optional named credentials.
///
/// Precedence: inline `api_key` → inline `api_key_env` → credential `api_key` →
/// credential `api_key_env`.
///
/// # Errors
///
/// Returns [`TalonError::Config`] when a referenced credential or env var is
/// missing.
pub fn resolve_api_key(
    credentials: &CredentialsConfig,
    auth: &EndpointAuthConfig,
) -> Result<Option<String>, TalonError> {
    if let Some(key) = non_empty(auth.api_key.as_deref()) {
        return Ok(Some(key.to_owned()));
    }
    if let Some(env_name) = non_empty(auth.api_key_env.as_deref()) {
        return read_env_key(env_name);
    }
    let Some(credential_name) = non_empty(auth.credential.as_deref()) else {
        return Ok(None);
    };
    let entry = credentials
        .entries
        .get(credential_name)
        .ok_or_else(|| TalonError::Config {
            message: format!("unknown credential: {credential_name}"),
        })?;
    if let Some(key) = non_empty(entry.api_key.as_deref()) {
        return Ok(Some(key.to_owned()));
    }
    if let Some(env_name) = non_empty(entry.api_key_env.as_deref()) {
        return read_env_key(env_name);
    }
    match crate::config::keychain::get(credential_name) {
        Ok(Some(key)) => Ok(Some(key)),
        Ok(None) => Ok(None),
        Err(error) => {
            tracing::debug!(%credential_name, %error, "failed to read credential from keychain");
            Ok(None)
        }
    }
}

fn read_env_key(env_name: &str) -> Result<Option<String>, TalonError> {
    match env::var(env_name) {
        Ok(value) if value.is_empty() => Err(TalonError::Config {
            message: format!("environment variable {env_name} is empty"),
        }),
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Err(TalonError::Config {
            message: format!("environment variable {env_name} is not set"),
        }),
        Err(env::VarError::NotUnicode(_)) => Err(TalonError::Config {
            message: format!("environment variable {env_name} is not valid UTF-8"),
        }),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.is_empty())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn creds() -> CredentialsConfig {
        let mut entries = BTreeMap::new();
        entries.insert(
            "openrouter".to_owned(),
            CredentialEntry {
                api_key: None,
                api_key_env: Some("OPENROUTER_API_KEY".to_owned()),
            },
        );
        CredentialsConfig { entries }
    }

    #[test]
    fn inline_api_key_wins() {
        let auth = EndpointAuthConfig {
            api_key: Some("inline".to_owned()),
            api_key_env: Some("IGNORE".to_owned()),
            ..EndpointAuthConfig::default()
        };
        assert_eq!(
            resolve_api_key(&creds(), &auth).expect("resolve inline api key"),
            Some("inline".to_owned())
        );
    }

    #[test]
    fn credential_entry_api_key_is_used_when_present() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "openrouter".to_string(),
            CredentialEntry {
                api_key: Some("from-table".to_owned()),
                api_key_env: Some("OPENROUTER_API_KEY".to_owned()),
            },
        );
        let creds = CredentialsConfig { entries };
        let auth = EndpointAuthConfig {
            credential: Some("openrouter".to_owned()),
            ..EndpointAuthConfig::default()
        };
        assert_eq!(
            resolve_api_key(&creds, &auth).expect("resolve credential api key"),
            Some("from-table".to_owned())
        );
    }
}
