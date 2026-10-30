// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Phone alert for a failed heal (Mate 2026-10-30 11:27 CET, decision 2:
//! "when a heal fails, ONLY alert Mate, critical, through the existing
//! nerve -> ntfy relay path").
//!
//! Why a second hop: the nerve folds every pushed cortex event into an
//! incident of at most `error` severity, which its webhook (min `crit`)
//! never forwards. So the cortex hands `R12-heal-failed` alerts to the same
//! loopback relay the nerve webhook uses (`galion-ntfy-relay`, which holds
//! the topic; the cortex never sees it), with ntfy priority `urgent`.
//!
//! * `CORTEX_ALERT_RELAY=http://127.0.0.1:<port>/<path>`: loopback only;
//!   unset = off (the nerve copy is still sent, as before).
//! * `CORTEX_ALERT_RELAY_PER_HOUR` (default 6, 1..=30): sliding-hour cap.
//! * Only `cortex.alert` events whose rule is `R12-heal-failed` go out.
//!   Body: the alert text (max 1000 chars); title names the unit.
//! * Never escalates to Hermes; a failed send is counted, not retried.

use galion_common::http::{send, ClientRequest};
use nerve_cortex_contract::Event;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

/// The only rule that reaches the phone.
pub const PHONE_RULE: &str = "R12-heal-failed";
pub const PER_HOUR_DEFAULT: u32 = 6;
pub const PER_HOUR_RANGE: (u32, u32) = (1, 30);
const BODY_MAX: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneCfg {
    pub addr: SocketAddr,
    pub path: String,
    pub per_hour: u32,
}

impl PhoneCfg {
    /// `None` = off. Errors refuse to start (exit 2), like every other knob.
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Option<PhoneCfg>, String> {
        let Some(url) = get("CORTEX_ALERT_RELAY").filter(|u| !u.trim().is_empty()) else {
            return Ok(None);
        };
        let (addr, path) = parse_loopback(&url).ok_or_else(|| {
            "CORTEX_ALERT_RELAY must be http://<loopback>:<port>/<path>".to_string()
        })?;
        let per_hour = match get("CORTEX_ALERT_RELAY_PER_HOUR") {
            None => PER_HOUR_DEFAULT,
            Some(v) => v
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (PER_HOUR_RANGE.0..=PER_HOUR_RANGE.1).contains(n))
                .ok_or_else(|| {
                    format!(
                        "CORTEX_ALERT_RELAY_PER_HOUR must be {}..={}, got {v:?}",
                        PER_HOUR_RANGE.0, PER_HOUR_RANGE.1
                    )
                })?,
        };
        Ok(Some(PhoneCfg {
            addr,
            path,
            per_hour,
        }))
    }
}

/// `http://<loopback ip>:<port>/<path>` only (no names, no query, no userinfo).
pub fn parse_loopback(url: &str) -> Option<(SocketAddr, String)> {
    let rest = url.trim().strip_prefix("http://")?;
    let (hp, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr: SocketAddr = hp.parse().ok()?;
    if !addr.ip().is_loopback() || addr.port() == 0 {
        return None;
    }
    let ok = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c));
    (ok && path.len() <= 128).then(|| (addr, path.to_string()))
}

/// Is this nerve post a failed-heal alert for the phone?
pub fn wants(ev: &Event) -> bool {
    ev.kind == "cortex.alert" && ev.data.get("rule").and_then(Value::as_str) == Some(PHONE_RULE)
}

fn clean(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect()
}

/// Title / body of the ntfy message for one alert.
pub fn message(ev: &Event) -> (String, String) {
    let d = |k: &str| ev.data.get(k).and_then(Value::as_str).unwrap_or("");
    let unit = ev
        .unit
        .clone()
        .unwrap_or_else(|| d("subject").trim_start_matches("unit:").to_string());
    (
        clean(&format!("Gaia: heal failed: {unit}"), 120),
        clean(
            &format!("{} [{} {}]", d("msg"), d("rule"), d("decision")),
            BODY_MAX,
        ),
    )
}

