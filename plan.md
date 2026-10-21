# Master Plan: 本場・門 honba-mon

> **The regulatory gate of the trading engine.**  
> Authoritative, synchronous pre-trade risk engine and SEBI compliance validator. Evaluates all order placements, cancellations, and modifications before they can touch network adapters or the broker execution pipeline. Enforces strict zero-I/O domain purity, sub-15 microsecond latency, and mandatory Indian exchange regulatory constraints (NSE, BSE, NFO).

---

## 0. Executive Summary & Architectural Scope

`honba-mon` ("Gate") protects accounts and trading infrastructure from runaway algorithms, fat-finger errors, and regulatory penalties. It operates as a synchronous pre-trade gatekeeper:

```text
 STRATEGY ACTOR (Python / Rust)
              │
              ▼ Candidate Decision
      [ honba-mon: RiskEngine ]  <-- Synchronous Pre-Trade Gate (< 15 µs)
      - SEBI Algo ID Valid?
      - OTR Ratio within Sliding Threshold?
      - Limit Price inside Scrip Price Band?
      - Quantity <= Exchange Freeze Limit?
      - Account Daily Loss Limit Breached?
              │
      ┌───────┴───────┐
   APPROVED        REJECTED
      │               │
      ▼               ▼
ExecutionActor    Logged to Audit Stream &
(Outbound Wire)   Returned as OrderRejectReason
```

---

## 1. Regulatory Tenets & Key Invariants

1. **Synchronous Interception**: No order can leave the system without an explicit `RiskApproved` verdict from `honba-mon`.
2. **Mandatory SEBI `algo_id`**: In compliance with SEBI Algorithmic Trading regulations, every automated order must carry a registered `algo_id`.
3. **Order-to-Trade Ratio (OTR) Circuit Breakers**: Tracks orders, modifications, cancellations, and fills across sliding time windows. If the ratio exceeds exchange limits (e.g. > 50:1), the breaker trips, placing the strategy in reduce-only / kill-switch mode.
4. **Exchange Price Bands**: Scrip-wise circuit filters (2%, 5%, 10%, 20%) are updated dynamically from market quote ticks. Orders dispatched outside active price bands are rejected before transmission.
5. **Freeze Limit Enforcement**: Orders exceeding exchange single-order quantity ceilings (e.g. NIFTY 1,800, BANKNIFTY 900) are intercepted and rejected (or routed to `honba-te-slicer`).

---

## 2. Phase-by-Phase Implementation Roadmap

### Phase 1: Core Type Unification & Legacy Risk Migration (M0) — Week 1–2
- [x] **1.1 Canonical Type Migration** (COMPLETED)
  - [x] Upgrade all risk limits to use `honba_core::{Price, Quantity, Money, InstrumentId}`.
  - [x] Eliminate all floating-point numbers; enforce `rust_decimal::Decimal`.
- [x] **1.2 Legacy Rule Absorption** (COMPLETED)
  - [x] Merge `honba-kokoro-risk::RiskLimits` into `honba-mon::RiskLimits`.
  - [x] **1.3 Cross-Repository Risk Migration Blueprint**: Formulate comprehensive migration plan in `honba-docs/docs/architecture/migration_plan_kokoro_risk_to_mon.md` to guide `honba-ya`, `honba-tamashii`, `honba-nou`, `honba-me`, and `honba-te` from deleted `honba-kokoro-risk` to `honba-mon`.
  - [x] Verify zero-error compilation across workspace.

### Phase 2: SEBI Compliance & Regulatory Circuit Breakers (M1) — Week 3–4
- [ ] **2.1 SEBI Algo ID Registry**
  - [ ] Implement `AlgoIdRegistry` verifying algo ID registration and active status.
  - [ ] Reject any order carrying empty or unregistered `algo_id`.
- [ ] **2.2 Order-to-Trade Ratio (OTR) Sliding-Window Tracker**
  - [ ] Implement circular ring-buffer tracking order events over 1-minute, 5-minute, and session windows per `algo_id`.
  - [ ] Implement automated circuit tripping when OTR exceeds penalty thresholds (e.g. > 50:1).
  - [ ] Support configurable grace periods and soft warning thresholds.
