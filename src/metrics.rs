//! Institutional Prometheus metrics for pre-trade risk engine observability.
//!
//! # Pre-Trade Risk Gate Service Level Objective (SLO)
//!
//! Under institutional Indian market microstructure rules (NSE / BSE / SEBI),
//! the pre-trade risk gate sits directly on the deterministic execution path.
//!
//! - **Latency Target**: $p99.99 < 15\,\mu\text{s}$ ($p50 < 1\,\mu\text{s}$, $p99 < 5\,\mu\text{s}$)
//! - **Throughput Capacity**: $\ge 50,000$ order checks/second burst load
//! - **Allocation Invariant**: Zero heap allocations on the critical evaluation path
//!
//! # Exported Telemetry
//!
//! - `risk_eval_duration_seconds`: Evaluation latency histogram with sub-microsecond buckets
//!   (100ns, 250ns, 500ns, 1µs, 5µs, 15µs, 50µs, 100µs).
//! - `risk_orders_evaluated_total{algo_id, result="approved"|"rejected"}`: Evaluated decisions.
//! - `risk_breaches_total{algo_id, rule_name}`: Individual regulatory and risk rule violations.
//! - `risk_otr_current_ratio{algo_id, scrip}`: Active rolling Order-to-Trade Ratio.

use prometheus::{
    CounterVec, Encoder, GaugeVec, Histogram, HistogramOpts, Opts, Registry, TextEncoder,
};
use std::sync::LazyLock;

/// Standard sub-microsecond histogram buckets for pre-trade risk evaluation.
///
/// Configured specifically to measure nanosecond and microsecond resolution:
/// - 100 ns (0.00000010)
/// - 250 ns (0.00000025)
/// - 500 ns (0.00000050)
/// - 1 µs   (0.00000100)
/// - 5 µs   (0.00000500)
/// - 15 µs  (0.00001500) - Regulatory & SLO ceiling
/// - 50 µs  (0.00005000)
/// - 100 µs (0.00010000)
pub const RISK_EVAL_BUCKETS: &[f64] = &[
    0.000_000_100, // 100 ns
    0.000_000_250, // 250 ns
    0.000_000_500, // 500 ns
    0.000_001_000, // 1 µs
    0.000_005_000, // 5 µs
    0.000_015_000, // 15 µs (Pre-Trade SLO ceiling)
    0.000_050_000, // 50 µs
    0.000_100_000, // 100 µs
];

/// Pre-trade risk gate Prometheus metrics collector.
#[derive(Clone)]
pub struct RiskMetrics {
    registry: Registry,
    pub eval_duration: Histogram,
    pub orders_evaluated: CounterVec,
    pub breaches: CounterVec,
    pub otr_ratio: GaugeVec,
}

impl RiskMetrics {
    /// Initialize a new RiskMetrics registry and register all pre-trade risk metrics.
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // 1. Evaluation duration histogram with sub-microsecond buckets
        let hist_opts = HistogramOpts::new(
            "risk_eval_duration_seconds",
            "Histogram of pre-trade risk evaluation latency in seconds",
        )
        .buckets(RISK_EVAL_BUCKETS.to_vec());
        let eval_duration = Histogram::with_opts(hist_opts)?;
        registry.register(Box::new(eval_duration.clone()))?;

        // 2. Orders evaluated counter (labels: algo_id, result)
        let orders_opts = Opts::new(
            "risk_orders_evaluated_total",
            "Total number of orders evaluated by pre-trade risk gate",
        );
        let orders_evaluated = CounterVec::new(orders_opts, &["algo_id", "result"])?;
        registry.register(Box::new(orders_evaluated.clone()))?;

        // 3. Breaches counter (labels: algo_id, rule_name)
        let breach_opts = Opts::new(
            "risk_breaches_total",
            "Total number of pre-trade risk and regulatory rule breaches",
        );
        let breaches = CounterVec::new(breach_opts, &["algo_id", "rule_name"])?;
        registry.register(Box::new(breaches.clone()))?;

        // 4. Current OTR ratio gauge (labels: algo_id, scrip)
        let otr_opts = Opts::new(
            "risk_otr_current_ratio",
            "Active rolling Order-to-Trade Ratio (OTR) per algo_id and scrip",
        );
        let otr_ratio = GaugeVec::new(otr_opts, &["algo_id", "scrip"])?;
        registry.register(Box::new(otr_ratio.clone()))?;

        Ok(Self {
            registry,
            eval_duration,
            orders_evaluated,
            breaches,
            otr_ratio,
        })
    }

    /// Record risk evaluation latency in seconds.
    #[inline]
    pub fn record_evaluation_duration(&self, duration_seconds: f64) {
        self.eval_duration.observe(duration_seconds);
    }

    /// Record an evaluated order decision (approved or rejected).
    #[inline]
    pub fn record_order_evaluated(&self, algo_id: &str, approved: bool) {
        let result = if approved { "approved" } else { "rejected" };
        self.orders_evaluated
            .with_label_values(&[algo_id, result])
            .inc();
    }

    /// Record a rule breach.
    #[inline]
    pub fn record_breach(&self, algo_id: &str, rule_name: &str) {
        self.breaches.with_label_values(&[algo_id, rule_name]).inc();
    }

    /// Set current active rolling OTR ratio for an algo and scrip.
    #[inline]
    pub fn set_otr_ratio(&self, algo_id: &str, scrip: &str, ratio: f64) {
        self.otr_ratio
            .with_label_values(&[algo_id, scrip])
            .set(ratio);
    }

    /// Access the underlying Prometheus registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Export registered Prometheus metrics as UTF-8 Prometheus text format.
    pub fn export_metrics(&self) -> Result<String, prometheus::Error> {
        let metric_families = self.registry.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        String::from_utf8(buffer)
            .map_err(|e| prometheus::Error::Msg(format!("Prometheus text conversion failed: {e}")))
    }
}

/// Global lazy-initialized singleton metrics collector.
pub static GLOBAL_RISK_METRICS: LazyLock<RiskMetrics> = LazyLock::new(|| {
    RiskMetrics::new().expect("Failed to initialize global pre-trade risk metrics collector")
});

/// Get a reference to the global pre-trade risk metrics collector.
#[inline]
pub fn global_metrics() -> &'static RiskMetrics {
    &GLOBAL_RISK_METRICS
}
