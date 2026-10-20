// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.7 per-port connection rate (B41 section 3.2 / 4.3, docs/cortex.md).
//!
//! Reads the kernel socket tables `/proc/net/tcp` and `/proc/net/tcp6`
//! (no bind, no packet capture, no `ss` subprocess) once per cycle, only when
//! [`Input::live`] is true, and counts **new connections** to each watched
//! local port since the previous cycle.
//!
//! B41 defines the rate as the growth of ESTAB + TIME-WAIT. That growth is
//! zero under any *steady* load (TIME-WAIT holds a closed socket for 60 s, so
//! a constant 50/s keeps ~3000 TIME-WAIT entries flat). This sensor therefore
//! counts connection *identities* (the 4-tuple, both loopback ends folded into
//! one) that were not present in the previous cycle. A closed connection stays
//! in the table for 60 s in TIME-WAIT on the closing side, so with a cycle of
//! 60 s or less (default 30 s) every connection is seen at least once. The
//! B41 ESTAB+TIME-WAIT growth is still computed and shown in the evidence.
//!
//! Thresholds are B41 section 4.3 verbatim (`rate_per_s >= 1 / 10 / 50` warn /
//! high / crit, kind `bleeding`). Env may only tighten them.
//!
//! Env:
//! - `NERVE_CONNRATE_PORTS=8420=memory-gateway,8002` turns the sensor on and
//!   names the watched ports (label defaults to the component that owns the
//!   port, else `port-<n>`). Unset or empty: sensor off.
//! - `NERVE_CONNRATE_RATES=warn,high,crit` (per second; each clamped to the
//!   B41 value: B41 says tighten, never loosen).
//!
//! Output: one finding `conn-rate-<port>` per port over warn, kind bleeding,
//! B33 source = the port label (B41 uses `memory-gateway`). Titles and events
//! carry the port, the rate and counts only; no addresses.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};

/// B41 section 4.3 memory-gateway thresholds, new connections per second.
pub const B41_RATES: [f64; 3] = [1.0, 10.0, 50.0];

/// Where the socket tables come from: the live `/proc` or a test/fixture.
pub type TableReader = Box<dyn FnMut() -> Option<String> + Send>;

/// Socket counts for one port in one table read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortCount {
    pub estab: u32,
    pub time_wait: u32,
    /// Hashed connection identities (no address is kept).
    pub conns: HashSet<u64>,
}

/// Parse `/proc/net/tcp` / `tcp6` text (both may be concatenated) into
/// per-port counts for the `watch` ports. LISTEN rows are skipped; every
/// other state counts as a connection identity. The two loopback ends of one
/// connection fold into one identity (the watched endpoint goes first).
pub fn parse_tables(text: &str, watch: &[u16]) -> BTreeMap<u16, PortCount> {
    let mut out: BTreeMap<u16, PortCount> =
        watch.iter().map(|p| (*p, PortCount::default())).collect();
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let (Some(_sl), Some(local), Some(remote), Some(st)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let (Some(lp), Some(rp)) = (port_of(local), port_of(remote)) else {
            continue; // header line or garbage
        };
        let state = u8::from_str_radix(st, 16).unwrap_or(0);
        if state == 0x0A || state == 0 {
            continue; // LISTEN (or unparsable)
        }
        // Server side: local is the watched port. Client side (loopback):
        // remote is the watched port. Fold both to (watched, other).
        let (port, key) = if out.contains_key(&lp) {
            (lp, (local, remote))
        } else if out.contains_key(&rp) {
            (rp, (remote, local))
        } else {
            continue;
        };
        let pc = out.get_mut(&port).expect("watched");
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        let first = pc.conns.insert(h.finish());
        // ESTAB / TIME-WAIT per B41, counted once per connection identity
        // (the server-side row, or the client row when only it is left).
        if first {
            match state {
                0x01 => pc.estab += 1,
                0x06 => pc.time_wait += 1,
                _ => {}
            }
        }
    }
    out
}

fn port_of(endpoint: &str) -> Option<u16> {
    let (addr, port) = endpoint.rsplit_once(':')?;
    if addr.is_empty() || !addr.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(port, 16).ok()
}

