//! LLM planning and synthesis for vault-grounded answers.

mod client;
mod error;
mod types;

pub use client::{AskClient, AskPlan, AskSynthesis};
pub use error::AskError;
pub use types::AskPlanBody;
