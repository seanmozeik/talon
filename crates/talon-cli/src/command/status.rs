use super::output_mode;
use crate::cli::CliArgs;
use crate::config;
use crate::output::emit_response;
use crate::telemetry::elapsed_ms;
use eyre::Result;
use std::time::Instant;
use talon_core::{ResponseMeta, StatusResponse, TalonEnvelope, TalonResponseData, open_database};

pub(super) fn emit(args: &CliArgs) -> Result<()> {
    let started = Instant::now();

    let config_res = config::load_config(args.config_file.as_deref());
    let config = match config_res {
        Ok(c) => c,
        Err(e) => {
            let resp = StatusResponse {
                state: talon_core::StatusState::ConfigError,
                enabled: false,
                reason: Some(format!("{e:#}")),
                container_mount: talon_core::ContainerPath::root(),
                index_version: env!("CARGO_PKG_VERSION").to_string(),
                index: talon_core::IndexStats {
                    active_notes: 0,
                    chunk_count: 0,
                    failed_embeddings: 0,
                    vector_dimensions: None,
                },
                scopes: None,
            };
            let meta = ResponseMeta {
                duration_ms: elapsed_ms(started),
                result_count: None,
                warnings: Vec::new(),
                scope_set: None,
                since: None,
            };
            let data = TalonResponseData::Status(resp);
            return emit_response(&TalonEnvelope::ok("status", data, meta), output_mode(args));
        }
    };

    let db_path = config.db_path.clone();
    let vault_path = config.vault_path.clone();

    let conn_res = open_database(&db_path);
    let response = match conn_res {
        Ok(conn) => talon_core::query_status(&conn, &config),
        Err(e) => {
            let mount = talon_core::ContainerPath::parse(vault_path.to_string_lossy().as_ref())
                .unwrap_or_else(|_| talon_core::ContainerPath::root());
            StatusResponse {
                state: talon_core::StatusState::ConfigError,
                enabled: false,
                reason: Some(format!("cannot open index at {}: {e:#}", db_path.display())),
                container_mount: mount,
                index_version: env!("CARGO_PKG_VERSION").to_string(),
                index: talon_core::IndexStats {
                    active_notes: 0,
                    chunk_count: 0,
                    failed_embeddings: 0,
                    vector_dimensions: None,
                },
                scopes: None,
            }
        }
    };

    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: None,
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    let data = TalonResponseData::Status(response);
    emit_response(&TalonEnvelope::ok("status", data, meta), output_mode(args))
}
