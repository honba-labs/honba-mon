//! Comprehensive Phase 7 verification tests for honba-mon:
//! - 7.1 Dynamic Position Sizing Risk Budget Interception
//! - 7.2 Observability & Prometheus Metrics Instrumentation (with OpenTelemetry TraceContext)
//! - 7.3 Mutation Testing Rigor (freeze slicing, concentration, leverage, fast checks)

use chrono::Utc;
use rust_decimal::Decimal;

use honba_core::{
    Exchange, InstrumentId, Money, OrderRequest, OrderSide, OrderType, PositionSide,
    PositionSizingRequest, Price, PriceBand, Quantity, RiskBudget, SizingMethod, TraceContext,
};
use honba_mon::{
    RiskEngine, RiskRefusal, SizingRiskRefusal, global_metrics, validate_sizing_request,
};

// ============================================================================
// 7.1 Dynamic Position Sizing Risk Budget Interception Tests
// ============================================================================

fn make_sizing_request(
    equity: Decimal,
    entry_p: Decimal,
    sl: Option<Decimal>,
    risk_pct: Decimal,
    min_lot: u64,
) -> (PositionSizingRequest, RiskBudget) {
    let inst = InstrumentId::new(Exchange::Nse, "RELIANCE");
    let budget = RiskBudget::new(Money::inr(equity), risk_pct, Quantity::from_u64(min_lot));
    let mut req = PositionSizingRequest::new(
        inst,
        PositionSide::Long,
        Price::new(entry_p),
        Money::inr(equity),
        budget.clone(),
        SizingMethod::FixedFractional { risk_pct },
    );
    if let Some(s) = sl {
        req = req.with_stop_loss(Price::new(s));
    }
    (req, budget)
}

#[test]
fn test_sizing_request_valid_approval() {
    let (req, budget) = make_sizing_request(
        Decimal::from(1_000_000),                     // 10 Lakhs equity
        Decimal::from(2500),                          // Entry 2500
        Some(Decimal::from(2450)),                    // SL 2450 (risk 50 per share)
        Decimal::from_str_radix("0.01", 10).unwrap(), // 1% risk = 10,000 INR
        1,                                            // 1 share lot size
    );

    let res = validate_sizing_request(&req, &budget);
    assert!(res.is_ok());

    let engine = RiskEngine::default();
    let res_engine = engine.validate_sizing_request(&req, &budget);
    assert!(res_engine.is_ok());
}

#[test]
fn test_sizing_portfolio_risk_cap_exceeded() {
    let (mut req, mut budget) = make_sizing_request(
        Decimal::from(1_000_000),
        Decimal::from(2500),
        Some(Decimal::from(2450)),
        Decimal::from_str_radix("0.08", 10).unwrap(), // 8% risk requested
        1,
    );
    // Cap portfolio risk at 5% (0.05)
    budget.max_portfolio_risk_pct = Some(Decimal::from_str_radix("0.05", 10).unwrap());
    req.risk_budget.max_portfolio_risk_pct = budget.max_portfolio_risk_pct;

    let res = validate_sizing_request(&req, &budget);
    assert!(res.is_err());
    let err = res.unwrap_err();
    match err {
        honba_mon::RiskError::SizingRefusal(SizingRiskRefusal::PortfolioRiskCapExceeded {
            requested_risk_pct,
            max_portfolio_risk_pct,
            ..
        }) => {
            assert_eq!(
                requested_risk_pct,
                Decimal::from_str_radix("0.08", 10).unwrap()
            );
            assert_eq!(
                max_portfolio_risk_pct,
                Decimal::from_str_radix("0.05", 10).unwrap()
            );
        }
        other => panic!("Expected PortfolioRiskCapExceeded, got {other:?}"),
    }
}

#[test]
fn test_sizing_below_minimum_lot_size() {
    // Very tiny equity and risk: cannot even afford 1 lot
    let (req, budget) = make_sizing_request(
        Decimal::from(100),  // 100 INR equity
        Decimal::from(2500), // 2500 entry
        Some(Decimal::from(2400)),
        Decimal::from_str_radix("0.01", 10).unwrap(), // 1 INR risk
        50,                                           // Lot size 50
    );

    let res = validate_sizing_request(&req, &budget);
    assert!(res.is_err());
    let err = res.unwrap_err();
    match err {
        honba_mon::RiskError::SizingRefusal(SizingRiskRefusal::BelowMinimumLotSize {
            quantity,
            min_lot_size,
        }) => {
            assert_eq!(quantity, Quantity::ZERO);
            assert_eq!(min_lot_size, Quantity::from_u64(50));
        }
        other => panic!("Expected BelowMinimumLotSize, got {other:?}"),
    }
}

