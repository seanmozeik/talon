use tokenx_rs::estimate_token_count;

use crate::query::{LinkedNote, NoteExcerpt};

pub(super) fn estimate_payload_tokens(
    active_notes: &[NoteExcerpt],
    linked_context: &[LinkedNote],
) -> usize {
    let mut total = 0usize;
    for n in active_notes {
        total += estimate_token_count(n.vault_path.as_str())
            + estimate_token_count(&n.title)
            + estimate_token_count(&n.snippet)
            + 10; // XML structure, mtime attr, score attr
    }
    for l in linked_context {
        total += estimate_token_count(&l.title) + estimate_token_count(&l.link_text) + 8;
    }
    total
}

/// Greedy budget trimmer.
///
/// Drops lowest-ranked items from the lowest-priority non-empty section until
/// the token estimate fits within `budget` (with 2% slack per AC).
///
/// Section priority (highest to lowest): `linked_context` > `active_notes`.
pub(super) fn trim_to_budget(
    budget: usize,
    active_notes: &mut Vec<NoteExcerpt>,
    linked_context: &mut Vec<LinkedNote>,
    excluded_by_budget: &mut Vec<String>,
) {
    loop {
        let used = estimate_payload_tokens(active_notes, linked_context);
        // Allow 2% slack per AC.  Compute using integer arithmetic to avoid casts.
        let budget_with_slack = budget.saturating_add(budget / 50);
        if used <= budget_with_slack {
            break;
        }
        // Trim proportionally: drop from whichever section has more items so both
        // shrink together rather than one being fully exhausted before the other.
        let trim_active = active_notes.len() >= linked_context.len();
        let popped = if trim_active {
            active_notes
                .pop()
                .map(|item| item.vault_path.as_str().to_string())
                .or_else(|| {
                    linked_context
                        .pop()
                        .map(|item| item.vault_path.as_str().to_string())
                })
        } else {
            linked_context
                .pop()
                .map(|item| item.vault_path.as_str().to_string())
                .or_else(|| {
                    active_notes
                        .pop()
                        .map(|item| item.vault_path.as_str().to_string())
                })
        };
        match popped {
            Some(path) => excluded_by_budget.push(path),
            None => break,
        }
    }
}
