use std::sync::Arc;

use color_eyre::eyre::{Result, WrapErr as _};
use serde_json::{Value, json};
use talon_core::{
    ExpansionClient, RecallInput, RecallResponse, TalonConfig, inference::InferenceClient,
    vec_ext::register_sqlite_vec,
};

use crate::mcp::session::chunk_id::derive_chunk_id;
use crate::mcp::session::ledger::{InjectedChunk, TurnLedger, TurnRecord};
use crate::mcp::session::suppression::{RecallCandidate, apply_suppression, to_injected_chunk};
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

/// Output format for hook recall responses.
#[derive(Copy, Clone)]
pub(super) enum RecallOutputFormat {
    HookJson,
    PromptXml,
    AgentJson,
}

impl RecallOutputFormat {
    pub(super) fn from_str(s: &str) -> Self {
        match s {
            "prompt-xml" => Self::PromptXml,
            "agent-json" => Self::AgentJson,
            _ => Self::HookJson,
        }
    }
}

/// Formats a `RecallResponse` into the MCP tool result for `talon_hook_recall`.
pub(super) fn build_recall_output(
    recall_response: &RecallResponse,
    format: RecallOutputFormat,
    vault: &str,
) -> Value {
    match format {
        RecallOutputFormat::HookJson => {
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
        RecallOutputFormat::PromptXml => {
            let xml_string = render_prompt_xml(recall_response, vault);
            json!({ "content": [{ "type": "text", "text": xml_string }] })
        }
        RecallOutputFormat::AgentJson => {
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

    // Compute now_ms before acquiring the lock.
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));

    // Single write lock: read ledger for suppression, build turn record, record turn.
    // Avoids the double-lock (read then write) that existed previously.
    let suppression_result = {
        let mut store = state
            .sessions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = store.sessions.get_mut(key) {
            let result = apply_suppression(candidates, &session.ledger, session.suppression_decay);
            let injected_chunks: Vec<InjectedChunk> =
                result.injected.iter().map(to_injected_chunk).collect();
            let turn_record = TurnRecord {
                turn_id,
                query_fingerprint: message.to_owned(),
                injected: injected_chunks,
                suppressed: result.suppressed.clone(),
                skipped: recall_response.skipped || result.injected.is_empty(),
            };
            session.ledger.record_turn(turn_record);
            session.last_seen_at_ms = now_ms;
            result
        } else {
            apply_suppression(
                candidates,
                &TurnLedger::new(),
                crate::mcp::session::suppression::DEFAULT_DECAY,
            )
        }
    };

    // If all candidates were suppressed, skip injection entirely.
    // Do not fall back to lower-ranked results — the agent already has the
    // relevant context from previous turns.
    let all_suppressed = suppression_result.injected.is_empty();

    let injected_paths: HashSet<String> = suppression_result
        .injected
        .iter()
        .map(|c| c.path.clone())
        .collect();

    // If all candidates were suppressed, mark as skipped so build_recall_output
    // returns no additionalContext. Do not substitute lower-ranked results.
    if all_suppressed {
        recall_response.skipped = true;
        return recall_response;
    }

    if let Some(vr) = recall_response.vault_recall.as_mut() {
        // Linked notes require a higher gate than active notes: they lack snippets
        // and are less immediately useful. Only inject linked notes whose sources
        // were all injected and whose recomputed aggregate score clears this gate.
        const LINKED_CTX_GATE: f64 = 0.70;
        vr.active_notes
            .retain(|note| injected_paths.contains(note.vault_path.as_str()));

        vr.linked_context.retain_mut(|link| {
            // Drop sources that were suppressed, recompute aggregate score.
            link.source_notes
                .retain(|(sn, _)| injected_paths.contains(sn.as_str()));
            if link.source_notes.is_empty() {
                return false;
            }
            link.aggregated_score = link.source_notes.iter().map(|(_, s)| s).sum();
            link.aggregated_score >= LINKED_CTX_GATE
        });
        // Re-sort by recomputed score after suppression filtering.
        vr.linked_context.sort_by(|a, b| {
            b.aggregated_score
                .partial_cmp(&a.aggregated_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Hard cap: prevents score compounding (N sources × high affinity) from
        // flooding the injection regardless of active note count.
        vr.linked_context.truncate(5);
    }

    recall_response
}
