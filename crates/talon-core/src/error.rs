//! Error types for Talon core operations.

use thiserror::Error;

/// Result alias for Talon core operations.
pub type TalonResult<T> = Result<T, TalonError>;

/// Typed failures produced by Talon core operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TalonError {
    /// A feature surface exists but is intentionally not implemented yet.
    #[error("{feature} is not implemented yet")]
    NotImplemented {
        /// Feature name.
        feature: &'static str,
    },

    /// User input failed boundary validation.
    #[error("invalid input for {field}: {message}")]
    InvalidInput {
        /// Input field name.
        field: &'static str,
        /// Validation failure detail.
        message: String,
    },
}
