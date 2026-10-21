// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-flight preflight|postflight <service> [options]
//!
//! Read-only go/no-go checks. Exit 0 = GO, 1 = NO-GO, 2 = usage/manifest.
//! See docs/ops/flight.md.

use galion_flight::manifest::Manifest;
use galion_flight::target::{Local, Target, Vps};
use galion_flight::{render, run, Options, Phase, BUILTIN};
use std::collections::BTreeSet;
use std::process::ExitCode;

const USAGE: &str = "usage: galion-flight preflight|postflight <service> [--target vps|local] \
[--host HOST] [--manifest FILE] [--set key=value]... [--pid PID] [--since-unix SECS] [--no-public] \
[--show-commands]\n\
       galion-flight list | --version | --help\n\
services: nerve memory hermes-rs galion-one tuner cloudflared\n\
Every check is one read (systemctl show / readlink / sha256sum / stat / ss / HTTP GET);\n\
no loops, no retries, no POST, no writes. Exit 0 GO, 1 NO-GO, 2 usage.\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match real_main(&args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("galion-flight: {e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn real_main(args: &[String]) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            print!("{USAGE}");
            return Ok(0);
        }
        Some("--version") | Some("-V") => {
            println!("galion-flight {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        Some("list") => {
            for (n, _) in BUILTIN {
                println!("{n}");
            }
            return Ok(0);
        }
        _ => {}
    }
    let phase = match args[0].as_str() {
        "preflight" => Phase::Pre,
        "postflight" => Phase::Post,
        o => return Err(format!("unknown command {o:?}")),
    };
    let svc = args.get(1).ok_or("missing service")?;
    let mut target = "vps".to_string();
    let mut host = "slavik-vps".to_string();
    let mut manifest_file: Option<String> = None;
    let mut sets: Vec<String> = Vec::new();
    let mut pid: Option<u32> = None;
    let mut since: Option<i64> = None;
    let mut public = true;
    let mut show = false;
    let mut it = args[2..].iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--target" => target = val()?,
            "--host" => host = val()?,
            "--manifest" => manifest_file = Some(val()?),
            "--set" => sets.push(val()?),
            "--pid" => pid = Some(val()?.parse().map_err(|_| "bad --pid")?),
            "--since-unix" => since = Some(val()?.parse().map_err(|_| "bad --since-unix")?),
            "--no-public" => public = false,
            "--show-commands" => show = true,
            o => return Err(format!("unknown option {o:?}")),
        }
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("bad --host".into());
    }
    let text = match &manifest_file {
        Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
        None => BUILTIN
            .iter()
            .find(|(n, _)| n == svc)
            .map(|(_, t)| t.to_string())
            .ok_or(format!("unknown service {svc:?} (see: galion-flight list)"))?,
    };
    let mut m = Manifest::parse(&text)?;
    let mut seen = BTreeSet::new();
    for s in &sets {
        let key = s.split('=').next().unwrap_or("").trim().to_string();
        let first = seen.insert(key);
        m.set_line(s, first)?;
    }
    m.validate()?;
    if &m.service != svc {
        return Err(format!("manifest is for {:?}, not {svc:?}", m.service));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let opt = Options { since, public, now };
    let at = galion_common::time::now_rfc3339();
    let (rep, label, sent) = match target.as_str() {
        "vps" => {
            let mut t = Vps::new(&host);
            let rep = run(phase, &m, &mut t, &opt);
            t.close();
            let l = t.label();
            (rep, l, t.sent)
        }
        "local" => {
            let mut t = Local { pid };
            let rep = run(phase, &m, &mut t, &opt);
            (rep, t.label(), Vec::new())
        }
        o => return Err(format!("unknown --target {o:?}")),
    };
    print!("{}", render(phase, &m, &label, &at, &rep));
    if show {
        for c in &sent {
            println!("SENT {c}");
        }
    }
    Ok(if rep.go() { 0 } else { 1 })
}
