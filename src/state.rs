//! Trading states for global platform and individual algorithm IDs.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Global platform trading state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GlobalState {
    /// Normal operation: all trading active.
    #[default]
    Active,
    /// Reduce-only: existing positions can be closed/reduced, no new risk.
    ReduceOnly,
    /// Global emergency halt: all order placements blocked across all algos.
    Halted {
        /// Reason for platform halt
        reason: String,
        /// Timestamp when halt took effect
        halted_at: DateTime<Utc>,
    },
}

impl GlobalState {
    /// Returns true if new orders can be accepted.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns true if halted.
    pub fn is_halted(&self) -> bool {
        matches!(self, Self::Halted { .. })
    }
}

/// State for an individual algo ID.
///
/// Mandated by SEBI Retail Algo circular (brokers must be able to automatically
/// halt orders from a specific algo ID).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AlgoState {
    /// Algo is running normally.
    #[default]
    Active,
    /// Algo has been halted (due to OTR breach, daily loss, or manual kill-switch).
    Halted {
        /// Explanation for the halt
        reason: String,
        /// Timestamp when the algo was halted
        halted_at: DateTime<Utc>,
    },
}

impl AlgoState {
    /// Returns true if the algo is active.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns true if the algo is halted.
    pub fn is_halted(&self) -> bool {
        matches!(self, Self::Halted { .. })
    }
}
