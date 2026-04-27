//! Evidence score computation for `talon recall`.
//!
//! Formula (v2 calibration, weights are NOT a config knob in v1):
//!
//! ```text
//! evidence_score =
//!   0.50 * top_rerank_score        // strongest signal: cross-encoder confidence
//! + 0.20 * top_lexical_indicator   // BM25 / title hit
//! + 0.20 * graph_density_bonus     // top result is well-linked
//! + 0.10 * recency_bonus           // top result was recently modified
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
}

/// Returns the evidence score for a recall response, clamped to [0, 1].
///
/// Returns 0.0 immediately when `inputs` represents a zero-result query
/// (indicated by `top_rerank_score == 0.0` and `top_lexical_indicator == 0.0`
/// and `top_result_link_count == 0`).
#[must_use]
pub fn compute_evidence_score(inputs: &EvidenceInputs) -> f64 {
    let rerank = inputs.top_rerank_score.clamp(0.0, 1.0);
    let lexical = inputs.top_lexical_indicator.clamp(0.0, 1.0);

    // Early-exit for zero-result queries (all signals absent).
    if inputs.top_result_link_count == 0 && rerank < f64::EPSILON && lexical < f64::EPSILON {
        return 0.0;
    }

    let graph_density = (f64::from(inputs.top_result_link_count) / 5.0).min(1.0);
    let recency = (-inputs.days_since_top_result_modified / 14.0).exp();

    0.50_f64.mul_add(
        inputs.top_rerank_score.clamp(0.0, 1.0),
        0.20_f64.mul_add(
            inputs.top_lexical_indicator.clamp(0.0, 1.0),
            0.20_f64.mul_add(
                graph_density.clamp(0.0, 1.0),
                0.10 * recency.clamp(0.0, 1.0),
            ),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_approx(got: f64, want: f64, label: &str) {
        assert!(
            (got - want).abs() < 1e-9,
            "{label}: got {got:.10}, want {want:.10}"
        );
    }

    #[test]
    fn zero_result_returns_zero() {
        let inputs = EvidenceInputs {
            top_rerank_score: 0.0,
            top_lexical_indicator: 0.0,
            top_result_link_count: 0,
            days_since_top_result_modified: 0.0,
        };
        assert_approx(compute_evidence_score(&inputs), 0.0, "zero result");
    }

    #[test]
    fn rerank_only_signal() {
        // 0.50*0.8 + 0.20*0 + 0.20*0 + 0.10*exp(0) = 0.40 + 0.10 = 0.50
        let inputs = EvidenceInputs {
            top_rerank_score: 0.8,
            top_lexical_indicator: 0.0,
            top_result_link_count: 0,
            days_since_top_result_modified: 0.0,
        };
        let expected = 0.50_f64.mul_add(0.8, 0.10 * (-0.0_f64 / 14.0_f64).exp());
        assert_approx(compute_evidence_score(&inputs), expected, "rerank only");
    }

    #[test]
    fn all_signals_strong_returns_one() {
        // 0.50 + 0.20 + 0.20*(10/5 capped 1.0) + 0.10*exp(0) = 1.0
        let inputs = EvidenceInputs {
            top_rerank_score: 1.0,
            top_lexical_indicator: 1.0,
            top_result_link_count: 10,
            days_since_top_result_modified: 0.0,
        };
        assert_approx(compute_evidence_score(&inputs), 1.0, "all strong");
    }
}
