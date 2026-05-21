//! Error types for Talon core operations.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result alias for Talon core operations.
pub type TalonResult<T> = Result<T, TalonError>;

/// Error codes used in the MCP and CLI output envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// Invalid scope name in config.
    InvalidScope,
    /// Invalid `--where` filter expression.
    InvalidWhere,
    /// Invalid `--since` timestamp.
    InvalidSince,
    /// Database busy (sync lock held).
    DbBusy,
    /// Database corruption detected.
    DbCorrupt,
    /// No index exists for the configured vault.
    NotIndexed,
    /// Internal/server error.
    Internal,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScope => write!(f, "invalid-scope"),
            Self::InvalidWhere => write!(f, "invalid-where"),
            Self::InvalidSince => write!(f, "invalid-since"),
            Self::DbBusy => write!(f, "db-busy"),
            Self::DbCorrupt => write!(f, "db-corrupt"),
            Self::NotIndexed => write!(f, "not-indexed"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

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

    /// Scope name not found in configuration.
    #[error("scope '{name}' is not declared in config")]
    InvalidScope {
        /// Invalid scope name.
        name: String,
    },

    /// `--where` filter expression is malformed.
    #[error("invalid --where filter: {message}")]
    InvalidWhere {
        /// Filter expression detail.
        message: String,
    },

    /// `--since` timestamp could not be parsed.
    #[error("invalid --since timestamp: {message}")]
    InvalidSince {
        /// Timestamp detail.
        message: String,
    },

    /// Database is locked by another sync operation.
    #[error("database busy: another sync is in progress")]
    DbBusy,

    /// Database file is corrupt or unreadable.
    #[error("database corrupt: {message}")]
    DbCorrupt {
        /// Corruption detail.
        message: String,
    },

    /// No index exists for the configured vault path.
    #[error("not indexed: vault '{path}' has no index")]
    NotIndexed {
        /// Vault path.
        path: String,
    },

    /// Configuration is invalid or incomplete.
    #[error("config error: {message}")]
    Config {
        /// Configuration detail.
        message: String,
    },

    /// Internal/server error.
    #[error("internal error: {message}")]
    Internal {
        /// Error detail.
        message: String,
    },

    /// `SQLite` operation failed.
    #[error("sqlite error in {context}: {source}")]
    Sqlite {
        /// Where the error occurred (e.g. "open database", "run migrations").
        context: &'static str,
        /// Underlying `rusqlite` error.
        #[source]
        source: rusqlite::Error,
    },
}

impl TalonError {
    /// Returns the error code for this error variant.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidScope { .. } => ErrorCode::InvalidScope,
            Self::InvalidWhere { .. } => ErrorCode::InvalidWhere,
            Self::InvalidSince { .. } => ErrorCode::InvalidSince,
            Self::DbBusy => ErrorCode::DbBusy,
            Self::DbCorrupt { .. } => ErrorCode::DbCorrupt,
            Self::NotIndexed { .. } => ErrorCode::NotIndexed,
            Self::Internal { .. }
            | Self::NotImplemented { .. }
            | Self::InvalidInput { .. }
            | Self::Config { .. }
            | Self::Sqlite { .. } => ErrorCode::Internal,
        }
    }
}
