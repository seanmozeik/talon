use std::path::PathBuf;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr as _};
use serde::Deserialize;
use serde_json::{Value, json};
use talon_core::{
    ChangesInput, ErrorCode, ErrorEnvelope, ExpansionClient, IndexerConfig, LintInput, MetaInput,
    ReadInput, RelatedInput, ResponseMeta, SearchInput, SearchMode, StatusResponse, SyncInput,
    SyncResponse, SyncStatus, TalonConfig, TalonEnvelope, TalonError, TalonInput,
    TalonResponseData, embed::EmbedPassOptions, find_related, inference::InferenceClient,
    open_database, query_changes, query_lint, query_meta, query_status, run_read, run_search,
    vec_ext::register_sqlite_vec,
};

use crate::config;

const TOOL_NAME: &str = "talon";
const TOOL_DESCRIPTION: &str = "Run one stateless Talon action against the configured vault.";

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug)]
struct ToolError {
    action: &'static str,
    code: ErrorCode,
    message: String,
    detail: Option<Value>,
}

impl ToolError {
    fn new(action: &'static str, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            action,
            code,
            message: message.into(),
            detail: None,
        }
    }

    fn with_detail(
        action: &'static str,
        code: ErrorCode,
        message: impl Into<String>,
        detail: Value,
    ) -> Self {
        Self {
            action,
            code,
            message: message.into(),
            detail: Some(detail),
        }
    }

    fn envelope(self) -> TalonEnvelope {
        TalonEnvelope::err(
            self.action,
            ErrorEnvelope {
                code: self.code,
                message: self.message,
                detail: self.detail,
            },
        )
    }
}

impl From<TalonError> for ToolError {
    fn from(error: TalonError) -> Self {
        Self::new("talon", error.code(), error.to_string())
    }
}

/// Returns the MCP `tools/list` payload.
#[must_use]
pub fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": TOOL_NAME,
                "description": TOOL_DESCRIPTION,
                "inputSchema": input_schema()
            }
        ]
    })
}

/// Executes one MCP `tools/call` request.
#[must_use]
pub fn tools_call_result(params: Option<Value>) -> Value {
    let envelope = match parse_call_params(params).and_then(dispatch_arguments) {
        Ok(envelope) => envelope,
        Err(error) => error.envelope(),
    };
    content_result(&envelope)
}

fn parse_call_params(params: Option<Value>) -> Result<Value, ToolError> {
    let params = params.ok_or_else(|| {
        ToolError::new(
            "talon",
            ErrorCode::Internal,
            "tools/call requires params with name and arguments",
        )
    })?;
    let call: ToolCallParams = serde_json::from_value(params).map_err(|error| {
        ToolError::with_detail(
            "talon",
            ErrorCode::Internal,
            "invalid tools/call params",
            json!({ "message": error.to_string() }),
        )
    })?;
    if call.name != TOOL_NAME {
        return Err(ToolError::with_detail(
            "talon",
            ErrorCode::Internal,
            format!("unknown tool '{}'", call.name),
            json!({ "expected": TOOL_NAME }),
        ));
    }
    Ok(call.arguments)
}

fn dispatch_arguments(arguments: Value) -> Result<TalonEnvelope, ToolError> {
    let action = action_from_arguments(&arguments);
    let input: TalonInput = serde_json::from_value(arguments).map_err(|error| {
        ToolError::with_detail(
            action.unwrap_or("talon"),
            ErrorCode::Internal,
            "invalid talon tool arguments",
            json!({ "message": error.to_string() }),
        )
    })?;
    let action = action_name(&input);
    dispatch_input(input)
        .map_err(|error| ToolError::new(action, ErrorCode::Internal, format!("{error:#}")))
}

fn action_from_arguments(arguments: &Value) -> Option<&'static str> {
    let action = arguments.get("action")?.as_str()?;
    match action {
        "search" => Some("search"),
        "read" => Some("read"),
        "sync" => Some("sync"),
        "status" => Some("status"),
        "related" => Some("related"),
        "meta" => Some("meta"),
        "changes" => Some("changes"),
        "lint" => Some("lint"),
        _ => Some("talon"),
    }
}

const fn action_name(input: &TalonInput) -> &'static str {
    match input {
        TalonInput::Search(_) => "search",
        TalonInput::Read(_) => "read",
        TalonInput::Sync(_) => "sync",
        TalonInput::Status(_) => "status",
        TalonInput::Related(_) => "related",
        TalonInput::Meta(_) => "meta",
        TalonInput::Changes(_) => "changes",
        TalonInput::Lint(_) => "lint",
    }
}

