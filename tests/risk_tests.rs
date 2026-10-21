use chrono::Utc;
use honba_core::*;
use honba_mon::*;
use rust_decimal::Decimal;

#[test]
fn test_global_kill_switch() {
    let mut engine = RiskEngine::default();
    engine.register_algo("algo-1");
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
    engine.register_algo("erratic-algo");
    engine.register_algo("good-algo");
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
    engine.register_algo("algo-1");
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
    engine.register_algo("algo-index");
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
        client_id: None,
        correlation_id: None,
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
        client_id: None,
        correlation_id: None,
    };
    let res2 = engine.check_order(&req_zero_qty, now);
    assert!(matches!(res2, Err(RiskRefusal::InvalidOrder { .. })));
}

#[test]
fn test_sebi_algo_not_registered() {
    let mut engine = RiskEngine::default();
    let now = Utc::now();

    // Unregistered algo_id should be rejected
    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "TCS"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(10),
        "unregistered-algo",
    );
    let res = engine.check_order(&req, now);
    assert!(matches!(
        res,
        Err(RiskRefusal::AlgoNotRegistered { .. })
    ));
}

#[test]
fn test_sebi_otr_circuit_breaker() {
    let limits = RiskLimits::default();
    let otr_config = OtrConfig {
        max_otr: Decimal::from(10),       // Max 10:1 ratio
        min_orders_before_enforcement: 5, // Enforce after 5 orders
        warning_threshold: Decimal::from(40),
        window_1min_ms: 60_000,
        window_5min_ms: 300_000,
        halt_cooldown_ms: 0,
    };

    let mut engine = RiskEngine::new(limits, otr_config);
    engine.register_algo("high-frequency-spammer");
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

    // Place more orders to cross 10:1 in 1-min window
    for _ in 0..5 {
        assert!(engine.check_order(&req, now).is_ok());
    }
    // Now 10 orders in 1-min window, no trades -> ratio = 10/1 = 10.
    // Order 11 reaches ratio 11 > 10, triggering breach

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
    let mut engine = RiskEngine::default();
    engine.register_algo("algo-rate");
    let limits = RiskLimits {
        order_rate_limit: Some(OrderRateLimit {
            max_orders: 3,
            window_ms: 1000,
        }),
        ..Default::default()
    };
    // Need to recreate engine with limits - but we already registered algo
    // Let's just create a new engine
    let mut engine = RiskEngine::new(limits, OtrConfig::default());
    engine.register_algo("algo-rate");
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

#[test]
fn test_otn_sliding_window() {
    let mut engine = RiskEngine::default();
    engine.register_algo("spammer");
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "WIPRO");

    let req = OrderRequest::new(
        inst.clone(),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(1),
        "spammer",
    );

    // Place orders at t=0
    for _ in 0..10 {
        assert!(engine.check_order(&req, now).is_ok());
    }

    // Advance time past 1 minute - events should fall out of window
    let later = now + chrono::Duration::seconds(61);

    // Should be able to place more orders since old ones expired from 1-min window
    for _ in 0..5 {
        assert!(engine.check_order(&req, later).is_ok());
    }
}

#[test]
fn test_algo_registry_suspend() {
    let mut engine = RiskEngine::default();
    engine.register_algo("suspended-algo");
    engine.suspend_algo("suspended-algo", "SEBI violation");
    let now = Utc::now();

    let req = OrderRequest::new(
        InstrumentId::new(Exchange::Nse, "TCS"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(10),
        "suspended-algo",
    );

    let res = engine.check_order(&req, now);
    assert!(matches!(
        res,
        Err(RiskRefusal::AlgoNotRegistered { .. })
    ));

    // Reactivate and verify
    engine.reactivate_algo("suspended-algo");
    assert!(engine.check_order(&req, now).is_ok());
}

#[test]
fn test_validate_price_band() {
    let mut engine = RiskEngine::default();
    engine.register_algo("algo-1");
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "RELIANCE");

    let band = PriceBand::new(
        Price::from_rupees("2000.00").unwrap(),
        Price::from_rupees("3000.00").unwrap(),
        Price::from_rupees("2500.00").unwrap(),
    );
    engine.set_price_band(inst.clone(), band);

    // Price within band
    assert!(engine
        .validate_price_band(&inst, Price::from_rupees("2550.00").unwrap())
        .is_ok());

    // Price above band
    assert!(matches!(
        engine.validate_price_band(&inst, Price::from_rupees("3050.00").unwrap()),
        Err(RiskRefusal::PriceBandViolation { .. })
    ));

    // Price below band
    assert!(matches!(
        engine.validate_price_band(&inst, Price::from_rupees("1950.00").unwrap()),
        Err(RiskRefusal::PriceBandViolation { .. })
    ));

    // Instrument with no band - always valid
    let unknown = InstrumentId::new(Exchange::Nse, "UNKNOWN");
    assert!(engine.validate_price_band(&unknown, Price::from_rupees("99999.00").unwrap()).is_ok());
}

