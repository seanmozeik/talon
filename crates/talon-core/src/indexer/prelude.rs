//! Shared utilities used across the indexer pipeline.
//!
//! Ports `services/talon/indexer/prelude.ts`. All functions here are pure
//! (or filesystem-pure) — DB-touching helpers live in [`crate::indexing::upsert`].

use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use fs_err as fs;

use crate::links::NoteReference;
use crate::text::frontmatter::{FrontmatterValue, parse_frontmatter};
use crate::text::normalize_vault_path;

/// Hard-coded ignore patterns matching the `TypeScript` reference. These are
/// always applied, in addition to whatever the caller passes through
/// [`matches_ignore_patterns`]'s `extra` argument.
pub const DEFAULT_IGNORE_PATHS: &[&str] = &[".obsidian", ".git", "templates", ".canvas"];

fn add_case_insensitive_glob(builder: &mut GlobSetBuilder, pattern: &str) -> Result<(), String> {
    let glob = GlobBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|err| err.to_string())?;
    builder.add(glob);
    Ok(())
}

fn add_pattern_variants(builder: &mut GlobSetBuilder, pattern: &str) -> Result<(), String> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    add_case_insensitive_glob(builder, trimmed)?;
    if !trimmed.starts_with("**/") && !trimmed.starts_with('/') {
        add_case_insensitive_glob(builder, &format!("**/{trimmed}"))?;
    }
    Ok(())
}

/// Returns the lowercase hex `SHA-256` of `content`.
///
/// Used for change detection on note bodies and for chunk dedup keys.
#[must_use]
pub fn hash_file_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Builds the case-insensitive ignore matcher used by scans and reconciliation.
///
/// # Errors
///
/// Returns a message from `globset` when any configured pattern is invalid.
pub fn build_ignore_globset(extra: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for default in DEFAULT_IGNORE_PATHS {
        add_case_insensitive_glob(&mut builder, &format!("{default}/**"))?;
        add_case_insensitive_glob(&mut builder, &format!("**/{default}/**"))?;
    }
    for pattern in extra {
        add_pattern_variants(&mut builder, pattern)?;
    }
    builder.build().map_err(|err| err.to_string())
}

/// Builds the case-insensitive include matcher used by scans and reconciliation.
///
/// # Errors
///
/// Returns a message from `globset` when any configured pattern is invalid.
pub fn build_include_globset(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        add_pattern_variants(&mut builder, pattern)?;
    }
    builder.build().map_err(|err| err.to_string())
}

/// Returns `true` when `path` matches the compiled ignore matcher.
#[must_use]
pub fn file_matches_ignore(path: &str, set: &GlobSet) -> bool {
    set.is_match(path)
}

/// Returns `true` when `path` is a markdown file matching the include matcher.
#[must_use]
pub fn file_matches_include(path: &str, set: &GlobSet) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    ext.eq_ignore_ascii_case("md") && set.is_match(path)
}

/// Returns `true` if `file_path` matches any of the default or extra ignore
/// patterns.
#[must_use]
pub fn matches_ignore_patterns(file_path: &str, extra: &[String]) -> bool {
    build_ignore_globset(extra).is_ok_and(|set| file_matches_ignore(file_path, &set))
}

/// Returns `true` if `file_path` is a markdown file matching any include
/// pattern.
#[must_use]
pub fn matches_include_patterns(file_path: &str, patterns: &[String]) -> bool {
    build_include_globset(patterns).is_ok_and(|set| file_matches_include(file_path, &set))
}

/// Loads vault notes for link resolution from the `notes` table.
///
/// Returns `(vault_path, title, aliases)` triples for every active note.
/// Used to seed the link-resolver cache at the start of a full scan.
///
/// # Errors
///
/// Returns the underlying `rusqlite::Error` if the query fails (the table
/// must exist — call [`crate::store::open_database`] first).
pub fn load_notes_for_linking(conn: &Connection) -> rusqlite::Result<Vec<NoteReference>> {
    let mut stmt =
        conn.prepare_cached("SELECT vault_path, title, aliases FROM notes WHERE active = 1")?;
    let rows = stmt.query_map([], |row| {
        let vault_path: String = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let aliases_json: Option<String> = row.get(2)?;
        let aliases: Vec<String> = aliases_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default();
        Ok(NoteReference {
            vault_path,
            title,
            aliases,
        })
    })?;
    rows.collect()
}

