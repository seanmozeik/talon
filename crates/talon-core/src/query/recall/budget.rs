use tokenx_rs::estimate_token_count;

use crate::query::{EditedNote, FrontmatterFact, FuzzyAnchor, LinkedNote, NoteExcerpt};

pub(super) fn estimate_payload_tokens(
    active_notes: &[NoteExcerpt],
    linked_context: &[LinkedNote],
    frontmatter: &[FrontmatterFact],
    recent_edits: &[EditedNote],
    fuzzy_anchors: &[FuzzyAnchor],
) -> usize {
    let mut total = 0usize;
    for n in active_notes {
        total += estimate_token_count(&n.title) + estimate_token_count(&n.snippet) + 10;
    }
    for l in linked_context {
        total += estimate_token_count(&l.title) + estimate_token_count(&l.link_text) + 8;
    }
    for f in frontmatter {
        total += estimate_token_count(&f.key) + estimate_token_count(&f.value.to_string()) + 6;
    }
    for e in recent_edits {
        total += estimate_token_count(&e.title) + 8;
    }
    for a in fuzzy_anchors {
        total += estimate_token_count(&a.title) + estimate_token_count(&a.snippet) + 8;
    }
    total
}

/// Greedy budget trimmer.
///
/// Drops lowest-ranked items from the lowest-priority non-empty section until
/// the token estimate fits within `budget` (with 2% slack per AC).
///
/// Section priority (highest to lowest):
/// `active_notes` > `linked_context` > `frontmatter` > `recent_edits` > `fuzzy_anchors`.
pub(super) fn trim_to_budget(
    budget: usize,
    active_notes: &mut Vec<NoteExcerpt>,
    linked_context: &mut Vec<LinkedNote>,
    frontmatter: &mut Vec<FrontmatterFact>,
    recent_edits: &mut Vec<EditedNote>,
    fuzzy_anchors: &mut Vec<FuzzyAnchor>,
    excluded_by_budget: &mut Vec<String>,
) {
    loop {
        let used = estimate_payload_tokens(
            active_notes,
            linked_context,
            frontmatter,
            recent_edits,
            fuzzy_anchors,
        );
        // Allow 2% slack per AC.  Compute using integer arithmetic to avoid casts.
        let budget_with_slack = budget.saturating_add(budget / 50);
        if used <= budget_with_slack {
            break;
        }
        if let Some(item) = fuzzy_anchors.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = recent_edits.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = frontmatter.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = linked_context.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else if let Some(item) = active_notes.pop() {
            excluded_by_budget.push(item.vault_path.as_str().to_string());
        } else {
            break;
        }
    }
}
