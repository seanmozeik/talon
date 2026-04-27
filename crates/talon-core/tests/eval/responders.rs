use serde_json::json;
use wiremock::{Request, Respond, ResponseTemplate};

use super::make_vector;

/// Dynamic `/embed` responder that returns a content-aware 5D vector per input.
pub struct SemanticQueryEmbedResponder;

impl Respond for SemanticQueryEmbedResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"inputs": []}));
        let inputs = body["inputs"].as_array().cloned().unwrap_or_default();
        let vectors: Vec<Vec<f32>> = inputs
            .iter()
            .map(|v| make_vector(v.as_str().unwrap_or("")))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!(vectors))
    }
}

/// Dynamic `/embed-chunked` responder that returns content-aware 5D vectors per chunk.
pub struct SemanticEmbedChunkedResponder;

impl Respond for SemanticEmbedChunkedResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).unwrap_or_else(|_| json!({"input": [[]]}));
        let groups = body["input"].as_array().cloned().unwrap_or_default();
        let data: Vec<serde_json::Value> = groups
            .iter()
            .enumerate()
            .map(|(i, group)| {
                let chunks = group.as_array().cloned().unwrap_or_default();
                let mut embeddings: Vec<Vec<f32>> = chunks
                    .iter()
                    .map(|c| make_vector(c.as_str().unwrap_or("")))
                    .collect();
                if embeddings.is_empty() {
                    embeddings.push(vec![0.0_f32; 5]);
                }
                json!({"embeddings": embeddings, "index": i})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({"data": data, "model": "semantic-test"}))
    }
}

/// Dynamic `/rerank` responder that scores candidates by keyword overlap with query.
pub struct SemanticRerankResponder;

impl Respond for SemanticRerankResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body)
            .unwrap_or_else(|_| json!({"query": "", "texts": []}));
        let texts = body["texts"].as_array().cloned().unwrap_or_default();
        let results: Vec<serde_json::Value> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let lower = t.as_str().unwrap_or("").to_lowercase();
                let score: f32 = if lower.contains("banana") {
                    0.98
                } else if lower.contains("orchard")
                    || lower.contains("apple")
                    || lower.contains("harvest")
                {
                    0.85
                } else if lower.contains("cafe") || lower.contains("café") {
                    0.60
                } else if lower.contains("graph") || lower.contains("link") || lower.contains("hub")
                {
                    0.70
                } else {
                    0.20
                };
                json!({"index": i, "score": score})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!(results))
    }
}
