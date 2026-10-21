// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! guardian-tick: report-only. Runs each probe once, writes a JSON report,
//! optionally prints suggestions as plain text. Changes nothing else.
//!
//! JSONL is off by default. Turn it on with `--jsonl PATH` or
//! `GUARDIAN_JSONL=PATH` (the flag wins if both are set). Each probe is then
//! one flat JSON object (`name`, `ok`, `detail`, `ts`) appended to that file.
//! `ts` is the probe stamp from `zagreb_time::now_rfc3339` (Europe/Zagreb,
//! never UTC). This writes a file; it is not a network client.
//!
//! Subcommands (see `heal.rs`): `heal` (probe, classify, plan, dry-run or
//! `--allow`-listed apply, incident store, nerve) and `classify` (offline
//! plan over a captured galion-health status.json). With no subcommand the
//! report-only behaviour above is unchanged.

mod heal;

use openmind_survival::{
    check_cloudflared, check_freshness, check_http, check_unit, parse_df, ProbeResult, Runner,
    SystemRunner,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct ProbeFile {
    #[serde(default)]
    probe: Vec<Spec>,
    #[serde(default)]
    repair: Vec<heal::RepairSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Spec {
    Http {
        name: String,
        port: u16,
        path: String,
        #[serde(default)]
        ok_codes: Vec<u16>,
    },
    Unit {
        name: Option<String>,
        unit: String,
    },
    Disk {
        path: String,
        #[serde(default = "d_warn")]
        warn: f64,
        #[serde(default = "d_crit")]
        crit: f64,
    },
    Mem,
    Load,
    Fresh {
        path: String,
        max_age_s: u64,
    },
    Cloudflared,
}

fn d_warn() -> f64 {
    85.0
}
fn d_crit() -> f64 {
    95.0
}

fn parse_file(text: &str) -> Result<ProbeFile, String> {
    let f: ProbeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if f.probe.is_empty() {
        return Err("no [[probe]] entries".into());
    }
    heal::validate(&f.repair)?;
    Ok(f)
}

fn parse_probes(text: &str) -> Result<Vec<Spec>, String> {
    parse_file(text).map(|f| f.probe)
}

fn run_http(name: &str, port: u16, path: &str, ok_codes: &[u16]) -> ProbeResult {
    let r = check_http(name, port, path);
    let code = r
        .detail
        .strip_prefix("status ")
        .and_then(|c| c.parse::<u16>().ok());
    match code {
        Some(c) => {
            let ok = if ok_codes.is_empty() {
                r.ok
            } else {
                ok_codes.contains(&c)
            };
            ProbeResult::new(name, ok, r.detail)
        }
        None => r,
    }
}

fn run_disk(path: &str, warn: f64, crit: f64) -> ProbeResult {
    let name = format!("disk:{path}");
    match SystemRunner.run("df", &["-P", path]) {
        Ok(out) => match parse_df(&out) {
            Some(p) => {
                let tag = if p >= crit {
                    "crit"
                } else if p >= warn {
                    "warn"
                } else {
                    "pass"
                };
                ProbeResult::new(&name, p < warn, format!("used {p:.1}% {tag}"))
            }
            None => ProbeResult::new(&name, false, "unparseable df output"),
        },
        Err(e) => ProbeResult::new(&name, false, e),
    }
}

fn run_spec(s: &Spec) -> ProbeResult {
    match s {
        Spec::Http {
            name,
            port,
            path,
            ok_codes,
        } => run_http(name, *port, path, ok_codes),
        Spec::Unit { name, unit } => {
            let r = check_unit(&SystemRunner, unit);
            match name {
                Some(n) => ProbeResult::new(n, r.ok, r.detail),
                None => r,
            }
        }
        Spec::Disk { path, warn, crit } => run_disk(path, *warn, *crit),
        Spec::Mem => match std::fs::read_to_string("/proc/meminfo") {
            Ok(t) => openmind_survival::check_meminfo(&t),
            Err(e) => ProbeResult::new("mem", false, e.to_string()),
        },
        Spec::Load => match std::fs::read_to_string("/proc/loadavg") {
            Ok(t) => {
                let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
                openmind_survival::check_loadavg(&t, cores)
            }
            Err(e) => ProbeResult::new("load", false, e.to_string()),
        },
        Spec::Fresh { path, max_age_s } => check_freshness(Path::new(path), *max_age_s),
        Spec::Cloudflared => check_cloudflared(Path::new("/proc")),
    }
}

fn report(results: &[ProbeResult]) -> Value {
    let rs: Vec<Value> = results
        .iter()
        .map(|r| json!({"name": r.name, "ok": r.ok, "detail": r.detail, "ts": r.ts}))
        .collect();
    json!({
        "ts": zagreb_time::now_rfc3339(),
        "host_ok": results.iter().all(|r| r.ok),
        "results": rs,
    })
}

/// Plain text only; never contains a command line or a path to run.
fn suggestion(r: &ProbeResult) -> String {
    let n = r.name.as_str();
    if let Some(u) = n.strip_prefix("unit:") {
        return format!("{u} not active: operator may restart {u}");
    }
    if n.starts_with("disk:") {
        return format!("{n} high: operator may free space or rotate logs");
    }
    if n.starts_with("fresh:") {
        return format!("{n} is stale: operator may check the writer service");
    }
    match n {
        "mem" => "memory high: operator may stop an idle heavy service".into(),
        "load" => "load high: operator may pause a batch job".into(),
        "cloudflared" => "cloudflared absent: operator may restart the tunnel".into(),
        _ => format!("{n} down: operator may restart {n}"),
    }
}

/// Append one JSON object per probe. `ts` is [`ProbeResult::ts`], which
/// `ProbeResult::new` sets with `zagreb_time::now_rfc3339`.
fn append_probe_jsonl(path: &Path, results: &[ProbeResult]) -> Result<(), String> {
    use std::io::Write;

    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut buf = String::new();
    for r in results {
        let line = serde_json::to_string(&json!({
            "name": r.name,
            "ok": r.ok,
            "detail": r.detail,
            "ts": r.ts,
        }))
        .map_err(|e| e.to_string())?;
        buf.push_str(&line);
        buf.push('\n');
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(buf.as_bytes()).map_err(|e| e.to_string())
}

struct Args {
    probes: Option<String>,
    out: String,
    suggest: bool,
    // None unless `--jsonl PATH` or a non-empty GUARDIAN_JSONL.
    jsonl: Option<String>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        probes: std::env::var("GUARDIAN_PROBES").ok(),
        out: "target/test-out/guardian.json".into(),
        suggest: false,
        jsonl: std::env::var("GUARDIAN_JSONL")
            .ok()
            .filter(|s| !s.is_empty()),
    };
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--probes" => a.probes = Some(it.next().ok_or("--probes needs a value")?.clone()),
            "--out" => a.out = it.next().ok_or("--out needs a value")?.clone(),
            "--suggest" => a.suggest = true,
            "--jsonl" => {
                let p = it.next().ok_or("--jsonl needs a value")?;
                if p.is_empty() {
                    return Err("--jsonl needs a value".into());
                }
                a.jsonl = Some(p.clone());
            }
            o => return Err(format!("unknown argument {o}")),
        }
    }
    Ok(a)
}

