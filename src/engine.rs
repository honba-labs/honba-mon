//! The synchronous, deterministic Risk Gate (`honba-mon`).
//!
//! A pure synchronous module that cannot be bypassed by LLMs or strategies.
//! Operates between Strategy Decision / Execution Request and the Broker Executor.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};

use honba_core::{Decision, FillReport, InstrumentId, OrderRequest, Price, PriceBand, Quantity};

use crate::limits::RiskLimits;
use crate::otr::{OtrConfig, OtrTracker};
use crate::refusal::RiskRefusal;
use crate::state::{AlgoState, GlobalState, MarketCircuitHalt};

/// Synchronous, deterministic risk engine.
#[derive(Debug, Clone)]
pub struct RiskEngine {
    /// Global trading state (Active, ReduceOnly, Halted)
    pub global_state: GlobalState,
    /// Market-wide index circuit breaker status (10%, 15%, 20%)
    pub market_circuit: Option<MarketCircuitHalt>,
    /// Per-algo ID operational states
    pub algo_states: HashMap<String, AlgoState>,
    /// Per-algo OTR trackers
    pub algo_trackers: HashMap<String, OtrTracker>,
    /// Per-scrip OTR trackers
    pub scrip_trackers: HashMap<InstrumentId, OtrTracker>,
    /// Refusal counters by rule name
    pub refusal_counts: HashMap<String, u64>,
    /// Cumulative daily notional turnover in INR per algo
    pub cumulative_turnover: HashMap<String, Decimal>,
    /// Cumulative daily realized PnL in INR per algo
    pub cumulative_pnl: HashMap<String, Decimal>,
    /// OTR regulatory configuration
    pub otr_config: OtrConfig,
    /// Hard risk limits
    pub limits: RiskLimits,
    /// Scrip-wise price bands / circuit limits
    pub price_bands: HashMap<InstrumentId, PriceBand>,
    /// Timestamps of recently approved orders for rate limiting
    pub order_history: VecDeque<DateTime<Utc>>,
    /// Cumulative daily realized loss (positive number representing loss in INR)
    pub daily_loss: Decimal,
}

impl Default for RiskEngine {
    fn default() -> Self {
        Self {
            global_state: GlobalState::Active,
            market_circuit: None,
            algo_states: HashMap::new(),
            algo_trackers: HashMap::new(),
            scrip_trackers: HashMap::new(),
            refusal_counts: HashMap::new(),
            cumulative_turnover: HashMap::new(),
            cumulative_pnl: HashMap::new(),
            otr_config: OtrConfig::default(),
            limits: RiskLimits::default(),
            price_bands: HashMap::new(),
            order_history: VecDeque::new(),
            daily_loss: Decimal::ZERO,
        }
    }
}

impl RiskEngine {
    /// Create a new risk engine with specified limits and OTR settings.
    pub fn new(limits: RiskLimits, otr_config: OtrConfig) -> Self {
        Self {
            global_state: GlobalState::Active,
            market_circuit: None,
            algo_states: HashMap::new(),
            algo_trackers: HashMap::new(),
            scrip_trackers: HashMap::new(),
            refusal_counts: HashMap::new(),
            cumulative_turnover: HashMap::new(),
            cumulative_pnl: HashMap::new(),
            otr_config,
            limits,
            price_bands: HashMap::new(),
            order_history: VecDeque::new(),
            daily_loss: Decimal::ZERO,
        }
    }

    /// Trigger platform-wide emergency kill switch.
    pub fn halt_global(&mut self, reason: impl Into<String>, timestamp: DateTime<Utc>) {
        let reason_str = reason.into();
        tracing::warn!("TRIGGERING GLOBAL KILL-SWITCH: {}", reason_str);
        self.global_state = GlobalState::Halted {
            reason: reason_str,
            halted_at: timestamp,
        };
    }

    /// Resume platform trading from global halt.
    pub fn resume_global(&mut self) {
        tracing::info!("Resuming global platform trading");
        self.global_state = GlobalState::Active;
    }

