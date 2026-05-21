use eyre::{Result, WrapErr as _};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use talon_core::{AskClient, ChatClient, ReasoningEffort, runtime::build_ask_chat_client};

pub(super) fn build_ask_client(config: &talon_core::TalonConfig, fast: bool) -> Result<AskClient> {
    let expansion = &config.chat.expansion;
    let ask = &config.chat.ask;
    let ask_model = ask.resolved_model(expansion);
    let planning_effort = ask_reasoning_effort(ask.planning_reasoning_effort, fast);
    let synthesis_effort = ask_reasoning_effort(ask.synthesis_reasoning_effort, fast);
    let planning_enable_thinking =
        ask_enable_thinking(planning_effort, ask.planning_enable_thinking, fast);
    let synthesis_enable_thinking =
        ask_enable_thinking(synthesis_effort, ask.synthesis_enable_thinking, fast);
    let planning_chat = ask_chat_client(
        config,
        ask_model,
        Some(ask.max_output_tokens),
        planning_enable_thinking,
        planning_effort,
        ask_kwargs(ask.planning_chat_template_kwargs.as_ref(), fast),
    )?;
    let synthesis_chat = ask_chat_client(
        config,
        ask_model,
        Some(ask.max_output_tokens),
        synthesis_enable_thinking,
        synthesis_effort,
        ask_kwargs(ask.synthesis_chat_template_kwargs.as_ref(), fast),
    )?;
    Ok(AskClient::with_stage_clients(planning_chat, synthesis_chat))
}

const fn ask_reasoning_effort(
    configured: Option<ReasoningEffort>,
    fast: bool,
) -> Option<ReasoningEffort> {
    if fast {
        Some(ReasoningEffort::None)
    } else {
        configured
    }
}

fn ask_enable_thinking(
    reasoning_effort: Option<ReasoningEffort>,
    configured: Option<bool>,
    fast: bool,
) -> Option<bool> {
    if fast {
        Some(false)
    } else {
        reasoning_effort
            .map(ReasoningEffort::enables_thinking)
            .or(configured)
    }
}

const fn ask_kwargs(
    configured: Option<&BTreeMap<String, Value>>,
    fast: bool,
) -> Option<&BTreeMap<String, Value>> {
    if fast { None } else { configured }
}

fn ask_chat_client(
    config: &talon_core::TalonConfig,
    ask_model: &str,
    max_tokens: Option<u32>,
    enable_thinking: Option<bool>,
    reasoning_effort: Option<ReasoningEffort>,
    chat_template_kwargs: Option<&BTreeMap<String, Value>>,
) -> Result<ChatClient> {
    let mut chat = build_ask_chat_client(config, ask_model, max_tokens)
        .wrap_err("building ask chat client")?;
    if let Some(reasoning_effort) = reasoning_effort {
        chat = chat.with_reasoning_effort(reasoning_effort);
    }
    if let Some(kwargs) = merged_chat_template_kwargs(enable_thinking, chat_template_kwargs) {
        chat = chat.with_chat_template_kwargs(kwargs);
    }
    Ok(chat)
}

fn merged_chat_template_kwargs(
    enable_thinking: Option<bool>,
    chat_template_kwargs: Option<&BTreeMap<String, Value>>,
) -> Option<Value> {
    let mut merged: Map<String, Value> = chat_template_kwargs
        .into_iter()
        .flat_map(|kwargs| {
            kwargs
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
        })
        .collect();
    if let Some(enable_thinking) = enable_thinking {
        merged.insert("enable_thinking".to_string(), Value::Bool(enable_thinking));
    }
    (!merged.is_empty()).then_some(Value::Object(merged))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_overrides_reasoning_and_thinking() {
        assert_eq!(
            ask_reasoning_effort(Some(ReasoningEffort::High), true),
            Some(ReasoningEffort::None)
        );
        assert_eq!(
            ask_enable_thinking(Some(ReasoningEffort::High), Some(true), true),
            Some(false)
        );
    }

    #[test]
    fn explicit_enable_thinking_wins_in_fast_merge() {
        let mut configured = BTreeMap::new();
        configured.insert("enable_thinking".to_string(), Value::Bool(true));
        let merged = merged_chat_template_kwargs(Some(false), Some(&configured))
            .unwrap_or_else(|| panic!("merged kwargs"));
        assert_eq!(merged["enable_thinking"].as_bool(), Some(false));
    }
}
