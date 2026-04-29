use std::fmt::Write as _;
use std::path::Path;

pub(super) fn format_ref(
    vault: Option<&str>,
    vault_path: &str,
    title: Option<&str>,
    heading: Option<&str>,
    clickable: bool,
) -> String {
    let label = wikilink_label(vault_path, title, heading);
    if !clickable {
        return label;
    }
    let Some(vault) = vault else {
        return label;
    };
    let absolute = Path::new(vault).join(vault_path);
    let uri = format!(
        "obsidian://open?path={}",
        percent_encode(&absolute.display().to_string())
    );
    format!("\u{1b}]8;;{uri}\u{1b}\\{label}\u{1b}]8;;\u{1b}\\")
}

fn wikilink_label(vault_path: &str, title: Option<&str>, heading: Option<&str>) -> String {
    let heading_suffix = heading.map_or_else(String::new, |heading| format!("#{heading}"));
    let target = format!("{vault_path}{heading_suffix}");
    title.filter(|title| *title != vault_path).map_or_else(
        || format!("[[{target}]]"),
        |title| format!("[[{target}|{title}]]"),
    )
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                out.push('%');
                let _ = write!(out, "{byte:02X}");
            }
        }
    }
    out
}
