// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Turn discovery candidates into OpenMind provider rows and keep the
//! approvals log.
//!
//! - [`store_found`]: every new device becomes a [`ProviderKind::Lan`] row in
//!   [`Approval::Pending`]. A row that already exists keeps its approval (a
//!   denied device stays denied when it is seen again).
//! - [`decide`]: `approve` / `deny` set [`Approval::Approved`] /
//!   [`Approval::Rejected`]. This is the only way a device is admitted.
//! - [`join`]: approved rows only; one TCP connect to a local endpoint,
//!   recorded as health up/down.
//!
//! Every event is one JSON line in the approvals log, stamped with a
//! Europe/Zagreb RFC 3339 time with an explicit offset (never `Z`). The log
//! and the registry hold endpoints and approval state, never secrets.

use crate::{Candidate, DeviceKind, Origin};
use openmind_provider::{
    Approval, Capability, ComputeProvider, Health, ProviderError, ProviderKind, ProviderRecord,
    Registry, StaticProvider,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Stable string for an approval state, as in the registry JSON.
pub fn approval_str(approval: Approval) -> &'static str {
    match approval {
        Approval::Pending => "pending",
        Approval::Approved => "approved",
        Approval::Rejected => "rejected",
    }
}

/// Stable string for a health snapshot.
pub fn health_str(health: Health) -> &'static str {
    match health {
        Health::Unknown => "unknown",
        Health::Up => "up",
        Health::Down => "down",
    }
}

/// Provider id for a candidate: `<kind>-<candidate id>`, `[A-Za-z0-9._-]`,
/// at most 64 bytes. `None` when the candidate has no known kind.
pub fn provider_id(c: &Candidate) -> Option<String> {
    let kind = c.kind()?.as_str();
    let raw = if c.id.starts_with(&format!("{kind}-")) {
        c.id.clone()
    } else {
        format!("{kind}-{}", c.id)
    };
    let mut id: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    id.truncate(64);
    Some(id)
}

/// Endpoint URL for a candidate. Prefers an IPv4 address.
pub fn endpoint(c: &Candidate) -> Option<String> {
    let kind = c.kind()?;
    let port = c.port.filter(|p| *p != 0)?;
    let addr: IpAddr = c
        .addrs
        .iter()
        .filter_map(|a| a.parse::<IpAddr>().ok())
        .min_by_key(|ip| !ip.is_ipv4())?;
    Some(match addr {
        IpAddr::V4(ip) => format!("{}://{ip}:{port}", kind.scheme()),
        IpAddr::V6(ip) => format!("{}://[{ip}]:{port}", kind.scheme()),
    })
}

fn capabilities(c: &Candidate, kind: DeviceKind) -> Vec<Capability> {
    let mut out = Vec::new();
    if let Some(caps) = c.txt.get("caps") {
        for cap in caps.split(',') {
            let cap = match cap.trim() {
                "llm" => Capability::Llm,
                "stt" => Capability::Stt,
                "tts" => Capability::Tts,
                "vision" => Capability::Vision,
                _ => continue,
            };
            if !out.contains(&cap) {
                out.push(cap);
            }
        }
    }
    if out.is_empty()
        && matches!(
            kind,
            DeviceKind::Exo | DeviceKind::Ollama | DeviceKind::LlamaCpp | DeviceKind::LlamaRpc
        )
    {
        out.push(Capability::Llm);
    }
    out
}

/// VRAM (GB) a candidate advertises in TXT `vram` or `vram_gb`, 1..=4096.
pub fn vram_gb(c: &Candidate) -> Option<u32> {
    c.txt
        .get("vram")
        .or_else(|| c.txt.get("vram_gb"))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|gb| (1..=4096).contains(gb))
}

/// True for loopback, RFC 1918, link-local and IPv6 ULA / link-local.
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            v6.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    }
}

/// Append-only JSON-lines approvals log.
#[derive(Debug, Clone)]
pub struct ApprovalsLog {
    path: PathBuf,
}

impl ApprovalsLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event. `fields` must be a JSON object; `ts` and `event`
    /// are added first. Returns the line written.
    pub fn append(&self, event: &str, fields: Value) -> io::Result<Value> {
        let mut line = serde_json::Map::new();
        line.insert("ts".into(), Value::String(zagreb_time::now_rfc3339()));
        line.insert("event".into(), Value::String(event.into()));
        if let Value::Object(map) = fields {
            for (k, v) in map {
                line.insert(k, v);
            }
        }
        let line = Value::Object(line);
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut text = serde_json::to_string(&line).map_err(io::Error::other)?;
        text.push('\n');
        file.write_all(text.as_bytes())?;
        Ok(line)
    }

    /// Every line, parsed. Unparseable lines are skipped.
    pub fn read(&self) -> io::Result<Vec<Value>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(err),
        }
    }
}

