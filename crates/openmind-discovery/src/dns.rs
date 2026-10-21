// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal DNS wire codec for mDNS / DNS-SD (RFC 1035, RFC 6762, RFC 6763).
//!
//! Only what LAN discovery needs: build a query, build a response, and parse
//! a message into [`Record`]s (`PTR`, `SRV`, `TXT`, `A`, `AAAA`). Other record
//! types are skipped. Name compression is decoded (with a jump limit) but
//! never emitted. This module does no I/O.

use crate::{RData, Record};
use std::net::{Ipv4Addr, Ipv6Addr};

pub const TYPE_A: u16 = 1;
pub const TYPE_PTR: u16 = 12;
pub const TYPE_TXT: u16 = 16;
pub const TYPE_AAAA: u16 = 28;
pub const TYPE_SRV: u16 = 33;
pub const TYPE_ANY: u16 = 255;
pub const CLASS_IN: u16 = 1;
/// Top bit of qclass in a question: unicast response requested (RFC 6762 5.4).
pub const QU_BIT: u16 = 0x8000;
/// Top bit of rrclass in an answer: cache flush (RFC 6762 10.2).
pub const FLUSH_BIT: u16 = 0x8000;
const FLAG_QR: u16 = 0x8000;
const FLAG_AA: u16 = 0x0400;
const MAX_JUMPS: usize = 16;
const MAX_NAME: usize = 255;
const MAX_ENTRIES: usize = 256;

/// One question from a message. `name` is lowercase with a trailing dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub name: String,
    pub qtype: u16,
    pub unicast: bool,
}

/// A decoded message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: u16,
    pub response: bool,
    pub questions: Vec<Question>,
    /// Answer, authority and additional records, in wire order.
    pub records: Vec<Record>,
}

/// Wire decode failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub &'static str);

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for WireError {}

/// Build a query. `unicast` sets the QU bit on every question.
pub fn build_query(id: u16, questions: &[(&str, u16)], unicast: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 * questions.len().max(1));
    put_u16(&mut out, id);
    put_u16(&mut out, 0);
    put_u16(&mut out, questions.len() as u16);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    for (name, qtype) in questions {
        put_name(&mut out, name);
        put_u16(&mut out, *qtype);
        put_u16(&mut out, CLASS_IN | if unicast { QU_BIT } else { 0 });
    }
    out
}

/// Build an authoritative response.
///
/// `questions` is echoed (needed for legacy unicast replies, RFC 6762 6.7).
/// `flush` sets the cache-flush bit on non-PTR records (multicast replies only).
pub fn build_response(
    id: u16,
    questions: &[Question],
    answers: &[Record],
    additionals: &[Record],
    ttl: u32,
    flush: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    put_u16(&mut out, id);
    put_u16(&mut out, FLAG_QR | FLAG_AA);
    put_u16(&mut out, questions.len() as u16);
    put_u16(&mut out, answers.len() as u16);
    put_u16(&mut out, 0);
    put_u16(&mut out, additionals.len() as u16);
    for q in questions {
        put_name(&mut out, &q.name);
        put_u16(&mut out, q.qtype);
        put_u16(&mut out, CLASS_IN);
    }
    for rec in answers.iter().chain(additionals) {
        put_record(&mut out, rec, ttl, flush);
    }
    out
}

