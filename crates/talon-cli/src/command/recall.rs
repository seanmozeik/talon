use super::{output_mode, should_spin};
use crate::cli::{Cli, RecallArgs};
use crate::config::{self, vault_container_path};
use crate::output::{emit_response, format_recall_prompt_xml};
use crate::spinner;
use crate::telemetry::{count_u32, elapsed_ms};
use eyre::{Result, WrapErr as _};
use std::time::Instant;
use talon_core::{
    ExpansionClient, RecallInput, RecallResponse, ResponseMeta, ScopeFilter, TalonEnvelope,
    TalonResponseData, inference::InferenceClient, open_database_read_only, run_recall,
    vec_ext::register_sqlite_vec,
};

fn recall_clients(
    config: &talon_core::TalonConfig,
) -> (Option<InferenceClient>, Option<ExpansionClient>) {
    talon_core::cache::rerank::configure_capacity(config.search.rerank_cache_size);
    let inference = InferenceClient::with_rerank_options_and_protocol(
        &config.inference.base_url,
        config.search.rerank_batch_size,
        config.search.rerank_max_tokens,
        config.inference.rerank,
    )
    .ok();
    let expansion = ExpansionClient::with_max_tokens(
        config.expansion.base_url.clone(),
        &config.expansion.model,
        config.expansion.max_output_tokens,
    )
    .ok();
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
    let config = config::load_config(cli.config_file.as_deref())?;
    ScopeFilter::from_args(&config, &input.scope, &input.scope_only, input.scope_all)
        .map_err(|e| eyre::eyre!("{e}"))?;

    let work = async move {
        tokio::task::spawn_blocking(move || -> Result<(RecallResponse, ResponseMeta, String)> {
            register_sqlite_vec().wrap_err("registering sqlite-vec extension")?;
            let conn = open_database_read_only(&config.db_path)
                .wrap_err_with(|| format!("opening index at {}", config.db_path.display()))?;

            let (inference, expansion) = if fast {
                (None, None)
            } else {
                recall_clients(&config)
            };

            let mut response = run_recall(
                &conn,
                inference.as_ref(),
                expansion.as_ref(),
                &input,
                Some(&config),
            );
            response.vault = vault_container_path(Some(&config));

            let duration_ms = elapsed_ms(started);
            let result_count = response
                .vault_recall
                .as_ref()
                .map(|r| count_u32(r.active_notes.len()));
            let vault = config.vault_path.to_string_lossy().into_owned();
            let scope_set =
                ScopeFilter::from_args(&config, &input.scope, &input.scope_only, input.scope_all)
                    .map_or_else(
                        |_| ScopeFilter::default_for(&config).resolved_set(),
                        |f| f.resolved_set(),
                    );
            let meta = ResponseMeta {
                duration_ms,
                result_count,
                warnings: Vec::new(),
                scope_set: Some(scope_set),
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
        if crate::banner::should_clear_fancy_prelude(cli) {
            crate::banner::clear_fancy_prelude();
        }
        let mut stdout = std::io::stdout().lock();
        format_recall_prompt_xml(&mut stdout, &recall_resp, &vault)?;
        return Ok(());
    }

    let envelope = TalonEnvelope::ok("recall", TalonResponseData::Recall(recall_resp), meta);
    if crate::banner::should_clear_fancy_prelude(cli) {
        crate::banner::clear_fancy_prelude();
    }
    emit_response(&envelope, output_mode(cli))
}
