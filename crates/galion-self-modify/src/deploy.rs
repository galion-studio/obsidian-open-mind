// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Root deploy verifier: refuses unless all four signatures verify, every
//! fingerprint matches, the one-time nonce is unused and the approval has
//! not expired. Then: burn nonce, back up, install binary + drop-in, ONE
//! restart, health check, automatic rollback, ntfy alert either way.
//!
//! systemctl, the health probe and ntfy go through [`System`] so the tests
//! drive a mock; [`RealSystem`] is the production one.

use crate::owner::{self, OwnerKey, MAX_SKEW_S, MAX_TTL_S};
use crate::records::{verify_dryrun, verify_test, Entry};
use crate::util::{is_hex, read_file, read_json, write_atomic, write_new, MAX_ARTIFACT, MAX_SMALL};
use hermes_rs::self_modify::sha256_hex;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const DEPLOY_V: &str = "galion-sm/deploy/v1";

/// The three public keys root trusts (all root-owned 0444 on the server).
pub struct Keys {
    pub stack: ed25519_dalek::VerifyingKey,
    pub sandbox: ed25519_dalek::VerifyingKey,
    pub owner: OwnerKey,
}

/// Everything that verified, bound together.
#[derive(Debug, Clone)]
pub struct Verified {
    pub id: String,
    pub base_commit: String,
    pub artifact_sha256: String,
    pub test_record_sha256: String,
    pub dryrun_record_sha256: String,
    pub nonce: String,
    pub issued_at: i64,
    pub expires: i64,
    pub owner_scheme: &'static str,
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

/// Nonces already used (one per line: `nonce<TAB>id<TAB>unix`).
pub fn ledger_has(ledger: &Path, nonce: &str) -> Result<bool, String> {
    if ledger.symlink_metadata().is_err() {
        return Ok(false);
    }
    let b = read_file(ledger, MAX_SMALL).map_err(|e| format!("nonce ledger unreadable: {e}"))?;
    Ok(String::from_utf8_lossy(&b)
        .lines()
        .any(|l| l.split('\t').next() == Some(nonce)))
}

/// Append a nonce under an exclusive lock; refuses if it is already there.
pub fn ledger_consume(ledger: &Path, nonce: &str, id: &str, now: i64) -> Result<(), String> {
    if let Some(d) = ledger.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(ledger)
        .map_err(|e| format!("nonce ledger {}: {e}", ledger.display()))?;
    use std::os::unix::io::AsRawFd;
    // SAFETY: valid open fd; flock only takes an advisory lock on it.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err("nonce ledger: lock failed".into());
    }
    let mut cur = String::new();
    f.read_to_string(&mut cur).map_err(|e| e.to_string())?;
    if cur.lines().any(|l| l.split('\t').next() == Some(nonce)) {
        return Err("refused: approval nonce already used (replay)".into());
    }
    f.write_all(format!("{nonce}\t{id}\t{now}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}

/// All checks, no side effects (the nonce is only read, not burned).
pub fn verify_all(entry: &Path, k: &Keys, ledger: &Path, now: i64) -> Result<Verified, String> {
    if k.stack == k.sandbox
        || k.owner.same_as_ed25519(&k.stack)
        || k.owner.same_as_ed25519(&k.sandbox)
    {
        return Err("refused: stack, sandbox and owner keys must all be different".into());
    }
    let e = Entry::new(entry);
    if e.deploy_record().symlink_metadata().is_ok() {
        return Err("refused: this entry was already deployed once".into());
    }
    // 1 + 2: stack signature (inside verify_test) and the test record.
    let (trec, trsha) = verify_test(entry, &k.stack, &k.sandbox)?;
    if trec["verdict"] != "pass" {
        return Err(format!(
            "refused: test record is not a pass ({})",
            s(&trec, "why")
        ));
    }
    // 3: dry-run record.
    let (drec, drsha) = verify_dryrun(entry, &k.sandbox, &trec, &trsha)?;
    if drec["verdict"] != "pass" || drec["model_calls_real"] != 0 {
        return Err(format!(
            "refused: dry run is not a pass ({})",
            s(&drec, "why")
        ));
    }
    // 4: owner.
    let env =
        read_json(&e.owner_approval()).map_err(|x| format!("refused: no owner approval ({x})"))?;
    let ap = owner::verify(&env, &k.owner).map_err(|x| format!("refused: {x}"))?;
    let art = s(&trec["artifact"], "sha256");
    if s(&ap, "artifact_sha256") != art || s(&drec, "artifact_sha256") != art {
        return Err("refused: owner approved a different artifact (fingerprint mismatch)".into());
    }
    if s(&ap, "base_commit") != s(&trec, "base_commit") {
        return Err("refused: owner approved a different base commit".into());
    }
    if s(&ap, "id") != s(&trec, "id")
        || s(&ap, "test_record_sha256") != trsha
        || s(&ap, "dryrun_record_sha256") != drsha
    {
        return Err("refused: owner approval is for different test/dry-run records".into());
    }
    let nonce = s(&ap, "nonce");
    if !is_hex(nonce, 32) {
        return Err("refused: approval nonce malformed".into());
    }
    let (issued, expires) = match (ap["issued_at"].as_i64(), ap["expires"].as_i64()) {
        (Some(i), Some(x)) => (i, x),
        _ => return Err("refused: approval has no issued_at/expires".into()),
    };
    if expires <= issued || expires - issued > MAX_TTL_S {
        return Err("refused: approval window longer than 30 minutes".into());
    }
    if issued > now + MAX_SKEW_S {
        return Err("refused: approval issued in the future".into());
    }
    if now > expires {
        return Err("refused: approval expired".into());
    }
    if ledger_has(ledger, nonce)? {
        return Err("refused: approval nonce already used (replay)".into());
    }
    Ok(Verified {
        id: s(&trec, "id").into(),
        base_commit: s(&trec, "base_commit").into(),
        artifact_sha256: art.into(),
        test_record_sha256: trsha,
        dryrun_record_sha256: drsha,
        nonce: nonce.into(),
        issued_at: issued,
        expires,
        owner_scheme: k.owner.scheme(),
    })
}

/// Side effects the deploy needs; mocked in tests.
pub trait System {
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String>;
    /// Ok once the service answers healthy (implementations poll).
    fn health(&mut self) -> Result<String, String>;
    fn alert(&mut self, title: &str, priority: &str, tags: &str, body: &str) -> Result<(), String>;
}

pub struct DeployCfg {
    pub bin_dir: PathBuf,
    pub dropin_dir: PathBuf,
    /// e.g. `99j-self-modify-approved.conf`
    pub dropin_name: String,
    /// e.g. `hermes-rs.service`
    pub unit: String,
    pub ledger: PathBuf,
    pub backup_dir: PathBuf,
    /// Extra `[Service]` lines kept in the drop-in.
    pub extra_lines: Vec<String>,
}

impl DeployCfg {
    pub fn dropin(&self) -> PathBuf {
        self.dropin_dir.join(&self.dropin_name)
    }
}

fn alert_quiet(sys: &mut dyn System, title: &str, prio: &str, tags: &str, body: &str) -> Value {
    match sys.alert(title, prio, tags, body) {
        Ok(()) => json!("sent"),
        Err(e) => json!(format!("ntfy failed: {e}")),
    }
}

/// Full root deploy. Returns the deploy record on success; on refusal or
/// rollback returns Err (after alerting).
pub fn deploy(
    entry: &Path,
    k: &Keys,
    cfg: &DeployCfg,
    sys: &mut dyn System,
    now: i64,
) -> Result<Value, String> {
    let e = Entry::new(entry);
    let label = entry
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_default();
    if cfg.dropin_name.contains('/') || !cfg.dropin_name.ends_with(".conf") {
        return Err("bad drop-in name".into());
    }
    let v = match verify_all(entry, k, &cfg.ledger, now) {
        Ok(v) => v,
        Err(why) => {
            alert_quiet(
                sys,
                "Hermes self-change REFUSED",
                "high",
                "no_entry",
                &format!("{label}: deploy refused, nothing changed. {why}"),
            );
            return Err(why);
        }
    };
    // Burn the nonce before touching anything: a failed deploy needs a fresh card.
    ledger_consume(&cfg.ledger, &v.nonce, &v.id, now)?;

    // Backup.
    let dropin = cfg.dropin();
    let bdir = cfg.backup_dir.join(format!("{}-{now}", v.id));
    std::fs::create_dir_all(&bdir).map_err(|x| format!("{}: {x}", bdir.display()))?;
    let prev = if dropin.symlink_metadata().is_ok() {
        let b = read_file(&dropin, MAX_SMALL)?;
        write_new(&bdir.join("previous-dropin"), &b, 0o600)?;
        true
    } else {
        false
    };

    // Install the exact signed artifact under a new versioned name.
    let art = read_file(&e.artifact(), MAX_ARTIFACT)?;
    if sha256_hex(&art) != v.artifact_sha256 {
        return Err("artifact changed during deploy".into());
    }
    std::fs::create_dir_all(&cfg.bin_dir).map_err(|x| x.to_string())?;
    let bin = cfg.bin_dir.join(format!(
        "hermes-gateway-sm-{}-{}",
        v.id,
        &v.artifact_sha256[..12]
    ));
    write_new(&bin, &art, 0o755)?;
    if sha256_hex(&read_file(&bin, MAX_ARTIFACT)?) != v.artifact_sha256 {
        return Err(format!("{}: installed bytes do not match", bin.display()));
    }
    let mut body = format!(
        "# galion-sm deploy {id}: owner-approved ({scheme}, nonce {nonce}), artifact sha256 {sha}\n\
         # base {base}; test record {tr}; dry-run record {dr}\n\
         # Backup: {bk}\n[Service]\nExecStart=\nExecStart={bin}\n",
        id = v.id,
        scheme = v.owner_scheme,
        nonce = v.nonce,
        sha = v.artifact_sha256,
        base = v.base_commit,
        tr = v.test_record_sha256,
        dr = v.dryrun_record_sha256,
        bk = bdir.display(),
        bin = bin.display()
    );
    for l in &cfg.extra_lines {
        body.push_str(l.trim());
        body.push('\n');
    }
    std::fs::create_dir_all(&cfg.dropin_dir).map_err(|x| x.to_string())?;
    write_atomic(&dropin, body.as_bytes(), 0o644)?;

    // ONE restart, then the health check.
    let started = sys
        .systemctl(&["daemon-reload"])
        .and_then(|_| sys.systemctl(&["restart", &cfg.unit]));
    let health = started.and_then(|_| sys.health());
    let mut rec = json!({
        "version": DEPLOY_V, "id": v.id, "binary": bin, "artifact_sha256": v.artifact_sha256,
        "base_commit": v.base_commit, "dropin": dropin, "previous_dropin_saved": prev,
        "backup": bdir, "nonce": v.nonce, "owner_scheme": v.owner_scheme,
        "deployed_at": now, "deployed_at_zagreb": hermes_rs::time::zagreb_rfc3339(now),
    });
    match health {
        Ok(h) => {
            rec["state"] = json!("deployed");
            rec["health"] = json!(h);
            rec["alert"] = alert_quiet(sys, "Hermes self-change DEPLOYED", "default", "white_check_mark",
                &format!("{}: new build {} live and healthy. Rollback: galion-sm-deploy rollback --entry {}",
                    v.id, &v.artifact_sha256[..12], entry.display()));
            write_new(&e.deploy_record(), format!("{rec}\n").as_bytes(), 0o644)?;
            Ok(rec)
        }
        Err(why) => {
            let rb = restore(&dropin, &bdir, prev, now, cfg, sys);
            rec["state"] = json!("rolled_back");
            rec["failure"] = json!(why);
            rec["rollback"] = rb.clone();
            rec["alert"] = alert_quiet(sys, "Hermes self-change ROLLED BACK", "urgent", "warning",
                &format!("{}: health check failed after restart ({why}); previous drop-in restored, rollback health: {}",
                    v.id, rb["health"]));
            write_new(&e.deploy_record(), format!("{rec}\n").as_bytes(), 0o644)?;
            Err(format!("health check failed ({why}); rolled back"))
        }
    }
}

/// Put the previous drop-in back (or move ours aside), reload, restart, re-check.
fn restore(
    dropin: &Path,
    bdir: &Path,
    prev: bool,
    now: i64,
    cfg: &DeployCfg,
    sys: &mut dyn System,
) -> Value {
    let put_back = if prev {
        read_file(&bdir.join("previous-dropin"), MAX_SMALL)
            .and_then(|b| write_atomic(dropin, &b, 0o644))
            .map(|_| "previous drop-in restored".to_string())
    } else {
        let aside = dropin.with_extension(format!("conf.rolled-back-{now}"));
        std::fs::rename(dropin, &aside)
            .map(|_| format!("drop-in moved to {}", aside.display()))
            .map_err(|x| x.to_string())
    };
    let restarted = sys
        .systemctl(&["daemon-reload"])
        .and_then(|_| sys.systemctl(&["restart", &cfg.unit]));
    let health = restarted.clone().and_then(|_| sys.health());
    json!({
        "dropin": put_back.unwrap_or_else(|x| format!("FAILED: {x}")),
        "restart": restarted.map(|_| "ok".to_string()).unwrap_or_else(|x| format!("FAILED: {x}")),
        "health": health.unwrap_or_else(|x| format!("FAILED: {x}")),
    })
}

/// Manual rollback by a person (root) after a deploy that went live.
pub fn rollback(
    entry: &Path,
    cfg: &DeployCfg,
    sys: &mut dyn System,
    now: i64,
) -> Result<Value, String> {
    let e = Entry::new(entry);
    let mut d = read_json(&e.deploy_record())?;
    if d["state"] != "deployed" {
        return Err(format!("nothing to roll back (state {})", d["state"]));
    }
    let dropin = PathBuf::from(s(&d, "dropin"));
    if dropin != cfg.dropin() {
        return Err("deploy record names a different drop-in".into());
    }
    let rb = restore(
        &dropin,
        Path::new(s(&d, "backup")),
        d["previous_dropin_saved"] == true,
        now,
        cfg,
        sys,
    );
    d["state"] = json!("rolled_back");
    d["rollback"] = rb.clone();
    d["rolled_back_at"] = json!(now);
    alert_quiet(
        sys,
        "Hermes self-change ROLLED BACK (manual)",
        "high",
        "warning",
        &format!("{}: manual rollback, health: {}", s(&d, "id"), rb["health"]),
    );
    write_atomic(&e.deploy_record(), format!("{d}\n").as_bytes(), 0o644)?;
    Ok(d)
}

// ---------- production System ----------

/// systemctl + loopback HTTP health probe + loopback ntfy relay.
pub struct RealSystem {
    pub systemctl: PathBuf,
    /// host:port of the service, e.g. 127.0.0.1:8642
    pub health_addr: String,
    pub health_path: String,
    pub health_timeout_s: u64,
    /// host:port of galion-ntfy-relay (POST /), e.g. 127.0.0.1:2586
    pub ntfy_addr: String,
}

fn http(addr: &str, req: &[u8]) -> Result<String, String> {
    use std::net::{TcpStream, ToSocketAddrs};
    let sa = addr
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut c = TcpStream::connect_timeout(&sa, std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();
    c.write_all(req).map_err(|e| e.to_string())?;
    let mut out = vec![];
    let _ = c.take(64 * 1024).read_to_end(&mut out);
    Ok(String::from_utf8_lossy(&out).to_string())
}

pub fn http_status(resp: &str) -> u16 {
    resp.split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0)
}

impl System for RealSystem {
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String> {
        let st = std::process::Command::new(&self.systemctl)
            .args(args)
            .status()
            .map_err(|e| format!("systemctl: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("systemctl {} -> {st}", args.join(" ")))
        }
    }
    fn health(&mut self) -> Result<String, String> {
        let req = format!(
            "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.health_path, self.health_addr
        );
        let mut last = String::from("no answer");
        for _ in 0..self.health_timeout_s.max(1) {
            match http(&self.health_addr, req.as_bytes()) {
                Ok(r) if http_status(&r) == 200 => return Ok(format!("{} 200", self.health_path)),
                Ok(r) => last = format!("http {}", http_status(&r)),
                Err(e) => last = e,
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        Err(format!("{} not healthy: {last}", self.health_path))
    }
    fn alert(&mut self, title: &str, priority: &str, tags: &str, body: &str) -> Result<(), String> {
        let body: String = body.chars().take(3500).collect();
        let req = format!(
            "POST / HTTP/1.0\r\nHost: {}\r\nTitle: {title}\r\nPriority: {priority}\r\nTags: {tags}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.ntfy_addr,
            body.len()
        );
        let r = http(&self.ntfy_addr, req.as_bytes())?;
        match http_status(&r) {
            200..=299 => Ok(()),
            c => Err(format!("relay http {c}")),
        }
    }
}
