use talon_core::{
    ResponseMeta, SearchMode, SearchResponse, SyncResponse, SyncStatus, TalonEnvelope,
    TalonResponseData,
};

#[test]
fn to_agent_value_returns_some_for_search() {
    let envelope = TalonEnvelope {
        action: "search".to_string(),
        version: "0.1.0".to_string(),
        ok: true,
        data: Some(TalonResponseData::Search(SearchResponse {
            vault: None,
            query: None,
            mode: SearchMode::Hybrid,
            fast: false,
            expanded: false,
            expanded_queries: vec![],
            reranked: false,
            index_version: "1".to_string(),
            total: 0,
            results: vec![],
            diagnostics: None,
        })),
        error: None,
        meta: Some(ResponseMeta {
            duration_ms: 10,
            result_count: Some(0),
            warnings: vec![],
            scope_set: None,
            since: None,
        }),
    };

    let value = super::to_agent_value(&envelope);
    assert!(value.is_some_and(|v| v.is_object()));
}

#[test]
fn to_agent_value_returns_none_for_sync() {
    let envelope = TalonEnvelope {
        action: "sync".to_string(),
        version: "0.1.0".to_string(),
        ok: true,
        data: Some(TalonResponseData::Sync(SyncResponse {
            completed: true,
            status: SyncStatus::Ok,
            fast: false,
            force: false,
            path_count: 0,
            indexed: 0,
            skipped: 0,
            deleted: 0,
            embedded: 0,
            embed_failed: 0,
            dimension_mismatch: false,
            embed_remediation: None,
            embed_diagnostics: vec![],
            duration_ms: 100,
        })),
        error: None,
        meta: Some(ResponseMeta {
            duration_ms: 100,
            result_count: None,
            warnings: vec![],
            scope_set: None,
            since: None,
        }),
    };

    let value = super::to_agent_value(&envelope);
    assert!(value.is_none());
}

#[test]
fn to_agent_value_returns_none_for_empty_envelope() {
    let envelope = TalonEnvelope {
        action: "test".to_string(),
        version: "0.1.0".to_string(),
        ok: false,
        data: None,
        error: None,
        meta: None,
    };

    let value = super::to_agent_value(&envelope);
    assert!(value.is_none());
}