- [ ] **2.3 Dynamic Exchange Price Band Monitor**
  - [ ] Ingest live quote ticks from `honba-me` and update scrip operational price bands.
  - [ ] Implement `validate_price_band(&Price, &PriceBand) -> Result<(), RiskError>`.

### Phase 3: Capital, Margin & Position Collars (M2) — Week 5–6
- [x] **3.1 Single-Order Notional & Freeze Limits**
  - [x] Implement maximum notional collar per order (e.g. ₹25,00,000 max single order).
  - [x] Implement exchange freeze limit enforcement (NIFTY: 1,800, BANKNIFTY: 900).
- [x] **3.2 Account-Level Maximum Daily Loss & Drawdown Breaker**
  - [x] Track cumulative daily realized + unrealized loss from `honba-kokoro-portfolio`.
  - [x] Trip global emergency kill-switch when daily loss reaches threshold, cancelling all open orders and blocking new submissions.
- [x] **3.3 Position Concentration & Leverage Caps**
  - [x] Enforce maximum single-scrip exposure as a percentage of total portfolio equity.
  - [x] Limit gross leverage for intraday (`MIS`) products.

### Phase 4: Actor Runtime Integration & Performance Benchmarks (M3) — Week 7–8
- [x] **4.1 Wire into `honba-ya-actors::RiskActor`**
  - [x] Expose zero-allocation evaluation methods to `RiskActor`.
  - [x] Emit structured `RiskBreachEvent` on rejections for the persistence audit stream.
- [x] **4.2 Microsecond Latency Optimization**
  - [x] Optimize critical evaluation path with Criterion benchmarks (target: $< 15\,\mu\text{s}$ per order check).
  - [x] Ensure zero heap allocations during the pre-trade evaluation loop.

## 3. Verification & Benchmark Suite

- [x] `cargo test -p honba-mon`: Comprehensive unit tests for OTR calculation, price band checking, and freeze limits.
- [x] `proptest`: Randomized order stream fuzzing ensuring risk engine never panics or yields false-positive approvals.
- [x] Criterion micro-benchmarks asserting $< 15\,\mu\text{s}$ evaluation latency under high load.

---

### Phase 5: Production Hardening & Cross-Repository Integration (M4) — Week 9–10
- [x] **5.1 Property-Based Testing with Proptest**: 16 properties covering no-panic guarantees, empty algo_id rejection, zero quantity rejection, unregistered algo rejection, valid order acceptance, price band consistency, freeze limit consistency, daily loss breaker, OTR breach halt, market circuit block, global halt block, check_order_fast equivalence, scrip OTR breach, freeze slice suggestion, auto-resume cooldown, no-freeze-limit returns one.
- [x] **5.2 Criterion Micro-Benchmarks**: All benchmarks 125-212 ns per individual check (well under 15µs target).
- [x] **5.3 Cross-Repository Integration**: honba-ya-actors RiskActor wiring, RiskBreachEvent emission, honba-ya-persistence breach event persistence, honba-nou-strategy dependency fix, honba-ya-api-rest rsa version fix.

### Phase 6: Advanced Regulatory Compliance & Order Slicing Integration (M5) — Week 11–12
- [x] **6.1 Per-Scrip OTR Circuit Breaker**
  - [x] Per-instrument OTR tracking via `scrip_trackers` HashMap
  - [x] Scrip OTR check integrated into `evaluate_core` pipeline (check #3.5)
  - [x] New `ScripOtrBreached` refusal variant with instrument, current_otr, max_otr
  - [x] Automatic algo halt on scrip OTR breach
- [x] **6.2 Freeze Limit Order Slicing Integration**
  - [x] `suggest_freeze_slices()` method computes minimum child orders needed
  - [x] Ceiling division: ceil(quantity / freeze_limit)
  - [x] Returns 1 when no freeze limit configured or order within limit
- [x] **6.3 Circuit Breaker Auto-Resume**
  - [x] `halt_cooldown_ms` field added to `OtrConfig`
  - [x] Automatic algo resume after cooling-off period expires
  - [x] Resume logic integrated into `evaluate_core` algo halt check
- [x] **6.4 Zero-Allocation Hot Path Fixes**
  - [x] `record_order_placement` called before OTR checks (correct ordering)
  - [x] Eliminated double-counting of scrip tracker events

---
