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
//! - `NERVE_CONNRATE_FORMULA=identity|b41flow|max` (slot 178, off by default):
//!   which rate drives the severity. `identity` (default, unset or unknown) is
//!   the new-identity count above, unchanged. `b41flow` is the corrected B41
//!   count formula [`b41_flow_rate`]: the ESTAB+TIME-WAIT delta per interval
//!   plus the TIME-WAIT entries that aged out in it (Linux holds TIME-WAIT for
//!   [`TIME_WAIT_S`]), so a steady load no longer reads ~0. `max` takes the
//!   larger of the two. Non-default modes add one evidence line with both.
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

/// Linux `TCP_TIMEWAIT_LEN` (fixed at 60 s in the kernel, not a sysctl).
pub const TIME_WAIT_S: f64 = 60.0;

/// Which rate drives the conn-rate severity (`NERVE_CONNRATE_FORMULA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Formula {
    /// New connection identities per second (default, cortex-s2-b behaviour).
    #[default]
    Identity,
    /// Corrected B41 count formula, see [`b41_flow_rate`].
    B41Flow,
    /// The larger of the two.
    Max,
}

impl Formula {
    /// `identity` / `b41flow` (also `flow`, `b41-flow`) / `max`; anything
    /// else (or unset) is the default `identity`, so the env can only opt in.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "b41flow" | "b41-flow" | "flow" => Formula::B41Flow,
            "max" => Formula::Max,
            _ => Formula::Identity,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Formula::Identity => "identity",
            Formula::B41Flow => "b41flow",
            Formula::Max => "max",
        }
    }
}

/// Corrected B41 rate from counts only (no identities): new connections per
/// second over `dt_s` = (ESTAB+TIME-WAIT delta + TIME-WAIT entries that aged
/// out during the interval) / `dt_s`. The aged-out part is estimated as
/// `prev_time_wait * min(dt_s, 60) / 60` (TIME-WAIT lasts [`TIME_WAIT_S`];
/// under a steady load its entries are spread evenly over that window).
///
/// B41 3.2 step 2 used the delta alone, which is ~0 under any steady load:
/// arrivals and TIME-WAIT expiries cancel. Never negative; 0 when `dt_s` is 0.
pub fn b41_flow_rate(prev_estab_tw: u32, prev_time_wait: u32, cur_estab_tw: u32, dt_s: u64) -> f64 {
    if dt_s == 0 {
        return 0.0;
    }
    let dt = dt_s as f64;
    let delta = cur_estab_tw as f64 - prev_estab_tw as f64;
    let aged_out = prev_time_wait as f64 * dt.min(TIME_WAIT_S) / TIME_WAIT_S;
    ((delta + aged_out) / dt).max(0.0)
}

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
    time_wait: u32,
}

/// Sensor `conn-rate`.
pub struct ConnRate {
    /// Watched ports and their explicit labels (None: derive from comps).
    ports: Vec<(u16, Option<String>)>,
    rates: [f64; 3],
    formula: Formula,
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
            formula: Formula::Identity,
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
        if let Some(f) = env("NERVE_CONNRATE_FORMULA") {
            s = s.with_formula(Formula::parse(&f));
        }
        Some(s)
    }

    /// Pick the rate formula (default [`Formula::Identity`]).
    pub fn with_formula(mut self, f: Formula) -> Self {
        self.formula = f;
        self
    }

    /// The rate formula in use.
    pub fn formula(&self) -> Formula {
        self.formula
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
                    time_wait: cur.time_wait,
                },
            );
            let Some(prev) = prev else { continue }; // first cycle: baseline
            let dt = i.now.saturating_sub(prev.at);
            if dt == 0 {
                continue;
            }
            let new = cur.conns.difference(&prev.conns).count();
            let id_rate = new as f64 / dt as f64;
            let growth = estab_tw.saturating_sub(prev.estab_tw) as f64 / dt as f64;
            let flow = b41_flow_rate(prev.estab_tw, prev.time_wait, estab_tw, dt);
            let rate = match self.formula {
                Formula::Identity => id_rate,
                Formula::B41Flow => flow,
                Formula::Max => id_rate.max(flow),
            };
            // Count shown in the title: exact identities, or the estimate.
            let exact = match self.formula {
                Formula::Identity => true,
                Formula::B41Flow => false,
                Formula::Max => id_rate >= flow,
            };
            let shown = if exact {
                new
            } else {
                (rate * dt as f64).round() as usize
            };
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
                    format!("port {port}: {rate:.1} new connections/s over {dt} s ({shown} new)"),
                )
                .with_evidence(evidence_lines(vec![
                    format!("new_connections={new} window_s={dt} rate_per_s={rate:.2}"),
                    format!(
                        "estab={} time_wait={} (b41 estab+time_wait growth {growth:.2}/s)",
                        cur.estab, cur.time_wait
                    ),
                    format!(
                        "thresholds warn/high/crit {}/{}/{} per s (B41 4.3)",
                        self.rates[0], self.rates[1], self.rates[2]
                    ),
                ], self.formula, id_rate, flow))
                .with_fix("loopback port: a local client is looping (probe/retry storm); find it with `ss -tnp` (read-only) and fix its interval; the nerve never kills or restarts it")
                .with_cause(format!("conn-rate:{port}"))
                .with_source(label),
            );
        }
    }
}

