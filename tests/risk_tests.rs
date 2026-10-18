use chrono::Utc;
use honba_core::*;
use honba_mon::*;
use rust_decimal::Decimal;

#[test]
fn test_global_kill_switch() {
    let mut engine = RiskEngine::default();
    let now = Utc::now();
    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "INFY"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(10),
        "algo-1",
    );

    // Initial order passes
    assert!(engine.check_order(&req, now).is_ok());

    // Trigger platform kill-switch
    engine.halt_global("SEBI regulatory query", now);

    // Now rejected
    let res = engine.check_order(&req, now);
    assert!(matches!(res, Err(RiskRefusal::GlobalHalted { .. })));

    // Resume
    engine.resume_global();
    assert!(engine.check_order(&req, now).is_ok());
}

#[test]
fn test_per_algo_halt() {
    let mut engine = RiskEngine::default();
    let now = Utc::now();

    let req_bad = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "TCS"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(5),
        "erratic-algo",
    );
    let req_good = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "TCS"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(5),
        "good-algo",
    );

    // Halt erratic-algo only
    engine.halt_algo("erratic-algo", "Manual kill-switch invoked", now);

    assert!(matches!(
        engine.check_order(&req_bad, now),
        Err(RiskRefusal::AlgoHalted { .. })
    ));
    assert!(engine.check_order(&req_good, now).is_ok());
}

#[test]
fn test_circuit_limits_and_price_bands() {
    let mut engine = RiskEngine::default();
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "RELIANCE");

    // 20% price band: 2000 to 3000
    let band = PriceBand::new(
        Price::from_rupees("2000.00").unwrap(),
        Price::from_rupees("3000.00").unwrap(),
        Price::from_rupees("2500.00").unwrap(),
    );
    engine.set_price_band(inst.clone(), band);

    // Within band
    let req_ok = OrderRequest::new(
        inst.clone(),
        OrderSide::Buy,
        OrderType::Limit(Price::from_rupees("2550.00").unwrap()),
        Quantity::from_u64(10),
        "algo-1",
    );
    assert!(engine.check_order(&req_ok, now).is_ok());

    // Above upper circuit
    let req_high = OrderRequest::new(
        inst.clone(),
        OrderSide::Buy,
        OrderType::Limit(Price::from_rupees("3050.00").unwrap()),
        Quantity::from_u64(10),
        "algo-1",
    );
    assert!(matches!(
        engine.check_order(&req_high, now),
        Err(RiskRefusal::PriceBandViolation { .. })
    ));

    // Below lower circuit
    let req_low = OrderRequest::new(
        inst,
        OrderSide::Sell,
        OrderType::Limit(Price::from_rupees("1950.00").unwrap()),
        Quantity::from_u64(10),
        "algo-1",
    );
    assert!(matches!(
        engine.check_order(&req_low, now),
        Err(RiskRefusal::PriceBandViolation { .. })
    ));
}

#[test]
fn test_market_circuit_breaker() {
    let mut engine = RiskEngine::default();
    let now = Utc::now();
    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "INFY"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(10),
        "algo-index",
    );

    assert!(engine.check_order(&req, now).is_ok());

    // 10% index circuit tripped with 45 min cooling off
    let resume_at = now + chrono::Duration::minutes(45);
    engine.trigger_market_circuit("NIFTY 50", 10, Some(resume_at), now);

    // Order during halt is rejected
    let during_halt = now + chrono::Duration::minutes(15);
    let res = engine.check_order(&req, during_halt);
    assert!(matches!(
        res,
        Err(RiskRefusal::MarketCircuitHalt { stage_pct: 10, .. })
    ));

    // After cooling-off expires
    let after_resume = now + chrono::Duration::minutes(46);
    assert!(engine.check_order(&req, after_resume).is_ok());
}

#[test]
fn test_sebi_algo_id_validation() {
    let mut engine = RiskEngine::default();
    let now = Utc::now();

    // Empty algo_id violates SEBI Retail Algo circular
    let req_empty_algo = OrderRequest {
        client_order_id: uuid::Uuid::new_v4(),
        instrument: InstrumentId::new(Exchange::Nse, "TCS"),
        side: OrderSide::Buy,
        order_type: OrderType::Market,
        quantity: Quantity::from_u64(10),
        tif: TimeInForce::Day,
        algo_id: "".to_string(),
        tag: None,
    };
    let res = engine.check_order(&req_empty_algo, now);
    assert!(matches!(res, Err(RiskRefusal::InvalidOrder { .. })));

    // Zero quantity violates order rules
    let req_zero_qty = OrderRequest {
        client_order_id: uuid::Uuid::new_v4(),
        instrument: InstrumentId::new(Exchange::Nse, "TCS"),
        side: OrderSide::Buy,
        order_type: OrderType::Market,
        quantity: Quantity::ZERO,
        tif: TimeInForce::Day,
        algo_id: "algo-valid".to_string(),
        tag: None,
    };
    let res2 = engine.check_order(&req_zero_qty, now);
    assert!(matches!(res2, Err(RiskRefusal::InvalidOrder { .. })));
}

#[test]
fn test_sebi_otr_circuit_breaker() {
    let limits = RiskLimits::default();
    let otr_config = OtrConfig {
        max_otr: Decimal::from(10),       // Max 10:1 ratio
        min_orders_before_enforcement: 5, // Enforce after 5 orders
    };

    let mut engine = RiskEngine::new(limits, otr_config);
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "WIPRO");

    let req = OrderRequest::new(
        inst.clone(),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(1),
        "high-frequency-spammer",
    );

    // Place 5 orders without any fill
    for _ in 0..5 {
        assert!(engine.check_order(&req, now).is_ok());
    }

    // Now orders = 5, trades = 0, so OTR = 5 / 1 = 5.
    // Place more orders to cross 10:1
    for _ in 0..5 {
        assert!(engine.check_order(&req, now).is_ok());
    }
    // Now orders = 10, trades = 0 -> ratio = 10. Next order will reach ratio 11 > 10
    assert!(engine.check_order(&req, now).is_ok());

    // Next order should breach and trigger automatic halt
    let res = engine.check_order(&req, now);
    assert!(matches!(res, Err(RiskRefusal::OtrBreached { .. })));

    // The algo is now marked Halted in engine
    assert!(matches!(
        engine.check_order(&req, now),
        Err(RiskRefusal::AlgoHalted { .. })
    ));
}

#[test]
fn test_order_rate_limiter() {
    let limits = RiskLimits {
        order_rate_limit: Some(OrderRateLimit {
            max_orders: 3,
            window_ms: 1000,
        }),
        ..Default::default()
    };
    let mut engine = RiskEngine::new(limits, OtrConfig::default());
    let now = Utc::now();
    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "SBIN"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(1),
        "algo-rate",
    );

    assert!(engine.check_order(&req, now).is_ok());
    assert!(engine.check_order(&req, now).is_ok());
    assert!(engine.check_order(&req, now).is_ok());

    // 4th order in same timestamp breaches rate limit
    let res = engine.check_order(&req, now);
    assert!(matches!(res, Err(RiskRefusal::OrderRateExceeded { .. })));

    // After 1001ms, rate limit window has slid forward
    let future = now + chrono::Duration::milliseconds(1001);
    assert!(engine.check_order(&req, future).is_ok());
}