/// Loads DB notes and overlays current vault notes for full-scan link resolution.
///
/// # Errors
///
/// Returns the underlying `rusqlite::Error` if loading DB notes fails.
pub fn load_scan_notes_for_linking(
    conn: &Connection,
    vault_root: &Path,
    include_patterns: &[String],
    ignore_patterns: &[String],
) -> rusqlite::Result<Vec<NoteReference>> {
    let mut cache = load_notes_for_linking(conn)?;
    for note in load_vault_notes_for_linking(vault_root, include_patterns, ignore_patterns) {
        cache = merge_current_path_for_linking(
            &cache,
            &note.vault_path,
            note.title.as_deref().unwrap_or_default(),
            &note.aliases,
        );
    }
    Ok(cache)
}

/// Loads note references directly from the vault for order-independent link resolution.
///
/// A fresh scan cannot resolve links against notes that have not been written
/// to the DB yet. This preflight cache makes every included markdown file
/// visible before per-note indexing begins.
#[must_use]
pub fn load_vault_notes_for_linking(
    vault_root: &Path,
    include_patterns: &[String],
    ignore_patterns: &[String],
) -> Vec<NoteReference> {
    let Ok(include_set) = build_include_globset(include_patterns) else {
        return Vec::new();
    };
    let Ok(ignore_set) = build_ignore_globset(ignore_patterns) else {
        return Vec::new();
    };
    load_vault_notes_for_linking_with_sets(vault_root, &include_set, &ignore_set)
}

/// Loads note references directly from the vault using precompiled filters.
#[must_use]
pub fn load_vault_notes_for_linking_with_sets(
    vault_root: &Path,
    include_set: &GlobSet,
    ignore_set: &GlobSet,
) -> Vec<NoteReference> {
    scan_vault_markdown(vault_root)
        .filter(|rel_path| {
            file_matches_include(rel_path, include_set)
                && !file_matches_ignore(rel_path, ignore_set)
        })
        .filter_map(|rel_path| {
            let content = fs::read_to_string(vault_root.join(&rel_path)).ok()?;
            let vault_path = normalize_vault_path(&rel_path);
            let parsed = parse_frontmatter(&content);
            let title = extract_title(&vault_path, &parsed.frontmatter);
            Some(NoteReference {
                vault_path,
                title: Some(title),
                aliases: parsed.aliases,
            })
        })
        .collect()
}

/// Returns `base` with the entry for `path` replaced by a fresh
/// [`NoteReference`] built from the current note's metadata.
///
/// This keeps the in-memory link-resolution cache fresh across the per-note
/// indexing loop without re-querying the database after every write.
#[must_use]
pub fn merge_current_path_for_linking(
    base: &[NoteReference],
    path: &str,
    title: &str,
    aliases: &[String],
) -> Vec<NoteReference> {
    let mut out: Vec<NoteReference> = base
        .iter()
        .filter(|n| n.vault_path != path)
        .cloned()
        .collect();
    out.push(NoteReference {
        vault_path: path.to_string(),
        title: Some(title.to_string()),
        aliases: aliases.to_vec(),
    });
    out
}

/// Extracts a display title for a note.
///
/// Prefers a non-empty `title` field in the parsed frontmatter; otherwise
/// uses the file stem (the last path component without the `.md` suffix).
#[must_use]
pub fn extract_title(
    vault_path: &str,
    frontmatter: &std::collections::BTreeMap<String, FrontmatterValue>,
) -> String {
    if let Some(FrontmatterValue::String(title)) = frontmatter.get("title") {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let stripped = vault_path.strip_suffix(".md").unwrap_or(vault_path);
    stripped
        .rsplit('/')
        .next()
        .unwrap_or(vault_path)
        .to_string()
}

/// Iterator over vault-relative markdown paths under `vault_root`.
///
/// Walks the directory tree and yields paths whose extension is `.md`,
/// normalized to forward slashes. Errors during traversal are silently
/// skipped — callers surface them through DB errors during indexing rather
/// than via this generator.
pub fn scan_vault_markdown(vault_root: &Path) -> impl Iterator<Item = String> + '_ {
    WalkDir::new(vault_root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(move |entry| {
            let path = entry.path();
            if !path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            {
                return None;
            }
            let rel = path.strip_prefix(vault_root).ok()?;
            Some(rel.to_string_lossy().replace('\\', "/"))
        })
}

#[cfg(test)]
#[path = "prelude_tests.rs"]
mod tests;