fn real_main() -> Result<(), (i32, String)> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("heal") => return heal_main(&argv[1..]),
        Some("classify") => return classify_main(&argv[1..]),
        _ => {}
    }
    let a = parse_args(&argv).map_err(|e| (2, e))?;
    let pf = a
        .probes
        .ok_or((2, "no probe file (--probes or GUARDIAN_PROBES)".to_string()))?;
    let text = std::fs::read_to_string(&pf).map_err(|e| (2, format!("{pf}: {e}")))?;
    let specs = parse_probes(&text).map_err(|e| (2, format!("{pf}: {e}")))?;
    let results: Vec<ProbeResult> = specs.iter().map(run_spec).collect();
    let rep = report(&results);
    let out = Path::new(&a.out);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| (1, e.to_string()))?;
    }
    let body = serde_json::to_string_pretty(&rep).map_err(|e| (1, e.to_string()))?;
    std::fs::write(out, body + "\n").map_err(|e| (1, format!("{}: {e}", a.out)))?;
    if let Some(path) = &a.jsonl {
        append_probe_jsonl(Path::new(path), &results).map_err(|e| (1, format!("{path}: {e}")))?;
    }
    let bad = results.iter().filter(|r| !r.ok).count();
    println!(
        "guardian-tick {} host_ok={} probes={} failed={}",
        rep["ts"].as_str().unwrap_or(""),
        bad == 0,
        results.len(),
        bad
    );
    if a.suggest {
        for r in results.iter().filter(|r| !r.ok) {
            println!("suggest: {}", suggestion(r));
        }
    }
    Ok(())
}

