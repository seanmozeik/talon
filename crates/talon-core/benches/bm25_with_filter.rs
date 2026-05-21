//! Criterion benchmark: FTS+JOIN latency with and without `--where` post-filter.
//!
//! Investigates whether `SQLite`'s query planner stays on the FTS index path when
//! `notes_fts_bm25 MATCH ?` is combined with `JOIN notes WHERE n.active = 1`.
//! A CTE barrier (wrapping the FTS match in a WITH clause) forces the planner to
//! materialise FTS results first; this bench determines whether that rewrite is
//! necessary at ~1k vault scale.
//!
//! Two variants are timed:
//!   - `bm25_unfiltered` — raw `search_bm25`, no post-filter.
//!   - `bm25_with_status_filter` — `run_search` (fulltext, fast) with
//!     `--where status:active`, which runs the same BM25 SQL then evaluates
//!     one DB lookup per hit.
//!
//! Fixture: 1 000 notes, deterministic content seeded from (row, word) indices so
//! BM25 has real IDF work to do. ~30 % of notes get `status: active` in
//! `note_frontmatter_fields`, giving a realistic selectivity for the filter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::cast_precision_loss
)]

use criterion::{Criterion, criterion_group, criterion_main};
use rusqlite::{Connection, params};
use std::env::temp_dir;
use std::sync::atomic::{AtomicU64, Ordering};
use talon_core::{
    PositiveCount, SearchInput, SearchMode, WhereClause, WhereOperator, open_database, run_search,
    vec_ext::register_sqlite_vec,
};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Returns a unique temp-dir path for each bench run so concurrent runs don't
/// collide even when run from the same PID.
fn unique_db_path() -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    temp_dir().join(format!("talon-bench-bm25-{pid}-{n}.sqlite"))
}

/// Word pool used to build varied, deterministic note content.
const WORD_POOL: &[&str] = &[
    "protocol",
    "network",
    "latency",
    "cache",
    "index",
    "buffer",
    "queue",
    "stream",
    "pipeline",
    "scheduler",
    "mutex",
    "semaphore",
    "channel",
    "socket",
    "packet",
    "header",
    "payload",
    "checksum",
    "handshake",
    "timeout",
    "retry",
    "backoff",
    "circuit",
    "breaker",
    "replica",
    "shard",
    "cluster",
    "consensus",
    "quorum",
    "snapshot",
    "journal",
    "commit",
    "rollback",
    "transaction",
    "isolation",
    "lock",
    "contention",
    "throughput",
    "bandwidth",
    "congestion",
    "flow",
    "window",
    "segment",
    "frame",
    "route",
    "hop",
    "metric",
    "gauge",
    "counter",
    "histogram",
    "percentile",
];

/// Builds deterministic note content for note `i`.  Every note contains the
/// query term "protocol" at least once so BM25 has meaningful recall work; the
/// rest of the content varies by row so IDF scores differ across the corpus.
fn note_content(i: usize) -> String {
    let n = WORD_POOL.len();
    let w0 = WORD_POOL[i % n];
    let w1 = WORD_POOL[(i * 3 + 7) % n];
    let w2 = WORD_POOL[(i * 7 + 13) % n];
    format!(
        "This note covers {w0}, {w1}, and {w2}. \
         The protocol discussed here relates to {w0} behaviour under {w1} load. \
         Key insight number {i}: {w2} patterns often interact with protocol boundaries.",
    )
}

/// Seeds a fresh in-memory-like `SQLite` database with `n` notes.
///
/// * Notes 0..( n*3/10 ) receive `status: active` in `note_frontmatter_fields`.
/// * All other notes receive `status: archived`.
/// * Every note contains the word "protocol" so BM25 has enough candidates.
///
/// Returns an open `Connection` ready for benchmarking.  The caller owns the
/// path and is responsible for cleanup (or simply letting the OS reclaim a
/// temp file).
fn setup_fixture_vault(n: usize) -> Connection {
    register_sqlite_vec().expect("sqlite-vec must load");
    let path = unique_db_path();
    let conn = open_database(&path).expect("open_database");

    // Wrap all inserts in a single transaction for speed.
    conn.execute_batch("BEGIN").expect("BEGIN");

    let active_cutoff = n * 3 / 10; // ~30 % active

    for i in 0..n {
        let vault_path = format!("bench-note-{i:04}.md");
        let title = format!("Bench Note {i}");
        let content = note_content(i);
        let status = if i < active_cutoff {
            "active"
        } else {
            "archived"
        };

        // Insert into `notes` (FTS5 trigger populates `notes_fts_bm25` automatically).
        conn.execute(
            "INSERT INTO notes
             (vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (?, ?, '[]', '[]', ?, 0, 0, 'h', 'd', 1)",
            params![vault_path, title, content],
        )
        .expect("insert note");

        let note_id = conn.last_insert_rowid();

        // Insert frontmatter field so the --where status:active filter has data.
        let status_norm = status.to_lowercase();
        conn.execute(
            "INSERT INTO note_frontmatter_fields (note_id, field, value, value_norm)
             VALUES (?, 'status', ?, ?)",
            params![note_id, status, status_norm],
        )
        .expect("insert frontmatter field");
    }

    conn.execute_batch("COMMIT").expect("COMMIT");

    conn
}

// ---------------------------------------------------------------------------
// Benchmark functions
// ---------------------------------------------------------------------------

/// Unfiltered BM25 search — exercises the raw FTS+JOIN SQL path.
fn bench_bm25_unfiltered(c: &mut Criterion) {
    // Setup happens once, outside the timing loop.
    let conn = setup_fixture_vault(1000);

    c.bench_function("bm25_unfiltered", |b| {
        b.iter(|| {
            let input = SearchInput {
                query: Some("protocol".to_string()),
                mode: SearchMode::Fulltext,
                fast: true,
                limit: PositiveCount::new(50, "limit").unwrap(),
                candidate_limit: PositiveCount::new(200, "candidate_limit").unwrap(),
                ..SearchInput::default()
            };
            let resp = run_search(&conn, &input, None, None, None, None);
            // Prevent dead-code elimination — criterion doesn't black_box the
            // output of `iter`, so we touch the result length.
            assert!(!resp.results.is_empty(), "expected BM25 hits");
        });
    });
}

/// Filtered BM25 search — same SQL path plus per-hit `note_frontmatter_fields`
/// lookups for the `--where status:active` post-filter.
fn bench_bm25_with_status_filter(c: &mut Criterion) {
    let conn = setup_fixture_vault(1000);

    let where_clause = WhereClause {
        key: "status".to_string(),
        op: WhereOperator::Equals,
        value: Some("active".to_string()),
    };

    c.bench_function("bm25_with_status_filter", |b| {
        b.iter(|| {
            let input = SearchInput {
                query: Some("protocol".to_string()),
                mode: SearchMode::Fulltext,
                fast: true,
                limit: PositiveCount::new(50, "limit").unwrap(),
                candidate_limit: PositiveCount::new(200, "candidate_limit").unwrap(),
                where_: vec![where_clause.clone()],
                ..SearchInput::default()
            };
            let resp = run_search(&conn, &input, None, None, None, None);
            assert!(!resp.results.is_empty(), "expected filtered hits");
        });
    });
}

criterion_group!(
    benches,
    bench_bm25_unfiltered,
    bench_bm25_with_status_filter
);
criterion_main!(benches);