/// The three cortex-s2-b evidence lines; non-default formulas add a fourth
/// with both rates, so the default output stays byte-identical.
fn evidence_lines(mut v: Vec<String>, f: Formula, id_rate: f64, flow: f64) -> Vec<String> {
    if f != Formula::Identity {
        v.push(format!(
            "formula={} identity_rate={id_rate:.2}/s b41flow_rate={flow:.2}/s (estab+time_wait delta + time_wait aged out over {TIME_WAIT_S} s)",
            f.name()
        ));
    }
    v
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
    // ---- slot 178 cortex-connrate-fix: corrected B41 flow formula ----

    #[test]
    fn b41_flow_rate_math() {
        // Steady 10/s: 600 in TIME-WAIT (60 s x 10/s), flat. Old B41 delta
        // = 0; aged out in 30 s = 300 -> 10/s.
        assert_eq!(b41_flow_rate(600, 600, 600, 30), 10.0);
        // dt 0: no rate.
        assert_eq!(b41_flow_rate(600, 600, 600, 0), 0.0);
        // Ramp-up from idle: delta alone, 300 in 30 s.
        assert_eq!(b41_flow_rate(0, 0, 300, 30), 10.0);
        // Load stopped: pool drains, never negative.
        assert_eq!(b41_flow_rate(600, 600, 0, 30), 0.0);
        // Long-lived ESTAB only, unchanged: 0.
        assert_eq!(b41_flow_rate(5, 0, 5, 30), 0.0);
        // dt > 60 s: at most the whole previous pool aged out (lower bound).
        assert_eq!(b41_flow_rate(600, 600, 600, 120), 5.0);
    }

    #[test]
    fn formula_parse_is_opt_in() {
        assert_eq!(Formula::default(), Formula::Identity);
        assert_eq!(Formula::parse("b41flow"), Formula::B41Flow);
        assert_eq!(Formula::parse(" FLOW "), Formula::B41Flow);
        assert_eq!(Formula::parse("b41-flow"), Formula::B41Flow);
        assert_eq!(Formula::parse("max"), Formula::Max);
        assert_eq!(Formula::parse(""), Formula::Identity);
        assert_eq!(
            Formula::parse("b41"),
            Formula::Identity,
            "unknown -> default"
        );
        let mk = |v: Option<&str>| {
            ConnRate::from_env(&move |k: &str| match k {
                "NERVE_CONNRATE_PORTS" => Some("8642".into()),
                "NERVE_CONNRATE_FORMULA" => v.map(String::from),
                _ => None,
            })
            .unwrap()
            .formula()
        };
        assert_eq!(mk(None), Formula::Identity, "off by default");
        assert_eq!(mk(Some("b41flow")), Formula::B41Flow);
        assert_eq!(mk(Some("max")), Formula::Max);
    }

    /// Steady 10/s: 600 TIME-WAIT flat, 300 replaced per 30 s cycle.
    fn steady(formula: Option<Formula>) -> Vec<crate::cortex::Finding> {
        let mut c = ConnRate::new(
            vec![(8642, Some("hermes-rs".into()))],
            feed(vec![
                Some(conns(8642, 10000, 600, 0x06)),
                Some(conns(8642, 10300, 600, 0x06)),
            ]),
        );
        if let Some(f) = formula {
            c = c.with_formula(f);
        }
        let mut s = Sensors::empty();
        s.add(c);
        assert!(run(&mut s, 1000).bleeding.is_empty());
        run(&mut s, 1030).bleeding
    }

    #[test]
    fn steady_load_b41flow_reads_the_real_rate() {
        // Default: unchanged cortex-s2-b output (3 evidence lines).
        let d = steady(None);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].sev, Sev::Error);
        assert_eq!(
            d[0].title,
            "port 8642: 10.0 new connections/s over 30 s (300 new)"
        );
        assert_eq!(d[0].evidence.len(), 3, "{:?}", d[0].evidence);
        assert!(d[0].evidence[1].contains("growth 0.00/s"));
        // b41flow: same verdict from counts only; 4th evidence line.
        let f = steady(Some(Formula::B41Flow));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].sev, Sev::Error);
        assert_eq!(
            f[0].title,
            "port 8642: 10.0 new connections/s over 30 s (300 new)"
        );
        assert_eq!(f[0].evidence.len(), 4);
        assert_eq!(
            f[0].evidence[3],
            "formula=b41flow identity_rate=10.00/s b41flow_rate=10.00/s (estab+time_wait delta + time_wait aged out over 60 s)"
        );
        let m = steady(Some(Formula::Max));
        assert_eq!(m[0].sev, Sev::Error);
        assert!(m[0].evidence[3].starts_with("formula=max "));
    }

    /// Replay of a READ-ONLY VPS sample (3 reads of /proc/net/tcp + tcp6,
    /// 30 s apart, 2026-10-21T09:24:42+02:00; rows for the watched ports
    /// only, all loopback, uid/inode zeroed): tests/fixtures/conn-rate-vps.
    #[test]
    fn replay_vps_sample_b41_vs_identity_vs_flow() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conn-rate-vps");
        let t: Vec<String> = (0..3)
            .map(|k| std::fs::read_to_string(dir.join(format!("tcp-{k}.txt"))).expect("fixture"))
            .collect();
        let ports = [8642u16, 8091, 8420, 8646, 8650];
        let m: Vec<_> = t.iter().map(|x| parse_tables(x, &ports)).collect();
        let mut rows = vec![];
        for p in ports {
            for k in 0..2 {
                let (a, b) = (&m[k][&p], &m[k + 1][&p]);
                let new = b.conns.difference(&a.conns).count();
                let id = new as f64 / 30.0;
                let (ea, eb) = (a.estab + a.time_wait, b.estab + b.time_wait);
                let old = eb.saturating_sub(ea) as f64 / 30.0;
                let flow = b41_flow_rate(ea, a.time_wait, eb, 30);
                eprintln!("VPS :{p} w{k}: estab+tw {ea}->{eb}; identity {id:.2}/s; b41 old {old:.2}/s; b41flow {flow:.2}/s");
                rows.push((p, k, id, old, flow));
            }
        }
        // hermes-rs :8642 under its steady ~1.4-1.7/s probe load: the old B41
        // delta reads ~0, the corrected flow tracks the identity count.
        for &(p, _, id, old, flow) in rows.iter().filter(|r| r.0 == 8642 || r.0 == 8091) {
            assert!(id >= 1.0, ":{p} identity {id}");
            assert!(old <= 0.2, ":{p} old B41 delta should read ~0, got {old}");
            assert!(
                (flow - id).abs() / id <= 0.25,
                ":{p} flow {flow} vs identity {id}"
            );
        }
        // Sensor end to end over the same sample, flow mode, B41 thresholds.
        let run_mode = |f: Formula| {
            let mut s = Sensors::empty();
            s.add(
                ConnRate::new(
                    vec![
                        (8642, Some("hermes-rs".into())),
                        (8420, Some("memory-gateway".into())),
                    ],
                    feed(t.iter().cloned().map(Some).collect()),
                )
                .with_formula(f),
            );
            (0..3u64)
                .map(|k| {
                    run(&mut s, 1_792_567_482 + 30 * k)
                        .bleeding
                        .iter()
                        .map(|f| (f.id.clone(), f.sev))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let warn8642 = vec![("conn-rate-8642".to_string(), Sev::Warn)];
        let want = vec![vec![], warn8642.clone(), warn8642];
        assert_eq!(run_mode(Formula::Identity), want);
        assert_eq!(run_mode(Formula::B41Flow), want);
        assert_eq!(run_mode(Formula::Max), want);
    }
}
