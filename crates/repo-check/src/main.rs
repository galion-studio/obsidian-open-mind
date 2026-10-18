// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! repo-check CLI. Read-only: scans the tree, prints findings, exits 1 when
//! any finding is at error level, 2 on usage or I/O errors.

use repo_check::{json_escape, scan_tree, Check, Level};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: repo-check [--root DIR] [--json] [--only a,b] [--warn a,b] [--error a,b]
                  [--copyright TEXT] [--list]
  --root DIR        tree to scan (default .)
  --json            JSON report on stdout (Europe/Zagreb timestamp)
  --only a,b        run only these checks
  --warn a,b        downgrade these checks to warnings
  --error a,b       upgrade these checks to errors
  --copyright TEXT  the LICENSE copyright line must contain TEXT
  --list            list the checks and exit
exit: 0 clean, 1 error findings, 2 usage/io error";

struct Opts {
    root: PathBuf,
    json: bool,
    only: Option<Vec<Check>>,
    levels: HashMap<Check, Level>,
    copyright: Option<String>,
    list: bool,
}

fn parse_checks(s: &str) -> Result<Vec<Check>, String> {
    s.split(',')
        .filter(|p| !p.is_empty())
        .map(|p| Check::parse(p.trim()).ok_or_else(|| format!("unknown check {p:?}")))
        .collect()
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts {
        root: PathBuf::from("."),
        json: false,
        only: None,
        levels: HashMap::new(),
        copyright: None,
        list: false,
    };
    let mut it = args;
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--root" => o.root = PathBuf::from(val("--root")?),
            "--json" => o.json = true,
            "--only" => o.only = Some(parse_checks(&val("--only")?)?),
            "--warn" => {
                for c in parse_checks(&val("--warn")?)? {
                    o.levels.insert(c, Level::Warn);
                }
            }
            "--error" => {
                for c in parse_checks(&val("--error")?)? {
                    o.levels.insert(c, Level::Error);
                }
            }
            "--copyright" => o.copyright = Some(val("--copyright")?),
            "--list" => o.list = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("repo-check: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if opts.list {
        for c in Check::ALL {
            println!("{:<9} {:<5} {}", c.name(), c.default_level(), c.describe());
        }
        return ExitCode::SUCCESS;
    }
    let report = match scan_tree(&opts.root, opts.copyright.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("repo-check: cannot scan {}: {e}", opts.root.display());
            return ExitCode::from(2);
        }
    };
    let mut findings: Vec<_> = report
        .findings
        .into_iter()
        .filter(|f| opts.only.as_ref().is_none_or(|o| o.contains(&f.check)))
        .map(|mut f| {
            if let Some(l) = opts.levels.get(&f.check) {
                f.level = *l;
            }
            f
        })
        .collect();
    findings.sort_by(|a, b| {
        (std::cmp::Reverse(a.level), a.check, &a.file, a.line).cmp(&(
            std::cmp::Reverse(b.level),
            b.check,
            &b.file,
            b.line,
        ))
    });
    let errors = findings.iter().filter(|f| f.level == Level::Error).count();
    let warnings = findings.len() - errors;
    let ts = zagreb_time::now_rfc3339();

    if opts.json {
        let items: Vec<String> = findings
            .iter()
            .map(|f| {
                format!(
                    "{{\"check\":\"{}\",\"level\":\"{}\",\"file\":\"{}\",\"line\":{},\"msg\":\"{}\"}}",
                    f.check.name(),
                    f.level,
                    json_escape(&f.file),
                    f.line,
                    json_escape(&f.msg)
                )
            })
            .collect();
        println!(
            "{{\"tool\":\"repo-check\",\"version\":\"{}\",\"ts\":\"{}\",\"files_scanned\":{},\"errors\":{},\"warnings\":{},\"pass\":{},\"findings\":[{}]}}",
            env!("CARGO_PKG_VERSION"),
            ts,
            report.files_scanned,
            errors,
            warnings,
            errors == 0,
            items.join(",")
        );
    } else {
        for f in &findings {
            let loc = if f.line == 0 {
                f.file.clone()
            } else {
                format!("{}:{}", f.file, f.line)
            };
            println!("{:<5} {:<8} {loc}: {}", f.level, f.check.name(), f.msg);
        }
        println!(
            "repo-check {ts}: {} files, {errors} error(s), {warnings} warning(s) -> {}",
            report.files_scanned,
            if errors == 0 { "PASS" } else { "FAIL" }
        );
    }
    if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
