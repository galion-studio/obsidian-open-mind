// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! `galion-guard` CLI.

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;

use galion_guard::config::{self, Config, Effective};
use galion_guard::daemon;
use galion_guard::engine::{Engine, Event};
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
  galion-guard replay       <file|-> [--config PATH] [--json] [--drain]
                                                 replay a log with its own timestamps (dry-run)
  galion-guard teardown     [--config PATH]      remove the nft table (lifts all bans)
  galion-guard version

default config: /etc/galion-guard/guard.toml (missing file = built-in VPS defaults)";

struct Args {
    cmd: String,
    pos: Vec<String>,
    config: Option<PathBuf>,
    json: bool,
    show: bool,
    drain: bool,
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
    };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--config" | "-c" => a.config = Some(it.next().ok_or("--config needs a path")?.into()),
            "--json" => a.json = true,
            "--show" => a.show = true,
            "--drain" => a.drain = true,
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
        "teardown" => {
            let e = load_config(&a.config)?;
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

/// `--check`: load the default config (or built-in VPS defaults); never
/// touches the packet filter, never reads the journal.
fn check() -> Result<String, String> {
    let e = load_config(&None)?;
    Ok(format!(
        "config {}\nsource {}\nbackend {}\ntable {}\nports {:?}\nmaxretry {}\nallowlist {} entries\n",
        if std::path::Path::new(config::DEFAULT_PATH).exists() { config::DEFAULT_PATH } else { "built-in VPS defaults (no /etc/galion-guard/guard.toml)" },
        e.cfg.sshd.source, e.cfg.guard.backend, e.cfg.guard.table, e.cfg.sshd.ports, e.params.maxretry, e.allow.len()
    ))
}

fn main() -> ExitCode {
    galion_common::flags::early("galion-guard", env!("CARGO_PKG_VERSION"), USAGE, check);
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("galion-guard: {e}");
            ExitCode::from(1)
        }
    }
}
