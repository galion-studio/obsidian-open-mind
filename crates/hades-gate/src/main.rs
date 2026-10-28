// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `hades-gate`: the admin approval gate CLI and a tiny localhost HTTP server.
//!
//! Gate side (holds only the admin PUBLIC key):
//!   hades-gate check   --config C --action A --target T --plan-file F [--token-file F]
//!   hades-gate serve   --config C --port P        (binds 127.0.0.1 only)
//!   hades-gate status  --config C
//!   hades-gate disable --config C                 (admin kill switch, no token)
//!   hades-gate enable  --config C
//!
//! Admin side (holds the seed; never on the gate host):
//!   hades-gate keygen    --seed PATH --pub PATH
//!   hades-gate mint      --seed PATH --action A --target T --plan-file F --out PATH [--ttl S] [--iat UNIX]
//!   hades-gate plan-hash --plan-file F
//!
//! Every mode is dry-run: an approval is a decision, nothing is executed.
//! Exit codes: 0 approved or dry-run, 2 refused, 1 usage or I/O error.
//! No key material and no token string is ever printed.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hades_gate::approval::{unix_now, Approver, Request, Verdict};
use hades_gate::config::Config;
use hades_gate::token::{plan_hash, to_hex, Claims};
use hades_gate::{keys, safety};

const USAGE: &str = "usage: hades-gate <check|check-typed|serve|status|disable|enable|keygen|mint|plan-hash> [--flags]  (see crate docs)";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hades-gate: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn flags(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let key = k
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument {k:?}\n{USAGE}"))?;
        let v = it.next().ok_or_else(|| format!("--{key} needs a value"))?;
        if map.insert(key.to_owned(), v.clone()).is_some() {
            return Err(format!("--{key} given twice"));
        }
    }
    Ok(map)
}

fn need<'a>(f: &'a HashMap<String, String>, key: &str) -> Result<&'a str, String> {
    f.get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("missing --{key}"))
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| {
            f.take(safety::MAX_PLAN_BYTES as u64 + 1)
                .read_to_end(&mut buf)
        })
        .map_err(|e| format!("{path}: {}", e.kind()))?;
    Ok(buf)
}

