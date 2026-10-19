// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Order + fill DTOs - the API shape and the event payload shape
//! (one type for both, per the platform's payload-completeness rule:
//! consumers rebuild from events alone).
//!
//! (`OrderNew` packed struct), `ORDERS`/`ORDER_TRANSACTION` tables,

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
// Leaf→leaf import: raw crate path by exception. The `trade8::`
// namespace facade (principles §2.1) cannot be used inside leaf
// crates - the facade depends on this crate, so importing it here
// would create a Cargo dependency cycle. Facade paths apply to
// consumers (services/apps) only.
use trade8_fixed::{Price, Qty};
use uuid::Uuid;

use crate::enums::*;

/// A normalized order. This is the full self-contained snapshot carried by
/// `trade8:desk:order:submitted` and persisted in `desk."order"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Order {
    pub order_id: Uuid,
    pub client_order_id: String,
    pub chain_root_id: Uuid,
    /// Previous order id on a cancel-replace chain.
    pub replaces_order_id: Option<Uuid>,
    /// Sentin8:Auth sub of the acting user.
    pub usr_id: String,
    pub office_id: String,
    pub symbol_id: Uuid,
    pub symbol_name: String,
    pub market_id: Uuid,
    pub destination_id: Uuid,
    pub side: Side,
    pub order_type: OrderType,
    pub tif: TimeInForce,
    pub qty: Qty,
    /// Limit price (market orders: `None`).
    pub price: Option<Price>,
    /// Stop trigger (stop/stop-limit orders).
    pub stop_price: Option<Price>,
    pub state: OrderState,
    pub leaves_qty: Qty,
    pub filled_qty: Qty,
    /// Order-class properties + custom flags (raw `ORDER_PROPERTY` rows).
    pub flags: serde_json::Value,
    /// Manager acting on a trader's behalf (attribution).
    pub on_behalf_of: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
}

impl Order {
    /// Notional at the limit price (buying-power input). Market orders use
    /// the provided reference mark (last quote/print) - Risk enforces.
    pub fn notional(&self, mark: Option<Price>) -> Option<i128> {
        match self.price {
            Some(p) => p.checked_notional(self.qty),
            None => mark.and_then(|m| m.checked_notional(self.qty)),
        }
    }

    /// Is this a short-opening order (needs a locate)?
    pub fn needs_locate(&self) -> bool {
        self.side.is_short_opening()
    }
}

/// New-order request as submitted through the BFF/WS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NewOrderRequest {
    /// Client-generated; dedupe key for resubmit-after-reconnect.
    pub client_order_id: String,
    pub symbol_name: String,
    pub destination_id: Uuid,
    pub side: Side,
    pub order_type: OrderType,
    pub tif: TimeInForce,
    /// Decimal strings - scale resolved against the instrument.
    pub qty: String,
    pub price: Option<String>,
    pub stop_price: Option<String>,
    pub flags: Option<serde_json::Value>,
    /// Manager acting on behalf (must hold `order:cancel:admin`-class scope).
    pub on_behalf_of: Option<String>,
}

/// Cancel-replace request: cancels the referenced order and submits the
/// replacement in one chain (desk design §4.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReplaceOrderRequest {
    pub order_id: Uuid,
    pub client_order_id: String,
    pub qty: String,
    pub price: Option<String>,
}

/// A normalized fill/exec - the payload of `trade8:router:fill:executed`,
/// persisted in `router.order_transaction`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Fill {
    pub transaction_id: Uuid,
    pub order_id: Uuid,
    pub state: FillState,
    pub qty: Qty,
    pub price: Price,
    /// Fee breakdown.
    pub fees: FeeBreakdown,
    pub liquidity_flag: Option<LiquidityFlag>,
    /// Broker-assigned id (ClOrdID(11)/ExecID(17)/OUCH order token).
    pub gateway_order_id: Option<String>,
    pub seq_number: Option<u64>,
    pub at: DateTime<Utc>,
}

/// Fill terminal classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum FillState {
    #[serde(rename = "fill")]
    Fill,
    #[serde(rename = "correction")]
    Correction,
    #[serde(rename = "cancel")]
    Cancel,
    #[serde(rename = "reject")]
    Reject,
}

/// Per-fill fee breakdown - fee columns
/// (SEC/activity/gateway/execution/clearing + currency), kept as scaled
/// i128 amounts with a currency code each.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FeeBreakdown {
    pub sec_fee: Option<ScaledMoney>,
    pub activity_fee: Option<ScaledMoney>,
    pub gateway_fee: Option<ScaledMoney>,
    pub execution_fee: Option<ScaledMoney>,
    pub clearing_fee: Option<ScaledMoney>,
}

/// A scaled monetary amount + its currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ScaledMoney {
    /// Scaled integer (×10⁴ money scale by default).
    pub amount: i128,
    pub currency: String,
}

