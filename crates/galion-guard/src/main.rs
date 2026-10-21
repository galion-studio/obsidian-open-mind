// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! `galion-guard` CLI.

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;

use galion_guard::compare::{self, Options as CmpOptions};
use galion_guard::config::{self, Config, Effective};
use galion_guard::daemon;
use galion_guard::engine::{Engine, Event};
use galion_guard::f2blog::{self, ShadowLog, Zone};
use galion_guard::filter::SshdFilter;
use galion_guard::jail::{Jail, Protect};
use galion_guard::nft::DryRun;
use galion_guard::session::{Outcome, Sessions};
use galion_guard::source::LogLine;
use galion_guard::state::now_secs;
use galion_guard::timefmt;

/// Like println!, but a closed pipe (`| head`) ends quietly instead of panicking.
macro_rules! outln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        if writeln!(std::io::stdout().lock(), $($t)*).is_err() {
            std::process::exit(0);
        }
    }};
}
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        if write!(std::io::stdout().lock(), $($t)*).is_err() {
            std::process::exit(0);
        }
    }};
}

const USAGE: &str = "galion-guard: Rust SSH ban daemon (fail2ban sshd jail replacement)

usage:
  galion-guard run          [--config PATH]      run the daemon
  galion-guard status       [--json] [--config PATH]
  galion-guard unban        <ip>|--all [--config PATH]
  galion-guard ban          <ip> [duration] [--config PATH]
  galion-guard stop         [--config PATH]      save state and exit (systemd ExecStop)
  galion-guard check-config [--config PATH]
  galion-guard test-filter  <file|-> [--show]    like fail2ban-regex for the sshd filter
  galion-guard replay       <file|-> [--config PATH] [--json] [--drain] [--shadow-log PATH] [--shadow-zone zagreb|utc]
                                                 replay a log with its own timestamps (dry-run);
                                                 --shadow-log also writes it in fail2ban format
  galion-guard compare      <fail2ban.log> <shadow.log> [--fail2ban-zone utc|zagreb] [--shadow-zone zagreb|utc]
                            [--tolerance SECS] [--since RFC3339] [--until RFC3339] [--mask] [--timeline] [--json]
                                                 side-by-side report: would galion-guard have done what
                                                 fail2ban did? exit 0 = parity, 3 = differences
  galion-guard teardown     [--config PATH]      remove the nft table (lifts all bans)
                                                 (shadow backend: nothing to remove, nft never runs)
  galion-guard version

default config: /etc/galion-guard/guard.toml (missing file = built-in VPS defaults)
backends: nft | dry-run (logs nft scripts) | shadow (log-only, fail2ban-format log, never touches nft)";

struct Args {
    cmd: String,
    pos: Vec<String>,
    config: Option<PathBuf>,
    json: bool,
    show: bool,
    drain: bool,
    mask: bool,
    timeline: bool,
    opt: std::collections::BTreeMap<String, String>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_else(|| "help".into());
    let mut a = Args {
        cmd,
        pos: vec![],
        config: None,
        json: false,
        show: false,
        drain: false,
        mask: false,
        timeline: false,
        opt: Default::default(),
    };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--config" | "-c" => a.config = Some(it.next().ok_or("--config needs a path")?.into()),
            "--json" => a.json = true,
            "--show" => a.show = true,
            "--drain" => a.drain = true,
            "--mask" => a.mask = true,
            "--timeline" => a.timeline = true,
            "--shadow-log" | "--shadow-zone" | "--fail2ban-zone" | "--tolerance" | "--since"
            | "--until" | "--jail" => {
                let v = it.next().ok_or(format!("{x} needs a value"))?;
                a.opt.insert(x.trim_start_matches('-').to_string(), v);
            }
            _ => a.pos.push(x),
        }
    }
    Ok(a)
}

fn load_config(p: &Option<PathBuf>) -> Result<Effective, String> {
    match p {
        Some(p) => Config::load(p),
        None => {
            let d = PathBuf::from(config::DEFAULT_PATH);
            if d.exists() {
                Config::load(&d)
            } else {
                Config::default().validate()
            }
        }
    }
}

fn open_input(p: &str) -> Result<Box<dyn BufRead>, String> {
    if p == "-" {
        Ok(Box::new(std::io::BufReader::new(std::io::stdin())))
    } else {
        let f = std::fs::File::open(p).map_err(|e| format!("open {p}: {e}"))?;
        Ok(Box::new(std::io::BufReader::new(f)))
    }
}

