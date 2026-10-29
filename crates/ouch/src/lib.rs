// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! trade8 OUCH 3.1 codec - order-entry **client role** (router design
//! §4.1) - binary framed messages for NASDAQ-style OUCH links.
//!
//! Scope: EnterOrder, CancelOrder, ReplaceOrder outbound; Accepted,
//! Rejected, Canceled, Executed, Replaced, CanceledByOther inbound; the
//! session header (Debug/Live flag + sequence). Client role only.
//!
//! BuildMessage/ParsePacket/BuildHeartbeat`), the `SrvMessage`-era binary
//! layout. Golden byte tests pin the packet forms.
//!
//! Wire layout (OUCH 3.1 client role):
//! - Packet frame: `u16 be length | payload`
//! - Enter Order (0x00 'O' block):
//!   `"O" | order_token[14] | buy_sell char | shares u32be | stock[8] |
//!   price u64be (×10⁴) | time_in_force char | firm[4] | display char |
//!   cross_type char | capacity u32be | intermarket_sweep u32be`
//!   (the field order follows the spec; fixed-width
//!   fields are space- or zero-padded per the OUCH spec)
//! - Accepted ('A'): `"A" | order_token[14] | buy_sell | shares | stock[8] |
//!   price | time_in_force | firm | display | capacity | state |
//!   order_ref u64`
//! - Executed ('E'): `"E" | order_token[14] | executed_shares u32 |
//!   execution_price u64 | time_to_fill | match_number u64`

use std::io::{Read, Write};

/// OUCH codec errors.
#[derive(Debug, thiserror::Error)]
pub enum OuchError {
    #[error("malformed packet: {0}")]
    Malformed(String),
    #[error("truncated packet: need {need}, got {got}")]
    Truncated { need: usize, got: usize },
    #[error("unknown block type {0}")]
    UnknownType(u8),
}

/// Max outbound order-token field width (OUCH 3.1: 14 bytes).
pub const TOKEN_LEN: usize = 14;
/// Stock field width.
pub const STOCK_LEN: usize = 8;
/// Firm (MPID) field width.
pub const FIRM_LEN: usize = 4;

// ------------------------------------------------------------------------
// Field encode/decode helpers (fixed-width, ASCII, padded)
// ------------------------------------------------------------------------

fn put_fixed(dst: &mut Vec<u8>, s: &str, width: usize) -> Result<(), OuchError> {
    let bytes = s.as_bytes();
    if bytes.len() > width {
        return Err(OuchError::Malformed(format!("field {s:?} exceeds {width}")));
    }
    dst.extend_from_slice(bytes);
    dst.extend(std::iter::repeat_n(b' ', width - bytes.len()));
    Ok(())
}

fn get_fixed(src: &[u8], width: usize) -> Result<(&[u8], &[u8]), OuchError> {
    if src.len() < width {
        return Err(OuchError::Truncated {
            need: width,
            got: src.len(),
        });
    }
    Ok((&src[..width], &src[width..]))
}

fn put_u32be(dst: &mut Vec<u8>, v: u32) {
    dst.extend_from_slice(&v.to_be_bytes());
}

fn put_u64be(dst: &mut Vec<u8>, v: u64) {
    dst.extend_from_slice(&v.to_be_bytes());
}

fn get_u32be(src: &[u8]) -> Result<(u32, &[u8]), OuchError> {
    let (raw, rest) = get_fixed(src, 4)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(raw);
    Ok((u32::from_be_bytes(a), rest))
}

fn get_u64be(src: &[u8]) -> Result<(u64, &[u8]), OuchError> {
    let (raw, rest) = get_fixed(src, 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(raw);
    Ok((u64::from_be_bytes(a), rest))
}

fn fixed_str(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .trim_end_matches(' ')
        .to_string()
}

// ------------------------------------------------------------------------
// Outbound messages (client role)
// ------------------------------------------------------------------------

/// Time-in-force codes (OUCH 3.1 §EnterOrder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuchTif {
    Day,
    Ioc,
    IocPlus, // 'K'
    Opening,
    Closing,
    Gtx,
    Gtt,
    Gtc, // broker dialect extension beyond the 3.1 core set
}

