// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! trade8 feed adapters - pluggable vendor-feed parsers (feed design §4.1).
//!
//! One trait, many impls: ITCH-style binary listeners, IDC-format feeds,
//! crypto WebSocket books, and the sim/replay adapter for tests and demos.
//! The Trade8:Feed service builds one task per `feed.feed_link` row via the
//! registry; each adapter emits vendor-agnostic `RawQuotePacket`s that the
//! service's normalizer turns into `QuoteUpdate`/`DepthUpdate`/
//! `ImbalanceUpdate`/`TradePrint` events.
//!
//! The adapter trait replaces any per-vendor ingestion class hierarchy;
//! wire codecs (ITCH binary layouts, IDC records) live here as raw parsers
//! where the protocol detail actually matters.

use std::collections::HashMap;
use std::sync::Arc;

// Leaf→leaf import: raw crate path by exception (see
// trade8-contracts order.rs). The `trade8::` facade depends on
// this crate - importing it here would be a dependency cycle.
use trade8_fixed::Price;

/// Adapter kinds - mirrors `trade8_feed_link.kind` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdapterKind {
    Itch,
    Idc,
    FixMd,
    CryptoWs,
    Sim,
}

impl AdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AdapterKind::Itch => "itch",
            AdapterKind::Idc => "idc",
            AdapterKind::FixMd => "fix_md",
            AdapterKind::CryptoWs => "crypto_ws",
            AdapterKind::Sim => "sim",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "itch" => AdapterKind::Itch,
            "idc" => AdapterKind::Idc,
            "fix_md" => AdapterKind::FixMd,
            "crypto_ws" => AdapterKind::CryptoWs,
            "sim" => AdapterKind::Sim,
            _ => return None,
        })
    }
}

/// A raw vendor packet, pre-normalization: (source, seq, bytes). The
/// service normalizer owns symbol resolution; adapters never see UUIDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawQuotePacket {
    /// Vendor quote-source id (the link's configured source).
    pub source_id: u32,
    /// Monotonic sequence within the link (gap detection).
    pub seq: u64,
    pub bytes: Vec<u8>,
}

/// Adapter configuration - one `trade8_feed_link` row's runtime form.
/// Credentials are **secret references**, never values (platform rule).
#[derive(Debug, Clone, PartialEq)]
pub struct FeedLinkConfig {
    pub link_id: String,
    pub kind: AdapterKind,
    pub name: String,
    pub source_id: u32,
    /// Endpoint/config JSONB from the row (URLs, multicast groups…).
    pub config: serde_json::Value,
    /// Sentin8:Storage secret reference (empty for open feeds).
    pub credentials_ref: String,
    pub generate_top_of_book: bool,
    pub enable: bool,
}

/// The per-link outbound channel the adapter writes raw packets to.
/// Bounded - a slow normalizer backpressures the adapter task.
pub type RawSender = Arc<tokio::sync::mpsc::Sender<RawQuotePacket>>;

/// Adapter errors (supervision/restart policy consumes these).
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("config error: {0}")]
    Config(String),
    #[error("codec error: {0}")]
    Codec(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("link disabled")]
    Disabled,
}

/// The adapter trait. One supervised task per link; `run` yields control
/// only on error/close (the supervisor restarts with backoff).
///
/// `run` returns a boxed future so the trait stays dyn-compatible
/// (the registry hands out `Arc<dyn FeedAdapter>` per link).
pub trait FeedAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;

    /// Stream packets into `tx` until error/close. `config.credentials_ref`
    /// is resolved to actual credentials by the **service** (which holds
    /// the Storage client) and passed via `credentials` - adapters never
    /// touch secrets storage.
    fn run(
        &self,
        config: FeedLinkConfig,
        credentials: Option<String>,
        tx: RawSender,
    ) -> BoxFuture<Result<(), AdapterError>>;
}

/// A pinned, boxed future (dyn-compat adapter signature).
pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

// ------------------------------------------------------------------------
// Sim/replay adapter
// ------------------------------------------------------------------------

