use super::*;

#[test]
fn distance_to_score_zero_distance_is_one() {
    assert_eq!(distance_to_score(0.0), 1.0);
}

#[test]
fn distance_to_score_max_distance_is_zero() {
    assert_eq!(distance_to_score(COSINE_DISTANCE_MAX), 0.0);
}

#[test]
fn distance_to_score_above_max_is_clamped_to_zero() {
    assert_eq!(distance_to_score(COSINE_DISTANCE_MAX + 1.0), 0.0);
}

#[test]
fn distance_to_score_midpoint_is_half() {
    assert_eq!(distance_to_score(COSINE_DISTANCE_MAX / 2.0), 0.5);
}

#[test]
fn search_vector_empty_embedding_returns_empty() {
    // No DB needed; the empty-input guard short-circuits.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert!(search_vector(&conn, &[], 10, &PreFilter::none()).is_empty());
}

#[test]
fn search_vector_zero_norm_embedding_returns_empty() {
    // No DB needed; the zero-norm guard short-circuits.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert!(search_vector(&conn, &[0.0, 0.0, 0.0], 10, &PreFilter::none()).is_empty());
}

#[test]
fn search_vector_without_extension_returns_empty() {
    // Without sqlite-vec loaded, the prepare will fail. The function should
    // swallow that and return an empty result rather than panicking.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert!(search_vector(&conn, &[0.1, 0.2, 0.3], 10, &PreFilter::none()).is_empty());
}

#[test]
fn search_vector_per_note_dedup_returns_one_result_per_note() {
    let path = unique_path("dedup");
    let conn = setup_vec_db(&path, 2);

    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES ('note.md', 'Note', '[]', '[]', '', 0, 0, 'h', 'd', 1)",
        [],
    )
    .unwrap();
    let note_id = conn.last_insert_rowid();

    let mut chunk_ids = Vec::new();
    for i in 0_i64..5 {
        conn.execute(
            "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
             VALUES (?, ?, 'body', 'body', '', 0, 4, ?, 1, 'pending')",
            rusqlite::params![note_id, i, format!("h{i}")],
        )
        .unwrap();
        chunk_ids.push(conn.last_insert_rowid());
    }
    for &cid in &chunk_ids {
        crate::embed::persist_chunk_vector(&conn, cid, "m", 2, 1, &[1.0, 0.0]).unwrap();
    }

    let results = search_vector(&conn, &[1.0, 0.0], 10, &PreFilter::none());
    assert_eq!(
        results.len(),
        1,
        "5 chunks from the same note should collapse to 1 result, got {}",
        results.len()
    );
    assert_eq!(results[0].path, "note.md");

    drop(conn);
    cleanup(&path);
}

#[test]
fn search_vector_normalizes_query_embedding_before_search() {
    let path = unique_path("query-norm");
    let conn = setup_vec_db(&path, 3);
    let chunk_id = seed_note_chunk(&conn, "unit.md", "Unit");
    crate::embed::persist_chunk_vector(&conn, chunk_id, "m", 3, 1, &[3.0, 4.0, 0.0]).unwrap();

    let results = search_vector(&conn, &[6.0, 8.0, 0.0], 10, &PreFilter::none());

    assert!(
        (results[0].score - 1.0).abs() < 1e-6,
        "scaled query should score as a perfect match, got {}",
        results[0].score
    );

    drop(conn);
    cleanup(&path);
}

#[test]
fn search_vector_candidate_pool_does_not_exceed_limit() {
    let path = unique_path("cap");
    let conn = setup_vec_db(&path, 2);

    for i in 0_i64..10 {
        conn.execute(
            "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', '', 0, 0, ?, ?, 1)",
            rusqlite::params![
                format!("n{i}.md"),
                format!("N{i}"),
                format!("h{i}"),
                format!("d{i}")
            ],
        )
        .unwrap();
        let nid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
             VALUES (?, 0, 'body', 'body', '', 0, 4, ?, 1, 'pending')",
            rusqlite::params![nid, format!("hh{i}")],
        )
        .unwrap();
        let cid = conn.last_insert_rowid();
        let v = if i % 2 == 0 {
            [1.0_f32, 0.0]
        } else {
            [0.0_f32, 1.0]
        };
        crate::embed::persist_chunk_vector(&conn, cid, "m", 2, 1, &v).unwrap();
    }

    let results = search_vector(&conn, &[1.0, 0.0], 2, &PreFilter::none());
    assert!(
        results.len() <= 2,
        "result count should not exceed limit=2, got {}",
        results.len()
    );

    drop(conn);
    cleanup(&path);
}

fn setup_vec_db(path: &std::path::Path, dimensions: u32) -> rusqlite::Connection {
    crate::vec_ext::register_sqlite_vec().unwrap();
    let conn = crate::store::open_database(path).unwrap();
    crate::vec_ext::ensure_vec_chunks(&conn, dimensions).unwrap();
    conn
}

fn seed_note_chunk(conn: &rusqlite::Connection, path: &str, title: &str) -> i64 {
    conn.execute(
        "INSERT INTO notes (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
         VALUES (?, ?, '[]', '[]', '', 0, 0, ?, ?, 1)",
        rusqlite::params![path, title, format!("h-{path}"), format!("d-{path}")],
    )
    .unwrap();
    let note_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO chunks (note_id, chunk_index, text, embedding_text, heading_path, char_start, char_end, chunk_hash, token_estimate, embedding_status)
         VALUES (?, 0, 'body', 'body', '', 0, 4, ?, 1, 'pending')",
        rusqlite::params![note_id, format!("hh-{path}")],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn unique_path(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "talon-vec-{label}-{}-{n}.sqlite",
        std::process::id()
    ))
}

fn cleanup(path: &std::path::Path) {
    let _ = fs_err::remove_file(path);
    let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
}
