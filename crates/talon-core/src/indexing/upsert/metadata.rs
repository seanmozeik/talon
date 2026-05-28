use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::TalonError;
use crate::links::ResolvedLink;
use crate::text::frontmatter::{FrontmatterValue, FrontmatterValueType, normalize_keyword};

/// Replaces the link rows originating from `vault_path`.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_links(
    conn: &Connection,
    vault_path: &str,
    links: &[ResolvedLink],
) -> Result<(), TalonError> {
    conn.execute("DELETE FROM links WHERE from_path = ?", [vault_path])
        .map_err(|source| TalonError::Sqlite {
            context: "delete old links",
            source,
        })?;
    for link in links {
        conn.execute(
            "INSERT OR IGNORE INTO links
               (from_path, to_path, raw_target, heading, alias)
             VALUES (?, ?, ?, ?, ?)",
            params![
                vault_path,
                link.to_path,
                link.raw_target,
                link.heading,
                link.alias,
            ],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert link",
            source,
        })?;
    }
    Ok(())
}

/// Replaces the alias rows for `note_id` with `aliases`. Each alias is
/// stored both verbatim and in normalized form for exact-match lookup.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_aliases(
    conn: &Connection,
    note_id: i64,
    aliases: &[String],
) -> Result<(), TalonError> {
    conn.execute("DELETE FROM note_aliases WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete old aliases",
            source,
        })?;
    for alias in aliases {
        let norm = normalize_keyword(alias);
        conn.execute(
            "INSERT INTO note_aliases (note_id, alias, alias_norm) VALUES (?, ?, ?)",
            params![note_id, alias, norm],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert alias",
            source,
        })?;
    }
    Ok(())
}

/// Replaces the tag rows for `note_id` with `tags`.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_tags(conn: &Connection, note_id: i64, tags: &[String]) -> Result<(), TalonError> {
    conn.execute("DELETE FROM note_tags WHERE note_id = ?", [note_id])
        .map_err(|source| TalonError::Sqlite {
            context: "delete old tags",
            source,
        })?;
    for tag in tags {
        let norm = normalize_keyword(tag);
        conn.execute(
            "INSERT INTO note_tags (note_id, tag, tag_norm) VALUES (?, ?, ?)",
            params![note_id, tag, norm],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert tag",
            source,
        })?;
    }
    Ok(())
}

fn flatten_frontmatter(
    value: &FrontmatterValue,
    prefix: &str,
    out: &mut Vec<(String, String, FrontmatterValueType)>,
) {
    let key = if prefix.is_empty() {
        String::from("value")
    } else {
        prefix.to_string()
    };
    match value {
        FrontmatterValue::String(s) => out.push((key, s.clone(), FrontmatterValueType::String)),
        FrontmatterValue::Date(s) => out.push((key, s.clone(), FrontmatterValueType::Date)),
        FrontmatterValue::Number(n) => out.push((key, n.to_string(), FrontmatterValueType::Number)),
        FrontmatterValue::Boolean(b) => out.push((key, b.to_string(), FrontmatterValueType::Bool)),
        FrontmatterValue::List(items) => {
            for item in items {
                out.push((key.clone(), item.clone(), FrontmatterValueType::List));
            }
        }
    }
}

/// Replaces the frontmatter-field rows for `note_id` with the flattened
/// representation of `frontmatter`.
///
/// Nested values get dotted keys (`outer.inner`); list values produce one
/// row per item under the same key.
///
/// # Errors
///
/// Returns [`TalonError::Sqlite`] on failure.
pub fn upsert_frontmatter_fields(
    conn: &Connection,
    note_id: i64,
    frontmatter: &BTreeMap<String, FrontmatterValue>,
) -> Result<(), TalonError> {
    conn.execute(
        "DELETE FROM note_frontmatter_fields WHERE note_id = ?",
        [note_id],
    )
    .map_err(|source| TalonError::Sqlite {
        context: "delete old frontmatter fields",
        source,
    })?;
    let mut flat: Vec<(String, String, FrontmatterValueType)> = Vec::new();
    for (key, value) in frontmatter {
        flatten_frontmatter(value, key, &mut flat);
    }
    for (field, value, value_type) in flat {
        let norm = normalize_keyword(&value);
        conn.execute(
            "INSERT OR IGNORE INTO note_frontmatter_fields (note_id, field, value, value_type, value_norm)
             VALUES (?, ?, ?, ?, ?)",
            params![note_id, field, value, value_type.as_db_str(), norm],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert frontmatter field",
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn duplicate_frontmatter_field_values_are_ignored() -> Result<(), TalonError> {
        let path = unique_db();
        let conn = open_database(&path)?;
        let mut frontmatter = BTreeMap::new();
        frontmatter.insert(
            "sources".into(),
            FrontmatterValue::List(vec!["same".into(), "same".into()]),
        );

        conn.execute(
            "INSERT INTO notes
             (id, vault_path, title, tags, aliases, content, mtime_ms, size_bytes, hash, docid, active)
             VALUES (1, 'a.md', 'A', '[]', '[]', 'x', 0, 0, 'h', 'd', 1)",
            [],
        )
        .map_err(|source| TalonError::Sqlite {
            context: "insert test note",
            source,
        })?;
        upsert_frontmatter_fields(&conn, 1, &frontmatter)?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_frontmatter_fields WHERE note_id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|source| TalonError::Sqlite {
                context: "count frontmatter fields",
                source,
            })?;

        assert_eq!(count, 1);
        cleanup(&path);
        Ok(())
    }

    fn unique_db() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "talon-frontmatter-upsert-{}-{}.sqlite",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = fs_err::remove_file(path);
        let _ = fs_err::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs_err::remove_file(path.with_extension("sqlite-shm"));
    }
}
