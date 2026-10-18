//! Configurable risk limits for orders, notional values, and rates.

use honba_core::Quantity;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Rate limit: maximum number of approved orders in an event-time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OrderRateLimit {
    /// Maximum allowed orders in the window.
    pub max_orders: u32,
    /// Window duration in milliseconds.
    pub window_ms: u64,
}

/// Hard risk limits applied synchronously to all outgoing orders and decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct RiskLimits {
    /// Maximum notional value (in INR) permitted per single order.
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
        true
    }
}