#[test]
fn test_scrip_otr_breach() {
    let limits = RiskLimits::default();
    let otr_config = OtrConfig {
        max_otr: Decimal::from(3),
        min_orders_before_enforcement: 1,
        warning_threshold: Decimal::from(0),
        window_1min_ms: 60_000,
        window_5min_ms: 300_000,
        halt_cooldown_ms: 0,
    };
    let mut engine = RiskEngine::new(limits, otr_config);
    engine.register_algo("spammer");
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "WIPRO");

    let req = OrderRequest::new(
        inst.clone(),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(1),
        "spammer",
    );

    // Place orders on same scrip - should breach OTR within 4 orders
    let mut breached = false;
    for _ in 0..5 {
        let res = engine.check_order(&req, now);
        if matches!(res, Err(RiskRefusal::OtrBreached { .. }))
            || matches!(res, Err(RiskRefusal::ScripOtrBreached { .. }))
        {
            breached = true;
            break;
        }
    }
    assert!(breached, "Expected OTR or scrip OTR breach");
}

#[test]
fn test_suggest_freeze_slices() {
    let mut limits = RiskLimits::default();
    limits.freeze_limits.insert(
        InstrumentId::new(Exchange::Nse, "NIFTY"),
        Quantity::from_u64(1800),
    );
    let engine = RiskEngine::new(limits, OtrConfig::default());
    let nifty = InstrumentId::new(Exchange::Nse, "NIFTY");

    // No freeze limit configured -> 1 slice
    let no_limit_engine = RiskEngine::new(RiskLimits::default(), OtrConfig::default());
    assert_eq!(
        no_limit_engine.suggest_freeze_slices(&nifty, Quantity::from_u64(5000)),
        1
    );

    // qty <= freeze -> 1 slice
    assert_eq!(
        engine.suggest_freeze_slices(&nifty, Quantity::from_u64(1800)),
        1
    );
    assert_eq!(
        engine.suggest_freeze_slices(&nifty, Quantity::from_u64(100)),
        1
    );

    // qty > freeze -> ceiling division
    assert_eq!(
        engine.suggest_freeze_slices(&nifty, Quantity::from_u64(1801)),
        2
    );
    assert_eq!(
        engine.suggest_freeze_slices(&nifty, Quantity::from_u64(3600)),
        2
    );
    assert_eq!(
        engine.suggest_freeze_slices(&nifty, Quantity::from_u64(3601)),
        3
    );
    assert_eq!(
        engine.suggest_freeze_slices(&nifty, Quantity::from_u64(5400)),
        3
    );
}

#[test]
fn test_auto_resume_after_cooldown() {
    let mut limits = RiskLimits::default();
    let otr_config = OtrConfig {
        max_otr: Decimal::from(1),
        min_orders_before_enforcement: 1,
        warning_threshold: Decimal::from(0),
        window_1min_ms: 60_000,
        window_5min_ms: 300_000,
        halt_cooldown_ms: 1000, // 1 second cooldown
    };
    let mut engine = RiskEngine::new(limits, otr_config);
    engine.register_algo("cooldown-algo");
    let now = Utc::now();
    let inst = InstrumentId::new(Exchange::Nse, "TCS");

    let req = OrderRequest::new(
        inst.clone(),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from_u64(1),
        "cooldown-algo",
    );

    // First order: OTR = 1/1 = 1, not > max(1), passes
    assert!(engine.check_order(&req, now).is_ok());

    // Second order without fill: OTR = 2/1 = 2 > max 1 -> breach, halt algo
    assert!(matches!(
        engine.check_order(&req, now),
        Err(RiskRefusal::OtrBreached { .. })
    ));

    // Immediately after halt, order rejected
    assert!(matches!(
        engine.check_order(&req, now),
        Err(RiskRefusal::AlgoHalted { .. })
    ));

    // After cooldown period, auto-resume should happen
    let after_cooldown = now + chrono::Duration::milliseconds(1001);
    let result = engine.check_order(&req, after_cooldown);
    // After auto-resume, order may pass or be re-halted immediately on next breach
    // Key: no panic, algo was resumed
    assert!(result.is_ok() || matches!(result, Err(RiskRefusal::OtrBreached { .. })));
}
