// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! "Wi-Fi identity": which network is this device on?
//!
//! We observe SSID, BSSID, default gateway and the gateway's MAC with whatever
//! the platform allows, then keep only a salted SHA-256 of
//! `SSID + gateway MAC` (BSSID when the gateway MAC is unreadable). The raw
//! SSID/MACs are never written to disk or into pairing payloads.

use crate::sysinfo;
use crate::util::{hex, read_file, run_cmd};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

const T: Duration = Duration::from_secs(4);

#[derive(Debug, Default, Clone)]
pub struct Observation {
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub iface: Option<String>,
    pub gateway_ip: Option<Ipv4Addr>,
    pub gateway_mac: Option<String>,
    pub local_ip: Option<Ipv4Addr>,
    pub sources: Vec<&'static str>,
}

/// Privacy-safe view: booleans and the gateway IP, never SSID or MACs.
#[derive(Debug, Serialize)]
pub struct PublicView {
    pub iface: Option<String>,
    pub ssid_known: bool,
    pub bssid_known: bool,
    pub gateway_ip: Option<Ipv4Addr>,
    pub gateway_mac_known: bool,
    pub local_ip: Option<Ipv4Addr>,
    pub sources: Vec<&'static str>,
}

impl Observation {
    pub fn public(&self) -> PublicView {
        PublicView {
            iface: self.iface.clone(),
            ssid_known: self.ssid.is_some(),
            bssid_known: self.bssid.is_some(),
            gateway_ip: self.gateway_ip,
            gateway_mac_known: self.gateway_mac.is_some(),
            local_ip: self.local_ip,
            sources: self.sources.clone(),
        }
    }

    /// Which material the hash is anchored on.
    pub fn anchor(&self) -> Option<(&'static str, String)> {
        let ssid = self.ssid.clone().unwrap_or_default();
        match (&self.gateway_mac, &self.bssid, self.ssid.is_some()) {
            (Some(m), _, true) => Some(("ssid+gateway_mac", format!("{ssid}\0{m}"))),
            (None, Some(b), true) => Some(("ssid+bssid", format!("{ssid}\0{b}"))),
            (Some(m), _, false) => Some(("gateway_mac", format!("\0{m}"))),
            (None, Some(b), false) => Some(("bssid", format!("\0{b}"))),
            (None, None, true) => Some(("ssid", format!("{ssid}\0"))),
            (None, None, false) => None,
        }
    }
}

/// Salted network hash: hex(SHA-256("omnet1" || salt || anchor))[..32].
pub fn network_hash(salt: &[u8], obs: &Observation) -> Option<(String, &'static str)> {
    let (kind, material) = obs.anchor()?;
    let mut h = Sha256::new();
    h.update(b"omnet1\0");
    h.update(salt);
    h.update(material.as_bytes());
    Some((hex(&h.finalize())[..32].to_string(), kind))
}

fn norm_mac(s: &str) -> Option<String> {
    let m = s.trim().replace('-', ":").to_ascii_lowercase();
    let ok = m.len() == 17
        && m.split(':')
            .all(|p| p.len() == 2 && u8::from_str_radix(p, 16).is_ok());
    // 02:00:00:00:00:00 is Android's "permission denied" placeholder.
    (ok && m != "00:00:00:00:00:00" && m != "02:00:00:00:00:00").then_some(m)
}

fn norm_ssid(s: &str) -> Option<String> {
    let s = s.trim().trim_matches('"');
    (!s.is_empty() && s != "<unknown ssid>" && s != "--").then(|| s.to_string())
}

/// termux-wifi-connectioninfo JSON (Termux:API app + location permission).
pub fn parse_termux(json: &str) -> (Option<String>, Option<String>, Option<Ipv4Addr>) {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    (
        s("ssid").and_then(|x| norm_ssid(&x)),
        s("bssid").and_then(|x| norm_mac(&x)),
        s("ip").and_then(|x| x.parse().ok()),
    )
}

/// `iw dev <if> link` -> (ssid, bssid)
pub fn parse_iw_link(s: &str) -> (Option<String>, Option<String>) {
    let mut ssid = None;
    let mut bssid = None;
    for l in s.lines() {
        let l = l.trim();
        if let Some(r) = l.strip_prefix("Connected to ") {
            bssid = r.split_whitespace().next().and_then(norm_mac);
        } else if let Some(r) = l.strip_prefix("SSID: ") {
            ssid = norm_ssid(r);
        }
    }
    (ssid, bssid)
}

