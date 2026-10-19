// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Minimal, defensive DNS packet parser for passive mDNS listening, plus the
//! one-packet query builder for the opt-in active query.

use std::net::{Ipv4Addr, Ipv6Addr};

pub const T_A: u16 = 1;
pub const T_PTR: u16 = 12;
pub const T_TXT: u16 = 16;
pub const T_AAAA: u16 = 28;
pub const T_SRV: u16 = 33;

#[derive(Debug, Clone, PartialEq)]
pub enum RData {
    A(Ipv4Addr),
    Aaaa(Ipv6Addr),
    Ptr(String),
    Srv { port: u16, target: String },
    Txt(Vec<String>),
    Other(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub name: String,
    pub data: RData,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

/// Read a (possibly compressed) name. Returns (name, offset after the name).
fn read_name(b: &[u8], mut i: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *b.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len & 0xC0 == 0xC0 {
            let ptr = (u16_at(b, i)? & 0x3FFF) as usize;
            end.get_or_insert(i + 2);
            jumps += 1;
            if jumps > 32 || ptr >= b.len() {
                return None;
            }
            i = ptr;
            continue;
        }
        if len & 0xC0 != 0 {
            return None;
        }
        let label = b.get(i + 1..i + 1 + len)?;
        out.push_str(&String::from_utf8_lossy(label));
        out.push('.');
        if out.len() > 255 {
            return None;
        }
        i += 1 + len;
    }
    Some((out, end.unwrap_or(i)))
}

/// Parse all resource records (answers, authority, additional) of a packet.
pub fn parse(b: &[u8]) -> Option<Vec<Record>> {
    let qd = u16_at(b, 4)? as usize;
    let rr = u16_at(b, 6)? as usize + u16_at(b, 8)? as usize + u16_at(b, 10)? as usize;
    if rr > 512 {
        return None;
    }
    let mut i = 12;
    for _ in 0..qd {
        let (_, j) = read_name(b, i)?;
        i = j + 4;
    }
    let mut out = Vec::new();
    for _ in 0..rr {
        let (name, j) = read_name(b, i)?;
        let ty = u16_at(b, j)?;
        let rdlen = u16_at(b, j + 8)? as usize;
        let rd = j + 10;
        let rdata = b.get(rd..rd + rdlen)?;
        let data = match ty {
            T_A if rdlen == 4 => RData::A(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3])),
            T_AAAA if rdlen == 16 => {
                let a: [u8; 16] = rdata.try_into().ok()?;
                RData::Aaaa(Ipv6Addr::from(a))
            }
            T_PTR => RData::Ptr(read_name(b, rd)?.0),
            T_SRV if rdlen >= 7 => RData::Srv {
                port: u16_at(b, rd + 4)?,
                target: read_name(b, rd + 6)?.0,
            },
            T_TXT => {
                let mut v = Vec::new();
                let mut k = 0;
                while k < rdata.len() {
                    let l = rdata[k] as usize;
                    if let Some(s) = rdata.get(k + 1..k + 1 + l) {
                        if !s.is_empty() {
                            v.push(String::from_utf8_lossy(s).into_owned());
                        }
                    }
                    k += 1 + l;
                }
                RData::Txt(v)
            }
            t => RData::Other(t),
        };
        out.push(Record { name, data });
        i = rd + rdlen;
    }
    Some(out)
}

fn push_name(p: &mut Vec<u8>, name: &str) {
    for l in name.trim_end_matches('.').split('.') {
        p.push(l.len() as u8);
        p.extend_from_slice(l.as_bytes());
    }
    p.push(0);
}

/// One mDNS query packet asking PTR for each name (QM, multicast replies).
pub fn build_query(names: &[&str]) -> Vec<u8> {
    let mut p = vec![0, 0, 0, 0, 0, names.len() as u8, 0, 0, 0, 0, 0, 0];
    for n in names {
        push_name(&mut p, n);
        p.extend_from_slice(&T_PTR.to_be_bytes());
        p.extend_from_slice(&1u16.to_be_bytes());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny mDNS response: PTR + SRV + TXT + A with compression.
    fn sample() -> Vec<u8> {
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 4, 0, 0, 0, 0];
        // PTR _openmind._tcp.local -> omn-1._openmind._tcp.local
        let svc_off = p.len();
        push_name(&mut p, "_openmind._tcp.local");
        p.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0, 120]);
        let inst = [&[5u8][..], b"omn-1", &[0xC0, svc_off as u8]].concat();
        p.extend_from_slice(&(inst.len() as u16).to_be_bytes());
        let inst_off = p.len();
        p.extend_from_slice(&inst);
        // SRV
        p.extend_from_slice(&[0xC0, inst_off as u8, 0, 33, 0x80, 1, 0, 0, 0, 120]);
        let mut srv = vec![0, 0, 0, 0, 0x1E, 0x6B];
        push_name(&mut srv, "phone.local");
        p.extend_from_slice(&(srv.len() as u16).to_be_bytes());
        p.extend_from_slice(&srv);
        // TXT
        p.extend_from_slice(&[0xC0, inst_off as u8, 0, 16, 0x80, 1, 0, 0, 0, 120]);
        let txt = [&[7u8][..], b"arch=x1", &[4], b"v=01"].concat();
        p.extend_from_slice(&(txt.len() as u16).to_be_bytes());
        p.extend_from_slice(&txt);
        // A phone.local
        push_name(&mut p, "phone.local");
        p.extend_from_slice(&[0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, 192, 168, 1, 9]);
        p
    }

    #[test]
    fn parses_response() {
        let r = parse(&sample()).unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].data, RData::Ptr("omn-1._openmind._tcp.local.".into()));
        assert_eq!(
            r[1].data,
            RData::Srv {
                port: 7787,
                target: "phone.local.".into()
            }
        );
        assert_eq!(r[2].data, RData::Txt(vec!["arch=x1".into(), "v=01".into()]));
        assert_eq!(r[3].data, RData::A(Ipv4Addr::new(192, 168, 1, 9)));
    }

    #[test]
    fn rejects_garbage_and_loops() {
        assert!(parse(&[1, 2, 3]).is_none());
        // Answer whose name is a pointer to itself.
        let p = vec![
            0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0,
        ];
        assert!(parse(&p).is_none());
        // Truncated packets never panic.
        let s = sample();
        for n in 0..s.len() {
            let _ = parse(&s[..n]);
        }
    }

    #[test]
    fn query_roundtrip_header() {
        let q = build_query(&["_openmind._tcp.local."]);
        assert_eq!(u16_at(&q, 4), Some(1));
        assert_eq!(parse(&q), Some(vec![]));
    }
}
