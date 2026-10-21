// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-realtest CLI. See `lib.rs` and `--help`.

use galion_realtest::report::{self, Outcome, Suite};
use galion_realtest::{live, scenarios, stack};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage:
  galion-realtest stack [--repo DIR] [--bin-dir DIR] [--build] [--jobs N]
                        [--base-port N] [--out DIR] [--known-failures FILE]
                        [--with-nerve] [--with-gaia]
      Boot the whole stack on the box (ports BASE..BASE+53, default 19500),
      run every scenario, tear down, write junit.xml + report.md.
      --with-gaia adds openmind-queue and the Gaia scenarios (heal-tick
      plan-only, hades-gate token refusals, POSEIDON submit/refuse); the
      heal-tick target must be a Gaia test port, so keep BASE in 19000-19946.
  galion-realtest live  [--ssh-host HOST] [--no-ssh] [--no-public] [--out DIR]
      READ-ONLY: GET-only probes of the live VPS over ssh (default host
      slavik-vps) and of the public endpoints. Never writes or restarts.
Exit: 0 = no unexpected failure, 1 = a check failed, 2 = usage/setup error.";

struct Opts {
    flags: Vec<String>,
    vals: Vec<(String, String)>,
}

impl Opts {
    fn parse(argv: &[String]) -> Result<Opts, String> {
        let bools = [
            "--build",
            "--with-nerve",
            "--with-gaia",
            "--no-ssh",
            "--no-public",
        ];
        let mut o = Opts {
            flags: vec![],
            vals: vec![],
        };
        let mut it = argv.iter();
        while let Some(a) = it.next() {
            if bools.contains(&a.as_str()) {
                o.flags.push(a.clone());
            } else if a.starts_with("--") {
                let v = it.next().ok_or(format!("{a} needs a value"))?;
                o.vals.push((a.clone(), v.clone()));
            } else {
                return Err(format!("unexpected argument {a}"));
            }
        }
        Ok(o)
    }
    fn flag(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.vals
            .iter()
            .rev()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

fn find_repo(start: &Path) -> PathBuf {
    let mut d = Some(start);
    while let Some(dir) = d {
        if std::fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|t| t.contains("[workspace]"))
        {
            return dir.to_path_buf();
        }
        d = dir.parent();
    }
    start.to_path_buf()
}

fn stamp_dir() -> String {
    zagreb_time::now_rfc3339()
        .replace(['-', ':'], "")
        .replace('T', "-")
}

fn sh(cmd: &str, args: &[&str], dir: &Path) -> String {
    Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn write_reports(out: &Path, title: &str, preamble: &str, suites: &[Suite]) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join("junit.xml"), report::junit(title, suites))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("report.md"),
        report::markdown(title, preamble, suites),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn print_summary(suites: &[Suite]) -> bool {
    let mut bad = false;
    for s in suites {
        let v = s.verdict();
        println!("{:<11} {:<40} {}", v.label(), s.name, s.service);
        for c in s.checks.iter().filter(|c| c.outcome.failed()) {
            println!(
                "    {} {}: {}",
                c.outcome.label(),
                c.name,
                c.detail.lines().next().unwrap_or("")
            );
        }
        bad |= s.checks.iter().any(|c| c.outcome == Outcome::Fail);
    }
    !bad
}

fn cmd_stack(o: &Opts) -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo = o
        .get("--repo")
        .map(PathBuf::from)
        .unwrap_or_else(|| find_repo(&cwd));
    let base: u16 = o
        .get("--base-port")
        .unwrap_or("19500")
        .parse()
        .map_err(|_| "--base-port: not a port")?;
    if base < 1024 || u32::from(base) + u32::from(stack::off::MAX) > 65535 {
        return Err("--base-port out of range".into());
    }
    // Absolute, so the fixture paths written into boot.toml are absolute too.
    let out = o
        .get("--out")
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p)))
        .unwrap_or_else(|| {
            repo.join("target/realtest")
                .join(format!("stack-{}", stamp_dir()))
        });
    let mut pre = format!(
        "Mode: **box stack** (real release binaries on 127.0.0.1:{base}-{}).\n\n- Repo: `{}` at commit `{}`\n- Host: `{}`\n",
        base + stack::off::MAX,
        repo.display(),
        sh("git", &["rev-parse", "--short=12", "HEAD"], &repo),
        sh("uname", &["-srm"], &repo)
    );
    if o.flag("--build") {
        let jobs: u32 = o.get("--jobs").unwrap_or("4").parse().unwrap_or(4);
        let mut extra: Vec<&str> = Vec::new();
        if o.flag("--with-nerve") {
            extra.push("openmind-nerve");
        }
        if o.flag("--with-gaia") {
            extra.extend_from_slice(stack::GAIA_PKGS);
        }
        let line = stack::build(&repo, jobs, &extra)?;
        pre.push_str(&format!("- Build: `{line}`\n"));
    }
    let bins = stack::locate(o.get("--bin-dir").map(Path::new), &repo);
    pre.push_str(&format!("- Binaries: `{}`\n", bins.dir.display()));
    let missing = bins.missing();
    if !missing.is_empty() {
        return Err(format!(
            "missing release binaries in {}: {} (use --build)",
            bins.dir.display(),
            missing.join(", ")
        ));
    }
    let known_path = o
        .get("--known-failures")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("tests/real/known-failures.txt"));
    let known = std::fs::read_to_string(&known_path)
        .map(|t| report::parse_known(&t))
        .unwrap_or_default();
    pre.push_str(&format!(
        "- Known-failures list: `{}` ({} entries)\n",
        known_path.display(),
        known.len()
    ));

    println!(
        "galion-realtest: booting the stack on 127.0.0.1:{base}.. (out {})",
        out.display()
    );
    let mut st = stack::Stack::up(
        bins,
        &out,
        base,
        o.flag("--with-nerve"),
        o.flag("--with-gaia"),
    )?;
    let mut suites = scenarios::all(&mut st);
    let mut td = Suite::new("teardown", "galion-realtest");
    let down = st.down();
    td.check("all-processes-gone-ports-free", || down.clone());
    suites.push(td);
    // The fixture key file is throwaway, but do not leave it behind.
    let _ = std::fs::remove_file(st.path("memory.env"));
    report::apply_known(&mut suites, &known);

    pre.push_str(&format!("- Boot (supervisor start to all services healthy): {} ms\n\n| Service | Ready after (ms) |\n|---|---|\n", st.boot_ms));
    for (name, ms) in &st.ready {
        pre.push_str(&format!(
            "| {name} | {} |\n",
            ms.map(|m| m.to_string()).unwrap_or_else(|| "NEVER".into())
        ));
    }
    pre.push_str(&format!(
        "\nService logs: `{}`\n",
        st.path("logs").display()
    ));
    write_reports(&out, "galion-realtest: box stack", &pre, &suites)?;
    let ok = print_summary(&suites);
    println!(
        "reports: {}/junit.xml {}/report.md",
        out.display(),
        out.display()
    );
    Ok(ok)
}

