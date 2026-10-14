// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! trade8 fixed-point numeric core.
//!
//! Desk-side convention (trade8 master design §10 T9): prices are `i128`
//! scaled by a **per-instrument exponent** — equities default ×10⁴,
//! futures per contract, crypto ×10⁸. Quantities are decimal-scaled `u64`.
//! One numeric convention shared with the exchange8 vertical
//! (`exchange8-fixed`) so orders routed desk→venue never rescale.
//!
//! Design: `../../DESIGN.md` §10 T9,
//! `sentin8-trade8-data-model-technical-design.md` §1.

use serde::{Deserialize, Serialize};

/// Equity price scale: 4 decimal digits (×10⁴).
pub const EQUITY_SCALE: u8 = 4;
/// Crypto price scale: 8 decimal digits (×10⁸).
pub const CRYPTO_SCALE: u8 = 8;
/// Default quantity scale: integer shares/contracts.
pub const QTY_SCALE_INT: u8 = 0;
/// Fractional quantity scale (crypto).
pub const QTY_SCALE_FRAC: u8 = 8;

/// 10ⁿ for u8 exponents (checked).
const fn pow10(exp: u8) -> i128 {
    let mut r: i128 = 1;
    let mut i = 0u8;
    while i < exp {
        r *= 10;
        i += 1;
    }
    r
}

/// A scaled price. The `scale` (decimal digits) is carried per value so
/// instruments with different scales never mix silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Price {
    /// Scaled integer value (ticks).
    pub raw: i128,
    /// Decimal digits (4 = equity, 8 = crypto, per-future DECIMAL_LOCATOR).
    pub scale: u8,
}

impl Price {
    /// Build from an already-scaled integer.
    pub const fn from_scaled(raw: i128, scale: u8) -> Self {
        Price { raw, scale }
    }

    /// Equity constructor (scale 4). `Price::equity(100_000)` == 10.00.
    pub const fn equity(raw: i128) -> Self {
        Price {
            raw,
            scale: EQUITY_SCALE,
        }
    }

    /// Parse a decimal string (`"10.05"`) at the given scale.
    ///
    /// Returns `None` on: more fractional digits than `scale`, non-digits,
    /// empty integer part, or a non-positive value (order prices are
    /// strictly positive; a stop at zero is modeled as no price).
    pub fn parse_decimal(s: &str, scale: u8) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let neg = s.starts_with('-');
        let body = if neg { &s[1..] } else { s };
        let (int_part, frac_part) = match body.split_once('.') {
            Some((i, f)) => (i, f),
            None => (body, ""),
        };
        if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if frac_part.len() > scale as usize {
            return None;
        }
        if !frac_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut frac_val: i128 = 0;
        for b in frac_part.bytes() {
            frac_val = frac_val * 10 + (b - b'0') as i128;
        }
        for _ in frac_part.len()..scale as usize {
            frac_val *= 10;
        }
        let int_val: i128 = int_part.parse().ok()?;
        let raw = int_val.checked_mul(pow10(scale))? + frac_val;
        if neg || raw <= 0 {
            return None;
        }
        Some(Price { raw, scale })
    }

    /// Re-scale to a common exponent. Upward is lossless; downward only
    /// when it divides exactly (returns `None` otherwise).
    pub fn rescale(self, to: u8) -> Option<Price> {
        if to == self.scale {
            return Some(self);
        }
        if to > self.scale {
            let mut r = self.raw;
            let mut i = self.scale;
            while i < to {
                r = r.checked_mul(10)?;
                i += 1;
            }
            Some(Price { raw: r, scale: to })
        } else {
            let div = pow10(self.scale - to);
            if self.raw % div == 0 {
                Some(Price {
                    raw: self.raw / div,
                    scale: to,
                })
            } else {
                None
            }
        }
    }

    /// Checked addition of same-scale prices.
    pub fn checked_add(self, rhs: Price) -> Option<Price> {
        if self.scale != rhs.scale {
            return None;
        }
        self.raw.checked_add(rhs.raw).map(|r| Price {
            raw: r,
            scale: self.scale,
        })
    }

    /// Checked subtraction of same-scale prices (`None` if negative result —
    /// desk prices don't go below zero).
    pub fn checked_sub(self, rhs: Price) -> Option<Price> {
        if self.scale != rhs.scale {
            return None;
        }
        if self.raw < rhs.raw {
            return None;
        }
        Some(Price {
            raw: self.raw - rhs.raw,
            scale: self.scale,
        })
    }

    /// Add `ticks` (1-tick units at this price's scale) with overflow check.
    pub fn checked_add_ticks(self, ticks: i128) -> Option<Price> {
        self.raw.checked_add(ticks).map(|r| Price {
            raw: r,
            scale: self.scale,
        })
    }

    /// Notional = price × qty, overflow-safely, expressed in the price's
    /// tick units. Fractional qty applies multiply-first-then-divide.
    pub fn checked_notional(self, qty: Qty) -> Option<i128> {
        let product = self.raw.checked_mul(qty.raw as i128)?;
        if qty.scale == 0 {
            Some(product)
        } else {
            product.checked_div(pow10(qty.scale))
        }
    }

    /// Render as a decimal string at the value's scale (display only —
    /// the scaled integer is the wire/DB truth).
    pub fn to_decimal_string(self) -> String {
        let neg = self.raw < 0;
        let abs = self.raw.abs();
        let unit = pow10(self.scale);
        let int = abs / unit;
        let frac = abs % unit;
        if self.scale == 0 {
            format!("{}{}", if neg { "-" } else { "" }, int)
        } else {
            format!(
                "{}{}.{:0width$}",
                if neg { "-" } else { "" },
                int,
                frac,
                width = self.scale as usize
            )
        }
    }
}

