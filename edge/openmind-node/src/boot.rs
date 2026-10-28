// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `omnode boot`: the edge node's boot path.
//!
//! 1. **First boot**: create the device identity in the state dir (no
//!    network is bound; that stays an explicit `identity bind`), record the
//!    boot in `boot.json`, and open a one-time pairing code.
//! 2. **Sandbox gate**: the seccomp deny-list is installed in throw-away
//!    children and real syscalls are tried (`seccomp::self_test`). With
//!    `--require-seccomp` the node refuses to start when it is not enforced.
//! 3. **Pairing**: a small HTTP/1.1 listener (loopback unless `--allow-lan`)
//!    trades the pairing code for a bearer token. The store follows
//!    galion-one's fail-closed pairing (swarm/one-probe b44cd4f, ws/ui
//!    15cdb53): a missing, unreadable or empty store pairs nobody and
//!    authorises nobody; codes are trimmed and upper-cased; every refusal is
//!    counted and 5 refusals lock pairing for 15 minutes. Stricter than
//!    galion-one: codes expire (`--code-ttl`, default 600 s) and are single
//!    use. Only SHA-256 hashes of the code and tokens are stored (file 600).
//!
//! The process runs in the foreground and needs no init system: run it
//! directly, under tini, or under any restart loop. SIGTERM/SIGINT exit 0
//! (also when it is PID 1); every state write is write-temp-then-rename, so
//! a kill at any point leaves the previous or the next state, never half.

use crate::identity::Identity;
use crate::util::{hex, random_bytes};
use crate::{seccomp, sysinfo, zagreb};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:7420";
pub const DEFAULT_CODE_TTL: i64 = 600;
pub const MAX_FAILS: u64 = 5;
pub const LOCK_SECS: i64 = 15 * 60;
const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 8;
const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;
const CONN_DEADLINE: Duration = Duration::from_secs(3);

pub fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

/// Constant-time equality for equal-length hex digests.
fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Write `data` to `path` atomically with mode 600.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    f.write_all(data)
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

// ---------------------------------------------------------------- boot record

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootRecord {
    pub boot_count: u64,
    pub first_boot: String,
    pub last_boot: String,
}

