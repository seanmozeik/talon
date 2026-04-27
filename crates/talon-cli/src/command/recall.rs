use super::{output_mode, should_spin};
use crate::cli::CliArgs;
use crate::config;
use crate::output::{emit_response, format_recall_prompt_xml};
use crate::spinner;
use eyre::{Result, WrapErr as _, bail};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    ExpansionClient, RecallInput, RecallResponse, ResponseMeta, TalonEnvelope, TalonResponseData,
    inference::InferenceClient, open_database, run_recall, vec_ext::register_sqlite_vec,
};

pub(super) async fn emit(args: &CliArgs, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        bail!("recall requires a message; usage: talon recall <message...>");
    }

    let message = rest.join(" ");
    let fast = args.fast.enabled();
    let prompt_xml = args.recall.format.as_deref() == Some("prompt-xml");

    let input = RecallInput {
        message,
        prior_messages: args.recall.prior_messages.clone(),
        budget_tokens: args.recall.budget_tokens.unwrap_or(2000),
        exclude: args.recall.exclude.clone(),
        scope: Vec::new(),
        scope_only: Vec::new(),
        since: args.since.clone(),
        format: if prompt_xml {
            talon_core::RecallFormat::PromptXml
        } else {
            talon_core::RecallFormat::Json
        },
        depth: args.depth.unwrap_or(1),
        recency_half_life_days: args.recall.recency_half_life_days.unwrap_or(7),
        min_confidence: args.recall.min_confidence.unwrap_or(0.0),
        fast,
    };

    let started = Instant::now();
    let config = config::load_config(args.config_file.as_deref()).ok();

    let work = async move {
        tokio::task::spawn_blocking(move || -> Result<(RecallResponse, ResponseMeta, String)> {
            let db_path: PathBuf = config
                .as_ref()
                .map_or_else(crate::config::default_db_path, |c| c.db_path.clone());
            register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
            let conn = open_database(&db_path)
                .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

            let (inference, expansion) = if fast {
                (None, None)
            } else {
                let inference_url = config.as_ref().map_or_else(
                    || "http://localhost:8080".to_string(),
                    |c| c.inference.base_url.clone(),
                );
                let inference = InferenceClient::new(inference_url).ok();
                let expansion = config
                    .as_ref()
                    .map(|c| {
                        ExpansionClient::with_max_tokens(
                            c.expansion.base_url.clone(),
                            &c.expansion.model,
                            c.expansion.max_tokens,
                        )
                    })
                    .transpose()
                    .ok()
                    .flatten();
                (inference, expansion)
            };

            let response = run_recall(
                &conn,
                inference.as_ref(),
                expansion.as_ref(),
                &input,
                config.as_ref().map(|c| c as &talon_core::TalonConfig),
            );

            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let result_count = response
                .vault_recall
                .as_ref()
                .map(|r| u32::try_from(r.active_notes.len()).unwrap_or(u32::MAX));
            let vault = config
                .as_ref()
                .map_or_else(String::new, |c| c.vault_path.to_string_lossy().into_owned());
            let meta = ResponseMeta {
                duration_ms,
                result_count,
                warnings: Vec::new(),
                scope_set: config
                    .as_ref()
                    .map(|c| c.default_scope_names().into_iter().cloned().collect()),
                since: input.since,
            };
            Ok((response, meta, vault))
        })
        .await
        .wrap_err("recall task join failed")?
    };

    let (recall_resp, meta, vault) = if should_spin(args) && !prompt_xml {
        spinner::with_spinner("Recalling...".to_string(), work).await?
    } else {
        work.await?
    };

    if prompt_xml {
        format_recall_prompt_xml(&mut std::io::stdout(), &recall_resp, &vault)?;
        return Ok(());
    }

    let envelope = TalonEnvelope::ok("recall", TalonResponseData::Recall(recall_resp), meta);
    emit_response(&envelope, output_mode(args))
}
