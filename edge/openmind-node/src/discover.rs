// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Polite LAN discovery.
//!
//! Default: passive mDNS/DNS-SD listening (UDP 5353) + passive SSDP NOTIFY
//! listening (UDP 1900) for N seconds, one SSDP M-SEARCH packet, ARP table
//! read. Opt-in: one active mDNS query packet (`--mdns-query`), and a
//! rate-limited TCP connect probe of a short port list (`--probe`), only on a
//! private network this device was bound to. No port scan by default.
//! `--dry-run` opens no sockets at all.

use crate::config::Config;
use crate::dns::{self, RData};
use crate::identity::Identity;
use crate::sysinfo::{self, ArpEntry};
use crate::{wifi, zagreb};
use serde::Serialize;
use serde_json::{json, Value};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const SSDP_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);

#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub seconds: u64,
    pub dry_run: bool,
    pub mdns_query: bool,
    pub msearch: bool,
    pub probe: bool,
    pub probe_subnet: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Service {
    pub instance: String,
    pub service_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub txt: Vec<String>,
    pub addrs: BTreeSet<IpAddr>,
    pub seen_from: BTreeSet<IpAddr>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SsdpHit {
    pub from: IpAddr,
    pub kind: String,
    pub st: Option<String>,
    pub usn: Option<String>,
    pub server: Option<String>,
    pub location: Option<String>,
}

enum Ev {
    Mdns(IpAddr, Vec<dns::Record>),
    Ssdp(SsdpHit),
    Note(String),
}

fn multicast_socket(port: u16, group: Ipv4Addr, iface: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(unix)]
    let _ = s.set_reuse_port(true);
    s.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).into())?;
    s.join_multicast_v4(&group, &iface)?;
    s.set_multicast_loop_v4(false)?;
    s.set_read_timeout(Some(Duration::from_millis(250)))?;
    Ok(s.into())
}

/// Parse an SSDP NOTIFY or M-SEARCH response (HTTP-over-UDP headers).
pub fn parse_ssdp(from: IpAddr, s: &str) -> Option<SsdpHit> {
    let mut lines = s.lines();
    let first = lines.next()?.trim();
    let kind = if first.starts_with("NOTIFY") {
        "notify"
    } else if first.starts_with("HTTP/1.1 200") {
        "response"
    } else {
        return None; // ignore other M-SEARCHes on the wire
    };
    let mut h = BTreeMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            h.insert(
                k.trim().to_ascii_uppercase(),
                v.trim().chars().take(200).collect::<String>(),
            );
        }
    }
    Some(SsdpHit {
        from,
        kind: kind.into(),
        st: h.get("ST").or_else(|| h.get("NT")).cloned(),
        usn: h.get("USN").cloned(),
        server: h.get("SERVER").cloned(),
        location: h.get("LOCATION").cloned(),
    })
}

fn listen_mdns(deadline: Instant, iface: Ipv4Addr, query: bool, tx: mpsc::Sender<Ev>) {
    let sock = match multicast_socket(5353, MDNS_GROUP, iface) {
        Ok(s) => s,
        Err(e) => return drop(tx.send(Ev::Note(format!("mDNS listen unavailable: {e}")))),
    };
    if query {
        let q = dns::build_query(&["_services._dns-sd._udp.local.", "_openmind._tcp.local."]);
        match sock.send_to(&q, (MDNS_GROUP, 5353)) {
            Ok(_) => drop(tx.send(Ev::Note("sent 1 active mDNS query packet".into()))),
            Err(e) => drop(tx.send(Ev::Note(format!("mDNS query failed: {e}")))),
        }
    }
    let mut buf = [0u8; 9000];
    while Instant::now() < deadline {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            if let Some(recs) = dns::parse(&buf[..n]) {
                if !recs.is_empty() {
                    let _ = tx.send(Ev::Mdns(from.ip(), recs));
                }
            }
        }
    }
}

fn listen_ssdp(deadline: Instant, iface: Ipv4Addr, tx: mpsc::Sender<Ev>) {
    match multicast_socket(1900, SSDP_GROUP, iface) {
        Ok(sock) => {
            let mut buf = [0u8; 4096];
            while Instant::now() < deadline {
                if let Ok((n, from)) = sock.recv_from(&mut buf) {
                    if let Some(h) = parse_ssdp(from.ip(), &String::from_utf8_lossy(&buf[..n])) {
                        let _ = tx.send(Ev::Ssdp(h));
                    }
                }
            }
        }
        Err(e) => drop(tx.send(Ev::Note(format!("SSDP passive listen unavailable: {e}")))),
    }
}

