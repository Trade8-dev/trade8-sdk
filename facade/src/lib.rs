// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! # trade8 - trade8 workspace namespace facade
//!
//! Every trade8 crate is named `trade8::{leaf}` and is imported through this
//! facade. See `sentin8/design/sentin8-architecture-principles.md` §2.1.
//!
//! Consumers (services, apps, SDK users) must depend on **this crate** and use
//! `trade8::{leaf}` paths - never depend on the raw `trade8-{leaf}` crates
//! directly.
//!
//! Exception - leaf→leaf imports: crates in `crates/` may import each other
//! by raw crate name (`trade8_fixed`, `trade8_contracts`, …) when one leaf
//! builds on another. The facade depends on all leaves, so a leaf importing
//! the facade would be a Cargo dependency cycle; facade paths apply to
//! consumers only.

pub use trade8_client as client;
pub use trade8_contracts as contracts;
pub use trade8_feed_adapters as feed_adapters;
pub use trade8_fix as fix;
pub use trade8_fixed as fixed;
pub use trade8_ouch as ouch;
