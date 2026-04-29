//! Accessor impls for `TalonResponseData` and `TalonResponseTrait`.

use crate::indexing::{LintResponse, StatusResponse, SyncResponse};
use crate::query::{AskResponse, ChangesResponse, MetaResponse, ReadResponse, RelatedResponse};
use crate::search::SearchResponse;

use super::{TalonResponseData, TalonResponseTrait};

impl TalonResponseTrait for TalonResponseData {
    fn action(&self) -> &str {
        match self {
            Self::Search(_) => "search",
            Self::Ask(_) => "ask",
            Self::Read(_) => "read",
            Self::Sync(_) => "sync",
            Self::Status(_) => "status",
            Self::Related(_) => "related",
            Self::Meta(_) => "meta",
            Self::Changes(_) => "changes",
            Self::Lint(_) => "lint",
            Self::Recall(_) => "recall",
        }
    }
}

// ── Response inner-type accessor impls ──────────────────────────────────────

impl TalonResponseData {
    /// Returns a reference to the inner `SearchResponse`, if present.
    #[must_use]
    pub const fn as_search(&self) -> Option<&SearchResponse> {
        match self {
            Self::Search(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `AskResponse`, if present.
    #[must_use]
    pub const fn as_ask(&self) -> Option<&AskResponse> {
        match self {
            Self::Ask(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `SyncResponse`, if present.
    #[must_use]
    pub const fn as_sync(&self) -> Option<&SyncResponse> {
        match self {
            Self::Sync(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `StatusResponse`, if present.
    #[must_use]
    pub const fn as_status(&self) -> Option<&StatusResponse> {
        match self {
            Self::Status(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `RelatedResponse`, if present.
    #[must_use]
    pub const fn as_related(&self) -> Option<&RelatedResponse> {
        match self {
            Self::Related(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `MetaResponse`, if present.
    #[must_use]
    pub const fn as_meta(&self) -> Option<&MetaResponse> {
        match self {
            Self::Meta(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `ChangesResponse`, if present.
    #[must_use]
    pub const fn as_changes(&self) -> Option<&ChangesResponse> {
        match self {
            Self::Changes(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `LintResponse`, if present.
    #[must_use]
    pub const fn as_lint(&self) -> Option<&LintResponse> {
        match self {
            Self::Lint(r) => Some(r),
            _ => None,
        }
    }

    /// Returns a reference to the inner `ReadResponse`, if present.
    #[must_use]
    pub const fn as_read(&self) -> Option<&ReadResponse> {
        match self {
            Self::Read(r) => Some(r),
            _ => None,
        }
    }
}
