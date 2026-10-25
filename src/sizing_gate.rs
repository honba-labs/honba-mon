//! Dynamic position sizing risk budget interception and validation.
//!
//! Synchronously validates [`PositionSizingRequest`] against a [`RiskBudget`]
//! before sizing decisions can emit executable orders into the broker pipeline.
//!
//! Enforces:
//! - Aggregate portfolio risk percentage caps (e.g. <= 5% equity at risk)
//! - Instrument lot size multiple conformity and positive quantity bounds
//! - Exchange scrip freeze limits
//! - Maximum position notional cash caps
//! - Maximum position share or contract quantity caps
//! - Non-negative, positive equity and entry price sanity bounds

use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

use honba_core::{
    InstrumentId, Money, PositionSizingRequest, Price, Quantity, RiskBudget, SizingMethod,
    calculate_position_size,
};

use crate::error::RiskError;

/// Structured refusal reason from the position sizing risk budget gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", content = "details", rename_all = "snake_case")]
pub enum SizingRiskRefusal {
    /// Aggregate portfolio risk percentage cap exceeded.
    PortfolioRiskCapExceeded {
        /// Requested risk percentage (e.g. 0.08 for 8%)
        requested_risk_pct: Decimal,
        /// Configured maximum portfolio risk percentage (e.g. 0.05 for 5%)
        max_portfolio_risk_pct: Decimal,
        /// Computed total capital at risk
        risk_at_stake: Money,
        /// Account equity
        equity: Money,
    },

    /// Calculated or requested quantity does not conform to instrument minimum lot size multiple.
    NonLotMultiple {
        /// Evaluated quantity
        quantity: Quantity,
        /// Instrument lot size
        lot_size: Quantity,
    },

    /// Sizing resulted in zero quantity or falls below minimum lot size.
    BelowMinimumLotSize {
        /// Evaluated quantity
        quantity: Quantity,
        /// Minimum lot size
        min_lot_size: Quantity,
    },

    /// Sized quantity breaches the exchange single-order freeze limit.
    FreezeLimitBreached {
        /// Instrument
        instrument: InstrumentId,
        /// Requested or sized quantity
        requested: Quantity,
        /// Exchange freeze limit
        limit: Quantity,
    },

    /// Sized quantity exceeds maximum configured position quantity cap.
    MaxQuantityExceeded {
        /// Instrument
        instrument: InstrumentId,
        /// Requested or sized quantity
        requested: Quantity,
        /// Maximum allowed quantity
        limit: Quantity,
    },

    /// Sized notional exceeds maximum configured position notional cap.
    MaxNotionalExceeded {
        /// Sized notional cash amount
        requested: Money,
        /// Maximum allowed notional cash amount
        limit: Money,
    },

    /// Account equity is zero, negative, or invalid for risk budgeting.
    InvalidEquity {
        /// Account equity
        equity: Money,
        /// Explanation
        reason: String,
    },

    /// Entry price is zero or negative.
    InvalidEntryPrice {
        /// Entry price
        price: Price,
    },

    /// Stop loss direction is inconsistent with position side.
    InvalidStopLoss {
        /// Stop loss price
        stop_loss: Price,
        /// Entry price
        entry_price: Price,
        /// Trade side
        side: honba_core::PositionSide,
    },

    /// Platform is in emergency halt.
    PlatformHalted {
        /// Halt reason
        reason: String,
    },

    /// Underlying sizing calculation failed.
    CalculationError(String),
}