/// Bump `boot.json`. A missing file is boot 1; a corrupt one is an error
/// (never silently reset).
pub fn record_boot(dir: &Path, now: i64) -> Result<BootRecord, String> {
    let path = dir.join("boot.json");
    let mut r = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str::<BootRecord>(&t)
            .map_err(|e| format!("{}: corrupt ({e}); refusing to reset it", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BootRecord::default(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let ts = zagreb::rfc3339(now);
    if r.boot_count == 0 {
        r.first_boot = ts.clone();
    }
    r.boot_count += 1;
    r.last_boot = ts;
    write_atomic(
        &path,
        serde_json::to_string(&r).unwrap_or_default().as_bytes(),
    )?;
    Ok(r)
}

// ---------------------------------------------------------------- pair store

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    /// sha256 of the current pairing code; empty = no code open.
    #[serde(default)]
    pub code_hash: String,
    /// Unix seconds after which the code is refused.
    #[serde(default)]
    pub code_expires: i64,
    #[serde(default)]
    pub token_hashes: Vec<String>,
    #[serde(default)]
    pub fails: u64,
    /// Unix seconds; pairing refused ("later") until then.
    #[serde(default)]
    pub locked_until: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairOutcome {
    /// Paired; the plain token is returned once and only its hash is kept.
    Ok(String),
    /// Wrong code, no code open, or no store: refused.
    No,
    /// The code was right shape-wise but its time is over.
    Expired,
    /// Too many refusals; try again after the lock.
    Later,
}

/// Where the store lives, and how it is read. Reads fail closed: anything
/// but a readable, parseable file is "nobody is paired".
pub struct PairStore {
    pub path: PathBuf,
}

impl PairStore {
    pub fn new(dir: &Path) -> PairStore {
        PairStore {
            path: dir.join("pair-store.json"),
        }
    }

    /// Ok(None) = no store yet; Err = unreadable or corrupt.
    pub fn read(&self) -> Result<Option<Store>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => serde_json::from_str(&t)
                .map(Some)
                .map_err(|e| format!("{}: corrupt ({e})", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", self.path.display())),
        }
    }

    pub fn write(&self, s: &Store) -> Result<(), String> {
        write_atomic(
            &self.path,
            serde_json::to_string(s).unwrap_or_default().as_bytes(),
        )
    }

    /// Open a fresh pairing code (replaces any open one; tokens are kept).
    /// Returns the plain code, shown once to the owner.
    pub fn open_code(&self, now: i64, ttl: i64) -> Result<String, String> {
        let mut s = self.read()?.unwrap_or_default();
        let code = new_code()?;
        s.code_hash = sha256_hex(&normalize(&code));
        s.code_expires = now + ttl.max(1);
        self.write(&s)?;
        Ok(code)
    }

    /// Trade a code for a token. Fail-closed on every path.
    pub fn pair(&self, code: &str, now: i64) -> PairOutcome {
        let mut s = match self.read() {
            Ok(Some(s)) => s,
            // No store or unreadable store: nobody can pair, nothing to count.
            _ => return PairOutcome::No,
        };
        if s.locked_until > 0 && now < s.locked_until {
            return PairOutcome::Later;
        }
        let code = normalize(code);
        let want = s.code_hash.clone();
        let matches = !want.is_empty() && !code.is_empty() && ct_eq(&sha256_hex(&code), &want);
        let refuse = |s: &mut Store, out: PairOutcome| {
            s.fails += 1;
            if s.fails >= MAX_FAILS {
                s.locked_until = now + LOCK_SECS;
            }
            // If the refusal cannot be counted, still refuse.
            let _ = self.write(s);
            out
        };
        if !matches {
            return refuse(&mut s, PairOutcome::No);
        }
        if now > s.code_expires {
            s.code_hash.clear();
            s.code_expires = 0;
            return refuse(&mut s, PairOutcome::Expired);
        }
        let token = match random_bytes::<32>() {
            Ok(b) => hex(&b),
            Err(_) => return PairOutcome::No,
        };
        let digest = sha256_hex(&token);
        if !s.token_hashes.contains(&digest) {
            s.token_hashes.push(digest);
        }
        // Single use.
        s.code_hash.clear();
        s.code_expires = 0;
        s.fails = 0;
        s.locked_until = 0;
        match self.write(&s) {
            Ok(()) => PairOutcome::Ok(token),
            // A token we could not persist would not survive a restart.
            Err(_) => PairOutcome::No,
        }
    }

    /// Is this bearer token one we issued? Missing/unreadable/empty store,
    /// empty token: no.
    pub fn token_ok(&self, token: &str) -> bool {
        let token = token.trim();
        if token.is_empty() {
            return false;
        }
        let Ok(Some(s)) = self.read() else {
            return false;
        };
        let d = sha256_hex(token);
        s.token_hashes.iter().any(|h| ct_eq(h, &d))
    }

    pub fn paired(&self) -> usize {
        self.read()
            .ok()
            .flatten()
            .map(|s| s.token_hashes.len())
            .unwrap_or(0)
    }
}

/// Codes are compared trimmed, upper-cased and without `-`/spaces, so
/// `abcd-efgh` and `ABCDEFGH` are the same code.
pub fn normalize(code: &str) -> String {
    code.trim()
        .chars()
        .filter(|c| *c != '-' && *c != ' ')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// 8 chars from a 32-symbol alphabet (no 0/O/1/I), 40 bits, unbiased.
pub fn new_code() -> Result<String, String> {
    let b = random_bytes::<CODE_LEN>().map_err(|e| e.to_string())?;
    let s: String = b
        .iter()
        .map(|x| CODE_ALPHABET[(x & 31) as usize] as char)
        .collect();
    Ok(format!("{}-{}", &s[..4], &s[4..]))
}

// ---------------------------------------------------------------- boot

pub struct Opts {
    pub state_dir: PathBuf,
    pub listen: String,
    pub allow_lan: bool,
    pub code_ttl: i64,
    pub code_file: Option<PathBuf>,
    pub new_code: bool,
    pub require_seccomp: bool,
}

pub struct Booted {
    pub id: Identity,
    pub record: BootRecord,
    pub first_boot: bool,
    pub seccomp: seccomp::SelfTest,
    pub store: PairStore,
    pub code_expires: Option<String>,
    pub listener: TcpListener,
    pub addr: SocketAddr,
}

fn check_listen(listen: &str, allow_lan: bool) -> Result<SocketAddr, String> {
    let addr: SocketAddr = listen
        .parse()
        .map_err(|_| format!("--listen {listen}: not IP:PORT"))?;
    if !addr.ip().is_loopback() && !allow_lan {
        return Err(format!(
            "--listen {addr}: not loopback; pairing is plain HTTP, add --allow-lan only on a network you own"
        ));
    }
    Ok(addr)
}

/// Everything up to (not including) serving. Split out so tests and the
/// real-life script see the same order of checks.
pub fn start(o: &Opts) -> Result<Booted, String> {
    let addr = check_listen(&o.listen, o.allow_lan)?;
    let now = zagreb::now_unix();
    let first_boot = !o.state_dir.join("node.key").exists();
    let id = if first_boot {
        Identity::init(&o.state_dir)?
    } else {
        Identity::load(&o.state_dir)?
    };
    let st = seccomp::self_test();
    if o.require_seccomp && !st.enforced {
        return Err(format!(
            "--require-seccomp: seccomp deny-list is not enforced here ({})",
            serde_json::to_string(&st).unwrap_or_default()
        ));
    }
    let store = PairStore::new(&o.state_dir);
    // A corrupt or unreadable store stops the boot: never reset pairings.
    let cur = store.read()?.unwrap_or_default();
    let open = !cur.code_hash.is_empty() && now <= cur.code_expires;
    let mut code_expires = open.then(|| zagreb::rfc3339(cur.code_expires));
    let need_code = o.new_code || (cur.token_hashes.is_empty() && !open);
    if need_code {
        let code = store.open_code(now, o.code_ttl)?;
        let exp = zagreb::rfc3339(now + o.code_ttl.max(1));
        match &o.code_file {
            Some(p) => {
                write_atomic(p, format!("{code}\n").as_bytes())?;
                eprintln!(
                    "omnode: pairing code written to {} (expires {exp})",
                    p.display()
                );
            }
            None => eprintln!("omnode: pairing code {code} (expires {exp}, single use)"),
        }
        code_expires = Some(exp);
    }
    let record = record_boot(&o.state_dir, now)?;
    let listener = TcpListener::bind(addr).map_err(|e| format!("bind {addr}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    Ok(Booted {
        id,
        record,
        first_boot,
        seccomp: st,
        store,
        code_expires,
        listener,
        addr,
    })
}

extern "C" fn on_term(_sig: libc::c_int) {
    const MSG: &[u8] = b"omnode: stopping (signal)\n";
    // SAFETY: write(2) and _exit(2) are async-signal-safe.
    unsafe {
        libc::write(2, MSG.as_ptr().cast(), MSG.len());
        libc::_exit(0);
    }
}

/// SIGTERM/SIGINT exit 0. Needed when we are PID 1 (the kernel ignores
/// default-action signals for PID 1), harmless otherwise.
pub fn install_signal_handlers() {
    let h = on_term as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: installing a handler that only calls async-signal-safe functions.
    unsafe {
        libc::signal(libc::SIGTERM, h);
        libc::signal(libc::SIGINT, h);
    }
}

pub fn boot_event(b: &Booted) -> serde_json::Value {
    serde_json::json!({
        "event": "boot",
        "at": zagreb::now_rfc3339(),
        "device_id": b.id.state.device_id,
        "fingerprint": b.id.fingerprint(),
        "first_boot": b.first_boot,
        "boot_count": b.record.boot_count,
        "listen": b.addr.to_string(),
        "pid": std::process::id(),
        "seccomp": b.seccomp,
        "paired": b.store.paired(),
        "pairing_open_until": b.code_expires,
    })
}

/// Serve forever, one connection at a time (no shared-state races, and an
/// edge node gets a handful of requests).
pub fn serve(b: &Booted) -> ! {
    loop {
        if let Ok((s, _)) = b.listener.accept() {
            let _ = handle(b, s);
        }
    }
}

// ---------------------------------------------------------------- HTTP

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub bearer: Option<String>,
    pub body: Vec<u8>,
}

/// Parse one HTTP/1.x request from `r`. Head ≤ 8 KiB, body ≤ 4 KiB, whole
/// request within 3 s; anything else is an error (connection dropped).
pub fn read_request(r: &mut impl Read, deadline: Instant) -> Result<Request, u16> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(431);
        }
        if Instant::now() > deadline {
            return Err(408);
        }
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let mut rl = lines.next().ok_or(400u16)?.split(' ');
    let method = rl.next().ok_or(400u16)?.to_string();
    let path = rl.next().ok_or(400u16)?.to_string();
    if !rl.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(400);
    }
    let mut len = 0usize;
    let mut bearer = None;
    for l in lines {
        let Some((k, v)) = l.split_once(':') else {
            return Err(400);
        };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.parse().map_err(|_| 400u16)?,
            "transfer-encoding" => return Err(411),
            "authorization" => {
                bearer = v
                    .strip_prefix("Bearer ")
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
            }
            _ => {}
        }
    }
    if len > MAX_BODY {
        return Err(413);
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        if Instant::now() > deadline {
            return Err(408);
        }
        match r.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(len);
    Ok(Request {
        method,
        path,
        bearer,
        body,
    })
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    }
}

