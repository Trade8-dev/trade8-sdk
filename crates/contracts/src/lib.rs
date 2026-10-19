// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! trade8 contracts - shared domain types for the vertical.
//!
//! File-purpose index:
//! - `enums.rs`: char-code enums (side, order type, TIF, trader state,
//!   fill liquidity, destination protocol/visibility)
//! - `order.rs`: order + fill DTOs (API + event payloads)
//! - `reject.rs`: reject-code catalog
//! - `events.rs`: event-type constants + envelope payload helpers
//! - `marketdata.rs`: quote/depth/imbalance/print/bar DTOs (Feed/Bars)
//! - `pnl.rs`: position/PnL snapshot DTOs (Pnl)
//! - `risk.rs`: authorize request/verdict DTOs (Desk↔Risk hot path)
//!
//! Design: `../../DESIGN.md` §7 (events)
//! + the per-service design docs.
//!
//! Import through the workspace facade: `use trade8::contracts::…`.
//! Leaf-to-leaf raw imports are the documented exception (see the facade
//! `lib.rs`).

pub mod enums;
pub mod events;
pub mod marketdata;
pub mod order;
pub mod pnl;
pub mod reject;
pub mod risk;

pub use enums::*;
pub use events::*;
pub use marketdata::*;
pub use order::*;
pub use pnl::*;
pub use reject::*;
pub use risk::*;