impl OuchTif {
    pub fn as_char(self) -> u8 {
        match self {
            OuchTif::Day => b'O',
            OuchTif::Ioc => b'I',
            OuchTif::IocPlus => b'K',
            OuchTif::Opening => b'M',
            OuchTif::Closing => b'P',
            OuchTif::Gtx => b'X',
            OuchTif::Gtt => b'T',
            OuchTif::Gtc => b'D',
        }
    }
}

/// OUCH Buy/Sell indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuchSide {
    Buy,
    Sell,
    SellShort,
    SellShortExempt,
}

impl OuchSide {
    pub fn as_char(self) -> u8 {
        match self {
            OuchSide::Buy => b'B',
            OuchSide::Sell => b'S',
            OuchSide::SellShort => b'S',
            OuchSide::SellShortExempt => b'T',
        }
    }
}

/// Display indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuchDisplay {
    Visible,
    HideNonDisplayed,
    VisiblePostOnly,
}

impl OuchDisplay {
    pub fn as_char(self) -> u8 {
        match self {
            OuchDisplay::Visible => b'Y',
            OuchDisplay::HideNonDisplayed => b'N',
            OuchDisplay::VisiblePostOnly => b'P',
        }
    }
}

/// An EnterOrder (0x00 'O') block body - the Router builds one per
/// accepted order routed to an OUCH destination.
#[derive(Debug, Clone, PartialEq)]
pub struct EnterOrder {
    pub order_token: String,
    pub side: OuchSide,
    pub shares: u32,
    pub stock: String,
    /// Price ×10⁴ (the OUCH native scale).
    pub price_scaled: u64,
    pub tif: OuchTif,
    pub firm: String,
    pub display: OuchDisplay,
    /// Cross type: 0 none, 1 opening, 2 closing.
    pub cross_type: u8,
}

/// Encode an EnterOrder block (no frame - the caller frames).
pub fn encode_enter_order(o: &EnterOrder) -> Result<Vec<u8>, OuchError> {
    let mut body =
        Vec::with_capacity(1 + TOKEN_LEN + 1 + 4 + STOCK_LEN + 8 + 1 + FIRM_LEN + 1 + 1 + 4 + 4);
    body.push(b'O');
    put_fixed(&mut body, &o.order_token, TOKEN_LEN)?;
    body.push(o.side.as_char());
    put_u32be(&mut body, o.shares);
    put_fixed(&mut body, &o.stock, STOCK_LEN)?;
    put_u64be(&mut body, o.price_scaled);
    body.push(o.tif.as_char());
    put_fixed(&mut body, &o.firm, FIRM_LEN)?;
    body.push(o.display.as_char());
    body.push(o.cross_type);
    // Capacity + intermarket sweep
    put_u32be(&mut body, 0);
    put_u32be(&mut body, 0);
    Ok(body)
}

/// A CancelOrder (0x02 'X') block.
#[derive(Debug, Clone, PartialEq)]
pub struct CancelOrder {
    pub order_token: String,
    pub shares: u32,
}

pub fn encode_cancel_order(o: &CancelOrder) -> Result<Vec<u8>, OuchError> {
    let mut body = Vec::with_capacity(1 + TOKEN_LEN + 4);
    body.push(b'X');
    put_fixed(&mut body, &o.order_token, TOKEN_LEN)?;
    put_u32be(&mut body, o.shares);
    Ok(body)
}

/// Frame a block: `u16 be length | payload` (the OUCH packet form).
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + payload.len());
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Read one framed block from a stream. Returns the block payload.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Vec<u8>, OuchError> {
    let mut len_buf = [0u8; 2];
    r.read_exact(&mut len_buf)
        .map_err(|e| OuchError::Malformed(format!("frame read: {e}")))?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)
        .map_err(|e| OuchError::Malformed(format!("payload read: {e}")))?;
    Ok(payload)
}

// ------------------------------------------------------------------------
// Inbound messages (client role - Router normalizes)
// ------------------------------------------------------------------------