    /// Set platform to reduce-only mode.
    pub fn set_reduce_only(&mut self) {
        self.global_state = GlobalState::ReduceOnly;
    }

    /// Trigger market-wide index circuit halt (10%, 15%, 20%).
    pub fn trigger_market_circuit(
        &mut self,
        index_symbol: impl Into<String>,
        stage_pct: u8,
        resume_at: Option<DateTime<Utc>>,
        halted_at: DateTime<Utc>,
    ) {
        let sym = index_symbol.into();
        tracing::warn!(
            "MARKET-WIDE INDEX CIRCUIT BREAKER TRIP: {} stage {}%",
            sym,
            stage_pct
        );
        self.market_circuit = Some(MarketCircuitHalt {
            index_symbol: sym,
            stage_pct,
            halted_at,
            resume_at,
        });
    }

    /// Clear market-wide index circuit breaker.
    pub fn clear_market_circuit(&mut self) {
        tracing::info!("Clearing market-wide index circuit breaker");
        self.market_circuit = None;
    }

    /// Halt an individual algorithm by ID (SEBI circular compliance).
    pub fn halt_algo(
        &mut self,
        algo_id: &str,
        reason: impl Into<String>,
        timestamp: DateTime<Utc>,
    ) {
        let reason_str = reason.into();
        tracing::warn!("Halted algo '{}': {}", algo_id, reason_str);
        self.algo_states.insert(
            algo_id.to_string(),
            AlgoState::Halted {
                reason: reason_str,
                halted_at: timestamp,
            },
        );
    }

    /// Resume an individual algorithm.
    pub fn resume_algo(&mut self, algo_id: &str) {
        tracing::info!("Resumed algo '{}'", algo_id);
        self.algo_states
            .insert(algo_id.to_string(), AlgoState::Active);
    }

    /// Update or register a scrip-wise price band.
    pub fn set_price_band(&mut self, instrument: InstrumentId, band: PriceBand) {
        self.price_bands.insert(instrument, band);
    }

    /// Record a trade fill to update OTR and trackers.
    pub fn record_fill(&mut self, fill: &FillReport) {
        let tracker = self.algo_trackers.entry(fill.algo_id.clone()).or_default();
        tracker.record_fill();

        let scrip_tracker = self
            .scrip_trackers
            .entry(fill.instrument.clone())
            .or_default();
        scrip_tracker.record_fill();

        let turnover = fill.price.0 * fill.quantity.0;
        *self
            .cumulative_turnover
            .entry(fill.algo_id.clone())
            .or_insert(Decimal::ZERO) += turnover;
    }

    /// Record an order cancellation or modification.
    pub fn record_cancellation(&mut self, algo_id: &str) {
        let tracker = self.algo_trackers.entry(algo_id.to_string()).or_default();
        tracker.record_order_mod_or_cancel();
    }

    /// Record an order cancellation or modification with instrument.
    pub fn record_cancellation_for_scrip(&mut self, algo_id: &str, instrument: &InstrumentId) {
        self.record_cancellation(algo_id);
        let scrip_tracker = self.scrip_trackers.entry(instrument.clone()).or_default();
        scrip_tracker.record_order_mod_or_cancel();
    }

    /// Record realized loss to update daily drawdown.
    pub fn record_realized_loss(&mut self, loss_inr: Decimal) {
        if loss_inr > Decimal::ZERO {
            self.daily_loss += loss_inr;
        }
    }

    /// Record realized PnL for an algorithm.
    pub fn record_pnl(&mut self, algo_id: &str, pnl: Decimal) {
        *self
            .cumulative_pnl
            .entry(algo_id.to_string())
            .or_insert(Decimal::ZERO) += pnl;
        if pnl < Decimal::ZERO {
            self.daily_loss += -pnl;
        }
    }

    /// Record a risk refusal occurrence for metrics.
    pub fn record_refusal(&mut self, refusal: &RiskRefusal) {
        let rule = refusal.rule();
        *self.refusal_counts.entry(rule.to_string()).or_insert(0) += 1;
    }

