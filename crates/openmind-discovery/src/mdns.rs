// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! mDNS / DNS-SD over plain UDP: a one-shot querier ([`browse`]) and a small
//! responder ([`Announcer`]).
//!
//! The querier sends from an ephemeral port, which makes it a "legacy
//! unicast" querier (RFC 6762 6.7): responders answer straight back to it,
//! so it never needs to bind 5353 or join the multicast group. It sends at
//! most two packets per call and waits a bounded time.
//!
//! The responder binds the mDNS port with SO_REUSEADDR/SO_REUSEPORT so it can
//! sit next to avahi. It answers PTR, SRV, TXT, A and ANY questions for its
//! own services only. It does not probe for name conflicts (RFC 6762 8.1);
//! it is a test and edge-node announcer, not a full host responder.

use crate::dns::{self, Question, TYPE_A, TYPE_ANY, TYPE_PTR, TYPE_SRV, TYPE_TXT};
use crate::{RData, Record};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::BTreeSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The mDNS IPv4 group.
pub const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
/// The standard mDNS port.
pub const MDNS_PORT: u16 = 5353;
/// DNS-SD service enumeration name.
pub const SERVICES_ENUM: &str = "_services._dns-sd._udp.local.";
/// Longest browse the CLI allows.
pub const MAX_BROWSE: Duration = Duration::from_secs(30);

/// Service types browsed by default: Galion edge nodes, Exo, llama.cpp,
/// Ollama and OpenMind peers. Ollama and llama.cpp do not announce mDNS by
/// themselves; these names match what a Galion edge wrapper announces for
/// them, and the active probe covers the plain servers.
pub const DEFAULT_SERVICES: [&str; 6] = [
    "_galion._tcp.local.",
    "_exo._tcp.local.",
    "_llama-rpc._tcp.local.",
    "_llamacpp._tcp.local.",
    "_ollama._tcp.local.",
    "_openmind._tcp.local.",
];

/// Browse settings.
#[derive(Debug, Clone)]
pub struct BrowseOpts {
    pub services: Vec<String>,
    pub timeout: Duration,
    /// Destination port. 5353 in real use; tests may use their own port.
    pub port: u16,
    /// Interface address for the outgoing multicast. `None` = route default.
    pub iface: Option<Ipv4Addr>,
}

impl Default for BrowseOpts {
    fn default() -> Self {
        Self {
            services: DEFAULT_SERVICES.iter().map(|s| s.to_string()).collect(),
            timeout: Duration::from_millis(2000),
            port: MDNS_PORT,
            iface: None,
        }
    }
}

/// What one browse saw.
#[derive(Debug, Clone, Default)]
pub struct BrowseOutcome {
    pub packets_sent: u32,
    pub packets_received: u32,
    /// Source addresses of the responses, sorted.
    pub responders: Vec<String>,
    /// All records from all responses, in arrival order.
    pub records: Vec<Record>,
    /// Interface address the query left from.
    pub iface: String,
}

/// The IPv4 address the kernel would use toward the mDNS group.
///
/// Uses `connect` on a UDP socket, which sends no packet.
pub fn local_ipv4() -> Ipv4Addr {
    let probe = || -> io::Result<Ipv4Addr> {
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.connect(SocketAddrV4::new(MDNS_GROUP, MDNS_PORT))?;
        match sock.local_addr()?.ip() {
            IpAddr::V4(ip) if !ip.is_unspecified() => Ok(ip),
            _ => Ok(Ipv4Addr::LOCALHOST),
        }
    };
    probe().unwrap_or(Ipv4Addr::LOCALHOST)
}

fn query_id() -> u16 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    ((nanos >> 8) as u16) ^ (std::process::id() as u16) | 1
}

