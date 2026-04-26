//! Evidence score computation for `talon recall`.
//!
//! Formula (v1 calibration, weights are NOT a config knob in v1):
//!
//! ```text
//! evidence_score =
//!   0.45 * top_rerank_score        // strongest signal: cross-encoder confidence
//! + 0.20 * top_lexical_indicator   // BM25 / title hit
//! + 0.15 * graph_density_bonus     // top result is well-linked
//! + 0.10 * recency_bonus           // top result was recently modified
//! + 0.10 * frontmatter_match_indicator  // structured frontmatter evidence
//! ```
//!
//! All inputs and the output are clamped to [0, 1].
//!
//! Calibration attribution: weight design inspired by MemoryBank/Ebbinghaus
//! decay for recency), Mem0/Zep dual-signal composites, and the Confidence
//! Gate pattern from the RAG literature.  Re-tuning via the US-022 eval suite
//! is deferred to a future story.

/// All inputs required to compute an evidence score.
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceInputs {
    /// Highest retrieval/rerank score across `active_notes` in \[0, 1\].
    /// When `--fast` skips rerank, substitute the top `hybrid_pipeline` score
    /// (already in \[0, 1\] after US-023a RRF normalization).
    pub top_rerank_score: f64,
    /// 1.0 if any BM25 / title match returned a result, 0.0 otherwise.
    pub top_lexical_indicator: f64,
    /// Outgoing + incoming link count for the top result (raw, uncapped).
    pub top_result_link_count: u32,
    /// Days since the top result was last modified (fractional).
    pub days_since_top_result_modified: f64,
    /// 1.0 if any frontmatter match (meta --where) returned a result, 0.0 otherwise.
    pub frontmatter_match_indicator: f64,
}

/// Returns the evidence score for a recall response, clamped to [0, 1].
///
/// Returns 0.0 immediately when `inputs` represents a zero-result query
/// (indicated by `top_rerank_score == 0.0` and `top_lexical_indicator == 0.0`
/// and `frontmatter_match_indicator == 0.0` and `top_result_link_count == 0`).
#[must_use]
pub fn compute_evidence_score(inputs: &EvidenceInputs) -> f64 {
    let rerank = inputs.top_rerank_score.clamp(0.0, 1.0);
    let lexical = inputs.top_lexical_indicator.clamp(0.0, 1.0);
    let frontmatter = inputs.frontmatter_match_indicator.clamp(0.0, 1.0);

    // Early-exit for zero-result queries (all signals absent).
    if inputs.top_result_link_count == 0
        && rerank < f64::EPSILON
        && lexical < f64::EPSILON
        && frontmatter < f64::EPSILON
    {
        return 0.0;
    }

    // graph_density_bonus = min(1.0, link_count / 5)
    let graph_density = (f64::from(inputs.top_result_link_count) / 5.0).min(1.0);

    // recency_bonus = exp(-days / 14)
    let recency = (-inputs.days_since_top_result_modified / 14.0).exp();

    let score = 0.10_f64.mul_add(
        frontmatter,
        0.10_f64.mul_add(
            recency,
            0.15_f64.mul_add(graph_density, 0.45_f64.mul_add(rerank, 0.20 * lexical)),
        ),
    );

    score.clamp(0.0, 1.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn assert_approx(actual: f64, expected: f64, label: &str) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "{label}: expected {expected:.6}, got {actual:.6}"
        );
    }

    #[test]
    fn zero_result_returns_zero() {
        let inputs = EvidenceInputs {
            top_rerank_score: 0.0,
            top_lexical_indicator: 0.0,
            top_result_link_count: 0,
            days_since_top_result_modified: 0.0,
            frontmatter_match_indicator: 0.0,
        };
        assert_approx(compute_evidence_score(&inputs), 0.0, "zero result");
    }

    #[test]
    fn rerank_skipped_substitutes_hybrid_score() {
        // --fast: no rerank; top hybrid score = 0.85, lexical hit, 2 links, 1 day ago.
        let inputs = EvidenceInputs {
            top_rerank_score: 0.85, // hybrid score substituted
            top_lexical_indicator: 1.0,
            top_result_link_count: 2,
            days_since_top_result_modified: 1.0,
            frontmatter_match_indicator: 0.0,
        };
        // 0.45*0.85 + 0.20*1.0 + 0.15*(2/5) + 0.10*exp(-1/14) + 0.10*0
        // = 0.3825 + 0.20 + 0.06 + 0.10*0.9310 + 0
        // = 0.3825 + 0.20 + 0.06 + 0.09310 = 0.73560
        let expected = 0.10_f64.mul_add(
            0.0,
            0.10_f64.mul_add(
                (-1.0_f64 / 14.0).exp(),
                0.15_f64.mul_add(2.0 / 5.0, 0.45_f64.mul_add(0.85, 0.20 * 1.0)),
            ),
        );
        assert_approx(compute_evidence_score(&inputs), expected, "rerank skipped");
    }

    #[test]
    fn all_signals_strong_returns_near_one() {
        let inputs = EvidenceInputs {
            top_rerank_score: 1.0,
            top_lexical_indicator: 1.0,
            top_result_link_count: 10,           // capped at 1.0
            days_since_top_result_modified: 0.0, // exp(0) = 1.0
            frontmatter_match_indicator: 1.0,
        };
        // 0.45 + 0.20 + 0.15 + 0.10 + 0.10 = 1.0
        assert_approx(compute_evidence_score(&inputs), 1.0, "all strong");
    }

    #[test]
    fn only_semantic_strong_returns_approx_045() {
        // Only rerank signal; no lexical, no graph, just modified today, no frontmatter.
        let inputs = EvidenceInputs {
            top_rerank_score: 1.0,
            top_lexical_indicator: 0.0,
            top_result_link_count: 0,
            days_since_top_result_modified: 0.0,
            frontmatter_match_indicator: 0.0,
        };
        // 0.45*1 + 0 + 0 + 0.10*exp(0) + 0 = 0.45 + 0.10 = 0.55
        // (recency_bonus still fires because days=0 → exp(0)=1)
        let expected = 0.10_f64.mul_add(1.0, 0.45);
        assert_approx(compute_evidence_score(&inputs), expected, "only semantic");
    }

    #[test]
    fn only_frontmatter_match_returns_approx_010() {
        // Only frontmatter match; rerank_score=0 but link_count > 0 to avoid zero-result path.
        let inputs = EvidenceInputs {
            top_rerank_score: 0.0,
            top_lexical_indicator: 0.0,
            top_result_link_count: 1, // avoids zero-result short-circuit
            days_since_top_result_modified: 9999.0, // recency ≈ 0
            frontmatter_match_indicator: 1.0,
        };
        // 0 + 0 + 0.15*(1/5) + 0.10*exp(-9999/14) + 0.10
        // ≈ 0.03 + ~0 + 0.10 = 0.13
        let score = compute_evidence_score(&inputs);
        assert!(
            score < 0.20,
            "only-frontmatter score should be low, got {score}"
        );
        assert!(
            score > 0.05,
            "only-frontmatter score should be nonzero, got {score}"
        );
    }

    #[test]
    fn score_is_clamped_to_unit_interval() {
        let inputs = EvidenceInputs {
            top_rerank_score: 2.0, // out of range
            top_lexical_indicator: 2.0,
            top_result_link_count: 100,
            days_since_top_result_modified: -1.0, // nonsensical negative
            frontmatter_match_indicator: 2.0,
        };
        let score = compute_evidence_score(&inputs);
        assert!(score <= 1.0, "score must not exceed 1.0, got {score}");
        assert!(score >= 0.0, "score must not be negative, got {score}");
    }
}
