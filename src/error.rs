//! Error definitions for the risk module.

use thiserror::Error;

/// Risk gate configuration or runtime error.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RiskError {
    /// Invalid risk limit configuration
    #[error("invalid risk limits: {0}")]
    InvalidConfiguration(String),
}
