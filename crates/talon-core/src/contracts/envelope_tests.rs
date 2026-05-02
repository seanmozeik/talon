//! Round-trip serialisation tests for `TalonEnvelope` and its variants.

use std::collections::BTreeMap;

use crate::contracts::{
    ContainerPath, ErrorEnvelope, ResponseMeta, TalonEnvelope, TalonResponseData, VaultPath,
};
use crate::error::ErrorCode;
use crate::indexing::{
    IndexStats, InspectCheck, InspectResponse, StatusResponse, StatusState, SyncResponse,
    SyncStatus,
};
use crate::query::{
    ChangesResponse, MetaResponse, ReadResponse, RecallResponse, RelatedResponse, VaultRecall,
};
use crate::search::Direction;
use crate::search::{SearchMode, SearchResponse};

fn success_meta() -> ResponseMeta {
    ResponseMeta {
        duration_ms: 42,
        result_count: Some(3),
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    }
}

fn error_envelope() -> ErrorEnvelope {
    ErrorEnvelope {
        code: ErrorCode::Internal,
        message: "something broke".to_string(),
        detail: None,
    }
}

// ── Success envelope ──────────────────────────────────────────────

#[test]
fn search_success_round_trip() {
    let data = TalonResponseData::Search(SearchResponse {
        vault: None,
        query: Some("hello world".to_string()),
        mode: SearchMode::Hybrid,
        fast: false,
        expanded: true,
        expanded_queries: vec!["hello docs".to_string()],
        reranked: true,
        index_version: "1".to_string(),
        total: 3,
        results: Vec::new(),
        diagnostics: None,
    });
    let envelope = TalonEnvelope::ok("search", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "search");
    assert!(round_trip.ok);
    assert!(round_trip.data.is_some());
    assert!(round_trip.meta.is_some());
    assert!(round_trip.error.is_none());
    // Verify top-level keys are exactly {action, version, ok, data, meta}
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut keys: Vec<String> = parsed.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["action", "data", "meta", "ok", "version"]);
}

#[test]
fn sync_success_round_trip() {
    let data = TalonResponseData::Sync(SyncResponse {
        completed: true,
        status: SyncStatus::Ok,
        fast: false,
        force: false,
        rebuild: false,
        path_count: 1,
        indexed: 5,
        skipped: 0,
        deleted: 0,
        embedded: 5,
        embed_failed: 0,
        dimension_mismatch: false,
        embed_remediation: None,
        embed_diagnostics: Vec::new(),
        graph: None,
        duration_ms: 100,
    });
    let envelope = TalonEnvelope::ok("sync", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "sync");
    assert!(round_trip.ok);
    assert!(round_trip.data.is_some());
}

#[test]
fn status_success_round_trip() {
    let data = TalonResponseData::Status(StatusResponse {
        state: StatusState::Ready,
        enabled: true,
        reason: None,
        container_mount: ContainerPath::parse("/vault").unwrap(),
        index_version: "1".to_string(),
        index: IndexStats {
            active_notes: 100,
            chunk_count: 500,
            failed_embeddings: 0,
            vector_dimensions: Some(384),
        },
        scopes: None,
        vault_path: None,
        config_path: None,
        db_path: None,
    });
    let envelope = TalonEnvelope::ok("status", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "status");
    assert!(round_trip.ok);
}

#[test]
fn read_success_round_trip() {
    let data = TalonResponseData::Read(ReadResponse::stub());
    let envelope = TalonEnvelope::ok("read", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "read");
    assert!(round_trip.ok);
}

#[test]
fn related_success_round_trip() {
    let data = TalonResponseData::Related(RelatedResponse {
        vault: None,
        path: VaultPath::parse("test.md").unwrap(),
        direction: Direction::Both,
        results: Vec::new(),
    });
    let envelope = TalonEnvelope::ok("related", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "related");
    assert!(round_trip.ok);
}

#[test]
fn meta_success_round_trip() {
    let data = TalonResponseData::Meta(MetaResponse {
        vault: None,
        entries: Vec::new(),
        tag_counts: Some(BTreeMap::new()),
    });
    let envelope = TalonEnvelope::ok("meta", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "meta");
    assert!(round_trip.ok);
}

#[test]
fn changes_success_round_trip() {
    let data = TalonResponseData::Changes(ChangesResponse {
        vault: None,
        added: Vec::new(),
        modified: Vec::new(),
        deleted: Vec::new(),
    });
    let envelope = TalonEnvelope::ok("changes", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "changes");
    assert!(round_trip.ok);
}

