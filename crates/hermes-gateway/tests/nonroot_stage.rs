// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 318 (sandbox-nonroot-stage, D-SB4): run hermes-gateway as a
//! dedicated non-root user.
//!
//! * Unit tests (run anywhere, start nothing): the staged drop-in
//!   `deploy/staged/hermes-rs.service.d/80-nonroot.conf` and the sysusers
//!   file are well-formed and hand config.yaml / the legacy state.db over
//!   in a way a non-root gateway can read.
//! * `real_nonroot_*` (gated `GALION_REAL=1` AND `GALION_NONROOT_ON_VPS=1`;
//!   sandbox_vps_only: run ON THE VPS as an ordinary non-root user, e.g.
//!   uid 65534, in /var/tmp/sandbox-test-<ts>). It starts the real release
//!   binary on loopback ports `HNR_PORT`.. (default 21880..21882), proves
//!   the two failure modes of a bare `User=` against the live host layout
//!   (`/opt/galion/hermes` is 0700 root), then the staged shape end to end.
//!   It never reads the live config.yaml or state.db (it only gets EACCES).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "hnr318-test-key-not-a-secret";
const LIVE_HOME: &str = "/opt/galion/hermes";
const DROPIN: &str = "deploy/staged/hermes-rs.service.d/80-nonroot.conf";
const SYSUSERS: &str = "deploy/staged/sysusers.d/hermes-rs.conf";

fn repo_root() -> PathBuf {
    std::env::var_os("HNR_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn read_repo(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// `(key, value)` directive lines of a unit file, comments skipped.
fn directives(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn get<'a>(d: &'a [(String, String)], k: &str) -> Vec<&'a str> {
    d.iter()
        .filter(|(x, _)| x == k)
        .map(|(_, v)| v.as_str())
        .collect()
}

fn env_of(d: &[(String, String)], k: &str) -> Option<String> {
    get(d, "Environment")
        .iter()
        .filter_map(|v| v.trim_matches('"').split_once('='))
        .filter(|(x, _)| *x == k)
        .map(|(_, v)| v.to_string())
        .next_back()
}

#[test]
fn dropin_sets_a_static_non_root_user() {
    let d = directives(&read_repo(DROPIN));
    assert_eq!(get(&d, "User"), ["hermes-rs"]);
    assert_eq!(get(&d, "Group"), ["hermes-rs"]);
    assert!(
        get(&d, "DynamicUser").is_empty(),
        "static user, not DynamicUser"
    );
    for bad in [
        "RestrictNamespaces",
        "PrivateUsers",
        "ProtectControlGroups",
        "ExecStart",
        "ReadWritePaths",
        "EnvironmentFile",
    ] {
        assert!(
            get(&d, bad).is_empty(),
            "{bad}= must not be set by 80-nonroot.conf"
        );
    }
}

#[test]
fn dropin_hands_config_yaml_over_as_a_credential() {
    let d = directives(&read_repo(DROPIN));
    let lc = get(&d, "LoadCredential");
    assert_eq!(lc.len(), 1, "{lc:?}");
    let (name, src) = lc[0].split_once(':').expect("LoadCredential=name:path");
    assert_eq!(src, format!("{LIVE_HOME}/config.yaml"));
    assert!(!name.contains('/') && !name.is_empty());
    assert_eq!(
        env_of(&d, "HERMES_RS_CONFIG").as_deref(),
        Some(format!("%d/{name}").as_str()),
        "HERMES_RS_CONFIG must point into $CREDENTIALS_DIRECTORY"
    );
}

#[test]
fn dropin_copies_legacy_db_into_the_state_dir() {
    let d = directives(&read_repo(DROPIN));
    let pre = get(&d, "ExecStartPre");
    assert_eq!(pre.len(), 1, "{pre:?}");
    let argv: Vec<&str> = pre[0].split_whitespace().collect();
    // "+" = privileged (root, no sandboxing) so it can read the 0700 home.
    assert_eq!(argv[0], "+/usr/bin/install");
    let legacy = env_of(&d, "HERMES_RS_LEGACY_STATE_DB").expect("legacy db env");
    assert!(legacy.starts_with("/var/lib/hermes-rs/"), "{legacy}");
    assert_eq!(argv.last(), Some(&legacy.as_str()));
    assert_eq!(argv[argv.len() - 2], format!("{LIVE_HOME}/state.db"));
    let s = argv.join(" ");
    for must in ["-m 0400", "-o hermes-rs", "-g hermes-rs", "-C"] {
        assert!(s.contains(must), "{must} missing in {s}");
    }
}

#[test]
fn dropin_layers_on_the_live_and_staged_units() {
    // The live unit (mirrored in the docs) and the staged base both keep
    // the state dir as the only writable path; the copy target is inside it.
    let base = directives(&read_repo("deploy/staged/hermes-rs.service"));
    assert_eq!(get(&base, "StateDirectory"), ["hermes-rs"]);
    assert_eq!(get(&base, "ReadWritePaths"), ["/var/lib/hermes-rs"]);
    assert!(get(&base, "User").is_empty(), "base unit still root today");
    // Coding drop-in must stay compatible (Delegate works for User= units).
    let coding = directives(&read_repo(
        "deploy/staged/hermes-rs.service.d/coding-tools.conf",
    ));
    assert!(!get(&coding, "Delegate").is_empty());
    assert!(get(&coding, "User").is_empty());
    for (k, v) in get(&coding, "Environment")
        .iter()
        .filter_map(|v| v.split_once('='))
    {
        if k.ends_with("_WORKTREE") || k.ends_with("_STATE") {
            assert!(v.starts_with("/var/lib/hermes-rs/"), "{k}={v}");
        }
    }
}

#[test]
fn sysusers_line_matches_the_unit() {
    let text = read_repo(SYSUSERS);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("u "))
        .expect("u line");
    // u NAME ID "GECOS" HOME SHELL
    let (head, rest) = line.split_once('"').unwrap();
    let (_gecos, tail) = rest.split_once('"').unwrap();
    let h: Vec<&str> = head.split_whitespace().collect();
    let t: Vec<&str> = tail.split_whitespace().collect();
    assert_eq!(h, ["u", "hermes-rs", "-"]);
    assert_eq!(t, ["/var/lib/hermes-rs", "/usr/sbin/nologin"]);
}