fn run(args: &[String]) -> Result<i32, String> {
    let Some((cmd, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    if matches!(cmd.as_str(), "-h" | "--help" | "help") {
        println!("{USAGE}");
        return Ok(0);
    }
    let f = flags(rest)?;
    match cmd.as_str() {
        "keygen" => {
            let public = keys::generate(Path::new(need(&f, "seed")?), Path::new(need(&f, "pub")?))?;
            println!(
                "{}",
                serde_json::json!({"ok": true, "admin_fingerprint": keys::fingerprint(&public)})
            );
            Ok(0)
        }
        "plan-hash" => {
            println!(
                "{}",
                to_hex(&plan_hash(&read_file(need(&f, "plan-file")?)?))
            );
            Ok(0)
        }
        "mint" => mint(&f),
        "check" => check(&f),
        "check-typed" => check_typed(&f),
        "status" => {
            let cfg = Config::load(Path::new(need(&f, "config")?))?;
            let a = Approver::from_config(&cfg)?;
            println!("{}", status_json(&a));
            Ok(0)
        }
        "disable" => {
            // Needs no token and no key: the admin can always switch HADES off.
            let cfg = Config::load(Path::new(need(&f, "config")?))?;
            std::fs::write(&cfg.disable_file, b"disabled by admin\n")
                .map_err(|e| format!("{}: {}", cfg.disable_file.display(), e.kind()))?;
            println!("{}", serde_json::json!({"ok": true, "disabled": true}));
            Ok(0)
        }
        "enable" => {
            let cfg = Config::load(Path::new(need(&f, "config")?))?;
            match std::fs::remove_file(&cfg.disable_file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {}", cfg.disable_file.display(), e.kind())),
            }
            println!("{}", serde_json::json!({"ok": true, "disabled": false}));
            Ok(0)
        }
        "serve" => serve(&f),
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

fn mint(f: &HashMap<String, String>) -> Result<i32, String> {
    let seed = keys::load_seed(Path::new(need(f, "seed")?))?;
    let plan = read_file(need(f, "plan-file")?)?;
    let ttl: u64 = f
        .get("ttl")
        .map_or(Ok(300), |t| t.parse())
        .map_err(|_| "--ttl must be seconds")?;
    let iat: u64 = f
        .get("iat")
        .map_or(Ok(unix_now()), |t| t.parse())
        .map_err(|_| "--iat must be Unix seconds")?;
    let claims = Claims {
        action: need(f, "action")?.to_owned(),
        target: need(f, "target")?.to_owned(),
        plan_hash: plan_hash(&plan),
        nonce: keys::os_random::<16>()?,
        issued_at: iat,
        expires_at: iat.saturating_add(ttl),
    };
    // Validate by round-tripping the payload.
    Claims::parse_payload(&claims.payload()).map_err(|e| e.reason())?;
    let token = claims.sign(&seed);
    let out = PathBuf::from(need(f, "out")?);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(&out)
        .and_then(|mut file| file.write_all(token.as_bytes()))
        .map_err(|e| format!("{}: {}", out.display(), e.kind()))?;
    println!("{}", claims_json(&claims));
    Ok(0)
}

fn claims_json(c: &Claims) -> serde_json::Value {
    let ts = |s: u64| zagreb_time::from_unix(i64::try_from(s).unwrap_or(i64::MAX));
    serde_json::json!({
        "action": c.action,
        "target": c.target,
        "plan_sha256": to_hex(&c.plan_hash),
        "nonce": to_hex(&c.nonce),
        "iat": ts(c.issued_at),
        "exp": ts(c.expires_at),
    })
}

fn verdict_json(v: &Verdict) -> serde_json::Value {
    let base = serde_json::json!({
        "decision": format!("{:?}", v.decision()),
        "dry_run": true,
        "executed": false,
        "ts": zagreb_time::now_rfc3339(),
    });
    let mut obj = base.as_object().cloned().unwrap_or_default();
    match v {
        Verdict::DryRun => {
            obj.insert("reason".into(), "no token: plan only".into());
        }
        Verdict::Approved(c) => {
            obj.insert("approved".into(), claims_json(c));
        }
        Verdict::Refused(r) => {
            obj.insert("code".into(), r.code().into());
            obj.insert("reason".into(), r.reason().into());
        }
    }
    serde_json::Value::Object(obj)
}

fn exit_for(v: &Verdict) -> i32 {
    i32::from(matches!(v, Verdict::Refused(_))) * 2
}

fn check(f: &HashMap<String, String>) -> Result<i32, String> {
    let cfg = Config::load(Path::new(need(f, "config")?))?;
    let approver = Approver::from_config(&cfg)?;
    let plan = read_file(need(f, "plan-file")?)?;
    let token = match f.get("token-file") {
        Some(p) => String::from_utf8(read_file(p)?).map_err(|_| "token file is not UTF-8")?,
        None => String::new(),
    };
    let v = approver.approve(
        Request {
            action: need(f, "action")?,
            target: need(f, "target")?,
            plan: &plan,
            token: token.trim(),
        },
        unix_now(),
    );
    println!("{}", verdict_json(&v));
    Ok(exit_for(&v))
}

fn check_typed(f: &HashMap<String, String>) -> Result<i32, String> {
    let cfg = Config::load(Path::new(need(f, "config")?))?;
    let approver = Approver::from_config(&cfg)?;
    // The plan file holds the exact canonical typed-plan bytes (no trailing
    // newline); they are hashed and signed unchanged.
    let plan = read_file(need(f, "plan-file")?)?;
    let token = match f.get("token-file") {
        Some(p) => String::from_utf8(read_file(p)?).map_err(|_| "token file is not UTF-8")?,
        None => String::new(),
    };
    let v = approver.approve_typed(
        Request {
            action: need(f, "action")?,
            target: need(f, "target")?,
            plan: &plan,
            token: token.trim(),
        },
        unix_now(),
    );
    println!("{}", verdict_json(&v));
    Ok(exit_for(&v))
}

fn status_json(a: &Approver) -> serde_json::Value {
    serde_json::json!({
        "service": "hades-gate",
        "mode": "dry-run",
        "disabled": a.is_disabled(),
        "admin_fingerprint": a.admin_fingerprint(),
        "ts": zagreb_time::now_rfc3339(),
    })
}

fn serve(f: &HashMap<String, String>) -> Result<i32, String> {
    let cfg = Config::load(Path::new(need(f, "config")?))?;
    let approver = Approver::from_config(&cfg)?;
    let port: u16 = need(f, "port")?.parse().map_err(|_| "--port")?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("bind 127.0.0.1:{port}: {}", e.kind()))?;
    eprintln!("hades-gate: serving on 127.0.0.1:{port} (dry-run)");
    // NERVE push (no-op unless NERVE_URL is set): started + heartbeat.
    nerve_client::event(
        "hades-gate",
        "started",
        &format!("serving on 127.0.0.1:{port}"),
    );
    nerve_client::start_heartbeat("hades-gate", 60);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if handle(stream, &approver) {
            eprintln!("hades-gate: stopped by admin request");
            return Ok(0);
        }
    }
    Ok(0)
}

/// Serve one connection. Returns `true` when the admin asked to stop.
fn handle(mut stream: TcpStream, approver: &Approver) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let t0 = Instant::now();
    let (method, path, status, body, stop) = match read_request(&stream) {
        Ok((method, path, body)) => {
            let (status, body, stop) = route(&method, &path, &body, approver);
            (method, path, status, body, stop)
        }
        Err(e) => (
            String::new(),
            String::new(),
            400,
            serde_json::json!({"error": e}),
            false,
        ),
    };
    let what = if method.is_empty() {
        "bad-request".to_string()
    } else {
        format!("{method} {path}")
    };
    nerve_client::observe_request("hades-gate", &what, status, t0.elapsed());
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    stop
}

fn route(
    method: &str,
    path: &str,
    body: &[u8],
    approver: &Approver,
) -> (u16, serde_json::Value, bool) {
    match (method, path) {
        ("GET", "/health") => (200, status_json(approver), false),
        // The admin can always stop HADES. Stopping only removes the ability
        // to approve, so it needs no token (localhost only).
        ("POST", "/v1/stop") => (200, serde_json::json!({"ok": true, "stopping": true}), true),
        ("POST", "/v1/approve") => {
            let Ok(req) = serde_json::from_slice::<serde_json::Value>(body) else {
                return (
                    400,
                    serde_json::json!({"error": "body must be JSON"}),
                    false,
                );
            };
            let field = |k: &str| req.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
            let v = approver.approve(
                Request {
                    action: field("action"),
                    target: field("target"),
                    plan: field("plan").as_bytes(),
                    token: field("token"),
                },
                unix_now(),
            );
            let status = if matches!(v, Verdict::Refused(_)) {
                403
            } else {
                200
            };
            (status, verdict_json(&v), false)
        }
        ("POST", "/v1/approve-typed") => {
            // Option B prototype: `plan` must be a canonical typed plan.
            let Ok(req) = serde_json::from_slice::<serde_json::Value>(body) else {
                return (
                    400,
                    serde_json::json!({"error": "body must be JSON"}),
                    false,
                );
            };
            let field = |k: &str| req.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
            let v = approver.approve_typed(
                Request {
                    action: field("action"),
                    target: field("target"),
                    plan: field("plan").as_bytes(),
                    token: field("token"),
                },
                unix_now(),
            );
            let status = if matches!(v, Verdict::Refused(_)) {
                403
            } else {
                200
            };
            (status, verdict_json(&v), false)
        }
        _ => (404, serde_json::json!({"error": "not found"}), false),
    }
}

fn read_request(stream: &TcpStream) -> Result<(String, String, Vec<u8>), String> {
    const MAX_HEAD: usize = 16 * 1024;
    const MAX_BODY: usize = 128 * 1024;
    let mut reader = BufReader::new(stream.take((MAX_HEAD + MAX_BODY) as u64));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| e.kind().to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("no method")?.to_owned();
    let path = parts.next().ok_or("no path")?.to_owned();
    let mut len = 0usize;
    let mut head = line.len();
    loop {
        let mut h = String::new();
        let n = reader.read_line(&mut h).map_err(|e| e.kind().to_string())?;
        head += n;
        if n == 0 || head > MAX_HEAD {
            return Err("bad headers".into());
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().map_err(|_| "bad content-length")?;
            }
        }
    }
    if len > MAX_BODY {
        return Err("body too large".into());
    }
    let mut body = vec![0; len];
    reader
        .read_exact(&mut body)
        .map_err(|e| e.kind().to_string())?;
    Ok((method, path, body))
}
