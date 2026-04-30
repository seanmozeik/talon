use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use talon_core::TalonConfig;

use crate::mcp::session::ledger::TurnLedger;

/// Process-local state shared across all MCP request handlers in a single
/// `talon mcp` process lifetime.
#[derive(Debug)]
pub struct McpServerState {
    pub config: Arc<ConfigState>,
    pub sessions: Arc<RwLock<SessionStore>>,
    pub diagnostics: Arc<DiagnosticsState>,
}

/// Resolved configuration paths and the loaded [`TalonConfig`].
#[derive(Debug)]
pub struct ConfigState {
    pub config: TalonConfig,
    pub config_path: Option<std::path::PathBuf>,
    pub vault_path: std::path::PathBuf,
    pub db_path: std::path::PathBuf,
}

/// Process-local session store keyed by host + session ID.
#[derive(Debug)]
pub struct SessionStore {
    pub sessions: HashMap<SessionKey, SessionState>,
}

/// Composite key identifying a single agent session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub host: HostKind,
    pub session_id: String,
}

/// Identifies the MCP host that opened a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostKind {
    ClaudeCode,
    Hermes,
    Unknown(String),
}

/// Per-session runtime state.
#[derive(Debug)]
pub struct SessionState {
    pub created_at_ms: i64,
    pub last_seen_at_ms: i64,
    /// Turn history with suppression ledger for recall deduplication.
    pub ledger: TurnLedger,
    /// Per-turn score decay multiplier for suppression (default `DEFAULT_DECAY`).
    pub suppression_decay: f64,
}

/// Lightweight diagnostics visible to health-check tooling.
#[derive(Debug)]
pub struct DiagnosticsState {
    pub watcher_running: std::sync::atomic::AtomicBool,
    pub last_refresh_error: Mutex<Option<String>>,
    pub last_embed_error: Mutex<Option<String>>,
}

impl McpServerState {
    /// Creates a new [`McpServerState`] with empty sessions and default
    /// diagnostics, wrapping it in an [`Arc`] for shared ownership.
    #[must_use]
    pub fn new(config: ConfigState) -> Arc<Self> {
        Arc::new(Self {
            config: Arc::new(config),
            sessions: Arc::new(RwLock::new(SessionStore {
                sessions: HashMap::new(),
            })),
            diagnostics: Arc::new(DiagnosticsState {
                watcher_running: std::sync::atomic::AtomicBool::new(false),
                last_refresh_error: Mutex::new(None),
                last_embed_error: Mutex::new(None),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{HostKind, McpServerState, SessionKey};
    use crate::mcp::state::ConfigState;
    use std::path::PathBuf;

    fn stub_config_state() -> ConfigState {
        let vault_path = PathBuf::from("/tmp/vault");
        let db_path = PathBuf::from("/tmp/vault.db");
        let config = crate::config::default_config_for_vault(vault_path.clone());
        ConfigState {
            config,
            config_path: None,
            vault_path,
            db_path,
        }
    }

    #[test]
    fn mcp_server_state_new_creates_empty_session_store() {
        let state = McpServerState::new(stub_config_state());
        let is_empty = state
            .sessions
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .is_empty();
        assert!(
            is_empty,
            "expected empty session store after McpServerState::new"
        );
    }

    #[test]
    fn session_key_equality() {
        let a = SessionKey {
            host: HostKind::ClaudeCode,
            session_id: "abc".to_string(),
        };
        let b = SessionKey {
            host: HostKind::ClaudeCode,
            session_id: "abc".to_string(),
        };
        assert_eq!(a, b);
    }
}