/// Inbound OUCH event (client role receives order-state replies).
#[derive(Debug, Clone, PartialEq)]
pub enum OuchInbound {
    Accepted {
        order_token: String,
        side: u8,
        shares: u32,
        stock: String,
        price_scaled: u64,
        order_ref: u64,
    },
    Rejected {
        order_token: String,
        reason: u32,
    },
    Canceled {
        order_token: String,
        reason: u32,
    },
    Executed {
        order_token: String,
        executed_shares: u32,
        execution_price: u64,
        match_number: u64,
    },
    Replaced {
        order_token: String,
        shares: u32,
        price_scaled: u64,
    },
    CanceledByOther {
        order_token: String,
    },
}

/// Parse one inbound block (payload after framing).
pub fn parse_inbound(payload: &[u8]) -> Result<OuchInbound, OuchError> {
    if payload.is_empty() {
        return Err(OuchError::Malformed("empty block".into()));
    }
    let mut p = payload;
    let tag = p[0];
    p = &p[1..];
    match tag {
        b'A' => {
            let (tok, p) = get_fixed(p, TOKEN_LEN)?;
            let side = p[0];
            let p = &p[1..];
            let (shares, p) = get_u32be(p)?;
            let (stock, p) = get_fixed(p, STOCK_LEN)?;
            let (price, p) = get_u64be(p)?;
            let (_tif, p) = get_fixed(p, 1)?;
            let (_firm, p) = get_fixed(p, FIRM_LEN)?;
            let (_display, p) = get_fixed(p, 1)?;
            let (_capacity, p) = get_u32be(p)?;
            let (_cross, p) = get_fixed(p, 1)?;
            let (order_ref, _p) = get_u64be(p)?;
            Ok(OuchInbound::Accepted {
                order_token: fixed_str(tok),
                side,
                shares,
                stock: fixed_str(stock),
                price_scaled: price,
                order_ref,
            })
        }
        b'J' => {
            // Rejected ('J'): order_token | reason u32be
            let (tok, p) = get_fixed(p, TOKEN_LEN)?;
            let (reason, _p) = get_u32be(p)?;
            Ok(OuchInbound::Rejected {
                order_token: fixed_str(tok),
                reason,
            })
        }
        b'C' => {
            // Canceled ('C'): order_token | reason u32be
            let (tok, p) = get_fixed(p, TOKEN_LEN)?;
            let (reason, _p) = get_u32be(p)?;
            Ok(OuchInbound::Canceled {
                order_token: fixed_str(tok),
                reason,
            })
        }
        b'E' => {
            // Executed ('E'): order_token | executed_shares u32be | execution_price u64be | time | match_number u64be
            let (tok, p) = get_fixed(p, TOKEN_LEN)?;
            let (shares, p) = get_u32be(p)?;
            let (price, p) = get_u64be(p)?;
            let (_time, p) = get_fixed(p, 1)?;
            let (match_number, _p) = get_u64be(p)?;
            Ok(OuchInbound::Executed {
                order_token: fixed_str(tok),
                executed_shares: shares,
                execution_price: price,
                match_number,
            })
        }
        b'U' => {
            // Replaced ('U'): new order_token | shares | price
            let (tok, p) = get_fixed(p, TOKEN_LEN)?;
            let (shares, p) = get_u32be(p)?;
            let (price, _p) = get_u64be(p)?;
            Ok(OuchInbound::Replaced {
                order_token: fixed_str(tok),
                shares,
                price_scaled: price,
            })
        }
        b'Q' => {
            let (tok, _p) = get_fixed(p, TOKEN_LEN)?;
            Ok(OuchInbound::CanceledByOther {
                order_token: fixed_str(tok),
            })
        }
        other => Err(OuchError::UnknownType(other)),
    }
}

/// Convenience: encode + frame an EnterOrder in one step.
pub fn frame_enter_order(o: &EnterOrder) -> Result<Vec<u8>, OuchError> {
    Ok(frame(&encode_enter_order(o)?))
}

