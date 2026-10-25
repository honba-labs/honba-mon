//! Error definitions for the risk module.

use crate::sizing_gate::SizingRiskRefusal;
use thiserror::Error;

/// Risk gate configuration or runtime error.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RiskError {
    /// Invalid risk limit configuration
    #[error("invalid risk limits: {0}")]
    InvalidConfiguration(String),

    /// Position sizing risk budget refusal
    #[error("position sizing risk refusal: {0}")]
    SizingRefusal(#[from] SizingRiskRefusal),
}