/// Send a PTR query for `opts.services` to the mDNS group and collect answers.
///
/// Sends one query at the start and one repeat at a third of the timeout
/// (RFC 6762 recommends a repeat because UDP may drop). Never more than two.
pub fn browse(opts: &BrowseOpts) -> io::Result<BrowseOutcome> {
    let timeout = opts.timeout.min(MAX_BROWSE);
    let iface = opts.iface.unwrap_or_else(local_ipv4);
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)).into())?;
    if !iface.is_loopback() {
        sock.set_multicast_if_v4(&iface)?;
    }
    sock.set_multicast_loop_v4(true)?;
    sock.set_multicast_ttl_v4(255)?;
    let sock: UdpSocket = sock.into();

    let names: Vec<(&str, u16)> = opts
        .services
        .iter()
        .map(|s| (s.as_str(), TYPE_PTR))
        .collect();
    let id = query_id();
    let query = dns::build_query(id, &names, false);
    let dest = SocketAddrV4::new(MDNS_GROUP, opts.port);

    let mut out = BrowseOutcome {
        iface: iface.to_string(),
        ..BrowseOutcome::default()
    };
    let mut responders = BTreeSet::new();
    let start = Instant::now();
    let deadline = start + timeout;
    let repeat_at = start + timeout / 3;
    let mut repeated = false;
    sock.send_to(&query, dest)?;
    out.packets_sent += 1;

    let mut buf = [0u8; 9000];
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if !repeated && now >= repeat_at {
            sock.send_to(&query, dest)?;
            out.packets_sent += 1;
            repeated = true;
        }
        let next = if repeated { deadline } else { repeat_at };
        let wait = next
            .saturating_duration_since(now)
            .clamp(Duration::from_millis(5), Duration::from_millis(250));
        sock.set_read_timeout(Some(wait))?;
        match sock.recv_from(&mut buf) {
            Ok((len, src)) => {
                let Ok(msg) = dns::parse_message(&buf[..len]) else {
                    continue;
                };
                if !msg.response {
                    continue;
                }
                out.packets_received += 1;
                responders.insert(src.ip().to_string());
                out.records
                    .extend(with_source_address(msg.records, src.ip()));
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(err) => return Err(err),
        }
    }
    out.responders = responders.into_iter().collect();
    Ok(out)
}

/// If a packet carries an SRV whose target has no address in the same
/// packet, use the packet's source address for that target.
fn with_source_address(mut records: Vec<Record>, src: IpAddr) -> Vec<Record> {
    let has_addr: BTreeSet<String> = records
        .iter()
        .filter(|r| matches!(r.rdata, RData::A(_) | RData::Aaaa(_)))
        .map(|r| r.name.clone())
        .collect();
    let mut extra = Vec::new();
    let mut added = BTreeSet::new();
    for rec in &records {
        if let RData::Srv { target, .. } = &rec.rdata {
            if !has_addr.contains(target) && added.insert(target.clone()) {
                extra.push(Record {
                    name: target.clone(),
                    ttl: rec.ttl,
                    rdata: match src {
                        IpAddr::V4(ip) => RData::A(ip.to_string()),
                        IpAddr::V6(ip) => RData::Aaaa(ip.to_string()),
                    },
                });
            }
        }
    }
    records.extend(extra);
    records
}

/// One announced DNS-SD service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Instance label, e.g. `box-galion`. Lowercase `[a-z0-9-]`.
    pub instance: String,
    /// Service type with `.local.`, e.g. `_galion._tcp.local.`.
    pub service_type: String,
    pub port: u16,
    pub txt: Vec<(String, String)>,
}

impl Service {
    pub fn fqdn(&self) -> String {
        format!("{}.{}", self.instance, self.service_type)
    }
}

/// A small mDNS responder for a fixed set of services on one host name.
#[derive(Debug, Clone)]
pub struct Announcer {
    /// Host name with `.local.`, e.g. `gaia-box.local.`.
    pub host: String,
    pub addr: Ipv4Addr,
    pub services: Vec<Service>,
    /// mDNS port to bind and announce on. 5353 in real use.
    pub port: u16,
    pub ttl: u32,
}

/// Counters from [`Announcer::run`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnnounceStats {
    pub queries_seen: u32,
    pub replies_sent: u32,
    pub announcements_sent: u32,
}

impl Announcer {
    fn a_record(&self) -> Record {
        Record {
            name: self.host.clone(),
            ttl: None,
            rdata: RData::A(self.addr.to_string()),
        }
    }

    fn srv_txt(&self, svc: &Service) -> [Record; 2] {
        [
            Record {
                name: svc.fqdn(),
                ttl: None,
                rdata: RData::Srv {
                    priority: 0,
                    weight: 0,
                    port: svc.port,
                    target: self.host.clone(),
                },
            },
            Record {
                name: svc.fqdn(),
                ttl: None,
                rdata: RData::Txt(svc.txt.clone()),
            },
        ]
    }

