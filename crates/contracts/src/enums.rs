// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Shared char-code enums for the trade8 domain.
//!
//! Wire values are the durable contract - order sides `B/S/T`, trader
//! states ACT/CLS/RSK/CMP/DIS/INA, destination visibility `B/D/H`, and the
//! feed-type codes - encoded verbatim by the FIX/OUCH codecs and the event
//! payloads.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Side
// ---------------------------------------------------------------------------

/// Order side. `T` (trade both sides / short-exempt cross) is preserved
/// from .
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    #[serde(rename = "b")]
    Buy,
    #[serde(rename = "s")]
    Sell,
    #[serde(rename = "t")]
    Trade, // short-exempt / cross side
}

impl Side {
    pub fn as_char(self) -> char {
        match self {
            Side::Buy => 'B',
            Side::Sell => 'S',
            Side::Trade => 'T',
        }
    }
    pub fn from_char(c: char) -> Option<Self> {
        match c.to_ascii_uppercase() {
            'B' => Some(Side::Buy),
            'S' => Some(Side::Sell),
            'T' => Some(Side::Trade),
            _ => None,
        }
    }
    /// True when the side increases a short position (needs locate).
    pub fn is_short_opening(self) -> bool {
        self == Side::Sell || self == Side::Trade
    }
}

// ---------------------------------------------------------------------------
// Order type / TIF
// ---------------------------------------------------------------------------

/// Order types` on ORDERS + OrderNew).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    #[serde(rename = "1")]
    Market,
    #[serde(rename = "2")]
    Limit,
    #[serde(rename = "3")]
    Stop, // stop-loss, becomes market on trigger
    #[serde(rename = "4")]
    StopLimit, // stop-limit
    #[serde(rename = "9")]
    Cross, // cross
}

impl OrderType {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            '1' => Some(OrderType::Market),
            '2' => Some(OrderType::Limit),
            '3' => Some(OrderType::Stop),
            '4' => Some(OrderType::StopLimit),
            '9' => Some(OrderType::Cross),
            _ => None,
        }
    }
    /// Wire/DB string form (the serde rename - code).
    pub fn serde_str(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "2".into())
    }
    /// Parse the wire/DB string form (deliberately not the
    /// `FromStr::from_str` trait - the wire parse is fallible-with-None
    /// by design and callers disambiguate via this name).
    pub fn parse_wire(s: &str) -> Option<Self> {
        let c = s.chars().next()?;
        Self::from_char(c)
    }
    pub fn needs_price(self) -> bool {
        matches!(self, OrderType::Limit | OrderType::StopLimit)
    }
}

/// Time-in-force: `overnight` and `on_close` survive sessions; `day` sweeps at session close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeInForce {
    #[serde(rename = "0")]
    Day,
    #[serde(rename = "1")]
    Gtc,
    #[serde(rename = "3")]
    Ioc,
    #[serde(rename = "4")]
    Fok,
    #[serde(rename = "6")]
    Gtd,
    #[serde(rename = "ON")]
    Overnight,
    #[serde(rename = "OC")]
    OnClose,
}

impl TimeInForce {
    pub fn sweeps_at_close(self) -> bool {
        self == TimeInForce::Day
    }
    pub fn parks_overnight(self) -> bool {
        self == TimeInForce::Overnight
    }
}

// ---------------------------------------------------------------------------
// Order state (Desk state machine, desk design §4.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderState {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "routed")]
    Routed,
    #[serde(rename = "partially_filled")]
    PartiallyFilled,
    #[serde(rename = "filled")]
    Filled,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "replaced")]
    Replaced,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "overnight_parked")]
    OvernightParked,
}

impl OrderState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Replaced
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    /// Legal transitions (desk design §4.1). Property-tested.
    pub fn can_transition(self, to: OrderState) -> bool {
        use OrderState::*;
        match self {
            Pending => matches!(to, Accepted | Rejected | Cancelled | Expired),
            Accepted => matches!(
                to,
                Routed | Rejected | Cancelled | PartiallyFilled | Expired
            ),
            Routed => matches!(
                to,
                PartiallyFilled | Filled | Cancelled | Rejected | Expired
            ),
            PartiallyFilled => matches!(to, Filled | Cancelled | Replaced | PartiallyFilled),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Trader state
// ---------------------------------------------------------------------------

/// Trading state of a trader, owned by Risk (not identity).
/// CMP→DisabledByCompliance, DIS→Disabled, INA→Inactive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraderState {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "closing_only")]
    ClosingOnly,
    #[serde(rename = "disabled_by_risk")]
    DisabledByRisk,
    #[serde(rename = "disabled_by_compliance")]
    DisabledByCompliance,
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "inactive")]
    Inactive,
}

impl TraderState {
    /// Can open new positions at all?
    pub fn can_open(self) -> bool {
        self == TraderState::Active
    }
    /// Can reduce/close existing positions?
    pub fn can_close(self) -> bool {
        matches!(
            self,
            TraderState::Active | TraderState::ClosingOnly | TraderState::Inactive
        )
    }
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "ACT" => Some(TraderState::Active),
            "CLS" => Some(TraderState::ClosingOnly),
            "RSK" => Some(TraderState::DisabledByRisk),
            "CMP" => Some(TraderState::DisabledByCompliance),
            "DIS" => Some(TraderState::Disabled),
            "INA" => Some(TraderState::Inactive),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Session state (desk design §4.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionState {
    #[serde(rename = "preopen")]
    Preopen,
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "close_pending")]
    ClosePending,
    #[serde(rename = "closed")]
    Closed,
}

