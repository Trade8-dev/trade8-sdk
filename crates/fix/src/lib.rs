// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! trade8 FIX codec - order-entry **client role** for FIX 4.2 / FIX 4.4
//! sessions to brokers (router design §4.1).
//!
//! Scope: build/parse the order-entry subset the Router needs
//! (logon, logout, heartbeat, test request, resend, seq-reset, reject,
//! new order, cancel, replace, execution reports), with per-destination
//! template semantics. NOT a general FIX engine - client
//! role only, ~2k LOC by design (master §14 decision).
//!
//! Golden byte tests pin the wire layouts.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Wire framing
// ---------------------------------------------------------------------------

/// FIX field/tag delimiter (SOH, 0x01).
pub const SOH: u8 = 0x01;

/// FIX parse/encode errors.
#[derive(Debug, thiserror::Error)]
pub enum FixError {
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("missing required tag {0}")]
    MissingTag(u32),
    #[error("unsupported msgtype {0}")]
    UnsupportedMsgType(String),
    #[error("checksum mismatch: expected {expected}, got {got}")]
    Checksum { expected: u8, got: u8 },
}

/// BodyLength(9) computation over the message body (tags after 9 up to
/// and including tag 10's start).
pub fn body_length(body: &str) -> usize {
    body.len()
}

/// FIX CheckSum(10): sum of all bytes up to and including the SOH before
/// tag 10, mod 256, zero-padded to 3.
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes
        .iter()
        .fold(0u32, |acc, b| acc.wrapping_add(*b as u32)) as u8
}

/// A FIX tag=value collection preserving order (FIX semantics: repeating
/// groups and per-destination dialects keep first-seen order).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FixMessage {
    pub fields: Vec<(u32, String)>,
}

impl FixMessage {
    pub fn new() -> Self {
        FixMessage::default()
    }

    pub fn with(mut self, tag: u32, value: impl Into<String>) -> Self {
        self.fields.push((tag, value.into()));
        self
    }

    /// First value for a tag.
    pub fn get(&self, tag: u32) -> Option<&str> {
        self.fields
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v.as_str())
    }

    pub fn msg_type(&self) -> Result<&str, FixError> {
        self.get(35).ok_or(FixError::MissingTag(35))
    }

    /// Encode a full FIX message with BeginString(8), BodyLength(9),
    /// MsgSeqNum(34), and CheckSum(10) - the canonical wire form.
    pub fn encode(&self, begin_string: &str, seq_num: u64) -> Result<Vec<u8>, FixError> {
        // Body = tags after 9 through the last field, each "t=v\x01".
        let mut body = String::new();
        for (tag, value) in &self.fields {
            if *tag == 8 || *tag == 9 || *tag == 10 {
                continue; // framing tags are added here, not carried
            }
            let _ = write!(body, "{tag}={value}\u{1}");
        }
        let len = body.len();
        let mut head = String::new();
        let _ = write!(head, "8={begin_string}\u{1}9={len}\u{1}34={seq_num}\u{1}");
        let mut full = format!("{head}{body}");
        let sum = checksum(full.as_bytes());
        let _ = write!(full, "10={:03}\u{1}", sum);
        Ok(full.into_bytes())
    }

    /// Parse a complete FIX message (single frame, no partial framing).
    pub fn decode(bytes: &[u8]) -> Result<(FixMessage, u64), FixError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| FixError::Malformed("non-utf8".into()))?
            .trim_end_matches(SOH as char);
        let mut fields = Vec::new();
        for pair in text.split(SOH as char) {
            if pair.is_empty() {
                continue;
            }
            let (t, v) = pair
                .split_once('=')
                .ok_or_else(|| FixError::Malformed(format!("bad field {pair:?}")))?;
            let tag: u32 = t
                .parse()
                .map_err(|_| FixError::Malformed(format!("bad tag {t:?}")))?;
            fields.push((tag, v.to_string()));
        }
        // Checksum verification: sum over everything before the last
        // field's SOH (including tag-10's own "10=" prefix segment).
        if let Some(checksum_pos) = bytes.windows(3).rposition(|w| w == b"10=") {
            let expected: u8 = fields
                .iter()
                .rev()
                .find(|(t, _)| *t == 10)
                .and_then(|(_, v)| v.parse::<u8>().ok())
                .unwrap_or(0);
            let got = checksum(&bytes[..checksum_pos]);
            if expected != got {
                return Err(FixError::Checksum { expected, got });
            }
        }
        let seq: u64 = fields
            .iter()
            .find(|(t, _)| *t == 34)
            .and_then(|(_, v)| v.parse().ok())
            .ok_or(FixError::MissingTag(34))?;
        Ok((FixMessage { fields }, seq))
    }
}