#[test]
fn lint_success_round_trip() {
    let data = TalonResponseData::Inspect(InspectResponse {
        vault: None,
        check: InspectCheck::Orphans,
        findings: Vec::new(),
    });
    let envelope = TalonEnvelope::ok("inspect", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "inspect");
    assert!(round_trip.ok);
}

#[test]
fn recall_success_round_trip() {
    let data = TalonResponseData::Recall(RecallResponse {
        vault: None,
        vault_recall: Some(VaultRecall {
            active_notes: Vec::new(),
            linked_context: Vec::new(),
        }),
        evidence_score: 0.75,
        tokens_used: 120,
        excluded: Vec::new(),
        excluded_by_budget: Vec::new(),
        skipped: false,
        diagnostics: None,
    });
    let envelope = TalonEnvelope::ok("recall", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "recall");
    assert!(round_trip.ok);
    assert!(round_trip.data.is_some());
}

#[test]
fn recall_skipped_round_trip() {
    let data = TalonResponseData::Recall(RecallResponse {
        vault: None,
        vault_recall: None,
        evidence_score: 0.05,
        tokens_used: 0,
        excluded: Vec::new(),
        excluded_by_budget: Vec::new(),
        skipped: true,
        diagnostics: None,
    });
    let envelope = TalonEnvelope::ok("recall", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "recall");
    assert!(round_trip.ok);
    if let Some(TalonResponseData::Recall(r)) = &round_trip.data {
        assert!(r.skipped);
        assert!(r.vault_recall.is_none());
    } else {
        panic!("expected Recall variant");
    }
}

// ── Error envelope ────────────────────────────────────────────────

#[test]
fn error_round_trip() {
    let envelope = TalonEnvelope::err("search", error_envelope());
    let json = serde_json::to_string(&envelope).unwrap();
    let round_trip: TalonEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.action, "search");
    assert!(!round_trip.ok);
    assert!(round_trip.data.is_none());
    assert!(round_trip.meta.is_none());
    assert!(round_trip.error.is_some());
    // Verify top-level keys are exactly {action, version, ok, error}
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut keys: Vec<String> = parsed.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["action", "error", "ok", "version"]);
}

// ── Top-level key assertions ──────────────────────────────────────

#[test]
fn success_envelope_has_exactly_five_keys() {
    let data = TalonResponseData::Search(SearchResponse::empty_input());
    let envelope = TalonEnvelope::ok("search", data, success_meta());
    let json = serde_json::to_string(&envelope).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.as_object().unwrap().keys().count(),
        5,
        "success envelope must have exactly 5 top-level keys"
    );
}

#[test]
fn error_envelope_has_exactly_four_keys() {
    let envelope = TalonEnvelope::err("search", error_envelope());
    let json = serde_json::to_string(&envelope).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.as_object().unwrap().keys().count(),
        4,
        "error envelope must have exactly 4 top-level keys"
    );
}

#[test]
fn version_is_cargo_pkg_version() {
    let data = TalonResponseData::Search(SearchResponse::empty_input());
    let envelope = TalonEnvelope::ok("search", data, success_meta());
    assert_eq!(envelope.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn action_is_kebab_case() {
    let data = TalonResponseData::Search(SearchResponse::empty_input());
    let envelope = TalonEnvelope::ok("search", data, success_meta());
    assert_eq!(envelope.action, "search");
    let data2 = TalonResponseData::Search(SearchResponse::empty_input());
    let envelope = TalonEnvelope::ok("my-action", data2, success_meta());
    assert_eq!(envelope.action, "my-action");
}

// ── ResponseMeta optional fields ──────────────────────────────────

#[test]
fn meta_skips_none_fields() {
    let meta = ResponseMeta {
        duration_ms: 10,
        result_count: None,
        warnings: Vec::new(),
        scope_set: None,
        since: None,
    };
    let data = TalonResponseData::Search(SearchResponse::empty_input());
    let envelope = TalonEnvelope::ok("search", data, meta);
    let json = serde_json::to_string(&envelope).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    eprintln!("JSON: {json}");
    let meta_obj = parsed.get("meta").unwrap().as_object().unwrap();
    assert!(!meta_obj.contains_key("resultCount"));
    assert!(!meta_obj.contains_key("scopeSet"));
    assert!(!meta_obj.contains_key("since"));
    assert!(meta_obj.contains_key("durationMs"));
    assert_eq!(meta_obj["durationMs"], 10);
}

#[test]
fn error_skips_none_detail() {
    let env = TalonEnvelope::err(
        "search",
        ErrorEnvelope {
            code: ErrorCode::Internal,
            message: "boom".to_string(),
            detail: None,
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let error_obj = parsed.get("error").unwrap().as_object().unwrap();
    assert!(!error_obj.contains_key("detail"));
}
