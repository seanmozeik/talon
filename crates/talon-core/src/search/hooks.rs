use core::fmt;

/// Optional search-pipeline progress callbacks.
#[derive(Default)]
pub struct SearchHooks {
    /// BM25 probe found a decisive match — expansion is being skipped.
    /// Argument is the top probe score that triggered the bypass.
    pub on_strong_signal: Option<Box<dyn Fn(f64) + Send + Sync + 'static>>,
    pub on_expand_start: Option<Box<dyn Fn() + Send + Sync + 'static>>,
    pub on_expand_end: Option<Box<dyn Fn(u64) + Send + Sync + 'static>>,
    pub on_embed_batch: Option<Box<dyn Fn(usize) + Send + Sync + 'static>>,
    pub on_rerank_start: Option<Box<dyn Fn(usize) + Send + Sync + 'static>>,
    pub on_rerank_end: Option<Box<dyn Fn(u64) + Send + Sync + 'static>>,
}

impl fmt::Debug for SearchHooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchHooks")
            .field("on_strong_signal", &self.on_strong_signal.is_some())
            .field("on_expand_start", &self.on_expand_start.is_some())
            .field("on_expand_end", &self.on_expand_end.is_some())
            .field("on_embed_batch", &self.on_embed_batch.is_some())
            .field("on_rerank_start", &self.on_rerank_start.is_some())
            .field("on_rerank_end", &self.on_rerank_end.is_some())
            .finish()
    }
}

impl SearchHooks {
    pub fn emit_strong_signal(&self, top_score: f64) {
        if let Some(cb) = &self.on_strong_signal {
            cb(top_score);
        }
    }

    pub fn emit_expand_start(&self) {
        if let Some(cb) = &self.on_expand_start {
            cb();
        }
    }

    pub fn emit_expand_end(&self, elapsed_ms: u64) {
        if let Some(cb) = &self.on_expand_end {
            cb(elapsed_ms);
        }
    }

    pub fn emit_embed_batch(&self, batch_size: usize) {
        if let Some(cb) = &self.on_embed_batch {
            cb(batch_size);
        }
    }

    pub fn emit_rerank_start(&self, candidate_count: usize) {
        if let Some(cb) = &self.on_rerank_start {
            cb(candidate_count);
        }
    }

    pub fn emit_rerank_end(&self, elapsed_ms: u64) {
        if let Some(cb) = &self.on_rerank_end {
            cb(elapsed_ms);
        }
    }
}
