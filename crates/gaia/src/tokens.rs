// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia tokens`: small batches of short-lived, single-use hades-gate tokens
//! for `gaia heal-tick --loop --token-dir D` (decision D-L3, accepted by Mate
//! 2026-10-21). There is **no standing grant** and **no HADES mode change**:
//! every restart still needs its own Ed25519-signed, expiring, plan-bound
//! token, consumed once in hades-gate's replay ledger.
//!
//! * `gaia tokens request` (gate host, no key): writes the exact plan and a
//!   request for N tokens (default [`DEFAULT_COUNT`], at most
//!   [`MAX_BATCH`]; lifetime default [`DEFAULT_TTL_S`], at most
//!   [`MAX_TTL_S`] = 1 h) into the token dir. It refuses when the live
//!   (unexpired, unused) tokens already there plus the new batch would pass
//!   [`MAX_BATCH`], so tokens cannot be stockpiled into a standing grant.
//! * `gaia tokens mint` (**admin side only**, needs the admin seed; the gate
//!   host never holds it): checks the request (plan hash, batch, lifetime,
//!   age <= [`REQUEST_MAX_AGE_S`], not minted before, live-token ceiling) and
//!   writes `<id>-<i>.tok` files (0600, never overwritten). Same signing
//!   code as `hades-gate mint`.
//! * `gaia tokens status` (gate host): every token file's state and expiry
//!   from its signed payload (and, with `--hades-config`, its signature
//!   and replay-ledger state). `--prune` renames expired `*.tok` to
//!   `*.tok.expired`.
//!
//! Token strings are never printed or logged: only file names, the nonce
//! (as hades-gate already prints it) and Zagreb times.

use crate::live_heal;
use gaia_live::heal;
use hades_gate::token::{from_hex, plan_hash, to_hex, Claims, TOKEN_PREFIX};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Tokens per batch when `--count` is not given.
pub const DEFAULT_COUNT: u64 = 3;
/// Most live tokens in one token dir (= the combined restart cap per hour:
/// more could never be used inside the window).
pub const MAX_BATCH: u64 = heal::MAX_RESTARTS_PER_HOUR as u64;
/// Token lifetime when `--ttl-s` is not given (hades-gate's default ceiling).
pub const DEFAULT_TTL_S: u64 = 900;
/// Longest token lifetime: 1 h (hades-gate's hard ceiling too).
pub const MAX_TTL_S: u64 = 3600;
/// A request older than this is refused by `mint` (ask again).
pub const REQUEST_MAX_AGE_S: u64 = 3600;
/// Action every token is scoped to.
pub const ACTION: &str = "heal-apply";

const MAX_FILE: u64 = 64 * 1024;

fn now() -> u64 {
    hades_gate::approval::unix_now()
}

fn zt(s: u64) -> String {
    zagreb_time::from_unix(i64::try_from(s).unwrap_or(i64::MAX))
}

fn read_small(p: &Path) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    std::fs::File::open(p)
        .and_then(|f| f.take(MAX_FILE + 1).read_to_end(&mut buf))
        .map_err(|e| format!("{}: {}", p.display(), e.kind()))?;
    if buf.len() as u64 > MAX_FILE {
        return Err(format!("{} is too large", p.display()));
    }
    Ok(buf)
}

fn write_new(p: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(p)
        .map_err(|e| format!("{}: {}", p.display(), e.kind()))?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("{}: {}", p.display(), e.kind()))
}

/// The signed claims inside a token string, WITHOUT checking the signature
/// (used for the expiry only; hades-gate does the real check). `None` for
/// anything that is not a well-formed token.
#[must_use]
pub fn peek_claims(token: &str) -> Option<Claims> {
    let mut parts = token.trim().split('.');
    if parts.next() != Some(TOKEN_PREFIX) {
        return None;
    }
    let payload = from_hex(parts.next()?)?;
    Claims::parse_payload(std::str::from_utf8(&payload).ok()?).ok()
}

fn peek_file(p: &Path) -> Option<Claims> {
    let b = read_small(p).ok()?;
    peek_claims(std::str::from_utf8(&b).ok()?)
}