fn lines(r: Box<dyn BufRead>) -> impl Iterator<Item = String> {
    r.split(b'\n')
        .map_while(Result::ok)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn test_filter(a: &Args) -> Result<(), String> {
    let p = a.pos.first().ok_or("test-filter needs a file")?;
    let f = SshdFilter::new();
    let mut s = Sessions::new();
    let now = now_secs();
    let mut per_rule = vec![0u64; f.rule_count()];
    let (mut total, mut matched, mut failures, mut accepted, mut unresolved) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut ips = std::collections::BTreeSet::new();
    for line in lines(open_input(p)?) {
        total += 1;
        let (t, text) = match timefmt::split_stamp(&line, now, 0) {
            Some((t, r)) => (t, r.to_string()),
            None => (now, line.clone()),
        };
        let Some(m) = f.matches(&text) else { continue };
        matched += 1;
        per_rule[m.rule] += 1;
        match s.feed(&m, t) {
            Outcome::Failure { ip, rule } => {
                failures += 1;
                ips.insert(ip);
                if a.show {
                    outln!("FAIL rule {rule:>2} {ip}: {line}");
                }
            }
            Outcome::Accepted { .. } => accepted += 1,
            Outcome::Unresolved { .. } => unresolved += 1,
            Outcome::Nothing => {
                if a.show {
                    outln!("     rule {:>2} (no count): {line}", m.rule);
                }
            }
        }
    }
    outln!("Lines: {total}, matched: {matched}, failures: {failures} from {} addresses, accepted logins: {accepted}, non-IP hosts: {unresolved}", ips.len());
    outln!("Matches per failregex:");
    for (i, n) in per_rule.iter().enumerate() {
        if *n > 0 {
            outln!("  [{i:>2}] {n}");
        }
    }
    Ok(())
}

fn replay(a: &Args) -> Result<(), String> {
    let p = a.pos.first().ok_or("replay needs a file")?;
    let e = load_config(&a.config)?;
    let jail = Jail::new(
        e.params.clone(),
        Protect {
            allow: e.allow.clone(),
            self_addrs: vec![],
        },
    );
    let mut eng = Engine::new(jail, Box::new(DryRun::new(None, None)), None);
    eng.record = true;
    if let Some(p) = a.opt.get("shadow-log") {
        let zone = Zone::parse(a.opt.get("shadow-zone").map_or("zagreb", |s| s.as_str()))?;
        eng.shadow = Some(ShadowLog::new(p.into(), zone));
        eng.emit_found = true;
    }
    eng.bsd_offset = e.cfg.sshd.bsd_utc_offset_minutes * 60;
    let wall = now_secs();
    eng.startup(wall)?;
    eng.events.clear();
    let mut last = i64::MIN;
    for line in lines(open_input(p)?) {
        let Some((t, _)) = timefmt::split_stamp(&line, wall, eng.bsd_offset) else {
            continue;
        };
        let t = t.max(last); // simulated clock never runs backwards
        if last != i64::MIN {
            for s in last + 1..=t.min(last + 7 * 86_400) {
                eng.tick(s);
            }
        }
        last = t;
        eng.handle_line(
            &LogLine {
                time: None,
                text: line,
                cursor: None,
                file_pos: None,
            },
            t,
        );
    }
    if a.drain && last != i64::MIN {
        let end = eng.jail.bans().map(|b| b.end).max().unwrap_or(last);
        let mut s = last;
        while s < end {
            s += 1;
            eng.tick(s);
        }
    }
    for ev in &eng.events {
        if a.json {
            outln!("{}", serde_json::to_string(ev).unwrap_or_default());
            continue;
        }
        match ev {
            Event::Found { at, ip, logged } => outln!(
                "{} FOUND {ip} (logged {})",
                zagreb_time::from_unix(*at),
                zagreb_time::from_unix(*logged)
            ),
            Event::Restored { at, ip, until } => outln!(
                "{} RESTORED {ip} until {}",
                zagreb_time::from_unix(*at),
                zagreb_time::from_unix(*until)
            ),
            Event::Ban {
                ip,
                at,
                until,
                count,
                failures,
                ..
            } => outln!(
                "{} BAN   {ip} until {} (ban #{count}, {failures} failures, {})",
                zagreb_time::from_unix(*at),
                zagreb_time::from_unix(*until),
                galion_guard::duration::format((until - at) as u64)
            ),
            Event::Unban { ip, at, reason } => {
                outln!("{} UNBAN {ip} ({reason})", zagreb_time::from_unix(*at))
            }
            Event::Protected { at, reason } => {
                outln!(
                    "{} PROTECTED failure ignored ({reason:?})",
                    zagreb_time::from_unix(*at)
                )
            }
            Event::Unresolved { at, rule } => {
                outln!(
                    "{} NON-IP host ignored (rule {rule})",
                    zagreb_time::from_unix(*at)
                )
            }
            Event::BackendError { at, error } => {
                outln!("{} FIREWALL ERROR {error}", zagreb_time::from_unix(*at))
            }
        }
    }
    eprintln!(
        "replay: {} lines, {} matched, {} bans, {} still banned at the end",
        eng.lines_seen,
        eng.lines_matched,
        eng.jail.totals.banned,
        eng.jail.bans().count()
    );
    Ok(())
}

fn compare(a: &Args) -> Result<(), String> {
    let (Some(fp), Some(gp)) = (a.pos.first(), a.pos.get(1)) else {
        return Err("compare needs <fail2ban.log> <shadow.log>".into());
    };
    let read = |p: &str| -> Result<String, String> {
        let mut s = String::new();
        let mut r = open_input(p)?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut r, &mut buf).map_err(|e| format!("read {p}: {e}"))?;
        s.push_str(&String::from_utf8_lossy(&buf));
        Ok(s)
    };
    if fp == "-" && gp == "-" {
        return Err("only one input can be stdin".into());
    }
    let fz = Zone::parse(a.opt.get("fail2ban-zone").map_or("utc", |s| s.as_str()))?;
    let gz = Zone::parse(a.opt.get("shadow-zone").map_or("zagreb", |s| s.as_str()))?;
    let jail = a.opt.get("jail").map_or("sshd", |s| s.as_str());
    let f = f2blog::parse_log(&read(fp)?, fz, jail);
    let g = f2blog::parse_log(&read(gp)?, gz, jail);
    let t = |k: &str| -> Result<Option<i64>, String> {
        a.opt
            .get(k)
            .map(|v| timefmt::parse_rfc3339(v).ok_or(format!("--{k}: not RFC 3339: {v}")))
            .transpose()
    };
    let o = CmpOptions {
        tolerance: a
            .opt
            .get("tolerance")
            .map(|v| {
                v.parse::<i64>()
                    .map_err(|_| "--tolerance: seconds".to_string())
            })
            .transpose()?
            .unwrap_or(5)
            .max(0),
        since: t("since")?,
        until: t("until")?,
        mask: a.mask,
    };
    let r = compare::compare(&f, &g, &o);
    if a.json {
        outln!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
    } else {
        out!("{}", compare::render(&r, a.timeline));
    }
    if !r.parity {
        std::process::exit(3);
    }
    Ok(())
}

