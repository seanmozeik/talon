use anstyle::Style;

/// Returns a style or the no-op style depending on whether colors are enabled.
pub(super) const fn cs(colors: bool, s: Style) -> Style {
    if colors { s } else { Style::new() }
}

/// Word-wraps `text` into lines of at most `max_width` chars.
pub(super) fn wrap_words(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 || text.len() <= max_width {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() {
            if current.len() + 1 + word.len() <= max_width {
                current.push(' ');
            } else {
                lines.push(current.clone());
                current.clear();
            }
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