impl SessionState {
    pub fn accepts_orders(self) -> bool {
        self == SessionState::Open
    }
    pub fn accepts_cancels(self) -> bool {
        matches!(self, SessionState::Open | SessionState::ClosePending)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Preopen => "preopen",
            SessionState::Open => "open",
            SessionState::ClosePending => "close_pending",
            SessionState::Closed => "closed",
        }
    }
    pub fn parse(s: &str) -> Option<SessionState> {
        Some(match s {
            "preopen" => SessionState::Preopen,
            "open" => SessionState::Open,
            "close_pending" => SessionState::ClosePending,
            "closed" => SessionState::Closed,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Destination (router design §4.2)
// ---------------------------------------------------------------------------

/// Broker order-entry protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DestinationProtocol {
    #[serde(rename = "fix42")]
    Fix42,
    #[serde(rename = "fix44")]
    Fix44,
    #[serde(rename = "ouch31")]
    Ouch31,
}

/// Destination visibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DestinationVisibility {
    #[serde(rename = "b")]
    Broadcast, // visible
    #[serde(rename = "d")]
    Dark,
    #[serde(rename = "h")]
    Hidden,
}

/// Gateway environment mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GatewayMode {
    #[serde(rename = "live")]
    Live,
    #[serde(rename = "shadow")]
    Shadow, // mirrors order flow without sending
}

// ---------------------------------------------------------------------------
// Fill liquidity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LiquidityFlag {
    #[serde(rename = "A")]
    Added,
    #[serde(rename = "R")]
    Removed,
    #[serde(rename = "X")]
    Mixed,
}

// ---------------------------------------------------------------------------
// L2 feed type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeedType {
    #[serde(rename = "book")]
    Book,
    #[serde(rename = "attributable")]
    AttributableBook,
    #[serde(rename = "venue")]
    VenueBook,
    #[serde(rename = "top")]
    TopOfBook,
    #[serde(rename = "consolidated")]
    ConsolidatedBook,
    #[serde(rename = "market_maker")]
    MarketMaker,
    #[serde(rename = "odd_lot")]
    OddLotBook,
}

// ---------------------------------------------------------------------------
// Asset class
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetClass {
    #[serde(rename = "equity")]
    Equity,
    #[serde(rename = "future")]
    Future,
    #[serde(rename = "fx")]
    Fx,
    #[serde(rename = "crypto")]
    Crypto,
}

// ---------------------------------------------------------------------------
// Risk limit kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLimitKind {
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
    #[serde(rename = "max_notional")]
    MaxNotional,
}

// ---------------------------------------------------------------------------
// Risk levels / actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskAction {
    #[serde(rename = "notify")]
    Notify,
    #[serde(rename = "close_only")]
    CloseOnly,
    #[serde(rename = "disable")]
    Disable,
    #[serde(rename = "flatten")]
    Flatten,
}

// ---------------------------------------------------------------------------
// Borrow category
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BorrowCategory {
    #[serde(rename = "etb")]
    Etb,
    #[serde(rename = "htb")]
    Htb,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_char_roundtrip() {
        for c in ['B', 'S', 'T'] {
            assert_eq!(Side::from_char(c).unwrap().as_char(), c);
        }
        assert!(Side::from_char('x').is_none());
        assert!(Side::Sell.is_short_opening());
    }

    #[test]
    fn order_type_wire_strings() {
        assert_eq!(OrderType::Market.serde_str(), "1");
        assert_eq!(OrderType::Limit.serde_str(), "2");
        assert_eq!(OrderType::parse_wire("4"), Some(OrderType::StopLimit));
        assert_eq!(OrderType::parse_wire("bogus"), None);
    }

    #[test]
    fn state_machine_legality() {
        use OrderState::*;
        // legal spine
        assert!(Pending.can_transition(Accepted));
        assert!(Accepted.can_transition(Routed));
        assert!(Routed.can_transition(PartiallyFilled));
        assert!(PartiallyFilled.can_transition(Filled));
        assert!(PartiallyFilled.can_transition(Replaced));
        // illegal jumps
        assert!(!Pending.can_transition(Filled));
        assert!(!Filled.can_transition(Cancelled));
        assert!(!Accepted.can_transition(Pending));
        // terminal states
        assert!(Filled.is_terminal());
        assert!(Replaced.is_terminal());
        assert!(!PartiallyFilled.is_terminal());
    }

    #[test]
    fn trader_state_wire_mapping() {
        assert_eq!(
            TraderState::from_wire("CLS"),
            Some(TraderState::ClosingOnly)
        );
        assert_eq!(TraderState::from_wire("XX"), None);
        assert!(TraderState::ClosingOnly.can_close());
        assert!(!TraderState::ClosingOnly.can_open());
        assert!(!TraderState::DisabledByRisk.can_close());
    }

    #[test]
    fn tif_semantics() {
        assert!(TimeInForce::Day.sweeps_at_close());
        assert!(TimeInForce::Overnight.parks_overnight());
        assert!(!TimeInForce::Gtc.sweeps_at_close());
    }
}
