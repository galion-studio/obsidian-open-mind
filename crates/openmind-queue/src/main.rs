// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `openmind-queue`: the POSEIDON queue service, its CLI client and a
//! built-in test worker. See `docs/poseidon.md`.

use openmind_queue::engine::Config;
use openmind_queue::{http, now_ms, server, zagreb_ms};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str =
    "openmind-queue (POSEIDON): durable job queue, quotas, spend caps (numbers only), VRAM routing

  serve   --bind 127.0.0.1:19120 --data DIR [--providers registry.json] [--lease-ms N]
          [--max-attempts N] [--backoff-base-ms N] [--backoff-max-ms N] [--max-depth N]
          [--compact-min N] [--allow-remote]
  submit  [--queue Q] [--agent A] [--kind K] [--priority P] [--max-attempts N] [--cost C]
          [--lease-ms N] [--vram-gb G] [--cpu-fallback] [--payload TEXT]
  lease   --worker W [--queue Q] [--lease-ms N] [--wait-ms N]
  ack     --id N --token T [--cost C]
  nack    --id N --token T [--cost C] [--error TEXT]
  extend  --id N --token T [--lease-ms N]
  redrive --id N
  get PATH | post PATH JSON | delete PATH
  worker  --worker W [--queue Q] [--max-jobs N] [--idle-exit-ms N]
          kinds: echo, sleep (payload ms), fail, flaky (ok from attempt payload),
                 hang (payload ms, no ack), crash (exit mid-lease),
                 probe (payload = http://127.0.0.1:PORT/path, loopback GET only)
  version

Client commands talk to --addr (default $POSEIDON_ADDR or 127.0.0.1:19120).";

struct Args {
    flags: HashMap<String, String>,
    pos: Vec<String>,
}

impl Args {
    fn parse(v: &[String]) -> Args {
        let mut flags = HashMap::new();
        let mut pos = Vec::new();
        let mut i = 0;
        while i < v.len() {
            if let Some(k) = v[i].strip_prefix("--") {
                if i + 1 < v.len() && !v[i + 1].starts_with("--") {
                    flags.insert(k.to_string(), v[i + 1].clone());
                    i += 2;
                } else {
                    flags.insert(k.to_string(), "true".into());
                    i += 1;
                }
            } else {
                pos.push(v[i].clone());
                i += 1;
            }
        }
        Args { flags, pos }
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.get(k).map(|s| s.as_str())
    }
    fn num<T: std::str::FromStr>(&self, k: &str, d: T) -> Result<T, String> {
        match self.get(k) {
            None => Ok(d),
            Some(v) => v.parse().map_err(|_| format!("--{k}: not a number: {v}")),
        }
    }
    fn addr(&self) -> String {
        self.get("addr")
            .map(String::from)
            .or_else(|| std::env::var("POSEIDON_ADDR").ok())
            .unwrap_or_else(|| "127.0.0.1:19120".into())
    }
}

fn call(addr: &str, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
    http::request(addr, method, path, body, Duration::from_secs(40))
        .map_err(|e| format!("{method} {path} at {addr}: {e}"))
}

fn print_call(addr: &str, method: &str, path: &str, body: Option<&str>) -> ExitCode {
    match call(addr, method, path, body) {
        Ok((code, b)) => {
            println!("{code} {b}");
            if code < 400 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn serve(a: &Args) -> Result<(), String> {
    let d = Config::default();
    let cfg = Config {
        default_lease_ms: a.num("lease-ms", d.default_lease_ms)?,
        default_max_attempts: a.num("max-attempts", d.default_max_attempts)?,
        backoff_base_ms: a.num("backoff-base-ms", d.backoff_base_ms)?,
        backoff_max_ms: a.num("backoff-max-ms", d.backoff_max_ms)?,
        max_depth: a.num("max-depth", d.max_depth)?,
        ..d
    };
    let opts = server::Options {
        bind: a.get("bind").unwrap_or("127.0.0.1:19120").to_string(),
        data: PathBuf::from(a.get("data").ok_or("--data DIR is required")?),
        cfg,
        compact_min: a.num("compact-min", 10_000)?,
        providers: a.get("providers").map(PathBuf::from),
        allow_remote: a.get("allow-remote").is_some(),
        ..server::Options::default()
    };
    let data = opts.data.clone();
    let h = server::start(opts).map_err(|e| format!("start: {e}"))?;
    let stats = call(&h.addr.to_string(), "GET", "/stats", None)
        .ok()
        .and_then(|(_, b)| serde_json::from_str::<Value>(&b).ok())
        .unwrap_or(Value::Null);
    println!(
        "{} openmind-queue POSEIDON listening on {} data {} replayed {} truncated_bytes {} live {}",
        zagreb_ms(now_ms()),
        h.addr,
        data.display(),
        stats["store"]["replayed"],
        stats["store"]["truncated_bytes"],
        stats["depth"],
    );
    let _ = std::io::stdout().flush();
    h.join();
    Ok(())
}

fn submit(a: &Args) -> Result<String, String> {
    let mut spec = json!({
        "queue": a.get("queue").unwrap_or("default"),
        "agent": a.get("agent").unwrap_or("anon"),
        "kind": a.get("kind").unwrap_or("echo"),
        "priority": a.num::<i32>("priority", 0)?,
        "max_attempts": a.num::<u32>("max-attempts", 0)?,
        "cost": a.num::<u64>("cost", 0)?,
        "lease_ms": a.num::<u64>("lease-ms", 0)?,
        "vram_gb": a.num::<u32>("vram-gb", 0)?,
        "payload": a.get("payload").unwrap_or(""),
    });
    if a.get("cpu-fallback").is_some() {
        spec["cpu_fallback"] = json!(true);
    }
    Ok(spec.to_string())
}

fn token_body(a: &Args) -> Result<String, String> {
    let mut b = json!({ "token": a.num::<u64>("token", 0)? });
    if let Some(c) = a.get("cost") {
        b["cost"] = json!(c.parse::<u64>().map_err(|_| "--cost")?);
    }
    if let Some(e) = a.get("error") {
        b["error"] = json!(e);
    }
    if let Some(l) = a.get("lease-ms") {
        b["lease_ms"] = json!(l.parse::<u64>().map_err(|_| "--lease-ms")?);
    }
    Ok(b.to_string())
}

/// The loopback-only rule for the probe kind: read-only GETs to this box.
fn loopback_url(url: &str) -> Option<(String, String)> {
    let (hp, path) = http::split_url(url)?;
    let host = hp.rsplit_once(':').map_or(hp.as_str(), |(h, _)| h);
    if ["127.0.0.1", "localhost", "[::1]"].contains(&host) {
        Some((hp, path))
    } else {
        None
    }
}

fn worker(a: &Args) -> Result<(), String> {
    let addr = a.addr();
    let name = a
        .get("worker")
        .ok_or("--worker NAME is required")?
        .to_string();
    let queue = a.get("queue").unwrap_or("default").to_string();
    let max_jobs: u64 = a.num("max-jobs", u64::MAX)?;
    let idle_exit: u64 = a.num("idle-exit-ms", 0)?;
    let mut done = 0u64;
    let mut idle_since = now_ms();
    let out = |line: String| {
        println!("{} worker={name} {line}", zagreb_ms(now_ms()));
        let _ = std::io::stdout().flush();
    };
    while done < max_jobs {
        let body = json!({"queue": queue, "worker": name, "wait_ms": 1000}).to_string();
        let (_, b) = call(&addr, "POST", "/lease", Some(&body))?;
        let v: Value = serde_json::from_str(&b).map_err(|e| format!("lease reply: {e}"))?;
        if v["id"].is_null() {
            if idle_exit > 0 && now_ms().saturating_sub(idle_since) >= idle_exit {
                out(format!(
                    "idle exit after {done} jobs (last miss {})",
                    v["miss"]
                ));
                return Ok(());
            }
            continue;
        }
        idle_since = now_ms();
        let id = v["id"].as_u64().unwrap_or(0);
        let token = v["token"].as_u64().unwrap_or(0);
        let j = &v["job"];
        let kind = j["spec"]["kind"].as_str().unwrap_or("").to_string();
        let payload = j["spec"]["payload"].as_str().unwrap_or("").to_string();
        let attempt = j["attempts"].as_u64().unwrap_or(0);
        let head = format!(
            "job={id} kind={kind} agent={} prio={} attempt={attempt} device={}",
            j["spec"]["agent"].as_str().unwrap_or(""),
            j["spec"]["priority"],
            v["device"]
                .as_str()
                .filter(|d| !d.is_empty())
                .unwrap_or("-"),
        );
        let ms = || payload.trim().parse::<u64>().unwrap_or(100).min(60_000);
        let (ok, note) = match kind.as_str() {
            "echo" => (true, format!("echo {payload}")),
            "sleep" => {
                std::thread::sleep(Duration::from_millis(ms()));
                (true, format!("slept {}ms", ms()))
            }
            "fail" => (false, "failed on purpose".to_string()),
            "flaky" => {
                let ok_at = payload.trim().parse::<u64>().unwrap_or(2);
                (attempt >= ok_at, format!("flaky ok_at={ok_at}"))
            }
            "hang" => {
                out(format!(
                    "{head} result=hang sleeping {}ms without ack",
                    ms()
                ));
                std::thread::sleep(Duration::from_millis(ms()));
                done += 1;
                continue;
            }
            "crash" => {
                out(format!("{head} result=crash exiting mid-lease"));
                std::process::exit(86);
            }
            "probe" => match loopback_url(&payload) {
                None => (
                    false,
                    "probe refused: loopback http:// URLs only".to_string(),
                ),
                Some((hp, path)) => {
                    match http::request(&hp, "GET", &path, None, Duration::from_secs(3)) {
                        Ok((code, body)) => (
                            true,
                            format!("probe GET {payload} -> {code} ({} bytes)", body.len()),
                        ),
                        Err(e) => (false, format!("probe GET {payload} -> error {e}")),
                    }
                }
            },
            other => (false, format!("unknown kind {other}")),
        };
        let (path, body) = if ok {
            (format!("/jobs/{id}/ack"), json!({"token": token}))
        } else {
            (
                format!("/jobs/{id}/nack"),
                json!({"token": token, "error": note}),
            )
        };
        let (code, reply) = call(&addr, "POST", &path, Some(&body.to_string()))?;
        out(format!(
            "{head} result={} note=\"{note}\" reply={code} {reply}",
            if ok { "ack" } else { "nack" }
        ));
        done += 1;
    }
    out(format!("max-jobs reached ({done})"));
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let a = Args::parse(&argv[1..]);
    let addr = a.addr();
    let res: Result<ExitCode, String> = (|| {
        Ok(match cmd.as_str() {
            "serve" => {
                serve(&a)?;
                ExitCode::SUCCESS
            }
            "submit" => print_call(&addr, "POST", "/jobs", Some(&submit(&a)?)),
            "lease" => {
                let b = json!({
                    "queue": a.get("queue").unwrap_or("default"),
                    "worker": a.get("worker").ok_or("--worker is required")?,
                    "lease_ms": a.num::<u64>("lease-ms", 0)?,
                    "wait_ms": a.num::<u64>("wait-ms", 0)?,
                });
                print_call(&addr, "POST", "/lease", Some(&b.to_string()))
            }
            "ack" | "nack" | "extend" => {
                let id: u64 = a.num("id", 0)?;
                print_call(
                    &addr,
                    "POST",
                    &format!("/jobs/{id}/{cmd}"),
                    Some(&token_body(&a)?),
                )
            }
            "redrive" => {
                let id: u64 = a.num("id", 0)?;
                print_call(&addr, "POST", &format!("/jobs/{id}/redrive"), None)
            }
            "get" | "delete" => {
                let p = a.pos.first().ok_or("PATH is required")?;
                print_call(&addr, &cmd.to_ascii_uppercase(), p, None)
            }
            "post" => {
                let p = a.pos.first().ok_or("PATH is required")?;
                let b = a.pos.get(1).map(String::as_str).unwrap_or("{}");
                print_call(&addr, "POST", p, Some(b))
            }
            "worker" => {
                worker(&a)?;
                ExitCode::SUCCESS
            }
            "version" | "--version" => {
                println!("openmind-queue {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
            "help" | "--help" | "-h" => {
                println!("{USAGE}");
                ExitCode::SUCCESS
            }
            other => return Err(format!("unknown command {other}\n\n{USAGE}")),
        })
    })();
    match res {
        Ok(c) => c,
        Err(e) => {
            eprintln!("openmind-queue: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_is_loopback_only() {
        assert!(loopback_url("http://127.0.0.1:8420/health").is_some());
        assert!(loopback_url("http://localhost:19120/healthz").is_some());
        assert!(loopback_url("http://10.0.0.1:80/").is_none());
        assert!(loopback_url("http://example.com/").is_none());
        assert!(loopback_url("https://127.0.0.1/").is_none());
    }

    #[test]
    fn args_parse_flags_and_positionals() {
        let v: Vec<String> = ["post", "/caps", "--addr", "x:1", "--cpu-fallback"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let a = Args::parse(&v);
        assert_eq!(a.pos, vec!["post", "/caps"]);
        assert_eq!(a.get("addr"), Some("x:1"));
        assert_eq!(a.get("cpu-fallback"), Some("true"));
    }
}
