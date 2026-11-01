// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The device list behind `/instances`.
//!
//! - The owner asks for a code ([`Fleet::mint_code`]); it lives in memory
//!   only (a restart voids it), 10 minutes, single use, at most
//!   [`MAX_PENDING`] at a time.
//! - A device joins with the code ([`Fleet::join`]) and gets a token it
//!   keeps; it then reports a reading every 30 s ([`Fleet::beat`]).
//! - Failed joins are counted: after [`MAX_FAILS`] in [`FAIL_WINDOW`] every
//!   join is refused until the window passes (codes stay valid).
//! - The list is saved to `DIR/fleet.json` (0600, write-then-rename) with
//!   token hashes only. Forgetting a device marks it removed; its token
//!   stops working at once. Nothing is ever deleted from disk.

use crate::code;
use crate::metrics::{clean, Metrics};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How long a pairing code works.
pub const CODE_TTL: u64 = 10 * 60;
/// Codes waiting at once.
pub const MAX_PENDING: usize = 5;
/// Failed joins allowed per window.
pub const MAX_FAILS: u32 = 20;
/// The failed-join window.
pub const FAIL_WINDOW: u64 = 10 * 60;
/// Seconds between device reports (sent to the device).
pub const BEAT_EVERY: u64 = 30;
/// A device is "online" if it reported within this many seconds.
pub const ONLINE_FOR: u64 = 3 * BEAT_EVERY;
/// Devices kept (removed ones included).
pub const MAX_DEVICES: usize = 200;
/// File name under the state directory.
pub const FILE: &str = "fleet.json";

#[derive(Debug, Clone)]
struct Pending {
    hash: String,
    expires: u64,
    label: String,
}

/// One device.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// "agent" (galion-join) or "browser" (/join page).
    pub via: String,
    pub added: u64,
    pub seen: u64,
    pub addr: String,
    pub lan: bool,
    pub removed: bool,
    pub metrics: Metrics,
    token_hash: String,
}

#[derive(Debug, Default)]
struct Inner {
    fleet_id: String,
    pending: Vec<Pending>,
    devices: Vec<Device>,
    fails: u32,
    fail_since: u64,
}

/// Why a join or beat was refused (plain words for the device).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    BadCode,
    TooMany,
    Unknown,
    Full,
    NoRandom,
}

impl Refused {
    pub fn status(&self) -> u16 {
        match self {
            Refused::BadCode | Refused::Unknown => 401,
            Refused::TooMany => 429,
            Refused::Full => 409,
            Refused::NoRandom => 500,
        }
    }
    pub fn words(&self) -> &'static str {
        match self {
            Refused::BadCode => "That code is wrong, used, or older than 10 minutes. Ask for a new one on the Instances page.",
            Refused::TooMany => "Too many wrong codes. Wait 10 minutes and try again.",
            Refused::Unknown => "This device was removed from the console. Join again with a new code.",
            Refused::Full => "The console has too many devices. Remove one first.",
            Refused::NoRandom => "The console could not make a secure code.",
        }
    }
}

/// What a successful join gives the device.
#[derive(Debug, Clone)]
pub struct Joined {
    pub id: String,
    pub token: String,
    pub name: String,
    pub fleet_id: String,
}

/// The device list (thread safe).
#[derive(Debug)]
pub struct Fleet {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
}

/// Is `ip` a private / link-local / loopback address (same network)?
pub fn is_lan(ip: &str) -> bool {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v)) => v.is_private() || v.is_link_local() || v.is_loopback(),
        Ok(std::net::IpAddr::V6(v)) => {
            v.is_loopback()
                || (v.segments()[0] & 0xfe00) == 0xfc00
                || (v.segments()[0] & 0xffc0) == 0xfe80
        }
        Err(_) => false,
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn device_json(d: &Device, full: bool) -> Value {
    let mut v = json!({
        "id": d.id, "name": d.name, "via": d.via, "added": d.added, "seen": d.seen,
        "lan": d.lan, "removed": d.removed, "metrics": d.metrics.to_json(),
    });
    if full {
        v["token_hash"] = json!(d.token_hash);
        v["addr"] = json!(d.addr);
    }
    v
}

