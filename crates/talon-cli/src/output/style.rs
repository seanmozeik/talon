use anstyle::Style;

/// Returns a style or the no-op style depending on whether colors are enabled.
pub(super) const fn cs(colors: bool, s: Style) -> Style {
    if colors { s } else { Style::new() }
}

/// Word-wraps `text` into lines of at most `max_width` chars.
pub(super) fn wrap_words(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 || text.chars().count() <= max_width {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.lines() {
        let mut current = String::new();
        let mut current_width = 0usize;
        for word in paragraph.split_whitespace() {
            let word_width = word.chars().count();
            if !current.is_empty() {
                if current_width + 1 + word_width <= max_width {
                    current.push(' ');
                    current_width += 1;
                } else {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
            }
            current.push_str(word);
            current_width += word_width;
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Wraps `text` for display after a fixed prefix.
pub(super) fn wrap_prefixed_words(prefix: &str, text: &str, max_width: usize) -> Vec<String> {
    let continuation = " ".repeat(prefix.chars().count());
    let first_width = max_width.saturating_sub(prefix.chars().count()).max(1);
    let rest_width = max_width
        .saturating_sub(continuation.chars().count())
        .max(1);
    let mut wrapped = wrap_words(text, first_width);
    let Some(first) = wrapped.first_mut() else {
        return vec![prefix.to_string()];
    };
    first.insert_str(0, prefix);
    for line in wrapped.iter_mut().skip(1) {
        let rewrapped = wrap_words(line, rest_width);
        if rewrapped.len() == 1 {
            line.insert_str(0, &continuation);
        }
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_words_breaks_on_word_boundaries() {
        assert_eq!(
            wrap_words("alpha beta gamma", 10),
            vec!["alpha beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn wrap_prefixed_words_indents_continuations() {
        assert_eq!(
            wrap_prefixed_words("queries: ", "alpha beta gamma", 16),
            vec![
                "queries: alpha".to_string(),
                "         beta".to_string(),
                "         gamma".to_string()
            ]
        );
    }
}
