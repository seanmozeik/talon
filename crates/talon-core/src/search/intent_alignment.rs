//! Intent alignment scoring for agent-supplied search intent.

use crate::text::nfd;

use super::types::RawSearchResult;

const INTENT_ALIGNMENT_WEIGHT: f64 = 0.35;
const OPERATIONAL_SCORE_WEIGHT: f64 = 0.50;
const OPERATIONAL_TERM_WEIGHT: f64 = 0.35;
const OPERATIONAL_COLLECTION_WEIGHT: f64 = 0.15;
const OPERATIONAL_INTENT_TERMS: &[&str] = &[
    "active",
    "blocker",
    "blockers",
    "checklist",
    "current",
    "next",
    "project",
    "status",
];

pub fn apply_intent_alignment_boost(
    candidates: Vec<RawSearchResult>,
    intent_terms: &[String],
) -> Vec<RawSearchResult> {
    if intent_terms.is_empty() {
        return candidates;
    }

    let mut out: Vec<RawSearchResult> = candidates
        .into_iter()
        .map(|candidate| {
            let score = intent_aligned_score(&candidate, intent_terms);
            RawSearchResult { score, ..candidate }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    out
}

fn intent_aligned_score(candidate: &RawSearchResult, intent_terms: &[String]) -> f64 {
    let alignment = intent_alignment(candidate, intent_terms);
    if is_operational_intent(intent_terms) {
        return (candidate.score.mul_add(
            OPERATIONAL_SCORE_WEIGHT,
            alignment.mul_add(
                OPERATIONAL_TERM_WEIGHT,
                operational_collection_alignment(candidate) * OPERATIONAL_COLLECTION_WEIGHT,
            ),
        ))
        .clamp(0.0, 1.0);
    }

    f64::mul_add(
        INTENT_ALIGNMENT_WEIGHT,
        alignment - candidate.score,
        candidate.score,
    )
    .clamp(0.0, 1.0)
}

fn intent_alignment(candidate: &RawSearchResult, intent_terms: &[String]) -> f64 {
    let text = nfd::normalize(&format!(
        "{}\n{}\n{}",
        candidate.title, candidate.path, candidate.snippet
    ))
    .to_lowercase();
    let hits = intent_terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .count();
    let hits = u32::try_from(hits).unwrap_or(u32::MAX);
    let term_count = u32::try_from(intent_terms.len()).unwrap_or(u32::MAX);
    f64::from(hits) / f64::from(term_count)
}

fn is_operational_intent(intent_terms: &[String]) -> bool {
    intent_terms
        .iter()
        .any(|term| OPERATIONAL_INTENT_TERMS.contains(&term.as_str()))
}

fn operational_collection_alignment(candidate: &RawSearchResult) -> f64 {
    if candidate.path.starts_with("projects/") || candidate.path.starts_with("artifacts/") {
        1.0
    } else {
        0.0
    }
}
