// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
use galion_admin::audit::{self, Verdict};
use galion_admin::cli::{self, Cmd, Global, USAGE};
use galion_admin::sandbox::{self, Paths};
use galion_admin::tunnel;
use galion_admin::{image, json};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn state_dir(g: &Global) -> Result<PathBuf, String> {
    if let Some(s) = &g.state {
        return Ok(s.clone());
    }
    if let Some(s) = std::env::var_os("GALION_ADMIN_HOME") {
        return Ok(PathBuf::from(s));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".galion-admin"))
        .ok_or_else(|| "no state dir: pass --state DIR or set GALION_ADMIN_HOME".into())
}

fn box_bin(g: &Global) -> Result<PathBuf, String> {
    if let Some(b) = &g.box_bin {
        return Ok(b.clone());
    }
    if let Some(b) = std::env::var_os("GALION_BOX") {
        return Ok(PathBuf::from(b));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("galion-box"))
        .find(|p| p.is_file())
        .ok_or_else(|| "galion-box not found: pass --box PATH or set GALION_BOX".into())
}

fn abs(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
}

fn audit_tail(p: &Path, n: usize, json: bool, follow: bool, secs: Option<u64>) -> i32 {
    let show = |line: &str| {
        if json {
            println!("{line}");
        } else {
            match audit::parse_line(line) {
                Ok(r) => println!("{}", audit::render(&r)),
                Err(e) => println!("!! unreadable record ({e})"),
            }
        }
    };
    match audit::tail_lines(p, n) {
        Ok(lines) => lines.iter().for_each(|l| show(l)),
        Err(e) => {
            eprintln!("galion-admin: audit: {e}");
            return 1;
        }
    }
    let _ = std::io::stdout().flush();
    if !follow {
        return 0;
    }
    let mut pos = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let mut pending = String::new();
    let end = secs.map(|s| Instant::now() + Duration::from_secs(s));
    loop {
        if end.is_some_and(|e| Instant::now() >= e) {
            return 0;
        }
        std::thread::sleep(Duration::from_millis(250));
        let len = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        if len < pos {
            eprintln!("galion-admin: audit log shrank ({pos} -> {len} bytes): possible truncation");
            pos = 0;
        }
        if len > pos {
            if let Ok(mut f) = std::fs::File::open(p) {
                let _ = f.seek(SeekFrom::Start(pos));
                let mut buf = String::new();
                if f.read_to_string(&mut buf).is_ok() {
                    pos += buf.len() as u64;
                    pending.push_str(&buf);
                    while let Some(i) = pending.find('\n') {
                        let line: String = pending.drain(..=i).collect();
                        show(line.trim_end());
                    }
                    let _ = std::io::stdout().flush();
                }
            }
        }
    }
}

fn image_verify(g: &Global, dir: &Path, key: &Path, json: bool) -> i32 {
    let started = Instant::now();
    let vk = match image::load_public(key) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("galion-admin: image verify: {e}");
            return 1;
        }
    };
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            eprintln!(
                "galion-admin: image verify: {}: not a directory",
                dir.display()
            );
            return 1;
        }
        Err(e) => {
            eprintln!(
                "galion-admin: image verify: {}: {}",
                dir.display(),
                e.kind()
            );
            return 1;
        }
    }
    let res = image::verify(dir, &vk);
    let ms = started.elapsed().as_millis();
    let (code, target, result, detail) = match &res {
        Ok(r) => {
            let m = &r.manifest;
            let head = format!(
                "{} {} {} commit {}, {} files, {} bytes, key {}, manifest {}",
                m.get("name"),
                m.get("version"),
                m.get("arch"),
                m.get("build.commit"),
                m.files.len(),
                m.total_bytes(),
                r.key,
                &image::to_hex(&r.manifest_sha256)[..16]
            );
            if json {
                let probs: Vec<String> = r
                    .problems
                    .iter()
                    .map(|p| {
                        format!(
                            "{{\"code\":\"{}\",\"path\":\"{}\",\"detail\":\"{}\"}}",
                            p.code(),
                            json::escape(p.path()),
                            json::escape(&p.detail())
                        )
                    })
                    .collect();
                println!(
                    "{{\"ok\":{},\"name\":\"{}\",\"version\":\"{}\",\"arch\":\"{}\",\"commit\":\"{}\",\"built\":\"{}\",\"files\":{},\"bytes\":{},\"bytes_hashed\":{},\"key\":\"{}\",\"manifest_sha256\":\"{}\",\"ms\":{},\"problems\":[{}]}}",
                    r.ok(),
                    m.get("name"),
                    m.get("version"),
                    m.get("arch"),
                    m.get("build.commit"),
                    m.get("build.time"),
                    m.files.len(),
                    m.total_bytes(),
                    r.bytes_hashed,
                    r.key,
                    image::to_hex(&r.manifest_sha256),
                    ms,
                    probs.join(",")
                );
            } else if r.ok() {
                println!("image ok: {head} ({ms} ms)");
            } else {
                println!("image FAILED: {} problem(s) in {head}", r.problems.len());
                for p in &r.problems {
                    println!("  {:<12} {}  ({})", p.code(), p.path(), p.detail());
                }
            }
            (
                if r.ok() { 0 } else { 4 },
                m.get("name").to_string(),
                if r.ok() { "ok" } else { "failed" },
                format!(
                    "files={} bytes={} problems={} manifest={} key={}",
                    m.files.len(),
                    m.total_bytes(),
                    r.problems.len(),
                    &image::to_hex(&r.manifest_sha256)[..16],
                    r.key
                ),
            )
        }
        Err(rej) => {
            if json {
                println!(
                    "{{\"ok\":false,\"rejected\":\"{}\",\"ms\":{ms}}}",
                    json::escape(&rej.to_string())
                );
            } else {
                println!("image REJECTED: {rej}");
            }
            (4, String::new(), "rejected", rej.to_string())
        }
    };
    // Best effort, like tunnel status: a missing or unusable state dir never changes the verdict.
    if let Ok(root) = state_dir(g) {
        let root = abs(root);
        if sandbox::ensure_private_dir(&root).is_ok() {
            let p = Paths { root };
            let _ = audit::append(&p.audit(), "image.verify", &target, result, &detail);
        }
    }
    code
}