/// `*.tok` files in `dir`, sorted by name.
fn tok_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("tok") && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Unexpired `*.tok` files at `now` (unreadable ones count as live: fail
/// closed for the stockpile ceiling).
#[must_use]
pub fn live_tokens(dir: &Path, now: u64) -> Vec<PathBuf> {
    tok_files(dir)
        .into_iter()
        .filter(|p| peek_file(p).is_none_or(|c| c.expires_at > now))
        .collect()
}

fn rename_suffix(p: &Path, suffix: &str) -> bool {
    let mut to = p.as_os_str().to_owned();
    to.push(format!(".{suffix}"));
    std::fs::rename(p, &to).is_ok()
}

/// The next usable token for the loop: first `*.tok` by name whose signed
/// expiry is still ahead. Expired ones are renamed `*.tok.expired` on the
/// way (they would only be refused by hades-gate). Returns the token and the
/// number of files moved aside.
pub fn next_live(dir: &Path, now: u64) -> (Option<PathBuf>, u64) {
    let mut moved = 0;
    for p in tok_files(dir) {
        match peek_file(&p) {
            Some(c) if c.expires_at <= now => {
                if rename_suffix(&p, "expired") {
                    moved += 1;
                }
            }
            // Live, or not parseable: hades-gate decides (and the loop
            // marks a refused one `*.tok.refused`).
            _ => return (Some(p), moved),
        }
    }
    (None, moved)
}

fn flag<'a>(own: &[&'a str], k: &str) -> Option<&'a str> {
    own.iter()
        .position(|a| *a == k)
        .and_then(|i| own.get(i + 1))
        .copied()
}

fn num(own: &[&str], k: &str, default: u64) -> Result<u64, String> {
    match flag(own, k) {
        None if own.contains(&k) => Err(format!("{k} needs a value")),
        None => Ok(default),
        Some(v) => v
            .parse()
            .map_err(|_| format!("{k} needs a number, got {v:?}")),
    }
}

/// `gaia tokens request --token-dir D [--count 3] [--ttl-s 900] --service S
/// --port P --allow-ports A-B [--health /health] -- START_CMD...`
pub fn request(args: &[&str]) -> Result<Value, String> {
    request_at(args, now())
}

/// [`request`] at an explicit time (tests).
pub fn request_at(args: &[&str], now: u64) -> Result<Value, String> {
    let split = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    let own = &args[..split];
    let dir = PathBuf::from(flag(own, "--token-dir").ok_or("tokens request needs --token-dir")?);
    let count = num(own, "--count", DEFAULT_COUNT)?;
    let ttl = num(own, "--ttl-s", DEFAULT_TTL_S)?;
    if !(1..=MAX_BATCH).contains(&count) {
        return Err(format!(
            "--count must be 1..={MAX_BATCH} (small batches only, D-L3)"
        ));
    }
    if !(1..=MAX_TTL_S).contains(&ttl) {
        return Err(format!(
            "--ttl-s must be 1..={MAX_TTL_S} (short-lived only, D-L3)"
        ));
    }
    // Same target parsing and scope guard as heal-plan.
    let plan = live_heal::heal_plan_cli(&strip(
        args,
        &["--token-dir", "--count", "--ttl-s", "--out"],
    ))?;
    let target = plan
        .get("target")
        .and_then(gaia_core::Json::as_str)
        .unwrap_or("")
        .to_string();
    let sha = plan
        .get("plan_sha256")
        .and_then(gaia_core::Json::as_str)
        .unwrap_or("")
        .to_string();
    if plan.get("admin_safety").and_then(gaia_core::Json::as_str) != Some("ok") {
        return Err("the heal plan fails hades-gate admin safety; nothing to request".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e.kind()))?;
    let live = live_tokens(&dir, now);
    if live.len() as u64 + count > MAX_BATCH {
        return Err(format!(
            "{} live token(s) already in {}; with {count} more that passes the ceiling of {MAX_BATCH} (no stockpiling, D-L3)",
            live.len(),
            dir.display()
        ));
    }
    let id = format!("req-{now}-{}", to_hex(&hades_gate::keys::os_random::<4>()?));
    let plan_bytes = hades_gate::adapter::gaia_plan_bytes(
        &plan
            .get("plan")
            .and_then(|p| match p {
                gaia_core::Json::Arr(a) => Some(
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default(),
    );
    if to_hex(&plan_hash(&plan_bytes)) != sha {
        return Err("internal: plan bytes do not match the plan hash".into());
    }
    let plan_path = dir.join(format!("{id}.plan"));
    let req_path = dir.join(format!("{id}.request.json"));
    write_new(&plan_path, &plan_bytes, 0o644)?;
    let req = json!({
        "v": 1, "id": id, "action": ACTION, "target": target, "plan_sha256": sha,
        "plan_file": plan_path.file_name().map(|n| n.to_string_lossy().to_string()),
        "count": count, "ttl_s": ttl, "created_unix": now, "created": zt(now),
        "mint_before": zt(now + REQUEST_MAX_AGE_S),
        "standing_grant": false, "hades_mode_change": false,
        "decision": "D-L3 accepted 2026-10-21: small batches of short-lived single-use tokens",
    });
    write_new(&req_path, format!("{req}\n").as_bytes(), 0o644)?;
    Ok(json!({
        "ok": true, "request": req_path.display().to_string(), "plan_file": plan_path.display().to_string(),
        "id": id, "target": target, "plan_sha256": sha, "count": count, "ttl_s": ttl,
        "live_tokens_before": live.len(), "created": zt(now),
        "admin_next_step": format!("on the ADMIN machine (never the gate host): gaia tokens mint --request {} --seed ADMIN_SEED_FILE", req_path.display()),
    }))
}

/// Drop `keys` and their values from Gaia's own flags (before `--`).
fn strip<'a>(args: &[&'a str], keys: &[&str]) -> Vec<&'a str> {
    let split = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    let mut out = Vec::new();
    let mut i = 0;
    while i < split {
        if keys.contains(&args[i]) {
            i += 2;
            continue;
        }
        out.push(args[i]);
        i += 1;
    }
    out.extend_from_slice(&args[split..]);
    out
}

