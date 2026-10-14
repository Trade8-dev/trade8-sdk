// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Risk authorize contract — the Desk↔Risk hot-path request/verdict
//! (risk design §2, §4). One batch call, itemized trace, µs budget.
//!

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{RiskLimitKind, TraderState};

/// The order-mutation context Risk needs to authorize. Carried in full so
/// Risk stays stateless on the request path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizeRequest {
    pub env_id: Uuid,
    /// The acting user (Auth sub) — limits/trader-state scope key.
    pub usr_id: String,
    pub office_id: String,
    pub symbol_id: Uuid,
    pub symbol_name: String,
    pub market_id: Uuid,
    pub destination_id: Uuid,
    pub side_char: char,
    /// Signed quantity (shares). Short opens are positive qty with
    /// side 'S'/'T'.
    pub qty: u64,
    /// Limit price scaled (None for market — Risk uses the mark).
    pub price_raw: Option<i128>,
    pub price_scale: u8,
    /// Last consolidated mark (Feed snapshot via Desk).
    pub mark_raw: Option<i128>,
    /// Order intent for state checks: `open` | `close` | `cancel` | `replace`.
    pub intent: Intent,
    /// Existing position for this (trader, symbol) — the position-limit
    /// inputs (Risk keeps authoritative copies; Desk passes its view for
    /// the verdict trace).
    pub current_position: i64,
    /// Whether the symbol/market is currently halted (from Desk catalog
    /// + Feed state).
    pub halted: bool,
    /// Uptick context: last print vs prior print on a short (Feed).
    pub last_print_raw: Option<i128>,
    pub prior_print_raw: Option<i128>,
    /// Order type wire string (order-class checks per
    /// destination). None when the caller omits it.
    #[serde(default)]
    pub order_type: Option<String>,
    /// TIF wire string.
    #[serde(default)]
    pub tif: Option<String>,
    /// Cancel intent target state (cancel-denial window): true when
    /// this authorize is a cancel of a working order.
    #[serde(default)]
    pub is_cancel: bool,
}

/// What the caller is asking to do (trader-state + period rules branch on
/// this).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "close")]
    Close,
    #[serde(rename = "cancel")]
    Cancel,
    #[serde(rename = "replace")]
    Replace,
}

/// One check outcome in the verdict trace (persisted on reject for the
/// audit trail — desk design §4.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    /// Which check family (risk design §2: 1..10).
    pub check: CheckKind,
    pub passed: bool,
    /// Structured context: limit value, requested value, etc.
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckKind {
    #[serde(rename = "trader_state")]
    TraderState,
    #[serde(rename = "buying_power")]
    BuyingPower,
    #[serde(rename = "max_order_shares")]
    MaxOrderShares,
    #[serde(rename = "max_position_shares")]
    MaxPositionShares,
    #[serde(rename = "max_position_value")]
    MaxPositionValue,
    #[serde(rename = "max_open_orders")]
    MaxOpenOrders,
    #[serde(rename = "symbol_restriction")]
    SymbolRestriction,
    #[serde(rename = "destination_restriction")]
    DestinationRestriction,
    #[serde(rename = "market_period")]
    MarketPeriod,
    #[serde(rename = "order_class")]
    OrderClass,
    #[serde(rename = "short_locate")]
    ShortLocate,
    #[serde(rename = "uptick_rule")]
    UptickRule,
}

/// The verdict — one `authorize` call, all checks, itemized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizeVerdict {
    pub allowed: bool,
    /// The first failing check's mapped code (None when allowed).
    pub reject_code: Option<String>,
    pub trace: Vec<CheckResult>,
    /// Trader state observed at authorize time (Desk surfaces it).
    pub trader_state: TraderState,
    /// Effective buying power after this order (informational on allow).
    pub remaining_buying_power: Option<i128>,
}

impl AuthorizeVerdict {
    pub fn deny(code: &str, trace: Vec<CheckResult>, state: TraderState) -> Self {
        AuthorizeVerdict {
            allowed: false,
            reject_code: Some(code.into()),
            trace,
            trader_state: state,
            remaining_buying_power: None,
        }
    }
    pub fn allow(trace: Vec<CheckResult>, state: TraderState, remaining_bp: Option<i128>) -> Self {
        AuthorizeVerdict {
            allowed: true,
            reject_code: None,
            trace,
            trader_state: state,
            remaining_buying_power: remaining_bp,
        }
    }
}

/// A limit row as loaded into Risk's snapshot cache
/// (risk design §4 — arc-swap snapshots, no DB on the authorize path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LimitRow {
    pub usr_id: Option<String>,
    pub office_id: Option<String>,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub kind: RiskLimitKind,
    /// Scaled value (shares ×1, money ×10⁴).
    pub value: i128,
    pub override_value: Option<i128>,
    pub disabled: bool,
}

impl LimitRow {
    /// Effective value (override wins when set — ).
    pub fn effective(&self) -> i128 {
        self.override_value.unwrap_or(self.value)
    }
    /// Does this row bind the given trader? A row with neither usr nor
    /// office binds everyone in the env (env-level default).
    pub fn binds(&self, usr: &str, office: &str) -> bool {
        match (&self.usr_id, &self.office_id) {
            (Some(u), _) => u == usr,
            (None, Some(o)) => o == office,
            (None, None) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_binding_rules() {
        let env_default = LimitRow {
            usr_id: None,
            office_id: None,
            asset_class: None,
            currency: None,
            kind: RiskLimitKind::BuyingPower,
            value: 1_000,
            override_value: None,
            disabled: false,
        };
        assert!(env_default.binds("any", "any"));
        let user_row = LimitRow {
            usr_id: Some("u1".into()),
            office_id: None,
            asset_class: None,
            currency: None,
            kind: RiskLimitKind::MaxOrderShares,
            value: 100,
            override_value: Some(50),
            disabled: false,
        };
        assert!(user_row.binds("u1", "X"));
        assert!(!user_row.binds("u2", "X"));
        assert_eq!(user_row.effective(), 50); // override wins
        let office_row = LimitRow {
            usr_id: None,
            office_id: Some("HO".into()),
            asset_class: None,
            currency: None,
            kind: RiskLimitKind::MaxOpenOrders,
            value: 10,
            override_value: None,
            disabled: false,
        };
        assert!(office_row.binds("u9", "HO"));
        assert!(!office_row.binds("u9", "BR"));
    }
}
