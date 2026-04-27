use super::*;

#[test]
fn test_resolve_wiki_link_by_path() {
    let notes = vec![
        NoteReference {
            vault_path: "notes/hello.md".to_string(),
            title: Some("Hello".to_string()),
            aliases: vec![],
        },
        NoteReference {
            vault_path: "notes/world.md".to_string(),
            title: Some("World".to_string()),
            aliases: vec![],
        },
    ];

    assert_eq!(
        resolve_wiki_link_target("notes/hello.md", &notes),
        Some("notes/hello.md".to_string())
    );
    assert_eq!(
        resolve_wiki_link_target("notes/hello", &notes),
        Some("notes/hello.md".to_string())
    );
}

#[test]
fn test_resolve_wiki_link_by_title() {
    let notes = vec![NoteReference {
        vault_path: "notes/a.md".to_string(),
        title: Some("My Note".to_string()),
        aliases: vec![],
    }];

    assert_eq!(
        resolve_wiki_link_target("My Note", &notes),
        Some("notes/a.md".to_string())
    );
}

#[test]
fn test_resolve_wiki_link_by_alias() {
    let notes = vec![NoteReference {
        vault_path: "notes/a.md".to_string(),
        title: Some("A".to_string()),
        aliases: vec!["alias1".to_string(), "alias2".to_string()],
    }];

    assert_eq!(
        resolve_wiki_link_target("alias1", &notes),
        Some("notes/a.md".to_string())
    );
}

#[test]
fn test_resolve_wiki_link_unresolved() {
    let notes = vec![NoteReference {
        vault_path: "notes/a.md".to_string(),
        title: Some("A".to_string()),
        aliases: vec![],
    }];

    assert_eq!(resolve_wiki_link_target("nonexistent", &notes), None);
}

#[test]
fn test_compute_backlinks() {
    let edges = vec![
        LinkEdge {
            from_path: "a.md".to_string(),
            to_path: "b.md".to_string(),
            resolved: true,
            raw_target: "b".to_string(),
            alias: None,
            heading: None,
        },
        LinkEdge {
            from_path: "c.md".to_string(),
            to_path: "b.md".to_string(),
            resolved: true,
            raw_target: "b".to_string(),
            alias: None,
            heading: None,
        },
        LinkEdge {
            from_path: "a.md".to_string(),
            to_path: "d.md".to_string(),
            resolved: true,
            raw_target: "d".to_string(),
            alias: None,
            heading: None,
        },
    ];

    let backlinks = compute_backlinks(&edges);
    assert_eq!(backlinks.get("b.md").unwrap().len(), 2);
    assert_eq!(backlinks.get("d.md").unwrap().len(), 1);
}

#[test]
fn test_find_unresolved_links() {
    let notes = vec![NoteReference {
        vault_path: "a.md".to_string(),
        title: Some("A".to_string()),
        aliases: vec![],
    }];

    let links = vec![
        WikiLink {
            target: "a".to_string(),
            raw_target: "a".to_string(),
            alias: None,
            heading: None,
            char_start: 0,
            char_end: 0,
            line_start: 0,
            line_end: 0,
            text: "[[a]]".to_string(),
        },
        WikiLink {
            target: "nonexistent".to_string(),
            raw_target: "nonexistent".to_string(),
            alias: None,
            heading: None,
            char_start: 0,
            char_end: 0,
            line_start: 0,
            line_end: 0,
            text: "[[nonexistent]]".to_string(),
        },
    ];

    let unresolved = find_unresolved_links("a.md", &links, &notes);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].raw_target, "nonexistent");
}

#[test]
fn test_compute_link_stats() {
    let edges = vec![
        LinkEdge {
            from_path: "a.md".to_string(),
            to_path: "b.md".to_string(),
            resolved: true,
            raw_target: "b".to_string(),
            alias: None,
            heading: None,
        },
        LinkEdge {
            from_path: "a.md".to_string(),
            to_path: "nonexistent.md".to_string(),
            resolved: false,
            raw_target: "nonexistent".to_string(),
            alias: None,
            heading: None,
        },
    ];

    let note_paths = vec!["a.md".to_string(), "b.md".to_string(), "c.md".to_string()];
    let stats = compute_link_stats(&edges, &note_paths);

    assert_eq!(stats.total_links, 2);
    assert_eq!(stats.resolved_links, 1);
    assert_eq!(stats.unresolved_links, 1);
    assert_eq!(stats.unique_targets, 1);
    assert_eq!(stats.isolated_nodes, 2); // b.md and c.md have no outgoing links
}