fn need(it: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<String, (i32, String)> {
    it.next()
        .filter(|v| !v.is_empty())
        .cloned()
        .ok_or((2, format!("{flag} needs a value")))
}

fn load_file(p: &str) -> Result<ProbeFile, (i32, String)> {
    let text = std::fs::read_to_string(p).map_err(|e| (2, format!("{p}: {e}")))?;
    parse_file(&text).map_err(|e| (2, format!("{p}: {e}")))
}

/// `guardian-tick heal --probes F [--state DIR] [--allow UNIT]... [--nerve P] [--cap N] [--out P]`
fn heal_main(argv: &[String]) -> Result<(), (i32, String)> {
    let mut probes = std::env::var("GUARDIAN_PROBES").ok();
    let mut a = heal::HealArgs {
        state: "target/test-out/guardian-heal".into(),
        nerve: None,
        allow: Default::default(),
        cap: openmind_survival::heal::REFLEX_CAP,
        out: None,
    };
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--probes" => probes = Some(need(&mut it, x)?),
            "--state" => a.state = need(&mut it, x)?.into(),
            "--nerve" => a.nerve = Some(need(&mut it, x)?.into()),
            "--out" => a.out = Some(need(&mut it, x)?.into()),
            "--allow" => {
                a.allow.insert(need(&mut it, x)?);
            }
            "--cap" => {
                a.cap = need(&mut it, x)?
                    .parse()
                    .map_err(|_| (2, "--cap needs a number".to_string()))?
            }
            o => return Err((2, format!("heal: unknown argument {o}"))),
        }
    }
    let pf = probes.ok_or((2, "no probe file (--probes or GUARDIAN_PROBES)".to_string()))?;
    let f = load_file(&pf)?;
    for u in &a.allow {
        if !f.repair.iter().any(|r| &r.unit == u) {
            return Err((2, format!("--allow {u}: no [[repair]] rule for that unit")));
        }
    }
    let rep = heal::heal(&f.probe, &f.repair, &a).map_err(|e| (1, e))?;
    heal::print_report(
        &rep,
        &format!(
            "guardian-tick heal {} mode={} cap={}/h",
            rep["ts"].as_str().unwrap_or(""),
            rep["mode"].as_str().unwrap_or(""),
            rep["cap"]
        ),
    );
    Ok(())
}

/// `guardian-tick classify --status-json F [--probes F] [--out P]`: never applies.
fn classify_main(argv: &[String]) -> Result<(), (i32, String)> {
    let (mut status, mut probes, mut out) = (None, None, None);
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--status-json" => status = Some(need(&mut it, x)?),
            "--probes" => probes = Some(need(&mut it, x)?),
            "--out" => out = Some(need(&mut it, x)?),
            o => return Err((2, format!("classify: unknown argument {o}"))),
        }
    }
    let sp = status.ok_or((2, "classify needs --status-json FILE".to_string()))?;
    let text = std::fs::read_to_string(&sp).map_err(|e| (2, format!("{sp}: {e}")))?;
    let f = probes.map(|p| load_file(&p)).transpose()?;
    let rep = heal::classify_status(&text, f.as_ref().map(|f| f.repair.as_slice()))
        .map_err(|e| (2, e))?;
    if let Some(o) = out {
        let body = serde_json::to_string_pretty(&rep).map_err(|e| (1, e.to_string()))?;
        std::fs::write(&o, body + "\n").map_err(|e| (1, format!("{o}: {e}")))?;
    }
    heal::print_report(
        &rep,
        &format!(
            "guardian-tick classify {} source_checked_at={} source_ok={} mode=classify-only (nothing applied)",
            rep["ts"].as_str().unwrap_or(""),
            rep["source_checked_at"].as_str().unwrap_or(""),
            rep["source_ok"]
        ),
    );
    Ok(())
}

fn main() {
    if let Err((code, msg)) = real_main() {
        eprintln!("guardian-tick: {msg}");
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_kinds() {
        let t = "[[probe]]\nkind=\"http\"\nname=\"a\"\nport=1\npath=\"/\"\n[[probe]]\nkind=\"mem\"\n[[probe]]\nkind=\"fresh\"\npath=\"/x\"\nmax_age_s=5\n";
        assert_eq!(parse_probes(t).unwrap().len(), 3);
    }

    #[test]
    fn rejects_bad() {
        assert!(parse_probes("kind = ").is_err());
        assert!(parse_probes("[[probe]]\nkind=\"nope\"\n").is_err());
        assert!(parse_probes("").is_err());
    }

    #[test]
    fn suggestion_has_no_path() {
        let r = ProbeResult::new("console", false, "x");
        let s = suggestion(&r);
        assert_eq!(s, "console down: operator may restart console");
        assert!(!s.contains('/'));
    }

    #[test]
    fn jsonl_flag_sets_path() {
        let a = parse_args(&["--jsonl".into(), "events.jsonl".into()]).unwrap();
        assert_eq!(a.jsonl.as_deref(), Some("events.jsonl"));
        match parse_args(&["--jsonl".into()]) {
            Err(err) => assert!(err.contains("--jsonl"), "{err}"),
            Ok(_) => panic!("--jsonl without a value must fail"),
        }
    }
}
