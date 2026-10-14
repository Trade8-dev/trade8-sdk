// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Bus consumer helper — the canonical SSE consume loop for trade8
//! services (the payments-worker pattern over `sentin8-bus-client`):
//! connect with a certificate-presenting Bus client, stream
//! `bus:{env}:{family}`, dispatch typed events, ack on success / nack
//! (retry→DLQ) on failure.
//!
//! AGENTS.md event-driven rule: consumers own their reaction to an
//! event; the loop is identical across services so at-least-once +
//! idempotent handlers = effectively-once.

use futures_util::StreamExt;

use crate::StreamEnvelope;

/// Reconnect delay when the Bus SSE stream drops (memory rule: 2s max —
/// a longer wait hides a bug).
const RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// One consumed bus message (the Bus ConsumeMessage shape the platform
/// sends on the SSE stream).
#[derive(Debug, Clone)]
pub struct BusMessage {
    pub message_id: String,
    pub event_type: String,
    pub env_id: String,
    pub payload: serde_json::Value,
    pub correlation_id: String,
    pub idempotency_key: Option<String>,
}

impl BusMessage {
    /// Decode the payload into the StreamEnvelope shape (trade8 events
    /// carry the full envelope in the payload).
    pub fn payload_as<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_value(self.payload.clone()).ok()
    }

    /// Decode as the trade8 event envelope.
    pub fn envelope(&self) -> Option<StreamEnvelope> {
        self.payload_as()
    }
}

/// Handler outcome — ack or nack (Bus retries nacks up to max_retries,
/// then lands the message in the DLQ for manual replay).
pub enum HandleOutcome {
    Ack,
    Nack,
}

/// Run the SSE consumer loop for one (family, group) pair, forever.
///
/// `handler` receives each decoded message; returning `Ack` commits it,
/// `Nack` sends it back for retry→DLQ. The handler must be idempotent
/// (at-least-once delivery).
pub async fn run_consumer<F, Fut>(
    bus: std::sync::Arc<sentin8::bus::client::BusClient>,
    stream_family: &str,
    consumer_group: &str,
    mut handler: F,
) where
    F: FnMut(BusMessage) -> Fut + Send,
    Fut: std::future::Future<Output = HandleOutcome> + Send,
{
    let family = stream_family.to_string();
    let group = consumer_group.to_string();
    tracing::info!(family = %family, group = %group, "bus consumer starting");
    loop {
        let resp = match bus.sse_consume(&family, &group).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, family = %family, "bus consume connect failed; retry in 2s");
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            }
        };
        let mut buffer = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, family = %family, "bus stream error; reconnect");
                    break;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            // SSE events are separated by blank lines.
            while let Some(pos) = buffer.find("\n\n") {
                let event_text = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();
                let mut data_line = String::new();
                for line in event_text.lines() {
                    if let Some(d) = line.strip_prefix("data: ") {
                        data_line = d.to_string();
                    }
                }
                if data_line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&data_line) else {
                    continue;
                };
                let msg = BusMessage {
                    message_id: v["message_id"].as_str().unwrap_or_default().to_string(),
                    event_type: v["event_type"].as_str().unwrap_or_default().to_string(),
                    env_id: v["env_id"].as_str().unwrap_or_default().to_string(),
                    payload: v["payload"].clone(),
                    correlation_id: v["correlation_id"].as_str().unwrap_or_default().to_string(),
                    idempotency_key: v["idempotency_key"].as_str().map(str::to_string),
                };
                if msg.message_id.is_empty() {
                    continue;
                }
                let message_id = msg.message_id.clone();
                match handler(msg).await {
                    HandleOutcome::Ack => {
                        let _ = bus.ack(&family, &group, &message_id).await;
                    }
                    HandleOutcome::Nack => {
                        // Nack (retry→DLQ) then ack to advance the cursor —
                        // the DLQ owns poison messages.
                        let _ = bus.nack(&family, &group, &message_id, Some(1)).await;
                        let _ = bus.ack(&family, &group, &message_id).await;
                    }
                }
            }
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_message_decodes_envelope() {
        let payload = serde_json::json!({
            "event_type": "trade8:desk:order:accepted",
            "env_id": "00000000-0000-0000-0000-000000000001",
            "idempotency_key": "k1",
            "seq": 3,
            "payload": {"qty": 100},
        });
        let msg = BusMessage {
            message_id: "1-1".into(),
            event_type: "trade8:desk:order:accepted".into(),
            env_id: "00000000-0000-0000-0000-000000000001".into(),
            payload,
            correlation_id: String::new(),
            idempotency_key: Some("k1".into()),
        };
        let env = msg.envelope().unwrap();
        assert_eq!(env.seq, 3);
        assert_eq!(env.idempotency_key, "k1");
    }
}
