// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Event-type constants (master design §7.1) - the platform colon
//! convention `trade8:{service}:{resource}:{operation}` (AGENTS.md:
//! colons only, never dots).
//!
//! Payloads are the DTOs in this crate (order.rs, marketdata.rs, pnl.rs)
//! - a trigger must run from the payload alone (payload-completeness).

/// Event-type registry: (constant, literal).
macro_rules! event_types {
    ($(pub const $name:ident: &str = $lit:literal;)*) => {
        /// All trade8 event types - asserted colon-named, no dots (AGENTS.md).
        pub const ALL: &[&str] = &[$($lit),*];
        $(pub const $name: &str = $lit;)*
    };
}

event_types! {
    // Desk - order lifecycle
    pub const ORDER_SUBMITTED: &str = "trade8:desk:order:submitted";
    pub const ORDER_ACCEPTED: &str = "trade8:desk:order:accepted";
    pub const ORDER_REJECTED: &str = "trade8:desk:order:rejected";
    pub const ORDER_CANCELLED: &str = "trade8:desk:order:cancelled";
    pub const ORDER_REPLACED: &str = "trade8:desk:order:replaced";
    pub const ORDER_ROUTED: &str = "trade8:desk:order:routed";
    pub const ORDER_OVERNIGHT: &str = "trade8:desk:order:overnight";
    pub const ORDER_OVERNIGHT_RESTORED: &str = "trade8:desk:order:overnight:restored";
    pub const ORDER_FLATTEN: &str = "trade8:desk:order:flatten";
    pub const SESSION_STATE: &str = "trade8:desk:session:state";
    pub const CATALOG_UPDATED: &str = "trade8:desk:catalog:updated";
    pub const BORROW_RESERVED: &str = "trade8:desk:borrow:reserved";
    pub const BORROW_RELEASED: &str = "trade8:desk:borrow:released";
    pub const BORROW_ADJUSTED: &str = "trade8:desk:borrow:adjusted";
    pub const BORROW_UPDATED: &str = "trade8:desk:borrow:updated";
    pub const KEYSTROKE_LOGGED: &str = "trade8:desk:keystroke:logged";

    // Router - fills + sessions
    pub const FILL_EXECUTED: &str = "trade8:router:fill:executed";
    pub const FILL_REJECTED: &str = "trade8:router:fill:rejected";
    pub const FILL_CANCELLED: &str = "trade8:router:fill:cancelled";
    pub const SESSION_STATUS: &str = "trade8:router:session:status";

    // Risk
    pub const RISK_LIMIT_UPDATED: &str = "trade8:risk:limit:updated";
    pub const RISK_TRADER_STATE: &str = "trade8:risk:trader:state";
    pub const RISK_RESTRICTION_UPDATED: &str = "trade8:risk:restriction:updated";

    // Feed
    pub const QUOTE_UPDATED: &str = "trade8:feed:quote:updated";
    pub const DEPTH_UPDATED: &str = "trade8:feed:depth:updated";
    pub const IMBALANCE_UPDATED: &str = "trade8:feed:imbalance:updated";
    pub const TRADE_PRINTED: &str = "trade8:feed:trade:printed";
    pub const ALERT_FIRED: &str = "trade8:feed:alert:fired";
    pub const FEED_STATUS: &str = "trade8:feed:feed:status";

    // Bars
    pub const BAR_CLOSED: &str = "trade8:bars:bar:closed";
    pub const EOD_COMPLETED: &str = "trade8:bars:eod:completed";

    // Pnl
    pub const POSITION_UPDATED: &str = "trade8:pnl:position:updated";
    pub const TRADER_SUMMARY: &str = "trade8:pnl:trader:summary";
    pub const OFFICE_SUMMARY: &str = "trade8:pnl:office:summary";
    pub const OVERNIGHT_MISMATCH: &str = "trade8:pnl:overnight:mismatch";
}

/// Bus stream family for a service - the MemBus key segment
/// (`mbus:{env}:{family}`). One family per producing service keeps
/// per-(service, family) sequences monotonic (membus design §5.5).
pub const FAMILY_DESK: &str = "desk";
pub const FAMILY_ROUTER: &str = "router";
pub const FAMILY_RISK: &str = "risk";
pub const FAMILY_FEED: &str = "feed";
pub const FAMILY_BARS: &str = "bars";
pub const FAMILY_PNL: &str = "pnl";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colon_naming_only() {
        // AGENTS.md: event names are colon-separated; dots are forbidden.
        for et in ALL {
            assert!(
                et.starts_with("trade8:"),
                "event type {et} must start with the vertical prefix"
            );
            assert!(!et.contains('.'), "event type {et} must not contain dots");
            assert!(
                et.split(':').count() >= 4,
                "event type {et} needs >= 4 segments"
            );
        }
    }

    #[test]
    fn event_types_unique() {
        let mut v = ALL.to_vec();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), ALL.len());
    }
}
