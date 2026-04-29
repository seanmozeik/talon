use std::sync::Arc;
use std::time::Duration;

use crate::mcp::state::McpServerState;

const DEFAULT_INTERVAL_SECS: u64 = 1800; // 30 minutes

/// Spawns a background thread that runs pending-chunk embedding on a fixed interval.
///
/// The ticker starts immediately at MCP startup and runs independently of the
/// vault watcher or any hook activity. If the embedding sidecar is unavailable,
/// the error is recorded in diagnostics and the ticker continues on the next tick.
///
/// If the thread fails to spawn, the error is silently ignored; the MCP
/// server continues without the ticker.
pub fn spawn_embed_ticker(state: Arc<McpServerState>) {
    let _ = std::thread::Builder::new()
        .name("talon-embed-ticker".to_owned())
        .spawn(move || {
            if let Err(e) = run_embed_ticker(&state) {
                let mut err = state
                    .diagnostics
                    .last_embed_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *err = Some(format!("embed ticker error: {e:#}"));
            }
        });
}

fn run_embed_ticker(state: &Arc<McpServerState>) -> color_eyre::eyre::Result<()> {
    let interval = Duration::from_secs(DEFAULT_INTERVAL_SECS);
    loop {
        std::thread::sleep(interval);
        if let Err(e) = run_embed_tick(state) {
            let mut err = state
                .diagnostics
                .last_embed_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *err = Some(format!("embed tick error: {e:#}"));
        } else {
            let mut err = state
                .diagnostics
                .last_embed_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *err = None;
        }
    }
}

fn run_embed_tick(state: &Arc<McpServerState>) -> color_eyre::eyre::Result<()> {
    use color_eyre::eyre::WrapErr as _;
    use talon_core::{
        embed::EmbedPassOptions, inference::InferenceClient, open_database,
        vec_ext::register_sqlite_vec,
    };

    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let conn = open_database(&state.config.db_path)
        .wrap_err_with(|| format!("opening index at {}", state.config.db_path.display()))?;

    let opts = EmbedPassOptions {
        force: false,
        restrict_paths: Vec::new(),
        chunk_embedding_model: state.config.config.inference.models.chunk_embedding.clone(),
        document_embedding_model: state
            .config
            .config
            .inference
            .models
            .document_embedding
            .clone(),
    };

    let client = InferenceClient::new(&state.config.config.inference.base_url)
        .wrap_err("building inference client")?;

    talon_core::embed::run_embed_pass(&conn, &client, &opts)
        .map(|_| ())
        .wrap_err("embedding pending chunks")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_ticker_interval_is_thirty_minutes() {
        assert_eq!(DEFAULT_INTERVAL_SECS, 1800);
    }
}
