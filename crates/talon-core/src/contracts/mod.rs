//! MCP and CLI tool contracts — shared envelope, path, and primitive types.

mod accessors;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod envelope_tests;

use serde::{Deserialize, Serialize};

use crate::constants::DEFAULT_LIMIT;
use crate::error::{ErrorCode, TalonError, TalonResult};

// ── Positive count ──────────────────────────────────────────────────────────

/// A positive count accepted at the tool boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct PositiveCount(u16);

impl Default for PositiveCount {
    fn default() -> Self {
        Self(DEFAULT_LIMIT)
    }
}

impl PositiveCount {
    /// Builds a positive count.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when `value` is zero.
    pub fn new(value: u16, field: &'static str) -> TalonResult<Self> {
        if value == 0 {
            return Err(TalonError::InvalidInput {
                field,
                message: "must be greater than zero".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the primitive count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Constructs directly from a known-valid constant (bypasses the zero-check).
    pub(crate) const fn from_const(value: u16) -> Self {
        Self(value)
    }
}

impl TryFrom<u16> for PositiveCount {
    type Error = TalonError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value, "count")
    }
}

impl From<PositiveCount> for u16 {
    fn from(value: PositiveCount) -> Self {
        value.0
    }
}

// ── Path types ──────────────────────────────────────────────────────────────

/// Vault-relative path returned by Talon.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultPath(String);

impl VaultPath {
    /// Parses a non-empty vault-relative path.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when the path is empty.
    pub fn parse(value: impl Into<String>) -> TalonResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TalonError::InvalidInput {
                field: "path",
                message: "must not be empty".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Container-absolute path used when a tool needs absolute addressing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContainerPath(String);

impl ContainerPath {
    /// Parses a non-empty container path.
    ///
    /// # Errors
    ///
    /// Returns [`TalonError::InvalidInput`] when the path is empty.
    pub fn parse(value: impl Into<String>) -> TalonResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TalonError::InvalidInput {
                field: "path",
                message: "must not be empty".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns a root (`/`) container path. Infallible alternative to `parse("/")`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_string())
    }

    /// Returns the path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── Response metadata ───────────────────────────────────────────────────────

/// Metadata included in every successful response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMeta {
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Number of results returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_count: Option<u32>,
    /// Warnings produced during the call.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Resolved active scope set, where applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_set: Option<Vec<String>>,
    /// Resolved absolute timestamp, if `--since` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// Error envelope used when `ok: false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope {
    /// Error code from the fixed enum.
    pub code: ErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

// ── Response data envelope ──────────────────────────────────────────────────

use crate::indexing::{InspectResponse, StatusResponse, SyncResponse};
use crate::query::{ChangesResponse, MetaResponse, ReadResponse, RecallResponse, RelatedResponse};
use crate::search::SearchResponse;

/// Action-discriminated response payload, serialized inside `TalonEnvelope.data`.
///
/// When serialized, produces `{ action: "<action>", ...fields }` — the action
/// discriminator is redundant with the envelope's top-level `action` but kept
/// for forward-compatibility with MCP tool call results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum TalonResponseData {
    /// Search response.
    Search(SearchResponse),
    /// Vault-grounded natural-language answer response.
    Ask(crate::query::AskResponse),
    /// Read response.
    Read(ReadResponse),
    /// Sync response.
    Sync(SyncResponse),
    /// Status response.
    Status(StatusResponse),
    /// Related-note response.
    Related(RelatedResponse),
    /// Frontmatter query response.
    Meta(MetaResponse),
    /// Change feed response.
    Changes(ChangesResponse),
    /// Inspect check response.
    Inspect(InspectResponse),
    /// Vault-native context recall response.
    Recall(RecallResponse),
}

/// Unified output envelope for all Talon responses.
///
/// Every JSON response uses this shape:
/// - Success: `{ action, version, ok: true, data: ..., meta: ... }`
/// - Error:  `{ action, version, ok: false, error: { code, message, detail } }`
///
/// See Decision 8 in the design spec for the locked contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TalonEnvelope {
    /// Action name in kebab-case (e.g. "search", "sync", "status").
    pub action: String,
    /// Cargo package version at build time.
    pub version: String,
    /// Whether the call succeeded.
    pub ok: bool,
    /// Action-discriminated payload (present when `ok: true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<TalonResponseData>,
    /// Metadata (present when `ok: true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
    /// Error envelope (present when `ok: false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorEnvelope>,
}

impl TalonEnvelope {
    /// Builds a success envelope.
    #[must_use]
    pub fn ok(action: &'static str, data: TalonResponseData, meta: ResponseMeta) -> Self {
        Self {
            action: action.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ok: true,
            data: Some(data),
            meta: Some(meta),
            error: None,
        }
    }

    /// Builds an error envelope.
    #[must_use]
    pub fn err(action: &str, error: ErrorEnvelope) -> Self {
        Self {
            action: action.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ok: false,
            data: None,
            meta: None,
            error: Some(error),
        }
    }

    /// Returns the inner response data, if present.
    #[must_use]
    pub const fn data(&self) -> Option<&TalonResponseData> {
        self.data.as_ref()
    }

    /// Returns the inner response data, if present and mutable.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn data_mut(&mut self) -> Option<&mut TalonResponseData> {
        self.data.as_mut()
    }

    /// Extracts the inner response data.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn into_data(self) -> Option<TalonResponseData> {
        self.data
    }

    /// Returns the human-readable response for this envelope.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn as_response(&self) -> Option<&dyn TalonResponseTrait> {
        self.data.as_ref().map(|d| d as &dyn TalonResponseTrait)
    }
}

/// Trait for accessing response data from `TalonResponseData`.
///
/// Implemented by each response variant so output formatters can match on
/// the inner type without knowing the enum discriminant.
pub trait TalonResponseTrait {
    /// Returns the action name.
    fn action(&self) -> &str;
}

// ── Tool input envelope ─────────────────────────────────────────────────────

use crate::indexing::{InspectInput, StatusInput, SyncInput};
use crate::query::{ChangesInput, MetaInput, ReadInput, RecallInput, RelatedInput};
use crate::search::SearchInput;

/// Tool input envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum TalonInput {
    /// Search request.
    Search(SearchInput),
    /// Read request.
    Read(ReadInput),
    /// Sync/index request.
    Sync(SyncInput),
    /// Status request.
    Status(StatusInput),
    /// Related-note request.
    Related(RelatedInput),
    /// Frontmatter query request.
    Meta(MetaInput),
    /// Change feed request.
    Changes(ChangesInput),
    /// Inspect check request.
    Inspect(InspectInput),
    /// Vault-native context recall request.
    Recall(RecallInput),
}
