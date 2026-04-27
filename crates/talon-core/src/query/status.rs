//! Real status handler — replaces the CLI scaffold.

use rusqlite::Connection;

use crate::config::TalonConfig;
use crate::contracts::ContainerPath;
use crate::indexing::{IndexStats, ScopeReport, StatusResponse, StatusState};
use crate::vec_ext::get_vec_chunks_dimensions;

/// Returns real index statistics for the connected vault.
///
/// Never panics: falls back to `/` if the vault path is empty.
pub fn query_status(conn: &Connection, config: &TalonConfig) -> StatusResponse {
    let active_notes = count_rows(conn, "SELECT COUNT(*) FROM notes WHERE active=1");
    let chunk_count = count_rows(conn, "SELECT COUNT(*) FROM chunks");
    let failed_embeddings = count_rows(
        conn,
        "SELECT COUNT(*) FROM chunks WHERE embedding_status='failed'",
    );
    let vector_dimensions = get_vec_chunks_dimensions(conn).and_then(|d| u16::try_from(d).ok());

    let vault_str = config.vault_path.to_string_lossy();
    let container_mount =
        ContainerPath::parse(vault_str.as_ref()).unwrap_or_else(|_| ContainerPath::root());

    let index_version = read_db_version(conn);
    let scopes = build_scope_report(conn, config);

    StatusResponse {
        state: StatusState::Ready,
        enabled: true,
        reason: None,
        container_mount,
        index_version,
        index: IndexStats {
            active_notes,
            chunk_count,
            failed_embeddings,
            vector_dimensions,
        },
        scopes: Some(scopes),
    }
}

fn count_rows(conn: &Connection, sql: &str) -> u32 {
    conn.query_row(sql, [], |r| r.get::<_, i64>(0))
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

fn read_db_version(conn: &Connection) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'db_version'",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "unknown".to_string())
}

fn build_scope_report(conn: &Connection, config: &TalonConfig) -> ScopeReport {
    let total_scopes = u32::try_from(config.scopes.len()).unwrap_or(u32::MAX);
    let default_scopes = config.default_scope_names().into_iter().cloned().collect();
    let unscoped_count = if config.scopes.is_empty() {
        0
    } else {
        count_unscoped(conn, config)
    };
    ScopeReport {
        total_scopes,
        default_scopes,
        unscoped_count,
    }
}

