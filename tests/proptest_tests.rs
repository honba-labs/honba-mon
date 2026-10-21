//! Property-based tests for honba-mon using proptest.
//!
//! Ensures the risk engine never panics or yields false-positive approvals
//! under randomized order streams, adversarial inputs, and edge-case scenarios.

use proptest::prelude::*;
use honba_core::{Exchange, InstrumentId, OrderRequest, OrderSide, OrderType, Price, Quantity};
use honba_mon::{OtrConfig, RiskEngine, RiskLimits};
use chrono::Utc;
use rust_decimal::Decimal;

/// Arbitrary order side generator.
fn arb_order_side() -> impl Strategy<Value = OrderSide> {
    prop_oneof![
        Just(OrderSide::Buy),
        Just(OrderSide::Sell),
    ]
}

/// Arbitrary order type generator.
fn arb_order_type() -> impl Strategy<Value = OrderType> {
    prop_oneof![
        Just(OrderType::Market),
        any::<f64>().prop_map(|p| OrderType::Limit(Price::from_rupees(&format!("{:.2}", p.abs() % 10000.0)).unwrap())),
    ]
}

/// Arbitrary instrument ID generator.
fn arb_instrument_id() -> impl Strategy<Value = InstrumentId> {
    prop_oneof![
        Just(InstrumentId::new(Exchange::Nse, "NIFTY")),
        Just(InstrumentId::new(Exchange::Nse, "BANKNIFTY")),
        Just(InstrumentId::new(Exchange::Nse, "RELIANCE")),
        Just(InstrumentId::new(Exchange::Bse, "SENSEX")),
    ]
}

/// Arbitrary quantity generator (positive).
fn arb_quantity() -> impl Strategy<Value = Quantity> {
    (1u64..10000u64).prop_map(|q| Quantity::from_u64(q))
}

