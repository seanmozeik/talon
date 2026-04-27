//! `SQLite` schema migrations for the Talon index.
//!
//! Ported verbatim from the TypeScript reference (`indexer/migrations.ts`).
//! The schema is split into three groups, applied in order:
//!
//! 1. [`SCHEMA_MIGRATIONS`] — `CREATE TABLE` and `CREATE INDEX` statements.
//! 2. [`TRIGGER_MIGRATIONS`] — FTS5 sync triggers and the `db_version` setting.
//!    These run inside the same transaction as the schema migrations.
//! 3. [`REBUILD_MIGRATIONS`] — FTS5 `'rebuild'` commands. These must run
//!    *outside* a transaction; FTS5 rejects them otherwise.
//!
//! `vec_chunks` is intentionally absent — it is created lazily by the
//! embedding pipeline once the embedding dimensionality is known.

use rusqlite::Connection;

use crate::TalonError;

/// Settings key under which the schema version is stored.
pub const DB_VERSION_KEY: &str = "db_version";

/// Default `busy_timeout` PRAGMA value, in milliseconds.
pub const TALON_SQLITE_BUSY_TIMEOUT_MS: u32 = 10_000;

/// Schema-defining DDL: tables and indexes.
pub const SCHEMA_MIGRATIONS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS notes (
       id          INTEGER PRIMARY KEY,
       vault_path  TEXT UNIQUE NOT NULL,
       title       TEXT,
       tags        TEXT,
       aliases     TEXT,
       content     TEXT NOT NULL,
       frontmatter TEXT NOT NULL DEFAULT '',
       mtime_ms    INTEGER NOT NULL,
       size_bytes  INTEGER NOT NULL,
       hash        TEXT NOT NULL,
       docid       TEXT NOT NULL,
       active      INTEGER NOT NULL DEFAULT 1
     )",
    "CREATE TABLE IF NOT EXISTS chunks (
       id               INTEGER PRIMARY KEY,
       note_id          INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
       chunk_index      INTEGER NOT NULL,
       text             TEXT NOT NULL,
       embedding_text   TEXT NOT NULL,
       heading_path     TEXT,
       char_start       INTEGER,
       char_end          INTEGER,
       line_start       INTEGER,
       line_end         INTEGER,
       chunk_hash       TEXT NOT NULL,
       token_estimate   INTEGER NOT NULL,
       embedding_status TEXT NOT NULL DEFAULT 'pending',
       UNIQUE(note_id, chunk_index)
     )",
    "CREATE TABLE IF NOT EXISTS links (
       from_path  TEXT NOT NULL,
       to_path    TEXT NOT NULL,
       raw_target TEXT,
       heading    TEXT,
       alias      TEXT,
       PRIMARY KEY (from_path, to_path, raw_target)
     )",
    "CREATE TABLE IF NOT EXISTS note_aliases (
       note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
       alias      TEXT NOT NULL,
       alias_norm TEXT NOT NULL,
       PRIMARY KEY (note_id, alias)
     )",
    "CREATE TABLE IF NOT EXISTS note_tags (
       note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
       tag      TEXT NOT NULL,
       tag_norm TEXT NOT NULL,
       PRIMARY KEY (note_id, tag)
     )",
    "CREATE TABLE IF NOT EXISTS note_frontmatter_fields (
       note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
       field      TEXT NOT NULL,
       value      TEXT NOT NULL,
       value_norm TEXT NOT NULL,
       PRIMARY KEY (note_id, field, value)
     )",
    "CREATE TABLE IF NOT EXISTS settings (
       key   TEXT PRIMARY KEY,
       value TEXT NOT NULL
     )",
    "CREATE TABLE IF NOT EXISTS event_log (
       id        INTEGER PRIMARY KEY,
       action    TEXT NOT NULL,
       path      TEXT NOT NULL,
       timestamp TEXT NOT NULL
     )",
    "CREATE TABLE IF NOT EXISTS llm_cache (
       key           TEXT PRIMARY KEY,
       value         TEXT NOT NULL,
       expires_at_ms INTEGER NOT NULL
     )",
    "CREATE TABLE IF NOT EXISTS vector_metadata (
       chunk_id       INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
       model          TEXT NOT NULL,
       dimensions     INTEGER NOT NULL,
       embedded_at_ms INTEGER NOT NULL
     )",
    "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts_bm25 USING fts5(
       title, aliases, content,
       content='notes', content_rowid='id',
       tokenize = \"unicode61 tokenchars '+#'\"
     )",
    "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts_fuzzy USING fts5(
       title, aliases,
       content='notes', content_rowid='id',
       tokenize = 'trigram'
     )",
    "CREATE INDEX IF NOT EXISTS idx_links_to ON links(to_path)",
    "CREATE INDEX IF NOT EXISTS idx_chunks_note_chunk_index ON chunks(note_id, chunk_index)",
    "CREATE INDEX IF NOT EXISTS idx_note_aliases_alias_norm ON note_aliases(alias_norm)",
    "CREATE INDEX IF NOT EXISTS idx_note_tags_tag_norm ON note_tags(tag_norm)",
    "CREATE INDEX IF NOT EXISTS idx_fm_field_value_norm ON note_frontmatter_fields(field, value_norm)",
    "CREATE INDEX IF NOT EXISTS idx_notes_active_path ON notes(active, vault_path)",
    "CREATE INDEX IF NOT EXISTS idx_notes_hash ON notes(hash)",
    "CREATE INDEX IF NOT EXISTS idx_notes_docid ON notes(docid)",
    "CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(chunk_hash)",
];

