# trade8-sdk - Trade8 Vertical SDK
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Docs](https://img.shields.io/badge/docs/read-3aa757.svg)](https://trade8.dev/docs/services/sdk)
[![Website](https://img.shields.io/badge/website-trade8.dev-1a73e8.svg)](https://trade8.dev/docs/sdk)

## 1. Summary

The Trade8 vertical's **shared plane** - the vertical's own
`sentin8-sdk` equivalent (SDD §3.4): fixed-point math, market-data
contracts, and trading protocol codecs that EVERY trade8 service
links. It stays one workspace because dissolving it would recreate
cross-service repo cycles one level down.

## 2. Crates

| Crate | Purpose |
|---|---|
| `facade` (package `trade8`) | Namespace facade - `use trade8::fixed::…`, `use trade8::contracts::…` |
| `crates/fixed` | `Price`/`Qty` fixed-point decimal math (the market invariant) |
| `crates/contracts` | Shared event/wire types - `Bar`, `BarInterval`, `TradePrint` |
| `crates/feed-adapters` | Market-feed ingestion adapters |
| `crates/fix` | FIX protocol codec |
| `crates/ouch` | OUCH protocol codec |
| `crates/trade8-client` | Shared consumer helpers (bus run loop) |

## 3. Consumption

```toml
trade8 = { git = "https://github.com/Trade8-dev/trade8-sdk", tag = "trade8-v0.1.0" }
```

The facade pins the platform SDK (`sentin8` - shared plane only,
SDD §2.2).

---

**Architecture & Design:** trade8:sdk and its underlying concepts were designed by [Hamid Jahanzad](https://www.linkedin.com/in/hamidjahanzad/).
