// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life tests for galion-flight (GALION_REAL=1; skipped otherwise).
//!
//! `real_box_postflight`: starts real release builds of nerve, memory,
//! hermes-gateway (--mock), galion-one and tuner on the box (ports
//! 20650-20659), runs `galion-flight postflight <svc> --target local` with
//! the SHIPPED live manifest re-pointed at the box instance, and checks GO,
//! plus negative controls (wrong sha, live-only body, killed instance).
//! cloudflared has no box build (a real tunnel would publish a URL), so its
//! box run uses an in-process /ready stand-in; the real check is the VPS one.
//!
//! `real_vps_flight`: with GALION_FLIGHT_VPS=<ssh host>, runs preflight and
//! postflight READ-ONLY against the live VPS for all six services and pins
//! that only read commands were sent.
//!
//! Binaries: GALION_FLIGHT_BIN_DIR, else $CARGO_TARGET_DIR/release.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn flight() -> &'static str {
    env!("CARGO_BIN_EXE_galion-flight")
}

fn bin_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GALION_FLIGHT_BIN_DIR") {
        return d.into();
    }
    let t = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
    Path::new(&t).join("release")
}

fn run_flight(args: &[&str]) -> (i32, String) {
    let o = Command::new(flight()).args(args).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned()
        + &String::from_utf8_lossy(&o.stderr);
    println!("$ galion-flight {}\n{out}", args.join(" "));
    (o.status.code().unwrap_or(-1), out)
}

fn fails(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.strip_prefix("FAIL "))
        .map(|l| l.split(" — ").next().unwrap_or("").to_string())
        .collect()
}

fn sha_file(p: &Path) -> String {
    let data = std::fs::read(p).unwrap();
    galion_common::hex::encode(&galion_common::sha256::digest(&data))
}

/// Test harness only: wait until the instance answers (the tool never waits).
fn wait_health(port: u16, path: &str) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let _ = write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
            let mut b = String::new();
            let _ = s.read_to_string(&mut b);
            if b.starts_with("HTTP/1.") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("port {port} never answered {path}");
}

struct Inst {
    name: &'static str,
    child: Child,
    exe: PathBuf,
}

impl Drop for Inst {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(name: &'static str, exe: PathBuf, args: &[&str], env: &[(&str, String)], dir: &Path) -> Inst {
    assert!(exe.is_file(), "missing {} (build release first)", exe.display());
    let log = std::fs::File::create(dir.join(format!("{name}.log"))).unwrap();
    let mut c = Command::new(&exe);
    c.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir.join("home"))
        .env("TZ", "Europe/Zagreb")
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    for (k, v) in env {
        c.env(k, v);
    }
    Inst {
        name,
        child: c.spawn().unwrap(),
        exe,
    }
}

fn rand_hex() -> String {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut b)
        .unwrap();
    galion_common::hex::encode(&b)
}

/// Overrides that turn a shipped live manifest into the box instance's.
fn box_sets(i: &Inst, port: u16, extra: &[String]) -> Vec<String> {
    let canon = std::fs::canonicalize(&i.exe).unwrap();
    let md = std::fs::metadata(&canon).unwrap();
    let mut v = vec![
        "unit=".to_string(),
        format!("port={port}"),
        format!("exe={}", canon.display()),
        format!("sha256={}", sha_file(&canon)),
        format!("mode={:o}", md.mode() & 0o7777),
        format!("owner={}:{}", md.uid(), md.gid()),
        "dropins=-".into(),
        "nerve=".into(),
    ];
    v.extend_from_slice(extra);
    v
}

fn args_with<'a>(base: &[&'a str], sets: &'a [String]) -> Vec<&'a str> {
    let mut a: Vec<&str> = base.to_vec();
    for s in sets {
        a.push("--set");
        a.push(s);
    }
    a
}