/// Live reader: `/proc/net/tcp` + `/proc/net/tcp6` (either may be absent).
pub fn proc_reader() -> TableReader {
    Box::new(|| {
        let a = std::fs::read_to_string("/proc/net/tcp").ok();
        let b = std::fs::read_to_string("/proc/net/tcp6").ok();
        if a.is_none() && b.is_none() {
            return None;
        }
        Some(format!(
            "{}\n{}",
            a.unwrap_or_default(),
            b.unwrap_or_default()
        ))
    })
}

struct Prev {
    at: u64,
    conns: HashSet<u64>,
    estab_tw: u32,
}

/// Sensor `conn-rate`.
pub struct ConnRate {
    /// Watched ports and their explicit labels (None: derive from comps).
    ports: Vec<(u16, Option<String>)>,
    rates: [f64; 3],
    reader: TableReader,
    prev: BTreeMap<u16, Prev>,
    /// Cycles where the table could not be read (shown in a finding).
    read_failures: u32,
}

impl ConnRate {
    /// Sensor over `ports` with B41 thresholds, reading `reader`.
    pub fn new(ports: Vec<(u16, Option<String>)>, reader: TableReader) -> Self {
        ConnRate {
            ports,
            rates: B41_RATES,
            reader,
            prev: BTreeMap::new(),
            read_failures: 0,
        }
    }

    /// Tighten thresholds (each value is clamped to the B41 value).
    pub fn with_rates(mut self, r: [f64; 3]) -> Self {
        for (i, v) in r.iter().enumerate() {
            if v.is_finite() && *v > 0.0 {
                self.rates[i] = v.min(B41_RATES[i]);
            }
        }
        self
    }

    /// `NERVE_CONNRATE_PORTS` (on/off + ports) and `NERVE_CONNRATE_RATES`.
    pub fn from_env(env: Env) -> Option<Self> {
        let ports = parse_ports(&env("NERVE_CONNRATE_PORTS")?);
        if ports.is_empty() {
            return None;
        }
        let mut s = ConnRate::new(ports, proc_reader());
        if let Some(r) = env("NERVE_CONNRATE_RATES") {
            let v: Vec<f64> = r.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() == 3 {
                s = s.with_rates([v[0], v[1], v[2]]);
            }
        }
        Some(s)
    }

    /// Current thresholds (warn, high, crit).
    pub fn rates(&self) -> [f64; 3] {
        self.rates
    }
}

/// `8420=memory-gateway, 8002` -> [(8420, Some(..)), (8002, None)]; bad
/// entries and port 0 are skipped, duplicates keep the first.
pub fn parse_ports(s: &str) -> Vec<(u16, Option<String>)> {
    let mut out: Vec<(u16, Option<String>)> = vec![];
    for item in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let (p, label) = match item.split_once('=') {
            Some((p, l)) => (
                p.trim(),
                Some(l.trim().to_string()).filter(|l| !l.is_empty()),
            ),
            None => (item, None),
        };
        if let Ok(p) = p.parse::<u16>() {
            if p != 0 && !out.iter().any(|(q, _)| *q == p) {
                out.push((p, label));
            }
        }
    }
    out
}

