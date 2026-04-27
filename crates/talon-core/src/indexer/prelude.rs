//! Shared utilities used across the indexer pipeline.
//!
//! Ports `services/talon/indexer/prelude.ts`. All functions here are pure
//! (or filesystem-pure) — DB-touching helpers live in [`crate::indexing::upsert`].

use std::path::Path;

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

/// Returns `true` if `file_path` matches any of the default or extra ignore
/// patterns.
///
/// A pattern matches when it is contained as a substring in `file_path` or
/// when `file_path` ends with it. This intentionally mirrors the loose
/// substring matching used by the `TypeScript` reference.
#[must_use]
pub fn matches_ignore_patterns(file_path: &str, extra: &[String]) -> bool {
    for default in DEFAULT_IGNORE_PATHS {
        if file_path.contains(default) || file_path.ends_with(default) {
            return true;
        }
    }
    for pattern in extra {
        if file_path.contains(pattern) || file_path.ends_with(pattern.as_str()) {
            return true;
        }
    }
    false
}

/// Returns `true` if `file_path` is a markdown file matching any of the
/// include patterns. The wildcard pattern `**/*.md` matches any markdown
/// path; other patterns are treated as substring matches.
#[must_use]
pub fn matches_include_patterns(file_path: &str, patterns: &[String]) -> bool {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !ext.eq_ignore_ascii_case("md") {
        return false;
    }
    for pattern in patterns {
        if pattern == "**/*.md" || file_path.contains(pattern) {
            return true;
        }
    }
    false
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
    scan_vault_markdown(vault_root)
        .filter(|rel_path| {
            matches_include_patterns(rel_path, include_patterns)
                && !matches_ignore_patterns(rel_path, ignore_patterns)
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
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                return None;
            }
            let rel = path.strip_prefix(vault_root).ok()?;
            Some(rel.to_string_lossy().replace('\\', "/"))
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::text::frontmatter::FrontmatterValue;
    use fs_err as fs;
    use std::collections::BTreeMap;

    #[test]
    fn hash_is_stable_and_deterministic() {
        let h1 = hash_file_content("hello");
        let h2 = hash_file_content("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // 32-byte SHA-256 → 64 hex chars
    }

    #[test]
    fn hash_changes_with_input() {
        assert_ne!(hash_file_content("a"), hash_file_content("b"));
    }

    #[test]
    fn ignore_matches_default_obsidian_paths() {
        assert!(matches_ignore_patterns(".obsidian/config.json", &[]));
        assert!(matches_ignore_patterns("notes/.git/HEAD", &[]));
        assert!(matches_ignore_patterns("templates/Daily.md", &[]));
    }

    #[test]
    fn ignore_matches_extra_patterns() {
        let extra = vec!["drafts".to_string()];
        assert!(matches_ignore_patterns("zone/drafts/wip.md", &extra));
        assert!(!matches_ignore_patterns("zone/notes/wip.md", &extra));
    }

    #[test]
    fn include_requires_md_extension() {
        let patterns = vec!["**/*.md".to_string()];
        assert!(matches_include_patterns("a/b.md", &patterns));
        assert!(!matches_include_patterns("a/b.txt", &patterns));
    }

    #[test]
    fn include_matches_substring_patterns() {
        let patterns = vec!["zone/".to_string()];
        assert!(matches_include_patterns("zone/note.md", &patterns));
        assert!(!matches_include_patterns("other/note.md", &patterns));
    }

    #[test]
    fn extract_title_uses_frontmatter_title_when_present() {
        let mut fm: BTreeMap<String, FrontmatterValue> = BTreeMap::new();
        fm.insert("title".into(), FrontmatterValue::String("My Title".into()));
        assert_eq!(extract_title("zone/note.md", &fm), "My Title");
    }

    #[test]
    fn extract_title_falls_back_to_filename() {
        let fm: BTreeMap<String, FrontmatterValue> = BTreeMap::new();
        assert_eq!(extract_title("zone/My Note.md", &fm), "My Note");
        assert_eq!(extract_title("toplevel.md", &fm), "toplevel");
    }

    #[test]
    fn extract_title_ignores_blank_frontmatter_title() {
        let mut fm: BTreeMap<String, FrontmatterValue> = BTreeMap::new();
        fm.insert("title".into(), FrontmatterValue::String("   ".into()));
        assert_eq!(extract_title("a/b.md", &fm), "b");
    }

    #[test]
    fn merge_replaces_existing_path_entry() {
        let base = vec![
            NoteReference {
                vault_path: "a.md".into(),
                title: Some("old A".into()),
                aliases: vec![],
            },
            NoteReference {
                vault_path: "b.md".into(),
                title: Some("B".into()),
                aliases: vec![],
            },
        ];
        let merged = merge_current_path_for_linking(&base, "a.md", "new A", &["alias".into()]);
        assert_eq!(merged.len(), 2);
        let a = merged.iter().find(|n| n.vault_path == "a.md").unwrap();
        assert_eq!(a.title.as_deref(), Some("new A"));
        assert_eq!(a.aliases, vec!["alias"]);
    }

    fn unique_dir(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("talon-prelude-test-{label}-{pid}-{n}"))
    }

    #[test]
    fn scan_vault_yields_only_md_files_with_relative_paths() {
        let root = unique_dir("scan");
        fs::create_dir_all(root.join("zone")).unwrap();
        fs::write(root.join("a.md"), "a").unwrap();
        fs::write(root.join("zone").join("b.md"), "b").unwrap();
        fs::write(root.join("zone").join("c.txt"), "c").unwrap();

        let mut paths: Vec<String> = scan_vault_markdown(&root).collect();
        paths.sort();
        assert_eq!(paths, vec!["a.md", "zone/b.md"]);

        fs::remove_dir_all(&root).unwrap();
    }
}