/// `nmcli -t -f ACTIVE,SSID,BSSID dev wifi` -> (ssid, bssid) of the active row.
pub fn parse_nmcli(s: &str) -> (Option<String>, Option<String>) {
    for l in s.lines() {
        // Split on ':' not preceded by '\'.
        let mut fields = vec![String::new()];
        let mut esc = false;
        for c in l.chars() {
            match (esc, c) {
                (true, _) => {
                    fields.last_mut().unwrap().push(c);
                    esc = false;
                }
                (false, '\\') => esc = true,
                (false, ':') => fields.push(String::new()),
                (false, _) => fields.last_mut().unwrap().push(c),
            }
        }
        if fields.len() >= 3 && fields[0] == "yes" {
            return (norm_ssid(&fields[1]), norm_mac(&fields[2]));
        }
    }
    (None, None)
}

/// `ip route get <addr>` / `ip route show default` -> (gateway, dev)
pub fn parse_ip_route(s: &str) -> (Option<Ipv4Addr>, Option<String>) {
    let f: Vec<&str> = s.split_whitespace().collect();
    let after = |k: &str| {
        f.iter()
            .position(|x| *x == k)
            .and_then(|i| f.get(i + 1))
            .map(|x| x.to_string())
    };
    (after("via").and_then(|x| x.parse().ok()), after("dev"))
}

fn wireless_ifaces() -> Vec<String> {
    let mut v: Vec<String> = read_file("/proc/net/wireless")
        .unwrap_or_default()
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next().map(|x| x.trim().to_string()))
        .filter(|x| !x.is_empty())
        .collect();
    if v.is_empty() {
        if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
            for e in rd.flatten() {
                if e.path().join("wireless").exists() {
                    v.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }
    v
}

/// Local source address the kernel would use (UDP connect sends no packet).
fn local_ip_towards(dst: Ipv4Addr) -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect((dst, 9)).ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v) if !v.is_unspecified() => Some(v),
        _ => None,
    }
}

