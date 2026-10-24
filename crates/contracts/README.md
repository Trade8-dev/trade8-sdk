# trade8 contracts - Shared Domain Types
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](../../../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Docs](https://img.shields.io/badge/docs/read-3aa757.svg)](https://trade8.dev/docs/sdk/facade)
[![Website](https://img.shields.io/badge/website-trade8.dev-1a73e8.svg)](https://trade8.dev/docs/sdk)

> Audience: consumers of the vertical's type vocabulary (services, apps,
> strategies). Wire-level construction in `DESIGN.md`; the vertical map in
> the top-level `DESIGN.md`.

## 1. Summary

`trade8::contracts` is the shared language of the trade8 vertical: the
wire-faithful enums, order/fill DTOs, the reject-code registry, the
event-type vocabulary, market-data shapes, P&L DTOs, and the Desk↔Risk
authorize contract. Every service and the app consume these types; the
event-name rules (colon-separated, four segments) are enforced here by
unit tests.

## 2. Motivation

Cross-service trading semantics drift painfully without one type source:
sides/STATES encoded differently per service, reject reasons as raw
strings, event names with dots. Contracts pins the vocabulary in one crate
with tests that (a) assert the colon-only event naming, (b) pin the wire
strings services encode, and (c) keep the state-machine legality table in
exactly one place.

## 3. Architecture & High-Level Design

```mermaid
graph TD
    C[trade8 contracts] --> EN[enums: wire char codes + state machines]
    C --> OR[order: Order/NewOrderRequest/Fill/FeeBreakdown/Flatten]
    C --> RE[reject: RejectCode + Rejection]
    C --> EV[events: event-type registry + family consts]
    C --> MD[marketdata: Quote/Depth/Imbalance/Print/Bar/Alert]
    C --> PN[pnl: Position/Resume/Summaries/OpeningBalance]
    C --> RI[risk: AuthorizeRequest/Verdict/CheckKind/LimitRow]
    DESK[Desk] --> C
    RISK[Risk] --> C
    ROUTER[Router] --> C
    FEED[Feed] --> C
    BARS[Bars] --> C
    PNL[Pnl] --> C
    APP[etrader/strategies] --> C
```

## 4. Key Traits and Data Structures

- **`OrderState`** - the lifecycle enum with `can_transition` (the
  legality table: Pending→Accepted→Routed→PartiallyFilled→
  Filled/Cancelled/Replaced…); the engine-side authority.
- **`RejectCode`** - 31 stable reject reasons with `from_wire`,
  `registry_id`, `message`.
- **`events`** - the `trade8:{service}:{resource}:{operation}` registry
  (`order:submitted/accepted/rejected/cancelled/replaced/flatten/overnight…`,
  `router:fill:*`, `risk:limit:updated`, `feed:*`, `bars:*`, `pnl:*`) plus
  `FAMILY_*` stream-family constants.
- **DTO families** - order snapshots + fill/fee shapes; market-data
  envelopes (`QuoteUpdate…Bar` with ×10⁴ price fields); P&L shapes;
  the `AuthorizeRequest`/`AuthorizeVerdict` hot contract.

## 5. Rationale, Trade-offs, and Alternatives

- **Glob re-exports from `lib.rs`** - one flat surface for vertical
  consumers (`trade8::contracts::OrderState`); module paths remain for
  precision. Rejected: per-module re-export ceremony (import churn).
- **Wire strings as the enum serialization** - serde renames encode the
  exact wire values the legacy codecs expect, keeping FIX/OUCH adapters
  byte-true without translation layers.
- **State machine in the DTO crate** (not the engine): both Desk (enforcer)
  and any test/replay tooling need the same legality table.
- **Event-name tests in the crate** - the naming convention is enforced
  where the constants live, failing at test time rather than at transport.

## 6. Internal Implementation & Invariants (Private Design)

- Event-type constants are `pub const &str` built exactly once in
  `events.rs`; unit tests assert the colon-naming and uniqueness.
- `RejectCode` ids are stable integers (registry key) - never reorder.
- All price/qty fields are `i128`/`u64` raws + explicit scale (×10⁴
  money, instrument ticks); no floats anywhere in the crate.
- `serde` renames pin every wire string; changing one is a vertical
  wire-contract change.

## 7. Unresolved Questions / Future Designs

- Dead event constants (`catalog:updated`, `borrow:reserved/released/
  adjusted`, `risk:restriction:updated`, `desk:order:batch-requested`) are
  declared but unemitted - prune or wire them (see service design docs).
- Feed-type vocabulary breadth (venue feeds beyond the current set) may
  grow `FeedType` variants - additive only.

# DESIGN.md highlights

The crate is a leaf library: no runtime, no tables, no endpoints. Deep
detail lives inline; the vertical-level contracts (event envelope, naming,
×10⁴ scale) are specified in
the top-level `DESIGN.md` §4 (events) and
`design/sentin8-trade8-data-model-technical-design.md`. Notable tests:
event-naming law, state-machine legality matrix, reject registry ids.

## Testing

18 unit tests: enums wire round-trips, order math/notional, reject
registry, events naming law, marketdata bar folds, pnl folds, risk
verdicts.

# BUSINESS.md highlights

**Business value**: one trade language for six services + the app - the
reason order terms, reject reasons, P&L numbers, and event names agree
everywhere the platform. Consumers: all trade8 services, strategies, and
the etrader app DTO layer.

---

**Architecture & Design:** trade8:contracts and its underlying concepts were designed by [Hamid Jahanzad](https://www.linkedin.com/in/hamidjahanzad/).
