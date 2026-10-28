// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Threshold *crossings* (disk use, certificate expiry). Only a change of
//! level is an event; a disk sitting at 91% for a week is one event, not
//! one per check. Checked on the liveness heartbeat and when a watched cert
//! file changes (inotify), never on a fast timer.

use super::{NerveEvent, Severity};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Level = how many thresholds are crossed (0 = fine).
#[derive(Default)]
pub struct Crossings {
    level: HashMap<String, usize>,
}

impl Crossings {
    /// Disk: `pct` used; thresholds ascending (85, 90, 95).
    pub fn disk(&mut self, path: &str, pct: u8, th: &[u8], t: u64) -> Option<NerveEvent> {
        let lvl = th.iter().filter(|x| pct >= **x).count();
        let k = format!("disk:{path}");
        let prev = self.level.insert(k.clone(), lvl);
        let prev = prev?; // first observation sets the baseline (still reported if already crossed)
        if lvl == prev {
            return None;
        }
        let sev = sev_for(lvl, th.len(), lvl < prev);
        Some(
            NerveEvent::new(
                "threshold",
                "disk.crossed",
                sev,
                format!("{path} {pct}% used (level {prev} -> {lvl}, thresholds {th:?})"),
            )
            .group("infra")
            .key(format!("disk.crossed:{path}:{lvl}"))
            .data(json!({"path": path, "pct": pct, "level": lvl, "prev": prev, "thresholds": th}))
            .at(t),
        )
    }

    /// First observation of a disk already above a threshold is an event too.
    pub fn disk_first(&mut self, path: &str, pct: u8, th: &[u8], t: u64) -> Option<NerveEvent> {
        let k = format!("disk:{path}");
        if self.level.contains_key(&k) {
            return self.disk(path, pct, th, t);
        }
        self.level.insert(k.clone(), 0);
        self.disk(path, pct, th, t)
    }

    /// Cert: days left; thresholds descending (30, 14, 3, 0).
    pub fn cert(&mut self, path: &str, days_left: i64, th: &[i64], t: u64) -> Option<NerveEvent> {
        let lvl = th.iter().filter(|x| days_left <= **x).count();
        let k = format!("cert:{path}");
        let prev = self.level.insert(k, lvl).unwrap_or(0);
        if lvl == prev {
            return None;
        }
        let sev = sev_for(lvl, th.len(), lvl < prev);
        Some(
            NerveEvent::new("threshold", "cert.crossed", sev, format!("{path}: {days_left} day(s) left (level {prev} -> {lvl})"))
                .group("infra")
                .key(format!("cert.crossed:{path}:{lvl}"))
                .data(json!({"path": path, "days_left": days_left, "level": lvl, "prev": prev, "thresholds": th}))
                .at(t),
        )
    }
}

fn sev_for(lvl: usize, n: usize, recovering: bool) -> Severity {
    if recovering || lvl == 0 {
        Severity::Info
    } else if lvl >= n {
        Severity::Critical
    } else if lvl + 1 >= n {
        Severity::High
    } else {
        Severity::Medium
    }
}

/// `df -P -k PATH` -> used percent.
pub fn parse_df(text: &str) -> Option<u8> {
    let l = text.lines().nth(1)?;
    let f: Vec<&str> = l.split_whitespace().collect();
    f.get(4)?.trim_end_matches('%').parse().ok()
}

pub fn disk_pct(path: &str) -> Option<u8> {
    let o = crate::util::run(
        "df",
        &["-P", "-k", path],
        std::time::Duration::from_secs(10),
    )?;
    parse_df(&o.stdout)
}

// ---- minimal X.509 notAfter reader (PEM -> DER -> TBSCertificate.validity) ----