fn real_main() -> Result<(), String> {
    let a = parse_args()?;
    match a.cmd.as_str() {
        "run" => daemon::run(load_config(&a.config)?),
        "status" => {
            let e = load_config(&a.config)?;
            let out = daemon::request(
                &e.cfg.guard.socket,
                if a.json { "status json" } else { "status" },
            )?;
            out!("{out}");
            Ok(())
        }
        "unban" | "ban" => {
            let e = load_config(&a.config)?;
            let target = a.pos.first().ok_or(format!("{} needs an address", a.cmd))?;
            let mut req = format!("{} {target}", a.cmd);
            if let Some(d) = a.pos.get(1) {
                req.push(' ');
                req.push_str(d);
            }
            let out = daemon::request(&e.cfg.guard.socket, &req)?;
            out!("{out}");
            if out.starts_with("error") {
                return Err("request refused".into());
            }
            Ok(())
        }
        "stop" => {
            let e = load_config(&a.config)?;
            out!("{}", daemon::request(&e.cfg.guard.socket, "stop")?);
            Ok(())
        }
        "check-config" => {
            let e = load_config(&a.config)?;
            outln!(
                "ok: source={} backend={} table={} ports={:?} maxretry={} findtime={}s bantime={}s increment={} factor={} maxtime={:?} purge_age={}s protect_accepted={}s allowlist={} entries ignoreself={}",
                e.cfg.sshd.source, e.cfg.guard.backend, e.cfg.guard.table, e.cfg.sshd.ports,
                e.params.maxretry, e.params.findtime, e.params.bantime, e.params.increment,
                e.params.factor, e.params.maxtime, e.params.purge_age, e.params.protect_accepted,
                e.allow.len(), e.cfg.guard.ignoreself
            );
            Ok(())
        }
        "test-filter" => test_filter(&a),
        "replay" => replay(&a),
        "compare" => compare(&a),
        "teardown" => {
            let e = load_config(&a.config)?;
            if e.cfg.guard.is_shadow() {
                outln!("shadow backend: no firewall table was ever created; nothing to remove (nft not run)");
                return Ok(());
            }
            let mut b = daemon::backend_for(&e)?;
            b.teardown()?;
            outln!("removed table inet {}", e.cfg.guard.table);
            Ok(())
        }
        "version" | "--version" => {
            outln!("galion-guard {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            outln!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("galion-guard: {e}");
            ExitCode::from(1)
        }
    }
}
