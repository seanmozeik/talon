use super::ledger::{InjectedChunk, SuppressedRecall, SuppressionReason, TurnLedger};

const CONFIDENCE_GATE: f64 = 0.55;

/// Default per-turn score decay for chunks seen in prior turns.
///
/// Lower = more aggressive suppression; higher = more permissive re-injection.
pub const DEFAULT_DECAY: f64 = 0.85;

/// Returns the effective score of a chunk seen `turns_since` turns ago, after
/// applying the per-turn decay multiplier.
///
/// `effective = raw × decay^turns_since`
///
/// The chunk passes suppression if `effective >= CONFIDENCE_GATE`.
/// A chunk seen 0 turns ago (same turn) always returns 0.0 (never re-inject).
fn effective_score(raw: f64, turns_since: usize, decay: f64) -> f64 {
    if turns_since == 0 {
        return 0.0;
    }
    raw * decay.powi(i32::try_from(turns_since).unwrap_or(i32::MAX))
}

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
    pub injected: Vec<RecallCandidate>,
    pub suppressed: Vec<SuppressedRecall>,
}

/// Apply output-level suppression to a list of recall candidates.
///
/// Suppresses chunks below the confidence gate or whose score, after applying
/// the per-turn decay multiplier, falls below the gate. Does NOT use query
/// similarity — we deduplicate injected context, not input messages.
///
/// `decay` is the per-turn multiplier (e.g. 0.85). A chunk last injected N
/// turns ago has its raw score multiplied by `decay^N` before comparing to
/// the confidence gate. If all candidates are suppressed, `injected` is empty
/// and the caller must skip injection entirely rather than substituting
/// lower-ranked results.
#[must_use]
pub fn apply_suppression(
    candidates: Vec<RecallCandidate>,
    ledger: &TurnLedger,
    decay: f64,
) -> SuppressionResult {
    let mut injected = Vec::new();
    let mut suppressed = Vec::new();

    for candidate in candidates {
        if candidate.score < CONFIDENCE_GATE {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::BelowConfidenceGate,
            });
            continue;
        }

        // Chunk-level decay.
        if ledger
            .turns_since_chunk_last_injected(&candidate.chunk_id)
            .is_some_and(|n| effective_score(candidate.score, n, decay) < CONFIDENCE_GATE)
        {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::SameChunkRecentlyInjected,
            });
            continue;
        }

        // Note-level decay: same multiplier as chunk, applied to the whole note path.
        if ledger
            .turns_since_note_last_injected(&candidate.path)
            .is_some_and(|n| effective_score(candidate.score, n, decay) < CONFIDENCE_GATE)
        {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::SameNoteRecentlyInjected,
            });
            continue;
        }

        injected.push(candidate);
    }

    SuppressionResult {
        injected,
        suppressed,
    }
}

