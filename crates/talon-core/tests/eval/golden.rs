use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GoldenQuery {
    pub id: String,
    pub query: String,
    pub expected_paths: Vec<String>,
    #[serde(default)]
    pub partial_paths: Vec<String>,
    pub category: String,
}

pub fn load_golden_set() -> Vec<GoldenQuery> {
    let json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/golden-set.json"
    ));
    serde_json::from_str(json).expect("golden-set.json must be valid JSON")
}
