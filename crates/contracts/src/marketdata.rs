// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Market-data DTOs - L1 quotes, L2 depth, imbalance, trade prints, bars.
//!
//! (`LV1Update`, `LV2Update` with `FeedType_e`), `ImbalanceUpdate` (extends
//! `TechUpdate`), `Broker/Library/Snapshot/ChartDB` (bars), `SYMBOL_PRICE`.

use serde::{Deserialize, Serialize};
// Leaf→leaf import: raw crate path by exception (see order.rs).
// The `trade8::` facade depends on this crate - importing it here
// would be a dependency cycle.
use trade8_fixed::Price;
use uuid::Uuid;

use crate::enums::FeedType;

/// L1 top-of-quote update - `trade8:feed:quote:updated` payload
///.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct QuoteUpdate {
    pub symbol_id: Uuid,
    pub symbol_name: String,
    /// Milliseconds from market midnight - the venue
    /// clock; wall-clock `ts` rides the envelope.
    pub venue_ts_ms: u64,
    pub bid_price: Price,
    pub ask_price: Price,
    pub bid_size: u64,
    pub ask_size: u64,
}

impl QuoteUpdate {
    pub fn mid(self) -> Option<Price> {
        let sum = self.bid_price.checked_add(self.ask_price)?;
        // mid may land off-tick; callers round to tick downstream
        Some(Price::from_scaled(sum.raw / 2, sum.scale))
    }
}

/// L2 price-level delta - `trade8:feed:depth:updated` payload
///.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DepthUpdate {
    pub symbol_id: Uuid,
    pub symbol_name: String,
    pub feed_type: FeedType,
    /// Quote-source id (the consolidation membership).
    pub source_id: Uuid,
    pub venue_ts_ms: u64,
    pub is_bid: bool,
    pub price: Price,
    pub volume: u64,
    /// Number of orders at the level (attributable books only).
    pub depth: Option<u32>,
    /// Market-maker id.
    pub mmid: Option<String>,
    pub sequence: u64,
}

/// Auction imbalance - `trade8:feed:imbalance:updated` payload
///.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ImbalanceUpdate {
    pub symbol_id: Uuid,
    pub symbol_name: String,
    /// `'B'` or `'A'` (imbalance side, ).
    pub side: char,
    /// Imbalance type.
    pub imbalance_type: char,
    /// Status code.
    pub status: char,
    pub venue_ts_ms: u64,
    pub price: Price,
    pub auction_price: Price,
    pub continuous_price: Price,
    pub volume: u64,
    pub paired_volume: u64,
}

/// Consolidated trade print - `trade8:feed:trade:printed` payload; the
/// Bars aggregation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TradePrint {
    pub symbol_id: Uuid,
    pub symbol_name: String,
    pub venue_ts_ms: u64,
    pub price: Price,
    pub qty: u64,
    /// Print sequence within the source (gap detection upstream).
    pub seq: u64,
}

/// A closed OHLCV bar - `trade8:bars:bar:closed` payload.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Bar {
    pub symbol_id: Uuid,
    pub interval: BarInterval,
    /// Bucket start (market clock).
    pub bucket_start_ms: u64,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: u64,
    pub print_count: u32,
}

impl Bar {
    pub fn from_first(p: TradePrint, interval: BarInterval, bucket_ms: u64) -> Self {
        Bar {
            symbol_id: p.symbol_id,
            interval,
            bucket_start_ms: bucket_ms,
            open: p.price,
            high: p.price,
            low: p.price,
            close: p.price,
            volume: p.qty,
            print_count: 1,
        }
    }

    pub fn apply(&mut self, p: &TradePrint) {
        if p.price.raw > self.high.raw {
            self.high = p.price;
        }
        if p.price.raw < self.low.raw {
            self.low = p.price;
        }
        self.close = p.price;
        self.volume += p.qty;
        self.print_count += 1;
    }
}

/// Bar intervals - the fixed service set (bars design §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
pub enum BarInterval {
    #[serde(rename = "1s")]
    S1,
    #[serde(rename = "1m")]
    M1,
    #[serde(rename = "5m")]
    M5,
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "1d")]
    D1,
}

