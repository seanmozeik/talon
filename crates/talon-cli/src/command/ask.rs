use super::{ask_client, ask_sources, output_mode, search, should_spin};
use crate::cli::{AskArgs, Cli};
use crate::config::{self, vault_container_path};
use crate::output::{RenderOptions, emit_response, format_ask_human};
use crate::spinner;
use crate::telemetry::elapsed_ms;
use eyre::{Result, WrapErr as _, bail};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;
use talon_core::{
    AskClient, AskDiagnostics, AskLlmStageDiagnostics, AskResponse, AskSearchDiagnostics,
    AskSource, ResponseMeta, SearchMode, TalonEnvelope, TalonResponseData, estimate_tokens,
};

const ASK_QUERY_LIMIT: u8 = 6;
const ASK_SYNTHESIS_INPUT_NUMERATOR: usize = 3;
const ASK_SYNTHESIS_INPUT_DENOMINATOR: usize = 10;
const ASK_SYNTHESIS_FIXED_OVERHEAD_TOKENS: usize = 512;

pub(super) async fn emit(args: &AskArgs, cli: &Cli) -> Result<()> {
    if args.question.is_empty() {
        bail!("ask requires a question");
    }

    let question = args.question.join(" ");
    let mode = args
        .shared
        .mode
        .map_or_else(SearchMode::default, std::convert::Into::into);
    let config = config::load_config(cli.config_file.as_deref())?;
    let fast = cli.fast;
    let input = search::build_search_input(question.clone(), &args.shared, &config, fast)?;
    let started = Instant::now();
    let verbose = cli.verbose;
    let progress =
        should_spin(cli).then(|| Arc::new(Mutex::new("Plotting searches...".to_string())));
    let work_progress = progress.clone();

    let work = async move {
        tokio::task::spawn_blocking(move || {
            execute_ask(
                AskRun {
                    question,
                    input,
                    started,
                    mode,
                    verbose,
                    fast,
                },
                &config,
                work_progress.as_deref(),
            )
        })
        .await
        .wrap_err("ask task join failed")?
    };

    let response = if let Some(progress) = progress {
        spinner::with_dynamic_spinner(progress, work).await?
    } else {
        work.await?
    };
    if crate::banner::should_clear_fancy_prelude(cli) {
        crate::banner::clear_fancy_prelude();
    }

    if output_mode(cli) == crate::output::OutputMode::Human
        && let Some(TalonResponseData::Ask(resp)) = response.data.as_ref()
    {
        let warnings = response
            .meta
            .as_ref()
            .map_or(&[][..], |m| m.warnings.as_slice());
        return format_ask_human(
            &mut io::stdout(),
            resp,
            RenderOptions::for_terminal(),
            warnings,
        );
    }
    emit_response(&response, output_mode(cli))
}

struct AskRun {
    question: String,
    input: talon_core::SearchInput,
    started: Instant,
    mode: SearchMode,
    verbose: bool,
    fast: bool,
}

fn execute_ask(
    run: AskRun,
    config: &talon_core::TalonConfig,
    progress: Option<&Mutex<String>>,
) -> Result<TalonEnvelope> {
    let AskRun {
        question,
        mut input,
        started,
        mode,
        verbose,
        fast,
    } = run;
    let ask = ask_client::build_ask_client(config, fast)?;
    set_progress(progress, "Plotting searches...");
    let planned = run_planning(&ask, &question, verbose)?;
    let mut queries = Vec::with_capacity(planned.plan.queries.len() + 1);
    queries.push(question.clone());
    queries.extend(planned.plan.queries.clone());
    input.queries.clone_from(&queries);

    set_progress(progress, "Searching notes...");
    let (search_response, search_meta, search_ms) =
        run_search_stage(input, config, mode, queries.len(), verbose, fast)?;
    let mut sources = ask_sources::build_ask_sources(&search_response, config, &queries)?;
    trim_ask_sources_to_budget(&question, &queries, &mut sources, config);
    set_progress(progress, "Distilling answer...");
    let synthesized = run_synthesis(&ask, &question, &queries, &sources, verbose)?;

    let response = AskResponse {
        vault: vault_container_path(Some(config)),
        question,
        answer: synthesized.answer,
        queries,
        sources,
        diagnostics: verbose.then_some(AskDiagnostics {
            endpoint: ask.base_url().to_string(),
            model: ask.model().to_string(),
            planning: AskLlmStageDiagnostics {
                duration_ms: planned.duration_ms,
                content: planned.plan.content,
            },
            search: AskSearchDiagnostics {
                duration_ms: search_ms,
                total: search_response.total,
            },
            synthesis: synthesized.diagnostics,
        }),
    };
    let meta = ResponseMeta {
        duration_ms: elapsed_ms(started),
        result_count: Some(response.sources.len().try_into().unwrap_or(u32::MAX)),
        warnings: search_meta
            .as_ref()
            .map_or_else(Vec::new, |m| m.warnings.clone()),
        scope_set: search_meta.as_ref().and_then(|m| m.scope_set.clone()),
        since: search_meta.and_then(|m| m.since),
    };
    Ok(TalonEnvelope::ok(
        "ask",
        TalonResponseData::Ask(response),
        meta,
    ))
}