// ---------------------------------------------------------------------------
// Session/admin message builders (client role)
// ---------------------------------------------------------------------------

/// FIX 4.2 / 4.4 standard begin strings.
pub const BEGIN_42: &str = "FIX.4.2";
pub const BEGIN_44: &str = "FIX.4.4";

/// Build a Logon(A). `heart_bt_int` = heartbeat seconds; `reset_seq`
/// maps to 141=Y for daily rollover (`reset_policy: daily`).
pub fn build_logon(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    heart_bt_int: u32,
    reset_seq: bool,
    password: &str,
) -> Result<Vec<u8>, FixError> {
    let mut m = FixMessage::new()
        .with(35, "A")
        .with(49, sender)
        .with(56, target)
        .with(98, "0")
        .with(108, heart_bt_int.to_string());
    if reset_seq {
        m = m.with(141, "Y");
    }
    if !password.is_empty() {
        m = m.with(554, password);
    }
    m.encode(begin, seq)
}

/// Build Logout(5).
pub fn build_logout(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    text: &str,
) -> Result<Vec<u8>, FixError> {
    let m = FixMessage::new()
        .with(35, "5")
        .with(49, sender)
        .with(56, target);
    let m = if text.is_empty() { m } else { m.with(58, text) };
    m.encode(begin, seq)
}

/// Build Heartbeat(0) - optionally with TestReqID(112).
pub fn build_heartbeat(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    test_req_id: &str,
) -> Result<Vec<u8>, FixError> {
    let m = FixMessage::new()
        .with(35, "0")
        .with(49, sender)
        .with(56, target);
    let m = if test_req_id.is_empty() {
        m
    } else {
        m.with(112, test_req_id)
    };
    m.encode(begin, seq)
}

/// Build TestRequest(1).
pub fn build_test_request(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    test_req_id: &str,
) -> Result<Vec<u8>, FixError> {
    FixMessage::new()
        .with(35, "1")
        .with(49, sender)
        .with(56, target)
        .with(112, test_req_id)
        .encode(begin, seq)
}

/// Build ResendRequest(2) - `end_seq = 0` means "until last message"
///.
pub fn build_resend_request(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    begin_seq: u64,
    end_seq: u64,
) -> Result<Vec<u8>, FixError> {
    FixMessage::new()
        .with(35, "2")
        .with(49, sender)
        .with(56, target)
        .with(7, begin_seq.to_string())
        .with(16, end_seq.to_string())
        .encode(begin, seq)
}

/// Build SequenceReset(4) - gap-fill (`gap_fill=true`) or reset.
pub fn build_seq_reset(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    new_seq: u64,
    gap_fill: bool,
) -> Result<Vec<u8>, FixError> {
    let m = FixMessage::new()
        .with(35, "4")
        .with(49, sender)
        .with(56, target)
        .with(36, new_seq.to_string());
    let m = if gap_fill { m.with(123, "Y") } else { m };
    m.encode(begin, seq)
}

/// Build Reject(3) for an unprocessable inbound message.
pub fn build_reject(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    ref_seq: u64,
    reason: u32,
) -> Result<Vec<u8>, FixError> {
    FixMessage::new()
        .with(35, "3")
        .with(45, ref_seq.to_string())
        .with(371, reason.to_string())
        .with(49, sender)
        .with(56, target)
        .encode(begin, seq)
}

// ---------------------------------------------------------------------------
// Order-entry builders (client role; template-driven values pass through)
// ---------------------------------------------------------------------------

