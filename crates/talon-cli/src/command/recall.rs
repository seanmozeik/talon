use super::{output_mode, should_spin};
use crate::cli::{Cli, RecallArgs};
use crate::config::{self, vault_container_path};
use crate::output::{emit_response, format_recall_prompt_xml};
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::path::PathBuf;
use std::time::Instant;
use talon_core::{
    ExpansionClient, RecallInput, RecallResponse, ResponseMeta, ScopeFilter, TalonEnvelope,
    TalonResponseData, inference::InferenceClient, open_database_read_only, run_recall,
    vec_ext::register_sqlite_vec,
};

fn recall_clients(
    config: Option<&talon_core::TalonConfig>,
) -> (Option<InferenceClient>, Option<ExpansionClient>) {
    let inference_url = config.map_or_else(
        || "http://localhost:8080".to_string(),
        |c| c.inference.base_url.clone(),
    );
    if let Some(config) = config {
        talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
    }
    let inference = match config {
        Some(config) => InferenceClient::with_rerank_options(
            inference_url,
            config.search.rerank_batch_size,
            config.search.rerank_max_tokens,
        ),
        None => InferenceClient::new(inference_url),
    }
    .ok();
    let expansion = config
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
}

pub(super) async fn emit(args: &RecallArgs, cli: &Cli) -> Result<()> {
    let message = args.message.join(" ");
    let fast = cli.fast;
    let prompt_xml = args.format.as_deref() == Some("prompt-xml");

    let depth = args.depth.unwrap_or(1);
    let min_confidence = args.min_confidence.unwrap_or(0.4);

    let input = RecallInput {
        message,
        prior_messages: args.prior_messages.clone(),
        budget_tokens: args.budget_tokens.unwrap_or(500),
        exclude: args.exclude.clone(),
        scope: args.scope.scope.clone(),
        scope_only: args.scope.scope_only.clone(),
        scope_all: args.scope.scope_all,
        format: if prompt_xml {
            talon_core::RecallFormat::PromptXml
        } else {
            talon_core::RecallFormat::Json
        },
        depth,
        min_confidence,
        fast,
    };

    let started = Instant::now();
    let config = config::load_config(cli.config_file.as_deref()).ok();
    if let Some(cfg) = config.as_ref() {
        ScopeFilter::from_args(cfg, &input.scope, &input.scope_only, input.scope_all)
            .map_err(|e| eyre::eyre!("{e}"))?;
    }

    let work = async move {
        tokio::task::spawn_blocking(move || -> Result<(RecallResponse, ResponseMeta, String)> {
            let db_path: PathBuf = config
                .as_ref()
                .map_or_else(crate::config::default_db_path, |c| c.db_path.clone());
            register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
            let conn = open_database_read_only(&db_path)
                .wrap_err_with(|| format!("opening index at {}", db_path.display()))?;

            let (inference, expansion) = if fast {
                (None, None)
            } else {
                recall_clients(config.as_ref().map(|c| c as &talon_core::TalonConfig))
            };

            let mut response = run_recall(
                &conn,
                inference.as_ref(),
                expansion.as_ref(),
                &input,
                config.as_ref().map(|c| c as &talon_core::TalonConfig),
            );
            response.vault =
                vault_container_path(config.as_ref().map(|c| c as &talon_core::TalonConfig));

            let duration_ms = elapsed_ms(started);
            let result_count = response
                .vault_recall
                .as_ref()
                .map(|r| count_u32(r.active_notes.len()));
            let vault = config
                .as_ref()
                .map_or_else(String::new, |c| c.vault_path.to_string_lossy().into_owned());
            let meta = ResponseMeta {
                duration_ms,
                result_count,
                warnings: Vec::new(),
                scope_set: config.as_ref().map(|c| {
                    ScopeFilter::from_args(c, &input.scope, &input.scope_only, input.scope_all)
                        .map_or_else(
                            |_| ScopeFilter::default_for(c).resolved_set(),
                            |f| f.resolved_set(),
                        )
                }),
                since: None,
            };
            Ok((response, meta, vault))
        })
        .await
        .wrap_err("recall task join failed")?
    };

    let (recall_resp, meta, vault) = if should_spin(cli) && !prompt_xml {
        spinner::with_spinner("Recalling...".to_string(), work).await?
    } else {
        work.await?
    };

    if prompt_xml {
        format_recall_prompt_xml(&mut std::io::stdout(), &recall_resp, &vault)?;
        return Ok(());
    }

    let envelope = TalonEnvelope::ok("recall", TalonResponseData::Recall(recall_resp), meta);
    emit_response(&envelope, output_mode(cli))
}