/// `gaia tokens mint --request R --seed S` (admin side).
pub fn mint(args: &[&str]) -> Result<Value, String> {
    mint_at(args, now())
}

/// [`mint`] at an explicit issue time (tests).
pub fn mint_at(args: &[&str], now: u64) -> Result<Value, String> {
    let req_path = PathBuf::from(flag(args, "--request").ok_or("tokens mint needs --request")?);
    let seed_path =
        PathBuf::from(flag(args, "--seed").ok_or("tokens mint needs --seed (admin side only)")?);
    let dir = req_path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut minted_marker = req_path.as_os_str().to_owned();
    minted_marker.push(".minted");
    let minted_marker = PathBuf::from(minted_marker);
    if minted_marker.exists() {
        return Err(format!(
            "{} was already minted; ask for a new request",
            req_path.display()
        ));
    }
    let req: Value = serde_json::from_slice(&read_small(&req_path)?)
        .map_err(|e| format!("{}: {e}", req_path.display()))?;
    let s = |k: &str| req.get(k).and_then(Value::as_str).map(str::to_string);
    let u = |k: &str| req.get(k).and_then(Value::as_u64);
    if u("v") != Some(1) || s("action").as_deref() != Some(ACTION) {
        return Err("request is not a v1 heal-apply request".into());
    }
    if req.get("standing_grant") != Some(&Value::Bool(false))
        || req.get("hades_mode_change") != Some(&Value::Bool(false))
    {
        return Err(
            "request asks for a standing grant or a HADES mode change; refused (D-L3)".into(),
        );
    }
    let id = s("id").ok_or("request has no id")?;
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || id.is_empty() || id.len() > 64
    {
        return Err("request id is not [A-Za-z0-9-]{1,64}".into());
    }
    let target = s("target").ok_or("request has no target")?;
    if !hades_gate::token::valid_target(&target) {
        return Err("request target is not a valid hades-gate target".into());
    }
    let count = u("count").ok_or("request has no count")?;
    let ttl = u("ttl_s").ok_or("request has no ttl_s")?;
    if !(1..=MAX_BATCH).contains(&count) || !(1..=MAX_TTL_S).contains(&ttl) {
        return Err(format!("request batch {count} / lifetime {ttl} s is outside 1..={MAX_BATCH} / 1..={MAX_TTL_S} (D-L3)"));
    }
    let created = u("created_unix").ok_or("request has no created_unix")?;
    if created > now + 60 || now - created.min(now) > REQUEST_MAX_AGE_S {
        return Err(format!(
            "request is older than {REQUEST_MAX_AGE_S} s (or from the future); ask again"
        ));
    }
    let plan_name = s("plan_file").ok_or("request has no plan_file")?;
    if plan_name.contains('/') {
        return Err("plan_file must be a plain file name".into());
    }
    let plan = read_small(&dir.join(&plan_name))?;
    let sha = to_hex(&plan_hash(&plan));
    if Some(&sha) != s("plan_sha256").as_ref() {
        return Err("plan file does not match the request's plan_sha256; refused".into());
    }
    let live = live_tokens(&dir, now);
    if live.len() as u64 + count > MAX_BATCH {
        return Err(format!(
            "{} live token(s) already in {}; minting {count} more passes the ceiling of {MAX_BATCH} (D-L3)",
            live.len(),
            dir.display()
        ));
    }
    let key = hades_gate::keys::load_seed(&seed_path)?;
    let mut out = Vec::new();
    for i in 1..=count {
        let claims = Claims {
            action: ACTION.into(),
            target: target.clone(),
            plan_hash: plan_hash(&plan),
            nonce: hades_gate::keys::os_random::<16>()?,
            issued_at: now,
            expires_at: now + ttl,
        };
        Claims::parse_payload(&claims.payload()).map_err(|e| e.reason())?;
        let file = dir.join(format!("{id}-{i}.tok"));
        write_new(&file, claims.sign(&key).as_bytes(), 0o600)?;
        out.push(json!({"file": file.file_name().map(|n| n.to_string_lossy().to_string()),
                        "nonce": to_hex(&claims.nonce), "iat": zt(now), "exp": zt(claims.expires_at)}));
    }
    drop(key);
    write_new(
        &minted_marker,
        format!("{{\"minted\":{count},\"at\":\"{}\"}}\n", zt(now)).as_bytes(),
        0o644,
    )?;
    Ok(
        json!({"ok": true, "minted": count, "target": target, "action": ACTION, "plan_sha256": sha,
              "ttl_s": ttl, "tokens": out, "standing_grant": false, "hades_mode_change": false}),
    )
}