    fn ptr(&self, svc: &Service) -> Record {
        Record {
            name: svc.service_type.clone(),
            ttl: None,
            rdata: RData::Ptr(svc.fqdn()),
        }
    }

    /// Every record this responder owns (used for unsolicited announcements).
    pub fn all_records(&self) -> Vec<Record> {
        let mut out = Vec::new();
        for svc in &self.services {
            out.push(self.ptr(svc));
            out.extend(self.srv_txt(svc));
        }
        out.push(self.a_record());
        out
    }

    /// Answers and additionals for a set of questions. Empty when none match.
    pub fn answer(&self, questions: &[Question]) -> (Vec<Record>, Vec<Record>) {
        let mut answers: Vec<Record> = Vec::new();
        let mut extra: Vec<Record> = Vec::new();
        let any = |q: &Question, t: u16| q.qtype == t || q.qtype == TYPE_ANY;
        for q in questions {
            for svc in &self.services {
                if q.name == svc.service_type && any(q, TYPE_PTR) {
                    answers.push(self.ptr(svc));
                    extra.extend(self.srv_txt(svc));
                    extra.push(self.a_record());
                }
                if q.name == SERVICES_ENUM && any(q, TYPE_PTR) {
                    answers.push(Record {
                        name: SERVICES_ENUM.to_string(),
                        ttl: None,
                        rdata: RData::Ptr(svc.service_type.clone()),
                    });
                }
                if q.name == svc.fqdn() {
                    let [srv, txt] = self.srv_txt(svc);
                    if any(q, TYPE_SRV) {
                        answers.push(srv);
                        extra.push(self.a_record());
                    }
                    if any(q, TYPE_TXT) {
                        answers.push(txt);
                    }
                }
            }
            if q.name == self.host && any(q, TYPE_A) {
                answers.push(self.a_record());
            }
        }
        dedupe(&mut answers);
        extra.retain(|r| !answers.contains(r));
        dedupe(&mut extra);
        (answers, extra)
    }

    fn socket(&self) -> io::Result<UdpSocket> {
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        sock.set_reuse_address(true)?;
        #[cfg(all(
            unix,
            not(any(target_os = "solaris", target_os = "illumos", target_os = "cygwin"))
        ))]
        sock.set_reuse_port(true)?;
        sock.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, self.port)).into())?;
        let iface = if self.addr.is_loopback() {
            Ipv4Addr::UNSPECIFIED
        } else {
            self.addr
        };
        sock.join_multicast_v4(&MDNS_GROUP, &iface)?;
        if !iface.is_unspecified() {
            sock.set_multicast_if_v4(&iface)?;
        }
        sock.set_multicast_loop_v4(true)?;
        sock.set_multicast_ttl_v4(255)?;
        sock.set_read_timeout(Some(Duration::from_millis(500)))?;
        Ok(sock.into())
    }

    /// Answer queries until `until`. Sends two unsolicited announcements at
    /// the start (RFC 6762 8.3). Replies are capped at 20 per second.
    /// `log` gets one line per event.
    pub fn run(&self, until: Instant, mut log: impl FnMut(&str)) -> io::Result<AnnounceStats> {
        let sock = self.socket()?;
        let group = SocketAddrV4::new(MDNS_GROUP, self.port);
        let mut stats = AnnounceStats::default();
        let announce = dns::build_response(0, &[], &self.all_records(), &[], self.ttl, true);
        let mut next_announce = Some(Instant::now());
        let mut window = Instant::now();
        let mut in_window = 0u32;
        let mut buf = [0u8; 9000];
        while Instant::now() < until {
            if let Some(at) = next_announce {
                if Instant::now() >= at {
                    sock.send_to(&announce, group)?;
                    stats.announcements_sent += 1;
                    log(&format!("announce multicast {group}"));
                    next_announce = (stats.announcements_sent < 2)
                        .then(|| Instant::now() + Duration::from_secs(1));
                }
            }
            let (len, src) = match sock.recv_from(&mut buf) {
                Ok(pair) => pair,
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(err) => return Err(err),
            };
            let Ok(msg) = dns::parse_message(&buf[..len]) else {
                continue;
            };
            if msg.response || msg.questions.is_empty() {
                continue;
            }
            stats.queries_seen += 1;
            let (answers, extra) = self.answer(&msg.questions);
            if answers.is_empty() {
                continue;
            }
            if window.elapsed() >= Duration::from_secs(1) {
                window = Instant::now();
                in_window = 0;
            }
            if in_window >= 20 {
                log(&format!("rate-limited reply to {src}"));
                continue;
            }
            in_window += 1;
            let legacy = src.port() != self.port;
            let (wire, dest, how) = if legacy {
                // Legacy unicast: echo id and questions, short TTL, no flush bit.
                let wire = dns::build_response(msg.id, &msg.questions, &answers, &extra, 10, false);
                (wire, src, "legacy-unicast")
            } else if msg.questions.iter().any(|q| q.unicast) {
                let wire = dns::build_response(0, &[], &answers, &extra, self.ttl, true);
                (wire, src, "unicast")
            } else {
                let wire = dns::build_response(0, &[], &answers, &extra, self.ttl, true);
                (wire, SocketAddr::V4(group), "multicast")
            };
            sock.send_to(&wire, dest)?;
            stats.replies_sent += 1;
            let asked: Vec<&str> = msg.questions.iter().map(|q| q.name.as_str()).collect();
            log(&format!(
                "query from {src} for {} -> {how} reply to {dest} ({} answers)",
                asked.join(","),
                answers.len()
            ));
        }
        Ok(stats)
    }
}