/// A `Write`-driven sink used by tests and the Router's journal.
pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    w.write_all(&(payload.len() as u16).to_be_bytes())?;
    w.write_all(payload)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_enter() -> EnterOrder {
        EnterOrder {
            order_token: "tok-1".into(),
            side: OuchSide::Buy,
            shares: 100,
            stock: "AAPL".into(),
            price_scaled: 100_500, // 10.05 ×10⁴
            tif: OuchTif::Day,
            firm: "MEGA".into(),
            display: OuchDisplay::Visible,
            cross_type: 0,
        }
    }

    #[test]
    fn enter_order_roundtrip_bytes() {
        let payload = encode_enter_order(&sample_enter()).unwrap();
        // block type + fixed widths pinned:
        // O(1) | token(14) | side(1) | shares(4) | stock(8) | px(8) | tif(1)
        // | firm(4) | display(1) | cross(1) | capacity(4) | sweep(4)
        assert_eq!(payload[0], b'O');
        assert_eq!(&payload[1..15], b"tok-1         ");
        assert_eq!(payload[15], b'B');
        assert_eq!(&payload[16..20], &100u32.to_be_bytes());
        assert_eq!(&payload[20..28], b"AAPL    ");
        assert_eq!(&payload[28..36], &100_500u64.to_be_bytes());
        // framed length prefix
        let framed = frame(&payload);
        assert_eq!(&framed[..2], &(payload.len() as u16).to_be_bytes());
    }

    #[test]
    fn accepted_parse_roundtrip() {
        // build a synthetic Accepted with the same field order the codec
        // writes outbound (Router journals both directions byte-for-byte)
        let mut p = vec![b'A'];
        put_fixed(&mut p, "tok-2", TOKEN_LEN).unwrap();
        p.push(b'S');
        put_u32be(&mut p, 50);
        put_fixed(&mut p, "MSFT", STOCK_LEN).unwrap();
        put_u64be(&mut p, 300_000);
        p.push(b'O');
        put_fixed(&mut p, "MEGA", FIRM_LEN).unwrap();
        p.push(b'Y');
        put_u32be(&mut p, 0);
        p.push(0u8);
        put_u64be(&mut p, 777);
        match parse_inbound(&p).unwrap() {
            OuchInbound::Accepted {
                order_token,
                shares,
                stock,
                price_scaled,
                order_ref,
                ..
            } => {
                assert_eq!(order_token, "tok-2");
                assert_eq!(shares, 50);
                assert_eq!(stock, "MSFT");
                assert_eq!(price_scaled, 300_000);
                assert_eq!(order_ref, 777);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn executed_and_cancel_parse() {
        let mut p = vec![b'E'];
        put_fixed(&mut p, "tok-3", TOKEN_LEN).unwrap();
        put_u32be(&mut p, 25);
        put_u64be(&mut p, 101_000);
        p.push(b'X');
        put_u64be(&mut p, 42);
        match parse_inbound(&p).unwrap() {
            OuchInbound::Executed {
                order_token,
                executed_shares,
                execution_price,
                match_number,
            } => {
                assert_eq!(order_token, "tok-3");
                assert_eq!(executed_shares, 25);
                assert_eq!(execution_price, 101_000);
                assert_eq!(match_number, 42);
            }
            other => panic!("wrong variant: {other:?}"),
        }

        let mut p = vec![b'C'];
        put_fixed(&mut p, "tok-4", TOKEN_LEN).unwrap();
        put_u32be(&mut p, 1);
        match parse_inbound(&p).unwrap() {
            OuchInbound::Canceled {
                order_token,
                reason,
            } => {
                assert_eq!(order_token, "tok-4");
                assert_eq!(reason, 1);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn truncated_and_unknown() {
        assert!(parse_inbound(&[]).is_err());
        assert!(matches!(
            parse_inbound(&[b'Z', 1, 2]),
            Err(OuchError::UnknownType(b'Z'))
        ));
        let mut p = vec![b'A', b'x'];
        assert!(matches!(
            parse_inbound(&p),
            Err(OuchError::Truncated { .. })
        ));
        p.clear();
        assert!(parse_inbound(&p).is_err());
    }

    #[test]
    fn frame_stream_roundtrip() {
        let payload = encode_cancel_order(&CancelOrder {
            order_token: "tok-5".into(),
            shares: 10,
        })
        .unwrap();
        let mut buf = Vec::new();
        write_frame(&mut buf, &payload).unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let back = read_frame(&mut cursor).unwrap();
        assert_eq!(back, payload);
    }
}
