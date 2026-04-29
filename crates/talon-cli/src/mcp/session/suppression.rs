use super::ledger::{InjectedChunk, SuppressedRecall, SuppressionReason, TurnLedger};

const CONFIDENCE_GATE: f64 = 0.4;

/// Minimum score to re-inject a chunk last seen N turns ago.
///
/// The threshold decays with distance: the further back a chunk was last
/// injected, the lower the score needed to inject it again. After enough
/// turns the chunk is treated as novel and only the base confidence gate applies.
const fn chunk_min_score(turns_since: usize) -> f64 {
    match turns_since {
        0 => f64::INFINITY,   // already injected this very turn
        1 => 0.90,            // last turn: very high confidence required
        2 => 0.75,            // two turns ago: high confidence
        3 => 0.60,            // three turns ago: moderate
        _ => CONFIDENCE_GATE, // four+ turns: just the base gate
    }
}

const fn note_min_score(turns_since: usize) -> f64 {
    match turns_since {
        0 => f64::INFINITY,
        1 => 0.85,
        2 => 0.65,
        _ => CONFIDENCE_GATE,
    }
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
/// Suppresses chunks already injected in recent turns and chunks below the
/// confidence gate. Does NOT use query similarity — we deduplicate output
/// context, not input messages.
///
/// Re-injection is allowed after enough turns if the score exceeds the
/// distance-decayed threshold. If all candidates are suppressed, `injected`
/// is empty and the caller must skip injection entirely rather than falling
/// back to lower-ranked results.
#[must_use]
pub fn apply_suppression(
    candidates: Vec<RecallCandidate>,
    ledger: &TurnLedger,
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

        // Chunk-level decay: suppress unless score exceeds the threshold for
        // how long ago this chunk was last injected.
        if ledger
            .turns_since_chunk_last_injected(&candidate.chunk_id)
            .is_some_and(|n| candidate.score < chunk_min_score(n))
        {
            suppressed.push(SuppressedRecall {
                chunk_id: candidate.chunk_id,
                path: candidate.path,
                score: candidate.score,
                reason: SuppressionReason::SameChunkRecentlyInjected,
            });
            continue;
        }

        // Note-level decay: same principle applied to the whole note path.
        if ledger
            .turns_since_note_last_injected(&candidate.path)
            .is_some_and(|n| candidate.score < note_min_score(n))
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
    use super::{RecallCandidate, apply_suppression};
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
        // Add n-1 subsequent empty turns so the chunk is n turns in the past.
        for i in 1..n {
            ledger.record_turn(TurnRecord {
                turn_id: format!("t{i}"),
                query_fingerprint: String::new(),
                injected: vec![],
                suppressed: vec![],
                skipped: false,
            });
        }
        ledger
    }

    #[test]
    fn chunk_suppressed_one_turn_ago_even_with_high_score() {
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 1);
        let result = apply_suppression(vec![candidate("c", "notes/foo.md", 0.89)], &ledger);
        assert_eq!(result.injected.len(), 0);
        assert_eq!(
            result.suppressed[0].reason,
            SuppressionReason::SameChunkRecentlyInjected
        );
    }

    #[test]
    fn chunk_allowed_four_turns_ago_at_moderate_score() {
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 4);
        let result = apply_suppression(vec![candidate("c", "notes/foo.md", 0.55)], &ledger);
        assert_eq!(
            result.injected.len(),
            1,
            "4 turns ago + moderate score should pass"
        );
    }

    #[test]
    fn chunk_suppressed_three_turns_ago_at_low_score() {
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 3);
        let result = apply_suppression(vec![candidate("c", "notes/foo.md", 0.55)], &ledger);
        // 3 turns ago requires score >= 0.60; 0.55 < 0.60 → suppressed.
        assert_eq!(result.injected.len(), 0);
    }

    #[test]
    fn chunk_allowed_three_turns_ago_at_high_score() {
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 3);
        let result = apply_suppression(vec![candidate("c", "notes/foo.md", 0.65)], &ledger);
        // 3 turns ago requires score >= 0.60; 0.65 >= 0.60 → injected.
        assert_eq!(result.injected.len(), 1);
    }

    #[test]
    fn below_confidence_gate_suppressed() {
        let result = apply_suppression(
            vec![candidate("new", "notes/bar.md", 0.2)],
            &TurnLedger::new(),
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
        );
        assert_eq!(result.injected.len(), 1);
        assert!(result.suppressed.is_empty());
    }

    #[test]
    fn all_suppressed_means_empty_injected() {
        let ledger = ledger_with_chunk_n_turns_ago("c", "notes/foo.md", 1);
        let result = apply_suppression(
            vec![
                candidate("c", "notes/foo.md", 0.9),
                candidate("d", "notes/foo.md", 0.8), // same note, 1 turn ago
            ],
            &ledger,
        );
        // Caller must return no additionalContext, not fall back to other results.
        assert_eq!(result.injected.len(), 0);
    }
}
