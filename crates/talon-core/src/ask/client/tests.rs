use super::*;

#[test]
fn normalize_queries_excludes_original_and_duplicates() {
    let queries = vec![
        "knife skills".to_string(),
        "Knife Skills".to_string(),
        "claw grip".to_string(),
        "julienne practice".to_string(),
    ];
    let normalized = normalize_queries("knife skills", queries, 4);
    assert_eq!(normalized, vec!["claw grip", "julienne practice"]);
}