/// Observe the current network with whatever tools exist. Never fails.
/// `sockets=false` skips the UDP route lookup used to learn the local IP.
pub fn observe(sockets: bool) -> Observation {
    let mut o = Observation::default();

    if crate::util::is_termux() {
        if let Some(j) = run_cmd("termux-wifi-connectioninfo", &[], T) {
            let (ssid, bssid, ip) = parse_termux(&j);
            o.ssid = ssid;
            o.bssid = bssid;
            o.local_ip = ip;
            o.sources.push("termux-wifi-connectioninfo");
        }
    }

    let wl = wireless_ifaces();
    if o.ssid.is_none() {
        for ifc in &wl {
            if let Some(out) = run_cmd("iw", &["dev", ifc, "link"], T) {
                let (ssid, bssid) = parse_iw_link(&out);
                if ssid.is_some() {
                    o.ssid = ssid;
                    o.bssid = o.bssid.take().or(bssid);
                    o.iface = Some(ifc.clone());
                    o.sources.push("iw");
                    break;
                }
            }
        }
    }
    if o.ssid.is_none() {
        if let Some(out) = run_cmd(
            "nmcli",
            &["-t", "-f", "ACTIVE,SSID,BSSID", "dev", "wifi"],
            T,
        ) {
            let (ssid, bssid) = parse_nmcli(&out);
            if ssid.is_some() {
                o.ssid = ssid;
                o.bssid = o.bssid.take().or(bssid);
                o.sources.push("nmcli");
            }
        }
    }
    if o.iface.is_none() {
        o.iface = wl.first().cloned();
    }

    // Default gateway: /proc/net/route, else `ip route` (route lookup only;
    // 192.0.2.1 is TEST-NET-1 documentation space, nothing is contacted).
    if let Some((ifc, gw)) = read_file("/proc/net/route").and_then(|s| sysinfo::parse_route(&s)) {
        o.gateway_ip = Some(gw);
        o.iface.get_or_insert(ifc);
        o.sources.push("/proc/net/route");
    } else {
        for args in [
            &["route", "show", "default"][..],
            &["route", "get", "192.0.2.1"][..],
        ] {
            if let Some(out) = run_cmd("ip", args, T) {
                let (gw, dev) = parse_ip_route(&out);
                if gw.is_some() {
                    o.gateway_ip = gw;
                    if o.iface.is_none() {
                        o.iface = dev;
                    }
                    o.sources.push("ip route");
                    break;
                }
            }
        }
    }

    if let Some(gw) = o.gateway_ip {
        if let Some(e) = sysinfo::arp_table().and_then(|t| t.into_iter().find(|e| e.ip == gw)) {
            o.gateway_mac = Some(e.mac);
            o.sources.push("/proc/net/arp");
        } else if let Some(out) = run_cmd("ip", &["neigh", "show", &gw.to_string()], T) {
            let f: Vec<&str> = out.split_whitespace().collect();
            if let Some(m) = f
                .iter()
                .position(|x| *x == "lladdr")
                .and_then(|i| f.get(i + 1))
                .and_then(|m| norm_mac(m))
            {
                o.gateway_mac = Some(m);
                o.sources.push("ip neigh");
            }
        }
        if o.local_ip.is_none() && sockets {
            o.local_ip = local_ip_towards(gw);
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(ssid: Option<&str>, bssid: Option<&str>, gw: Option<&str>) -> Observation {
        Observation {
            ssid: ssid.map(Into::into),
            bssid: bssid.map(Into::into),
            gateway_mac: gw.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn hash_is_salted_and_stable() {
        let o = obs(Some("HomeNet"), None, Some("aa:bb:cc:dd:ee:ff"));
        let (a, kind) = network_hash(b"salt-1", &o).unwrap();
        let (b, _) = network_hash(b"salt-1", &o).unwrap();
        let (c, _) = network_hash(b"salt-2", &o).unwrap();
        assert_eq!(kind, "ssid+gateway_mac");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 32);
        assert!(!a.contains("HomeNet"));
    }

    #[test]
    fn hash_changes_with_network() {
        let a = network_hash(b"s", &obs(Some("HomeNet"), None, Some("aa:bb:cc:dd:ee:ff")))
            .unwrap()
            .0;
        let b = network_hash(b"s", &obs(Some("HomeNet"), None, Some("aa:bb:cc:dd:ee:00")))
            .unwrap()
            .0;
        assert_ne!(
            a, b,
            "same SSID behind a different gateway is a different network"
        );
    }

    #[test]
    fn anchors_degrade() {
        assert_eq!(
            obs(Some("x"), Some("aa:bb:cc:dd:ee:01"), None)
                .anchor()
                .unwrap()
                .0,
            "ssid+bssid"
        );
        assert_eq!(
            obs(None, None, Some("aa:bb:cc:dd:ee:01"))
                .anchor()
                .unwrap()
                .0,
            "gateway_mac"
        );
        assert!(obs(None, None, None).anchor().is_none());
    }

    #[test]
    fn termux_json() {
        let j = r#"{"bssid":"AA:BB:CC:DD:EE:FF","ssid":"HomeNet","ip":"192.168.1.23","link_speed_mbps":433}"#;
        let (s, b, ip) = parse_termux(j);
        assert_eq!(s.as_deref(), Some("HomeNet"));
        assert_eq!(b.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(ip, Some(Ipv4Addr::new(192, 168, 1, 23)));
        let denied = r#"{"bssid":"02:00:00:00:00:00","ssid":"<unknown ssid>"}"#;
        assert_eq!(parse_termux(denied), (None, None, None));
    }

    #[test]
    fn iw_and_nmcli() {
        let iw = "Connected to aa:bb:cc:dd:ee:ff (on wlan0)\n\tSSID: HomeNet\n\tfreq: 5180\n";
        assert_eq!(
            parse_iw_link(iw),
            (Some("HomeNet".into()), Some("aa:bb:cc:dd:ee:ff".into()))
        );
        let nm =
            "no:Other:11\\:22\\:33\\:44\\:55\\:66\nyes:Home\\:Net:AA\\:BB\\:CC\\:DD\\:EE\\:FF\n";
        assert_eq!(
            parse_nmcli(nm),
            (Some("Home:Net".into()), Some("aa:bb:cc:dd:ee:ff".into()))
        );
    }

    #[test]
    fn ip_route() {
        let (gw, dev) = parse_ip_route(
            "192.0.2.1 via 192.168.1.1 dev wlan0 src 192.168.1.23 uid 10234\n    cache\n",
        );
        assert_eq!(gw, Some(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(dev.as_deref(), Some("wlan0"));
    }
}
