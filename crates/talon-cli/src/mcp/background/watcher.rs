use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::Result;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer};

use crate::config::RefreshLockPolicy;
use crate::mcp::state::McpServerState;

const DEBOUNCE_SECS: u64 = 60;

/// Spawns a background thread watching `vault_path` for `.md` changes.
///
/// Changes trigger a fast (no-embed) index refresh via the existing
/// `refresh_index_if_needed` function. Non-`.md` files are ignored.
/// Errors are recorded in diagnostics; the watcher thread runs until
/// the process exits.
///
/// If the thread fails to spawn, the error is silently ignored; the MCP
/// server continues without the watcher.
pub fn spawn_watcher(vault_path: PathBuf, state: Arc<McpServerState>) {
    let _ = std::thread::Builder::new()
        .name("talon-vault-watcher".to_owned())
        .spawn(move || {
            if let Err(e) = run_watcher(&vault_path, &state) {
                let mut err = state
                    .diagnostics
                    .last_refresh_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *err = Some(format!("watcher error: {e:#}"));
            }
        });
}

fn run_watcher(vault_path: &Path, state: &Arc<McpServerState>) -> Result<()> {
    use std::sync::atomic::Ordering;

    let state_clone = Arc::clone(state);
    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_secs(DEBOUNCE_SECS), tx)?;
    debouncer.watcher().watch(
        vault_path,
        notify_debouncer_mini::notify::RecursiveMode::Recursive,
    )?;

    state
        .diagnostics
        .watcher_running
        .store(true, Ordering::Relaxed);

    for result in rx {
        match result {
            Ok(events) => {
                // Only react to .md file changes.
                let has_md = events
                    .iter()
                    .any(|e| e.path.extension().is_some_and(|ext| ext == "md"));
                if !has_md {
                    continue;
                }
                // Refresh the text index (no-embed, skip if busy).
                if let Err(e) = {
                    let mut conn = talon_core::open_database(&state_clone.config.db_path)?;
                    crate::config::refresh_index_if_needed(
                        &state_clone.config.config,
                        &mut conn,
                        true,
                        RefreshLockPolicy::SkipIfBusy,
                    )
                } {
                    let mut err = state_clone
                        .diagnostics
                        .last_refresh_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *err = Some(format!("refresh error: {e:#}"));
                } else {
                    let mut err = state_clone
                        .diagnostics
                        .last_refresh_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *err = None;
                }
            }
            Err(e) => {
                let mut err = state
                    .diagnostics
                    .last_refresh_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *err = Some(format!("watcher recv error: {e:?}"));
            }
        }
    }

    state
        .diagnostics
        .watcher_running
        .store(false, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_watcher_sets_watcher_running_false_initially() {
        let vault_path = PathBuf::from("/tmp/vault");
        let db_path = PathBuf::from("/tmp/vault.db");
        let config = crate::config::default_config_for_vault(vault_path.clone());
        let config_state = crate::mcp::state::ConfigState {
            config,
            config_path: None,
            vault_path,
            db_path,
        };
        let state = McpServerState::new(config_state);
        // Verify watcher_running starts as false
        assert!(
            !state
                .diagnostics
                .watcher_running
                .load(std::sync::atomic::Ordering::Relaxed),
            "expected watcher_running to start as false"
        );
    }
}
