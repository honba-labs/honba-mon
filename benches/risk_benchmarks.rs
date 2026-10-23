//! Microsecond-latency benchmarks for honba-mon RiskEngine.
//!
//! Target: < 15µs per order check on the pre-trade evaluation loop.
//! Uses Criterion for statistical measurement and regression detection.

use chrono::Utc;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use honba_core::{Exchange, InstrumentId, OrderRequest, OrderSide, OrderType, Price, Quantity};
use honba_mon::{OtrConfig, RiskEngine, RiskLimits};

fn make_order_request(algo_id: &str, symbol: &str, qty: u64) -> OrderRequest {
    OrderRequest::new(
        InstrumentId::new(Exchange::Nse, symbol),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(qty),
        algo_id,
    )
}

/// Benchmark: Basic order check with no limits configured.
fn bench_check_order_basic(c: &mut Criterion) {
    let mut engine = RiskEngine::default();
    engine.register_algo("bench-algo");
    let req = make_order_request("bench-algo", "RELIANCE", 10);
    let now = Utc::now();

    c.bench_function("check_order_basic", |b| {
        b.iter(|| {
            let result = engine.check_order(black_box(&req), black_box(now));
            let _ = black_box(result);
        })
    });
}

/// Benchmark: Order check with price band validation.
fn bench_check_order_with_price_band(c: &mut Criterion) {
    let mut engine = RiskEngine::default();
    engine.register_algo("bench-algo");
    let band = honba_core::PriceBand::new(
        Price::from_rupees("2000.00").unwrap(),
        Price::from_rupees("3000.00").unwrap(),
        Price::from_rupees("2500.00").unwrap(),
    );
    engine.set_price_band(InstrumentId::new(Exchange::Nse, "RELIANCE"), band);
    let req = make_order_request("bench-algo", "RELIANCE", 10);
    let now = Utc::now();

    c.bench_function("check_order_with_price_band", |b| {
        b.iter(|| {
            let result = engine.check_order(black_box(&req), black_box(now));
            let _ = black_box(result);
        })
    });
}

/// Benchmark: Order check with OTR tracking (multiple orders).
fn bench_check_order_with_otr(c: &mut Criterion) {
    let mut engine = RiskEngine::default();
    engine.register_algo("bench-algo");
    let req = make_order_request("bench-algo", "RELIANCE", 10);
    let now = Utc::now();

    c.bench_function("check_order_with_otr", |b| {
        b.iter(|| {
            let result = engine.check_order(black_box(&req), black_box(now));
            let _ = black_box(result);
        })
    });
}

/// Benchmark: Order check with freeze limit validation.
fn bench_check_order_with_freeze_limit(c: &mut Criterion) {
    let mut limits = RiskLimits::default();
    limits.freeze_limits.insert(
        InstrumentId::new(Exchange::Nse, "NIFTY"),
        Quantity::from_u64(1800),
    );
    let mut engine = RiskEngine::new(limits, OtrConfig::default());
    engine.register_algo("bench-algo");
    let req = make_order_request("bench-algo", "NIFTY", 100);
    let now = Utc::now();

    c.bench_function("check_order_with_freeze_limit", |b| {
        b.iter(|| {
            let result = engine.check_order(black_box(&req), black_box(now));
            let _ = black_box(result);
        })
    });
}

/// Benchmark: Order check with notional limit.
fn bench_check_order_with_notional_limit(c: &mut Criterion) {
    let mut limits = RiskLimits::default();
    limits.max_notional_per_order = Some(rust_decimal::Decimal::from(2500000));
    let mut engine = RiskEngine::new(limits, OtrConfig::default());
    engine.register_algo("bench-algo");
    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "RELIANCE"),
        OrderSide::Buy,
        OrderType::Limit(Price::from_rupees("3000.00").unwrap()),
        Quantity::from_u64(1000),
        "bench-algo",
    );
    let now = Utc::now();

    c.bench_function("check_order_with_notional_limit", |b| {
        b.iter(|| {
            let result = engine.check_order(black_box(&req), black_box(now));
            let _ = black_box(result);
        })
    });
}

/// Benchmark: check_order_fast (zero-allocation path) with pre-registered algo.
fn bench_check_order_fast(c: &mut Criterion) {
    let mut engine = RiskEngine::default();
    engine.register_algo("bench-algo");
    let now = Utc::now();

    c.bench_function("check_order_fast", |b| {
        b.iter(|| {
            let result = engine.check_order_fast(
                black_box("bench-algo"),
                black_box(&InstrumentId::new(Exchange::Nse, "RELIANCE")),
                black_box(Quantity::from_u64(10)),
                black_box(None),
                black_box(now),
            );
            let _ = black_box(result);
        })
    });
}

/// Benchmark: Multiple sequential order checks (realistic throughput scenario).
fn bench_sequential_order_checks(c: &mut Criterion) {
    let mut engine = RiskEngine::default();
    engine.register_algo("bench-algo");
    let now = Utc::now();

    c.bench_function("sequential_order_checks", |b| {
        b.iter(|| {
            for i in 0..100u64 {
                let req = make_order_request("bench-algo", "RELIANCE", i + 1);
                let result = engine.check_order(black_box(&req), black_box(now));
                let _ = black_box(result);
            }
        })
    });
}

/// Benchmark: OTR breach scenario (algo gets halted after threshold).
fn bench_otr_breach(c: &mut Criterion) {
    let limits = RiskLimits::default();
    let otr_config = OtrConfig {
        max_otr: rust_decimal::Decimal::from(10),
        min_orders_before_enforcement: 5,
        warning_threshold: rust_decimal::Decimal::from(40),
        window_1min_ms: 60_000,
        window_5min_ms: 300_000,
        halt_cooldown_ms: 0,
    };
    let mut engine = RiskEngine::new(limits, otr_config);
    engine.register_algo("breach-algo");
    let req = make_order_request("breach-algo", "WIPRO", 1);
    let now = Utc::now();

    c.bench_function("otr_breach", |b| {
        b.iter(|| {
            let result = engine.check_order(black_box(&req), black_box(now));
            let _ = black_box(result);
        })
    });
}

criterion_group!(
    risk_benches,
    bench_check_order_basic,
    bench_check_order_with_price_band,
    bench_check_order_with_otr,
    bench_check_order_with_freeze_limit,
    bench_check_order_with_notional_limit,
    bench_check_order_fast,
    bench_sequential_order_checks,
    bench_otr_breach,
);

criterion_main!(risk_benches);
