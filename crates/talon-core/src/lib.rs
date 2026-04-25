//! Core types and contracts for Talon.
//!
//! The scaffold keeps parsing, configuration, constants, and response contracts
//! in the library so the CLI remains a thin process boundary.

pub mod config;
pub mod constants;
pub mod error;
pub mod tool;

pub use config::{ExpansionConfig, InferenceConfig, InferenceModels, TalonConfig};
pub use error::{TalonError, TalonResult};
pub use tool::{
    ContainerPath, Direction, FrontmatterFilter, IndexStats, MatchKind, PositiveCount, ReadInput,
    ReadResponse, SearchInput, SearchMode, SearchResponse, SearchResult, StatusResponse,
    StatusState, SyncInput, SyncResponse, TalonInput, TalonResponse, VaultPath,
};
