//! SEBI Order-to-Trade Ratio (OTR) tracker and per-algo circuit breaker.
//!
//! Under SEBI regulatory framework (and Retail Algo Circular effective August 1, 2025),
//! algorithmic orders must maintain acceptable OTR levels to avoid exchange penalties
//! and regulatory intervention.
//!
//! The tracker supports both cumulative totals (for backward compatibility) and
//! sliding-window OTR computation over 1-minute, 5-minute, and session windows
//! using a circular ring buffer of event timestamps.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Configuration for OTR enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OtrConfig {
    /// Maximum allowed Order-to-Trade Ratio (e.g., 50.0).
    pub max_otr: Decimal,
    /// Minimum order operations before OTR limits are strictly enforced.
    /// Prevents premature halting on the very first few orders.
    pub min_orders_before_enforcement: u64,
    /// Soft warning threshold ratio. When OTR exceeds this but is below
    /// `max_otr`, a warning is emitted but the algo is not halted.
    pub warning_threshold: Decimal,
    /// Sliding window duration in milliseconds for short-term OTR (default 1 minute).
    pub window_1min_ms: u64,
    /// Sliding window duration in milliseconds for medium-term OTR (default 5 minutes).
    pub window_5min_ms: u64,

    /// Cooling-off period in milliseconds after an OTR breach halt before
    /// automatic resumption is attempted. `0` means no auto-resume.
    pub halt_cooldown_ms: u64,
}

impl Default for OtrConfig {
    fn default() -> Self {
        Self {
            // Default 50:1 ratio
            max_otr: Decimal::from(50),
            // Grace period of first 20 orders before hard clamping
            min_orders_before_enforcement: 20,
            // Soft warning at 80% of the max ratio
            warning_threshold: Decimal::from(40),
            // 1 minute window
            window_1min_ms: 60_000,
            // 5 minute window
            window_5min_ms: 300_000,
            // No auto-resume by default
            halt_cooldown_ms: 0,
        }
    }
}

/// A timestamped order event for sliding-window OTR computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderEvent {
    /// Event timestamp.
    pub ts: DateTime<Utc>,
    /// Event type: placement, modification/cancellation, or fill.
    pub kind: OrderEventKind,
}

/// Type of order event tracked for OTR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderEventKind {
    /// New order placed.
    Placement,
    /// Order modified or cancelled.
    ModOrCancel,
    /// Trade executed / fill received.
    Fill,
}

/// Statistics and real-time state for an algorithm's OTR.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct OtrTracker {
    /// Total new orders placed by this algo (cumulative).
    pub orders_placed: u64,
    /// Total order modifications requested (cumulative).
    pub orders_modified: u64,
    /// Total cancellations requested (cumulative).
    pub orders_cancelled: u64,
    /// Total executed trades / fills (cumulative).
    pub trades_executed: u64,
    /// Ring buffer of recent order events for sliding-window computation.
    #[serde(skip)]
    events: VecDeque<OrderEvent>,
}

impl OtrTracker {
    /// Create a new OTR tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a newly placed order.
    pub fn record_order_placement(&mut self, ts: DateTime<Utc>) {
        self.orders_placed += 1;
        self.events.push_back(OrderEvent {
            ts,
            kind: OrderEventKind::Placement,
        });
    }

    /// Record an order modification or cancellation.
    pub fn record_order_mod_or_cancel(&mut self, ts: DateTime<Utc>) {
        self.orders_cancelled += 1;
        self.events.push_back(OrderEvent {
            ts,
            kind: OrderEventKind::ModOrCancel,
        });
    }

    /// Record an executed trade / fill.
    pub fn record_fill(&mut self, ts: DateTime<Utc>) {
        self.trades_executed += 1;
        self.events.push_back(OrderEvent {
            ts,
            kind: OrderEventKind::Fill,
        });
    }

    /// Total order operations (placements + modifications + cancellations).
    pub fn total_order_operations(&self) -> u64 {
        self.orders_placed + self.orders_modified + self.orders_cancelled
    }

    /// Current real-time OTR ratio as Decimal (cumulative).
    pub fn current_otr(&self) -> Decimal {
        let ops = Decimal::from(self.total_order_operations());
        let trades = Decimal::from(self.trades_executed.max(1));
        ops / trades
    }

