// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-boot <command>
//!   hardware [--state-dir D] [--public-host H] [--lan host:port] [--wan host:port]
//!   secrets-restore --bundle B --identity K --live DIR --name N... [--apply]
//!   tunnel-env --token-file F --out O [--apply]
//!   render-units --config boot.toml --out DIR [--install-section]
//!   supervise --config boot.toml [--ticks N]
//!   status --config boot.toml
//!   fire-loops [--addr host:port] [--path P]
//! Every write command is dry-run unless --apply (hardware/render-units
//! only write their own outputs).

use galion_boot::{config, hardware, http, secrets, status, supervise, systemd_present, units};
use openmind_survival::SystemRunner;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Args {
    flags: Vec<String>,
    vals: Vec<(String, String)>,
}

impl Args {
    fn parse(argv: &[String]) -> Result<Args, String> {
        let bools = ["--apply", "--install-section"];
        let mut a = Args {
            flags: vec![],
            vals: vec![],
        };
        let mut it = argv.iter();
        while let Some(x) = it.next() {
            if bools.contains(&x.as_str()) {
                a.flags.push(x.clone());
            } else if x.starts_with("--") {
                let v = it.next().ok_or(format!("{x} needs a value"))?;
                a.vals.push((x.clone(), v.clone()));
            } else {
                return Err(format!("unexpected argument {x}"));
            }
        }
        Ok(a)
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
    fn all(&self, k: &str) -> Vec<String> {
        self.vals
            .iter()
            .filter(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
            .collect()
    }
    fn need(&self, k: &str) -> Result<&str, String> {
        self.get(k).ok_or(format!("{k} is required"))
    }
}

fn reachable(hp: Option<&str>) -> Option<bool> {
    let addr = hp?.to_socket_addrs().ok()?.next()?;
    Some(TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok())
}

fn load(a: &Args) -> Result<config::File, String> {
    let p = a.need("--config")?;
    let t = std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
    config::parse(&t).map_err(|e| format!("{p}: {e}"))
}

fn run(cmd: &str, a: &Args) -> Result<i32, String> {
    // NERVE push (no-op unless NERVE_URL is set): started; heartbeat on supervise.
    nerve_client::event("galion-boot", "started", cmd);
    if cmd == "supervise" {
        nerve_client::start_heartbeat("galion-boot", 60);
    }
    match cmd {
        "hardware" => {
            let dir = PathBuf::from(a.get("--state-dir").unwrap_or("target/test-out/boot/state"));
            let p = hardware::Probe {
                runner: &SystemRunner,
                proc_root: Path::new("/proc"),
                sys_root: Path::new("/sys"),
                lan_ok: reachable(a.get("--lan")).unwrap_or(false),
                wan_ok: reachable(a.get("--wan")).unwrap_or(true),
            };
            let hw = hardware::hardware(&p);
            let public = a
                .get("--public-host")
                .map(str::to_string)
                .or_else(|| std::env::var("GALION_PUBLIC_HOST").ok());
            let m = hardware::machine(
                &hw,
                &hardware::hostname(Path::new("/proc")),
                public.as_deref(),
            );
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let pretty =
                |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default() + "\n";
            std::fs::write(dir.join("hardware.json"), pretty(&hw)).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("machine.json"), pretty(&m)).map_err(|e| e.to_string())?;
            let line = hw["line"].as_str().unwrap_or("");
            std::fs::write(dir.join("machine.txt"), format!("{line}\n"))
                .map_err(|e| e.to_string())?;
            println!("{line}");
            Ok(0)
        }
        "secrets-restore" => {
            let tools = secrets::Tools {
                age: a.get("--age-bin").unwrap_or("age"),
                tar: a.get("--tar-bin").unwrap_or("tar"),
            };
            let names = a.all("--name");
            if names.is_empty() {
                return Err("at least one --name".into());
            }
            let steps = secrets::restore(
                &tools,
                Path::new(a.need("--bundle")?),
                Path::new(a.need("--identity")?),
                Path::new(a.need("--live")?),
                &names,
                a.flag("--apply"),
            )?;
            let mode = if a.flag("--apply") {
                "apply"
            } else {
                "dry-run"
            };
            for s in &steps {
                println!("{mode} {} {}", s.name, s.action);
            }
            Ok(if steps.iter().any(|s| s.action.contains("failed")) {
                1
            } else {
                0
            })
        }
        "tunnel-env" => {
            let r = secrets::tunnel_env(
                Path::new(a.need("--token-file")?),
                Path::new(a.need("--out")?),
                a.flag("--apply"),
            )?;
            println!("{r}");
            Ok(0)
        }
        "render-units" => {
            let f = load(a)?;
            let out = PathBuf::from(a.need("--out")?);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            for (name, body) in units::render_all(&f.service, a.flag("--install-section")) {
                std::fs::write(out.join(&name), body).map_err(|e| e.to_string())?;
                println!("wrote {}", out.join(&name).display());
            }
            Ok(0)
        }
        "supervise" => {
            let f = load(a)?;
            if systemd_present() {
                eprintln!("galion-boot: note: systemd is present; prefer the rendered units");
            }
            let ticks: Option<u64> = a.get("--ticks").and_then(|t| t.parse().ok());
            let mut sup = supervise::Supervisor::new(&f);
            let mut n = 0u64;
            loop {
                sup.tick(Instant::now());
                n += 1;
                if ticks.is_some_and(|t| n >= t) {
                    sup.stop_all();
                    return Ok(0);
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "status" => {
            let f = load(a)?;
            let (lines, ok) = status(&f, &SystemRunner, systemd_present());
            for l in lines {
                println!("{l}");
            }
            Ok(if ok { 0 } else { 1 })
        }
        "fire-loops" => {
            let hp = a.get("--addr").unwrap_or("127.0.0.1:8080");
            let addr = hp
                .to_socket_addrs()
                .map_err(|e| e.to_string())?
                .next()
                .ok_or("no address")?;
            let path = a.get("--path").unwrap_or("/api/hermes/loops");
            match http::post_json(addr, path, "{\"action\":\"fire\"}", Duration::from_secs(25)) {
                Ok(code) => {
                    println!("{} fire-loops {code}", zagreb_time::now_rfc3339());
                    Ok(0)
                }
                Err(e) => {
                    println!("{} fire-loops {e}", zagreb_time::now_rfc3339());
                    Ok(0)
                }
            }
        }
        "--version" => {
            println!(
                "{}",
                galion_common::build_info::version_line("galion-boot", env!("CARGO_PKG_VERSION"))
            );
            Ok(0)
        }
        o => Err(format!("unknown command {o}")),
    }
}

fn main() {
    galion_common::build_info::exit_if_version("galion-boot", env!("CARGO_PKG_VERSION"));
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        eprintln!("galion-boot: hardware | secrets-restore | tunnel-env | render-units | supervise | status | fire-loops");
        std::process::exit(2);
    };
    let code = Args::parse(&argv[1..])
        .and_then(|a| run(&cmd, &a))
        .unwrap_or_else(|e| {
            eprintln!("galion-boot: {e}");
            2
        });
    // Bounded flush of the nerve push queue so one-shot commands keep `started` (slot 327).
    nerve_client::flush(std::time::Duration::from_millis(500));
    std::process::exit(code);
}
