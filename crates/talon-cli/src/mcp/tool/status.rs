use std::path::PathBuf;
use std::time::Instant;

use talon_core::{ResponseMeta, StatusResponse, TalonEnvelope, TalonResponseData, open_database};

use crate::config;

use super::time::elapsed_ms;

pub(super) fn dispatch_status(_input: talon_core::StatusInput) -> TalonEnvelope {
    let started = Instant::now();
    let response = match config::load_config(None) {
        Ok(config) => match open_database(&config.db_path) {
            Ok(conn) => talon_core::query_status(&conn, &config),
            Err(error) => {
                status_config_error(
                    started,
                    format!(
                        "cannot open index at {}: {error:#}",
                        config.db_path.display()
                    ),
                    &config.vault_path,
                )
                .0
            }
        },
        Err(error) => status_config_error(started, format!("{error:#}"), &PathBuf::from("/")).0,
    };
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: None,
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    TalonEnvelope::ok("status", TalonResponseData::Status(response), meta)
}

fn status_config_error(
    started: Instant,
    reason: String,
    vault_path: &std::path::Path,
) -> (StatusResponse, ResponseMeta) {
    let mount = talon_core::ContainerPath::parse(vault_path.to_string_lossy().as_ref())
        .unwrap_or_else(|_| talon_core::ContainerPath::root());
    (
        StatusResponse {
            state: talon_core::StatusState::ConfigError,
            enabled: false,
            reason: Some(reason),
            container_mount: mount,
            index_version: env!("CARGO_PKG_VERSION").to_string(),
            index: talon_core::IndexStats {
                active_notes: 0,
                chunk_count: 0,
                failed_embeddings: 0,
                vector_dimensions: None,
            },
            scopes: None,
        },
        ResponseMeta {
            duration_ms: elapsed_ms(started),
            result_count: None,
            warnings: Vec::new(),
            scope_set: None,
            since: None,
        },
    )
}
