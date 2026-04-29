use super::constants::DEFAULT_SNIPPET_LENGTH;

pub const CHUNK_QUERY_TERM_WEIGHT: u32 = 1;
pub const CHUNK_INTENT_TERM_WEIGHT: u32 = 5;

pub fn focused_chunk_excerpt(
    text: &str,
    query_terms: &[String],
    intent_terms: &[String],
) -> String {
    let char_count = text.chars().count();
    let snippet_len = DEFAULT_SNIPPET_LENGTH as usize;
    if char_count <= snippet_len {
        return text.to_owned();
    }

    let Some(anchor) = best_excerpt_anchor(text, query_terms, intent_terms) else {
        return text.chars().take(snippet_len).collect();
    };
    let context_before = snippet_len / 3;
    let start = anchor.saturating_sub(context_before);
    let start = start.min(char_count.saturating_sub(snippet_len));
    let end = (start + snippet_len).min(char_count);

    let mut excerpt = String::new();
    if start > 0 {
        excerpt.push_str("...");
    }
    excerpt.extend(text.chars().skip(start).take(end.saturating_sub(start)));
    if end < char_count {
        excerpt.push_str("...");
    }
    excerpt
}

fn best_excerpt_anchor(
    text: &str,
    query_terms: &[String],
    intent_terms: &[String],
) -> Option<usize> {
    let normalized = crate::text::nfd::normalize(text).to_lowercase();
    let mut best: Option<(u32, usize)> = None;
    for (terms, weight) in [
        (query_terms, CHUNK_QUERY_TERM_WEIGHT),
        (intent_terms, CHUNK_INTENT_TERM_WEIGHT),
    ] {
        for term in terms {
            let Some(byte_index) = normalized.find(term.as_str()) else {
                continue;
            };
            let char_index = normalized[..byte_index].chars().count();
            match best {
                Some((best_weight, best_index))
                    if weight < best_weight
                        || (weight == best_weight && char_index >= best_index) => {}
                _ => best = Some((weight, char_index)),
            }
        }
    }
    best.map(|(_, char_index)| char_index)
}