/// Decode a message. Unknown record types are skipped, not errors.
pub fn parse_message(buf: &[u8]) -> Result<Message, WireError> {
    let mut cur = Cursor { buf, pos: 0 };
    let id = cur.u16()?;
    let flags = cur.u16()?;
    let qd = cur.u16()? as usize;
    let an = cur.u16()? as usize;
    let ns = cur.u16()? as usize;
    let ar = cur.u16()? as usize;
    if qd + an + ns + ar > MAX_ENTRIES {
        return Err(WireError("too many entries"));
    }
    let mut questions = Vec::with_capacity(qd);
    for _ in 0..qd {
        let name = cur.name()?;
        let qtype = cur.u16()?;
        let qclass = cur.u16()?;
        questions.push(Question {
            name,
            qtype,
            unicast: qclass & QU_BIT != 0,
        });
    }
    let mut records = Vec::new();
    for _ in 0..(an + ns + ar) {
        let name = cur.name()?;
        let rtype = cur.u16()?;
        let _class = cur.u16()?;
        let ttl = cur.u32()?;
        let len = cur.u16()? as usize;
        let start = cur.pos;
        let end = start.checked_add(len).ok_or(WireError("rdata overflow"))?;
        if end > buf.len() {
            return Err(WireError("rdata past end"));
        }
        let rdata = match rtype {
            TYPE_A if len == 4 => Some(RData::A(
                Ipv4Addr::new(buf[start], buf[start + 1], buf[start + 2], buf[start + 3])
                    .to_string(),
            )),
            TYPE_AAAA if len == 16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&buf[start..end]);
                Some(RData::Aaaa(Ipv6Addr::from(octets).to_string()))
            }
            TYPE_PTR => Some(RData::Ptr(cur.name()?)),
            TYPE_SRV => {
                let priority = cur.u16()?;
                let weight = cur.u16()?;
                let port = cur.u16()?;
                let target = cur.name()?;
                Some(RData::Srv {
                    priority,
                    weight,
                    port,
                    target,
                })
            }
            TYPE_TXT => Some(RData::Txt(parse_txt(&buf[start..end]))),
            _ => None,
        };
        cur.pos = end;
        if let Some(rdata) = rdata {
            records.push(Record {
                name,
                ttl: Some(ttl),
                rdata,
            });
        }
    }
    Ok(Message {
        id,
        response: flags & FLAG_QR != 0,
        questions,
        records,
    })
}

fn parse_txt(mut data: &[u8]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    while let Some((&len, rest)) = data.split_first() {
        let len = len as usize;
        if len > rest.len() {
            break;
        }
        let (item, tail) = rest.split_at(len);
        data = tail;
        if item.is_empty() || pairs.len() >= 32 {
            continue;
        }
        let (key, value) = match item.iter().position(|&b| b == b'=') {
            Some(i) => (&item[..i], &item[i + 1..]),
            None => (item, &item[..0]),
        };
        let key_ok = !key.is_empty()
            && key.len() <= 63
            && key
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
        let value_ok = value.iter().all(|b| (b' '..=b'~').contains(b));
        if key_ok && value_ok {
            pairs.push((
                String::from_utf8_lossy(key).to_ascii_lowercase(),
                String::from_utf8_lossy(value).into_owned(),
            ));
        }
    }
    pairs
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn u16(&mut self) -> Result<u16, WireError> {
        let bytes = self
            .buf
            .get(self.pos..self.pos + 2)
            .ok_or(WireError("short message"))?;
        self.pos += 2;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, WireError> {
        let bytes = self
            .buf
            .get(self.pos..self.pos + 4)
            .ok_or(WireError("short message"))?;
        self.pos += 4;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a possibly compressed name. Labels are lowercased; bytes outside
    /// `[a-z0-9_-]` become `-` so names stay safe ASCII for ids and logs.
    fn name(&mut self) -> Result<String, WireError> {
        let mut out = String::new();
        let mut pos = self.pos;
        let mut jumped = false;
        let mut jumps = 0;
        loop {
            let len = *self.buf.get(pos).ok_or(WireError("name past end"))? as usize;
            if len & 0xC0 == 0xC0 {
                let low = *self.buf.get(pos + 1).ok_or(WireError("pointer past end"))? as usize;
                if !jumped {
                    self.pos = pos + 2;
                    jumped = true;
                }
                jumps += 1;
                if jumps > MAX_JUMPS {
                    return Err(WireError("compression loop"));
                }
                pos = ((len & 0x3F) << 8) | low;
                continue;
            }
            if len & 0xC0 != 0 {
                return Err(WireError("bad label type"));
            }
            if len == 0 {
                if !jumped {
                    self.pos = pos + 1;
                }
                break;
            }
            let label = self
                .buf
                .get(pos + 1..pos + 1 + len)
                .ok_or(WireError("label past end"))?;
            for &b in label {
                let ch = b.to_ascii_lowercase();
                out.push(if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-' {
                    ch as char
                } else {
                    '-'
                });
            }
            out.push('.');
            if out.len() > MAX_NAME {
                return Err(WireError("name too long"));
            }
            pos += 1 + len;
        }
        if out.is_empty() {
            out.push('.');
        }
        Ok(out)
    }
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() {
            continue;
        }
        let bytes = &label.as_bytes()[..label.len().min(63)];
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    }
    out.push(0);
}

