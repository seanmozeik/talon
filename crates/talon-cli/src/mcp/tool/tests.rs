use serde_json::{Value, json};

use super::{tools_call_result, tools_list_result};

#[test]
fn tools_list_returns_single_talon_tool_with_expected_actions() {
    let result = tools_list_result();
    let Some(tools) = result["tools"].as_array() else {
        panic!("tools array missing");
    };
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "talon");
    let Some(actions) = tools[0]["inputSchema"]["properties"]["action"]["enum"].as_array() else {
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