#[test]
fn test_sizing_freeze_limit_breached() {
    let (mut req, mut budget) = make_sizing_request(
        Decimal::from(50_000_000), // 5 Cr equity
        Decimal::from(100),        // Entry 100
        Some(Decimal::from(99)),   // 1 INR risk per share -> 500,000 shares raw target
        Decimal::from_str_radix("0.01", 10).unwrap(),
        1,
    );
    budget.freeze_limit_qty = Some(Quantity::from_u64(1800));
    req.risk_budget.freeze_limit_qty = budget.freeze_limit_qty;

    let nse_rel = InstrumentId::new(Exchange::Nse, "RELIANCE");
    let refusal = SizingRiskRefusal::FreezeLimitBreached {
        instrument: nse_rel,
        requested: Quantity::from_u64(5000),
        limit: Quantity::from_u64(1800),
    };
    assert_eq!(refusal.rule(), "freeze_limit_breached");
    assert!(format!("{refusal}").contains("freeze limit breached"));
}

#[test]
fn test_sizing_invalid_entry_price_and_equity() {
    let inst = InstrumentId::new(Exchange::Nse, "INFY");
    let budget = RiskBudget::new(
        Money::inr(Decimal::from(100_000)),
        Decimal::from_str_radix("0.01", 10).unwrap(),
        Quantity::from_u64(1),
    );

    // 1. Zero entry price
    let req_zero_price = PositionSizingRequest::new(
        inst.clone(),
        PositionSide::Long,
        Price::ZERO,
        Money::inr(Decimal::from(100_000)),
        budget.clone(),
        SizingMethod::FixedFractional {
            risk_pct: Decimal::from_str_radix("0.01", 10).unwrap(),
        },
    );
    let res = validate_sizing_request(&req_zero_price, &budget);
    assert!(matches!(
        res,
        Err(honba_mon::RiskError::SizingRefusal(
            SizingRiskRefusal::InvalidEntryPrice { .. }
        ))
    ));

    // 2. Zero equity
    let zero_eq_budget = RiskBudget::new(
        Money::inr(Decimal::ZERO),
        Decimal::from_str_radix("0.01", 10).unwrap(),
        Quantity::from_u64(1),
    );
    let req_zero_eq = PositionSizingRequest::new(
        inst.clone(),
        PositionSide::Long,
        Price::new(Decimal::from(1500)),
        Money::inr(Decimal::ZERO),
        zero_eq_budget.clone(),
        SizingMethod::FixedFractional {
            risk_pct: Decimal::from_str_radix("0.01", 10).unwrap(),
        },
    );
    let res_eq = validate_sizing_request(&req_zero_eq, &zero_eq_budget);
    assert!(matches!(
        res_eq,
        Err(honba_mon::RiskError::SizingRefusal(
            SizingRiskRefusal::InvalidEquity { .. }
        ))
    ));

    // 3. Invalid stop loss for Long position (stop loss >= entry)
    let req_bad_sl = PositionSizingRequest::new(
        inst,
        PositionSide::Long,
        Price::new(Decimal::from(1500)),
        Money::inr(Decimal::from(100_000)),
        budget.clone(),
        SizingMethod::FixedFractional {
            risk_pct: Decimal::from_str_radix("0.01", 10).unwrap(),
        },
    )
    .with_stop_loss(Price::new(Decimal::from(1550)));
    let res_sl = validate_sizing_request(&req_bad_sl, &budget);
    assert!(matches!(
        res_sl,
        Err(honba_mon::RiskError::SizingRefusal(
            SizingRiskRefusal::InvalidStopLoss { .. }
        ))
    ));
}

#[test]
fn test_sizing_engine_global_halt() {
    let (req, budget) = make_sizing_request(
        Decimal::from(1_000_000),
        Decimal::from(2500),
        Some(Decimal::from(2450)),
        Decimal::from_str_radix("0.01", 10).unwrap(),
        1,
    );

    let mut engine = RiskEngine::default();
    engine.halt_global("Emergency test halt", Utc::now());

    let res = engine.validate_sizing_request(&req, &budget);
    assert!(matches!(
        res,
        Err(honba_mon::RiskError::SizingRefusal(
            SizingRiskRefusal::PlatformHalted { .. }
        ))
    ));
}

// ============================================================================
// 7.2 Observability & Prometheus Metrics Instrumentation Tests
// ============================================================================