impl Sensor for ConnRate {
    fn name(&self) -> &'static str {
        "conn-rate"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return; // replay/tests: never read the host
        }
        let watch: Vec<u16> = self.ports.iter().map(|(p, _)| *p).collect();
        let Some(text) = (self.reader)() else {
            self.read_failures += 1;
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "conn-rate-unreadable",
                    Sev::Warn,
                    "host",
                    format!(
                        "socket table unreadable ({} cycle(s)); connection rate unknown",
                        self.read_failures
                    ),
                )
                .with_fix("check that /proc/net/tcp is readable by the nerve"),
            );
            return;
        };
        self.read_failures = 0;
        let now = parse_tables(&text, &watch);
        for (port, label) in &self.ports {
            let Some(cur) = now.get(port) else { continue };
            let estab_tw = cur.estab + cur.time_wait;
            let prev = self.prev.insert(
                *port,
                Prev {
                    at: i.now,
                    conns: cur.conns.clone(),
                    estab_tw,
                },
            );
            let Some(prev) = prev else { continue }; // first cycle: baseline
            let dt = i.now.saturating_sub(prev.at);
            if dt == 0 {
                continue;
            }
            let new = cur.conns.difference(&prev.conns).count();
            let rate = new as f64 / dt as f64;
            let growth = estab_tw.saturating_sub(prev.estab_tw) as f64 / dt as f64;
            let sev = if rate >= self.rates[2] {
                Sev::Critical
            } else if rate >= self.rates[1] {
                Sev::Error
            } else if rate >= self.rates[0] {
                Sev::Warn
            } else {
                continue; // under every threshold: write nothing (B41 4)
            };
            let comp = i
                .comps
                .iter()
                .find(|c| c.spec.port == Some(*port))
                .map(|c| c.spec.name.to_string());
            let label = label
                .clone()
                .or_else(|| comp.clone())
                .unwrap_or_else(|| format!("port-{port}"));
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    format!("conn-rate-{port}"),
                    sev,
                    comp.unwrap_or_else(|| "host".into()),
                    format!("port {port}: {rate:.1} new connections/s over {dt} s ({new} new)"),
                )
                .with_evidence(vec![
                    format!("new_connections={new} window_s={dt} rate_per_s={rate:.2}"),
                    format!(
                        "estab={} time_wait={} (b41 estab+time_wait growth {growth:.2}/s)",
                        cur.estab, cur.time_wait
                    ),
                    format!(
                        "thresholds warn/high/crit {}/{}/{} per s (B41 4.3)",
                        self.rates[0], self.rates[1], self.rates[2]
                    ),
                ])
                .with_fix("loopback port: a local client is looping (probe/retry storm); find it with `ss -tnp` (read-only) and fix its interval; the nerve never kills or restarts it")
                .with_cause(format!("conn-rate:{port}"))
                .with_source(label),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;
    use std::sync::{Arc, Mutex};

    const HDR: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";

    /// One /proc/net/tcp row (port numbers decimal, converted to hex).
    fn row(lport: u16, rport: u16, st: u8) -> String {
        format!(
            "   0: 0100007F:{lport:04X} 0100007F:{rport:04X} {st:02X} 00000000:00000000 00:00000000 00000000     0        0 0 1 0000000000000000\n"
        )
    }

    /// `n` connections to `port` from ephemeral ports `base..`, both loopback
    /// ends present (server ESTAB, client ESTAB).
    fn conns(port: u16, base: u16, n: u16, st: u8) -> String {
        (0..n)
            .map(|k| row(port, base + k, st) + &row(base + k, port, st))
            .collect()
    }

    fn feed(tables: Vec<Option<String>>) -> TableReader {
        let q = Arc::new(Mutex::new(tables.into_iter()));
        Box::new(move || q.lock().unwrap().next().flatten())
    }

    fn run(s: &mut Sensors, now: u64) -> crate::cortex::Cortex {
        let logs = LogInsights::default();
        let mut i = Input::new(now, &[], &logs);
        i.live = true;
        s.run(&i)
    }

    #[test]
    fn parse_counts_folds_loopback_ends_and_skips_listen() {
        let t = format!(
            "{HDR}{}{}{}{}",
            row(8420, 0, 0x0A),
            conns(8420, 40000, 3, 0x01),
            conns(8420, 41000, 2, 0x06),
            conns(9999, 42000, 4, 0x01)
        );
        let m = parse_tables(&t, &[8420, 1234]);
        let p = &m[&8420];
        assert_eq!((p.estab, p.time_wait, p.conns.len()), (3, 2, 5));
        assert!(m[&1234].conns.is_empty());
        assert!(!m.contains_key(&9999));
        // tcp6 row form (32 hex chars) and only the client end left.
        let t6 = "   1: 0000000000000000FFFF00000100007F:A000 0000000000000000FFFF00000100007F:20E4 06 0:0 0:0 0 0 0 0\n";
        let m = parse_tables(t6, &[8420]);
        assert_eq!((m[&8420].time_wait, m[&8420].conns.len()), (1, 1));
    }

    #[test]
    fn steady_load_is_seen_where_b41_growth_is_flat() {
        // Cycle 1: 300 TIME-WAIT conns (baseline). Cycle 2, 30 s later: the
        // old ones aged out, 300 *new* ones in TIME-WAIT. B41 growth = 0;
        // new identities = 300 / 30 s = 10/s -> high.
        let mut s = Sensors::empty();
        s.add(ConnRate::new(
            vec![(8420, Some("memory-gateway".into()))],
            feed(vec![
                Some(conns(8420, 10000, 300, 0x06)),
                Some(conns(8420, 20000, 300, 0x06)),
                Some(conns(8420, 20000, 300, 0x06)),
            ]),
        ));
        assert!(
            run(&mut s, 1000).bleeding.is_empty(),
            "first cycle is the baseline"
        );
        let cx = run(&mut s, 1030);
        assert_eq!(cx.bleeding.len(), 1);
        let f = &cx.bleeding[0];
        assert_eq!(
            (f.id.as_str(), f.sev, f.source.as_str()),
            ("conn-rate-8420", Sev::Error, "memory-gateway")
        );
        assert_eq!(
            f.title,
            "port 8420: 10.0 new connections/s over 30 s (300 new)"
        );
        assert!(f.evidence[1].contains("growth 0.00/s"), "{:?}", f.evidence);
        // Same table again: nothing new, finding clears.
        assert!(run(&mut s, 1060).bleeding.is_empty());
    }

    #[test]
    fn thresholds_b41_and_tighten_only() {
        let mk = |n: u16| {
            let mut s = Sensors::empty();
            s.add(ConnRate::new(
                vec![(8420, None)],
                feed(vec![Some(String::new()), Some(conns(8420, 30000, n, 0x01))]),
            ));
            run(&mut s, 0);
            let cx = run(&mut s, 10);
            cx.bleeding
                .first()
                .map(|f| (f.sev, f.component.clone(), f.source.clone()))
        };
        assert_eq!(mk(9), None, "0.9/s is under warn");
        assert_eq!(mk(10), Some((Sev::Warn, "host".into(), "port-8420".into())));
        assert_eq!(mk(100).unwrap().0, Sev::Error);
        assert_eq!(mk(500).unwrap().0, Sev::Critical);
        let r = ConnRate::new(vec![], feed(vec![]))
            .with_rates([0.5, 99.0, -1.0])
            .rates();
        assert_eq!(r, [0.5, 10.0, 50.0], "env can tighten, never loosen");
    }

    #[test]
    fn not_live_reads_nothing_and_unreadable_warns() {
        let reads = Arc::new(Mutex::new(0));
        let r2 = reads.clone();
        let mut s = Sensors::empty();
        s.add(ConnRate::new(
            vec![(1, None)],
            Box::new(move || {
                *r2.lock().unwrap() += 1;
                None
            }),
        ));
        let logs = LogInsights::default();
        let cx = s.run(&Input::new(5, &[], &logs));
        assert!(cx.findings().next().is_none());
        assert_eq!(*reads.lock().unwrap(), 0, "replay must not read the host");
        let cx = run(&mut s, 6);
        assert_eq!(cx.bleeding[0].id, "conn-rate-unreadable");
        assert_eq!(*reads.lock().unwrap(), 1);
    }

    #[test]
    fn env_parsing() {
        assert_eq!(
            parse_ports(" 8420=memory-gateway, 8002,x, 0, 8420=dup ,9000= "),
            vec![
                (8420, Some("memory-gateway".into())),
                (8002, None),
                (9000, None)
            ]
        );
        assert!(ConnRate::from_env(&|_| None).is_none());
        assert!(
            ConnRate::from_env(&|k| (k == "NERVE_CONNRATE_PORTS").then(|| ",".into())).is_none()
        );
        let s = ConnRate::from_env(&|k| match k {
            "NERVE_CONNRATE_PORTS" => Some("8420".into()),
            "NERVE_CONNRATE_RATES" => Some("0.5,5,20".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(s.rates(), [0.5, 5.0, 20.0]);
    }

    /// Replay fixture: three captured-shape socket tables, 30 s apart
    /// (tests/fixtures/conn-rate/). Expected verdicts in its README.
    #[test]
    fn replay_fixture() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conn-rate");
        let tables: Vec<Option<String>> = (0..3)
            .map(|k| std::fs::read_to_string(dir.join(format!("tcp-{k}.txt"))).ok())
            .collect();
        assert!(tables.iter().all(|t| t.is_some()), "fixture present");
        let mut s = Sensors::empty();
        s.add(ConnRate::new(
            vec![(8420, Some("memory-gateway".into())), (8002, None)],
            feed(tables),
        ));
        let base = 1_792_000_000;
        let got: Vec<Vec<(String, Sev)>> = (0..3)
            .map(|k| {
                run(&mut s, base + 30 * k)
                    .bleeding
                    .iter()
                    .map(|f| (f.id.clone(), f.sev))
                    .collect()
            })
            .collect();
        assert_eq!(
            got,
            vec![
                vec![],
                vec![
                    ("conn-rate-8420".into(), Sev::Error),
                    ("conn-rate-8002".into(), Sev::Warn)
                ],
                vec![],
            ]
        );
    }
}
