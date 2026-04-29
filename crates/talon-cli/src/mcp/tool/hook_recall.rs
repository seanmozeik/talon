use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr as _};
use serde_json::{Value, json};
use talon_core::{
    ExpansionClient, RecallInput, RecallResponse, TalonConfig, inference::InferenceClient,
    vec_ext::register_sqlite_vec,
};

use crate::mcp::session::chunk_id::derive_chunk_id;
use crate::mcp::session::fingerprint::QueryFingerprint;
use crate::mcp::session::ledger::{InjectedChunk, TurnLedger, TurnRecord};
use crate::mcp::session::suppression::{RecallCandidate, apply_suppression};
use crate::mcp::state::{McpServerState, SessionKey};
use crate::output::format_recall_prompt_xml;

/// Runs recall using a pre-loaded `TalonConfig`, mirroring the logic in
/// `dispatch.rs`'s `dispatch_recall` but without loading config from disk.
pub(super) fn dispatch_recall_for_hook(
    input: &RecallInput,
    config: &TalonConfig,
) -> Result<RecallResponse> {
    use talon_core::{open_database_read_only, run_recall};

    register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
    let conn = open_database_read_only(&config.db_path)
        .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;

    let (inference, expansion) = if input.fast {
        (None, None)
    } else {
        talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
        (
            InferenceClient::with_rerank_options_and_protocol(
                &config.inference.base_url,
                config.search.rerank_batch_size,
                config.search.rerank_max_tokens,
                config.inference.rerank,
            )
            .ok(),
            ExpansionClient::with_max_tokens(
                config.expansion.base_url.clone(),
                &config.expansion.model,
                config.expansion.max_tokens,
            )
            .ok(),
        )
    };

    Ok(run_recall(
        &conn,
        inference.as_ref(),
        expansion.as_ref(),
        input,
        Some(config),
    ))
}

/// Formats a `RecallResponse` into the MCP tool result for `talon_hook_recall`.
pub(super) fn build_recall_output(
    recall_response: &RecallResponse,
    format: &str,
    vault: &str,
) -> Value {
    match format {
        "hook-json" => {
            if recall_response.skipped {
                let hook_output = json!({
                    "hookSpecificOutput": {
                        "hookEventName": "UserPromptSubmit"
                    }
                });
                let text = serde_json::to_string(&hook_output).unwrap_or_else(|_| "{}".to_owned());
                json!({ "content": [{ "type": "text", "text": text }] })
            } else {
                let xml_string = render_prompt_xml(recall_response, vault);
                let hook_output = json!({
                    "hookSpecificOutput": {
                        "hookEventName": "UserPromptSubmit",
                        "additionalContext": xml_string
                    }
                });
                let text = serde_json::to_string(&hook_output).unwrap_or_else(|_| "{}".to_owned());
                json!({ "content": [{ "type": "text", "text": text }] })
            }
        }
        "prompt-xml" => {
            let xml_string = render_prompt_xml(recall_response, vault);
            json!({ "content": [{ "type": "text", "text": xml_string }] })
        }
        "agent-json" => {
            use talon_core::{ResponseMeta, TalonEnvelope, TalonResponseData};
            let result_count = recall_response
                .vault_recall
                .as_ref()
                .map(|r| u32::try_from(r.active_notes.len()).unwrap_or(u32::MAX));
            let envelope = TalonEnvelope::ok(
                "recall",
                TalonResponseData::Recall(recall_response.clone()),
                ResponseMeta {
                    duration_ms: 0,
                    result_count,
                    warnings: Vec::new(),
                    scope_set: None,
                    since: None,
                },
            );
            let text = crate::output::json::agent::to_agent_value(&envelope)
                .and_then(|v| serde_json::to_string(&v).ok())
                .unwrap_or_else(|| serde_json::to_string(&envelope).unwrap_or_default());
            json!({ "content": [{ "type": "text", "text": text }] })
        }
        _ => {
            // Unknown format — fall back to hook-json behaviour.
            build_recall_output(recall_response, "hook-json", vault)
        }
    }
}

/// Renders `recall_response` as prompt XML, returning an empty string on error.
fn render_prompt_xml(recall_response: &RecallResponse, vault: &str) -> String {
    let mut buf = Vec::new();
    if format_recall_prompt_xml(&mut buf, recall_response, vault).is_ok() {
        String::from_utf8(buf).unwrap_or_default()
    } else {
        String::new()
    }
}

/// Applies turn-aware suppression to a `RecallResponse`, records the turn in
/// the session ledger, and returns a filtered response containing only the
/// chunks that were not suppressed.
pub(super) fn apply_recall_suppression(
    mut recall_response: RecallResponse,
    state: &Arc<McpServerState>,
    key: &SessionKey,
    message: &str,
    turn_id: String,
    budget_tokens: u32,
) -> RecallResponse {
    use std::collections::HashSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Build suppression candidates from active notes.
    let candidates: Vec<RecallCandidate> = recall_response
        .vault_recall
        .as_ref()
        .map(|vr| {
            vr.active_notes
                .iter()
                .map(|note| {
                    let rank = usize::try_from(note.rank).unwrap_or(0);
                    let chunk_id = derive_chunk_id(note.vault_path.as_str(), rank, &note.snippet);
                    RecallCandidate {
                        chunk_id,
                        path: note.vault_path.as_str().to_owned(),
                        score: note.score,
                        title: note.title.clone(),
                        snippet: note.snippet.clone(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let fp = QueryFingerprint::from_message(message);

    // Apply suppression against the session ledger (read lock).
    let suppression_result = {
        let store = state
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = store.sessions.get(key) {
            apply_suppression(candidates, &session.ledger, &fp, budget_tokens)
        } else {
            // No session found — inject everything, suppress nothing.
            apply_suppression(candidates, &TurnLedger::new(), &fp, budget_tokens)
        }
    };

    // Build ledger record from suppression result.
    let injected_chunks: Vec<InjectedChunk> = suppression_result
        .injected
        .iter()
        .map(|(c, _)| InjectedChunk {
            chunk_id: c.chunk_id.clone(),
            path: c.path.clone(),
            score: c.score,
        })
        .collect();
    let injected_paths: HashSet<String> = suppression_result
        .injected
        .iter()
        .map(|(c, _)| c.path.clone())
        .collect();

    let turn_record = TurnRecord {
        turn_id,
        query_fingerprint: fp.normalized,
        injected: injected_chunks,
        suppressed: suppression_result.suppressed,
        skipped: recall_response.skipped,
    };

    // Record turn to ledger (write lock).
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    {
        let mut store = state
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = store.sessions.get_mut(key) {
            session.ledger.record_turn(turn_record);
            session.last_seen_at_ms = now_ms;
        }
    }

    // Filter active_notes to only injected paths.
    if let Some(vr) = recall_response.vault_recall.as_mut() {
        vr.active_notes
            .retain(|note| injected_paths.contains(note.vault_path.as_str()));
    }

    recall_response
}