fn device_from(v: &Value) -> Option<Device> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    let id = s("id");
    let token_hash = s("token_hash");
    (!id.is_empty() && token_hash.len() == 64).then(|| Device {
        name: s("name"),
        via: s("via"),
        added: n("added"),
        seen: n("seen"),
        addr: s("addr"),
        lan: v.get("lan").and_then(Value::as_bool).unwrap_or(false),
        removed: v.get("removed").and_then(Value::as_bool).unwrap_or(false),
        metrics: Metrics::from_json(v.get("metrics").unwrap_or(&Value::Null)),
        id,
        token_hash,
    })
}

/// A short name from what the owner or the device said.
pub fn clean_name(s: &str) -> String {
    clean(s, 40)
}

impl Fleet {
    /// In memory only (tests, or no state dir).
    pub fn memory() -> Fleet {
        Fleet {
            path: None,
            inner: Mutex::new(Inner {
                fleet_id: code::new_token()
                    .map(|t| t[..16].to_string())
                    .unwrap_or_default(),
                ..Inner::default()
            }),
        }
    }

    /// Load `dir/fleet.json` (a missing file is an empty list).
    pub fn open(dir: &Path) -> std::io::Result<Fleet> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE);
        let mut inner = Inner::default();
        match std::fs::read(&path) {
            Ok(b) => {
                let v: Value = serde_json::from_slice(&b).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{}: {e}", path.display()),
                    )
                })?;
                inner.fleet_id = v
                    .get("fleet_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                inner.devices = v
                    .get("devices")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(device_from).collect())
                    .unwrap_or_default();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if inner.fleet_id.is_empty() {
            inner.fleet_id = code::new_token()
                .map(|t| t[..16].to_string())
                .ok_or_else(|| std::io::Error::other("no /dev/urandom"))?;
        }
        let f = Fleet {
            path: Some(path),
            inner: Mutex::new(inner),
        };
        f.save(&f.lock())?;
        Ok(f)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save(&self, inner: &Inner) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let body = json!({
            "fleet_id": inner.fleet_id,
            "devices": inner.devices.iter().map(|d| device_json(d, true)).collect::<Vec<_>>(),
        });
        let tmp = path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
            let mut f = o.open(&tmp)?;
            f.write_all(
                serde_json::to_string_pretty(&body)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    }

    /// This console's fleet id (not secret: devices use it to recognise
    /// the console on the LAN).
    pub fn fleet_id(&self) -> String {
        self.lock().fleet_id.clone()
    }

    /// A new pairing code (shown once). `label` is the name the owner
    /// typed for the device, if any.
    pub fn mint_code(&self, label: &str, now: u64) -> Result<String, Refused> {
        let c = code::new_code().ok_or(Refused::NoRandom)?;
        let mut g = self.lock();
        g.pending.retain(|p| p.expires > now);
        if g.pending.len() >= MAX_PENDING {
            g.pending.remove(0);
        }
        g.pending.push(Pending {
            hash: code::hash(&code::normalize(&c).unwrap_or_default()),
            expires: now + CODE_TTL,
            label: clean_name(label),
        });
        Ok(c)
    }

    /// Codes still waiting (count only).
    pub fn pending(&self, now: u64) -> usize {
        self.lock()
            .pending
            .iter()
            .filter(|p| p.expires > now)
            .count()
    }

    /// A device joins with a code.
    pub fn join(
        &self,
        typed: &str,
        name: &str,
        via: &str,
        m: Metrics,
        ip: &str,
        now: u64,
    ) -> Result<Joined, Refused> {
        let mut g = self.lock();
        if now.saturating_sub(g.fail_since) > FAIL_WINDOW {
            g.fails = 0;
            g.fail_since = now;
        }
        if g.fails >= MAX_FAILS {
            return Err(Refused::TooMany);
        }
        g.pending.retain(|p| p.expires > now);
        let h = code::normalize(typed).map(|c| code::hash(&c));
        let at = h.and_then(|h| g.pending.iter().position(|p| code::same(&p.hash, &h)));
        let Some(at) = at else {
            g.fails += 1;
            return Err(Refused::BadCode);
        };
        if g.devices.len() >= MAX_DEVICES {
            return Err(Refused::Full);
        }
        let token = code::new_token().ok_or(Refused::NoRandom)?;
        let id = code::new_token().ok_or(Refused::NoRandom)?[..12].to_string();
        let p = g.pending.remove(at);
        let mut name = if p.label.is_empty() {
            clean_name(name)
        } else {
            p.label
        };
        if name.is_empty() {
            name = if m.hostname.is_empty() {
                "New device".into()
            } else {
                clean_name(&m.hostname)
            };
        }
        let d = Device {
            id: id.clone(),
            name: name.clone(),
            via: if via == "browser" { "browser" } else { "agent" }.into(),
            added: now,
            seen: now,
            addr: ip.to_string(),
            lan: is_lan(ip),
            removed: false,
            metrics: m,
            token_hash: code::hash(&token),
        };
        g.devices.push(d);
        let _ = self.save(&g);
        Ok(Joined {
            id,
            token,
            name,
            fleet_id: g.fleet_id.clone(),
        })
    }

    fn find(g: &Inner, token: &str) -> Option<usize> {
        let h = code::hash(token);
        g.devices.iter().position(|d| code::same(&d.token_hash, &h))
    }

    /// The name of the (not removed) device holding `token`, without
    /// counting it as a report.
    pub fn name_for_token(&self, token: &str) -> Option<String> {
        let g = self.lock();
        let i = Self::find(&g, token)?;
        (!g.devices[i].removed).then(|| g.devices[i].name.clone())
    }

    /// A device reports. Saved to disk at most once a minute per device
    /// (memory always has the latest).
    pub fn beat(
        &self,
        token: &str,
        m: Metrics,
        ip: &str,
        now: u64,
    ) -> Result<(String, String), Refused> {
        let mut g = self.lock();
        let i = Self::find(&g, token).ok_or(Refused::Unknown)?;
        if g.devices[i].removed {
            return Err(Refused::Unknown);
        }
        let d = &mut g.devices[i];
        let save = now.saturating_sub(d.seen) >= 60 || d.addr != ip;
        d.seen = now;
        d.addr = ip.to_string();
        d.lan = is_lan(ip);
        d.metrics = m;
        let out = (d.id.clone(), d.name.clone());
        if save {
            let _ = self.save(&g);
        }
        Ok(out)
    }

    /// Mark a device removed (its token stops working). Returns its name.
    pub fn forget(&self, id: &str) -> Option<String> {
        let mut g = self.lock();
        let d = g.devices.iter_mut().find(|d| d.id == id && !d.removed)?;
        d.removed = true;
        let name = d.name.clone();
        let _ = self.save(&g);
        Some(name)
    }

    /// Rename a device. Returns the new name.
    pub fn rename(&self, id: &str, name: &str) -> Option<String> {
        let name = clean_name(name);
        if name.is_empty() {
            return None;
        }
        let mut g = self.lock();
        let d = g.devices.iter_mut().find(|d| d.id == id && !d.removed)?;
        d.name = name.clone();
        let _ = self.save(&g);
        Some(name)
    }

    /// Devices for the page / owner API: no tokens, no addresses beyond
    /// "same network or not", newest first, removed ones last.
    pub fn snapshot(&self, now: u64) -> Value {
        let g = self.lock();
        let mut list: Vec<&Device> = g.devices.iter().collect();
        list.sort_by(|a, b| a.removed.cmp(&b.removed).then(b.seen.cmp(&a.seen)));
        let devices: Vec<Value> = list
            .iter()
            .map(|d| {
                let mut v = device_json(d, false);
                let ago = now.saturating_sub(d.seen);
                v["online"] = json!(!d.removed && ago <= ONLINE_FOR);
                v["seen_ago"] = json!(ago);
                v
            })
            .collect();
        json!({
            "fleet_id": g.fleet_id,
            "now": now,
            "pending_codes": g.pending.iter().filter(|p| p.expires > now).count(),
            "devices": devices,
        })
    }

    /// Wall clock seconds (exposed so callers share one clock).
    pub fn now() -> u64 {
        now_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(host: &str) -> Metrics {
        Metrics {
            hostname: host.into(),
            cores: Some(8),
            ..Metrics::default()
        }
    }

    #[test]
    fn code_is_single_use_and_expires() {
        let f = Fleet::memory();
        let c = f.mint_code("", 1000).unwrap();
        assert_eq!(f.pending(1000), 1);
        let j = f
            .join(
                &c.to_lowercase(),
                "",
                "agent",
                m("studio"),
                "192.168.1.5",
                1001,
            )
            .unwrap();
        assert_eq!(j.name, "studio");
        assert_eq!(j.token.len(), 64);
        assert_eq!(
            f.join(&c, "", "agent", m("x"), "1.2.3.4", 1002)
                .unwrap_err(),
            Refused::BadCode
        );
        let c2 = f.mint_code("Mate's Mac", 2000).unwrap();
        assert_eq!(
            f.join(&c2, "", "agent", m("x"), "1.2.3.4", 2000 + CODE_TTL + 1)
                .unwrap_err(),
            Refused::BadCode
        );
        let snap = f.snapshot(1001);
        assert_eq!(snap["devices"][0]["lan"], true);
        assert_eq!(snap["devices"][0]["online"], true);
        let text = snap.to_string();
        assert!(!text.contains(&j.token));
        assert!(!text.contains("token_hash"));
        assert!(!text.contains("192.168.1.5"));
    }

    #[test]
    fn owner_label_wins_and_beat_updates() {
        let f = Fleet::memory();
        let c = f.mint_code("  Work laptop ", 10).unwrap();
        let j = f
            .join(&c, "ignored", "browser", m("h"), "8.8.8.8", 10)
            .unwrap();
        assert_eq!(j.name, "Work laptop");
        let mut r = m("h");
        r.cpu_pct = Some(42.0);
        assert!(f.beat(&j.token, r, "10.0.0.2", 40).is_ok());
        let s = f.snapshot(40);
        assert_eq!(s["devices"][0]["metrics"]["cpu_pct"], 42.0);
        assert_eq!(s["devices"][0]["via"], "browser");
        assert_eq!(s["devices"][0]["lan"], true);
        assert_eq!(
            f.snapshot(40 + ONLINE_FOR + 1)["devices"][0]["online"],
            false
        );
        assert_eq!(
            f.beat("nope", m("h"), "x", 41).unwrap_err(),
            Refused::Unknown
        );
        assert_eq!(f.rename(&j.id, "Desk PC").as_deref(), Some("Desk PC"));
        assert_eq!(f.forget(&j.id).as_deref(), Some("Desk PC"));
        assert_eq!(
            f.beat(&j.token, m("h"), "x", 42).unwrap_err(),
            Refused::Unknown
        );
        assert_eq!(f.forget(&j.id), None);
    }

    #[test]
    fn wrong_codes_are_rate_limited() {
        let f = Fleet::memory();
        let c = f.mint_code("", 0).unwrap();
        for _ in 0..MAX_FAILS {
            assert_eq!(
                f.join("AAAA-AAAA", "", "agent", m("x"), "x", 5)
                    .unwrap_err(),
                Refused::BadCode
            );
        }
        assert_eq!(
            f.join(&c, "", "agent", m("x"), "x", 6).unwrap_err(),
            Refused::TooMany
        );
        // After the window the right code still works (if not expired).
        let c = f.mint_code("", FAIL_WINDOW + 10).unwrap();
        assert!(f
            .join(&c, "", "agent", m("x"), "x", FAIL_WINDOW + 10)
            .is_ok());
    }

    #[test]
    fn pending_is_capped() {
        let f = Fleet::memory();
        let first = f.mint_code("", 0).unwrap();
        for _ in 0..MAX_PENDING {
            f.mint_code("", 0).unwrap();
        }
        assert_eq!(f.pending(0), MAX_PENDING);
        assert_eq!(
            f.join(&first, "", "agent", m("x"), "x", 1).unwrap_err(),
            Refused::BadCode
        );
    }

    #[test]
    fn saved_and_loaded_without_secrets() {
        let dir = std::env::temp_dir().join(format!("galion-fleet-test-{}", std::process::id()));
        let f = Fleet::open(&dir).unwrap();
        let id0 = f.fleet_id();
        let c = f.mint_code("", 0).unwrap();
        let j = f
            .join(&c, "lap", "agent", m("lap"), "192.168.0.9", 0)
            .unwrap();
        let raw = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!raw.contains(&j.token));
        assert!(!raw.contains(&c));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let g = Fleet::open(&dir).unwrap();
        assert_eq!(g.fleet_id(), id0);
        assert!(g.beat(&j.token, m("lap"), "192.168.0.9", 5).is_ok());
        assert_eq!(g.pending(0), 0, "codes do not survive a restart");
    }

    #[test]
    fn lan_ranges() {
        assert!(is_lan("192.168.1.2"));
        assert!(is_lan("10.1.2.3"));
        assert!(is_lan("172.16.0.1"));
        assert!(is_lan("127.0.0.1"));
        assert!(is_lan("fe80::1"));
        assert!(is_lan("fd00::1"));
        assert!(!is_lan("8.8.8.8"));
        assert!(!is_lan("2001:db8::1"));
        assert!(!is_lan("garbage"));
    }
}