/// Property: No order check should ever panic, regardless of input.
proptest! {
    #[test]
    fn test_no_panic_on_random_orders(
        instrument in arb_instrument_id(),
        side in arb_order_side(),
        order_type in arb_order_type(),
        qty in arb_quantity(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        engine.register_algo(algo_id.clone());
        let req = OrderRequest::new(instrument, side, order_type, qty, algo_id);
        let now = Utc::now();

        // Should never panic - must return Ok or Err
        let _result = engine.check_order(&req, now);
    }
}

/// Property: Empty algo_id is always rejected.
proptest! {
    #[test]
    fn test_empty_algo_id_always_rejected(
        instrument in arb_instrument_id(),
        side in arb_order_side(),
        order_type in arb_order_type(),
        qty in arb_quantity(),
    ) {
        let mut engine = RiskEngine::default();
        let req = OrderRequest::new(instrument, side, order_type, qty, "");
        let now = Utc::now();

        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: Zero quantity is always rejected.
proptest! {
    #[test]
    fn test_zero_quantity_always_rejected(
        instrument in arb_instrument_id(),
        side in arb_order_side(),
        order_type in arb_order_type(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        engine.register_algo(algo_id.clone());
        let req = OrderRequest::new(instrument, side, order_type, Quantity::ZERO, algo_id);
        let now = Utc::now();

        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: Unregistered algo_id is always rejected.
proptest! {
    #[test]
    fn test_unregistered_algo_always_rejected(
        instrument in arb_instrument_id(),
        side in arb_order_side(),
        order_type in arb_order_type(),
        qty in arb_quantity(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        // Do NOT register the algo_id
        let req = OrderRequest::new(instrument, side, order_type, qty, algo_id);
        let now = Utc::now();

        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: Registered algo_id with valid order passes when no limits configured.
proptest! {
    #[test]
    fn test_valid_order_passes_with_no_limits(
        instrument in arb_instrument_id(),
        side in arb_order_side(),
        order_type in arb_order_type(),
        qty in arb_quantity(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        engine.register_algo(algo_id.clone());
        let req = OrderRequest::new(instrument, side, order_type, qty, algo_id);
        let now = Utc::now();

        let result = engine.check_order(&req, now);
        prop_assert!(result.is_ok());
    }
}

/// Property: Price band violation is consistently detected.
proptest! {
    #[test]
    fn test_price_band_violation_consistently_detected(
        instrument in arb_instrument_id(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        engine.register_algo(algo_id.clone());

        // Set a tight price band: 1000 to 2000
        let band = honba_core::PriceBand::new(
            Price::from_rupees("1000.00").unwrap(),
            Price::from_rupees("2000.00").unwrap(),
            Price::from_rupees("1500.00").unwrap(),
        );
        engine.set_price_band(instrument.clone(), band);

        // Price above band should be rejected
        let req_high = OrderRequest::new(
            instrument.clone(),
            OrderSide::Buy,
            OrderType::Limit(Price::from_rupees("2500.00").unwrap()),
            Quantity::from_u64(10),
            algo_id.clone(),
        );
        let now = Utc::now();
        let result_high = engine.check_order(&req_high, now);
        prop_assert!(result_high.is_err());

        // Price below band should be rejected
        let req_low = OrderRequest::new(
            instrument,
            OrderSide::Sell,
            OrderType::Limit(Price::from_rupees("500.00").unwrap()),
            Quantity::from_u64(10),
            algo_id,
        );
        let result_low = engine.check_order(&req_low, now);
        prop_assert!(result_low.is_err());
    }
}

/// Property: Freeze limit violation is consistently detected.
proptest! {
    #[test]
    fn test_freeze_limit_violation_consistently_detected(
        instrument in arb_instrument_id(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut limits = RiskLimits::default();
        limits.freeze_limits.insert(instrument.clone(), Quantity::from_u64(100));
        let mut engine = RiskEngine::new(limits, OtrConfig::default());
        engine.register_algo(algo_id.clone());

        // Order exceeding freeze limit should be rejected
        let req_over = OrderRequest::new(
            instrument.clone(),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from_u64(200), // > 100 freeze limit
            algo_id.clone(),
        );
        let now = Utc::now();
        let result_over = engine.check_order(&req_over, now);
        prop_assert!(result_over.is_err());

        // Order within freeze limit should pass
        let req_under = OrderRequest::new(
            instrument,
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from_u64(50), // < 100 freeze limit
            algo_id,
        );
        let result_under = engine.check_order(&req_under, now);
        prop_assert!(result_under.is_ok());
    }
}

/// Property: Daily loss breaker triggers global halt.
proptest! {
    #[test]
    fn test_daily_loss_breaker_triggers_global_halt(
        instrument in arb_instrument_id(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut limits = RiskLimits::default();
        limits.max_daily_loss = Some(Decimal::from(100000)); // ₹1,00,000
        let mut engine = RiskEngine::new(limits, OtrConfig::default());
        engine.register_algo(algo_id.clone());

        // Record loss that exceeds the limit
        engine.record_realized_loss(Decimal::from(150000)); // ₹1,50,000 > ₹1,00,000

        // Any subsequent order should be rejected by global halt
        let req = OrderRequest::new(instrument, OrderSide::Buy, OrderType::Market, Quantity::from_u64(10), algo_id);
        let now = Utc::now();
        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: OTR breach triggers automatic algo halt.
proptest! {
    #[test]
    fn test_otr_breach_triggers_algo_halt(
        instrument in arb_instrument_id(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let limits = RiskLimits::default();
        let otr_config = OtrConfig {
            max_otr: Decimal::from(5), // Low threshold for fast breach
            min_orders_before_enforcement: 2,
            warning_threshold: Decimal::from(40),
            window_1min_ms: 60_000,
            window_5min_ms: 300_000,
        };
        let mut engine = RiskEngine::new(limits, otr_config);
        engine.register_algo(algo_id.clone());

        let req = OrderRequest::new(instrument, OrderSide::Buy, OrderType::Market, Quantity::from_u64(1), algo_id.clone());
        let now = Utc::now();

        // Place 2 orders without fills -> OTR = 2/1 = 2 (below max 5)
        let _ = engine.check_order(&req, now);
        let _ = engine.check_order(&req, now);

        // Place more orders to cross OTR 5:1
        for _ in 0..4 {
            let _ = engine.check_order(&req, now);
        }

        // Now OTR should be > 5, triggering halt
        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: Market circuit breaker blocks all orders.
proptest! {
    #[test]
    fn test_market_circuit_blocks_all_orders(
        instrument in arb_instrument_id(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        engine.register_algo(algo_id.clone());

        let now = Utc::now();
        let resume_at = now + chrono::Duration::minutes(60);
        engine.trigger_market_circuit("NIFTY 50", 10, Some(resume_at), now);

        let req = OrderRequest::new(instrument, OrderSide::Buy, OrderType::Market, Quantity::from_u64(10), algo_id);
        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: Global halt blocks all orders.
proptest! {
    #[test]
    fn test_global_halt_blocks_all_orders(
        instrument in arb_instrument_id(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine = RiskEngine::default();
        engine.register_algo(algo_id.clone());

        let now = Utc::now();
        engine.halt_global("SEBI regulatory query", now);

        let req = OrderRequest::new(instrument, OrderSide::Buy, OrderType::Market, Quantity::from_u64(10), algo_id);
        let result = engine.check_order(&req, now);
        prop_assert!(result.is_err());
    }
}

/// Property: check_order_fast behaves identically to check_order for pre-registered algos.
proptest! {
    #[test]
    fn test_check_order_fast_matches_check_order(
        instrument in arb_instrument_id(),
        side in arb_order_side(),
        order_type in arb_order_type(),
        qty in arb_quantity(),
        algo_id in "[a-z0-9-]{1,20}",
    ) {
        let mut engine_fast = RiskEngine::default();
        engine_fast.register_algo(algo_id.clone());

        let mut engine_slow = RiskEngine::default();
        engine_slow.register_algo(algo_id.clone());

        let req = OrderRequest::new(instrument, side, order_type, qty, algo_id.clone());
        let now = Utc::now();

        let result_fast = engine_fast.check_order_fast(
            &algo_id,
            &req.instrument,
            req.quantity,
            req.order_type.limit_price(),
            now,
        );
        let result_slow = engine_slow.check_order(&req, now);

        // Both should agree on accept/reject
        prop_assert_eq!(result_fast.is_ok(), result_slow.is_ok());
    }
}
