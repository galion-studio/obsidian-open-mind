// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Store <-> JSON by hand (strict: exact field sets, exact types). Done
//! without serde derive on purpose: it keeps hermes-gateway under the 2 MiB
//! release budget (scripts/size-budget.sh), and every reader stays explicit.

use super::{Credential, Device, Flag, Outcome, Pending, State};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

type R<T> = Result<T, String>;

fn obj<'a>(v: &'a Value, what: &str, keys: &[&str]) -> R<&'a Map<String, Value>> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("{what}: not an object"))?;
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(format!("{what}: unknown field `{k}`"));
    }
    if let Some(k) = keys.iter().find(|k| !o.contains_key(**k)) {
        return Err(format!("{what}: missing field `{k}`"));
    }
    Ok(o)
}

fn st(o: &Map<String, Value>, k: &str) -> R<String> {
    o[k].as_str()
        .map(String::from)
        .ok_or_else(|| format!("`{k}` is not a string"))
}

fn ost(o: &Map<String, Value>, k: &str) -> R<Option<String>> {
    match &o[k] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(format!("`{k}` is not a string or null")),
    }
}

fn int(o: &Map<String, Value>, k: &str) -> R<i64> {
    o[k].as_i64()
        .ok_or_else(|| format!("`{k}` is not an integer"))
}

fn uint(o: &Map<String, Value>, k: &str) -> R<u64> {
    o[k].as_u64()
        .ok_or_else(|| format!("`{k}` is not an unsigned integer"))
}

fn strs(o: &Map<String, Value>, k: &str) -> R<Vec<String>> {
    o[k].as_array()
        .and_then(|a| a.iter().map(|x| x.as_str().map(String::from)).collect())
        .ok_or_else(|| format!("`{k}` is not a list of strings"))
}

const PENDING: [&str; 13] = [
    "code",
    "device_id",
    "public_key",
    "fingerprint",
    "capabilities",
    "network_hash",
    "arch",
    "cores",
    "ram_mb",
    "claimed_platform",
    "claimed_network_trusted",
    "requested_at",
    "requested_unix",
];
const CREDENTIAL: [&str; 4] = ["token_sha256", "issued_at", "expires_unix", "expires"];
const DEVICE: [&str; 14] = [
    "device_id",
    "public_key",
    "fingerprint",
    "scopes",
    "claimed_capabilities",
    "arch",
    "cores",
    "ram_mb",
    "network_hash",
    "approved_at",
    "approved_by",
    "credential",
    "revoked_at",
    "revoked_by",
];
const FLAG: [&str; 3] = ["at", "device_id", "reason"];
const STATE: [&str; 6] = ["v", "nonces", "pending", "devices", "flags", "outcomes"];
const OUTCOME: [&str; 6] = ["device_id", "public_key", "fingerprint", "status", "at", "until_unix"];

/// Like [`obj`], but the fields in `optional` may be missing (stores written
/// by an older build). Unknown fields are still refused.
fn obj_opt<'a>(
    v: &'a Value,
    what: &str,
    keys: &[&str],
    optional: &[&str],
) -> R<&'a Map<String, Value>> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("{what}: not an object"))?;
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(format!("{what}: unknown field `{k}`"));
    }
    if let Some(k) = keys
        .iter()
        .find(|k| !o.contains_key(**k) && !optional.contains(k))
    {
        return Err(format!("{what}: missing field `{k}`"));
    }
    Ok(o)
}

impl Pending {
    pub fn to_json(&self) -> Value {
        json!({
            "device_id": self.device_id, "public_key": self.public_key, "fingerprint": self.fingerprint,
            "capabilities": self.capabilities, "network_hash": self.network_hash, "arch": self.arch,
            "cores": self.cores, "ram_mb": self.ram_mb, "claimed_platform": self.claimed_platform,
            "claimed_network_trusted": self.claimed_network_trusted,
            "requested_at": self.requested_at, "requested_unix": self.requested_unix,
            "code": self.code,
        })
    }
    fn from_json(v: &Value) -> R<Pending> {
        let o = obj_opt(v, "pending", &PENDING, &["code"])?;
        Ok(Pending {
            device_id: st(o, "device_id")?,
            public_key: st(o, "public_key")?,
            fingerprint: st(o, "fingerprint")?,
            capabilities: strs(o, "capabilities")?,
            network_hash: ost(o, "network_hash")?,
            arch: st(o, "arch")?,
            cores: uint(o, "cores")?,
            ram_mb: uint(o, "ram_mb")?,
            claimed_platform: st(o, "claimed_platform")?,
            claimed_network_trusted: o["claimed_network_trusted"]
                .as_bool()
                .ok_or("`claimed_network_trusted` is not a bool")?,
            requested_at: st(o, "requested_at")?,
            requested_unix: int(o, "requested_unix")?,
            code: match o.get("code") {
                None => String::new(),
                Some(_) => st(o, "code")?,
            },
        })
    }
}