impl BarInterval {
    pub fn duration_ms(self) -> u64 {
        match self {
            BarInterval::S1 => 1_000,
            BarInterval::M1 => 60_000,
            BarInterval::M5 => 300_000,
            BarInterval::M15 => 900_000,
            BarInterval::H1 => 3_600_000,
            BarInterval::D1 => 86_400_000,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            BarInterval::S1 => "1s",
            BarInterval::M1 => "1m",
            BarInterval::M5 => "5m",
            BarInterval::M15 => "15m",
            BarInterval::H1 => "1h",
            BarInterval::D1 => "1d",
        }
    }
    /// Resolve the interval covering a duration in ms (largest whose
    /// duration ≥ the span, minimum 1s). Used by tests and API helpers.
    pub fn from_ms(span_ms: u64) -> Option<BarInterval> {
        Some(match span_ms {
            0..=1_000 => BarInterval::S1,
            1_001..=60_000 => BarInterval::M1,
            60_001..=300_000 => BarInterval::M5,
            300_001..=900_000 => BarInterval::M15,
            900_001..=3_600_000 => BarInterval::H1,
            _ => return None,
        })
    }
}

/// A fired price alert - `trade8:feed:alert:fired` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AlertFired {
    pub alert_id: Uuid,
    pub rule_id: Uuid,
    pub usr_id: String,
    pub symbol_name: String,
    /// The triggering snapshot.
    pub quote: QuoteUpdate,
    /// The matched condition, human-rendered.
    pub condition: String,
    pub at_ms: u64,
}

/// Feed-link health - `trade8:feed:feed:status` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FeedStatus {
    pub link_id: Uuid,
    pub link_name: String,
    #[serde(rename = "state")]
    pub state: FeedState,
    /// Last observed sequence on the link (gap detection).
    pub last_seq: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum FeedState {
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "degraded")]
    Degraded,
    #[serde(rename = "down")]
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_apply_ohlc() {
        let sym = Uuid::new_v4();
        let mk = |raw: i128, qty: u64, ts: u64| TradePrint {
            symbol_id: sym,
            symbol_name: "X".into(),
            venue_ts_ms: ts,
            price: Price::equity(raw),
            qty,
            seq: 0,
        };
        let mut bar = Bar::from_first(mk(100_000, 10, 0), BarInterval::M1, 0);
        bar.apply(&mk(105_000, 5, 1));
        bar.apply(&mk(98_000, 20, 2));
        bar.apply(&mk(101_000, 7, 3));
        assert_eq!(bar.open.raw, 100_000);
        assert_eq!(bar.high.raw, 105_000);
        assert_eq!(bar.low.raw, 98_000);
        assert_eq!(bar.close.raw, 101_000);
        assert_eq!(bar.volume, 42);
        assert_eq!(bar.print_count, 4);
    }

    #[test]
    fn quote_mid() {
        let q = QuoteUpdate {
            symbol_id: Uuid::new_v4(),
            symbol_name: "AAPL".into(),
            venue_ts_ms: 0,
            bid_price: Price::equity(100_000),
            ask_price: Price::equity(100_400),
            bid_size: 10,
            ask_size: 8,
        };
        assert_eq!(q.mid().unwrap().raw, 100_200);
    }

    #[test]
    fn interval_table() {
        assert_eq!(BarInterval::M5.duration_ms(), 300_000);
        // bucket table: 0..=1s → S1, 1.001s..60s → M1, 60.001s..300s → M5 …
        assert_eq!(BarInterval::from_ms(1_000), Some(BarInterval::S1));
        assert_eq!(BarInterval::from_ms(1_001), Some(BarInterval::M1));
        assert_eq!(BarInterval::from_ms(59_999), Some(BarInterval::M1));
        assert_eq!(BarInterval::from_ms(60_001), Some(BarInterval::M5));
        assert_eq!(BarInterval::from_ms(0), Some(BarInterval::S1));
    }
}