#[test]
fn real_box_postflight() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("galion-flight-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["home", "h", "m", "n", "one", "www"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    std::fs::write(
        dir.join("www/index.html"),
        "<!doctype html><html><body>tuner fixture</body></html>\n",
    )
    .unwrap();
    let key = rand_hex(); // fixture key, never printed
    let p = |r: &str| dir.join(r).to_string_lossy().into_owned();
    let b = bin_dir();
    let none = p("absent/none");

    let nerve = spawn(
        "nerve",
        b.join("openmind-nerve"),
        &[],
        &[
            ("NERVE_ADDR", "127.0.0.1:20650".into()),
            ("NERVE_DATA_DIR", p("n")),
            ("NERVE_GAIA_BIN", "/bin/false".into()),
            ("NERVE_DEPLOY_BIN", "/bin/false".into()),
            ("NERVE_DEPLOY_BACKUPS", p("n/backups")),
            ("NERVE_SWITCH_DIR", p("n/switch")),
            ("NERVE_TELEGRAM_ENV", none.clone()),
            ("NERVE_VOICE_UNIT", "realtest-none.service".into()),
            ("NERVE_VOICE_PORT", "20658".into()),
            ("NERVE_DISK_PATH", p("")),
        ],
        &dir,
    );
    let memory = spawn(
        "memory",
        b.join("openmind-memory"),
        &[],
        &[
            ("OPENMIND_MEMORY_HOST", "127.0.0.1".into()),
            ("OPENMIND_MEMORY_PORT", "20651".into()),
            ("OPENMIND_MEMORY_DATA_DIR", p("m")),
            ("OPENMIND_MEMORY_API_KEY", key.clone()),
        ],
        &dir,
    );
    let hermes = spawn(
        "hermes-rs",
        b.join("hermes-gateway"),
        &["--mock"],
        &[
            ("API_SERVER_KEY", key.clone()),
            ("HERMES_HOME", p("h")),
            ("HERMES_RS_HOST", "127.0.0.1".into()),
            ("HERMES_RS_PORT", "20652".into()),
            ("HERMES_RS_STATE_DB", p("h/state.db")),
        ],
        &dir,
    );
    let one = spawn(
        "galion-one",
        b.join("galion-one"),
        &[],
        &[
            ("ONE_BIND", "127.0.0.1:20653".into()),
            ("ONE_MOUTH_BIND", "127.0.0.1:20659".into()),
            ("ONE_DEVICE_STORE", p("one/admin-device.json")),
            ("ONE_HERMES_DIR", p("h")),
            ("ONE_HERMES_ENV", none.clone()),
            ("ONE_NO_WARM", "1".into()),
            ("ONE_WARM_MIND", "0".into()),
            ("GALION_HOME", p("home")),
            ("OPENROUTER_API_KEY", String::new()),
            ("OPENROUTER_KEY_FILE", none.clone()),
            ("OPENROUTER_BASE_URL", "http://127.0.0.1:9".into()),
        ],
        &dir,
    );
    let tuner = spawn(
        "tuner",
        b.join("tuner"),
        &[],
        &[
            ("TUNER_ADDR", "127.0.0.1:20654".into()),
            ("TUNER_ROOT", p("www")),
        ],
        &dir,
    );
    // cloudflared stand-in: /ready on the metrics ports (no real tunnel).
    for port in [20655u16, 20656] {
        let l = TcpListener::bind(("127.0.0.1", port)).unwrap();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let body = r#"{"status":200,"readyConnections":4,"connectorId":"stand-in"}"#;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
    }
    wait_health(20650, "/healthz");
    wait_health(20651, "/health");
    wait_health(20652, "/health");
    wait_health(20653, "/health");
    wait_health(20654, "/health");
    wait_health(20655, "/ready");

    let mut summary = Vec::new();
    let mut insts = vec![nerve, memory, hermes, one, tuner];
    let cases: Vec<(&str, u16, Vec<String>)> = vec![
        // Box nerve runs dry-run (no units to act on): mode differs from live.
        (
            "nerve",
            20650,
            vec![
                "get=/healthz 200 exact:ok".into(),
                "get=/state 200 json:service=galion-nerve".into(),
                "get=/state 200 json:mode=dry-run".into(),
            ],
        ),
        ("memory", 20651, vec![]),
        ("hermes-rs", 20652, vec![]),
        (
            "galion-one",
            20653,
            vec![
                "get=/health 200 exact:ok".into(),
                "get=:20659 /health 503 json:status=unavailable".into(),
            ],
        ),
        ("tuner", 20654, vec![]),
    ];
    for (inst, (svc, port, extra)) in insts.iter().zip(cases.iter()) {
        let pid = inst.child.id().to_string();
        let sets = box_sets(inst, *port, extra);
        let base = [
            "postflight", svc, "--target", "local", "--pid", &pid, "--no-public",
        ];
        let (code, out) = run_flight(&args_with(&base, &sets));
        assert_eq!(code, 0, "{svc} postflight must be GO:\n{out}");
        assert!(out.contains(&format!("RESULT GO {svc} postflight")), "{out}");
        summary.push(format!("{svc}: GO"));

        // Negative 1: the change was not the tested binary.
        let mut bad = sets.clone();
        bad.push(format!("sha256={}", "0".repeat(64)));
        let (code, out) = run_flight(&args_with(&base, &bad));
        assert_eq!(code, 1, "{out}");
        let f = fails(&out);
        assert!(
            f.contains(&"exe.running-sha256".to_string()) && f.contains(&"exe.file-sha256".to_string()),
            "{f:?}"
        );
        // Negative 2: a wrong body expectation is caught.
        let mut bad = sets.clone();
        bad.push(format!("get=:{port} /flight-nope 200 exact:x"));
        let (code, _) = run_flight(&args_with(&base, &bad));
        assert_eq!(code, 1);
    }
    // The shipped live manifests, unmodified bodies, against 42de672 box
    // builds: exactly the known live/box differences fail.
    {
        let one = &insts[3];
        let pid = one.child.id().to_string();
        let sets = box_sets(one, 20653, &["get=/health 200 exact:ok".into(), "get=:20659 /health 503 json:status=loading".into()]);
        let (code, out) = run_flight(&args_with(
            &["postflight", "galion-one", "--target", "local", "--pid", &pid, "--no-public"],
            &sets,
        ));
        assert_eq!(code, 1);
        assert_eq!(fails(&out), vec!["http.get :20659/health".to_string()], "{out}");
        summary.push("galion-one live mouth body (loading) vs 42de672 box (unavailable): NO-GO caught".into());
    }
    // cloudflared stand-in (status-only /ready, as on the VPS).
    {
        let me = std::process::id().to_string();
        let sets = vec![
            "unit=".to_string(),
            "also_unit=".into(),
            "port=20655".into(),
            "exe=".into(),
            "sha256=".into(),
            "mode=".into(),
            "owner=".into(),
            "dropins=-".into(),
            "nerve=".into(),
            "get=/ready 200 any".into(),
            "get=:20656 /ready 200 any".into(),
        ];
        let (code, out) = run_flight(&args_with(
            &["postflight", "cloudflared", "--target", "local", "--pid", &me, "--no-public"],
            &sets,
        ));
        assert_eq!(code, 0, "{out}");
        assert!(!out.contains("connectorId") && !out.contains("stand-in"), "body leaked");
        summary.push("cloudflared (stand-in): GO, body not shown".into());
    }
    // Negative 3: kill each instance, postflight must say NO-GO.
    for (inst, (svc, port, extra)) in insts.iter_mut().zip(cases.iter()) {
        let sets = box_sets(inst, *port, extra);
        let pid = inst.child.id().to_string();
        let _ = inst.child.kill();
        let _ = inst.child.wait();
        let (code, out) = run_flight(&args_with(
            &["postflight", svc, "--target", "local", "--pid", &pid, "--no-public"],
            &sets,
        ));
        assert_eq!(code, 1, "{out}");
        let f = fails(&out);
        assert!(f.iter().any(|x| x.starts_with("port.listen")), "{f:?}");
        assert!(f.iter().any(|x| x.starts_with("http.get")), "{f:?}");
        summary.push(format!("{} killed: NO-GO ({} fails)", inst.name, f.len()));
    }
    drop(insts);
    println!("SUMMARY\n{}", summary.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_vps_flight() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let Ok(host) = std::env::var("GALION_FLIGHT_VPS") else {
        eprintln!("skip: set GALION_FLIGHT_VPS=<ssh host> for the live read-only run");
        return;
    };
    let services = ["nerve", "memory", "hermes-rs", "galion-one", "tuner", "cloudflared"];
    let mut summary = Vec::new();
    for phase in ["preflight", "postflight"] {
        for svc in services {
            let t0 = Instant::now();
            let (code, out) = run_flight(&[phase, svc, "--host", &host, "--show-commands"]);
            let f = fails(&out);
            // Known pre-existing live state: the nerve rates cloudflared
            // failing-soon (closed origin ports). Anything else must pass.
            let allowed: Vec<String> = if svc == "cloudflared" {
                vec!["nerve.view cloudflared".into()]
            } else {
                vec![]
            };
            assert!(f.iter().all(|x| allowed.contains(x)), "{phase} {svc}: {f:?}\n{out}");
            assert_eq!(code, if f.is_empty() { 0 } else { 1 });
            for l in out.lines().filter_map(|l| l.strip_prefix("SENT ")) {
                assert!(
                    ["systemctl show ", "readlink ", "sha256sum ", "stat ", "ss -ltnH ", "curl -sS "]
                        .iter()
                        .any(|v| l.starts_with(v)),
                    "non-read command sent: {l}"
                );
                assert!(!l.contains("-X") && !l.contains("--data") && !l.contains("Environment"));
            }
            let sent = out.lines().filter(|l| l.starts_with("SENT ")).count();
            let checks = out
                .lines()
                .filter(|l| l.starts_with("PASS ") || l.starts_with("FAIL ") || l.starts_with("SKIP "))
                .count();
            summary.push(format!(
                "{phase} {svc}: exit {code}, {checks} checks, {sent} remote reads, {} ms, fails {f:?}",
                t0.elapsed().as_millis()
            ));
        }
    }
    // Negative controls, still read-only.
    let (code, out) = run_flight(&["preflight", "nerve", "--host", &host, "--set", &format!("sha256={}", "0".repeat(64))]);
    assert_eq!(code, 1);
    assert!(fails(&out).contains(&"exe.running-sha256".to_string()));
    summary.push("nerve wrong sha: NO-GO".into());
    let (code, out) = run_flight(&["preflight", "tuner", "--host", &host, "--set", "quiet_timer=galion-backup.timer 999999"]);
    assert_eq!(code, 1);
    assert_eq!(fails(&out), vec!["timer.quiet galion-backup.timer".to_string()]);
    summary.push("tuner backup due within window: NO-GO".into());
    let (code, out) = run_flight(&["postflight", "memory", "--host", &host, "--since-unix", "4102444800"]);
    assert_eq!(code, 1);
    assert_eq!(fails(&out), vec!["unit.restarted openmind-memory.service".to_string()]);
    summary.push("memory not restarted since T: NO-GO".into());
    let (code, _) = run_flight(&["preflight", "nope", "--host", &host]);
    assert_eq!(code, 2);
    let (code, _) = run_flight(&["preflight", "nerve", "--set", "exe=/opt/x;id"]);
    assert_eq!(code, 2);
    println!("SUMMARY\n{}", summary.join("\n"));
}
