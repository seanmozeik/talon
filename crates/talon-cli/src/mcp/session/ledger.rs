use std::collections::{HashMap, VecDeque};

pub const MAX_TURNS: usize = 32;

#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub turn_id: String,
    pub query_fingerprint: String,
    pub injected: Vec<InjectedChunk>,
    pub suppressed: Vec<SuppressedRecall>,
    pub skipped: bool,
}

#[derive(Debug, Clone)]
pub struct InjectedChunk {
    /// Deterministic chunk ID — see `chunk_id.rs` for how this is derived.
    pub chunk_id: String,
    pub path: String,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct SuppressedRecall {
    pub chunk_id: String,
    pub path: String,
    pub score: f64,
    pub reason: SuppressionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionReason {
    SameChunkRecentlyInjected,
    SameNoteRecentlyInjected,
    QueryRepeated,
    BelowConfidenceGate,
    BudgetTrimmed,
}

#[derive(Debug, Clone)]
pub enum InjectionReason {
    Novel,
    QueryDrift,
}

/// Bounded turn history.
#[derive(Debug)]
pub struct TurnLedger {
    turns: VecDeque<TurnRecord>,
    /// `chunk_id` → list of `turn_id`s where it was injected.
    injected_chunks: HashMap<String, Vec<String>>,
    /// `path` → list of `turn_id`s where it was injected.
    injected_notes: HashMap<String, Vec<String>>,
}

impl Default for TurnLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnLedger {
    #[must_use]
    pub fn new() -> Self {
        Self {
            turns: VecDeque::with_capacity(MAX_TURNS),
            injected_chunks: HashMap::new(),
            injected_notes: HashMap::new(),
        }
    }

    /// Record a completed turn with injected and suppressed chunks.
    pub fn record_turn(&mut self, record: TurnRecord) {
        // If we're at capacity, remove the oldest turn and clean up its chunk/note refs.
        if self.turns.len() >= MAX_TURNS
            && let Some(evicted) = self.turns.pop_front()
        {
            self.remove_turn_refs(&evicted.turn_id);
        }

        for chunk in &record.injected {
            self.injected_chunks
                .entry(chunk.chunk_id.clone())
                .or_default()
                .push(record.turn_id.clone());
            self.injected_notes
                .entry(chunk.path.clone())
                .or_default()
                .push(record.turn_id.clone());
        }
        self.turns.push_back(record);
    }

    /// Removes `turn_id` references from chunk/note maps when a turn is evicted.
    fn remove_turn_refs(&mut self, turn_id: &str) {
        for ids in self.injected_chunks.values_mut() {
            ids.retain(|id| id != turn_id);
        }
        for ids in self.injected_notes.values_mut() {
            ids.retain(|id| id != turn_id);
        }
    }

    /// Returns how many of the last N turns contained this `chunk_id`.
    #[must_use]
    pub fn chunk_injected_in_last_n(&self, chunk_id: &str, n: usize) -> usize {
        let recent_ids: Vec<&str> = self
            .turns
            .iter()
            .rev()
            .take(n)
            .map(|t| t.turn_id.as_str())
            .collect();
        self.injected_chunks.get(chunk_id).map_or(0, |ids| {
            ids.iter()
                .filter(|id| recent_ids.contains(&id.as_str()))
                .count()
        })
    }

    /// Returns how many of the last N turns contained this note `path`.
    #[must_use]
    pub fn note_injected_in_last_n(&self, path: &str, n: usize) -> usize {
        let recent_ids: Vec<&str> = self
            .turns
            .iter()
            .rev()
            .take(n)
            .map(|t| t.turn_id.as_str())
            .collect();
        self.injected_notes.get(path).map_or(0, |ids| {
            ids.iter()
                .filter(|id| recent_ids.contains(&id.as_str()))
                .count()
        })
    }

    /// Returns the fingerprint of the most recent turn, if any.
    #[must_use]
    pub fn last_fingerprint(&self) -> Option<&str> {
        self.turns.back().map(|t| t.query_fingerprint.as_str())
    }
}