impl FeeBreakdown {
    /// Sum of present fees in the given currency (converts others at
    /// `rate_to_usd` scaled ×10⁴; unknown currencies are ignored to match
    /// conversion).
    pub fn total_in(&self, currency: &str, rates: &[(String, i128)]) -> i128 {
        let conv = |m: &Option<ScaledMoney>| -> Option<i128> {
            let m = m.as_ref()?;
            if m.currency == currency {
                return Some(m.amount);
            }
            let from: i128 = *rates
                .iter()
                .find(|(c, _)| c == &m.currency)
                .map(|(_, r)| r)?;
            let to: i128 = *rates.iter().find(|(c, _)| c == currency).map(|(_, r)| r)?;
            if from == 0 {
                return None;
            }
            Some(m.amount * to / from)
        };
        [
            conv(&self.sec_fee),
            conv(&self.activity_fee),
            conv(&self.gateway_fee),
            conv(&self.execution_fee),
            conv(&self.clearing_fee),
        ]
        .into_iter()
        .flatten()
        .sum()
    }
}

/// Blotter row (API read shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OrderSummary {
    pub order_id: Uuid,
    pub client_order_id: String,
    pub symbol_name: String,
    pub side: Side,
    pub state: OrderState,
    pub qty: Qty,
    pub leaves_qty: Qty,
    pub filled_qty: Qty,
    pub price: Option<Price>,
    pub usr_id: String,
    pub office_id: String,
    pub submitted_at: DateTime<Utc>,
}

/// Flatten instruction (desk design §5.1; ).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FlattenRequest {
    /// Trader to flatten (self by default; office scope needs manager scope).
    pub usr_id: Option<String>,
    pub office_id: Option<String>,
    /// Flatten only this symbol when set.
    pub symbol_name: Option<String>,
    /// Strategy params (typed ).
    pub strategy: FlattenStrategy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FlattenStrategy {
    pub attempts: u16,
    /// Hold time between attempts (ms).
    pub hold_time_ms: u32,
    /// Cancel working orders older than this before re-attempting (ms).
    pub cancel_after_ms: u32,
    pub priority: i16,
    /// `market`, `mid`, or `offset_ticks`.
    pub price_adjust: String,
    /// Ticks away from reference when price_adjust = offset_ticks
    ///.
    pub offset_ticks: i32,
    pub visibility: DestinationVisibility,
}

impl Default for FlattenStrategy {
    fn default() -> Self {
        FlattenStrategy {
            attempts: 3,
            hold_time_ms: 2_000,
            cancel_after_ms: 1_000,
            priority: 0,
            price_adjust: "market".into(),
            offset_ticks: 0,
            visibility: DestinationVisibility::Broadcast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::Side;

    fn order(price: Option<Price>, qty: u64) -> Order {
        Order {
            order_id: Uuid::new_v4(),
            client_order_id: "c-1".into(),
            chain_root_id: Uuid::new_v4(),
            replaces_order_id: None,
            usr_id: "u1".into(),
            office_id: "HO".into(),
            symbol_id: Uuid::new_v4(),
            symbol_name: "AAPL".into(),
            market_id: Uuid::new_v4(),
            destination_id: Uuid::new_v4(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            tif: TimeInForce::Day,
            qty: Qty::shares(qty),
            price,
            stop_price: None,
            state: OrderState::Pending,
            leaves_qty: Qty::shares(qty),
            filled_qty: Qty::shares(0),
            flags: serde_json::Value::Null,
            on_behalf_of: None,
            submitted_at: Utc::now(),
            terminal_at: None,
        }
    }

    #[test]
    fn notional_at_limit() {
        // 10.10 ×10⁴ = 101_000 (memory: classic price-scale literal).
        // Notional stays at price scale ×10⁴: 10.10 × 100 = 1010.00 → 10_100_000.
        let o = order(Some(Price::equity(101_000)), 100);
        assert_eq!(o.notional(None), Some(10_100_000));
    }

    #[test]
    fn notional_market_uses_mark() {
        let o = order(None, 100);
        assert_eq!(o.notional(None), None);
        // 5.00 ×10⁴ = 50_000; 5.00 × 100 = 500.00 → 5_000_000
        assert_eq!(o.notional(Some(Price::equity(50_000))), Some(5_000_000));
    }

    #[test]
    fn fee_conversion() {
        let fees = FeeBreakdown {
            sec_fee: Some(ScaledMoney {
                amount: 100,
                currency: "USD".into(),
            }),
            gateway_fee: Some(ScaledMoney {
                amount: 200,
                currency: "EUR".into(),
            }),
            ..Default::default()
        };
        // EUR rate 1.10 USD → 200 EUR = 220 USD-ish at ×10⁴ scale 11000/10000
        let rates = vec![("USD".to_string(), 10_000), ("EUR".to_string(), 11_000)];
        let total = fees.total_in("USD", &rates);
        assert_eq!(total, 100 + 200 * 10_000 / 11_000);
    }
}