fn dispatch_input(input: TalonInput) -> Result<TalonEnvelope> {
    match input {
        TalonInput::Search(input) => dispatch_search(&input),
        TalonInput::Read(input) => dispatch_read(&input),
        TalonInput::Sync(input) => dispatch_sync(&input),
        TalonInput::Status(input) => Ok(dispatch_status(input)),
        TalonInput::Related(input) => dispatch_related(&input),
        TalonInput::Meta(input) => dispatch_meta(&input),
        TalonInput::Changes(input) => dispatch_changes(&input),
        TalonInput::Lint(input) => dispatch_lint(&input),
    }
}

fn dispatch_search(input: &SearchInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let mode = input.mode;
    let fast = input.fast;
    let (inference, expansion) =
        if fast || mode == SearchMode::Fulltext || mode == SearchMode::Title {
            (None, None)
        } else {
            (
                InferenceClient::new(&config.inference.base_url).ok(),
                ExpansionClient::new(config.expansion.base_url.clone(), &config.expansion.model)
                    .ok(),
            )
        };
    let response = run_search(
        &conn,
        input,
        inference.as_ref(),
        expansion.as_ref(),
        Some(&config),
    );
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(response.total),
        warnings: Vec::new(),
        scope_set: Some(config.default_scope_names().into_iter().cloned().collect()),
        since: input.since.clone(),
    };
    Ok(TalonEnvelope::ok(
        "search",
        TalonResponseData::Search(response),
        meta,
    ))
}

fn dispatch_read(input: &ReadInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let response = run_read(&conn, &config.vault_path, input);
    let result_count = response
        .results
        .iter()
        .filter(|result| result.found)
        .count();
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(u32::try_from(result_count).unwrap_or(u32::MAX)),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "read",
        TalonResponseData::Read(response),
        meta,
    ))
}

fn dispatch_sync(input: &SyncInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let vault_path: PathBuf = config.vault_path.clone();
    let db_path: PathBuf = config.db_path.clone();
    let lock_path: PathBuf = db_path.parent().map_or_else(
        || PathBuf::from("sync.lock"),
        |parent| parent.join("sync.lock"),
    );
    let indexer_config = IndexerConfig {
        include_patterns: config.include_patterns.clone(),
        ignore_patterns: config.ignore_patterns.clone(),
    };

    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let mut conn = open_database(&db_path)
        .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;
    let (embed_opts, inference) = embed_options(&config, input)?;
    let (stats, embed_stats) = talon_core::run_sync_with_chunker(
        &mut conn,
        &vault_path,
        &lock_path,
        &indexer_config,
        embed_opts,
        inference.as_ref(),
        &config.chunker,
    )
    .wrap_err("sync failed")?;
    let (embedded, embed_failed, dimension_mismatch, embed_remediation, embed_diagnostics) =
        embed_stats.map_or((0, 0, false, None, Vec::new()), |stats| {
            (
                stats.succeeded,
                stats.failed,
                stats.dimension_mismatch,
                stats.remediation,
                stats.diagnostics,
            )
        });
    let response = SyncResponse {
        completed: true,
        status: SyncStatus::Ok,
        fast: input.fast,
        force: input.force,
        path_count: u32::try_from(input.paths.len()).unwrap_or(u32::MAX),
        indexed: stats.indexed,
        skipped: stats.skipped,
        deleted: stats.deleted,
        embedded,
        embed_failed,
        dimension_mismatch,
        embed_remediation,
        embed_diagnostics,
        duration_ms: elapsed_ms(started),
    };
    let meta = ResponseMeta {
        duration_ms: response.duration_ms,
        result_count: Some(response.indexed),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "sync",
        TalonResponseData::Sync(response),
        meta,
    ))
}

fn embed_options(
    config: &TalonConfig,
    input: &SyncInput,
) -> Result<(Option<EmbedPassOptions>, Option<InferenceClient>)> {
    if input.fast {
        return Ok((None, None));
    }
    let opts = EmbedPassOptions {
        force: input.force,
        restrict_paths: input.paths.clone(),
        chunk_embedding_model: config.inference.models.chunk_embedding.clone(),
        document_embedding_model: config.inference.models.document_embedding.clone(),
    };
    let inference =
        InferenceClient::new(&config.inference.base_url).wrap_err("building inference client")?;
    Ok((Some(opts), Some(inference)))
}

fn dispatch_status(_input: talon_core::StatusInput) -> TalonEnvelope {
    let started = Instant::now();
    let response = match config::load_config(None) {
        Ok(config) => match open_database(&config.db_path) {
            Ok(conn) => query_status(&conn, &config),
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

fn dispatch_related(input: &RelatedInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let response = find_related(&conn, input);
    let result_count = u32::try_from(response.results.len()).unwrap_or(u32::MAX);
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "related",
        TalonResponseData::Related(response),
        meta,
    ))
}

fn dispatch_meta(input: &MetaInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let since = input.since.clone();
    let response = query_meta(&conn, input);
    let result_count = u32::try_from(response.entries.len()).unwrap_or(u32::MAX);
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since,
    };
    Ok(TalonEnvelope::ok(
        "meta",
        TalonResponseData::Meta(response),
        meta,
    ))
}