impl fmt::Display for SizingRiskRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PortfolioRiskCapExceeded {
                requested_risk_pct,
                max_portfolio_risk_pct,
                risk_at_stake,
                equity,
            } => {
                write!(
                    f,
                    "Aggregate portfolio risk cap exceeded: requested {requested_risk_pct} > max {max_portfolio_risk_pct} (risk at stake: {} INR, equity: {} INR)",
                    risk_at_stake.amount, equity.amount
                )
            }
            Self::NonLotMultiple { quantity, lot_size } => {
                write!(
                    f,
                    "Quantity {quantity} is not a multiple of lot size {lot_size}"
                )
            }
            Self::BelowMinimumLotSize {
                quantity,
                min_lot_size,
            } => {
                write!(
                    f,
                    "Calculated quantity {quantity} is below minimum lot size {min_lot_size}"
                )
            }
            Self::FreezeLimitBreached {
                instrument,
                requested,
                limit,
            } => {
                write!(
                    f,
                    "Exchange freeze limit breached for {instrument}: requested {requested} > freeze limit {limit}"
                )
            }
            Self::MaxQuantityExceeded {
                instrument,
                requested,
                limit,
            } => {
                write!(
                    f,
                    "Maximum position quantity exceeded for {instrument}: requested {requested} > max {limit}"
                )
            }
            Self::MaxNotionalExceeded { requested, limit } => {
                write!(
                    f,
                    "Maximum position notional exceeded: requested {} > max {}",
                    requested.amount, limit.amount
                )
            }
            Self::InvalidEquity { equity, reason } => {
                write!(f, "Invalid account equity: {} ({reason})", equity.amount)
            }
            Self::InvalidEntryPrice { price } => {
                write!(f, "Invalid entry price: {price} must be strictly positive")
            }
            Self::InvalidStopLoss {
                stop_loss,
                entry_price,
                side,
            } => {
                write!(
                    f,
                    "Invalid stop loss {stop_loss} for {side:?} with entry price {entry_price}"
                )
            }
            Self::PlatformHalted { reason } => {
                write!(f, "Platform is globally halted: {reason}")
            }
            Self::CalculationError(e) => {
                write!(f, "Position sizing calculation error: {e}")
            }
        }
    }
}

impl std::error::Error for SizingRiskRefusal {}

impl SizingRiskRefusal {
    /// Return the regulatory/risk rule name identifying this refusal.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::PortfolioRiskCapExceeded { .. } => "portfolio_risk_cap_exceeded",
            Self::NonLotMultiple { .. } => "non_lot_multiple",
            Self::BelowMinimumLotSize { .. } => "below_minimum_lot_size",
            Self::FreezeLimitBreached { .. } => "freeze_limit_breached",
            Self::MaxQuantityExceeded { .. } => "max_quantity_exceeded",
            Self::MaxNotionalExceeded { .. } => "max_notional_exceeded",
            Self::InvalidEquity { .. } => "invalid_equity",
            Self::InvalidEntryPrice { .. } => "invalid_entry_price",
            Self::InvalidStopLoss { .. } => "invalid_stop_loss",
            Self::PlatformHalted { .. } => "platform_halted",
            Self::CalculationError(_) => "calculation_error",
        }
    }
}

