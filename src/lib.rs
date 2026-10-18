//! # 本場・門 honba-mon
//!
//! Synchronous, deterministic pre-trade risk engine for Indian equities.
//!
//! Implements:
//! - SEBI Retail Algo circular per-algo ID stateful tracking and circuit breaker
//! - Real-time Order-to-Trade Ratio (OTR) calculation & enforcement
//! - Scrip-wise circuit limits & price band validation
//! - Global emergency kill-switch and per-algo halt controls
//! - Maximum notional, position size, and event-time rate limiting
//! - Strict compliance gate between AI/LLM decisions and broker execution

pub mod engine;
pub mod error;
pub mod limits;
pub mod otr;
pub mod refusal;
pub mod state;

pub use engine::RiskEngine;
pub use error::RiskError;
pub use limits::{OrderRateLimit, RiskLimits};
pub use otr::{OtrConfig, OtrTracker};
pub use refusal::RiskRefusal;
pub use state::{AlgoState, GlobalState};
