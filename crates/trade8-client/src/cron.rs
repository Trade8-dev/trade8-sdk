// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Cron schedule registration (the Ledger pattern, trade8 form).
//!
//! Trade8 services delegate ALL scheduling to Sentin8:Cron: each service
//! registers its default schedules at boot via a certificate-presenting
//! caller (the `{service_type}-cron` workload certificate — CERTFREE-4V)
//! and exposes the proxy-back endpoint Cron calls when the schedule fires:
//! - Desk — session choreography: preopen 04:00, open 09:30,
//!   close-pending 15:55, close 16:00 (config-tz).
//! - Bars — EOD finalization 16:15 + the daily bucket reset.
//! - Router — journal retention purge 02:00 (30 d default).
//!
//! Registration is idempotent-per-day (Cron owns dedup by domain+name;
//! a duplicate create is logged, not fatal — the boot pass retries
//! cleanly on restart).

use crate::ServiceCaller;

/// One boot-time schedule registration.
#[derive(Debug, Clone)]
pub struct BootSchedule {
    /// Cron domain (e.g. `trade8-desk`).
    pub domain: String,
    /// Schedule name (dedup key within the domain).
    pub name: String,
    /// Cron expression (5-field, Cron server tz).
    pub cron_expression: String,
    /// Target config — must carry `callback_url` (the proxy-back).
    pub target_config: serde_json::Value,
}

/// The default trade8 schedule set for one service (by service type).
pub fn default_schedules(service_type: &str, port: u16) -> Vec<BootSchedule> {
    let cb = format!("http://localhost:{port}");
    match service_type {
        "trade8-desk" => vec![
            BootSchedule {
                domain: "trade8-desk".into(),
                name: "session-preopen".into(),
                cron_expression: "0 4 * * 1-5".into(),
                target_config: serde_json::json!({
                    "callback_url": format!("{cb}/v1/session"),
                    "body": {"action": "preopen"},
                }),
            },
            BootSchedule {
                domain: "trade8-desk".into(),
                name: "session-open".into(),
                cron_expression: "30 9 * * 1-5".into(),
                target_config: serde_json::json!({
                    "callback_url": format!("{cb}/v1/session"),
                    "body": {"action": "open"},
                }),
            },
            BootSchedule {
                domain: "trade8-desk".into(),
                name: "session-close-pending".into(),
                cron_expression: "55 15 * * 1-5".into(),
                target_config: serde_json::json!({
                    "callback_url": format!("{cb}/v1/session"),
                    "body": {"action": "close_pending"},
                }),
            },
            BootSchedule {
                domain: "trade8-desk".into(),
                name: "session-close".into(),
                cron_expression: "0 16 * * 1-5".into(),
                target_config: serde_json::json!({
                    "callback_url": format!("{cb}/v1/session"),
                    "body": {"action": "close"},
                }),
            },
        ],
        "trade8-bars" => vec![BootSchedule {
            domain: "trade8-bars".into(),
            name: "eod-finalize".into(),
            cron_expression: "15 16 * * 1-5".into(),
            target_config: serde_json::json!({
                "callback_url": format!("{cb}/v1/eod"),
                "body": {"date": "today"},
            }),
        }],
        "trade8-router" => vec![BootSchedule {
            domain: "trade8-router".into(),
            name: "journal-purge".into(),
            cron_expression: "0 2 * * *".into(),
            target_config: serde_json::json!({
                "callback_url": format!("{cb}/v1/journal/purge"),
                "body": {"days": 30},
            }),
        }],
        _ => Vec::new(),
    }
}

/// Register one schedule with Cron via the certificate-presenting caller.
pub async fn register(cron: &ServiceCaller, s: &BootSchedule) -> Result<(), String> {
    #[derive(serde::Serialize)]
    struct CreateSchedule {
        domain: String,
        schedule_type: String,
        cron_expression: String,
        target_config: serde_json::Value,
        max_retries: i32,
    }
    let body = CreateSchedule {
        domain: s.domain.clone(),
        schedule_type: s.name.clone(),
        cron_expression: s.cron_expression.clone(),
        target_config: s.target_config.clone(),
        max_retries: 3,
    };
    cron.post_json::<serde_json::Value>("/v1/schedules", &body)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Boot-time registration of a service's default schedule set. Failures
/// are logged, not fatal (Cron may be booting concurrently — the next
/// restart re-registers; Cron dedups by domain+type).
pub async fn register_defaults(auth_issuer: &str, cron_url: &str, service_type: &str, port: u16) {
    let schedules = default_schedules(service_type, port);
    if schedules.is_empty() {
        return;
    }
    let caller = match crate::build_caller(
        auth_issuer,
        cron_url,
        &format!("{service_type}-cron"),
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, service_type, "cron caller unavailable; schedules not registered");
            return;
        }
    };
    for s in &schedules {
        match register(&caller, s).await {
            Ok(()) => tracing::info!(schedule = %s.name, "cron schedule registered"),
            Err(e) => {
                tracing::warn!(error = %e, schedule = %s.name, "cron schedule registration failed (retry on restart)")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_schedules_cover_the_choreography() {
        let desk = default_schedules("trade8-desk", 8100);
        assert_eq!(desk.len(), 4);
        assert!(desk.iter().all(|s| s.target_config["callback_url"]
            .as_str()
            .unwrap()
            .contains("/v1/session")));
        let bars = default_schedules("trade8-bars", 8400);
        assert_eq!(bars.len(), 1);
        assert!(bars[0].target_config["callback_url"]
            .as_str()
            .unwrap()
            .contains("/v1/eod"));
        let router = default_schedules("trade8-router", 8300);
        assert_eq!(router.len(), 1);
        assert!(router[0].target_config["callback_url"]
            .as_str()
            .unwrap()
            .contains("/v1/journal/purge"));
        assert!(default_schedules("trade8-feed", 8200).is_empty());
    }
}