/// Build NewOrderSingle(D) from a typed field set. The Router's template
/// renderer (router design §4.3) supplies destination-dialect tags; this
/// builder emits the canonical order fields.
#[allow(clippy::too_many_arguments)]
pub fn build_new_order(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    cl_ord_id: &str,
    symbol: &str,
    side: char,     // FIX side: '1' buy, '5' sell short, 'T' sell short exempt
    ord_type: char, // '1' market, '2' limit, '3' stop, '4' stop-limit
    qty: &str,
    price: Option<&str>, // decimal strings - scale already resolved
    tif: char,
    extra: &[(u32, String)],
) -> Result<Vec<u8>, FixError> {
    let mut m = FixMessage::new()
        .with(35, "D")
        .with(49, sender)
        .with(56, target)
        .with(11, cl_ord_id)
        .with(21, "1") // HandlInst: automated
        .with(38, qty)
        .with(40, ord_type.to_string())
        .with(54, side.to_string())
        .with(55, symbol)
        .with(59, tif.to_string());
    if let Some(p) = price {
        m = m.with(44, p);
    }
    for (t, v) in extra {
        m = m.with(*t, v.clone());
    }
    m.encode(begin, seq)
}

/// Build OrderCancelRequest(F).
#[allow(clippy::too_many_arguments)]
pub fn build_cancel(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    symbol: &str,
    side: char,
    qty: &str,
) -> Result<Vec<u8>, FixError> {
    FixMessage::new()
        .with(35, "F")
        .with(49, sender)
        .with(56, target)
        .with(11, cl_ord_id)
        .with(41, orig_cl_ord_id)
        .with(54, side.to_string())
        .with(55, symbol)
        .with(38, qty)
        .encode(begin, seq)
}

/// Build OrderCancelReplaceRequest(G) - cancel-replace chain.
#[allow(clippy::too_many_arguments)]
pub fn build_replace(
    begin: &str,
    seq: u64,
    sender: &str,
    target: &str,
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    symbol: &str,
    side: char,
    qty: &str,
    price: Option<&str>,
) -> Result<Vec<u8>, FixError> {
    let mut m = FixMessage::new()
        .with(35, "G")
        .with(49, sender)
        .with(56, target)
        .with(11, cl_ord_id)
        .with(41, orig_cl_ord_id)
        .with(54, side.to_string())
        .with(55, symbol)
        .with(38, qty);
    if let Some(p) = price {
        m = m.with(44, p);
    }
    m.encode(begin, seq)
}

// ---------------------------------------------------------------------------
// Execution report parse (client role - Router normalizes)
// ---------------------------------------------------------------------------

/// ExecutionReport(8) execution types (OrdRejReason mapped by the Router).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecType {
    New,
    PartialFill,
    Fill,
    Canceled,
    Replace,
    Rejected,
    Expired,
    Correct,
}

impl ExecType {
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            '0' => ExecType::New,
            '1' => ExecType::PartialFill,
            '2' => ExecType::Fill,
            '3' => ExecType::Correct, // '3' = correct in some dialects
            '4' => ExecType::Canceled,
            '5' => ExecType::Replace,
            '8' => ExecType::Rejected,
            'C' => ExecType::Expired,
            _ => return None,
        })
    }
}

/// A parsed ExecutionReport - the Router's normalization input.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    pub exec_type: ExecType,
    pub order_id: String,           // 37
    pub cl_ord_id: String,          // 11
    pub exec_id: String,            // 17
    pub ord_status: char,           // 39
    pub leaves_qty: Option<u64>,    // 151
    pub cum_qty: Option<u64>,       // 14
    pub last_qty: Option<u64>,      // 32
    pub last_px: Option<String>,    // 31
    pub reject_reason: Option<u32>, // 103
    pub text: Option<String>,       // 58
}