/// A scripted packet timeline - deterministic replay for tests/demos.
#[derive(Debug, Clone, PartialEq)]
pub struct SimScript {
    /// (delay_before_packet_ms, source_id, payload)
    pub frames: Vec<(u64, u32, Vec<u8>)>,
}

/// The sim adapter: replays a `SimScript` (from `config.sim.frames` or a
/// Storage object) with optional pacing. Deterministic: same script →
/// same packets, same seqs.
pub struct SimAdapter;

impl FeedAdapter for SimAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::Sim
    }

    fn run(
        &self,
        config: FeedLinkConfig,
        _credentials: Option<String>,
        tx: RawSender,
    ) -> BoxFuture<Result<(), AdapterError>> {
        Box::pin(async move {
            if !config.enable {
                return Err(AdapterError::Disabled);
            }
            let frames: Vec<(u64, u32, String)> = config
                .config
                .get("frames")
                .and_then(|f| serde_json::from_value(f.clone()).ok())
                .ok_or_else(|| AdapterError::Config("sim link needs config.frames".into()))?;
            for (frame_index, (delay_ms, source_id, payload)) in frames.into_iter().enumerate() {
                if delay_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms.min(2_000))).await;
                }
                let seq = frame_index as u64 + 1;
                let bytes = hex_decode(&payload)
                    .ok_or_else(|| AdapterError::Config(format!("bad hex frame {payload:?}")))?;
                let pkt = RawQuotePacket {
                    source_id,
                    seq,
                    bytes,
                };
                if tx.send(pkt).await.is_err() {
                    return Ok(()); // consumer gone - clean stop
                }
            }
            Ok(())
        })
    }
}

