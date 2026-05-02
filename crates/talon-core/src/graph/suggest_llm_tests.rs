use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::suggest::PROVENANCE_LLM;
use super::suggest_llm::{
    ASK_SUGGESTION_TIMEOUT, GraphSuggestionClient, LlmCandidate, build_llm_link_suggestions,
    validate_llm_candidates,
};
use super::{GraphNode, GraphSnapshot};
use crate::llm::ChatClient;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("build runtime: {err}"))
}

#[test]
fn llm_validation_rejects_unknown_targets_and_missing_terms() {
    let snapshot = snapshot_with_target();
    let candidates = vec![
        LlmCandidate {
            path: "Source.md".into(),
            target: "Target.md".into(),
            term: "Known Term".into(),
            line: 1,
        },
        LlmCandidate {
            path: "Source.md".into(),
            target: "Missing.md".into(),
            term: "Known Term".into(),
            line: 1,
        },
    ];
    let bodies = vec![("Source.md".into(), "Known Term appears here.".into())];

    let suggestions = validate_llm_candidates(candidates, &snapshot, &bodies);

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].provenance, PROVENANCE_LLM);
}

#[test]
fn llm_suggestions_are_validated_after_chat_response() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = runtime();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": json!([
                            {
                                "path": "Source.md",
                                "target": "Target.md",
                                "term": "Known Term",
                                "line": 1
                            },
                            {
                                "path": "Source.md",
                                "target": "Missing.md",
                                "term": "Known Term",
                                "line": 1
                            }
                        ]).to_string()
                    }
                }]
            })))
            .mount(&server),
    );
    let mut conn = rusqlite::Connection::open_in_memory()?;
    crate::indexing::migrations::run_migrations(&mut conn)?;
    insert_note(&conn)?;
    let chat = ChatClient::with_timeout_and_max_tokens(
        server.uri(),
        "ask-model",
        ASK_SUGGESTION_TIMEOUT,
        Some(64),
    )?;
    let client = GraphSuggestionClient::new(chat);

    let suggestions = build_llm_link_suggestions(&conn, &snapshot_with_target(), &client);

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].target, "Target.md");
    assert_eq!(suggestions[0].provenance, PROVENANCE_LLM);
    Ok(())
}

fn insert_note(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO notes (
           vault_path, title, tags, aliases, content, frontmatter,
           mtime_ms, size_bytes, hash, docid, active, scope
         ) VALUES ('Source.md', 'Source', '[]', '[]', 'Known Term appears here.', '',
           0, 0, 'Source.md', 'Source.md', 1, '')",
        [],
    )?;
    Ok(())
}

fn snapshot_with_target() -> GraphSnapshot {
    let mut snapshot = GraphSnapshot::default();
    snapshot.nodes.insert(
        "Target.md".into(),
        GraphNode {
            vault_path: "Target.md".into(),
            title: "Target".into(),
            aliases: vec!["Known Term".into()],
            tags: Vec::new(),
            scope: String::new(),
            note_type: None,
            sources: Vec::new(),
            outgoing_degree: 0,
            backlink_degree: 0,
            total_degree: 0,
            structural: false,
            community_id: None,
            community_cohesion: 0.0,
            community_neighbor_count: 0,
            bridge_weight: 0.0,
        },
    );
    snapshot
}
