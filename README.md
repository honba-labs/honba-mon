# 本場・門 honba-mon

> *The deterministic risk gate of Honba.*

A synchronous, pure pre-trade risk engine enforcing SEBI regulatory mandates, order-to-trade ratios (OTR), circuit price bands, and platform kill-switches.

## Features
- **Deterministic Risk Gate**: Pure synchronous evaluation pipeline between strategy decisions and broker execution. Cannot be bypassed by LLM or automated strategies.
- **SEBI Retail Algo Compliance**: Mandatory per-`algo_id` tracking, independent audit state, and automatic circuit breakers.
- **Order-to-Trade Ratio (OTR)**: Real-time tracking of order placements, modifications, cancellations, and fills with automatic halting on threshold breach.
- **Exchange Circuit Limits**: Scrip-wise `PriceBand` validation rejecting any out-of-band limit orders.
- **Emergency Kill-Switch**: Global platform-level emergency halt, reduce-only mode, and granular per-algo halt controls.
- **Hard Risk Guardrails**: Max notional (exact Decimal), max quantity, max daily cumulative loss, and sliding-window event-time rate limiting.

## Testing
```bash
cargo test
```

## License
AGPL-3.0