/// Failure of an admit step.
#[derive(Debug)]
pub enum AdmitError {
    Registry(ProviderError),
    Log(io::Error),
    /// `join` on a row that is not approved.
    NotApproved {
        id: String,
        approval: Approval,
    },
    /// `join` endpoint is not a local address (or does not parse).
    NotLocal {
        id: String,
        endpoint: String,
    },
}

impl std::fmt::Display for AdmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdmitError::Registry(err) => write!(f, "registry: {err}"),
            AdmitError::Log(err) => write!(f, "approvals log: {err}"),
            AdmitError::NotApproved { id, approval } => write!(
                f,
                "{id} is {}; only an approved device can join (run approve first)",
                approval_str(*approval)
            ),
            AdmitError::NotLocal { id, endpoint } => write!(
                f,
                "{id}: endpoint {endpoint} is not a loopback, private or link-local address; not dialing"
            ),
        }
    }
}

impl std::error::Error for AdmitError {}

impl From<ProviderError> for AdmitError {
    fn from(err: ProviderError) -> Self {
        AdmitError::Registry(err)
    }
}

impl From<io::Error> for AdmitError {
    fn from(err: io::Error) -> Self {
        AdmitError::Log(err)
    }
}

/// What [`store_found`] did with one candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stored {
    pub provider_id: String,
    /// `new` (registered pending now) or `known` (row already there).
    pub status: &'static str,
    pub approval: &'static str,
    pub endpoint: String,
    /// Set when a known row was seen at a different endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen_endpoint: Option<String>,
    pub kind: DeviceKind,
    pub origin: Origin,
    pub service_type: String,
    pub instance: String,
    pub host: String,
    pub verified: Option<bool>,
}

/// A candidate [`store_found`] could not store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub instance: String,
    pub service_type: String,
    pub reason: String,
}

/// Store every candidate as a pending provider row (new) or report the
/// existing row (known). `dry_run` writes nothing.
pub fn store_found(
    registry: &mut Registry,
    log: &ApprovalsLog,
    candidates: &[Candidate],
    by: &str,
    dry_run: bool,
) -> Result<(Vec<Stored>, Vec<Skipped>), AdmitError> {
    let mut stored: Vec<Stored> = Vec::new();
    let mut skipped = Vec::new();
    for c in candidates {
        let skip = |reason: &str| Skipped {
            instance: c.instance.clone(),
            service_type: c.service_type.clone(),
            reason: reason.into(),
        };
        let Some(kind) = c.kind() else {
            skipped.push(skip("not a discovery service type"));
            continue;
        };
        let (Some(id), Some(ep)) = (provider_id(c), endpoint(c)) else {
            skipped.push(skip("no address or port"));
            continue;
        };
        // The same endpoint found another way (mDNS vs probe) is the same device.
        let id = registry
            .providers()
            .iter()
            .find(|r| r.endpoint() == ep)
            .map(|r| r.id().to_string())
            .unwrap_or(id);
        if stored.iter().any(|s| s.provider_id == id) {
            continue;
        }
        let verified = c.txt.get("verified").map(|v| v == "true");
        let mut row = Stored {
            provider_id: id.clone(),
            status: "known",
            approval: "pending",
            endpoint: ep.clone(),
            seen_endpoint: None,
            kind,
            origin: c.origin,
            service_type: c.service_type.clone(),
            instance: c.instance.clone(),
            host: c.host.clone(),
            verified,
        };
        let details = json!({
            "id": id,
            "endpoint": ep,
            "kind": kind.as_str(),
            "origin": c.origin,
            "service_type": c.service_type,
            "instance": c.instance,
            "by": by,
        });
        if let Some(existing) = registry.get(&id) {
            row.approval = approval_str(existing.approval());
            if existing.endpoint() != ep {
                row.endpoint = existing.endpoint().to_string();
                row.seen_endpoint = Some(ep.clone());
            }
            if !dry_run {
                // A move is only noted; the routed endpoint changes on re-approval.
                let changed = registry.note_endpoint(&id, &ep)?;
                let mut details = details;
                details["approval"] = json!(row.approval);
                if let Some(seen) = &row.seen_endpoint {
                    details["endpoint"] = json!(row.endpoint);
                    details["seen_endpoint"] = json!(seen);
                    if changed {
                        let mut moved = details.clone();
                        moved["needs"] =
                            json!("re-approval (approve ID) before the new endpoint is used");
                        log.append("moved", moved)?;
                    }
                }
                log.append("seen", details)?;
            }
        } else {
            row.status = "new";
            if !dry_run {
                let health = match verified {
                    Some(true) => Health::Up,
                    _ => Health::Unknown,
                };
                let provider =
                    StaticProvider::try_new(&id, ProviderKind::Lan, &ep, capabilities(c, kind))?
                        .with_health(health)
                        .with_vram_gb(vram_gb(c));
                registry.register(&provider)?;
                let mut details = details;
                details["approval"] = json!("pending");
                log.append("found", details)?;
            }
        }
        stored.push(row);
    }
    Ok((stored, skipped))
}

