/// Round `idx` down to the nearest UTF-8 char boundary in `s`. Required when
/// truncating arbitrary vault content; slicing inside a non-ASCII char panics.
const fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub(super) fn to_headline(snippet: &str) -> String {
    let first = snippet
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if first.len() <= 120 {
        return first.to_owned();
    }
    let cap = floor_char_boundary(first, 120);
    let prefix = &first[..cap];
    if let Some(i) = prefix.rfind(['.', '!', '?']) {
        return first[..=i].to_owned();
    }
    let trimmed = floor_char_boundary(first, 117);
    format!("{}…", &first[..trimmed])
}