/// Route one request. Pure apart from the pair store.
pub fn route(b: &Booted, req: &Request, now: i64) -> (u16, serde_json::Value) {
    use serde_json::json;
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => (200, json!({"ok": true})),
        ("GET", "/v1/node") => {
            let me = sysinfo::self_info();
            (
                200,
                json!({
                    "device_id": b.id.state.device_id,
                    "fingerprint": b.id.fingerprint(),
                    "version": env!("CARGO_PKG_VERSION"),
                    "arch": me.arch,
                    "platform": me.platform,
                    "boot_count": b.record.boot_count,
                    "first_boot": b.record.first_boot,
                    "last_boot": b.record.last_boot,
                    "pid": std::process::id(),
                    "seccomp": b.seccomp,
                    "paired": b.store.paired(),
                    "now": zagreb::rfc3339(now),
                }),
            )
        }
        ("POST", "/v1/pair") => {
            let code = serde_json::from_slice::<serde_json::Value>(&req.body)
                .ok()
                .and_then(|v| v.get("code").and_then(|c| c.as_str()).map(String::from));
            let Some(code) = code else {
                return (
                    400,
                    json!({"ok": false, "error": "body must be {\"code\":\"...\"}"}),
                );
            };
            match b.store.pair(&code, now) {
                PairOutcome::Ok(t) => (200, json!({"ok": true, "token": t})),
                PairOutcome::No => (403, json!({"ok": false, "error": "no"})),
                PairOutcome::Expired => (403, json!({"ok": false, "error": "expired"})),
                PairOutcome::Later => (429, json!({"ok": false, "error": "later"})),
            }
        }
        ("GET", "/v1/whoami") => {
            if req.bearer.as_deref().is_some_and(|t| b.store.token_ok(t)) {
                (200, json!({"ok": true, "device_id": b.id.state.device_id}))
            } else {
                (401, json!({"ok": false, "error": "unpaired"}))
            }
        }
        (_, "/health" | "/v1/node" | "/v1/pair" | "/v1/whoami") => {
            (405, json!({"ok": false, "error": "method not allowed"}))
        }
        _ => (404, json!({"ok": false, "error": "not found"})),
    }
}