fn dispatch_changes(input: &ChangesInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let since = input.since.clone();
    let response = query_changes(&conn, input);
    let result_count =
        u32::try_from(response.added.len() + response.modified.len() + response.deleted.len())
            .unwrap_or(u32::MAX);
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since: Some(since),
    };
    Ok(TalonEnvelope::ok(
        "changes",
        TalonResponseData::Changes(response),
        meta,
    ))
}

fn dispatch_lint(input: &LintInput) -> Result<TalonEnvelope> {
    let started = Instant::now();
    let config = config::load_config(None)?;
    let conn = open_database(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;
    let response = query_lint(&conn, input);
    let result_count = u32::try_from(response.findings.len()).unwrap_or(u32::MAX);
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(result_count),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    Ok(TalonEnvelope::ok(
        "lint",
        TalonResponseData::Lint(response),
        meta,
    ))
}

fn content_result(envelope: &TalonEnvelope) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string(envelope).unwrap_or_else(|_| "{}".to_owned())
            }
        ],
        "isError": !envelope.ok,
        "structuredContent": envelope
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["search", "read", "sync", "status", "related", "meta", "changes", "lint"]
            },
            "query": { "type": ["string", "null"] },
            "queries": { "type": "array", "items": { "type": "string" } },
            "mode": { "type": "string", "enum": ["hybrid", "semantic", "fulltext", "title"] },
            "fast": { "type": "boolean" },
            "limit": { "type": "integer", "minimum": 1 },
            "path": { "type": ["string", "null"] },
            "paths": { "type": "array", "items": { "type": "string" } },
            "raw": { "type": "boolean" },
            "fromLine": { "type": ["integer", "null"], "minimum": 1 },
            "maxLines": { "type": ["integer", "null"], "minimum": 1 },
            "force": { "type": "boolean" },
            "noWait": { "type": "boolean" },
            "depth": { "type": "integer", "minimum": 1, "maximum": 3 },
            "direction": { "type": "string", "enum": ["outgoing", "backlinks", "both"] },
            "scope": { "type": "array", "items": { "type": "string" } },
            "scopeOnly": { "type": "array", "items": { "type": "string" } },
            "where": { "type": "array", "items": { "$ref": "#/$defs/whereClause" } },
            "since": { "type": ["string", "null"] },
            "anchors": { "type": ["boolean", "null"], "description": "Include previewAnchors (BM25 + semantic) in each search result. Opt-in; adds one DB lookup per result." },
            "select": { "type": "array", "items": { "type": "string" } },
            "tagCounts": { "type": "boolean" },
            "sources": { "type": ["string", "null"] },
            "check": { "type": "string", "enum": ["orphans", "broken-links", "dangling-refs", "unreferenced"] }
        },
        "$defs": {
            "whereClause": {
                "type": "object",
                "required": ["key", "op"],
                "properties": {
                    "key": { "type": "string" },
                    "op": {
                        "type": "string",
                        "enum": ["equals", "not-equals", "less-than", "less-than-or-equal", "greater-than", "greater-than-or-equal", "contains", "exists"]
                    },
                    "value": { "type": ["string", "null"] }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{tools_call_result, tools_list_result};

    #[test]
    fn tools_list_returns_single_talon_tool_with_expected_actions() {
        let result = tools_list_result();
        let Some(tools) = result["tools"].as_array() else {
            panic!("tools array missing");
        };
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "talon");
        let Some(actions) = tools[0]["inputSchema"]["properties"]["action"]["enum"].as_array()
        else {
            panic!("action enum missing");
        };
        assert_eq!(actions.len(), 8);
        assert!(!actions.contains(&Value::String("embed".to_owned())));
        assert!(actions.contains(&Value::String("search".to_owned())));
        assert!(actions.contains(&Value::String("lint".to_owned())));
    }

    #[test]
    fn tools_call_rejects_unknown_tool_name() {
        let result = tools_call_result(Some(json!({
            "name": "other",
            "arguments": { "action": "status" }
        })));

        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["ok"], false);
    }

    #[test]
    fn tools_call_wraps_invalid_arguments_in_error_envelope() {
        let result = tools_call_result(Some(json!({
            "name": "talon",
            "arguments": { "action": "embed" }
        })));

        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["ok"], false);
        assert_eq!(result["structuredContent"]["action"], "talon");
    }
}
