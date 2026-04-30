#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fs_err as fs;

use super::*;
use crate::links::NoteReference;
use crate::text::frontmatter::FrontmatterValue;

#[test]
fn hash_is_stable_and_deterministic() {
    let h1 = hash_file_content("hello");
    let h2 = hash_file_content("hello");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
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
    assert!(matches_ignore_patterns("zone/Templates/Daily.md", &[]));
}

#[test]
fn ignore_matches_extra_patterns() {
    let extra = vec!["drafts/**".to_string()];
    assert!(matches_ignore_patterns("zone/drafts/wip.md", &extra));
    assert!(matches_ignore_patterns("zone/Drafts/wip.md", &extra));
    assert!(!matches_ignore_patterns("zone/notes/wip.md", &extra));
}

#[test]
fn include_requires_md_extension() {
    let patterns = vec!["**/*.md".to_string()];
    assert!(matches_include_patterns("a/b.md", &patterns));
    assert!(matches_include_patterns("a/b.MD", &patterns));
    assert!(!matches_include_patterns("a/b.txt", &patterns));
}

#[test]
fn include_matches_nested_patterns() {
    let patterns = vec!["zone/**".to_string()];
    assert!(matches_include_patterns("zone/note.md", &patterns));
    assert!(matches_include_patterns("a/Zone/note.md", &patterns));
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
    fs::write(root.join("zone").join("b.MD"), "b").unwrap();
    fs::write(root.join("zone").join("c.txt"), "c").unwrap();

    let mut paths: Vec<String> = scan_vault_markdown(&root).collect();
    paths.sort();
    assert_eq!(paths, vec!["a.md", "zone/b.MD"]);

    fs::remove_dir_all(&root).unwrap();
}