fn respond(s: &mut TcpStream, code: u16, body: &serde_json::Value) -> std::io::Result<()> {
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 {code} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n",
        reason(code),
        body.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(body.as_bytes())?;
    s.flush()
}

fn handle(b: &Booted, mut s: TcpStream) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(1)))?;
    s.set_write_timeout(Some(Duration::from_secs(2)))?;
    let (code, body) = match read_request(&mut s, Instant::now() + CONN_DEADLINE) {
        Ok(req) => route(b, &req, zagreb::now_unix()),
        Err(code) => (
            code,
            serde_json::json!({"ok": false, "error": reason(code)}),
        ),
    };
    respond(&mut s, code, &body)?;
    let _ = s.shutdown(std::net::Shutdown::Both);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "omnode-test-boot-{tag}-{}",
            hex(&random_bytes::<6>().unwrap())
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_or_corrupt_store_fails_closed() {
        let d = tmpdir("closed");
        let s = PairStore::new(&d);
        assert_eq!(s.pair("ABCD-EFGH", 1000), PairOutcome::No);
        assert!(!s.path.exists(), "a refusal must not create a store");
        assert!(!s.token_ok("anything"));
        assert!(!s.token_ok(""));
        std::fs::write(&s.path, b"{not json").unwrap();
        assert_eq!(s.pair("ABCD-EFGH", 1000), PairOutcome::No);
        assert!(!s.token_ok("anything"));
        assert_eq!(s.paired(), 0);
        assert!(s.read().is_err());
        // empty store: no code open, nobody paired
        s.write(&Store::default()).unwrap();
        assert_eq!(s.pair("", 1000), PairOutcome::No);
        assert!(!s.token_ok("x"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn code_pairs_once_then_token_works() {
        let d = tmpdir("ok");
        let s = PairStore::new(&d);
        let code = s.open_code(1000, 600).unwrap();
        assert_eq!(code.len(), 9);
        let raw = std::fs::read_to_string(&s.path).unwrap();
        assert!(!raw.contains(&code) && !raw.contains(&normalize(&code)));
        // lower case, no dash, padded: same code
        let messy = format!("  {} ", normalize(&code).to_lowercase());
        let PairOutcome::Ok(tok) = s.pair(&messy, 1100) else {
            panic!("pairing refused")
        };
        assert!(!std::fs::read_to_string(&s.path).unwrap().contains(&tok));
        assert!(s.token_ok(&tok));
        assert!(!s.token_ok(&tok[1..]));
        assert_eq!(s.paired(), 1);
        // single use
        assert_eq!(s.pair(&code, 1101), PairOutcome::No);
        // the refusal does not drop the token
        assert!(s.token_ok(&tok));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn expired_code_refused_and_closed() {
        let d = tmpdir("exp");
        let s = PairStore::new(&d);
        let code = s.open_code(1000, 10).unwrap();
        assert_eq!(s.pair(&code, 1011), PairOutcome::Expired);
        let st = s.read().unwrap().unwrap();
        assert!(st.code_hash.is_empty());
        assert_eq!(st.fails, 1);
        // once expired it is gone, even back in time
        assert_eq!(s.pair(&code, 1001), PairOutcome::No);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn wrong_codes_count_and_lock() {
        let d = tmpdir("lock");
        let s = PairStore::new(&d);
        let code = s.open_code(1000, 600).unwrap();
        for i in 1..MAX_FAILS {
            assert_eq!(s.pair("WRONG-CODE", 1000), PairOutcome::No);
            assert_eq!(s.read().unwrap().unwrap().fails, i);
        }
        assert_eq!(s.pair("WRONG-CODE", 1000), PairOutcome::No);
        assert_eq!(s.read().unwrap().unwrap().locked_until, 1000 + LOCK_SECS);
        // locked: even the right code is "later"
        assert_eq!(s.pair(&code, 1001), PairOutcome::Later);
        // after the lock the right code (still within its ttl? no: 600 s
        // ttl < 15 min lock) is expired -> owner must reboot with a new code
        assert_eq!(s.pair(&code, 1000 + LOCK_SECS), PairOutcome::Expired);
        // that refusal counted too (fails stay >= 5), so the lock is renewed;
        // a new code does not lift it (a crash-restart must not reset it)
        let relock = s.read().unwrap().unwrap().locked_until;
        assert_eq!(relock, 1000 + 2 * LOCK_SECS);
        let code2 = s.open_code(relock - 1, 600).unwrap();
        assert_eq!(s.pair(&code2, relock - 1), PairOutcome::Later);
        assert!(matches!(s.pair(&code2, relock), PairOutcome::Ok(_)));
        assert_eq!(s.read().unwrap().unwrap().fails, 0);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn code_shape() {
        for _ in 0..50 {
            let c = new_code().unwrap();
            assert_eq!(c.as_bytes()[4], b'-');
            assert!(normalize(&c).bytes().all(|b| CODE_ALPHABET.contains(&b)));
        }
        assert_eq!(normalize(" ab-cd ef "), "ABCDEF");
    }

    #[test]
    fn boot_record_counts_and_refuses_corrupt() {
        let d = tmpdir("rec");
        let a = record_boot(&d, 1_790_000_000).unwrap();
        assert_eq!(a.boot_count, 1);
        assert!(a.first_boot.contains('+'));
        let b = record_boot(&d, 1_790_000_100).unwrap();
        assert_eq!(b.boot_count, 2);
        assert_eq!(b.first_boot, a.first_boot);
        assert_ne!(b.last_boot, a.last_boot);
        std::fs::write(d.join("boot.json"), b"garbage").unwrap();
        assert!(record_boot(&d, 1).is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn listen_must_be_loopback_unless_allowed() {
        assert!(check_listen("127.0.0.1:0", false).is_ok());
        assert!(check_listen("[::1]:0", false).is_ok());
        assert!(check_listen("0.0.0.0:7420", false).is_err());
        assert!(check_listen("192.168.1.5:7420", false).is_err());
        assert!(check_listen("192.168.1.5:7420", true).is_ok());
        assert!(check_listen("localhost:7420", false).is_err());
    }

    #[test]
    fn parse_requests() {
        let far = Instant::now() + Duration::from_secs(5);
        let raw = b"POST /v1/pair HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer abc\r\nContent-Length: 4\r\n\r\nabcdEXTRA";
        let r = read_request(&mut &raw[..], far).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/v1/pair");
        assert_eq!(r.bearer.as_deref(), Some("abc"));
        assert_eq!(r.body, b"abcd");
        let big = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(read_request(&mut big.as_bytes(), far), Err(413));
        let te = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(read_request(&mut &te[..], far), Err(411));
        assert_eq!(read_request(&mut &b"GET /\r\n\r\n"[..], far), Err(400));
        assert_eq!(read_request(&mut &b"GET / HTTP/1.1\r\n"[..], far), Err(400));
        let huge = vec![b'a'; MAX_HEAD + 2048];
        assert_eq!(read_request(&mut &huge[..], far), Err(431));
    }

    fn get(addr: SocketAddr, raw: &str) -> (u16, serde_json::Value) {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let code = out[9..12].parse().unwrap();
        let body = out.split("\r\n\r\n").nth(1).unwrap();
        (code, serde_json::from_str(body).unwrap())
    }

    #[test]
    fn boot_serves_pairing_end_to_end_and_restart_keeps_identity() {
        let d = tmpdir("e2e");
        let code_file = d.join("code.txt");
        let o = Opts {
            state_dir: d.join("state"),
            listen: "127.0.0.1:0".into(),
            allow_lan: false,
            code_ttl: 600,
            code_file: Some(code_file.clone()),
            new_code: false,
            require_seccomp: false,
        };
        let b = start(&o).unwrap();
        assert!(b.first_boot);
        assert_eq!(b.record.boot_count, 1);
        let addr = b.addr;
        let code = std::fs::read_to_string(&code_file).unwrap();
        let dev = b.id.state.device_id.clone();
        std::thread::spawn(move || serve(&b));
        let (c, v) = get(addr, "GET /health HTTP/1.1\r\n\r\n");
        assert_eq!((c, v["ok"].as_bool()), (200, Some(true)));
        let (c, _) = get(addr, "GET /v1/whoami HTTP/1.1\r\n\r\n");
        assert_eq!(c, 401);
        let wrong = r#"{"code":"AAAA-AAAA"}"#;
        let (c, v) = get(
            addr,
            &format!(
                "POST /v1/pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n{wrong}",
                wrong.len()
            ),
        );
        assert_eq!((c, v["error"].as_str()), (403, Some("no")));
        let ok = format!(r#"{{"code":"{}"}}"#, code.trim());
        let (c, v) = get(
            addr,
            &format!(
                "POST /v1/pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n{ok}",
                ok.len()
            ),
        );
        assert_eq!(c, 200, "{v}");
        let tok = v["token"].as_str().unwrap().to_string();
        let (c, v) = get(
            addr,
            &format!("GET /v1/whoami HTTP/1.1\r\nAuthorization: Bearer {tok}\r\n\r\n"),
        );
        assert_eq!((c, v["device_id"].as_str()), (200, Some(dev.as_str())));
        let (c, _) = get(addr, "DELETE /v1/pair HTTP/1.1\r\n\r\n");
        assert_eq!(c, 405);
        let (c, _) = get(addr, "GET /nope HTTP/1.1\r\n\r\n");
        assert_eq!(c, 404);

        // "restart": same state dir, new listener
        std::fs::remove_file(&code_file).unwrap();
        let b2 = start(&o).unwrap();
        assert!(!b2.first_boot);
        assert_eq!(b2.record.boot_count, 2);
        assert_eq!(b2.id.state.device_id, dev);
        assert!(b2.store.token_ok(&tok));
        // paired and no --new-code: pairing stays closed
        assert!(!code_file.exists());
        assert!(b2.code_expires.is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn boot_refuses_corrupt_store_and_public_listen() {
        let d = tmpdir("refuse");
        let mut o = Opts {
            state_dir: d.clone(),
            listen: "0.0.0.0:0".into(),
            allow_lan: false,
            code_ttl: 600,
            code_file: Some(d.join("c")),
            new_code: false,
            require_seccomp: false,
        };
        assert!(start(&o).err().unwrap().contains("not loopback"));
        assert!(
            !d.join("node.key").exists(),
            "nothing created before the listen check"
        );
        o.listen = "127.0.0.1:0".into();
        std::fs::write(d.join("pair-store.json"), b"{").unwrap();
        assert!(start(&o).err().unwrap().contains("corrupt"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn require_seccomp_matches_self_test() {
        let d = tmpdir("sec");
        let o = Opts {
            state_dir: d.clone(),
            listen: "127.0.0.1:0".into(),
            allow_lan: false,
            code_ttl: 600,
            code_file: Some(d.join("c")),
            new_code: false,
            require_seccomp: true,
        };
        let enforced = seccomp::self_test().enforced;
        assert_eq!(start(&o).is_ok(), enforced);
        std::fs::remove_dir_all(&d).ok();
    }
}