#[derive(Debug, Default)]
struct Book {
    sent_at: VecDeque<u64>,
    sent: u64,
    failed: u64,
    rate_limited: u64,
    last: Option<String>,
}

/// Sends failed-heal alerts to the relay (one short loopback POST, 5 s).
#[derive(Debug)]
pub struct Phone {
    pub cfg: PhoneCfg,
    book: Mutex<Book>,
}

/// What [`Phone::offer`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    NotForPhone,
    RateLimited,
    Ok,
    Failed(String),
}

impl Phone {
    pub fn new(cfg: PhoneCfg) -> Self {
        Phone {
            cfg,
            book: Mutex::new(Book::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Offer one nerve post; only failed-heal alerts are sent.
    pub fn offer(&self, ev: &Event, now: u64) -> Sent {
        if !wants(ev) {
            return Sent::NotForPhone;
        }
        {
            let mut b = self.lock();
            while b
                .sent_at
                .front()
                .is_some_and(|t| now.saturating_sub(*t) >= 3600)
            {
                b.sent_at.pop_front();
            }
            if b.sent_at.len() as u32 >= self.cfg.per_hour {
                b.rate_limited += 1;
                return Sent::RateLimited;
            }
            b.sent_at.push_back(now);
        }
        let (title, body) = message(ev);
        let host = self.cfg.addr.to_string();
        let mut req = ClientRequest::post(
            &self.cfg.path,
            &host,
            "text/plain; charset=utf-8",
            body.as_bytes(),
        );
        req.headers.push(("Title".into(), title));
        req.headers.push(("Priority".into(), "urgent".into()));
        req.headers
            .push(("Tags".into(), "rotating_light,gaia".into()));
        let r = match send(self.cfg.addr, &req, Duration::from_secs(5), 64 * 1024) {
            Ok(r) if (200..300).contains(&r.status) => Sent::Ok,
            Ok(r) => Sent::Failed(format!("relay HTTP {}", r.status)),
            Err(e) => Sent::Failed(format!("relay: {e:?}").chars().take(120).collect()),
        };
        let mut b = self.lock();
        match &r {
            Sent::Ok => b.sent += 1,
            Sent::Failed(_) => b.failed += 1,
            _ => {}
        }
        b.last = Some(format!("{} {:?}", crate_ts(now), r));
        r
    }

    pub fn to_json(&self) -> Value {
        let b = self.lock();
        json!({"configured": true, "path": self.cfg.path, "priority": "urgent", "rule": PHONE_RULE,
               "per_hour": self.cfg.per_hour, "in_window": b.sent_at.len(), "sent": b.sent,
               "failed": b.failed, "rate_limited": b.rate_limited, "last": b.last})
    }
}

fn crate_ts(now: u64) -> String {
    zagreb_time::from_unix(now as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn env(p: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let v: Vec<(String, String)> = p
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        move |k| v.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone())
    }

    fn alert(rule: &str) -> Event {
        let mut e = Event::new(
            "gaia-cortex",
            "cortex.alert",
            "critical",
            json!({"rule": rule, "decision": "dec-7", "subject": "unit:tuner.service",
                   "msg": "tuner.service: restart dec-7 did not heal it within 30s (HTTP 503)\ncircuit opened"}),
        );
        e.unit = Some("tuner.service".into());
        e
    }

    #[test]
    fn config_is_loopback_only_and_bounded() {
        assert_eq!(PhoneCfg::from_env(&env(&[])).unwrap(), None);
        assert_eq!(
            PhoneCfg::from_env(&env(&[("CORTEX_ALERT_RELAY", " ")])).unwrap(),
            None
        );
        let c = PhoneCfg::from_env(&env(&[(
            "CORTEX_ALERT_RELAY",
            "http://127.0.0.1:2586/gaia-cortex",
        )]))
        .unwrap()
        .unwrap();
        assert_eq!(
            (c.addr.port(), c.path.as_str(), c.per_hour),
            (2586, "/gaia-cortex", 6)
        );
        for bad in [
            "https://127.0.0.1:2586/x",
            "http://10.0.0.5:2586/x",
            "http://example.org:80/x",
            "http://localhost:2586/x",
            "http://127.0.0.1:0/x",
            "http://127.0.0.1:2586/x?topic=y",
            "http://user@127.0.0.1:2586/x",
        ] {
            assert!(
                PhoneCfg::from_env(&env(&[("CORTEX_ALERT_RELAY", bad)])).is_err(),
                "{bad}"
            );
        }
        for bad in ["0", "31", "x"] {
            assert!(PhoneCfg::from_env(&env(&[
                ("CORTEX_ALERT_RELAY", "http://127.0.0.1:2586/a"),
                ("CORTEX_ALERT_RELAY_PER_HOUR", bad)
            ]))
            .is_err());
        }
    }

    #[test]
    fn only_failed_heals_reach_the_phone() {
        assert!(wants(&alert("R12-heal-failed")));
        for r in [
            "R12-heal-unverified",
            "R13-frozen",
            "R13-freeze-wide",
            "R2-circuit-open",
            "R99-default",
        ] {
            assert!(!wants(&alert(r)), "{r}");
        }
        let mut esc = alert("R12-heal-failed");
        esc.kind = "cortex.escalation".into();
        assert!(!wants(&esc));
        let (t, b) = message(&alert("R12-heal-failed"));
        assert_eq!(t, "Gaia: heal failed: tuner.service");
        assert!(
            !b.contains('\n') && b.contains("did not heal") && b.contains("dec-7"),
            "{b}"
        );
    }

    /// One loopback "relay" that records requests and answers 200.
    fn relay(n: usize) -> (SocketAddr, std::thread::JoinHandle<Vec<String>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let mut got = vec![];
            for _ in 0..n {
                let (mut s, _) = l.accept().unwrap();
                s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut buf = vec![0u8; 8192];
                let mut all = vec![];
                while let Ok(k) = s.read(&mut buf) {
                    if k == 0 {
                        break;
                    }
                    all.extend_from_slice(&buf[..k]);
                    let t = String::from_utf8_lossy(&all).to_string();
                    if let Some(i) = t.find("\r\n\r\n") {
                        let len = t
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if all.len() >= i + 4 + len {
                            break;
                        }
                    }
                }
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
                got.push(String::from_utf8_lossy(&all).to_string());
            }
            got
        });
        (a, h)
    }

    #[test]
    fn sends_urgent_and_caps_per_hour() {
        let (addr, h) = relay(2);
        let p = Phone::new(PhoneCfg {
            addr,
            path: "/gaia-cortex".into(),
            per_hour: 2,
        });
        assert_eq!(p.offer(&alert("R13-frozen"), 100), Sent::NotForPhone);
        assert_eq!(p.offer(&alert("R12-heal-failed"), 100), Sent::Ok);
        assert_eq!(p.offer(&alert("R12-heal-failed"), 200), Sent::Ok);
        assert_eq!(p.offer(&alert("R12-heal-failed"), 300), Sent::RateLimited);
        let got = h.join().unwrap();
        assert_eq!(got.len(), 2);
        assert!(
            got[0].starts_with("POST /gaia-cortex HTTP/1."),
            "{}",
            got[0]
        );
        assert!(got[0].contains("Priority: urgent"), "{}", got[0]);
        assert!(
            got[0].contains("Title: Gaia: heal failed: tuner.service"),
            "{}",
            got[0]
        );
        let j = p.to_json();
        assert_eq!(
            (j["sent"].as_u64(), j["rate_limited"].as_u64()),
            (Some(2), Some(1))
        );
        // the window slides: an hour later one more goes out (to a dead port: counted as failed)
        let dead = Phone::new(PhoneCfg {
            addr: "127.0.0.1:9".parse().unwrap(),
            path: "/x".into(),
            per_hour: 1,
        });
        assert!(matches!(
            dead.offer(&alert("R12-heal-failed"), 10),
            Sent::Failed(_)
        ));
        assert_eq!(dead.offer(&alert("R12-heal-failed"), 20), Sent::RateLimited);
        assert!(matches!(
            dead.offer(&alert("R12-heal-failed"), 3700),
            Sent::Failed(_)
        ));
        assert_eq!(dead.to_json()["failed"].as_u64(), Some(2));
    }
}
