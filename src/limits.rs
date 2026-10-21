//! Configurable risk limits for orders, notional values, and rates.

use honba_core::{InstrumentId, Quantity};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Rate limit: maximum number of approved orders in an event-time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OrderRateLimit {
    /// Maximum allowed orders in the window.
    pub max_orders: u32,
    /// Window duration in milliseconds.
    pub window_ms: u64,
}

/// Hard risk limits applied synchronously to all outgoing orders and decisions.
///
/// This struct merges rules from `honba-kokoro-risk::RiskLimits` (deprecated)
/// with the canonical honba-mon risk limit schema.
///
/// Legacy fields from `honba-kokoro-risk` (mapped during Phase 1.2):
/// - `max_notional` (f64, per-order notional ceiling) → `max_notional_per_order` (Decimal)
/// - `max_participation` (f64, fraction of ADV) → `max_quantity_per_order` (Quantity)
/// - `order_rate` (OrderRateLimit) → `order_rate_limit` (OrderRateLimit)
/// - `stale_after_ms` (u64, feed staleness) → tracked via separate circuit breaker
///
/// New honba-mon fields:
/// - `max_daily_loss` (Decimal, daily cumulative loss limit)
/// - `price_collar_bps` (u32, market order deviation collar in basis points)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct RiskLimits {
    /// Maximum notional value (in INR) permitted per single order.
    /// `None` = no notional rule; `Some` must be finite and `> 0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_notional_per_order: Option<Decimal>,

    /// Maximum quantity permitted per single order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_quantity_per_order: Option<Quantity>,

    /// Maximum allowable daily cumulative loss (in INR).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_daily_loss: Option<Decimal>,

    /// Global order rate limit across the platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_rate_limit: Option<OrderRateLimit>,

    /// Allowed market order deviation collar in basis points (1 bp = 0.01%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_collar_bps: Option<u32>,

    /// Per-scrip freeze limits (exchange single-order quantity ceilings).
    /// Maps instrument to maximum allowed quantity per single order.
    /// Examples: NIFTY → 1,800, BANKNIFTY → 900.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub freeze_limits: HashMap<InstrumentId, Quantity>,
}

impl RiskLimits {
    /// Validate structural limits sanity.
    pub fn is_valid(&self) -> bool {
        if let Some(notional) = self.max_notional_per_order {
            if notional <= Decimal::ZERO {
                return false;
            }
        }
        if let Some(rate) = self.order_rate_limit {
            if rate.max_orders == 0 || rate.window_ms == 0 {
                return false;
            }
        }
        if let Some(collar) = self.price_collar_bps {
            if collar == 0 {
                return false;
            }
        }
        for (_, qty) in &self.freeze_limits {
            if !qty.is_positive() {
                return false;
            }
        }
        true
    }
}