/// FTS5 sync triggers and the seeded `db_version` setting.
///
/// Triggers are dropped first so re-running migrations after a trigger body
/// change replaces the old definition rather than failing.
pub const TRIGGER_MIGRATIONS: &[&str] = &[
    "DROP TRIGGER IF EXISTS notes_fts_ai",
    "DROP TRIGGER IF EXISTS notes_fts_au",
    "DROP TRIGGER IF EXISTS notes_fts_ad",
    "CREATE TRIGGER notes_fts_ai AFTER INSERT ON notes
     WHEN NEW.active = 1
     BEGIN
       INSERT INTO notes_fts_bm25(rowid, title, aliases, content)
       VALUES (NEW.id, NEW.title, NEW.aliases, NEW.content);
       INSERT INTO notes_fts_fuzzy(rowid, title, aliases)
       VALUES (NEW.id, NEW.title, NEW.aliases);
     END",
    "CREATE TRIGGER notes_fts_au AFTER UPDATE OF title, aliases, content, active ON notes
     BEGIN
       INSERT INTO notes_fts_bm25(notes_fts_bm25, rowid, title, aliases, content)
       VALUES ('delete', OLD.id, OLD.title, OLD.aliases, OLD.content);
       INSERT INTO notes_fts_fuzzy(notes_fts_fuzzy, rowid, title, aliases)
       VALUES ('delete', OLD.id, OLD.title, OLD.aliases);
       INSERT INTO notes_fts_bm25(rowid, title, aliases, content)
       SELECT NEW.id, NEW.title, NEW.aliases, NEW.content
       WHERE NEW.active = 1;
       INSERT INTO notes_fts_fuzzy(rowid, title, aliases)
       SELECT NEW.id, NEW.title, NEW.aliases
       WHERE NEW.active = 1;
     END",
    "CREATE TRIGGER notes_fts_ad AFTER DELETE ON notes
     BEGIN
       INSERT INTO notes_fts_bm25(notes_fts_bm25, rowid, title, aliases, content)
       VALUES ('delete', OLD.id, OLD.title, OLD.aliases, OLD.content);
       INSERT INTO notes_fts_fuzzy(notes_fts_fuzzy, rowid, title, aliases)
       VALUES ('delete', OLD.id, OLD.title, OLD.aliases);
     END",
    "INSERT OR IGNORE INTO settings(key, value) VALUES ('db_version', '0')",
];

/// FTS5 rebuild commands. Must run **outside** a transaction.
pub const REBUILD_MIGRATIONS: &[&str] = &[
    "INSERT INTO notes_fts_bm25(notes_fts_bm25) VALUES('rebuild')",
    "INSERT INTO notes_fts_fuzzy(notes_fts_fuzzy) VALUES('rebuild')",
];

/// Runs the full migration sequence on `conn`.
///
/// Sets the `WAL`, `busy_timeout`, and `foreign_keys` PRAGMAs, then applies
/// schema and trigger migrations inside a single transaction, then runs the
/// FTS5 rebuild statements outside that transaction.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] if any statement fails.
pub fn run_migrations(conn: &mut Connection) -> Result<(), TalonError> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|source| TalonError::Sqlite {
            context: "set journal_mode",
            source,
        })?;
    conn.pragma_update(None, "busy_timeout", TALON_SQLITE_BUSY_TIMEOUT_MS)
        .map_err(|source| TalonError::Sqlite {
            context: "set busy_timeout",
            source,
        })?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|source| TalonError::Sqlite {
            context: "set foreign_keys",
            source,
        })?;

    let tx = conn.transaction().map_err(|source| TalonError::Sqlite {
        context: "begin migration transaction",
        source,
    })?;
    run_statements(&tx, SCHEMA_MIGRATIONS, "schema migration")?;
    run_statements(&tx, TRIGGER_MIGRATIONS, "trigger migration")?;
    tx.commit().map_err(|source| TalonError::Sqlite {
        context: "commit migrations",
        source,
    })?;

    for statement in REBUILD_MIGRATIONS {
        conn.execute_batch(statement)
            .map_err(|source| TalonError::Sqlite {
                context: "fts rebuild",
                source,
            })?;
    }
    Ok(())
}

fn run_statements(
    conn: &Connection,
    statements: &[&str],
    context: &'static str,
) -> Result<(), TalonError> {
    for statement in statements {
        conn.execute_batch(statement)
            .map_err(|source| TalonError::Sqlite { context, source })?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