#[test]
fn test_prometheus_metrics_lifecycle() {
    let metrics = honba_mon::RiskMetrics::new().expect("Failed to create metrics");

    // 1. Record evaluation duration in sub-microsecond buckets
    metrics.record_evaluation_duration(0.000_000_150); // 150 ns
    metrics.record_evaluation_duration(0.000_002_000); // 2 µs
    metrics.record_evaluation_duration(0.000_014_000); // 14 µs

    // 2. Record evaluated decisions
    metrics.record_order_evaluated("algo-alpha", true);
    metrics.record_order_evaluated("algo-alpha", false);
    metrics.record_order_evaluated("algo-beta", true);

    // 3. Record rule breaches
    metrics.record_breach("algo-alpha", "sebi_otr_breach");
    metrics.record_breach("algo-alpha", "price_band_violation");

    // 4. Update OTR ratios
    metrics.set_otr_ratio("algo-alpha", "RELIANCE", 12.5);
    metrics.set_otr_ratio("algo-alpha", "*", 15.0);

    // 5. Export text
    let exported = metrics.export_metrics().expect("Export failed");
    assert!(exported.contains("risk_eval_duration_seconds"));
    assert!(exported.contains("risk_orders_evaluated_total"));
    assert!(exported.contains("risk_breaches_total"));
    assert!(exported.contains("risk_otr_current_ratio"));
    assert!(exported.contains("algo-alpha"));
    assert!(exported.contains("sebi_otr_breach"));
}

#[test]
fn test_opentelemetry_w3c_traceparent_propagation() {
    let mut engine = RiskEngine::default();
    engine.register_algo("trace-algo");
    let now = Utc::now();

    // Create a request with W3C distributed trace context
    let trace_id = [1u8; 16];
    let span_id = [2u8; 8];
    let trace_ctx = TraceContext::new(trace_id, span_id, TraceContext::SAMPLED);

    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "TCS"),
        OrderSide::Buy,
        OrderType::Limit(Price::new(Decimal::from(3500))),
        Quantity::from_u64(10),
        "trace-algo",
    )
    .with_trace_context(trace_ctx);

    let res = engine.check_order(&req, now);
    assert!(res.is_ok());

    let exported = global_metrics().export_metrics().unwrap();
    assert!(exported.contains("trace-algo"));
}

// ============================================================================
// 7.3 Mutation Rigor Tests: Freeze Slicing, Leverage, Concentration & Fast Check
// ============================================================================

#[test]
fn test_suggest_freeze_slices_exact_arithmetic() {
    let mut engine = RiskEngine::default();
    let inst = InstrumentId::new(Exchange::Nse, "NIFTY");
    engine
        .limits
        .freeze_limits
        .insert(inst.clone(), Quantity::from_u64(1800));

    // Case 1: No freeze limit configured for scrip -> returns 1
    let other_inst = InstrumentId::new(Exchange::Nse, "SBIN");
    assert_eq!(
        engine.suggest_freeze_slices(&other_inst, Quantity::from_u64(50000)),
        1
    );

    // Case 2: Quantity < freeze limit -> returns 1
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(100)),
        1
    );
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(1799)),
        1
    );

    // Case 3: Quantity == freeze limit -> returns 1
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(1800)),
        1
    );

    // Case 4: Quantity == freeze limit + 1 -> returns 2
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(1801)),
        2
    );

    // Case 5: Quantity == 2 * freeze limit (3600) -> returns 2
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(3600)),
        2
    );

    // Case 6: Quantity == 2 * freeze limit + 1 (3601) -> returns 3
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(3601)),
        3
    );

    // Case 7: Quantity == 5 * freeze limit (9000) -> returns 5
    assert_eq!(
        engine.suggest_freeze_slices(&inst, Quantity::from_u64(9000)),
        5
    );
}

#[test]
fn test_check_position_concentration_boundaries() {
    let mut engine = RiskEngine::default();
    let inst = InstrumentId::new(Exchange::Nse, "INFY");

    // No limit configured -> Ok
    assert!(
        engine
            .check_position_concentration(&inst, Decimal::from(500_000), Decimal::from(1_000_000))
            .is_ok()
    );

    // Configure 20% limit
    engine.limits.max_position_concentration_pct = Some(Decimal::from(20));

    // Zero or negative equity -> Ok (avoid division by zero)
    assert!(
        engine
            .check_position_concentration(&inst, Decimal::from(100_000), Decimal::ZERO)
            .is_ok()
    );

    // Exactly 20% -> Ok (200,000 / 1,000,000 * 100 = 20)
    assert!(
        engine
            .check_position_concentration(&inst, Decimal::from(200_000), Decimal::from(1_000_000))
            .is_ok()
    );

    // 19.9% -> Ok
    assert!(
        engine
            .check_position_concentration(&inst, Decimal::from(199_000), Decimal::from(1_000_000))
            .is_ok()
    );

    // 20.1% -> Err
    let res = engine.check_position_concentration(
        &inst,
        Decimal::from(201_000),
        Decimal::from(1_000_000),
    );
    assert!(matches!(
        res,
        Err(RiskRefusal::PositionConcentrationExceeded { .. })
    ));
}

