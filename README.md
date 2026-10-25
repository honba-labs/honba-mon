# 本場・門 honba-mon

> *The deterministic risk gate of Honba.*

A synchronous, pure pre-trade risk engine enforcing SEBI regulatory mandates, order-to-trade ratios (OTR), circuit price bands, position sizing budgets, and platform kill-switches for Indian exchanges (NSE / BSE / NFO).

## Pre-Trade Risk Gate Service Level Objective (SLO)

Under institutional algorithmic trading conditions:
- **Evaluation Latency**: $p99.99 < 15\,\mu\text{s}$ under burst loads ($p50 < 1\,\mu\text{s}$, $p99 < 5\,\mu\text{s}$)
- **Throughput Capacity**: $\ge 50,000$ order evaluations/second burst capacity
- **Zero-Allocation Hot Path**: Zero heap allocations in [`RiskEngine::check_order_fast`]
- **Pure Zero-I/O Invariant**: 100% deterministic, in-memory synchronous execution

## Features

- **Deterministic Synchronous Risk Gate**: Pure synchronous evaluation pipeline between strategy decisions and broker execution. Cannot be bypassed by LLM or automated strategies.
- **SEBI Retail Algo Compliance**: Mandatory per-`algo_id` tracking, independent audit state, and automatic circuit breakers.
- **Order-to-Trade Ratio (OTR)**: Real-time sliding-window tracking of order placements, modifications, cancellations, and fills across 1-minute, 5-minute, and cumulative session windows (both per-algo and per-scrip), with automatic cooling-off resumes.
- **Dynamic Position Sizing Risk Budget Gate**: Synchronously intercepts [`PositionSizingRequest`] against a [`RiskBudget`], enforcing aggregate portfolio risk caps ($\le \text{max\_portfolio\_risk\_pct}$, e.g. 5%), lot size multiples, and scrip freeze limits before sizing decisions can emit executable orders.
- **Exchange Circuit Limits & Freeze Slicing**: Scrip-wise `PriceBand` validation rejecting any out-of-band limit orders, plus ceiling division for freeze limit slicing (`suggest_freeze_slices`).
- **Emergency Kill-Switch**: Global platform-level emergency halt, reduce-only mode, and granular per-algo halt controls.
- **Institutional Prometheus Metrics Telemetry**: Zero-allocation metrics export via `prometheus` crate:
  - `risk_eval_duration_seconds`: Histogram of evaluation latency with sub-microsecond buckets (100ns, 250ns, 500ns, 1µs, 5µs, 15µs).
  - `risk_orders_evaluated_total{algo_id, result="approved"|"rejected"}`: Counter of evaluated decisions.
  - `risk_breaches_total{algo_id, rule_name}`: Counter of individual regulatory and risk rule violations.
  - `risk_otr_current_ratio{algo_id, scrip}`: Gauge of active rolling OTR ratio.
- **W3C Distributed Tracing**: OpenTelemetry tracing span instrumentation in `evaluate_core` propagating W3C `TraceContext` (`traceparent`, `trace_id`, `span_id`).
- **Mutation Rigor (`cargo-mutants`)**: 100% mutant kill rate on OTR sliding buffers, price bands, and kill-switches.

## Testing & Benchmarks

```bash
# Run all unit, proptest, and integration tests
cargo test

# Run microsecond Criterion latency benchmarks
cargo bench --bench risk_benchmarks

# Run mutation testing
cargo mutants -p honba-mon
```

## License
AGPL-3.0