#[test]
fn hermes_rs_config_override_is_what_the_credential_path_uses() {
    // Library-level proof that HERMES_RS_CONFIG wins over $HERMES_HOME/config.yaml.
    let d = std::env::temp_dir().join(format!("hnr318-cfg-{}", std::process::id()));
    std::fs::create_dir_all(d.join("home")).unwrap();
    std::fs::write(d.join("home/config.yaml"), "model:\n  default: from-home\n").unwrap();
    std::fs::write(d.join("cred.yaml"), "model:\n  default: from-credential\n").unwrap();
    let home = d.join("home").display().to_string();
    let cred = d.join("cred.yaml").display().to_string();
    let env = |k: &str| match k {
        "HERMES_HOME" => Some(home.clone()),
        "HERMES_RS_CONFIG" => Some(cred.clone()),
        _ => None,
    };
    let c = hermes_rs::config::Config::load(&env).unwrap();
    assert_eq!(c.default_model, "from-credential");
    let env2 = |k: &str| (k == "HERMES_HOME").then(|| home.clone());
    assert_eq!(
        hermes_rs::config::Config::load(&env2)
            .unwrap()
            .default_model,
        "from-home"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------- real

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1")
        && std::env::var("GALION_NONROOT_ON_VPS").as_deref() == Ok("1");
    if !on {
        eprintln!(
            "SKIP: real_nonroot_* need GALION_REAL=1 GALION_NONROOT_ON_VPS=1 (VPS, non-root)"
        );
    }
    on
}

fn status_field(k: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
        .unwrap_or_default()
}

fn bin() -> PathBuf {
    std::env::var_os("HNR_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn port(i: u16) -> u16 {
    std::env::var("HNR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(21880)
        + i
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Base env of the live unit (paths only; no secret), state remapped into `d`.
fn gw(d: &Path, envs: &[(&str, String)], args: &[&str]) -> Command {
    let mut c = Command::new(bin());
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("API_SERVER_KEY", KEY)
        .env("HERMES_HOME", LIVE_HOME)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_STATE_DB", d.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", d.join("rs-spend.json"))
        .env("HERMES_RS_CRON_STATE", d.join("rs-cron-state.json"))
        .env("HERMES_RS_CRON_OUTPUT", d.join("rs-cron-output"))
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_MOCK_DELAY_MS", "0")
        .args(args)
        .stdin(Stdio::null());
    for (k, v) in envs {
        c.env(k, v);
    }
    c
}

fn http(port: u16, method: &str, path: &str, sid: Option<&str>, body: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(s) = sid {
        req.push_str(&format!("X-Hermes-Session-Id: {s}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    let _ = s.write_all(req.as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .to_string();
    (code, body)
}

fn wait_health(p: u16, child: &mut Child) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if let Ok(Some(_)) = child.try_wait() {
            return false;
        }
        if http(p, "GET", "/health", None, "").0 == 200 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn check_config(d: &Path, envs: &[(&str, String)]) -> String {
    let o = gw(d, envs, &["--check-config"]).output().unwrap();
    assert!(o.status.success(), "--check-config: {o:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn field<'a>(line: &'a str, k: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(k)?.strip_prefix('='))
        .unwrap_or("")
}

fn chat_body(text: &str) -> String {
    serde_json::json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]})
        .to_string()
}

#[test]
fn real_nonroot_gateway_on_vps() {
    if !real() {
        return;
    }
    let uid = status_field("Uid:");
    let euid = uid.split_whitespace().nth(1).unwrap_or("0").to_string();
    assert_ne!(euid, "0", "must run as a non-root user (got Uid: {uid})");
    let umask = status_field("Umask:");
    eprintln!("RUN as Uid: {uid} Umask: {umask} bin={}", bin().display());
    let base = std::env::var_os("SANDBOX_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("hnr318-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // --- host facts: a bare User= cannot see the live home (EACCES only).
    let cfg_err = std::fs::metadata(format!("{LIVE_HOME}/config.yaml")).unwrap_err();
    let db_err = std::fs::File::open(format!("{LIVE_HOME}/state.db")).unwrap_err();
    eprintln!(
        "HOST config.yaml stat: {:?}; state.db open: {:?}",
        cfg_err.kind(),
        db_err.kind()
    );
    assert_eq!(cfg_err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(db_err.kind(), std::io::ErrorKind::PermissionDenied);

    // --- hazard A: config.yaml is skipped silently (output == pure defaults).
    let a = root.join("a");
    std::fs::create_dir_all(&a).unwrap();
    let bare = check_config(&a, &[]);
    let defaults = check_config(
        &a,
        &[(
            "HERMES_RS_CONFIG",
            a.join("absent.yaml").display().to_string(),
        )],
    );
    eprintln!(
        "HAZARD A: bare User= check-config == defaults: {} (default_model={})",
        bare == defaults,
        field(&bare, "default_model")
    );
    assert_eq!(bare, defaults, "config.yaml was read?");

    // --- hazard B: the live legacy path makes the gateway exit (crash loop).
    let b = root.join("b");
    std::fs::create_dir_all(&b).unwrap();
    let t = Instant::now();
    let o = gw(
        &b,
        &[
            ("HERMES_RS_PORT", port(0).to_string()),
            ("HERMES_RS_LEGACY_STATE_DB", format!("{LIVE_HOME}/state.db")),
        ],
        &["--mock"],
    )
    .stderr(Stdio::piped())
    .stdout(Stdio::null())
    .spawn()
    .unwrap();
    let mut o = Proc(o);
    let mut exited = None;
    while t.elapsed() < Duration::from_secs(5) {
        if let Ok(Some(s)) = o.0.try_wait() {
            exited = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut err = String::new();
    if let Some(mut e) = o.0.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let up = http(port(0), "GET", "/health", None, "").0;
    eprintln!(
        "HAZARD B: exit={exited:?} after {} ms, /health={up}, stderr: {}",
        t.elapsed().as_millis(),
        err.trim()
    );
    let st = exited.expect("gateway should exit on the unreadable legacy db");
    assert!(!st.success());
    assert!(err.contains("state.db"), "{err}");
    drop(o);

    // --- staged shape: credential config + 0400 legacy copy in the state dir.
    let s = root.join("state");
    let cred = root.join("cred");
    std::fs::create_dir_all(&s).unwrap();
    std::fs::create_dir_all(&cred).unwrap();
    let cfg = cred.join("hermes-config.yaml");
    std::fs::write(&cfg, "model:\n  default: nonroot-318-model\n").unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o400)).unwrap();
    let cfg_env = ("HERMES_RS_CONFIG", cfg.display().to_string());
    let staged_line = check_config(&s, std::slice::from_ref(&cfg_env));
    eprintln!(
        "STAGED check-config default_model={}",
        field(&staged_line, "default_model")
    );
    assert_eq!(field(&staged_line, "default_model"), "nonroot-318-model");
    assert_eq!(field(&staged_line, "home"), LIVE_HOME);

    // Seed a legacy db with one session (stands in for the 40 MB copy).
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    let sid = format!("hnr318-{}", std::process::id());
    {
        let mut g = Proc(
            gw(
                &seed,
                &[("HERMES_RS_PORT", port(1).to_string()), cfg_env.clone()],
                &["--mock"],
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        );
        assert!(
            wait_health(port(1), &mut g.0),
            "seed gateway did not come up"
        );
        let (c, _) = http(
            port(1),
            "POST",
            "/v1/chat/completions",
            Some(&sid),
            &chat_body("seed 318"),
        );
        assert_eq!(c, 200, "seed chat");
    }
    let legacy = s.join("legacy-state.db");
    std::fs::copy(seed.join("rs-state.db"), &legacy).unwrap();
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o400)).unwrap();

    let mut g = Proc(
        gw(
            &s,
            &[
                ("HERMES_RS_PORT", port(2).to_string()),
                cfg_env.clone(),
                ("HERMES_RS_LEGACY_STATE_DB", legacy.display().to_string()),
            ],
            &["--mock"],
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap(),
    );
    let t = Instant::now();
    assert!(
        wait_health(port(2), &mut g.0),
        "staged gateway did not come up"
    );
    eprintln!("STAGED up in {} ms on {}", t.elapsed().as_millis(), port(2));
    let models = http(port(2), "GET", "/v1/models", None, "");
    let sess = http(port(2), "GET", &format!("/api/sessions/{sid}"), None, "");
    let chat = http(
        port(2),
        "POST",
        "/v1/chat/completions",
        None,
        &chat_body("staged 318"),
    );
    let noauth = {
        let mut st = TcpStream::connect(("127.0.0.1", port(2))).unwrap();
        st.write_all(b"GET /v1/models HTTP/1.0\r\nHost: x\r\n\r\n")
            .unwrap();
        let mut o = String::new();
        let _ = st.read_to_string(&mut o);
        o.split_whitespace().nth(1).unwrap_or("").to_string()
    };
    eprintln!(
        "STAGED /v1/models={} legacy session {sid}={} chat={} no-auth models={noauth}",
        models.0, sess.0, chat.0
    );
    assert_eq!(models.0, 200);
    assert_eq!(sess.0, 200, "legacy session not served: {}", sess.1);
    assert!(sess.1.contains(&sid));
    assert_eq!(chat.0, 200);
    assert_eq!(noauth, "401");
    drop(g);

    // Files the non-root gateway created: owned by us, no group/other bits
    // when the unit's UMask=0077 is in force.
    let me: u32 = euid.parse().unwrap();
    for f in ["rs-state.db", "rs-spend.json"] {
        let m = std::fs::metadata(s.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        let mode = m.permissions().mode() & 0o777;
        eprintln!("STATE {f} uid={} mode={mode:o}", m.uid());
        assert_eq!(m.uid(), me);
        if umask == "0077" {
            assert_eq!(mode & 0o077, 0, "{f} mode {mode:o}");
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}