/// An admin decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Deny,
}

/// Apply `decision` to row `id` and log it. Returns the log line.
pub fn decide(
    registry: &mut Registry,
    log: &ApprovalsLog,
    id: &str,
    decision: Decision,
    by: &str,
    note: Option<&str>,
) -> Result<Value, AdmitError> {
    let (event, to) = match decision {
        Decision::Approve => ("approve", Approval::Approved),
        Decision::Deny => ("deny", Approval::Rejected),
    };
    let row = registry
        .get(id)
        .ok_or_else(|| ProviderError::NotFound { id: id.into() })?;
    let from = row.approval();
    let old_endpoint = row.endpoint().to_string();
    let pending = row.pending_endpoint().map(str::to_string);
    registry.set_approval(id, to)?;
    let endpoint = registry
        .get(id)
        .map_or(old_endpoint.clone(), |r| r.endpoint().to_string());
    let mut fields = json!({
        "id": id,
        "from": approval_str(from),
        "to": approval_str(to),
        "endpoint": endpoint,
        "by": by,
    });
    if endpoint != old_endpoint {
        fields["moved_from"] = json!(old_endpoint);
        fields["health"] = json!("unknown");
        fields["needs"] = json!("join");
    } else if let Some(p) = pending {
        fields["pending_endpoint"] = json!(p);
    }
    if let Some(note) = note {
        fields["note"] = json!(note.chars().take(200).collect::<String>());
    }
    Ok(log.append(event, fields)?)
}

/// Host and port of an endpoint like `http://10.0.0.5:11434` or `tcp://[fe80::1]:50052`.
pub fn endpoint_addr(endpoint: &str) -> Option<SocketAddr> {
    let rest = endpoint
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(endpoint);
    let hostport = rest.split('/').next()?;
    let addr: SocketAddr = hostport.parse().ok()?;
    Some(addr)
}

/// `join` an approved row: one TCP connect (no payload) to its local
/// endpoint, recorded as health up/down. Refuses (and logs) anything else.
pub fn join(
    registry: &mut Registry,
    log: &ApprovalsLog,
    id: &str,
    by: &str,
    timeout: Duration,
) -> Result<Value, AdmitError> {
    let row = registry
        .get(id)
        .ok_or_else(|| ProviderError::NotFound { id: id.into() })?;
    let approval = row.approval();
    let endpoint = row.endpoint().to_string();
    if approval != Approval::Approved {
        log.append(
            "join-refused",
            json!({"id": id, "approval": approval_str(approval), "endpoint": endpoint, "by": by}),
        )?;
        return Err(AdmitError::NotApproved {
            id: id.into(),
            approval,
        });
    }
    let addr = endpoint_addr(&endpoint).filter(|a| is_local(a.ip()));
    let Some(addr) = addr else {
        log.append(
            "join-refused",
            json!({"id": id, "approval": "approved", "endpoint": endpoint, "by": by, "reason": "endpoint not local"}),
        )?;
        return Err(AdmitError::NotLocal {
            id: id.into(),
            endpoint,
        });
    };
    let health = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(_) => Health::Up,
        Err(_) => Health::Down,
    };
    registry.set_health(id, health)?;
    Ok(log.append(
        "join",
        json!({"id": id, "endpoint": endpoint, "health": health_str(health), "by": by}),
    )?)
}

/// One provider row as JSON.
pub fn row_json(row: &dyn ComputeProvider, approval: Approval) -> Value {
    let mut v = json!({
        "id": row.id(),
        "endpoint": row.endpoint(),
        "approval": approval_str(approval),
        "health": health_str(row.health()),
        "capabilities": row.capabilities().iter().map(|c| c.as_str()).collect::<Vec<_>>(),
    });
    if let Some(gb) = row.vram_gb() {
        v["vram_gb"] = json!(gb);
    }
    v
}