/// Builds an [`InjectedChunk`] record from a suppression-approved candidate.
#[must_use]
pub fn to_injected_chunk(candidate: &RecallCandidate) -> InjectedChunk {
    InjectedChunk {
        chunk_id: candidate.chunk_id.clone(),
        path: candidate.path.clone(),
        score: candidate.score,
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_DECAY, RecallCandidate, apply_suppression};
    use crate::mcp::session::ledger::{InjectedChunk, SuppressionReason, TurnLedger, TurnRecord};

    fn candidate(chunk_id: &str, path: &str, score: f64) -> RecallCandidate {
        RecallCandidate {
            chunk_id: chunk_id.to_owned(),
            path: path.to_owned(),
            score,
            title: "Test".to_owned(),
            snippet: "snippet".to_owned(),
        }
    }

    fn ledger_with_chunk_n_turns_ago(chunk_id: &str, path: &str, n: usize) -> TurnLedger {
        let mut ledger = TurnLedger::new();
        // Insert the target chunk injection in turn 0.
        ledger.record_turn(TurnRecord {
            turn_id: "t0".to_owned(),
            query_fingerprint: String::new(),
            injected: vec![InjectedChunk {
                chunk_id: chunk_id.to_owned(),
                path: path.to_owned(),
                score: 0.9,
            }],
            suppressed: vec![],
            skipped: false,
        });
        // Add n subsequent empty turns so the chunk is n turns in the past.
        for i in 0..n {
            ledger.record_turn(TurnRecord {
                turn_id: format!("e{i}"),
                query_fingerprint: String::new(),
                injected: vec![],
                suppressed: vec![],
                skipped: false,
            });
        }
        ledger
    }

    // Verify the decay formula at DEFAULT_DECAY = 0.85, CONFIDENCE_GATE = 0.55:
    //   effective = score × 0.85^turns_since
    //   inject if effective >= 0.55

    #[test]
    fn low_score_chunk_suppressed_by_confidence_gate() {
        // 0.60 passes raw gate (0.60 > 0.55) but after decay: 0.60 × 0.85 = 0.51 < 0.55 → suppressed
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 1);
        let result = apply_suppression(
            vec![candidate("c", "notes/foo.md", 0.60)],
            &ledger,
            DEFAULT_DECAY,
        );
        assert_eq!(result.injected.len(), 0);
        assert_eq!(
            result.suppressed[0].reason,
            SuppressionReason::SameChunkRecentlyInjected
        );
    }

    #[test]
    fn high_score_chunk_passes_one_turn_ago() {
        // 0.85 × 0.85^1 = 0.72 >= 0.40 → high-confidence chunks still eligible after 1 turn
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 1);
        let result = apply_suppression(
            vec![candidate("c", "notes/foo.md", 0.85)],
            &ledger,
            DEFAULT_DECAY,
        );
        assert_eq!(
            result.injected.len(),
            1,
            "score 0.85 should pass after 1 turn with decay 0.85"
        );
    }

    #[test]
    fn moderate_score_suppressed_three_turns_ago() {
        // 0.65 × 0.85^3 = 0.65 × 0.614 = 0.399 < 0.55 → suppressed
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 3);
        let result = apply_suppression(
            vec![candidate("c", "notes/foo.md", 0.65)],
            &ledger,
            DEFAULT_DECAY,
        );
        assert_eq!(result.injected.len(), 0);
    }

    #[test]
    fn high_score_eligible_three_turns_ago() {
        // 0.92 × 0.85^3 = 0.92 × 0.614 = 0.565 >= 0.55 → passes
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 3);
        let result = apply_suppression(
            vec![candidate("c", "notes/foo.md", 0.92)],
            &ledger,
            DEFAULT_DECAY,
        );
        assert_eq!(
            result.injected.len(),
            1,
            "score 0.92 should re-emerge after 3 turns with gate 0.55"
        );
    }

    #[test]
    fn below_confidence_gate_suppressed() {
        let result = apply_suppression(
            vec![candidate("new", "notes/bar.md", 0.2)],
            &TurnLedger::new(),
            DEFAULT_DECAY,
        );
        assert_eq!(result.injected.len(), 0);
        assert_eq!(
            result.suppressed[0].reason,
            SuppressionReason::BelowConfidenceGate
        );
    }

    #[test]
    fn novel_chunk_passes_through() {
        let result = apply_suppression(
            vec![candidate("new", "notes/new.md", 0.85)],
            &TurnLedger::new(),
            DEFAULT_DECAY,
        );
        assert_eq!(result.injected.len(), 1);
        assert!(result.suppressed.is_empty());
    }

    #[test]
    fn all_suppressed_means_empty_injected() {
        // Both chunks have scores that decay below gate after 1 turn (0.46 × 0.85 = 0.391)
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 1);
        let result = apply_suppression(
            vec![
                candidate("c", "notes/foo.md", 0.46),
                candidate("d", "notes/foo.md", 0.46),
            ],
            &ledger,
            DEFAULT_DECAY,
        );
        assert_eq!(
            result.injected.len(),
            0,
            "caller must skip injection, not substitute lower-ranked results"
        );
    }
}