    /// Returns the current OTR for a specific algo ID.
    pub fn otr_for_algo(&self, algo_id: &str) -> Option<Decimal> {
        self.algo_trackers.get(algo_id).map(|t| t.current_otr())
    }

    /// Returns the current OTR for a specific instrument.
    pub fn otr_for_scrip(&self, instrument: &InstrumentId) -> Option<Decimal> {
        self.scrip_trackers.get(instrument).map(|t| t.current_otr())
    }

    /// Returns total refusal count across all rules.
    pub fn total_refusals(&self) -> u64 {
        self.refusal_counts.values().sum()
    }

    /// Returns refusal count for a specific rule.
    pub fn refusals_for_rule(&self, rule: &str) -> u64 {
        self.refusal_counts.get(rule).copied().unwrap_or(0)
    }

    /// Returns cumulative turnover for an algo ID in INR.
    pub fn turnover_for_algo(&self, algo_id: &str) -> Decimal {
        self.cumulative_turnover
            .get(algo_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    /// Returns cumulative realized PnL for an algo ID in INR.
    pub fn pnl_for_algo(&self, algo_id: &str) -> Decimal {
        self.cumulative_pnl
            .get(algo_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    /// Synchronously check an AI/LLM Decision.
    pub fn check_decision(
        &mut self,
        decision: &Decision,
        now: DateTime<Utc>,
    ) -> Result<(), RiskRefusal> {
        // Validate basic decision structure
        if let Err(e) = decision.validate() {
            let refusal = RiskRefusal::InvalidOrder {
                error: e.to_string(),
            };
            self.record_refusal(&refusal);
            return Err(refusal);
        }

        match decision {
            Decision::Buy {
                symbol,
                exchange,
                qty,
                price_limit,
                algo_id,
                ..
            }
            | Decision::Sell {
                symbol,
                exchange,
                qty,
                price_limit,
                algo_id,
                ..
            } => {
                let inst = InstrumentId::new(*exchange, symbol.clone());
                self.evaluate_core(algo_id, &inst, *qty, *price_limit, now)
            }
            Decision::Hold { .. } => Ok(()),
            Decision::CancelAll { algo_id, .. } => {
                if let Some(id) = algo_id {
                    self.record_cancellation(id);
                }
                Ok(())
            }
        }
    }

    /// Synchronously check an outgoing OrderRequest before sending to broker.
    pub fn check_order(
        &mut self,
        order: &OrderRequest,
        now: DateTime<Utc>,
    ) -> Result<(), RiskRefusal> {
        // Enforce structural order validation per SEBI & market rules
        if let Err(e) = order.validate() {
            let refusal = RiskRefusal::InvalidOrder {
                error: e.to_string(),
            };
            self.record_refusal(&refusal);
            return Err(refusal);
        }

        let price_limit = order.order_type.limit_price();
        self.evaluate_core(
            &order.algo_id,
            &order.instrument,
            order.quantity,
            price_limit,
            now,
        )
    }

    /// Core synchronous risk evaluation pipeline.
    fn evaluate_core(
        &mut self,
        algo_id: &str,
        instrument: &InstrumentId,
        quantity: Quantity,
        limit_price: Option<Price>,
        now: DateTime<Utc>,
    ) -> Result<(), RiskRefusal> {
        // 1. Global Kill-Switch Check
        if let GlobalState::Halted { reason, .. } = &self.global_state {
            let r = RiskRefusal::GlobalHalted {
                reason: reason.clone(),
            };
            self.record_refusal(&r);
            return Err(r);
        }

        // 1.5 Market-Wide Index Circuit Breaker Check
        if let Some(circuit) = &self.market_circuit {
            match circuit.resume_at {
                Some(resume) if now < resume => {
                    let r = RiskRefusal::MarketCircuitHalt {
                        index_symbol: circuit.index_symbol.clone(),
                        stage_pct: circuit.stage_pct,
                        resume_at: Some(resume),
                    };
                    self.record_refusal(&r);
                    return Err(r);
                }
                None => {
                    let r = RiskRefusal::MarketCircuitHalt {
                        index_symbol: circuit.index_symbol.clone(),
                        stage_pct: circuit.stage_pct,
                        resume_at: None,
                    };
                    self.record_refusal(&r);
                    return Err(r);
                }
                _ => {} // cooling-off period has passed
            }
        }

        // 2. Per-Algo ID Halt Check
        if let Some(AlgoState::Halted { reason, .. }) = self.algo_states.get(algo_id) {
            let r = RiskRefusal::AlgoHalted {
                algo_id: algo_id.to_string(),
                reason: reason.clone(),
            };
            self.record_refusal(&r);
            return Err(r);
        }

        // 3. SEBI Order-to-Trade Ratio Check
        let tracker = self.algo_trackers.entry(algo_id.to_string()).or_default();
        if tracker.is_breached(&self.otr_config) {
            let current_otr = tracker.current_otr();
            // Automatically trip circuit breaker for this algo
            self.halt_algo(
                algo_id,
                format!("Automatic circuit breaker: OTR breached (ratio: {current_otr})"),
                now,
            );
            let r = RiskRefusal::OtrBreached {
                algo_id: algo_id.to_string(),
                current_otr,
                max_otr: self.otr_config.max_otr,
            };
            self.record_refusal(&r);
            return Err(r);
        }

        // 4. Circuit Limits & Price Band Check
        if let Some(band) = self.price_bands.get(instrument) {
            if let Some(price) = limit_price {
                if !band.contains(price) {
                    let r = RiskRefusal::PriceBandViolation { price, band: *band };
                    self.record_refusal(&r);
                    return Err(r);
                }
            }
        }

        // 5. Hard Maximum Quantity Check
        if let Some(max_qty) = self.limits.max_quantity_per_order {
            if quantity.0 > max_qty.0 {
                let r = RiskRefusal::MaxQuantityExceeded {
                    requested: quantity,
                    limit: max_qty,
                };
                self.record_refusal(&r);
                return Err(r);
            }
        }

        // 6. Hard Maximum Notional Check
        if let Some(max_notional) = self.limits.max_notional_per_order {
            let eval_price = limit_price
                .or_else(|| self.price_bands.get(instrument).map(|b| b.reference_price))
                .unwrap_or(Price::ZERO);

            let notional = eval_price.0 * quantity.0;
            if notional > max_notional {
                let r = RiskRefusal::MaxNotionalExceeded {
                    requested: notional,
                    limit: max_notional,
                };
                self.record_refusal(&r);
                return Err(r);
            }
        }

        // 7. Maximum Daily Loss Check
        if let Some(max_loss) = self.limits.max_daily_loss {
            if self.daily_loss >= max_loss {
                let r = RiskRefusal::DailyLossExceeded {
                    current_loss: self.daily_loss,
                    limit: max_loss,
                };
                self.record_refusal(&r);
                return Err(r);
            }
        }

        // 8. Order Rate Limit Check (Sliding Event-Time Window)
        if let Some(rate_limit) = self.limits.order_rate_limit {
            let window_duration = chrono::Duration::milliseconds(rate_limit.window_ms as i64);
            let cutoff = now - window_duration;

            // Evict expired entries
            while let Some(front) = self.order_history.front() {
                if *front < cutoff {
                    self.order_history.pop_front();
                } else {
                    break;
                }
            }

            if self.order_history.len() >= rate_limit.max_orders as usize {
                let r = RiskRefusal::OrderRateExceeded {
                    count: self.order_history.len() as u32,
                    limit: rate_limit.max_orders,
                    window_ms: rate_limit.window_ms,
                };
                self.record_refusal(&r);
                return Err(r);
            }
        }

        // All checks passed! Record order in rate limiter, algo tracker, and scrip tracker
        self.order_history.push_back(now);
        let tracker = self.algo_trackers.entry(algo_id.to_string()).or_default();
        tracker.record_order_placement();

        let scrip_tracker = self.scrip_trackers.entry(instrument.clone()).or_default();
        scrip_tracker.record_order_placement();

        Ok(())
    }
}
