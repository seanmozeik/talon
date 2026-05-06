use super::output_mode;
use crate::cli::{Cli, StatusArgs};
use crate::config;
use crate::output::emit_response;
use crate::telemetry::elapsed_ms;
use eyre::Result;
use std::time::Instant;
use talon_core::{ResponseMeta, StatusResponse, TalonEnvelope, TalonResponseData, open_database};

pub(super) fn emit(_args: &StatusArgs, cli: &Cli) -> Result<()> {
    let started = Instant::now();

    let config_res = config::load_config(cli.config_file.as_deref());
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
                vault_path: None,
                config_path: None,
                db_path: None,
            };
            let meta = ResponseMeta {
                duration_ms: elapsed_ms(started),
                result_count: None,
                warnings: Vec::new(),
                scope_set: None,
                since: None,
            };
            let data = TalonResponseData::Status(resp);
            return emit_response(&TalonEnvelope::ok("status", data, meta), output_mode(cli));
        }
    };

    let db_path = config.db_path.clone();
    let vault_path = config.vault_path.clone();

    let conn_res = open_database(&db_path);
    let mut response = match conn_res {
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
                vault_path: Some(vault_path.to_string_lossy().into_owned()),
                config_path: config
                    .config_file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                db_path: Some(db_path.to_string_lossy().into_owned()),
            }
        }
    };

    let mut warnings = Vec::new();
    if let Some(warning) = crate::mcp::diagnostics::crash_status_warning() {
        if response.reason.is_none() {
            response.reason = Some(warning.clone());
        }
        warnings.push(warning);
    }

    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: None,
        warnings,
        scope_set: None,
        since: None,
    };
    let data = TalonResponseData::Status(response);
    emit_response(&TalonEnvelope::ok("status", data, meta), output_mode(cli))
}