/// `gaia tokens status --token-dir D [--hades-config C] [--prune]`.
pub fn status(args: &[&str]) -> Result<Value, String> {
    status_at(args, now())
}

/// [`status`] at an explicit time (tests).
pub fn status_at(args: &[&str], now: u64) -> Result<Value, String> {
    let dir = PathBuf::from(flag(args, "--token-dir").ok_or("tokens status needs --token-dir")?);
    let gate = match flag(args, "--hades-config") {
        Some(c) => {
            let cfg = hades_gate::config::Config::load(Path::new(c))?;
            let key = hades_gate::keys::load_public(&cfg.admin_pubkey)?;
            // Read-only: never create the replay ledger just to look at it.
            let ledger = if cfg.ledger.exists() {
                Some(hades_gate::ledger::Ledger::open(&cfg.ledger).map_err(|e| e.reason())?)
            } else {
                None
            };
            Some((key, ledger))
        }
        None => None,
    };
    let prune = args.contains(&"--prune");
    let mut rows = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {}", dir.display(), e.kind()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.ends_with(".tok")
                || n.ends_with(".tok.spent")
                || n.ends_with(".tok.refused")
                || n.ends_with(".tok.expired")
        })
        .collect();
    entries.sort();
    let (mut live, mut expired_now, mut pruned) = (0u64, 0u64, 0u64);
    for p in entries {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let text = read_small(&p)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let claims = peek_claims(&text);
        let file_state = name
            .rsplit_once(".tok")
            .map(|(_, s)| s.trim_start_matches('.'))
            .unwrap_or("");
        let mut state = match (file_state, &claims) {
            ("", Some(c)) if c.expires_at <= now => "expired".to_string(),
            ("", Some(_)) => "ready".to_string(),
            ("", None) => "malformed".to_string(),
            (s, _) => s.to_string(),
        };
        let mut sig = Value::Null;
        let mut consumed = Value::Null;
        if let (Some((key, ledger)), Some(c)) = (&gate, &claims) {
            sig = Value::Bool(hades_gate::token::verify_signature(text.trim(), key).is_ok());
            consumed = match ledger {
                Some(l) => l
                    .is_consumed(&c.nonce)
                    .map(Value::Bool)
                    .unwrap_or(Value::Null),
                None => Value::Bool(false),
            };
        }
        if state == "ready" {
            live += 1;
        }
        if state == "expired" && file_state.is_empty() {
            expired_now += 1;
            if prune && rename_suffix(&p, "expired") {
                pruned += 1;
                state = "expired (pruned)".into();
            }
        }
        rows.push(json!({
            "file": name, "state": state,
            "target": claims.as_ref().map(|c| c.target.clone()),
            "action": claims.as_ref().map(|c| c.action.clone()),
            "nonce": claims.as_ref().map(|c| to_hex(&c.nonce)),
            "exp": claims.as_ref().map(|c| zt(c.expires_at)),
            "remaining_s": claims.as_ref().map(|c| c.expires_at.saturating_sub(now)),
            "signature_ok": sig, "consumed_in_ledger": consumed,
        }));
    }
    Ok(
        json!({"token_dir": dir.display().to_string(), "now": zt(now), "ready": live,
              "expired_unpruned": expired_now - pruned, "pruned": pruned, "ceiling": MAX_BATCH,
              "tokens": rows, "standing_grant": false}),
    )
}

/// `gaia tokens <request|mint|status> ...`
pub fn cli(args: &[&str]) -> Result<Value, String> {
    match args.first().copied() {
        Some("request") => request(&args[1..]),
        Some("mint") => mint(&args[1..]),
        Some("status") => status(&args[1..]),
        _ => Err("usage: gaia tokens <request|mint|status> ... (see gaia --help)".into()),
    }
}
