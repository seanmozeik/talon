use serde_json::json;

use super::{tools_call_result, tools_list_result};

#[test]
fn tools_list_includes_named_tools() {
    let result = tools_list_result();
    let Some(tools) = result["tools"].as_array() else {
        panic!("tools array missing");
    };
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"talon_search"), "missing talon_search");
    assert!(names.contains(&"talon_read"), "missing talon_read");
    assert!(names.contains(&"talon_related"), "missing talon_related");
    assert!(
        !names.contains(&"talon_ask"),
        "talon_ask should stay CLI-only"
    );
    assert!(
        !names.contains(&"talon"),
        "generic talon MCP tool should not be exposed"
    );
    assert_eq!(
        names.iter().filter(|name| **name == "talon_search").count(),
        1,
        "talon_search should be listed once"
    );
}

#[test]
fn search_tool_documents_default_scope_exclusions() {
    let result = tools_list_result();
    let Some(tools) = result["tools"].as_array() else {
        panic!("tools array missing");
    };
    let Some(search) = tools.iter().find(|tool| tool["name"] == "talon_search") else {
        panic!("talon_search should be listed");
    };

    let Some(description) = search["description"].as_str() else {
        panic!("search description should be a string");
    };
    assert!(description.contains("default = false"));
    assert!(description.contains("scopeAll: true"));
    assert!(description.contains("recall-injected paths"));

    let Some(scope_all_description) =
        search["inputSchema"]["properties"]["scopeAll"]["description"].as_str()
    else {
        panic!("scopeAll description should be a string");
    };
    assert!(scope_all_description.contains("raw/"));
    assert!(scope_all_description.contains("default = false"));
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

#[test]
fn named_tool_call_search_returns_compact_agent_shape() {
    // Successful named-tool calls must surface the compact agent-contract shape
    // in BOTH `content[0].text` and `structuredContent`. MCP clients that prefer
    // structured output (Claude Code among them) read `structuredContent` first;
    // if it carried the full TalonEnvelope, agents would silently load the
    // verbose shape and burn tokens.
    let result = tools_call_result(Some(json!({
        "name": "talon_search",
        "arguments": { "query": "test" }
    })));

    assert!(result.is_object());
    assert!(result["content"].is_array());
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].is_string());
    assert!(result["structuredContent"].is_object());

    if result["isError"] == json!(true) {
        // Error envelope (no compact form) — falls back to the full envelope.
        assert_eq!(result["structuredContent"]["ok"], false);
        assert_eq!(result["structuredContent"]["action"], "talon_search");
        assert!(result["structuredContent"]["error"].is_object());
    } else {
        // Compact AgentSearchResponse: only `vault?` + `results`. Verbose
        // envelope keys must NOT appear.
        assert!(result["structuredContent"]["results"].is_array());
        assert!(result["structuredContent"]["ok"].is_null());
        assert!(result["structuredContent"]["action"].is_null());
        assert!(result["structuredContent"]["error"].is_null());
        assert!(result["structuredContent"]["version"].is_null());
        assert!(result["structuredContent"]["meta"].is_null());
    }
}

#[test]
fn named_tool_call_read_returns_compact_agent_shape() {
    // Same contract as search: structuredContent must be the compact agent
    // shape on success, and the full envelope only when there is no compact
    // representation (error path).
    let result = tools_call_result(Some(json!({
        "name": "talon_read",
        "arguments": { "path": "test.md" }
    })));

    assert!(result.is_object());
    assert!(result["content"].is_array());
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].is_string());
    assert!(result["structuredContent"].is_object());

    if result["isError"] == json!(true) {
        assert_eq!(result["structuredContent"]["ok"], false);
        assert_eq!(result["structuredContent"]["action"], "talon_read");
        assert!(result["structuredContent"]["error"].is_object());
    } else {
        assert!(result["structuredContent"]["ok"].is_null());
        assert!(result["structuredContent"]["action"].is_null());
        assert!(result["structuredContent"]["error"].is_null());
        assert!(result["structuredContent"]["version"].is_null());
    }
}
