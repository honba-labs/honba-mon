//! # 本場・門 honba-mon
//!
//! Synchronous, deterministic pre-trade risk engine and regulatory compliance validator for Indian equities (NSE / BSE / NFO).
//!
//! # Pre-Trade Risk Gate SLO
//!
//! Under high-throughput algorithmic execution conditions:
//! - **Evaluation Latency**: $p99.99 < 15\,\mu\text{s}$ under burst loads ($p50 < 1\,\mu\text{s}$, $p99 < 5\,\mu\text{s}$)
//! - **Throughput Capacity**: $\ge 50,000$ checks/second burst capacity
//! - **Zero-Allocation Invariant**: Zero heap allocations on the critical evaluation hot path ([`RiskEngine::check_order_fast`])
//! - **Pure Zero-I/O Domain Invariant**: 100% deterministic, in-memory synchronous execution
//!
//! # Core Capabilities
//! - SEBI Retail Algo circular per-algo ID stateful tracking and circuit breaker
//! - Real-time Order-to-Trade Ratio (OTR) calculation & enforcement (overall and scrip-level)
//! - Scrip-wise circuit limits & price band validation
//! - Global emergency kill-switch and per-algo halt controls
//! - Maximum notional, position size, exchange freeze limit slicing, and event-time rate limiting
//! - Dynamic position sizing risk budget gate ([`validate_sizing_request`])
//! - Institutional Prometheus metrics ([`RiskMetrics`]) and OpenTelemetry W3C distributed trace context propagation

pub mod algo_registry;
pub mod engine;
pub mod error;
pub mod limits;
pub mod metrics;
pub mod otr;
pub mod refusal;
pub mod sizing_gate;
pub mod state;

pub use algo_registry::AlgoIdRegistry;
pub use engine::RiskEngine;
pub use error::RiskError;
pub use limits::{OrderRateLimit, RiskLimits};
pub use metrics::{RISK_EVAL_BUCKETS, RiskMetrics, global_metrics};
pub use otr::{OtrConfig, OtrTracker};
pub use refusal::RiskRefusal;
pub use sizing_gate::{SizingRiskRefusal, validate_sizing_request};
pub use state::{AlgoState, GlobalState};