impl Credential {
    fn to_json(&self) -> Value {
        json!({"token_sha256": self.token_sha256, "issued_at": self.issued_at,
               "expires_unix": self.expires_unix, "expires": self.expires})
    }
    fn from_json(v: &Value) -> R<Credential> {
        let o = obj(v, "credential", &CREDENTIAL)?;
        Ok(Credential {
            token_sha256: st(o, "token_sha256")?,
            issued_at: st(o, "issued_at")?,
            expires_unix: int(o, "expires_unix")?,
            expires: st(o, "expires")?,
        })
    }
}

impl Device {
    fn to_json(&self) -> Value {
        json!({
            "device_id": self.device_id, "public_key": self.public_key, "fingerprint": self.fingerprint,
            "scopes": self.scopes, "claimed_capabilities": self.claimed_capabilities, "arch": self.arch,
            "cores": self.cores, "ram_mb": self.ram_mb, "network_hash": self.network_hash,
            "approved_at": self.approved_at, "approved_by": self.approved_by,
            "credential": self.credential.as_ref().map(Credential::to_json),
            "revoked_at": self.revoked_at, "revoked_by": self.revoked_by,
        })
    }
    fn from_json(v: &Value) -> R<Device> {
        let o = obj(v, "device", &DEVICE)?;
        Ok(Device {
            device_id: st(o, "device_id")?,
            public_key: st(o, "public_key")?,
            fingerprint: st(o, "fingerprint")?,
            scopes: strs(o, "scopes")?,
            claimed_capabilities: strs(o, "claimed_capabilities")?,
            arch: st(o, "arch")?,
            cores: uint(o, "cores")?,
            ram_mb: uint(o, "ram_mb")?,
            network_hash: ost(o, "network_hash")?,
            approved_at: st(o, "approved_at")?,
            approved_by: st(o, "approved_by")?,
            credential: match &o["credential"] {
                Value::Null => None,
                c => Some(Credential::from_json(c)?),
            },
            revoked_at: ost(o, "revoked_at")?,
            revoked_by: ost(o, "revoked_by")?,
        })
    }
}

impl Flag {
    pub fn to_json(&self) -> Value {
        json!({"at": self.at, "device_id": self.device_id, "reason": self.reason})
    }
    fn from_json(v: &Value) -> R<Flag> {
        let o = obj(v, "flag", &FLAG)?;
        Ok(Flag {
            at: st(o, "at")?,
            device_id: st(o, "device_id")?,
            reason: st(o, "reason")?,
        })
    }
}

impl State {
    pub fn to_json(&self) -> Value {
        let devices: Map<String, Value> = self
            .devices
            .iter()
            .map(|(k, d)| (k.clone(), d.to_json()))
            .collect();
        json!({
            "v": self.v, "nonces": self.nonces,
            "pending": self.pending.iter().map(Pending::to_json).collect::<Vec<_>>(),
            "devices": devices,
            "flags": self.flags.iter().map(Flag::to_json).collect::<Vec<_>>(),
            "outcomes": self.outcomes.iter().map(Outcome::to_json).collect::<Vec<_>>(),
        })
    }
    pub fn from_json(v: &Value) -> R<State> {
        let o = obj_opt(v, "store", &STATE, &["outcomes"])?;
        let ver = o["v"].as_u64().ok_or("`v` is not a number")?;
        let mut nonces = BTreeMap::new();
        for (k, t) in o["nonces"].as_object().ok_or("`nonces` is not an object")? {
            nonces.insert(k.clone(), t.as_i64().ok_or("nonce time is not an integer")?);
        }
        let list = |k: &str| {
            o[k].as_array()
                .ok_or_else(|| format!("`{k}` is not a list"))
        };
        let mut devices = BTreeMap::new();
        for (k, d) in o["devices"]
            .as_object()
            .ok_or("`devices` is not an object")?
        {
            let d = Device::from_json(d)?;
            if &d.device_id != k {
                return Err(format!("device key {k} does not match its device_id"));
            }
            devices.insert(k.clone(), d);
        }
        Ok(State {
            v: u32::try_from(ver).map_err(|_| "`v` out of range")?,
            nonces,
            pending: list("pending")?
                .iter()
                .map(Pending::from_json)
                .collect::<R<_>>()?,
            devices,
            flags: list("flags")?
                .iter()
                .map(Flag::from_json)
                .collect::<R<_>>()?,
            outcomes: match o.get("outcomes") {
                None => Vec::new(),
                Some(_) => list("outcomes")?
                    .iter()
                    .map(Outcome::from_json)
                    .collect::<R<_>>()?,
            },
        })
    }
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        json!({"device_id": self.device_id, "public_key": self.public_key, "fingerprint": self.fingerprint,
               "status": self.status, "at": self.at, "until_unix": self.until_unix})
    }
    fn from_json(v: &Value) -> R<Outcome> {
        let o = obj(v, "outcome", &OUTCOME)?;
        let status = st(o, "status")?;
        if !matches!(status.as_str(), "rejected" | "expired") {
            return Err(format!("outcome: unknown status `{status}`"));
        }
        Ok(Outcome {
            device_id: st(o, "device_id")?,
            public_key: st(o, "public_key")?,
            fingerprint: st(o, "fingerprint")?,
            status,
            at: st(o, "at")?,
            until_unix: int(o, "until_unix")?,
        })
    }
}