/// Validate a position sizing request against the specified risk budget.
///
/// Ensures that:
/// 1. Entry price and equity are strictly positive.
/// 2. If stop loss is provided, its position relative to entry price matches the side.
/// 3. Aggregate portfolio risk cap is not exceeded:
///    - If sizing method specifies a fractional risk, that fraction <= `max_portfolio_risk_pct`.
///    - The resulting capital at risk (`risk_at_stake / equity`) <= `max_portfolio_risk_pct`.
/// 4. Computed executable quantity conforms strictly to `min_lot_size` multiples and is positive.
/// 5. Sized quantity does not breach exchange freeze limits (`freeze_limit_qty`).
/// 6. Sized quantity and notional do not breach `max_position_qty` or `max_position_notional`.
pub fn validate_sizing_request(
    request: &PositionSizingRequest,
    budget: &RiskBudget,
) -> Result<(), RiskError> {
    // 1. Sanity: Entry Price strictly positive
    if !request.entry_price.is_positive() {
        return Err(RiskError::SizingRefusal(
            SizingRiskRefusal::InvalidEntryPrice {
                price: request.entry_price,
            },
        ));
    }

    // 2. Sanity: Equity strictly positive
    let equity = budget.equity.amount.min(request.account_equity.amount);
    if equity <= Decimal::ZERO {
        return Err(RiskError::SizingRefusal(SizingRiskRefusal::InvalidEquity {
            equity: budget.equity,
            reason: "account equity must be strictly positive".to_string(),
        }));
    }

    // 3. Stop loss consistency with position side
    if let Some(sl) = request.stop_loss {
        if !sl.is_positive() {
            return Err(RiskError::SizingRefusal(
                SizingRiskRefusal::InvalidStopLoss {
                    stop_loss: sl,
                    entry_price: request.entry_price,
                    side: request.side,
                },
            ));
        }
        match request.side {
            honba_core::PositionSide::Long => {
                if sl.value() >= request.entry_price.value() {
                    return Err(RiskError::SizingRefusal(
                        SizingRiskRefusal::InvalidStopLoss {
                            stop_loss: sl,
                            entry_price: request.entry_price,
                            side: request.side,
                        },
                    ));
                }
            }
            honba_core::PositionSide::Short => {
                if sl.value() <= request.entry_price.value() {
                    return Err(RiskError::SizingRefusal(
                        SizingRiskRefusal::InvalidStopLoss {
                            stop_loss: sl,
                            entry_price: request.entry_price,
                            side: request.side,
                        },
                    ));
                }
            }
        }
    }

    // 4. Aggregate Portfolio Risk Cap check on sizing method specification
    let max_portfolio_risk = budget
        .max_portfolio_risk_pct
        .or(request.risk_budget.max_portfolio_risk_pct);

    if let Some(max_risk_pct) = max_portfolio_risk {
        match &request.sizing_method {
            SizingMethod::FixedFractional { risk_pct } if *risk_pct > max_risk_pct => {
                return Err(RiskError::SizingRefusal(
                    SizingRiskRefusal::PortfolioRiskCapExceeded {
                        requested_risk_pct: *risk_pct,
                        max_portfolio_risk_pct: max_risk_pct,
                        risk_at_stake: Money::new(equity * *risk_pct, budget.equity.currency),
                        equity: budget.equity,
                    },
                ));
            }
            _ => {}
        }
    }

    // 5. Compute deterministic position sizing using honba-core
    let mut aligned_req = request.clone();
    aligned_req.risk_budget = budget.clone();

    let sizing_result = calculate_position_size(&aligned_req).map_err(|e| {
        RiskError::SizingRefusal(SizingRiskRefusal::CalculationError(e.to_string()))
    })?;

    // 6. Enforce aggregate portfolio risk cap on actual risk at stake
    if let Some(max_risk_pct) = max_portfolio_risk {
        let actual_risk_pct = sizing_result.risk_at_stake.amount / equity;
        if actual_risk_pct > max_risk_pct {
            return Err(RiskError::SizingRefusal(
                SizingRiskRefusal::PortfolioRiskCapExceeded {
                    requested_risk_pct: actual_risk_pct,
                    max_portfolio_risk_pct: max_risk_pct,
                    risk_at_stake: sizing_result.risk_at_stake,
                    equity: budget.equity,
                },
            ));
        }
    }

    // 7. Validate lot size conformity & positive quantity
    let lot_size = if budget.min_lot_size.value() > Decimal::ZERO {
        budget.min_lot_size
    } else {
        Quantity::from_u64(1)
    };

    if sizing_result.quantity == Quantity::ZERO {
        return Err(RiskError::SizingRefusal(
            SizingRiskRefusal::BelowMinimumLotSize {
                quantity: sizing_result.quantity,
                min_lot_size: lot_size,
            },
        ));
    }

    let lot_dec = lot_size.value();
    if (sizing_result.quantity.value() % lot_dec) != Decimal::ZERO {
        return Err(RiskError::SizingRefusal(
            SizingRiskRefusal::NonLotMultiple {
                quantity: sizing_result.quantity,
                lot_size,
            },
        ));
    }

    // 8. Validate exchange freeze limit
    if let Some(freeze_limit) = budget
        .freeze_limit_qty
        .or(request.risk_budget.freeze_limit_qty)
    {
        if sizing_result.quantity.value() > freeze_limit.value() {
            return Err(RiskError::SizingRefusal(
                SizingRiskRefusal::FreezeLimitBreached {
                    instrument: request.instrument.clone(),
                    requested: sizing_result.quantity,
                    limit: freeze_limit,
                },
            ));
        }
    }

    // 9. Max quantity cap
    if let Some(max_qty) = budget
        .max_position_qty
        .or(request.risk_budget.max_position_qty)
    {
        if sizing_result.quantity.value() > max_qty.value() {
            return Err(RiskError::SizingRefusal(
                SizingRiskRefusal::MaxQuantityExceeded {
                    instrument: request.instrument.clone(),
                    requested: sizing_result.quantity,
                    limit: max_qty,
                },
            ));
        }
    }

    // 10. Max notional cap
    if let Some(max_notional) = budget
        .max_position_notional
        .or(request.risk_budget.max_position_notional)
    {
        if sizing_result.allocated_notional.amount > max_notional.amount {
            return Err(RiskError::SizingRefusal(
                SizingRiskRefusal::MaxNotionalExceeded {
                    requested: sizing_result.allocated_notional,
                    limit: max_notional,
                },
            ));
        }
    }

    Ok(())
}