fn put_record(out: &mut Vec<u8>, rec: &Record, ttl: u32, flush: bool) {
    put_name(out, &rec.name);
    let (rtype, shared) = match &rec.rdata {
        RData::A(_) => (TYPE_A, false),
        RData::Aaaa(_) => (TYPE_AAAA, false),
        RData::Ptr(_) => (TYPE_PTR, true),
        RData::Srv { .. } => (TYPE_SRV, false),
        RData::Txt(_) => (TYPE_TXT, false),
    };
    put_u16(out, rtype);
    put_u16(out, CLASS_IN | if flush && !shared { FLUSH_BIT } else { 0 });
    out.extend_from_slice(&rec.ttl.unwrap_or(ttl).min(ttl).to_be_bytes());
    let len_at = out.len();
    put_u16(out, 0);
    match &rec.rdata {
        RData::A(addr) => {
            let ip: Ipv4Addr = addr.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
            out.extend_from_slice(&ip.octets());
        }
        RData::Aaaa(addr) => {
            let ip: Ipv6Addr = addr.parse().unwrap_or(Ipv6Addr::UNSPECIFIED);
            out.extend_from_slice(&ip.octets());
        }
        RData::Ptr(target) => put_name(out, target),
        RData::Srv {
            priority,
            weight,
            port,
            target,
        } => {
            put_u16(out, *priority);
            put_u16(out, *weight);
            put_u16(out, *port);
            put_name(out, target);
        }
        RData::Txt(pairs) => {
            if pairs.is_empty() {
                out.push(0);
            }
            for (key, value) in pairs {
                let item = if value.is_empty() {
                    key.clone()
                } else {
                    format!("{key}={value}")
                };
                let bytes = &item.as_bytes()[..item.len().min(255)];
                out.push(bytes.len() as u8);
                out.extend_from_slice(bytes);
            }
        }
    }
    let rdlen = (out.len() - len_at - 2) as u16;
    out[len_at..len_at + 2].copy_from_slice(&rdlen.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, rdata: RData) -> Record {
        Record {
            name: name.into(),
            ttl: Some(120),
            rdata,
        }
    }

    #[test]
    fn query_roundtrip() {
        let q = build_query(
            7,
            &[
                ("_exo._tcp.local.", TYPE_PTR),
                ("_galion._tcp.local.", TYPE_PTR),
            ],
            true,
        );
        let msg = parse_message(&q).unwrap();
        assert_eq!(msg.id, 7);
        assert!(!msg.response);
        assert_eq!(msg.questions.len(), 2);
        assert_eq!(msg.questions[0].name, "_exo._tcp.local.");
        assert_eq!(msg.questions[1].qtype, TYPE_PTR);
        assert!(msg.questions[0].unicast);
        assert!(msg.records.is_empty());
    }

    #[test]
    fn response_roundtrip_all_types() {
        let answers = [rec(
            "_exo._tcp.local.",
            RData::Ptr("mac._exo._tcp.local.".into()),
        )];
        let extra = [
            rec(
                "mac._exo._tcp.local.",
                RData::Srv {
                    priority: 0,
                    weight: 0,
                    port: 52415,
                    target: "mac.local.".into(),
                },
            ),
            rec(
                "mac._exo._tcp.local.",
                RData::Txt(vec![
                    ("id".into(), "mac-1".into()),
                    ("gpu".into(), String::new()),
                ]),
            ),
            rec("mac.local.", RData::A("192.168.1.20".into())),
            rec("mac.local.", RData::Aaaa("fe80::1".into())),
        ];
        let q = [Question {
            name: "_exo._tcp.local.".into(),
            qtype: TYPE_PTR,
            unicast: false,
        }];
        let wire = build_response(9, &q, &answers, &extra, 10, false);
        let msg = parse_message(&wire).unwrap();
        assert!(msg.response);
        assert_eq!(msg.id, 9);
        assert_eq!(msg.questions.len(), 1);
        assert_eq!(msg.records.len(), 5);
        assert_eq!(msg.records[0].ttl, Some(10));
        assert_eq!(
            &msg.records[1..],
            &extra
                .iter()
                .cloned()
                .map(|mut r| {
                    r.ttl = Some(10);
                    r
                })
                .collect::<Vec<_>>()[..]
        );
        let rows = crate::fold_records(&msg.records);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "mac-1");
        assert_eq!(rows[0].port, Some(52415));
        assert_eq!(rows[0].addrs, vec!["192.168.1.20", "fe80::1"]);
    }

    #[test]
    fn compressed_names_and_unknown_types() {
        // Header: response, 0 questions, 2 answers.
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 2, 0, 0, 0, 0];
        // Answer 1: _x._tcp.local PTR a.<ptr to 12>
        let name_at = m.len();
        put_name(&mut m, "_x._tcp.local.");
        m.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0, 120]);
        let rdata = [1, b'A', 0xC0, name_at as u8];
        m.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        m.extend_from_slice(&rdata);
        // Answer 2: unknown type 99 with 3 bytes, name is a pointer.
        m.extend_from_slice(&[0xC0, name_at as u8, 0, 99, 0, 1, 0, 0, 0, 1, 0, 3, 1, 2, 3]);
        let msg = parse_message(&m).unwrap();
        assert_eq!(msg.records.len(), 1);
        assert_eq!(msg.records[0].rdata, RData::Ptr("a._x._tcp.local.".into()));
    }

    #[test]
    fn hostile_input_is_an_error_not_a_panic() {
        assert!(parse_message(&[]).is_err());
        assert!(parse_message(&[0; 11]).is_err());
        // Pointer loop.
        let mut m = vec![0, 0, 0x84, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        m.extend_from_slice(&[0xC0, 12, 0, 12, 0, 1]);
        assert!(parse_message(&m).is_err());
        // Count overflow.
        assert!(parse_message(&[0, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0]).is_err());
        // rdata past end.
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        put_name(&mut m, "a.local.");
        m.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 1, 0, 200, 1]);
        assert!(parse_message(&m).is_err());
        for len in 0..64 {
            let junk: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let _ = parse_message(&junk);
        }
    }

    #[test]
    fn odd_label_bytes_are_sanitized() {
        let mut m = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        m.extend_from_slice(&[
            6, b'M', b'y', b' ', b'M', b'a', b'c', 4, b'_', b'e', b'x', b'o',
        ]);
        m.extend_from_slice(&[
            4, b'_', b't', b'c', b'p', 5, b'l', b'o', b'c', b'a', b'l', 0,
        ]);
        m.extend_from_slice(&[0, 255, 0, 1]);
        let msg = parse_message(&m).unwrap();
        assert_eq!(msg.questions[0].name, "my-mac._exo._tcp.local.");
        assert_eq!(msg.questions[0].qtype, TYPE_ANY);
    }
}
