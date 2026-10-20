//! Refusal types indicating why an order or decision was rejected by the risk gate.

use chrono::{DateTime, Utc};
use honba_core::{Price, PriceBand, Quantity};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Typed refusal reason from the risk gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", content = "details", rename_all = "snake_case")]
pub enum RiskRefusal {
    /// Platform is under global halt (kill switch active).
    GlobalHalted {
        /// Explanation for the platform halt
        reason: String,
    },

    /// The specific algorithm ID is halted.
    AlgoHalted {
        /// Algo ID
        algo_id: String,
        /// Reason for halting
        reason: String,
    },

    /// Market-wide index circuit breaker triggered (10%, 15%, 20%).
    MarketCircuitHalt {
        /// Index symbol (e.g. "NIFTY 50", "SENSEX")
        index_symbol: String,
        /// Circuit percentage stage (10, 15, or 20)
        stage_pct: u8,
        /// Expected cooling-off resume time if applicable
        resume_at: Option<DateTime<Utc>>,
    },

    /// SEBI Order-to-Trade Ratio limit exceeded.
    OtrBreached {
        /// Breaching algo ID
        algo_id: String,
        /// Current calculated OTR
        current_otr: Decimal,
        /// Configured ceiling
        max_otr: Decimal,
    },

    /// Order price violates the scrip-wise circuit price band.
    PriceBandViolation {
        /// Order price requested
        price: Price,
        /// Scrip price band bounds
        band: PriceBand,
    },

    /// Single order notional value exceeds configured limit.
    MaxNotionalExceeded {
        /// Requested order notional
        requested: Decimal,
        /// Configured limit
        limit: Decimal,
    },

    /// Order quantity exceeds configured limit.
    MaxQuantityExceeded {
        /// Requested quantity
        requested: Quantity,
        /// Configured limit
        limit: Quantity,
    },

    /// Order rate limit exceeded in the current event window.
    OrderRateExceeded {
        /// Number of orders approved in window
        count: u32,
        /// Maximum allowed
        limit: u32,
        /// Window length in milliseconds
        window_ms: u64,
    },

    /// Cumulative daily loss threshold exceeded.
    DailyLossExceeded {
        /// Realized daily loss
        current_loss: Decimal,
        /// Maximum allowable loss
        limit: Decimal,
    },

    /// Platform is in reduce-only mode and order increases risk.
    ReduceOnlyViolation {
        /// Details
        details: String,
    },

    /// Order failed structural or tick validation.
    InvalidOrder {
        /// Description of failure
        error: String,
    },
}

impl fmt::Display for RiskRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlobalHalted { reason } => {
                write!(f, "Platform trading globally halted: {reason}")
            }
            Self::AlgoHalted { algo_id, reason } => {
                write!(f, "Algorithm '{algo_id}' is halted: {reason}")
            }
            Self::MarketCircuitHalt {
                index_symbol,
                stage_pct,
                resume_at,
            } => match resume_at {
                Some(res) => write!(
                    f,
                    "Market-wide circuit halt on {index_symbol} ({stage_pct}%); resumes at {res}"
                ),
                None => write!(
                    f,
                    "Market-wide circuit halt on {index_symbol} ({stage_pct}%); market closed for remainder of day"
                ),
            },
            Self::OtrBreached {
                algo_id,
                current_otr,
                max_otr,
            } => write!(
                f,
                "SEBI OTR threshold breached for algo '{algo_id}': current {current_otr} > limit {max_otr}"
            ),
            Self::PriceBandViolation { price, band } => write!(
                f,
                "Price band violation: order price {} outside circuit bounds [{}, {}]",
                price, band.lower, band.upper
            ),
            Self::MaxNotionalExceeded { requested, limit } => write!(
                f,
                "Order notional {} INR exceeds ceiling {} INR",
                requested, limit
            ),
            Self::MaxQuantityExceeded { requested, limit } => {
                write!(f, "Order quantity {} exceeds ceiling {}", requested, limit)
            }
            Self::OrderRateExceeded {
                count,
                limit,
                window_ms,
            } => write!(
                f,
                "Rate limit exceeded: {count} orders in {window_ms}ms (max {limit})"
            ),
            Self::DailyLossExceeded {
                current_loss,
                limit,
            } => write!(
                f,
                "Daily loss limit reached: -{} INR exceeds max loss -{} INR",
                current_loss, limit
            ),
            Self::ReduceOnlyViolation { details } => {
                write!(f, "Reduce-only mode violation: {details}")
            }
            Self::InvalidOrder { error } => write!(f, "Invalid order: {error}"),
        }
    }
}

impl std::error::Error for RiskRefusal {}

impl RiskRefusal {
    /// Return the regulatory/risk rule name identifying this refusal.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::GlobalHalted { .. } => "global_halt",
            Self::AlgoHalted { .. } => "algo_halt",
            Self::MarketCircuitHalt { .. } => "market_circuit_halt",
            Self::OtrBreached { .. } => "sebi_otr_breach",
            Self::PriceBandViolation { .. } => "price_band_violation",
            Self::MaxNotionalExceeded { .. } => "max_notional_exceeded",
            Self::MaxQuantityExceeded { .. } => "max_quantity_exceeded",
            Self::OrderRateExceeded { .. } => "order_rate_exceeded",
            Self::DailyLossExceeded { .. } => "daily_loss_exceeded",
            Self::ReduceOnlyViolation { .. } => "reduce_only_violation",
            Self::InvalidOrder { .. } => "invalid_order",
        }
    }
}
