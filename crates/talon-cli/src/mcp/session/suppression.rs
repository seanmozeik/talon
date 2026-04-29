use super::fingerprint::QueryFingerprint;
use super::ledger::{InjectionReason, SuppressedRecall, SuppressionReason, TurnLedger};

const CONFIDENCE_GATE: f64 = 0.4;
const SAME_CHUNK_TURNS: usize = 3;
const SAME_NOTE_TURNS: usize = 2;
/// Jaccard below this = high drift
const HIGH_DRIFT_THRESHOLD: f64 = 0.4;

/// A candidate from recall output before suppression filtering.
#[derive(Debug)]
pub struct RecallCandidate {
    pub chunk_id: String,
    pub path: String,
    pub score: f64,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug)]
pub struct SuppressionResult {
    pub injected: Vec<(RecallCandidate, InjectionReason)>,
    pub suppressed: Vec<SuppressedRecall>,
}

/// Apply suppression policy to a list of candidates.
///
/// `current_fp` is the current turn's fingerprint.
/// `ledger` holds prior turn history.
#[must_use]
pub fn apply_suppression(
    candidates: Vec<RecallCandidate>,
    ledger: &TurnLedger,
    current_fp: &QueryFingerprint,
    budget_tokens: u32,
) -> SuppressionResult {
    let last_fp = ledger
        .last_fingerprint()
        .map(QueryFingerprint::from_message);
    let similarity = last_fp.as_ref().map_or(0.0, |fp| current_fp.similarity(fp));
    let high_drift = similarity < HIGH_DRIFT_THRESHOLD;

    // Query repeated exactly or very closely: suppress all
    if similarity > 0.9 {
        return SuppressionResult {
            injected: vec![],
            suppressed: candidates
                .into_iter()
                .map(|c| SuppressedRecall {
                    chunk_id: c.chunk_id,
                    path: c.path,
                    score: c.score,
                    reason: SuppressionReason::QueryRepeated,
                })
                .collect(),
        };
    }

    let mut injected = Vec::new();
    let mut suppressed = Vec::new();
    let mut tokens_used: u32 = 0;

    for candidate in candidates {
        // Confidence gate
        if candidate.score < CONFIDENCE_GATE {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::BelowConfidenceGate,
            });
            continue;
        }

        // Same chunk recently injected
        let chunk_recent = ledger.chunk_injected_in_last_n(&candidate.chunk_id, SAME_CHUNK_TURNS);
        if chunk_recent > 0 && !high_drift {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::SameChunkRecentlyInjected,
            });
            continue;
        }

        // Same note recently injected
        let note_recent = ledger.note_injected_in_last_n(&candidate.path, SAME_NOTE_TURNS);
        if note_recent > 0 && !high_drift {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::SameNoteRecentlyInjected,
            });
            continue;
        }

        // Budget check (rough: ~4 chars per token). Truncation from usize to u32
        // is intentional — snippet lengths never approach 4 GiB.
        #[allow(clippy::cast_possible_truncation)]
        let token_est = (candidate.snippet.len() / 4) as u32;
        if tokens_used + token_est > budget_tokens {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::BudgetTrimmed,
            });
            continue;
        }

        tokens_used += token_est;
        let reason = if high_drift && chunk_recent > 0 {
            InjectionReason::QueryDrift
        } else {
            InjectionReason::Novel
        };
        injected.push((candidate, reason));
    }

    SuppressionResult {
        injected,
        suppressed,
    }
}

#[cfg(test)]
mod tests {
    use super::{RecallCandidate, apply_suppression};
    use crate::mcp::session::fingerprint::QueryFingerprint;
    use crate::mcp::session::ledger::{InjectedChunk, SuppressionReason, TurnLedger, TurnRecord};

    fn make_candidate(chunk_id: &str, path: &str, score: f64, snippet: &str) -> RecallCandidate {
        RecallCandidate {
            chunk_id: chunk_id.to_owned(),
            path: path.to_owned(),
            score,
            title: "Test Note".to_owned(),
            snippet: snippet.to_owned(),
        }
    }

    fn ledger_with_chunk(chunk_id: &str, path: &str) -> TurnLedger {
        let mut ledger = TurnLedger::new();
        ledger.record_turn(TurnRecord {
            turn_id: "turn-1".to_owned(),
            query_fingerprint: "some query".to_owned(),
            injected: vec![InjectedChunk {
                chunk_id: chunk_id.to_owned(),
                path: path.to_owned(),
                score: 0.9,
            }],
            suppressed: vec![],
            skipped: false,
        });
        ledger
    }

    #[test]
    fn same_chunk_suppressed_in_last_three_turns() {
        // The ledger stores fingerprint "some query" for the prior turn.
        // Use a query with significant overlap so Jaccard >= HIGH_DRIFT_THRESHOLD (0.4)
        // and suppression is not overridden by drift detection.
        // "some query topic" vs "some query": intersection={some,query}, union={some,query,topic}
        // Jaccard = 2/3 ≈ 0.67 >= 0.4, so high_drift = false and suppression applies.
        let ledger = ledger_with_chunk("chunk-abc", "notes/foo.md");
        let fp = QueryFingerprint::from_message("some query topic");
        let candidates = vec![make_candidate(
            "chunk-abc",
            "notes/foo.md",
            0.85,
            "some snippet content here for the test",
        )];
        let result = apply_suppression(candidates, &ledger, &fp, 10_000);
        assert_eq!(result.injected.len(), 0);
        assert_eq!(result.suppressed.len(), 1);
        assert_eq!(
            result.suppressed[0].reason,
            SuppressionReason::SameChunkRecentlyInjected
        );
    }

    #[test]
    fn below_confidence_gate_suppressed() {
        let ledger = TurnLedger::new();
        let fp = QueryFingerprint::from_message("any query here");
        let candidates = vec![make_candidate(
            "chunk-xyz",
            "notes/bar.md",
            0.2,
            "low score snippet content",
        )];
        let result = apply_suppression(candidates, &ledger, &fp, 10_000);
        assert_eq!(result.injected.len(), 0);
        assert_eq!(result.suppressed.len(), 1);
        assert_eq!(
            result.suppressed[0].reason,
            SuppressionReason::BelowConfidenceGate
        );
    }

    #[test]
    fn query_repeated_suppresses_all() {
        // Set up a ledger with a prior turn using the same query
        let mut ledger = TurnLedger::new();
        let prior_query = "what is the vault indexing strategy";
        ledger.record_turn(TurnRecord {
            turn_id: "turn-1".to_owned(),
            query_fingerprint: prior_query.to_owned(),
            injected: vec![],
            suppressed: vec![],
            skipped: false,
        });

        // Current query is nearly identical (>0.9 Jaccard)
        let fp = QueryFingerprint::from_message(prior_query);
        let candidates = vec![
            make_candidate("chunk-1", "notes/a.md", 0.9, "snippet one"),
            make_candidate("chunk-2", "notes/b.md", 0.8, "snippet two"),
        ];
        let result = apply_suppression(candidates, &ledger, &fp, 10_000);
        assert_eq!(result.injected.len(), 0);
        assert_eq!(result.suppressed.len(), 2);
        for s in &result.suppressed {
            assert_eq!(s.reason, SuppressionReason::QueryRepeated);
        }
    }
}
