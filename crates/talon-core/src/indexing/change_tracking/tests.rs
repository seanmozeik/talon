use super::*;

#[test]
fn test_file_change_state_active() {
    let state = FileChangeState::active("test.md".to_string(), 1000);
    assert!(state.is_active());
    assert!(!state.is_modified());
}

#[test]
fn test_file_change_state_modified() {
    let mut state = FileChangeState::active("test.md".to_string(), 2000);
    state.mark_indexed(1000);
    assert!(state.is_modified());
}

#[test]
fn test_file_change_state_tombstoned() {
    let mut state = FileChangeState::active("test.md".to_string(), 1000);
    state.tombstone(2000);
    assert!(!state.is_active());
    assert!(state.tombstoned);
    assert_eq!(state.tombstoned_at, Some(2000));
}

#[test]
fn test_change_index_register_and_update() {
    let mut idx = ChangeIndex::default();
    idx.register_active("a.md".to_string(), 1000, 1000);
    idx.update_mtime("a.md", 2000);

    let state = idx.states.get("a.md").unwrap();
    assert_eq!(state.mtime, 2000);
    assert!(state.is_modified());
}

#[test]
fn test_change_index_tombstone() {
    let mut idx = ChangeIndex::default();
    idx.register_active("a.md".to_string(), 1000, 1000);
    idx.tombstone("a.md", 2000);

    assert!(idx.states.get("a.md").unwrap().tombstoned);
    assert_eq!(idx.tombstones.len(), 1);
}

#[test]
fn test_change_index_prune_tombstones() {
    let mut idx = ChangeIndex::default();
    idx.register_active("a.md".to_string(), 1000, 1000);
    idx.tombstone("a.md", 1000);

    // Prune tombstones older than 500ms (should prune)
    let pruned = idx.prune_tombstones(500, 2000);
    assert_eq!(pruned.len(), 1);
    assert!(idx.tombstones.is_empty());
}

#[test]
fn test_parse_since_numeric() {
    assert_eq!(parse_since("1700000000000").unwrap(), 1_700_000_000_000);
}

#[test]
fn test_parse_since_iso8601() {
    let result = parse_since("2024-01-15T10:30:00Z").unwrap();
    // Just verify it parses without error
    assert!(result > 0);
}

#[test]
fn test_parse_since_date_only() {
    let result = parse_since("2024-01-15").unwrap();
    assert!(result > 0);
}

#[test]
fn test_parse_since_relative_duration() {
    let before = now_ms();
    let result = parse_since("3h").unwrap();
    let after = now_ms();
    let three_hours = 3 * 60 * 60 * 1000;

    assert!(result <= before.saturating_sub(three_hours));
    assert!(result >= after.saturating_sub(three_hours + 1000));
}

#[test]
fn test_parse_since_invalid() {
    assert!(parse_since("not-a-timestamp").is_err());
}

#[test]
fn test_change_feed_computation() {
    let mut idx = ChangeIndex::default();
    idx.register_active("a.md".to_string(), 1000, 1000);
    idx.register_active("b.md".to_string(), 2000, 2000);
    idx.update_mtime("b.md", 3000);

    let feed = idx.compute_change_feed(1500);

    // a.md was indexed before since, so not in feed
    // b.md was indexed after since and is modified
    assert!(feed.added.is_empty());
    assert_eq!(feed.modified.len(), 1);
    assert_eq!(feed.modified[0].path, "b.md");
}