    /// Compute sliding-window OTR over the given window duration.
    ///
    /// Evicts events older than `window_ms` from the ring buffer and computes
    /// the OTR ratio over events within the window.
    pub fn window_otr(&mut self, now: DateTime<Utc>, window_ms: u64) -> Decimal {
        let cutoff = now - chrono::Duration::milliseconds(window_ms as i64);

        // Evict expired events from the front of the deque
        while let Some(front) = self.events.front() {
            if front.ts < cutoff {
                self.events.pop_front();
            } else {
                break;
            }
        }

        let ops_in_window: u64 = self
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    OrderEventKind::Placement | OrderEventKind::ModOrCancel
                )
            })
            .count() as u64;
        let trades_in_window: u64 = self
            .events
            .iter()
            .filter(|e| matches!(e.kind, OrderEventKind::Fill))
            .count() as u64;

        let ops = Decimal::from(ops_in_window);
        let trades = Decimal::from(trades_in_window.max(1));
        ops / trades
    }

    /// OTR over the 1-minute sliding window.
    pub fn window_otr_1min(&mut self, now: DateTime<Utc>) -> Decimal {
        self.window_otr(now, 60_000)
    }

    /// OTR over the 5-minute sliding window.
    pub fn window_otr_5min(&mut self, now: DateTime<Utc>) -> Decimal {
        self.window_otr(now, 300_000)
    }

    /// Check if this algo has breached the OTR threshold (cumulative).
    pub fn is_breached(&self, config: &OtrConfig) -> bool {
        if self.total_order_operations() < config.min_orders_before_enforcement {
            return false;
        }
        self.current_otr() > config.max_otr
    }

    /// Check if this algo is in the warning zone (OTR above warning threshold
    /// but below max). Returns `Some(warning_ratio)` if in warning zone.
    pub fn warning_ratio(&self, config: &OtrConfig) -> Option<Decimal> {
        let otr = self.current_otr();
        if otr > config.warning_threshold && otr <= config.max_otr {
            Some(otr)
        } else {
            None
        }
    }

    /// Returns the number of events currently in the ring buffer.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_cumulative_otr() {
        let mut tracker = OtrTracker::new();
        let now = Utc::now();

        // Place 10 orders, 1 fill -> OTR = 10
        for _ in 0..10 {
            tracker.record_order_placement(now);
        }
        tracker.record_fill(now);

        assert_eq!(tracker.total_order_operations(), 10);
        assert_eq!(tracker.current_otr(), Decimal::from(10));
    }

    #[test]
    fn test_sliding_window_otr() {
        let mut tracker = OtrTracker::new();
        let base = Utc.with_ymd_and_hms(2025, 8, 1, 10, 0, 0).unwrap();

        // Place 5 orders at t=0
        for _ in 0..5 {
            tracker.record_order_placement(base);
        }
        // 1 fill at t=0
        tracker.record_fill(base);

        // 5 more orders at t=55 seconds (within 1-minute window)
        let t55 = base + chrono::Duration::seconds(55);
        for _ in 0..5 {
            tracker.record_order_placement(t55);
        }

        // At t=55s, 1-min window should show 10 ops, 1 trade = OTR 10
        let otr = tracker.window_otr_1min(t55);
        assert_eq!(otr, Decimal::from(10));

        // At t=61 seconds, the t=0 events fall out of the 1-minute window
        let t61 = base + chrono::Duration::seconds(61);
        let otr = tracker.window_otr_1min(t61);
        // Only the 5 orders at t=55s remain in window, no trades = OTR 5
        assert_eq!(otr, Decimal::from(5));
    }

    #[test]
    fn test_is_breached_grace_period() {
        let config = OtrConfig {
            max_otr: Decimal::from(10),
            min_orders_before_enforcement: 5,
            ..Default::default()
        };

        let mut tracker = OtrTracker::new();
        let now = Utc::now();

        // Place 3 orders, 0 fills (below grace period)
        for _ in 0..3 {
            tracker.record_order_placement(now);
        }
        assert!(!tracker.is_breached(&config));

        // Place 2 more (total 5, meets grace period), OTR = 5/1 = 5 (below max 10)
        for _ in 0..2 {
            tracker.record_order_placement(now);
        }
        tracker.record_fill(now);
        assert!(!tracker.is_breached(&config));
    }

    #[test]
    fn test_is_breached_exceeded() {
        let config = OtrConfig {
            max_otr: Decimal::from(10),
            min_orders_before_enforcement: 5,
            ..Default::default()
        };

        let mut tracker = OtrTracker::new();
        let now = Utc::now();

        // Place 11 orders, 1 fill -> OTR = 11 > max 10
        for _ in 0..11 {
            tracker.record_order_placement(now);
        }
        tracker.record_fill(now);
        assert!(tracker.is_breached(&config));
    }

    #[test]
    fn test_warning_ratio() {
        let config = OtrConfig {
            max_otr: Decimal::from(50),
            warning_threshold: Decimal::from(40),
            ..Default::default()
        };

        let mut tracker = OtrTracker::new();
        let now = Utc::now();

        // Place 45 orders, 1 fill -> OTR = 45, in warning zone (>40, <=50)
        for _ in 0..45 {
            tracker.record_order_placement(now);
        }
        tracker.record_fill(now);

        assert_eq!(tracker.warning_ratio(&config), Some(Decimal::from(45)));

        // Place 55 orders, 1 fill -> OTR = 55, above max, no warning
        let mut tracker2 = OtrTracker::new();
        for _ in 0..55 {
            tracker2.record_order_placement(now);
        }
        tracker2.record_fill(now);
        assert_eq!(tracker2.warning_ratio(&config), None);
    }
}