/// [`row_json`] for a registry row, plus its pending move if any.
pub fn record_json(row: &ProviderRecord) -> Value {
    let mut v = row_json(row, row.approval());
    if let Some(p) = row.pending_endpoint() {
        v["pending_endpoint"] = json!(p);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fold_records, parse_mdns_sd};
    use std::net::TcpListener;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openmind-discovery-admit-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn found(port: u16) -> Vec<Candidate> {
        let text = format!(
            "\
_galion._tcp.local. IN PTR lab._galion._tcp.local.
lab._galion._tcp.local. IN SRV 0 0 {port} lab.local.
lab._galion._tcp.local. IN TXT \"caps=llm,tts\"
_exo._tcp.local. IN PTR lab._exo._tcp.local.
lab._exo._tcp.local. IN SRV 0 0 52415 lab.local.
_ipp._tcp.local. IN PTR printer._ipp._tcp.local.
printer._ipp._tcp.local. IN SRV 0 0 631 printer.local.
_ollama._tcp.local. IN PTR noaddr._ollama._tcp.local.
noaddr._ollama._tcp.local. IN SRV 0 0 11434 nowhere.local.
lab.local. IN A 127.0.0.1
printer.local. IN A 127.0.0.1
"
        );
        fold_records(&parse_mdns_sd(&text).unwrap())
    }

    #[test]
    fn ids_and_endpoints() {
        let rows = found(19111);
        let galion = rows
            .iter()
            .find(|c| c.service_type.starts_with("_galion"))
            .unwrap();
        assert_eq!(provider_id(galion).unwrap(), "galion-lab");
        assert_eq!(endpoint(galion).unwrap(), "http://127.0.0.1:19111");
        assert_eq!(
            capabilities(galion, DeviceKind::Galion),
            vec![Capability::Llm, Capability::Tts]
        );
        assert_eq!(
            endpoint_addr("tcp://[fe80::1]:50052").unwrap().port(),
            50052
        );
        assert!(is_local("10.1.2.3".parse().unwrap()));
        assert!(is_local("fd00::1".parse().unwrap()));
        assert!(!is_local("8.8.8.8".parse().unwrap())); // repo-check: allow (test fixture address, never dialled)
        assert!(!is_local("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn found_pending_denied_approved_joined() {
        let dir = tmpdir("flow");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut reg = Registry::load(dir.join("providers.json")).unwrap();
        let log = ApprovalsLog::new(dir.join("approvals.log"));

        let (stored, skipped) = store_found(&mut reg, &log, &found(port), "test", false).unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored
            .iter()
            .all(|s| s.status == "new" && s.approval == "pending"));
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert_eq!(reg.len(), 2);
        assert!(reg
            .providers()
            .iter()
            .all(|r| r.approval() == Approval::Pending && r.kind() == ProviderKind::Lan));

        // Join is refused while pending.
        let err = join(
            &mut reg,
            &log,
            "galion-lab",
            "test",
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(matches!(err, AdmitError::NotApproved { .. }));

        let line = decide(
            &mut reg,
            &log,
            "galion-lab",
            Decision::Deny,
            "test",
            Some("not yet"),
        )
        .unwrap();
        assert_eq!(line["from"], "pending");
        assert_eq!(line["to"], "rejected");

        // Seen again: stays rejected, no new row.
        let (again, _) = store_found(&mut reg, &log, &found(port), "test", false).unwrap();
        let g = again
            .iter()
            .find(|s| s.provider_id == "galion-lab")
            .unwrap();
        assert_eq!((g.status, g.approval), ("known", "rejected"));
        assert_eq!(reg.len(), 2);

        let line = decide(
            &mut reg,
            &log,
            "galion-lab",
            Decision::Approve,
            "test",
            None,
        )
        .unwrap();
        assert_eq!(line["to"], "approved");
        assert_eq!(
            reg.get("galion-lab").unwrap().approval(),
            Approval::Approved
        );

        let line = join(
            &mut reg,
            &log,
            "galion-lab",
            "test",
            Duration::from_millis(500),
        )
        .unwrap();
        assert_eq!(line["health"], "up");
        drop(listener);

        // Reload from disk: state persisted.
        let reg2 = Registry::load(dir.join("providers.json")).unwrap();
        assert_eq!(
            reg2.get("galion-lab").unwrap().approval(),
            Approval::Approved
        );
        assert_eq!(reg2.get("galion-lab").unwrap().health(), Health::Up);
        assert_eq!(reg2.get("exo-lab").unwrap().approval(), Approval::Pending);

        let events: Vec<String> = log
            .read()
            .unwrap()
            .iter()
            .map(|l| l["event"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            events,
            [
                "found",
                "found",
                "join-refused",
                "deny",
                "seen",
                "seen",
                "approve",
                "join"
            ]
        );
        for line in log.read().unwrap() {
            let ts = line["ts"].as_str().unwrap();
            assert!(zagreb_time::require_rfc3339(ts).is_ok(), "{ts}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_hit_on_a_known_endpoint_is_the_same_device() {
        let dir = tmpdir("same");
        let mut reg = Registry::load(dir.join("providers.json")).unwrap();
        let log = ApprovalsLog::new(dir.join("approvals.log"));
        store_found(&mut reg, &log, &found(19111), "t", false).unwrap();
        let mut txt = std::collections::BTreeMap::new();
        txt.insert("kind".to_string(), "galion".to_string());
        let probe_hit = Candidate {
            id: "galion-127-0-0-1-19111".into(),
            instance: "127.0.0.1:19111".into(),
            service_type: "probe:galion".into(),
            host: "127.0.0.1".into(),
            port: Some(19111),
            addrs: vec!["127.0.0.1".into()],
            txt,
            origin: Origin::Probe,
            approval: crate::APPROVAL.into(),
        };
        let (stored, _) = store_found(&mut reg, &log, &[probe_hit], "t", false).unwrap();
        assert_eq!(stored[0].provider_id, "galion-lab");
        assert_eq!(stored[0].status, "known");
        assert_eq!(reg.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_device_is_routed_at_its_old_endpoint_until_re_approved() {
        let dir = tmpdir("moved");
        let mut reg = Registry::load(dir.join("providers.json")).unwrap();
        let log = ApprovalsLog::new(dir.join("approvals.log"));
        store_found(&mut reg, &log, &found(19111), "t", false).unwrap();
        decide(&mut reg, &log, "galion-lab", Decision::Approve, "t", None).unwrap();

        let (seen, _) = store_found(&mut reg, &log, &found(19115), "t", false).unwrap();
        let g = seen.iter().find(|s| s.provider_id == "galion-lab").unwrap();
        assert_eq!(g.endpoint, "http://127.0.0.1:19111");
        assert_eq!(g.seen_endpoint.as_deref(), Some("http://127.0.0.1:19115"));
        let row = reg.get("galion-lab").unwrap();
        assert_eq!(row.endpoint(), "http://127.0.0.1:19111");
        assert_eq!(row.approval(), Approval::Approved);
        assert_eq!(row.pending_endpoint(), Some("http://127.0.0.1:19115"));
        assert_eq!(
            record_json(row)["pending_endpoint"],
            "http://127.0.0.1:19115"
        );

        let line = decide(&mut reg, &log, "galion-lab", Decision::Approve, "t", None).unwrap();
        assert_eq!(line["moved_from"], "http://127.0.0.1:19111");
        assert_eq!(line["endpoint"], "http://127.0.0.1:19115");
        assert_eq!(line["needs"], "join");
        let row = reg.get("galion-lab").unwrap();
        assert_eq!(row.endpoint(), "http://127.0.0.1:19115");
        assert_eq!(row.health(), Health::Unknown);
        let events: Vec<String> = log
            .read()
            .unwrap()
            .iter()
            .map(|l| l["event"].as_str().unwrap().to_string())
            .collect();
        assert!(events.contains(&"moved".to_string()), "{events:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn txt_vram_is_stored() {
        let dir = tmpdir("vram");
        let mut reg = Registry::load(dir.join("providers.json")).unwrap();
        let log = ApprovalsLog::new(dir.join("approvals.log"));
        let mut c = found(19111);
        for cand in &mut c {
            cand.txt.insert("vram".into(), "24".into());
        }
        store_found(&mut reg, &log, &c, "t", false).unwrap();
        assert_eq!(reg.get("galion-lab").unwrap().vram_gb(), Some(24));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let dir = tmpdir("dry");
        let mut reg = Registry::load(dir.join("providers.json")).unwrap();
        let log = ApprovalsLog::new(dir.join("approvals.log"));
        let (stored, _) = store_found(&mut reg, &log, &found(19111), "test", true).unwrap();
        assert_eq!(stored.len(), 2);
        assert!(reg.is_empty());
        assert!(!dir.join("providers.json").exists());
        assert!(!dir.join("approvals.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_id_is_an_error() {
        let dir = tmpdir("unknown");
        let mut reg = Registry::load(dir.join("providers.json")).unwrap();
        let log = ApprovalsLog::new(dir.join("approvals.log"));
        let err = decide(&mut reg, &log, "nope", Decision::Approve, "t", None).unwrap_err();
        assert!(matches!(
            err,
            AdmitError::Registry(ProviderError::NotFound { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