/// A decimal-scaled quantity. Equities: scale 0 (integer shares);
/// crypto: scale 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Qty {
    /// Scaled integer value.
    pub raw: u64,
    /// Decimal digits.
    pub scale: u8,
}

impl Qty {
    /// Integer-quantity constructor (equities/futures board lots).
    pub const fn shares(raw: u64) -> Self {
        Qty {
            raw,
            scale: QTY_SCALE_INT,
        }
    }

    /// Parse a decimal quantity string at the given scale. Zero is legal
    /// for quantities (an order is validated as strictly positive by the
    /// Desk, but derived quantities like remaining shares may be 0).
    pub fn parse_decimal(s: &str, scale: u8) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let neg = s.starts_with('-');
        if neg {
            return None; // quantities are never negative (signed position is a PnL concept)
        }
        let (int_part, frac_part) = match s.split_once('.') {
            Some((i, f)) => (i, f),
            None => (s, ""),
        };
        if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if frac_part.len() > scale as usize {
            return None;
        }
        if !frac_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut frac_val: u64 = 0;
        for b in frac_part.bytes() {
            frac_val = frac_val * 10 + (b - b'0') as u64;
        }
        for _ in frac_part.len()..scale as usize {
            frac_val = frac_val.checked_mul(10)?;
        }
        let int_val: u64 = int_part.parse().ok()?;
        let raw = int_val
            .checked_mul(pow10(scale) as u64)?
            .checked_add(frac_val)?;
        Some(Qty { raw, scale })
    }

    /// Re-scale (upward lossless; downward only exact).
    pub fn rescale(self, to: u8) -> Option<Qty> {
        if to == self.scale {
            return Some(self);
        }
        if to > self.scale {
            let mut r = self.raw;
            let mut i = self.scale;
            while i < to {
                r = r.checked_mul(10)?;
                i += 1;
            }
            Some(Qty { raw: r, scale: to })
        } else {
            let div = pow10(self.scale - to) as u64;
            if self.raw % div == 0 {
                Some(Qty {
                    raw: self.raw / div,
                    scale: to,
                })
            } else {
                None
            }
        }
    }
}

