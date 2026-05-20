use std::path::Path;
use std::time::UNIX_EPOCH;

use fs_err as fs;
use rusqlite::Connection;

pub(super) fn file_mtime_ms(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_millis()).ok()
}

pub(super) fn existing_metadata_is_up_to_date(
    conn: &Connection,
    vault_path: &str,
    mtime_ms: i64,
    size_bytes: i64,
) -> bool {
    let row: Option<(i64, i64, i64)> = conn
        .query_row(
            "SELECT mtime_ms, size_bytes, active FROM notes WHERE vault_path = ?",
            [vault_path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();
    matches!(row, Some((stored_mtime, stored_size, 1)) if stored_mtime == mtime_ms && stored_size == size_bytes)
}
