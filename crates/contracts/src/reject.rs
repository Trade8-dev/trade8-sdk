// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Reject-code catalog - typed.
//!
//! Registered into `sentin8_vertical_error_codes` at enroll via the
//! service registration; surfaced to clients as `{code, message, params}`.

use serde::{Deserialize, Serialize};

/// Every order-rejection reason the Desk can emit. Numeric codes keep a
/// stable ordering; the string form is the platform registration key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
pub enum RejectCode {
    // structural (Desk, desk design §4.2 step 1)
    #[serde(rename = "trade8_symbol_unknown")]
    SymbolUnknown, // 1
    #[serde(rename = "trade8_symbol_inactive")]
    SymbolInactive, // 2
    #[serde(rename = "trade8_destination_invalid")]
    DestinationInvalid, // 3
    #[serde(rename = "trade8_side_invalid")]
    SideInvalid, // 4
    #[serde(rename = "trade8_qty_invalid")]
    QtyInvalid, // 5
    #[serde(rename = "trade8_price_invalid")]
    PriceInvalid, // 6
    #[serde(rename = "trade8_price_off_tick")]
    PriceOffTick, // 7
    #[serde(rename = "trade8_type_needs_price")]
    TypeNeedsPrice, // 8
    #[serde(rename = "trade8_duplicate_client_order_id")]
    DuplicateClientOrderId, // 9

    // risk denials (Risk verdicts, risk design §2)
    #[serde(rename = "trade8_risk_trader_inactive")]
    RiskTraderInactive, // 10
    #[serde(rename = "trade8_risk_trader_disabled")]
    RiskTraderDisabled, // 11
    #[serde(rename = "trade8_risk_closing_only")]
    RiskClosingOnly, // 12
    #[serde(rename = "trade8_risk_buying_power")]
    RiskBuyingPower, // 13
    #[serde(rename = "trade8_risk_max_order_shares")]
    RiskMaxOrderShares, // 14
    #[serde(rename = "trade8_risk_max_position_shares")]
    RiskMaxPositionShares, // 15
    #[serde(rename = "trade8_risk_max_position_value")]
    RiskMaxPositionValue, // 16
    #[serde(rename = "trade8_risk_max_open_orders")]
    RiskMaxOpenOrders, // 17
    #[serde(rename = "trade8_risk_symbol_blocked")]
    RiskSymbolBlocked, // 18
    #[serde(rename = "trade8_risk_symbol_not_allowed")]
    RiskSymbolNotAllowed, // 19
    #[serde(rename = "trade8_risk_destination_blocked")]
    RiskDestinationBlocked, // 20
    #[serde(rename = "trade8_risk_market_blocked")]
    RiskMarketBlocked, // 21
    #[serde(rename = "trade8_risk_period_blocked")]
    RiskPeriodBlocked, // 22
    #[serde(rename = "trade8_risk_cancel_denied")]
    RiskCancelDenied, // 23
    #[serde(rename = "trade8_risk_type_not_allowed")]
    RiskTypeNotAllowed, // 24
    #[serde(rename = "trade8_risk_tif_not_allowed")]
    RiskTifNotAllowed, // 25
    #[serde(rename = "trade8_risk_locate_unavailable")]
    RiskLocateUnavailable, // 26
    #[serde(rename = "trade8_risk_uptick_rule")]
    RiskUptickRule, // 27

    // state / session (Desk)
    #[serde(rename = "trade8_session_not_open")]
    SessionNotOpen, // 28
    #[serde(rename = "trade8_order_not_cancellable")]
    OrderNotCancellable, // 29
    #[serde(rename = "trade8_order_not_found")]
    OrderNotFound, // 30
    #[serde(rename = "trade8_market_halted")]
    MarketHalted, // 31
}