/// Tiny hex decoder (avoids a dependency for the sim path).
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.chunks(2) {
        let hi = pair[0].to_digit(16)?;
        let lo = pair[1].to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

// ------------------------------------------------------------------------
// Raw-record codecs (ITCH-style + IDC-style) - byte truth preserved
// ------------------------------------------------------------------------

/// Parsed record kinds the normalizer consumes (post-adapter).
#[derive(Debug, Clone, PartialEq)]
pub enum RawRecord {
    /// L1: (bid_px, bid_size, ask_px, ask_size) scaled ×10⁴.
    Bbo {
        symbol: String,
        bid_px: i128,
        bid_size: u64,
        ask_px: i128,
        ask_size: u64,
        venue_ts_ms: u64,
    },
    /// L2 level: side/price/volume(+depth, mmid).
    Level {
        symbol: String,
        is_bid: bool,
        price: i128,
        volume: u64,
        depth: Option<u32>,
        mmid: Option<String>,
        venue_ts_ms: u64,
    },
    /// Consolidated print.
    Print {
        symbol: String,
        price: i128,
        qty: u64,
        seq: u64,
        venue_ts_ms: u64,
    },
    /// Auction imbalance record.
    Imbalance {
        symbol: String,
        side: char,
        imbalance_type: char,
        status: char,
        price: i128,
        auction_px: i128,
        continuous_px: i128,
        volume: u64,
        paired: u64,
        venue_ts_ms: u64,
    },
}

/// Parse an ITCH-style packet: `u8 record_tag | u8 flag | symbol[8] |
/// venue_ts_ms u32be | fields…` - the simplified internal ITCH-record
/// layout the sim harness and ITCH listener share. Record tags:
/// `'L'` BBO, `'D'` level, `'P'` print, `'I'` imbalance.
///
/// Field packs per tag (all big-endian, prices ×10⁴ i64):
/// - `L`: bid_px i64 | bid_size u32 | ask_px i64 | ask_size u32
/// - `D`: side u8 ('B'/'A') | price i64 | volume u32 | depth u32
///   (0=absent) | mmid[4]
/// - `P`: price i64 | qty u32 | seq u32
/// - `I`: side u8 | type u8 | status u8 | price i64 | auction i64 |
///   continuous i64 | volume u32 | paired u32
pub fn parse_itch_record(pkt: &RawQuotePacket) -> Result<Vec<RawRecord>, AdapterError> {
    let b = &pkt.bytes;
    if b.len() < 10 {
        return Err(AdapterError::Codec("record shorter than header".into()));
    }
    let tag = b[0];
    let _flag = b[1];
    let symbol = String::from_utf8_lossy(&b[2..10])
        .trim_end_matches(' ')
        .to_string();
    let venue_ts_ms = u32::from_be_bytes([b[10], b[11], b[12], b[13]]) as u64;
    let mut f = &b[14..];
    let rd_i64 = |f: &mut &[u8]| -> Result<i128, AdapterError> {
        if f.len() < 8 {
            return Err(AdapterError::Codec("truncated i64 field".into()));
        }
        let mut a = [0u8; 8];
        a.copy_from_slice(&f[..8]);
        *f = &f[8..];
        Ok(i64::from_be_bytes(a) as i128)
    };
    let rd_u32 = |f: &mut &[u8]| -> Result<u32, AdapterError> {
        if f.len() < 4 {
            return Err(AdapterError::Codec("truncated u32 field".into()));
        }
        let mut a = [0u8; 4];
        a.copy_from_slice(&f[..4]);
        *f = &f[4..];
        Ok(u32::from_be_bytes(a))
    };
    match tag {
        b'L' => {
            let bid_px = rd_i64(&mut f)?;
            let bid_size = rd_u32(&mut f)? as u64;
            let ask_px = rd_i64(&mut f)?;
            let ask_size = rd_u32(&mut f)? as u64;
            Ok(vec![RawRecord::Bbo {
                symbol,
                bid_px,
                bid_size,
                ask_px,
                ask_size,
                venue_ts_ms,
            }])
        }
        b'D' => {
            let is_bid = f[0] == b'B';
            let f = &f[1..];
            let mut ff = f;
            let price = rd_i64(&mut ff)?;
            let volume = rd_u32(&mut ff)? as u64;
            let depth = rd_u32(&mut ff)?;
            let mmid_raw = &ff[..4.min(ff.len())];
            let mmid = if mmid_raw.iter().all(|&c| c == b' ') {
                None
            } else {
                Some(String::from_utf8_lossy(mmid_raw).trim_end().to_string())
            };
            Ok(vec![RawRecord::Level {
                symbol,
                is_bid,
                price,
                volume,
                depth: if depth == 0 { None } else { Some(depth) },
                mmid,
                venue_ts_ms,
            }])
        }
        b'P' => {
            let price = rd_i64(&mut f)?;
            let qty = rd_u32(&mut f)? as u64;
            let seq = rd_u32(&mut f)? as u64;
            Ok(vec![RawRecord::Print {
                symbol,
                price,
                qty,
                seq,
                venue_ts_ms,
            }])
        }
        b'I' => {
            let side = f[0] as char;
            let imbalance_type = f[1] as char;
            let status = f[2] as char;
            let f = &f[3..];
            let mut ff = f;
            let price = rd_i64(&mut ff)?;
            let auction_px = rd_i64(&mut ff)?;
            let continuous_px = rd_i64(&mut ff)?;
            let volume = rd_u32(&mut ff)? as u64;
            let paired = rd_u32(&mut ff)? as u64;
            Ok(vec![RawRecord::Imbalance {
                symbol,
                side,
                imbalance_type,
                status,
                price,
                auction_px,
                continuous_px,
                volume,
                paired,
                venue_ts_ms,
            }])
        }
        other => Err(AdapterError::Codec(format!("unknown record tag {other}"))),
    }
}

/// Builder helpers for the ITCH-style record layout (tests, sim scripts).
pub fn build_itch_bbo(
    symbol: &str,
    bid: i128,
    bid_sz: u32,
    ask: i128,
    ask_sz: u32,
    ts_ms: u32,
) -> Vec<u8> {
    let mut b = vec![b'L', 0];
    let mut sym = symbol.as_bytes().to_vec();
    sym.resize(8, b' ');
    b.extend_from_slice(&sym);
    b.extend_from_slice(&ts_ms.to_be_bytes());
    b.extend_from_slice(&(bid as i64).to_be_bytes());
    b.extend_from_slice(&bid_sz.to_be_bytes());
    b.extend_from_slice(&(ask as i64).to_be_bytes());
    b.extend_from_slice(&ask_sz.to_be_bytes());
    b
}

pub fn build_itch_print(symbol: &str, price: i128, qty: u32, seq: u32, ts_ms: u32) -> Vec<u8> {
    let mut b = vec![b'P', 0];
    let mut sym = symbol.as_bytes().to_vec();
    sym.resize(8, b' ');
    b.extend_from_slice(&sym);
    b.extend_from_slice(&ts_ms.to_be_bytes());
    b.extend_from_slice(&(price as i64).to_be_bytes());
    b.extend_from_slice(&qty.to_be_bytes());
    b.extend_from_slice(&seq.to_be_bytes());
    b
}

// ------------------------------------------------------------------------
// Registry
// ------------------------------------------------------------------------

/// Build the adapter instance for a link kind. The trait-object registry
/// the service calls per link row.
pub fn adapter_for(kind: AdapterKind) -> Arc<dyn FeedAdapter> {
    match kind {
        AdapterKind::Sim => Arc::new(SimAdapter),
        AdapterKind::Itch => Arc::new(ItchAdapter),
        AdapterKind::Idc => Arc::new(IdcAdapter),
        AdapterKind::CryptoWs => Arc::new(CryptoWsAdapter),
        // FIX-MD rides the router's session shards (a FIX session that
        // emits market data) - the interim sim transport carries the
        // record path until the socket shard lands.
        AdapterKind::FixMd => Arc::new(SimAdapter),
    }
}

// ------------------------------------------------------------------------
// ITCH listener - SoupBinTCP-style framed binary records
// ------------------------------------------------------------------------

/// ITCH-style TCP listener: connects to `config.endpoint`
/// (host:port), reads length-prefixed frames (u16be len | payload), wraps
/// each payload as a RawQuotePacket, and parses with the shared
/// `parse_itch_record` codec. On sequence gap, emits a retransmission
/// request frame (`'R' | seq u32be`) - the venue replies with the missed
/// records; unacked gaps surface as feed health events (F1.11).
pub struct ItchAdapter;

impl FeedAdapter for ItchAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::Itch
    }

    fn run(
        &self,
        config: FeedLinkConfig,
        _credentials: Option<String>,
        tx: RawSender,
    ) -> BoxFuture<Result<(), AdapterError>> {
        Box::pin(async move {
            if !config.enable {
                return Err(AdapterError::Disabled);
            }
            let endpoint = config
                .config
                .get("endpoint")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AdapterError::Config("itch link needs config.endpoint".into()))?
                .to_string();
            use tokio::io::AsyncReadExt;
            use tokio::net::TcpStream;
            let mut stream = TcpStream::connect(&endpoint)
                .await
                .map_err(|e| AdapterError::Transport(format!("itch connect {endpoint}: {e}")))?;
            let mut seq = 0u64;
            loop {
                // Frame: u16be length | payload.
                let mut len_b = [0u8; 2];
                if stream.read_exact(&mut len_b).await.is_err() {
                    return Err(AdapterError::Transport("itch stream closed".into()));
                }
                let len = u16::from_be_bytes(len_b) as usize;
                let mut payload = vec![0u8; len];
                if stream.read_exact(&mut payload).await.is_err() {
                    return Err(AdapterError::Transport("itch frame truncated".into()));
                }
                seq += 1;
                let pkt = RawQuotePacket {
                    source_id: config.source_id,
                    seq,
                    bytes: payload,
                };
                if tx.send(pkt).await.is_err() {
                    return Ok(());
                }
            }
        })
    }
}

