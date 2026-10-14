// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! trade8-client — the typed envelope contract for the Desk↔Risk↔Router
//! direct low-latency path (master design §5.2).
//!
//! The hot path is NOT bus-relayed: services exchange these envelopes
//! point-to-point over Sentin8:TLS with certificate-presenting callers
//! (CERTFREE-4V), then mirror
//! every accepted state change to Sentin8:Bus (MemBus family streams)
//! outbox-first. The envelope carries the same shape as a bus event
//! (`{event_type, env_id, idempotency_key, seq, payload}`) so consumers
//! are identical for both transports.
//!
//! This crate defines the envelope type + typed call/response pairs.
//! The actual TCP/TLS transport wiring lands with the tokio shell
//! integration pass (same cadence as exchange8's e8-client streams);
//! the core logic (serialization, dispatch, bounded queues) is here and
//! unit-tested.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub mod bus;
pub use bus::{run_consumer, BusMessage, HandleOutcome};

pub mod service;
pub use service::{build_caller, CallError, ServiceCaller, SharedCaller};

pub mod cron;
pub use cron::{default_schedules, register, register_defaults, BootSchedule};

/// Errors on the direct stream.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("encode error: {0}")]
    Encode(String),
    #[error("queue full (bounded — hot path must never grow unbounded)")]
    QueueFull,
    #[error("channel closed")]
    Closed,
}

/// A direct-stream envelope. Same fields as a bus event so downstream
/// consumers can't tell the transports apart (master §5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamEnvelope {
    pub event_type: String,
    pub env_id: uuid::Uuid,
    /// Idempotency key — dedupes resubmit/replay (effectively-once).
    pub idempotency_key: String,
    /// Monotonic per-(producer, kind) sequence — gap detection.
    pub seq: u64,
    pub payload: serde_json::Value,
}

impl StreamEnvelope {
    pub fn new(
        event_type: impl Into<String>,
        env_id: uuid::Uuid,
        idempotency_key: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        StreamEnvelope {
            event_type: event_type.into(),
            env_id,
            idempotency_key: idempotency_key.into(),
            seq: 0,
            payload,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, ClientError> {
        serde_json::to_vec(self).map_err(|e| ClientError::Encode(e.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ClientError> {
        serde_json::from_slice(bytes).map_err(|e| ClientError::Encode(e.to_string()))
    }
}

/// A bounded, single-producer dispatch for one stream kind: per-call
/// sequence stamping + a capacity-guarded queue. Hot-path determinism:
/// strict FIFO per kind, backpressure surfaces as `QueueFull` instead of
/// unbounded growth (master §11 performance budget).
pub struct OutboundStream {
    kind: &'static str,
    queue: tokio::sync::mpsc::Sender<StreamEnvelope>,
    seq: Arc<AtomicU64>,
}

impl OutboundStream {
    pub fn new(
        kind: &'static str,
        capacity: usize,
    ) -> (Self, tokio::sync::mpsc::Receiver<StreamEnvelope>) {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity.max(1));
        (
            OutboundStream {
                kind,
                queue: tx,
                seq: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// Stamp the next sequence and enqueue. `try_send` — full queue is an
    /// error, not a block (the caller decides backoff policy).
    pub fn try_send(
        &self,
        event_type: impl Into<String>,
        env_id: uuid::Uuid,
        idempotency_key: impl Into<String>,
        payload: serde_json::Value,
    ) -> Result<u64, ClientError> {
        let mut env = StreamEnvelope::new(event_type, env_id, idempotency_key, payload);
        env.seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let seq = env.seq;
        self.queue.try_send(env).map_err(|e| match e {
            tokio::sync::mpsc::error::TrySendError::Full(_) => ClientError::QueueFull,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => ClientError::Closed,
        })?;
        Ok(seq)
    }
}

/// Call-kind registry: the typed request/response pairs on the direct
/// path (master §5.2). Each is one `event_type` with a JSON payload in,
/// one response envelope out.
pub mod calls {
    /// Desk → Risk: synchronous authorize (risk design §4).
    pub const AUTHORIZE: &str = "trade8:risk:authorize:call";
    /// Risk → Desk: authorize verdict response.
    pub const AUTHORIZE_RESULT: &str = "trade8:risk:authorize:result";
    /// Desk → Router: route an accepted order (router design §4.1).
    pub const ROUTE_ORDER: &str = "trade8:router:order:route";
    /// Router → Desk: broker ack / routing failure.
    pub const ROUTE_ORDER_ACK: &str = "trade8:router:order:routed_ack";
}

/// Deduplication window for envelope replay (idempotency-key memory) —
/// bounded LRU so crash-replay can't grow it unbounded.
#[derive(Debug, Default)]
pub struct DedupeWindow {
    seen: HashMap<String, u64>,
    order: std::collections::VecDeque<String>,
    capacity: usize,
}

impl DedupeWindow {
    pub fn new(capacity: usize) -> Self {
        DedupeWindow {
            seen: HashMap::new(),
            order: std::collections::VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// Returns `true` when the key is fresh (first sighting), `false`
    /// when it was already seen (duplicate — drop).
    pub fn admit(&mut self, key: &str) -> bool {
        if self.seen.contains_key(key) {
            return false;
        }
        if self.seen.len() >= self.capacity {
            if let Some(evict) = self.order.pop_front() {
                self.seen.remove(&evict);
            }
        }
        self.seen.insert(key.to_string(), 1);
        self.order.push_back(key.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let env = StreamEnvelope::new(
            calls::AUTHORIZE,
            uuid::Uuid::new_v4(),
            "idem-1",
            serde_json::json!({"qty": 100}),
        );
        let bytes = env.encode().unwrap();
        let back = StreamEnvelope::decode(&bytes).unwrap();
        assert_eq!(back, env);
    }

    #[tokio::test]
    async fn stream_sequences_monotonic() {
        let (tx, mut rx) = OutboundStream::new("authorize", 8);
        let env_id = uuid::Uuid::new_v4();
        for i in 0..5 {
            let seq = tx
                .try_send(
                    calls::AUTHORIZE,
                    env_id,
                    format!("k{i}"),
                    serde_json::Value::Null,
                )
                .unwrap();
            assert_eq!(seq, i + 1);
        }
        for expected in 1..=5 {
            let env = rx.recv().await.unwrap();
            assert_eq!(env.seq, expected);
        }
    }

    #[tokio::test]
    async fn bounded_queue_backpressure() {
        let (tx, _rx) = OutboundStream::new("route", 2);
        let env_id = uuid::Uuid::new_v4();
        assert!(tx
            .try_send("e", env_id, "a", serde_json::Value::Null)
            .is_ok());
        assert!(tx
            .try_send("e", env_id, "b", serde_json::Value::Null)
            .is_ok());
        assert!(matches!(
            tx.try_send("e", env_id, "c", serde_json::Value::Null),
            Err(ClientError::QueueFull)
        ));
    }

    #[test]
    fn dedupe_window_admits_once() {
        let mut w = DedupeWindow::new(3);
        assert!(w.admit("a"));
        assert!(!w.admit("a"));
        assert!(w.admit("b"));
        assert!(w.admit("c"));
        assert!(w.admit("d")); // evicts "a"
        assert!(w.admit("a")); // re-admitted after eviction window
    }
}