fn b64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// One DER TLV: (tag, content, rest).
fn tlv(d: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *d.first()?;
    let l0 = *d.get(1)? as usize;
    let (len, hdr) = if l0 < 0x80 {
        (l0, 2)
    } else {
        let n = l0 & 0x7f;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *d.get(2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let body = d.get(hdr..hdr + len)?;
    Some((tag, body, &d[hdr + len..]))
}

fn der_time(tag: u8, b: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(b).ok()?;
    let (y, rest) = match tag {
        0x17 => {
            let yy: i64 = s.get(0..2)?.parse().ok()?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, s.get(2..)?)
        }
        0x18 => (s.get(0..4)?.parse().ok()?, s.get(4..)?),
        _ => return None,
    };
    let n = |a: usize| -> Option<u32> { rest.get(a..a + 2)?.parse().ok() };
    let days = crate::util::days_from_civil(y, n(0)?, n(2)?);
    let secs =
        days * 86_400 + (n(4)? as i64) * 3600 + (n(6)? as i64) * 60 + n(8).unwrap_or(0) as i64;
    u64::try_from(secs).ok()
}

/// notAfter (unix secs) of the first certificate in a PEM or DER file body.
pub fn cert_not_after(body: &[u8]) -> Option<u64> {
    let der = match std::str::from_utf8(body) {
        Ok(t) if t.contains("-----BEGIN CERTIFICATE-----") => {
            let a = t.find("-----BEGIN CERTIFICATE-----")? + 27;
            let e = t[a..].find("-----END CERTIFICATE-----")? + a;
            b64(&t[a..e])?
        }
        _ => body.to_vec(),
    };
    let (_, cert, _) = tlv(&der)?;
    let (_, tbs, _) = tlv(cert)?;
    let mut r = tbs;
    let (t0, _, rest) = tlv(r)?;
    if t0 == 0xa0 {
        r = rest; // explicit version
    }
    let (_, _, r) = tlv(r)?; // serial
    let (_, _, r) = tlv(r)?; // signature alg
    let (_, _, r) = tlv(r)?; // issuer
    let (_, validity, _) = tlv(r)?;
    let (_, _, v) = tlv(validity)?; // notBefore
    let (tag, na, _) = tlv(v)?;
    der_time(tag, na)
}

pub fn cert_days_left(path: &Path, now: u64) -> Option<i64> {
    let b = std::fs::read(path).ok()?;
    let na = cert_not_after(&b)?;
    Some((na as i64 - now as i64).div_euclid(86_400))
}

/// Run every disk and cert check once; returns crossing events.
pub fn check_all(
    c: &mut Crossings,
    disks: &[String],
    pct: &[u8],
    certs: &[PathBuf],
    days: &[i64],
    now: u64,
) -> Vec<NerveEvent> {
    let mut v = Vec::new();
    for d in disks {
        if let Some(p) = disk_pct(d) {
            v.extend(c.disk_first(d, p, pct, now));
        }
    }
    for p in certs {
        if let Some(dl) = cert_days_left(p, now) {
            v.extend(c.cert(&p.display().to_string(), dl, days, now));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_edges_only() {
        let mut c = Crossings::default();
        let th = [85u8, 90, 95];
        assert!(c.disk_first("/", 26, &th, 0).is_none());
        assert!(c.disk("/", 60, &th, 1).is_none());
        let e = c.disk("/", 91, &th, 2).unwrap();
        assert_eq!(
            (e.kind.as_str(), e.severity),
            ("disk.crossed", Severity::High)
        );
        assert!(c.disk("/", 92, &th, 3).is_none(), "same level, no event");
        assert_eq!(
            c.disk("/", 97, &th, 4).unwrap().severity,
            Severity::Critical
        );
        assert_eq!(c.disk("/", 50, &th, 5).unwrap().severity, Severity::Info);
        let mut c2 = Crossings::default();
        assert_eq!(
            c2.disk_first("/", 86, &th, 0).unwrap().severity,
            Severity::Medium
        );
        assert_eq!(parse_df("Filesystem 1024-blocks Used Available Capacity Mounted on\nrpool/x 104857600 27262976 77594624 26% /\n"), Some(26));
    }

    #[test]
    fn cert_levels() {
        let mut c = Crossings::default();
        let th = [30i64, 14, 3, 0];
        assert!(c.cert("a.pem", 80, &th, 0).is_none());
        assert_eq!(
            c.cert("a.pem", 13, &th, 1).unwrap().severity,
            Severity::Medium
        );
        assert_eq!(c.cert("a.pem", 2, &th, 2).unwrap().severity, Severity::High);
        assert_eq!(
            c.cert("a.pem", -1, &th, 3).unwrap().severity,
            Severity::Critical
        );
    }

    #[test]
    fn der_validity() {
        // Hand-built minimal TBS: SEQ{ SEQ{ [0]{INT 2}, INT 1, SEQ{}, SEQ{}, SEQ{ UTCTime, GeneralizedTime } } }
        let nb = b"260101000000Z";
        let na = b"20270315120000Z";
        let mut val = vec![0x17, nb.len() as u8];
        val.extend_from_slice(nb);
        val.extend([0x18, na.len() as u8]);
        val.extend_from_slice(na);
        let mut tbs = vec![
            0xa0,
            3,
            0x02,
            1,
            2,
            0x02,
            1,
            1,
            0x30,
            0,
            0x30,
            0,
            0x30,
            val.len() as u8,
        ];
        tbs.extend(val);
        let mut cert = vec![0x30, tbs.len() as u8];
        cert.extend(tbs);
        let mut der = vec![0x30, cert.len() as u8];
        der.extend(cert);
        let t = cert_not_after(&der).unwrap();
        assert_eq!(crate::util::ts(t).get(0..10), Some("2027-03-15"));
        // PEM wrapping of the same bytes
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for ch in der.chunks(3) {
            let n = ch.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - ch.len()));
            for i in 0..4 {
                s.push(if i <= ch.len() {
                    T[((n >> (18 - 6 * i)) & 63) as usize] as char
                } else {
                    '='
                });
            }
        }
        let pem = format!("-----BEGIN CERTIFICATE-----\n{s}\n-----END CERTIFICATE-----\n");
        assert_eq!(cert_not_after(pem.as_bytes()), Some(t));
        assert!(cert_not_after(b"garbage").is_none());
    }
}