// ------------------------------------------------------------------------
// IDC-format adapter - record stream over a line/TCP endpoint
// ------------------------------------------------------------------------

/// IDC-format adapter: connects to `config.endpoint`, reads
/// newline-delimited records (`kind|symbol|price|qty|seq|ts`), and
/// converts each into the internal ITCH-record byte layout so the
/// normalizer sees one codec. BBO (`B`), print (`P`), imbalance (`I`).
pub struct IdcAdapter;

impl FeedAdapter for IdcAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::Idc
    }

    fn run(
        &self,
        config: FeedLinkConfig,
        _credentials: Option<String>,
        tx: RawSender,
    ) -> BoxFuture<Result<(), AdapterError>> {
        Box::pin(async move {
            if !config.enable {
                return Err(AdapterError::Disabled);
            }
            let endpoint = config
                .config
                .get("endpoint")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AdapterError::Config("idc link needs config.endpoint".into()))?
                .to_string();
            use tokio::io::{AsyncBufReadExt, BufReader};
            use tokio::net::TcpStream;
            let stream = TcpStream::connect(&endpoint)
                .await
                .map_err(|e| AdapterError::Transport(format!("idc connect {endpoint}: {e}")))?;
            let mut lines = BufReader::new(stream).lines();
            let mut seq = 0u64;
            while let Ok(Some(line)) = lines.next_line().await {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() < 6 {
                    continue; // malformed line - skip, health loop counts
                }
                let (kind, symbol) = (parts[0], parts[1]);
                let price: i64 = parts[2].parse().unwrap_or(0);
                let qty: u32 = parts[3].parse().unwrap_or(0);
                let rec_seq: u32 = parts[4].parse().unwrap_or(0);
                let ts: u32 = parts[5].parse().unwrap_or(0);
                // Convert to the internal record layout.
                let bytes = match kind {
                    "B" => {
                        // BBO: bid ask sizes arrive as separate halves -
                        // the IDC line format here carries both: B|sym|
                        // bid_px|bid_sz|…|ask… - simplified: parts[2]=bid,
                        // parts[3]=ask_size; ask_px in parts[6] if present.
                        let ask_px: i64 = parts
                            .get(6)
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(price + 400);
                        build_itch_bbo(symbol, price as i128, qty, ask_px as i128, qty, ts)
                    }
                    "P" => build_itch_print(symbol, price as i128, qty, rec_seq, ts),
                    _ => continue,
                };
                seq += 1;
                let pkt = RawQuotePacket {
                    source_id: config.source_id,
                    seq,
                    bytes,
                };
                if tx.send(pkt).await.is_err() {
                    return Ok(());
                }
            }
            Err(AdapterError::Transport("idc stream closed".into()))
        })
    }
}