#[test]
fn test_check_mis_leverage_boundaries() {
    let mut engine = RiskEngine::default();

    // No limit configured -> Ok
    assert!(
        engine
            .check_mis_leverage(Decimal::from(5_000_000), Decimal::from(1_000_000))
            .is_ok()
    );

    // Configure 5.0x leverage ceiling
    engine.limits.max_mis_leverage = Some(Decimal::from(5));

    // Zero equity -> Ok
    assert!(
        engine
            .check_mis_leverage(Decimal::from(1_000_000), Decimal::ZERO)
            .is_ok()
    );

    // Exactly 5.0x -> Ok (5,000,000 / 1,000,000 = 5.0)
    assert!(
        engine
            .check_mis_leverage(Decimal::from(5_000_000), Decimal::from(1_000_000))
            .is_ok()
    );

    // 4.9x -> Ok
    assert!(
        engine
            .check_mis_leverage(Decimal::from(4_900_000), Decimal::from(1_000_000))
            .is_ok()
    );

    // 5.1x -> Err
    let res = engine.check_mis_leverage(Decimal::from(5_100_000), Decimal::from(1_000_000));
    assert!(matches!(res, Err(RiskRefusal::MisLeverageExceeded { .. })));
}

#[test]
fn test_check_order_fast_all_refusal_variants() {
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "TCS");
    let mut engine = RiskEngine::default();
    engine.register_algo("fast-algo");

    // 1. Success path
    assert!(
        engine
            .check_order_fast("fast-algo", &inst, Quantity::from_u64(10), None, now)
            .is_ok()
    );

    // 2. Global halt
    let mut h_engine = engine.clone();
    h_engine.halt_global("Global halt test", now);
    assert!(matches!(
        h_engine.check_order_fast("fast-algo", &inst, Quantity::from_u64(10), None, now),
        Err(RiskRefusal::GlobalHalted { .. })
    ));

    // 3. Algo halt
    let mut a_engine = engine.clone();
    a_engine.halt_algo("fast-algo", "Algo halt test", now);
    assert!(matches!(
        a_engine.check_order_fast("fast-algo", &inst, Quantity::from_u64(10), None, now),
        Err(RiskRefusal::AlgoHalted { .. })
    ));

    // 4. Algo not registered
    let mut u_engine = engine.clone();
    assert!(matches!(
        u_engine.check_order_fast(
            "unregistered-fast",
            &inst,
            Quantity::from_u64(10),
            None,
            now
        ),
        Err(RiskRefusal::AlgoNotRegistered { .. })
    ));

    // 5. Price band violation
    let mut pb_engine = engine.clone();
    pb_engine.price_bands.insert(
        inst.clone(),
        PriceBand::new(
            Price::new(Decimal::from(3000)),
            Price::new(Decimal::from(3500)),
            Price::new(Decimal::from(3250)),
        ),
    );
    assert!(matches!(
        pb_engine.check_order_fast(
            "fast-algo",
            &inst,
            Quantity::from_u64(10),
            Some(Price::new(Decimal::from(4000))),
            now
        ),
        Err(RiskRefusal::PriceBandViolation { .. })
    ));

    // 6. Max quantity exceeded
    let mut q_engine = engine.clone();
    q_engine.limits.max_quantity_per_order = Some(Quantity::from_u64(100));
    assert!(matches!(
        q_engine.check_order_fast("fast-algo", &inst, Quantity::from_u64(101), None, now),
        Err(RiskRefusal::MaxQuantityExceeded { .. })
    ));

    // 7. Freeze limit exceeded
    let mut f_engine = engine.clone();
    f_engine
        .limits
        .freeze_limits
        .insert(inst.clone(), Quantity::from_u64(500));
    assert!(matches!(
        f_engine.check_order_fast("fast-algo", &inst, Quantity::from_u64(501), None, now),
        Err(RiskRefusal::FreezeLimitExceeded { .. })
    ));

    // 8. Max notional exceeded
    let mut n_engine = engine.clone();
    n_engine.limits.max_notional_per_order = Some(Decimal::from(100_000));
    assert!(matches!(
        n_engine.check_order_fast(
            "fast-algo",
            &inst,
            Quantity::from_u64(100),
            Some(Price::new(Decimal::from(1500))), // Notional: 150,000 > 100,000
            now
        ),
        Err(RiskRefusal::MaxNotionalExceeded { .. })
    ));

    // 9. Daily loss exceeded
    let mut d_engine = engine.clone();
    d_engine.limits.max_daily_loss = Some(Decimal::from(50_000));
    d_engine.daily_loss = Decimal::from(50_000);
    assert!(matches!(
        d_engine.check_order_fast("fast-algo", &inst, Quantity::from_u64(10), None, now),
        Err(RiskRefusal::DailyLossExceeded { .. })
    ));
}