fn dedupe(records: &mut Vec<Record>) {
    let mut seen: Vec<Record> = Vec::with_capacity(records.len());
    records.retain(|r| {
        if seen.contains(r) {
            false
        } else {
            seen.push(r.clone());
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn announcer() -> Announcer {
        Announcer {
            host: "gaia-box.local.".into(),
            addr: Ipv4Addr::new(192, 0, 2, 7),
            services: vec![
                Service {
                    instance: "box-galion".into(),
                    service_type: "_galion._tcp.local.".into(),
                    port: 19111,
                    txt: vec![("id".into(), "box-galion".into())],
                },
                Service {
                    instance: "box-exo".into(),
                    service_type: "_exo._tcp.local.".into(),
                    port: 19112,
                    txt: vec![],
                },
            ],
            port: MDNS_PORT,
            ttl: 120,
        }
    }

    fn q(name: &str, qtype: u16) -> Question {
        Question {
            name: name.into(),
            qtype,
            unicast: false,
        }
    }

    #[test]
    fn ptr_question_gets_ptr_plus_srv_txt_a() {
        let a = announcer();
        let (ans, extra) = a.answer(&[q("_exo._tcp.local.", TYPE_PTR)]);
        assert_eq!(ans.len(), 1);
        assert_eq!(ans[0].rdata, RData::Ptr("box-exo._exo._tcp.local.".into()));
        assert_eq!(extra.len(), 3);
        let rows = crate::fold_records(&[ans, extra].concat());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].port, Some(19112));
        assert_eq!(rows[0].addrs, vec!["192.0.2.7"]);
    }

    #[test]
    fn unrelated_question_gets_nothing() {
        let a = announcer();
        let (ans, extra) = a.answer(&[q("_ipp._tcp.local.", TYPE_PTR)]);
        assert!(ans.is_empty() && extra.is_empty());
        let (ans, _) = a.answer(&[q("other.local.", TYPE_A)]);
        assert!(ans.is_empty());
    }

    #[test]
    fn enumeration_srv_and_a() {
        let a = announcer();
        let (ans, _) = a.answer(&[q(SERVICES_ENUM, TYPE_PTR)]);
        assert_eq!(ans.len(), 2);
        let (ans, extra) = a.answer(&[q("box-galion._galion._tcp.local.", TYPE_ANY)]);
        assert_eq!(ans.len(), 2);
        assert_eq!(extra.len(), 1);
        let (ans, _) = a.answer(&[q("gaia-box.local.", TYPE_A)]);
        assert_eq!(ans, vec![a.a_record()]);
    }

    #[test]
    fn source_address_fills_missing_a() {
        let recs = vec![Record {
            name: "x._exo._tcp.local.".into(),
            ttl: Some(10),
            rdata: RData::Srv {
                priority: 0,
                weight: 0,
                port: 1,
                target: "x.local.".into(),
            },
        }];
        let out = with_source_address(recs, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9)));
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].rdata, RData::A("10.0.0.9".into()));
    }
}