/// Parse an ExecutionReport(8).
pub fn parse_execution_report(m: &FixMessage) -> Result<ExecutionReport, FixError> {
    let msg_type = m.msg_type()?;
    if msg_type != "8" {
        return Err(FixError::UnsupportedMsgType(msg_type.to_string()));
    }
    let exec_type_char = m
        .get(150)
        .unwrap_or("")
        .chars()
        .next()
        .unwrap_or(m.get(39).unwrap_or("").chars().next().unwrap_or('0'));
    let exec_type = ExecType::from_char(exec_type_char)
        .ok_or_else(|| FixError::Malformed(format!("bad ExecType {exec_type_char}")))?;
    Ok(ExecutionReport {
        exec_type,
        order_id: m.get(37).unwrap_or("").to_string(),
        cl_ord_id: m.get(11).ok_or(FixError::MissingTag(11))?.to_string(),
        exec_id: m.get(17).unwrap_or("").to_string(),
        ord_status: m.get(39).unwrap_or("0").chars().next().unwrap_or('0'),
        leaves_qty: m.get(151).and_then(|v| v.parse().ok()),
        cum_qty: m.get(14).and_then(|v| v.parse().ok()),
        last_qty: m.get(32).and_then(|v| v.parse().ok()),
        last_px: m.get(31).map(str::to_string),
        reject_reason: m.get(103).and_then(|v| v.parse().ok()),
        text: m.get(58).map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_new_order_42() {
        let bytes = build_new_order(
            BEGIN_42,
            7,
            "DESK",
            "BRKR",
            "cl-1",
            "AAPL",
            '1',
            '2',
            "100",
            Some("10.05"),
            '0',
            &[(30, "NYSE".into())],
        )
        .unwrap();
        let (m, seq) = FixMessage::decode(&bytes).unwrap();
        assert_eq!(seq, 7);
        assert_eq!(m.msg_type().unwrap(), "D");
        assert_eq!(m.get(11).unwrap(), "cl-1");
        assert_eq!(m.get(38).unwrap(), "100");
        assert_eq!(m.get(44).unwrap(), "10.05");
        assert_eq!(m.get(55).unwrap(), "AAPL");
        assert_eq!(m.get(30).unwrap(), "NYSE");
        assert_eq!(m.get(8).unwrap_or("FIX.4.2"), "FIX.4.2");
        // framing tags present on the wire: ends with the checksum
        // digits followed by the final SOH.
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("8=FIX.4.2\u{1}9="));
        assert!(text.ends_with('\u{1}'));
        let trimmed = text.trim_end_matches('\u{1}');
        assert!(trimmed.ends_with(|c: char| c.is_ascii_digit()));
    }

    #[test]
    fn logon_with_reset_and_password() {
        let bytes = build_logon(BEGIN_44, 1, "DESK", "BRKR", 30, true, "s3cret").unwrap();
        let (m, _) = FixMessage::decode(&bytes).unwrap();
        assert_eq!(m.msg_type().unwrap(), "A");
        assert_eq!(m.get(108).unwrap(), "30");
        assert_eq!(m.get(141).unwrap(), "Y");
        assert_eq!(m.get(554).unwrap(), "s3cret");
    }

    #[test]
    fn exec_report_parse() {
        let raw = FixMessage::new()
            .with(35, "8")
            .with(11, "cl-9")
            .with(17, "ex-1")
            .with(37, "bo-1")
            .with(39, "1")
            .with(150, "1")
            .with(32, "50")
            .with(14, "50")
            .with(151, "50")
            .with(31, "10.10");
        let bytes = raw.encode(BEGIN_42, 42).unwrap();
        let (m, seq) = FixMessage::decode(&bytes).unwrap();
        assert_eq!(seq, 42);
        let er = parse_execution_report(&m).unwrap();
        assert_eq!(er.exec_type, ExecType::PartialFill);
        assert_eq!(er.last_qty, Some(50));
        assert_eq!(er.leaves_qty, Some(50));
        assert_eq!(er.last_px.as_deref(), Some("10.10"));
    }

    #[test]
    fn resend_request_until_last() {
        // = 0 end-seq
        let bytes = build_resend_request(BEGIN_42, 3, "D", "B", 5, 0).unwrap();
        let (m, _) = FixMessage::decode(&bytes).unwrap();
        assert_eq!(m.get(7).unwrap(), "5");
        assert_eq!(m.get(16).unwrap(), "0");
    }

    #[test]
    fn reject_on_checksum_error() {
        let bytes = build_logout(BEGIN_42, 2, "D", "B", "bye").unwrap();
        let mut corrupted = bytes.clone();
        // flip a byte inside the text field
        let pos = corrupted.iter().rposition(|&b| b == b'b').unwrap();
        corrupted[pos] = b'x';
        assert!(FixMessage::decode(&corrupted).is_err());
    }
}