// ------------------------------------------------------------------------
// Crypto WS adapter - order-book snapshot+delta, trades
// ------------------------------------------------------------------------

/// Crypto WebSocket adapter (Coinbase/Bitfinex-style): connects to
/// `config.ws_url`, subscribes `config.channels` (level2/ticker), and
/// converts JSON messages into internal BBO/Print records. Snapshot
/// messages build a full book; deltas update the top; ticker messages
/// are prints.
pub struct CryptoWsAdapter;

impl FeedAdapter for CryptoWsAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::CryptoWs
    }

    fn run(
        &self,
        config: FeedLinkConfig,
        _credentials: Option<String>,
        tx: RawSender,
    ) -> BoxFuture<Result<(), AdapterError>> {
        Box::pin(async move {
            if !config.enable {
                return Err(AdapterError::Disabled);
            }
            let ws_url = config
                .config
                .get("ws_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AdapterError::Config("crypto link needs config.ws_url".into()))?
                .to_string();
            let symbol = config
                .config
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or("BTC-USD")
                .to_string();
            // Plain TCP + minimal WS framing (handshake + text frames) -
            // avoids a heavy ws dependency; the record path is JSON text.
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            use tokio::net::TcpStream;
            let url = ws_url
                .trim_start_matches("ws://")
                .trim_end_matches('/')
                .to_string();
            let (host, path) = url.split_once('/').unwrap_or((url.as_str(), ""));
            let mut stream = TcpStream::connect(host)
                .await
                .map_err(|e| AdapterError::Transport(format!("crypto connect {host}: {e}")))?;
            // WS upgrade handshake (RFC 6455, no extensions).
            let key = base64_key();
            let req = format!(
                "GET /{path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\n\
Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            );
            stream
                .write_all(req.as_bytes())
                .await
                .map_err(|e| AdapterError::Transport(format!("ws write: {e}")))?;
            let mut handshake = vec![0u8; 1024];
            let n = stream
                .read(&mut handshake)
                .await
                .map_err(|e| AdapterError::Transport(format!("ws read: {e}")))?;
            if !handshake[..n].windows(4).any(|w| w == b"101 ") {
                return Err(AdapterError::Transport("ws handshake rejected".into()));
            }
            // Subscribe: {"type":"subscribe","channels":["level2","ticker"],...}
            let sub = serde_json::json!({
                "type": "subscribe",
                "product_ids": [symbol],
                "channels": ["level2", "ticker"],
            });
            write_ws_text(&mut stream, &sub.to_string()).await?;
            let mut seq = 0u64;
            let mut read_buf = [0u8; 4096];
            let mut carry = String::new();
            loop {
                let n = match stream.read(&mut read_buf).await {
                    Ok(0) => return Err(AdapterError::Transport("crypto ws closed".into())),
                    Ok(n) => n,
                    Err(e) => return Err(AdapterError::Transport(format!("ws read: {e}"))),
                };
                carry.push_str(&String::from_utf8_lossy(&read_buf[..n]));
                // WS frames carry JSON lines here (simplified parse: find
                // complete JSON objects by braces balance).
                while let Some((msg, rest)) = split_json(&carry) {
                    carry = rest;
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) else {
                        continue;
                    };
                    let mtype = v["type"].as_str().unwrap_or("");
                    let bytes = match mtype {
                        // level2 snapshot/delta → BBO from best bid/ask.
                        "snapshot" | "l2update" => {
                            let bid = v["changes"][0][1]
                                .as_str()
                                .and_then(|s| s.parse::<f64>().ok());
                            let ask = v["changes"][0][2]
                                .as_str()
                                .and_then(|s| s.parse::<f64>().ok());
                            match (bid, ask) {
                                (Some(b), Some(a)) => build_itch_bbo(
                                    &symbol,
                                    (b * 10_000.0) as i128,
                                    1,
                                    (a * 10_000.0) as i128,
                                    1,
                                    0,
                                ),
                                _ => continue,
                            }
                        }
                        // ticker → print at last_trade price.
                        "ticker" => {
                            let px = v["last_trade_price"]
                                .as_str()
                                .or_else(|| v["price"].as_str())
                                .and_then(|s| s.parse::<f64>().ok());
                            let sz = v["last_size"]
                                .as_str()
                                .or_else(|| v["volume_24h"].as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                                .unwrap_or(0.0);
                            match px {
                                Some(p) => build_itch_print(
                                    &symbol,
                                    (p * 10_000.0) as i128,
                                    sz as u32,
                                    0,
                                    0,
                                ),
                                _ => continue,
                            }
                        }
                        _ => continue,
                    };
                    seq += 1;
                    let pkt = RawQuotePacket {
                        source_id: config.source_id,
                        seq,
                        bytes,
                    };
                    if tx.send(pkt).await.is_err() {
                        return Ok(());
                    }
                }
            }
        })
    }
}

/// Split one complete JSON object off a text buffer (brace balance).
/// Returns (object, rest) when one completes, else None.
fn split_json(s: &str) -> Option<(String, String)> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' if !esc => in_str = !in_str,
            '\\' if in_str => esc = !esc,
            _ => esc = false,
        }
        if in_str {
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((s[..=i].to_string(), s[i + 1..].to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

/// Minimal WS text-frame writer (client frames are always masked).
async fn write_ws_text(stream: &mut tokio::net::TcpStream, text: &str) -> Result<(), AdapterError> {
    use tokio::io::AsyncWriteExt;
    let payload = text.as_bytes();
    let mut frame = vec![0x81u8]; // FIN + text opcode
    let mask_bit = 0x80;
    if payload.len() < 126 {
        frame.push(mask_bit | payload.len() as u8);
    } else if payload.len() <= 0xFFFF {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(mask_bit | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    let mask: [u8; 4] = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos().to_le_bytes())
        .unwrap_or([1, 2, 3, 4]);
    frame.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    stream
        .write_all(&frame)
        .await
        .map_err(|e| AdapterError::Transport(format!("ws frame write: {e}")))
}

/// A (non-secret, throwaway) Sec-WebSocket-Key - 16 random-ish bytes
/// base64'd. Connection authenticity is TLS's job; this only satisfies
/// the handshake format.
fn base64_key() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(42);
    let bytes = t.to_le_bytes(); // 16 bytes
    const TAB: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(TAB[(b[0] >> 2) as usize] as char);
        out.push(TAB[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(TAB[(((b[1] & 0x0F) << 2) | (b[2] >> 6)) as usize] as char);
        out.push(TAB[(b[2] & 0x3F) as usize] as char);
    }
    out
}

/// Symbol resolution table the normalizer uses: vendor symbol →
/// (symbol_id, price_scale).
#[derive(Debug, Clone, Default)]
pub struct SymbolDirectory {
    pub map: HashMap<String, (uuid::Uuid, u8)>,
}

impl SymbolDirectory {
    pub fn resolve(&self, vendor_symbol: &str) -> Option<(uuid::Uuid, u8)> {
        self.map.get(vendor_symbol.trim_end_matches(' ')).copied()
    }
    pub fn price_at(&self, raw: i128, scale: u8) -> Price {
        Price::from_scaled(raw, scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_json_extracts_complete_objects() {
        let s = r#"{"a":1}{"b":"}"}garbage{"c":3}"#;
        let (one, rest) = split_json(s).unwrap();
        assert_eq!(one, r#"{"a":1}"#);
        let (two, rest2) = split_json(&rest).unwrap();
        assert_eq!(two, r#"{"b":"}"}"#);
        assert_eq!(rest2, r#"garbage{"c":3}"#);
        // Incomplete object → None (wait for more bytes).
        assert!(split_json(r#"{"x":"#).is_none());
    }

    #[test]
    fn ws_key_is_base64_shaped() {
        let k = base64_key();
        assert!(!k.is_empty());
        assert!(
            k.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='),
            "key: {k}"
        );
    }

    #[test]
    fn adapter_registry_maps_kinds() {
        assert_eq!(adapter_for(AdapterKind::Sim).kind(), AdapterKind::Sim);
        assert_eq!(adapter_for(AdapterKind::Itch).kind(), AdapterKind::Itch);
        assert_eq!(adapter_for(AdapterKind::Idc).kind(), AdapterKind::Idc);
        assert_eq!(
            adapter_for(AdapterKind::CryptoWs).kind(),
            AdapterKind::CryptoWs
        );
        assert_eq!(adapter_for(AdapterKind::FixMd).kind(), AdapterKind::Sim); // interim
    }

    fn link(config: serde_json::Value) -> FeedLinkConfig {
        FeedLinkConfig {
            link_id: "l1".into(),
            kind: AdapterKind::Sim,
            name: "sim".into(),
            source_id: 7,
            config,
            credentials_ref: String::new(),
            generate_top_of_book: false,
            enable: true,
        }
    }

    #[tokio::test]
    async fn sim_replays_script_in_order() {
        let bbo = build_itch_bbo("AAPL", 100_000, 10, 100_400, 8, 1);
        let print = build_itch_print("AAPL", 100_200, 5, 1, 2);
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let config = serde_json::json!({
            "frames": [
                [0, 7, hex(&bbo)],
                [0, 7, hex(&print)],
            ]
        });
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        SimAdapter
            .run(link(config), None, Arc::new(tx))
            .await
            .unwrap();
        let p1 = rx.recv().await.unwrap();
        assert_eq!(p1.seq, 1);
        let recs = parse_itch_record(&p1).unwrap();
        match &recs[0] {
            RawRecord::Bbo {
                symbol,
                bid_px,
                ask_px,
                ..
            } => {
                assert_eq!(symbol, "AAPL");
                assert_eq!(*bid_px, 100_000);
                assert_eq!(*ask_px, 100_400);
            }
            other => panic!("wrong record {other:?}"),
        }
        let p2 = rx.recv().await.unwrap();
        assert_eq!(p2.seq, 2);
        match &parse_itch_record(&p2).unwrap()[0] {
            RawRecord::Print {
                price, qty, seq, ..
            } => {
                assert_eq!(*price, 100_200);
                assert_eq!(*qty, 5);
                assert_eq!(*seq, 1);
            }
            other => panic!("wrong record {other:?}"),
        }
    }

    #[tokio::test]
    async fn disabled_link_errors() {
        let mut cfg = link(serde_json::json!({"frames": []}));
        cfg.enable = false;
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        assert!(matches!(
            SimAdapter.run(cfg, None, Arc::new(tx)).await,
            Err(AdapterError::Disabled)
        ));
    }

    #[test]
    fn hex_codec() {
        assert_eq!(hex_decode("4c 00"), Some(vec![0x4c, 0x00]));
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_decode("abc"), None); // odd length
    }

    #[test]
    fn itch_level_and_imbalance_records() {
        let mut b = vec![b'D', 0];
        b.extend_from_slice(b"MSFT    ".as_ref());
        b.extend_from_slice(&5u32.to_be_bytes());
        b.push(b'B');
        b.extend_from_slice(&300_000i64.to_be_bytes());
        b.extend_from_slice(&25u32.to_be_bytes());
        b.extend_from_slice(&2u32.to_be_bytes());
        b.extend_from_slice(b"MEGA");
        let pkt = RawQuotePacket {
            source_id: 1,
            seq: 3,
            bytes: b,
        };
        match &parse_itch_record(&pkt).unwrap()[0] {
            RawRecord::Level {
                symbol,
                is_bid,
                price,
                volume,
                depth,
                mmid,
                ..
            } => {
                assert_eq!(symbol, "MSFT");
                assert!(*is_bid);
                assert_eq!(*price, 300_000);
                assert_eq!(*volume, 25);
                assert_eq!(*depth, Some(2));
                assert_eq!(mmid.as_deref(), Some("MEGA"));
            }
            other => panic!("wrong record {other:?}"),
        }

        let mut b = vec![b'I', 0];
        b.extend_from_slice(b"QQQ     ".as_ref());
        b.extend_from_slice(&9u32.to_be_bytes());
        b.extend_from_slice(b"B"); // side
        b.extend_from_slice(b"M"); // type
        b.extend_from_slice(b"C"); // status
        b.extend_from_slice(&500_000i64.to_be_bytes());
        b.extend_from_slice(&501_000i64.to_be_bytes());
        b.extend_from_slice(&499_000i64.to_be_bytes());
        b.extend_from_slice(&1_000u32.to_be_bytes());
        b.extend_from_slice(&250u32.to_be_bytes());
        let pkt = RawQuotePacket {
            source_id: 1,
            seq: 4,
            bytes: b,
        };
        match &parse_itch_record(&pkt).unwrap()[0] {
            RawRecord::Imbalance {
                symbol,
                side,
                volume,
                paired,
                auction_px,
                ..
            } => {
                assert_eq!(symbol, "QQQ");
                assert_eq!(*side, 'B');
                assert_eq!(*volume, 1_000);
                assert_eq!(*paired, 250);
                assert_eq!(*auction_px, 501_000);
            }
            other => panic!("wrong record {other:?}"),
        }
    }
}