fn cmd_live(o: &Opts) -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo = find_repo(&cwd);
    let out = o.get("--out").map(PathBuf::from).unwrap_or_else(|| {
        repo.join("target/realtest")
            .join(format!("live-{}", stamp_dir()))
    });
    let host = if o.flag("--no-ssh") {
        None
    } else {
        Some(o.get("--ssh-host").unwrap_or("slavik-vps"))
    };
    let (mut suites, notes) = live::run(host, !o.flag("--no-public"));
    let known_path = repo.join("tests/real/known-failures.txt");
    let known = std::fs::read_to_string(&known_path)
        .map(|t| report::parse_known(&t))
        .unwrap_or_default();
    report::apply_known(&mut suites, &known);
    let pre = format!(
        "Mode: **live probe, READ-ONLY**. Only HTTP GET, `systemctl is-active` and `ss -ltnH`; no POST, no file written on the VPS, nothing restarted.\n\n{notes}"
    );
    write_reports(&out, "galion-realtest: live probe", &pre, &suites)?;
    let ok = print_summary(&suites);
    println!(
        "reports: {}/junit.xml {}/report.md",
        out.display(),
        out.display()
    );
    Ok(ok)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let opts = match Opts::parse(&argv[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("galion-realtest: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let r = match cmd.as_str() {
        "stack" => cmd_stack(&opts),
        "live" => cmd_live(&opts),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!("unknown command {other}")),
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("galion-realtest: {e}");
            ExitCode::from(2)
        }
    }
}
