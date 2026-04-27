use serde_json::{Value, json};

pub(super) fn input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["search", "read", "sync", "status", "related", "meta", "changes", "lint", "recall"]
            },
            "query": { "type": ["string", "null"] },
            "queries": { "type": "array", "items": { "type": "string" } },
            "intent": { "type": "string" },
            "mode": { "type": "string", "enum": ["hybrid", "semantic", "fulltext", "title"] },
            "fast": { "type": "boolean" },
            "limit": { "type": "integer", "minimum": 1 },
            "candidate_limit": { "type": "integer", "minimum": 1 },
            "path": { "type": ["string", "null"] },
            "paths": { "type": "array", "items": { "type": "string" } },
            "raw": { "type": "boolean" },
            "fromLine": { "type": ["integer", "null"], "minimum": 1 },
            "maxLines": { "type": ["integer", "null"], "minimum": 1 },
            "force": { "type": "boolean" },
            "noWait": { "type": "boolean" },
            "depth": { "type": "integer", "minimum": 1, "maximum": 3 },
            "direction": { "type": "string", "enum": ["outgoing", "backlinks", "both"] },
            "scope": { "type": "array", "items": { "type": "string" } },
            "scopeOnly": { "type": "array", "items": { "type": "string" } },
            "where": { "type": "array", "items": { "$ref": "#/$defs/whereClause" } },
            "since": { "type": ["string", "null"] },
            "anchors": { "type": ["boolean", "null"], "description": "Include previewAnchors (BM25 + semantic) in each search result. Opt-in; adds one DB lookup per result." },
            "select": { "type": "array", "items": { "type": "string" } },
            "tagCounts": { "type": "boolean" },
            "sources": { "type": ["string", "null"] },
            "check": { "type": "string", "enum": ["all", "orphans", "broken-links", "dangling-refs", "unreferenced"] },
            "message": { "type": "string", "description": "User message for recall context" },
            "priorMessages": { "type": "array", "items": { "type": "string" }, "description": "Prior conversation turns fed to expansion" },
            "budgetTokens": { "type": "integer", "minimum": 1, "description": "Token budget for the recall payload (default 500)" },
            "exclude": { "type": "array", "items": { "type": "string" }, "description": "Vault paths to exclude from all retrieval" },
            "format": { "type": "string", "enum": ["json", "prompt-xml"], "description": "Output format" },
            "minConfidence": { "type": "number", "minimum": 0.0, "maximum": 1.0, "description": "Minimum evidence score threshold (default 0.4)" }
        },
        "$defs": {
            "whereClause": {
                "type": "object",
                "required": ["key", "op"],
                "properties": {
                    "key": { "type": "string" },
                    "op": {
                        "type": "string",
                        "enum": ["equals", "not-equals", "less-than", "less-than-or-equal", "greater-than", "greater-than-or-equal", "contains", "exists"]
                    },
                    "value": { "type": ["string", "null"] }
                }
            }
        }
    })
}
