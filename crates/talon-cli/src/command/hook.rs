use std::io::{self, Read as _, Write as _};

use eyre::{Result, WrapErr};
use serde_json::{Value, json};
use talon_core::{RecallInput, ScopeFilter};

use crate::cli::{Cli, HookArgs, HookRecallArgs, HookSubcommand};
use crate::mcp::state::HostKind;
use crate::mcp::tool::hook_recall::{build_host_hook_json_text, dispatch_recall_for_hook};

pub(super) async fn emit(args: &HookArgs, cli: &Cli) -> Result<()> {
    match &args.subcommand {
        HookSubcommand::Recall(recall_args) => emit_recall(recall_args, cli).await,
    }
}

async fn emit_recall(args: &HookRecallArgs, cli: &Cli) -> Result<()> {
    let hook_input = read_hook_input().unwrap_or_else(|error| {
        eprintln!("talon hook recall: failed to read hook input: {error}");
        Value::Null
    });
    let Some(message) = pick_message(&hook_input) else {
        return write_continue_only();
    };

    let config_file = cli.config_file.clone();
    let scope = args.scope.clone();
    let budget_tokens = args.budget_tokens;
    let fast = cli.fast || args.fast;
    let host = HostKind::parse(&args.host);

    let Some(hook_json) = tokio::task::spawn_blocking(move || {
        build_recall_hook_json(
            config_file.as_deref(),
            message,
            scope,
            budget_tokens,
            fast,
            &host,
        )
    })
    .await
    .wrap_err("hook recall task join failed")?
    else {
        return write_continue_only();
    };

    writeln!(io::stdout().lock(), "{hook_json}")?;
    Ok(())
}

fn build_recall_hook_json(
    config_file: Option<&std::path::Path>,
    message: String,
    scope: Vec<String>,
    budget_tokens: u32,
    fast: bool,
    host: &HostKind,
) -> Option<String> {
    let config = match crate::config::load_config(config_file) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("talon hook recall: failed to load config: {error:?}");
            return None;
        }
    };

    if let Err(error) = ScopeFilter::from_args(&config, &scope, &Vec::new(), false) {
        eprintln!("talon hook recall: invalid scope: {error}");
        return None;
    }

    let input = RecallInput {
        message,
        prior_messages: Vec::new(),
        budget_tokens,
        exclude: Vec::new(),
        scope,
        scope_only: Vec::new(),
        scope_all: false,
        format: talon_core::RecallFormat::default(),
        depth: 1,
        min_confidence: 0.0,
        fast,
        diagnostics: true,
        deadline_ms: Some(config.mcp.hooks.recall_deadline_ms),
    };

    let recall_response = match dispatch_recall_for_hook(&input, &config) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("talon hook recall: recall failed: {error:?}");
            return None;
        }
    };

    let vault = config.vault_path.to_string_lossy();
    Some(build_host_hook_json_text(&recall_response, &vault, host))
}

fn read_hook_input() -> Result<Value> {
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .wrap_err("reading stdin")?;
    if raw.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&raw).wrap_err("parsing hook JSON")
}

fn pick_message(input: &Value) -> Option<String> {
    let message = ["prompt", "message", "user_prompt", "userPrompt", "text"]
        .into_iter()
        .find_map(|key| input.get(key).and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_owned();
    (!message.is_empty()).then_some(message)
}

fn write_continue_only() -> Result<()> {
    writeln!(io::stdout().lock(), "{}", json!({ "continue": true }))?;
    Ok(())
}
