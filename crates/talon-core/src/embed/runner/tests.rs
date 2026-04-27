use super::*;
use crate::embed::EmbedDiagnostics;

#[test]
fn embed_pass_stats_from_diagnostics_passthrough() {
    let stats: EmbedPassStats = EmbedDiagnostics {
        processed: 3,
        succeeded: 2,
        failed: 1,
        dimension_mismatch: false,
        diagnostics: vec!["a".into(), "b".into()],
    }
    .into();
    assert_eq!(stats.processed, 3);
    assert_eq!(stats.succeeded, 2);
    assert_eq!(stats.failed, 1);
    assert!(stats.remediation.is_none());
    assert_eq!(stats.diagnostics, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn dimension_mismatch_populates_remediation() {
    let stats: EmbedPassStats = EmbedDiagnostics {
        processed: 2,
        succeeded: 1,
        failed: 1,
        dimension_mismatch: true,
        diagnostics: vec!["dim mismatch".into()],
    }
    .into();
    assert!(stats.dimension_mismatch);
    let remediation = stats.remediation.as_deref().unwrap_or("");
    assert!(remediation.contains("--force"));
    assert!(remediation.contains("vec_chunks") || remediation.contains("dimensionality"));
}

#[test]
fn embed_pass_options_defaults_use_sidecar_model_ids() {
    let opts = EmbedPassOptions::defaults();
    assert_eq!(opts.chunk_embedding_model, "embed");
    assert_eq!(opts.document_embedding_model, "embed_chunked");
    assert!(!opts.force);
    assert!(opts.restrict_paths.is_empty());
}
