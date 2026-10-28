// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! `tunnel-lint`: flag cloudflared ingress routes that point at closed local ports.
//!
//! Read-only. It reads the ingress config (never the credentials file it names),
//! checks listeners live (TCP connect on loopback) or from a captured
//! `ss -ltnp` text, and can write a staged config with dead routes commented out.
//!
//! Exit codes: 0 clean, 1 findings at error level, 2 usage / read / parse error.

use std::process::ExitCode;
use std::time::Duration;
use tunnel_lint::{lint, parse, stage, Listeners, Origin, Probe, Severity, SsSnapshot, Status};

const USAGE: &str =
    "usage: tunnel-lint [--ss FILE] [--timeout-ms N] [--stage OUT] [--tsv] [--no-fail] CONFIG.yml

  --ss FILE        use captured `ss -ltn[p]` output instead of live TCP probes
  --timeout-ms N   live probe connect timeout (default 300)
  --stage OUT      write a copy of CONFIG with closed-port rules commented out
  --tsv            tab-separated rule table (rule, hostname, path, service, port, status, owner)
  --no-fail        exit 0 even when routes are closed";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (mut ss, mut stage_out, mut cfg) = (None, None, None);
    let (mut timeout, mut tsv, mut no_fail) = (300u64, false, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--ss" => ss = args.next(),
            "--stage" => stage_out = args.next(),
            "--timeout-ms" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => timeout = v,
                None => return usage("--timeout-ms needs a number"),
            },
            "--tsv" => tsv = true,
            "--no-fail" => no_fail = true,
            s if s.starts_with('-') => return usage(&format!("unknown flag {s}")),
            s => cfg = Some(s.to_string()),
        }
    }
    let Some(cfg) = cfg else {
        return usage("missing CONFIG");
    };
    // Guard: this tool must never be pointed at tunnel credentials.
    let base = std::path::Path::new(&cfg)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if base.ends_with(".json") || base.ends_with(".pem") {
        eprintln!("tunnel-lint: refusing to read {base}: credentials are never read");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(&cfg) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("tunnel-lint: cannot read {cfg}: {e}");
            return ExitCode::from(2);
        }
    };
    let rules = match parse(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tunnel-lint: {cfg}: {e}");
            return ExitCode::from(2);
        }
    };
    let snap;
    let probe = Probe {
        timeout: Duration::from_millis(timeout),
    };
    let listeners: &dyn Listeners = match &ss {
        Some(f) => match std::fs::read_to_string(f) {
            Ok(t) => {
                snap = SsSnapshot::parse(&t);
                &snap
            }
            Err(e) => {
                eprintln!("tunnel-lint: cannot read {f}: {e}");
                return ExitCode::from(2);
            }
        },
        None => &probe,
    };
    let mut report = lint(&rules, listeners);
    if let Some(mode) = file_mode(&cfg) {
        if tunnel_lint::world_writable(mode) {
            report.findings.push(tunnel_lint::Finding {
                severity: Severity::Warn,
                rule: None,
                msg: format!(
                    "{cfg} is world-writable (mode {:o}): any local user can reroute the tunnel",
                    mode & 0o7777
                ),
            });
        }
    }

    if tsv {
        println!("rule\thostname\tpath\tservice\tport\tstatus\towner");
    }
    for r in &report.rules {
        let port = match &r.origin {
            Origin::Local { port, .. } | Origin::Remote { port, .. } => port.to_string(),
            _ => "-".into(),
        };
        let (st, owner) = match &r.status {
            Status::Open(o) => ("OPEN", o.as_str()),
            Status::Closed => ("CLOSED", ""),
            Status::NotChecked(why) => ("SKIP", why.as_str()),
        };
        let host = r.rule.hostname.as_deref().unwrap_or("*");
        let path = r.rule.path.as_deref().unwrap_or("");
        if tsv {
            println!(
                "{}\t{host}\t{path}\t{}\t{port}\t{st}\t{owner}",
                r.rule.index, r.rule.service
            );
        } else {
            let hp = if path.is_empty() {
                host.to_string()
            } else {
                format!("{host} {path}")
            };
            println!(
                "{:>3} {:<6} {:<38} {:<28} {}",
                r.rule.index, st, hp, r.rule.service, owner
            );
        }
    }
    let closed: Vec<_> = report.closed().collect();
    let ports = report.closed_ports();
    if !tsv {
        for f in &report.findings {
            let sev = match f.severity {
                Severity::Error => "error",
                Severity::Warn => "warn",
                Severity::Info => "info",
            };
            match f.rule {
                Some(i) => println!("{sev}: rule {i}: {}", f.msg),
                None => println!("{sev}: {}", f.msg),
            }
        }
        println!(
            "summary: {} rules, {} closed routes on {} ports {:?}, {} findings",
            report.rules.len(),
            closed.len(),
            ports.len(),
            ports,
            report.findings.len()
        );
        if text
            .lines()
            .any(|l| l.trim_start().starts_with("credentials-file:"))
        {
            println!("note: credentials-file is referenced; tunnel-lint did not read it");
        }
    }
    if let Some(out) = stage_out {
        if let Err(e) = std::fs::write(&out, stage(&text, &report)) {
            eprintln!("tunnel-lint: cannot write {out}: {e}");
            return ExitCode::from(2);
        }
        eprintln!("tunnel-lint: staged config written to {out} (not applied)");
    }
    if report.has_errors() && !no_fail {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("tunnel-lint: {msg}\n{USAGE}");
    ExitCode::from(2)
}

#[cfg(unix)]
fn file_mode(p: &str) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).ok().map(|m| m.permissions().mode())
}

#[cfg(not(unix))]
fn file_mode(_: &str) -> Option<u32> {
    None
}