/// Resolve the tick size for a price from ascending band rules
///.
///
/// `bands` is `[(price_upper_exclusive_or_max, tick_size), …]` sorted
/// ascending by price bound; the first band whose bound ≥ the price wins.
/// An empty band list yields the fallback tick.
pub fn tick_size_for(price: Price, bands: &[(Price, i128)], fallback: i128) -> i128 {
    for (bound, tick) in bands {
        // Compare only same-scale prices; mismatched scales fall through
        // to the fallback (callers normalize bands to the instrument scale).
        if bound.scale == price.scale && price.raw <= bound.raw {
            return *tick;
        }
    }
    fallback
}

/// Whether `price` sits on a valid tick for the given tick size at the
/// same scale (offset-from-zero rule; negative prices are invalid anyway).
pub fn is_on_tick(price: Price, tick: i128) -> bool {
    tick > 0 && price.raw > 0 && price.raw % tick == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_equity_price() {
        let p = Price::parse_decimal("10.05", EQUITY_SCALE).unwrap();
        assert_eq!(p.raw, 100_500);
        assert_eq!(p, Price::equity(100_500));
        // more digits than scale → reject; exactly-scale digits are fine
        assert!(Price::parse_decimal("10.05011", EQUITY_SCALE).is_none());
        assert_eq!(
            Price::parse_decimal("10.0501", EQUITY_SCALE).unwrap().raw,
            100_501
        );
        // zero/negative → reject
        assert!(Price::parse_decimal("0", EQUITY_SCALE).is_none());
        assert!(Price::parse_decimal("-3.00", EQUITY_SCALE).is_none());
    }

    #[test]
    fn notional_scale_integrity() {
        // 10.10 = 101_000 scaled (see memory: ×10⁴ price scale)
        let p = Price::equity(101_000);
        let q = Qty::shares(100);
        // Notional stays at price scale ×10⁴: 10.10 × 100 = 1010.00 → 10_100_000.
        // (Beware: `10_100_000_00` parses as 1_010_000_000 — underscores are
        // grouping separators, not scale markers.)
        assert_eq!(p.checked_notional(q), Some(10_100_000));
        assert_eq!(p.to_decimal_string(), "10.1000");
    }

    #[test]
    fn rescale_between_scales() {
        let p = Price::equity(100_000); // 10.00 ×10⁴
        let c = p.rescale(CRYPTO_SCALE).unwrap();
        assert_eq!(c.raw, 1_000_000_000); // ×10⁸
        assert_eq!(c.rescale(EQUITY_SCALE).unwrap(), p);
        // 10.0001 ×10⁴ does not fit in scale 3
        let odd = Price::equity(100_001);
        assert!(odd.rescale(3).is_none());
    }

    #[test]
    fn tick_bands() {
        let bands = [
            (Price::equity(50_000), 100),    // below 5.00 → 0.01
            (Price::equity(1_000_000), 500), // 5.00..100.00 → 0.05
        ];
        assert_eq!(tick_size_for(Price::equity(49_990), &bands, 1000), 100);
        assert_eq!(tick_size_for(Price::equity(500_000), &bands, 1000), 500);
        assert_eq!(tick_size_for(Price::equity(2_000_000), &bands, 1000), 1000);
        assert!(is_on_tick(Price::equity(100_500), 500));
        assert!(!is_on_tick(Price::equity(100_501), 500));
    }

    #[test]
    fn qty_parse_and_rescale() {
        let q = Qty::parse_decimal("100", 0).unwrap();
        assert_eq!(q.raw, 100);
        let c = Qty::parse_decimal("0.5", 8).unwrap();
        assert_eq!(c.raw, 50_000_000);
        // 0.5 shares cannot be exactly represented as whole shares → None
        assert_eq!(c.rescale(0), None);
        assert!(Qty::parse_decimal("-1", 0).is_none());
    }
}