fn run(g: Global, cmd: Cmd) -> Result<i32, String> {
    let paths = || state_dir(&g).map(|r| Paths { root: abs(r) });
    match cmd {
        Cmd::Help => {
            print!("{USAGE}");
            Ok(0)
        }
        Cmd::Version => {
            println!(
                "galion-admin {} ({}-{})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH,
                std::env::consts::OS
            );
            Ok(0)
        }
        Cmd::SandboxStart(o) => {
            let p = paths()?;
            let b = abs(box_bin(&g)?);
            let msg = sandbox::start(&p, &b, &o)?;
            println!("{msg}");
            Ok(0)
        }
        Cmd::SandboxStop { name, grace_secs } => {
            let p = paths()?;
            match sandbox::stop(&p, &name, Duration::from_secs(grace_secs))? {
                Some(m) => {
                    println!("{m}");
                    Ok(0)
                }
                None => {
                    println!("sandbox {name} is not running");
                    Ok(4)
                }
            }
        }
        Cmd::SandboxStatus(only) => {
            let p = paths()?;
            let rows = sandbox::status(&p, only.as_deref())?;
            if rows.is_empty() {
                println!("no sandboxes");
            }
            for (l, _) in &rows {
                println!("{l}");
            }
            Ok(if only.is_some() && !rows.iter().any(|r| r.1) {
                4
            } else {
                0
            })
        }
        Cmd::SandboxProbe => {
            let p = paths()?;
            sandbox::ensure_private_dir(&p.root).map_err(|e| e.to_string())?;
            let b = box_bin(&g)?;
            let st = std::process::Command::new(&b)
                .arg("probe")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .status()
                .map_err(|e| format!("{}: {e}", b.display()))?;
            let code = st.code().unwrap_or(1);
            let _ = audit::append(&p.audit(), "sandbox.probe", "", &format!("exit:{code}"), "");
            Ok(code)
        }
        Cmd::TunnelStatus { config, timeout_ms } => {
            let p = paths()?;
            let cfg = config.unwrap_or_else(|| p.root.join("tunnel.conf"));
            let text = match std::fs::read_to_string(&cfg) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    println!(
                        "no tunnels configured ({} missing; lines `name = 127.0.0.1:PORT`)",
                        cfg.display()
                    );
                    return Ok(0);
                }
                Err(e) => return Err(format!("{}: {e}", cfg.display())),
            };
            let eps = tunnel::parse_config(&text).map_err(|e| format!("{}: {e}", cfg.display()))?;
            let mut down = 0;
            for e in &eps {
                let s = tunnel::probe(&e.addr, Duration::from_millis(timeout_ms));
                if matches!(s, tunnel::State::Down { .. }) {
                    down += 1;
                }
                println!("{}", tunnel::render(e, &s));
            }
            println!(
                "{} tunnel(s), {} up, {} down",
                eps.len(),
                eps.len() - down,
                down
            );
            if sandbox::ensure_private_dir(&p.root).is_ok() {
                let _ = audit::append(
                    &p.audit(),
                    "tunnel.status",
                    "",
                    if down == 0 { "ok" } else { "down" },
                    &format!("{} up, {} down", eps.len() - down, down),
                );
            }
            Ok(if down == 0 { 0 } else { 4 })
        }
        Cmd::AuditTail {
            n,
            json,
            follow,
            follow_secs,
        } => {
            let p = paths()?;
            Ok(audit_tail(&p.audit(), n, json, follow, follow_secs))
        }
        Cmd::AuditVerify => {
            let p = paths()?;
            match audit::verify(&p.audit()).map_err(|e| e.to_string())? {
                Verdict::Ok { records, head } => {
                    println!("audit ok: {records} records, head {head}");
                    Ok(0)
                }
                Verdict::Broken { line, reason } => {
                    println!("audit BROKEN at line {line}: {reason}");
                    Ok(4)
                }
            }
        }
        Cmd::ImageVerify { dir, key, json } => Ok(image_verify(&g, &dir, &key, json)),
        Cmd::Supervise {
            name,
            cmd,
            box_bin,
            box_args,
        } => {
            let p = paths()?;
            Ok(sandbox::supervise(&p, &name, &cmd, &box_bin, &box_args))
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // SAFETY: trivial getter.
    if let Err(e) = cli::refuse_root(unsafe { libc::geteuid() }) {
        eprintln!("galion-admin: {e}");
        std::process::exit(3);
    }
    let code = match cli::parse(&args) {
        Err(e) => {
            eprintln!("galion-admin: {e}");
            2
        }
        Ok((g, c)) => match run(g, c) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("galion-admin: {e}");
                1
            }
        },
    };
    std::process::exit(code);
}