fn msearch(deadline: Instant, tx: mpsc::Sender<Ev>) {
    let r = (|| -> std::io::Result<()> {
        let s = UdpSocket::bind("0.0.0.0:0")?;
        s.set_multicast_ttl_v4(2)?;
        s.set_read_timeout(Some(Duration::from_millis(250)))?;
        let msg = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: ssdp:all\r\n\r\n";
        s.send_to(msg.as_bytes(), (SSDP_GROUP, 1900))?;
        let _ = tx.send(Ev::Note("sent 1 SSDP M-SEARCH packet".into()));
        let mut buf = [0u8; 4096];
        while Instant::now() < deadline {
            if let Ok((n, from)) = s.recv_from(&mut buf) {
                if let Some(h) = parse_ssdp(from.ip(), &String::from_utf8_lossy(&buf[..n])) {
                    let _ = tx.send(Ev::Ssdp(h));
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = r {
        let _ = tx.send(Ev::Note(format!("SSDP M-SEARCH failed: {e}")));
    }
}

/// Fold mDNS records into services keyed by instance name.
/// (services by instance, service types, host name -> addresses)
pub type Folded = (
    BTreeMap<String, Service>,
    BTreeSet<String>,
    BTreeMap<String, BTreeSet<IpAddr>>,
);

pub fn fold_mdns(events: &[(IpAddr, Vec<dns::Record>)]) -> Folded {
    let mut svcs: BTreeMap<String, Service> = BTreeMap::new();
    let mut types = BTreeSet::new();
    let mut hosts: BTreeMap<String, BTreeSet<IpAddr>> = BTreeMap::new();
    for (from, recs) in events {
        for r in recs {
            match &r.data {
                RData::Ptr(t) if r.name == "_services._dns-sd._udp.local." => {
                    types.insert(t.clone());
                }
                RData::Ptr(inst) if r.name.starts_with('_') => {
                    types.insert(r.name.clone());
                    let s = svcs.entry(inst.clone()).or_default();
                    s.instance = inst.clone();
                    s.service_type = r.name.clone();
                    s.seen_from.insert(*from);
                }
                RData::Srv { port, target } => {
                    let s = svcs.entry(r.name.clone()).or_default();
                    s.instance = r.name.clone();
                    s.port = Some(*port);
                    s.host = Some(target.clone());
                    s.seen_from.insert(*from);
                }
                RData::Txt(t) => {
                    let s = svcs.entry(r.name.clone()).or_default();
                    s.instance = r.name.clone();
                    s.txt = t.clone();
                }
                RData::A(a) => {
                    hosts
                        .entry(r.name.clone())
                        .or_default()
                        .insert(IpAddr::V4(*a));
                }
                RData::Aaaa(a) => {
                    hosts
                        .entry(r.name.clone())
                        .or_default()
                        .insert(IpAddr::V6(*a));
                }
                _ => {}
            }
        }
    }
    for s in svcs.values_mut() {
        if s.service_type.is_empty() {
            if let Some(i) = s.instance.find("._") {
                s.service_type = s.instance[i + 1..].to_string();
            }
        }
        if let Some(h) = &s.host {
            if let Some(a) = hosts.get(h) {
                s.addrs.extend(a.iter().copied());
            }
        }
    }
    (svcs, types, hosts)
}

fn probe(targets: &BTreeSet<Ipv4Addr>, cfg: &Config) -> Vec<Value> {
    let ports: Vec<u16> = cfg.discover.probe_ports.iter().copied().take(16).collect();
    let rate = cfg.discover.probe_rate_per_sec.clamp(1, 50);
    let gap = Duration::from_millis(1000 / rate as u64);
    let timeout = Duration::from_millis(cfg.discover.probe_timeout_ms.clamp(50, 2000));
    let mut out = Vec::new();
    for ip in targets {
        let mut open = Vec::new();
        for &p in &ports {
            let t = Instant::now();
            if TcpStream::connect_timeout(&SocketAddr::from((*ip, p)), timeout).is_ok() {
                open.push(p);
            }
            if let Some(rest) = gap.checked_sub(t.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        if !open.is_empty() {
            out.push(json!({"ip": ip, "open_ports": open}));
        }
    }
    out
}

pub fn run(cfg: &Config, opts: &Opts, id: Option<&Identity>) -> Result<Value, String> {
    let started = zagreb::now_rfc3339();
    let me = sysinfo::self_info();
    let arp: Option<Vec<ArpEntry>> = sysinfo::arp_table();
    let obs = wifi::observe(!opts.dry_run);
    let net = id.and_then(|i| i.current_network(cfg, &obs));
    let mut notes: Vec<String> = Vec::new();
    if arp.is_none() {
        notes.push("/proc/net/arp not readable (normal on Android 10+)".into());
    }
    if sysinfo::platform() == "android-termux" {
        notes.push("Android drops Wi-Fi multicast for apps without a MulticastLock; Termux holds none, so passive mDNS/SSDP may see little. ARP/neighbour and opt-in probe still work.".into());
    }

    let mut base = json!({
        "generated": started,
        "self": me,
        "network": {
            "observed": obs.public(),
            "hash": net.as_ref().map(|n| &n.0),
            "anchor": net.as_ref().map(|n| n.1),
            "trusted": net.as_ref().map(|n| n.2).unwrap_or(false),
            "identity": id.is_some(),
        },
        "arp": arp.clone().unwrap_or_default(),
    });

    if opts.dry_run {
        base["mode"] = json!("dry-run");
        base["plan"] = json!({
            "opens_sockets": false,
            "would_listen_seconds": opts.seconds,
            "would_listen": ["udp 0.0.0.0:5353 (mDNS group 224.0.0.251)", "udp 0.0.0.0:1900 (SSDP group 239.255.255.250)"],
            "would_send": {
                "ssdp_msearch_packets": if opts.msearch { 1 } else { 0 },
                "mdns_query_packets": if opts.mdns_query { 1 } else { 0 },
            },
            "tcp_probe": opts.probe,
            "tcp_probe_ports": cfg.discover.probe_ports,
        });
        base["notes"] = json!(notes);
        return Ok(base);
    }

    let iface = obs.local_ip.unwrap_or(Ipv4Addr::UNSPECIFIED);
    let deadline = Instant::now() + Duration::from_secs(opts.seconds.clamp(1, 120));
    let (tx, rx) = mpsc::channel();
    let mut hs = Vec::new();
    {
        let tx = tx.clone();
        let q = opts.mdns_query;
        hs.push(std::thread::spawn(move || {
            listen_mdns(deadline, iface, q, tx)
        }));
    }
    {
        let tx = tx.clone();
        hs.push(std::thread::spawn(move || listen_ssdp(deadline, iface, tx)));
    }
    if opts.msearch {
        let tx = tx.clone();
        hs.push(std::thread::spawn(move || msearch(deadline, tx)));
    }
    drop(tx);
    let mut mdns_ev = Vec::new();
    let mut ssdp: Vec<SsdpHit> = Vec::new();
    for ev in rx {
        match ev {
            Ev::Mdns(f, r) => mdns_ev.push((f, r)),
            Ev::Ssdp(h) => {
                if !ssdp.contains(&h) {
                    ssdp.push(h)
                }
            }
            Ev::Note(n) => notes.push(n),
        }
    }
    for h in hs {
        let _ = h.join();
    }

    let (svcs, types, hostmap) = fold_mdns(&mdns_ev);
    let peers: Vec<&Service> = svcs
        .values()
        .filter(|s| s.service_type.starts_with("_openmind._tcp"))
        .collect();

    // Hosts: union of ARP, mDNS senders/addresses and SSDP senders.
    fn entry(m: &mut BTreeMap<IpAddr, Value>, ip: IpAddr) -> &mut Value {
        m.entry(ip).or_insert_with(
            || json!({"ip": ip, "mac": null, "names": [], "services": [], "ssdp": false}),
        )
    }
    let mut hosts: BTreeMap<IpAddr, Value> = BTreeMap::new();
    for e in arp.iter().flatten() {
        entry(&mut hosts, IpAddr::V4(e.ip))["mac"] = json!(e.mac);
    }
    for (name, addrs) in &hostmap {
        for a in addrs {
            entry(&mut hosts, *a)["names"]
                .as_array_mut()
                .unwrap()
                .push(json!(name));
        }
    }
    for s in svcs.values() {
        for a in s.addrs.iter().chain(s.seen_from.iter()) {
            let arr = entry(&mut hosts, *a)["services"].as_array_mut().unwrap();
            if !arr.contains(&json!(s.instance)) {
                arr.push(json!(s.instance));
            }
        }
    }
    for h in &ssdp {
        entry(&mut hosts, h.from)["ssdp"] = json!(true);
    }

    let mut probe_out = Value::Null;
    if opts.probe {
        let trusted = net.as_ref().map(|n| n.2).unwrap_or(false);
        let local = obs.local_ip.filter(|ip| sysinfo::is_private(*ip));
        match (trusted, local) {
            (false, _) => notes.push("TCP probe refused: this network is not bound as trusted (run `omnode identity bind` on a network you own)".into()),
            (true, None) => notes.push("TCP probe refused: no private IPv4 address on this network".into()),
            (true, Some(me_ip)) => {
                let o = me_ip.octets();
                let in_24 = |ip: &Ipv4Addr| ip.octets()[..3] == o[..3] && *ip != me_ip;
                let mut targets: BTreeSet<Ipv4Addr> = hosts
                    .keys()
                    .filter_map(|ip| match ip {
                        IpAddr::V4(v) if in_24(v) => Some(*v),
                        _ => None,
                    })
                    .collect();
                if opts.probe_subnet {
                    targets.extend((1..=254).map(|d| Ipv4Addr::new(o[0], o[1], o[2], d)).filter(in_24));
                }
                notes.push(format!(
                    "TCP probe: {} hosts x {} ports, <= {}/s",
                    targets.len(),
                    cfg.discover.probe_ports.len().min(16),
                    cfg.discover.probe_rate_per_sec.clamp(1, 50)
                ));
                probe_out = json!(probe(&targets, cfg));
            }
        }
    }

    base["mode"] = json!("live");
    base["finished"] = json!(zagreb::now_rfc3339());
    base["service_types"] = json!(types);
    base["services"] = json!(svcs.values().collect::<Vec<_>>());
    base["openmind_peers"] = json!(peers);
    base["ssdp"] = json!(ssdp);
    base["hosts"] = json!(hosts.values().collect::<Vec<_>>());
    base["probe"] = probe_out;
    base["notes"] = json!(notes);
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssdp_parse() {
        let ip: IpAddr = "192.168.1.5".parse().unwrap();
        let n = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nNT: upnp:rootdevice\r\nUSN: uuid:x::upnp:rootdevice\r\nLOCATION: http://192.168.1.5:80/desc.xml\r\nSERVER: Linux UPnP/1.0\r\n\r\n";
        let h = parse_ssdp(ip, n).unwrap();
        assert_eq!(h.kind, "notify");
        assert_eq!(h.st.as_deref(), Some("upnp:rootdevice"));
        assert_eq!(
            h.location.as_deref(),
            Some("http://192.168.1.5:80/desc.xml")
        );
        assert!(parse_ssdp(ip, "M-SEARCH * HTTP/1.1\r\n").is_none());
        assert_eq!(
            parse_ssdp(ip, "HTTP/1.1 200 OK\r\nST: ssdp:all\r\n")
                .unwrap()
                .kind,
            "response"
        );
    }

    #[test]
    fn fold() {
        let from: IpAddr = "192.168.1.9".parse().unwrap();
        let recs = vec![
            dns::Record {
                name: "_openmind._tcp.local.".into(),
                data: RData::Ptr("omn-1._openmind._tcp.local.".into()),
            },
            dns::Record {
                name: "omn-1._openmind._tcp.local.".into(),
                data: RData::Srv {
                    port: 7787,
                    target: "phone.local.".into(),
                },
            },
            dns::Record {
                name: "phone.local.".into(),
                data: RData::A(Ipv4Addr::new(192, 168, 1, 9)),
            },
        ];
        let (s, t, h) = fold_mdns(&[(from, recs)]);
        let svc = &s["omn-1._openmind._tcp.local."];
        assert_eq!(svc.service_type, "_openmind._tcp.local.");
        assert_eq!(svc.port, Some(7787));
        assert!(svc.addrs.contains(&from));
        assert!(t.contains("_openmind._tcp.local."));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn dry_run_opens_nothing() {
        let v = run(
            &Config::default(),
            &Opts {
                seconds: 3,
                dry_run: true,
                msearch: true,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(v["mode"], "dry-run");
        assert_eq!(v["plan"]["opens_sockets"], false);
        assert_eq!(v["plan"]["would_send"]["mdns_query_packets"], 0);
        assert!(v.get("services").is_none());
    }
}
