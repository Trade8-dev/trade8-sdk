// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! PnL DTOs - positions, day summaries, opening balances, overnight
//! reconciliation.
//!
//! (m_TotalFees/m_TotalGross/m_TotalNetRegion/m_TotalSharesExecutes/
//! m_MaxNetProfitForDay/m_Unrealized/m_UnrealizedLFT/m_LFT),

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Live position for (trader, symbol) - `trade8:pnl:position:updated`
/// payload. Signed size: positive long, negative short (the PnL concept
/// of a signed position; trade8 order quantities themselves are unsigned).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub usr_id: String,
    pub office_id: String,
    pub symbol_id: Uuid,
    pub symbol_name: String,
    /// Signed size (negative = short).
    pub signed_size: i64,
    /// Average cost of the open position (price scale of the instrument).
    pub avg_cost_raw: i128,
    pub price_scale: u8,
    /// Realized day P&L for this symbol (money scaled ×10⁴).
    pub realized: i128,
    /// Unrealized at the last mark.
    pub unrealized: i128,
    /// Fees accumulated for this symbol today.
    pub fees: i128,
}

impl Position {
    pub fn is_flat(&self) -> bool {
        self.signed_size == 0
    }
    pub fn is_long(&self) -> bool {
        self.signed_size > 0
    }
    /// Position value at a mark (scaled price × signed shares - the
    /// Risk max-position-value input).
    pub fn value_at(&self, mark_raw: i128) -> i128 {
        mark_raw * self.signed_size as i128
    }
    /// Unrealized at a mark: (mark − avg_cost) × signed_size.
    pub fn unrealized_at(&self, mark_raw: i128) -> i128 {
        (mark_raw - self.avg_cost_raw) * self.signed_size as i128
    }
}

/// Per-symbol day resume - the `pnl.symbol_resume` row shape
/// (gross/fees/net/shares/max_net/lft).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SymbolResume {
    pub gross: i128,
    pub fees: i128,
    pub net: i128,
    pub shares: i64,
    /// High-water mark of net profit today.
    pub max_net: i128,
    /// Lost-from-top: max_net − current net (≥ 0).
    pub lft: i128,
    pub unrealized: i128,
}

impl SymbolResume {
    /// Fold a position update into the resume (idempotent per update -
    /// the engine recomputes; this is the fold used by tests and the
    /// service's snapshot builder).
    pub fn fold(
        &mut self,
        realized_delta: i128,
        fees_delta: i128,
        shares_delta: i64,
        unrealized: i128,
    ) {
        self.gross += realized_delta;
        self.fees += fees_delta;
        self.net = self.gross - self.fees + unrealized;
        self.shares += shares_delta;
        self.max_net = self.max_net.max(self.net);
        self.lft = (self.max_net - self.net).max(0);
        self.unrealized = unrealized;
    }
}

/// Trader day summary - `trade8:pnl:trader:summary` payload;
/// `ProfitAndLoss` field-for-field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraderPnlSummary {
    pub usr_id: String,
    pub office_id: String,
    pub trading_date: String,
    pub total_fees: i128,
    pub total_gross: i128,
    pub total_net: i128,
    pub total_shares: i64,
    pub max_net_day: i128,
    pub unrealized: i128,
    /// Lost-from-top for the day (drawdown from the high-water mark).
    pub lft: i128,
}

/// Office roll-up - `trade8:pnl:office:summary` payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OfficePnlSummary {
    pub office_id: String,
    pub trading_date: String,
    pub total_fees: i128,
    pub total_gross: i128,
    pub total_net: i128,
    pub total_shares: i64,
    pub trader_count: u32,
}

/// Opening balance carried across sessions - `pnl.opening_balance`
/// row shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningBalance {
    pub trading_date: String,
    pub usr_id: String,
    pub office_id: String,
    pub symbol_id: Uuid,
    pub symbol_name: String,
    pub signed_size: i64,
    pub avg_cost_raw: i128,
    pub price_scale: u8,
    pub realized_carry: i128,
}

/// Overnight reconciliation result: expected vs found, with a mismatch flag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OvernightVerification {
    pub trading_date: String,
    pub symbol_id: Uuid,
    pub symbol_name: String,
    pub usr_id: String,
    pub side: char,
    pub expected_shares: i64,
    pub found_shares: i64,
    pub matched: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_lft_high_water() {
        let mut r = SymbolResume::default();
        // +100, then drawdown −40 → LFT 40, max_net 100
        r.fold(100, 0, 0, 0);
        assert_eq!(r.max_net, 100);
        r.fold(0, 0, 0, 0);
        r.unrealized = -40;
        r.net = r.gross - r.fees + r.unrealized;
        r.max_net = r.max_net.max(r.net);
        r.lft = (r.max_net - r.net).max(0);
        assert_eq!(r.lft, 40);
    }

    #[test]
    fn position_unrealized_math() {
        let p = Position {
            usr_id: "u".into(),
            office_id: "HO".into(),
            symbol_id: Uuid::new_v4(),
            symbol_name: "AAPL".into(),
            signed_size: 100,
            avg_cost_raw: 100_000, // 10.00 ×10⁴
            price_scale: 4,
            realized: 0,
            unrealized: 0,
            fees: 0,
        };
        // mark 10.50 → +50.00 per share → 100 shares = 5_000_00 scaled? No:
        // (105_000−100_000)×100 = 500_000 → 50.00 in price ticks ×100 shares
        assert_eq!(p.unrealized_at(105_000), 500_000);
        assert_eq!(p.value_at(105_000), 10_500_000);
        assert!(p.is_long());
        assert!(!p.is_flat());
    }
}
