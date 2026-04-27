fn bool_to_f32(b: bool) -> f32 {
    f32::from(u8::from(b))
}

pub fn make_vector(text: &str) -> Vec<f32> {
    let lower = text.to_lowercase();
    vec![
        bool_to_f32(
            lower.contains("orchard")
                || lower.contains("apple")
                || lower.contains("harvest")
                || lower.contains("cider"),
        ),
        bool_to_f32(lower.contains("banana") || lower.contains("grove")),
        bool_to_f32(lower.contains("cafe") || lower.contains("café") || lower.contains("espresso")),
        bool_to_f32(
            lower.contains("graph")
                || lower.contains("link")
                || lower.contains("hub")
                || lower.contains("child"),
        ),
        bool_to_f32(
            lower.contains("lifecycle") || lower.contains("delete") || lower.contains("rename"),
        ),
    ]
}
