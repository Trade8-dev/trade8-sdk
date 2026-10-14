# README.md: trade8 fixed — Fixed-Point Price/Quantity Core

> Audience: every trade8 consumer (services, crates, app converters).
> Scale conventions: `design/sentin8-trade8-technical-design.md` §10.

## 1. Summary

Fixed-point decimal arithmetic for the vertical's money and quantity
semantics: `Price`/`Qty` as raw integers + explicit scale (×10⁴ equity
money, ×10⁸ crypto, integer quantity with fractional scale 8), tick-band
helpers, and checked notional arithmetic. No floats ever touch a trade.

## 2. Motivation

Floating-point money is how trading bugs are born (0.1 + 0.2 != 0.2).
Trading systems need exact, scale-disciplined arithmetic: prices compare
and sum exactly, notionals overflow-checked, tick bands evaluated on raw
integers. Cross-service payloads carry `{raw, scale}` pairs, not decimals.

## 3. Architecture & High-Level Design

```mermaid
graph LR
    P[Price raw i128 + scale] --> T[tick_size_for / is_on_tick]
    P --> N[checked_notional against Qty]
    Q[Qty raw u64 + scale] --> N
    W[wire: serde {raw, scale} + decimal strings] --> P
```

## 4. Key Traits and Data Structures

- **`struct Price`** — `{raw: i128, scale: u8}` with scale constructors
  (`equity` ×10⁴, `from_scaled`, `parse_decimal`), rescaling, checked add/
  sub/ticks, `checked_notional`, and exact `to_decimal_string`.
- **`struct Qty`** — `{raw: u64, scale: u8}` (shares integer, fractional
  scale 8) with `parse_decimal`, `rescale`.
- **Tick helpers** — `tick_size_for(price, bands, fallback)` and
  `is_on_tick` over ascending band tables.

## 5. Rationale, Trade-offs, and Alternatives

- **i128 raw + u8 scale** — enough range for scaled notionals with
  headroom; cheap copy semantics on the hot path. Rejected: a decimal
  library (allocation + slower than raw ints at these scales), f64 (the
  bug class this crate exists to kill).
- **Explicit scale, not implicit** — payloads carry the scale; rescale is
  explicit at boundaries. Rejected: fixed single scale (crypto needs 8).
- **Checked arithmetic returning Results** — overflow is a caller-visible
  error, never a panic or wrap.

## 6. Internal Implementation & Invariants (Private Design)

- Scale is the power of ten of the *fractional* digits (×10⁴ = 4); every
  cross-scale op first `rescale`s to the coarser/finer side.
- Ticks/bands are raw-integer compares — no string math on the hot path.
- `to_decimal_string` is exact (no rounding).

## 7. Unresolved Questions / Future Designs

- Rounding modes for display at scales finer than the instrument (truncate
  today); possible banker's rounding for UI formatting only.

## Testing

5 unit tests: decimal parsing, notional scaling, rescale, tick bands,
quantity parsing.

---

**Design attribution:** trade8:fixed and its underlying concepts were designed by Hamid Jahanzad <Hamid@Jahanzad.com>.