fn count_unscoped(conn: &Connection, config: &TalonConfig) -> u32 {
    let paths: Vec<String> = conn
        .prepare("SELECT vault_path FROM notes WHERE active=1")
        .map(|mut stmt| {
            stmt.query_map([], |r| r.get(0))
                .map(|rows| rows.flatten().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let count = paths.iter().filter(|p| is_unscoped(p, config)).count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn is_unscoped(path: &str, config: &TalonConfig) -> bool {
    config.scopes.values().all(|scope| {
        !scope
            .glob
            .patterns()
            .iter()
            .any(|g| glob::Pattern::new(g).is_ok_and(|pat| pat.matches(path)))
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::config::{ExpansionConfig, InferenceConfig, InferenceModels, ScopesConfig};
    use crate::indexing::migrations::run_migrations;
    use rusqlite::Connection;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn
    }

    fn minimal_config(vault_path: &str) -> TalonConfig {
        TalonConfig {
            vault_path: PathBuf::from(vault_path),
            db_path: PathBuf::from(":memory:"),
            include_patterns: vec![],
            ignore_patterns: vec![],
            inference: InferenceConfig {
                base_url: "http://localhost:11434".to_string(),
                models: InferenceModels {
                    query_embedding: "nomic-embed-text".to_string(),
                    document_embedding: "nomic-embed-text".to_string(),
                    chunk_embedding: "nomic-embed-text".to_string(),
                    reranker: "ms-marco-MiniLM-L-6-v2".to_string(),
                },
            },
            expansion: ExpansionConfig {
                provider: "openai-compatible".to_string(),
                base_url: "http://localhost:11434".to_string(),
                model: "mistral".to_string(),
                max_tokens: None,
            },
            scopes: ScopesConfig::new(),
            chunker: crate::config::ChunkerConfig::default(),
        }
    }

    fn insert_note(conn: &Connection, path: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, frontmatter, mtime_ms, size_bytes, hash, docid, active) VALUES (?, ?, '', '', '', '{}', 0, 0, 'h', 'd', 1)",
            rusqlite::params![path, path],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_chunk(conn: &Connection, note_id: i64, chunk_index: i64, status: &str) {
        conn.execute(
            "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status) VALUES (?, ?, 'text', 'text', '', 0, 4, 'hash', 1, ?)",
            rusqlite::params![note_id, chunk_index, status],
        )
        .unwrap();
    }

    #[test]
    fn seeded_vault_returns_ready_with_correct_counts() {
        let conn = fresh_db();
        let nid = insert_note(&conn, "Atlas/Home.md");
        insert_chunk(&conn, nid, 0, "ok");
        insert_chunk(&conn, nid, 1, "failed");
        let nid2 = insert_note(&conn, "Graph/Node.md");
        insert_chunk(&conn, nid2, 0, "pending");

        let config = minimal_config("/vault/obsidian");
        let resp = query_status(&conn, &config);

        assert_eq!(resp.state, StatusState::Ready);
        assert!(resp.enabled);
        assert!(resp.reason.is_none());
        assert_eq!(resp.index.active_notes, 2);
        assert_eq!(resp.index.chunk_count, 3);
        assert_eq!(resp.index.failed_embeddings, 1);
    }

    #[test]
    fn empty_db_returns_zero_counts() {
        let conn = fresh_db();
        let config = minimal_config("/vault/obsidian");
        let resp = query_status(&conn, &config);

        assert_eq!(resp.state, StatusState::Ready);
        assert_eq!(resp.index.active_notes, 0);
        assert_eq!(resp.index.chunk_count, 0);
        assert_eq!(resp.index.failed_embeddings, 0);
        assert!(resp.index.vector_dimensions.is_none());
    }

    #[test]
    fn container_mount_uses_vault_path_from_config() {
        let conn = fresh_db();
        let config = minimal_config("/opt/vault");
        let resp = query_status(&conn, &config);
        assert_eq!(resp.container_mount.as_str(), "/opt/vault");
    }

    #[test]
    fn scope_report_counts_unscoped_notes() {
        use crate::config::{Scope, ScopeGlob, ScopePriority};

        let conn = fresh_db();
        insert_note(&conn, "Atlas/Home.md");
        insert_note(&conn, "Other/Misc.md");

        let mut scopes = BTreeMap::new();
        scopes.insert(
            "atlas".to_string(),
            Scope {
                glob: ScopeGlob::Single("Atlas/**".to_string()),
                priority: ScopePriority::Normal,
                default: true,
            },
        );

        let config = TalonConfig {
            vault_path: PathBuf::from("/vault"),
            db_path: PathBuf::from(":memory:"),
            include_patterns: vec![],
            ignore_patterns: vec![],
            inference: InferenceConfig {
                base_url: "http://localhost:11434".to_string(),
                models: InferenceModels {
                    query_embedding: "nomic-embed-text".to_string(),
                    document_embedding: "nomic-embed-text".to_string(),
                    chunk_embedding: "nomic-embed-text".to_string(),
                    reranker: "ms-marco-MiniLM-L-6-v2".to_string(),
                },
            },
            expansion: ExpansionConfig {
                provider: "openai-compatible".to_string(),
                base_url: "http://localhost:11434".to_string(),
                model: "mistral".to_string(),
                max_tokens: None,
            },
            scopes,
            chunker: crate::config::ChunkerConfig::default(),
        };

        let resp = query_status(&conn, &config);
        let scope_report = resp.scopes.unwrap();
        assert_eq!(scope_report.total_scopes, 1);
        assert_eq!(scope_report.default_scopes, vec!["atlas"]);
        assert_eq!(scope_report.unscoped_count, 1); // Other/Misc.md is unscoped
    }

    #[test]
    fn no_config_error_response_has_correct_shape() {
        // Verify that a ConfigError StatusResponse can be constructed correctly
        // (used by the CLI handler when config/DB loading fails)
        let resp = StatusResponse {
            state: StatusState::ConfigError,
            enabled: false,
            reason: Some("talon init not run".to_string()),
            container_mount: ContainerPath::root(),
            index_version: "unknown".to_string(),
            index: IndexStats {
                active_notes: 0,
                chunk_count: 0,
                failed_embeddings: 0,
                vector_dimensions: None,
            },
            scopes: None,
        };
        assert_eq!(resp.state, StatusState::ConfigError);
        assert!(!resp.enabled);
        assert!(resp.reason.is_some());
        assert_eq!(resp.index.active_notes, 0);
        assert!(resp.scopes.is_none());
    }
}