fn trim_ask_sources_to_budget(
    question: &str,
    queries: &[String],
    sources: &mut Vec<AskSource>,
    config: &talon_core::TalonConfig,
) {
    let output_reserve = usize::try_from(config.ask.max_output_tokens).unwrap_or(2_048);
    let context = usize::try_from(config.ask.context_tokens).unwrap_or(usize::MAX);
    let input_budget = context
        .saturating_mul(ASK_SYNTHESIS_INPUT_NUMERATOR)
        .checked_div(ASK_SYNTHESIS_INPUT_DENOMINATOR)
        .unwrap_or(context)
        .min(
            context
                .saturating_sub(output_reserve)
                .saturating_sub(ASK_SYNTHESIS_FIXED_OVERHEAD_TOKENS),
        )
        .max(256);
    let fixed_tokens = ask_synthesis_fixed_tokens(question, queries);
    let source_budget = input_budget.saturating_sub(fixed_tokens);

    let mut used = 0_usize;
    sources.retain(|source| {
        let source_tokens = ask_source_tokens(source);
        if used.saturating_add(source_tokens) <= source_budget {
            used = used.saturating_add(source_tokens);
            true
        } else {
            false
        }
    });
}

#[cfg(test)]
fn ask_synthesis_tokens(question: &str, queries: &[String], sources: &[AskSource]) -> usize {
    ask_synthesis_fixed_tokens(question, queries)
        + sources.iter().map(ask_source_tokens).sum::<usize>()
}

fn ask_synthesis_fixed_tokens(question: &str, queries: &[String]) -> usize {
    estimate_tokens(question)
        + queries
            .iter()
            .map(|query| estimate_tokens(query))
            .sum::<usize>()
}

fn ask_source_tokens(source: &AskSource) -> usize {
    estimate_tokens(source.vault_path.as_str())
        + estimate_tokens(&source.title)
        + estimate_tokens(&source.snippet)
        + 8
}

fn set_progress(progress: Option<&Mutex<String>>, label: &str) {
    if let Some(progress) = progress {
        *progress.lock().unwrap_or_else(PoisonError::into_inner) = label.to_string();
    }
}

struct TimedPlan {
    plan: talon_core::AskPlan,
    duration_ms: u64,
}

fn run_planning(ask: &AskClient, question: &str, verbose: bool) -> Result<TimedPlan> {
    if verbose {
        eprintln!(
            "ask: planning queries with model={} endpoint={}",
            ask.model(),
            ask.base_url()
        );
    }
    let planning_started = Instant::now();
    let plan = ask
        .plan_queries_detailed(question, ASK_QUERY_LIMIT)
        .wrap_err_with(|| {
            format!(
                "ask query planning failed after {}ms",
                elapsed_ms(planning_started)
            )
        })?;
    let duration_ms = elapsed_ms(planning_started);
    if verbose {
        eprintln!("ask: planning completed in {duration_ms}ms");
        eprintln!("ask: planner content:\n{}", plan.content);
        if let Some(reasoning) = plan.reasoning_content.as_deref() {
            eprintln!("ask: planner reasoning_content:\n{reasoning}");
        }
        eprintln!("ask: planner raw response:\n{}", plan.raw_response);
        eprintln!("ask: parsed planner queries: {:?}", plan.queries);
    }
    Ok(TimedPlan { plan, duration_ms })
}

fn run_search_stage(
    input: talon_core::SearchInput,
    config: &talon_core::TalonConfig,
    mode: SearchMode,
    query_count: usize,
    verbose: bool,
    fast: bool,
) -> Result<(talon_core::SearchResponse, Option<ResponseMeta>, u64)> {
    if verbose {
        eprintln!("ask: running search over {query_count} queries");
    }
    let search_started = Instant::now();
    let search_envelope = search::execute_search(input, config, search_started, fast, mode, true)?;
    let search_ms = elapsed_ms(search_started);
    let search_meta = search_envelope.meta.clone();
    let Some(TalonResponseData::Search(search_response)) = search_envelope.into_data() else {
        bail!("ask search stage returned non-search response");
    };
    if verbose {
        eprintln!(
            "ask: search completed in {search_ms}ms with {} total results",
            search_response.total
        );
    }
    Ok((search_response, search_meta, search_ms))
}

struct TimedSynthesis {
    answer: String,
    diagnostics: Option<AskLlmStageDiagnostics>,
}

fn run_synthesis(
    ask: &AskClient,
    question: &str,
    queries: &[String],
    sources: &[AskSource],
    verbose: bool,
) -> Result<TimedSynthesis> {
    if sources.is_empty() {
        return Ok(TimedSynthesis {
            answer: "I couldn't find enough relevant vault notes to answer that.".to_string(),
            diagnostics: None,
        });
    }
    if verbose {
        eprintln!("ask: synthesizing answer from {} sources", sources.len());
    }
    let synthesis_started = Instant::now();
    let synthesis = ask
        .synthesize_detailed(question, queries, sources)
        .wrap_err_with(|| {
            format!(
                "ask answer synthesis failed after {}ms",
                elapsed_ms(synthesis_started)
            )
        })?;
    let synthesis_ms = elapsed_ms(synthesis_started);
    if verbose {
        eprintln!("ask: synthesis completed in {synthesis_ms}ms");
        eprintln!("ask: synthesis content:\n{}", synthesis.content);
        if let Some(reasoning) = synthesis.reasoning_content.as_deref() {
            eprintln!("ask: synthesis reasoning_content:\n{reasoning}");
        }
        eprintln!("ask: synthesis raw response:\n{}", synthesis.raw_response);
    }
    Ok(TimedSynthesis {
        answer: synthesis.answer,
        diagnostics: Some(AskLlmStageDiagnostics {
            duration_ms: synthesis_ms,
            content: synthesis.content,
        }),
    })
}

#[cfg(test)]
#[path = "ask_tests.rs"]
mod ask_tests;
