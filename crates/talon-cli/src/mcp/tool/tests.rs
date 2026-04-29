use serde_json::{Value, json};

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
    // Legacy tool still present.
    assert!(names.contains(&"talon"), "missing deprecated talon tool");
}

#[test]
fn named_tool_call_search_missing_vault_returns_error_shape() {
    let result = tools_call_result(Some(json!({
        "name": "talon_search",
        "arguments": { "query": "test" }
    })));

    // Verify the response has the standard MCP shape.
    assert!(result.is_object());
    assert!(result["content"].is_array());
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].is_string());

    // Verify the structured content is a TalonEnvelope with error state.
    assert!(result["structuredContent"].is_object());
    assert_eq!(result["structuredContent"]["ok"], false);
    assert!(result["structuredContent"]["error"].is_object());
    assert!(result["structuredContent"]["error"]["code"].is_string());
    assert!(result["structuredContent"]["error"]["message"].is_string());

    // Verify error is propagated to top-level isError.
    assert_eq!(result["isError"], true);
}

#[test]
fn tools_list_returns_talon_tool_with_expected_actions() {
    let result = tools_list_result();
    let Some(tools) = result["tools"].as_array() else {
        panic!("tools array missing");
    };
    // Should have the deprecated talon tool plus the three named tools.
    assert!(tools.len() >= 4);
    let Some(talon_tool) = tools.iter().find(|t| t["name"] == "talon") else {
        panic!("deprecated talon tool missing");
    };
    let Some(actions) = talon_tool["inputSchema"]["properties"]["action"]["enum"].as_array() else {
        panic!("action enum missing");
    };
    assert_eq!(actions.len(), 9);
    assert!(!actions.contains(&Value::String("embed".to_owned())));
    assert!(actions.contains(&Value::String("search".to_owned())));
    assert!(actions.contains(&Value::String("lint".to_owned())));
    assert!(actions.contains(&Value::String("recall".to_owned())));
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
fn tools_call_search_missing_vault_returns_error_with_expected_shape() {
    // Test that calling search without a valid vault config returns an error
    // with the expected MCP response structure.
    let result = tools_call_result(Some(json!({
        "name": "talon",
        "arguments": { "action": "search", "query": "test" }
    })));

    // Verify the response has the standard MCP shape.
    assert!(result.is_object());
    assert!(result["content"].is_array());
    assert!(result["content"][0].is_object());
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].is_string());

    // Verify the structured content is a TalonEnvelope with error state.
    assert!(result["structuredContent"].is_object());
    assert_eq!(result["structuredContent"]["ok"], false);
    assert_eq!(result["structuredContent"]["action"], "search");
    assert!(result["structuredContent"]["error"].is_object());
    assert!(result["structuredContent"]["error"]["code"].is_string());
    assert!(result["structuredContent"]["error"]["message"].is_string());

    // Verify error is propagated to top-level isError.
    assert_eq!(result["isError"], true);
}

#[test]
fn tools_call_recall_missing_vault_returns_error_with_expected_shape() {
    // Test that calling recall without a valid vault config returns an error
    // with the expected MCP response structure.
    let result = tools_call_result(Some(json!({
        "name": "talon",
        "arguments": { "action": "recall", "message": "test" }
    })));

    // Verify the response has the standard MCP shape.
    assert!(result.is_object());
    assert!(result["content"].is_array());
    assert!(result["content"][0].is_object());
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].is_string());

    // Verify the structured content is a TalonEnvelope with error state.
    assert!(result["structuredContent"].is_object());
    assert_eq!(result["structuredContent"]["ok"], false);
    assert_eq!(result["structuredContent"]["action"], "recall");
    assert!(result["structuredContent"]["error"].is_object());
    assert!(result["structuredContent"]["error"]["code"].is_string());
    assert!(result["structuredContent"]["error"]["message"].is_string());

    // Verify error is propagated to top-level isError.
    assert_eq!(result["isError"], true);
}

#[test]
fn tools_call_read_missing_vault_returns_error_with_expected_shape() {
    // Test that calling read without a valid vault config returns an error
    // with the expected MCP response structure.
    let result = tools_call_result(Some(json!({
        "name": "talon",
        "arguments": { "action": "read", "path": "test.md" }
    })));

    // Verify the response has the standard MCP shape.
    assert!(result.is_object());
    assert!(result["content"].is_array());
    assert!(result["content"][0].is_object());
    assert_eq!(result["content"][0]["type"], "text");
    assert!(result["content"][0]["text"].is_string());

    // Verify the structured content is a TalonEnvelope with error state.
    assert!(result["structuredContent"].is_object());
    assert_eq!(result["structuredContent"]["ok"], false);
    assert_eq!(result["structuredContent"]["action"], "read");
    assert!(result["structuredContent"]["error"].is_object());
    assert!(result["structuredContent"]["error"]["code"].is_string());
    assert!(result["structuredContent"]["error"]["message"].is_string());

    // Verify error is propagated to top-level isError.
    assert_eq!(result["isError"], true);
}