impl RejectCode {
    /// Parse the wire string (the serde rename) back to the enum - Risk
    /// verdict codes map to the Desk catalog. Unknown → None.
    pub fn from_wire(s: &str) -> Option<RejectCode> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }

    /// id (stable wire truth).
    pub fn registry_id(self) -> i32 {
        use RejectCode::*;
        match self {
            SymbolUnknown => 1,
            SymbolInactive => 2,
            DestinationInvalid => 3,
            SideInvalid => 4,
            QtyInvalid => 5,
            PriceInvalid => 6,
            PriceOffTick => 7,
            TypeNeedsPrice => 8,
            DuplicateClientOrderId => 9,
            RiskTraderInactive => 10,
            RiskTraderDisabled => 11,
            RiskClosingOnly => 12,
            RiskBuyingPower => 13,
            RiskMaxOrderShares => 14,
            RiskMaxPositionShares => 15,
            RiskMaxPositionValue => 16,
            RiskMaxOpenOrders => 17,
            RiskSymbolBlocked => 18,
            RiskSymbolNotAllowed => 19,
            RiskDestinationBlocked => 20,
            RiskMarketBlocked => 21,
            RiskPeriodBlocked => 22,
            RiskCancelDenied => 23,
            RiskTypeNotAllowed => 24,
            RiskTifNotAllowed => 25,
            RiskLocateUnavailable => 26,
            RiskUptickRule => 27,
            SessionNotOpen => 28,
            OrderNotCancellable => 29,
            OrderNotFound => 30,
            MarketHalted => 31,
        }
    }

    /// Human message.
    pub fn message(self) -> &'static str {
        use RejectCode::*;
        match self {
            SymbolUnknown => "Unknown symbol",
            SymbolInactive => "Symbol is not active",
            DestinationInvalid => "Destination not valid for symbol's market",
            SideInvalid => "Invalid side",
            QtyInvalid => "Invalid quantity",
            PriceInvalid => "Invalid price",
            PriceOffTick => "Price not on valid tick",
            TypeNeedsPrice => "Order type requires a price",
            DuplicateClientOrderId => "Duplicate client order id",
            RiskTraderInactive => "Trader is inactive",
            RiskTraderDisabled => "Trader is disabled",
            RiskClosingOnly => "Trader is closing-only",
            RiskBuyingPower => "Insufficient buying power",
            RiskMaxOrderShares => "Order exceeds max order shares",
            RiskMaxPositionShares => "Order exceeds max position shares",
            RiskMaxPositionValue => "Order exceeds max position value",
            RiskMaxOpenOrders => "Max open orders reached",
            RiskSymbolBlocked => "Symbol is blocked",
            RiskSymbolNotAllowed => "Symbol not on allowed list",
            RiskDestinationBlocked => "Destination blocked for user or office",
            RiskMarketBlocked => "Market blocked",
            RiskPeriodBlocked => "Trading period blocked",
            RiskCancelDenied => "Cancel denied in this period",
            RiskTypeNotAllowed => "Order type not allowed on destination",
            RiskTifNotAllowed => "Time-in-force not allowed on destination",
            RiskLocateUnavailable => "No borrow locate available",
            RiskUptickRule => "Uptick rule violated",
            SessionNotOpen => "Trading session is not open",
            OrderNotCancellable => "Order is not cancellable",
            OrderNotFound => "Order not found",
            MarketHalted => "Market is halted",
        }
    }
}

/// A rejection payload - carried by `trade8:desk:order:rejected` and the
/// API 422 body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Rejection {
    pub code: RejectCode,
    pub message: String,
    /// Structured context (limit values, tick expected, etc).
    pub params: serde_json::Value,
}

impl Rejection {
    pub fn new(code: RejectCode) -> Self {
        Rejection {
            code,
            message: code.message().to_string(),
            params: serde_json::Value::Null,
        }
    }
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_unique_and_stable() {
        // every distinct
        let all = [
            RejectCode::SymbolUnknown,
            RejectCode::SymbolInactive,
            RejectCode::DestinationInvalid,
            RejectCode::SideInvalid,
            RejectCode::QtyInvalid,
            RejectCode::PriceInvalid,
            RejectCode::PriceOffTick,
            RejectCode::TypeNeedsPrice,
            RejectCode::DuplicateClientOrderId,
            RejectCode::RiskTraderInactive,
            RejectCode::RiskTraderDisabled,
            RejectCode::RiskClosingOnly,
            RejectCode::RiskBuyingPower,
            RejectCode::RiskMaxOrderShares,
            RejectCode::RiskMaxPositionShares,
            RejectCode::RiskMaxPositionValue,
            RejectCode::RiskMaxOpenOrders,
            RejectCode::RiskSymbolBlocked,
            RejectCode::RiskSymbolNotAllowed,
            RejectCode::RiskDestinationBlocked,
            RejectCode::RiskMarketBlocked,
            RejectCode::RiskPeriodBlocked,
            RejectCode::RiskCancelDenied,
            RejectCode::RiskTypeNotAllowed,
            RejectCode::RiskTifNotAllowed,
            RejectCode::RiskLocateUnavailable,
            RejectCode::RiskUptickRule,
            RejectCode::SessionNotOpen,
            RejectCode::OrderNotCancellable,
            RejectCode::OrderNotFound,
            RejectCode::MarketHalted,
        ];
        let mut ids: Vec<i32> = all.iter().map(|c| c.registry_id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        assert!(all.iter().all(|c| !c.message().is_empty()));
    }

    #[test]
    fn rejection_serializes() {
        let r = Rejection::new(RejectCode::RiskBuyingPower)
            .with_params(serde_json::json!({"available": 100, "needed": 200}));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("trade8_risk_buying_power"));
        assert!(s.contains("Insufficient buying power"));
    }
}
