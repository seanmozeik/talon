use super::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("build runtime: {err}"))
}

#[test]
fn strip_code_fences_removes_markdown_wrapper() {
    let wrapped = "```json\n{\"answer\":\"ok\"}\n```";
    let cleaned = strip_code_fences(wrapped);
    assert_eq!(cleaned, "{\"answer\":\"ok\"}");
}

#[test]
fn reasoning_effort_accepts_off_alias_as_none() {
    let effort: ReasoningEffort = serde_json::from_str("\"off\"")
        .unwrap_or_else(|err| panic!("reasoning effort should parse: {err}"));
    assert_eq!(effort, ReasoningEffort::None);
    let serialized = serde_json::to_string(&effort)
        .unwrap_or_else(|err| panic!("reasoning effort should serialize: {err}"));
    assert_eq!(serialized, "\"none\"");
}

#[test]
fn request_sends_reasoning_effort_kwargs_and_token_cap() {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "ok"
                    }
                }]
            })))
            .mount(&server),
    );
    let client = ChatClient::with_timeout_and_max_tokens(
        server.uri(),
        "test-model",
        DEFAULT_CHAT_TIMEOUT,
        Some(2048),
    )
    .unwrap_or_else(|err| panic!("build chat client: {err}"))
    .with_reasoning_effort(ReasoningEffort::None)
    .with_chat_template_kwargs(json!({ "enable_thinking": false }));
    let _ = client
        .complete_raw(vec![ChatMessage::new("user", "hello")], 0.0)
        .unwrap_or_else(|err| panic!("chat completion: {err}"));

    let requests = runtime
        .block_on(server.received_requests())
        .unwrap_or_else(|| panic!("received requests"));
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body)
        .unwrap_or_else(|err| panic!("parse request body: {err}"));
    assert_eq!(body["max_tokens"].as_u64(), Some(2048));
    assert_eq!(body["reasoning_effort"].as_str(), Some("none"));
    assert_eq!(
        body["chat_template_kwargs"]["enable_thinking"].as_bool(),
        Some(false)
    );
}
