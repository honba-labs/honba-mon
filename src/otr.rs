//! SEBI Order-to-Trade Ratio (OTR) tracker and per-algo circuit breaker.
//!
//! Under SEBI regulatory framework (and Retail Algo Circular effective August 1, 2025),
//! algorithmic orders must maintain acceptable OTR levels to avoid exchange penalties
//! and regulatory intervention.

use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Configuration for OTR enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OtrConfig {
    /// Maximum allowed Order-to-Trade Ratio (e.g., 50.0).
    pub max_otr: Decimal,
    /// Minimum order operations before OTR limits are strictly enforced.
    /// Prevents premature halting on the very first few orders.
    pub min_orders_before_enforcement: u64,
}

impl Default for OtrConfig {
    fn default() -> Self {
        Self {
            // Default 50:1 ratio
            max_otr: Decimal::from(50),
            // Grace period of first 20 orders before hard clamping
            min_orders_before_enforcement: 20,
        }
    }
}

/// Statistics and real-time state for an algorithm's OTR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct OtrTracker {
    /// Total new orders placed by this algo
    pub orders_placed: u64,
    /// Total order modifications requested
    pub orders_modified: u64,
    /// Total cancellations requested
    pub orders_cancelled: u64,
    /// Total executed trades / fills
    pub trades_executed: u64,
}

impl OtrTracker {
    /// Create a new OTR tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a newly placed order.
    pub fn record_order_placement(&mut self) {
        self.orders_placed += 1;
    }

    /// Record an order modification or cancellation.
    pub fn record_order_mod_or_cancel(&mut self) {
        self.orders_cancelled += 1;
    }

    /// Record an executed trade / fill.
    pub fn record_fill(&mut self) {
        self.trades_executed += 1;
    }

    /// Total order operations (placements + modifications + cancellations).
    pub fn total_order_operations(&self) -> u64 {
        self.orders_placed + self.orders_modified + self.orders_cancelled
    }

    /// Current real-time OTR ratio as Decimal.
    pub fn current_otr(&self) -> Decimal {
        let ops = Decimal::from(self.total_order_operations());
        let trades = Decimal::from(self.trades_executed.max(1));
        ops / trades
    }

    /// Check if this algo has breached the OTR threshold.
    pub fn is_breached(&self, config: &OtrConfig) -> bool {
        if self.total_order_operations() < config.min_orders_before_enforcement {
            return false;
        }
        self.current_otr() > config.max_otr
    }
}
