// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Build the stack copy for one run from a redacted layout snapshot.
//!
//! Every live absolute path under the mirrored prefixes is moved under
//! `<run>/root` (so `/opt/galion/hermes/config.yaml` becomes
//! `<run>/root/opt/galion/hermes/config.yaml`), every network address is
//! pointed at a loopback stub, and every secret is replaced with a fresh
//! dummy value. Anything that cannot be mapped makes the build fail closed.

use crate::util::{nonce, sha256_bytes};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Live path prefixes mirrored under `<run>/root` (longest match wins).
pub const PREFIXES: &[&str] = &[
    "/usr/local/lib/hermes-self-modify",
    "/usr/local/lib/hermes-browser",
    "/var/lib/openmind-mcp-stack",
    "/etc/openmind-mcp-stack",
    "/var/lib/openmind-hermes",
    "/etc/hermes-self-modify",
    "/srv/hermes-workspace",
    "/var/lib/galion-gate",
    "/var/lib/hermes-rs",
    "/etc/hermes-rs",
    "/var/backups",
    "/opt/galion",
];

/// Absolute-path roots that must never appear unmapped in the gateway env
/// (a write there could touch live state when the harness runs on the VPS).
const LIVE_ROOTS: &[&str] = &[
    "/etc/",
    "/var/",
    "/opt/",
    "/srv/",
    "/root/",
    "/home/",
    "/usr/local/",
    "/run/",
];

/// openmind-mcp-stack 61f9c08c built-in stack defaults (mirrored when the
/// live host has no stack.conf, which is the case on 2026-10-31).
const STACK_DEFAULTS: &[(&str, &str)] = &[
    ("nerve_url", "http://127.0.0.1:8670"),
    ("cortex_url", "http://127.0.0.1:8674"),
    ("queue_url", "http://127.0.0.1:19120"),
    ("spend_file", "/var/lib/hermes-rs/rs-spend.json"),
    ("backup_dir", "/var/backups/galion"),
    ("docs_root", "/opt/galion/galion-light/docs"),
    (
        "readiness_file",
        "/var/lib/openmind-mcp-stack/hermes-readiness.json",
    ),
];

/// Ed25519 public key of the all-zero seed: a key nobody uses, so nothing
/// signed in the copy can ever verify against a real key.
pub const DUMMY_PUB: &str = "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29";

#[derive(Clone, Debug)]
pub struct Ports {
    pub gw: u16,
    pub model: u16,
    pub memory: u16,
    pub nerve: u16,
    pub cortex: u16,
    pub queue: u16,
    pub sink: u16,
}

impl Ports {
    pub fn all(&self) -> Vec<u16> {
        vec![
            self.gw,
            self.model,
            self.memory,
            self.nerve,
            self.cortex,
            self.queue,
            self.sink,
        ]
    }
    /// Live loopback port -> stub port.
    fn map_port(&self, live: u16) -> u16 {
        match live {
            8420 => self.memory,
            8670 => self.nerve,
            8674 => self.cortex,
            19120 => self.queue,
            11434 => self.model,
            8642 => self.gw,
            _ => self.sink,
        }
    }
}

pub struct Built {
    pub root: PathBuf,
    pub gateway: PathBuf,
    pub gateway_args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub dummies: BTreeMap<String, String>,
    pub api_key: String,
    pub env_file: PathBuf,
    pub hermes_home: PathBuf,
    pub workspace: PathBuf,
    pub deviations: Vec<String>,
    pub fingerprints: Vec<(String, String)>,
    pub vault_canary: String,
}

struct Mapper<'a> {
    root: &'a Path,
    ports: &'a Ports,
}

impl Mapper<'_> {
    fn root_s(&self) -> String {
        self.root.display().to_string()
    }

    /// Single left-to-right pass, so an already mapped path is never mapped
    /// twice (the run dir itself may live under a mirrored prefix).
    fn paths(&self, s: &str) -> String {
        let b = s.as_bytes();
        let mut out = String::with_capacity(s.len() + 64);
        let mut i = 0;
        while i < s.len() {
            let prev_ok = i == 0 || {
                let c = b[i - 1] as char;
                !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '$'))
            };
            let mut hit = None;
            if prev_ok && b[i] == b'/' {
                for p in PREFIXES {
                    if s[i..].starts_with(p) {
                        let next = s[i + p.len()..].chars().next();
                        if next.is_none_or(|c| {
                            !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                        }) {
                            hit = Some(*p);
                            break;
                        }
                    }
                }
            }
            if let Some(p) = hit {
                out.push_str(&self.root_s());
                out.push_str(p);
                i += p.len();
            } else {
                let c = s[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
        out
    }

    /// Rewrite every http(s) URL: loopback/`<IP>` hosts keep their path and
    /// go to the matching stub; model-provider URLs (`base_url`, `api`) go
    /// to the fake model; anything else goes to the sink.
    fn urls(&self, line: &str) -> String {
        let key = line
            .trim_start()
            .trim_start_matches("- ")
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .to_ascii_lowercase();
        let env_key = line.split('=').next().unwrap_or("").to_ascii_uppercase();
        let provider_ctx = matches!(key.as_str(), "base_url" | "api" | "api_base" | "endpoint")
            || env_key.ends_with("BASE_URL");
        let mut out = String::new();
        let mut rest = line;
        loop {
            let pos = [rest.find("http://"), rest.find("https://")]
                .into_iter()
                .flatten()
                .min();
            let Some(p) = pos else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..p]);
            let u = &rest[p..];
            let end = u
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ']' | '}' | ')' | '>' | '`')
                })
                .unwrap_or(u.len());
            let url = &u[..end];
            let after_scheme = url.split_once("://").map(|x| x.1).unwrap_or("");
            let (auth, path) = match after_scheme.find('/') {
                Some(i) => (&after_scheme[..i], &after_scheme[i..]),
                None => (after_scheme, ""),
            };
            let (host, port) = match auth.rsplit_once(':') {
                Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
                    (h, p.parse::<u16>().ok())
                }
                _ => (auth, None),
            };
            let loop_host = matches!(
                host,
                "127.0.0.1" | "localhost" | "[::1]" | "<IP>" | "0.0.0.0"
            );
            let new = if loop_host {
                let live = port.unwrap_or(80);
                format!("http://127.0.0.1:{}{}", self.ports.map_port(live), path)
            } else if provider_ctx {
                format!("http://127.0.0.1:{}{}", self.ports.model, path)
            } else {
                format!(
                    "http://127.0.0.1:{}/blocked/{}{}",
                    self.ports.sink,
                    host.replace(['[', ']'], ""),
                    path
                )
            };
            out.push_str(&new);
            rest = &u[end..];
        }
        out
    }

    fn line(&self, l: &str, dummy: &str) -> String {
        let l = l.replace("<IP>", "127.0.0.1").replace("<REDACTED>", dummy);
        self.paths(&self.urls(&l))
    }
}

/// Non-comment lines of a generated file that still point off-box or still
/// carry a redaction marker.
fn leftovers(text: &str) -> Vec<String> {
    let mut v = vec![];
    for (i, l) in text.lines().enumerate() {
        let t = l.trim_start();
        if t.starts_with('#') {
            continue;
        }
        let code = match t.find(" #") {
            Some(i) => &t[..i],
            None => t,
        };
        if code.contains("<REDACTED>") || code.contains("<IP>") {
            v.push(format!("line {}: redaction marker", i + 1));
        }
        let mut r = code;
        while let Some(p) = r.find("://") {
            let host = r[p + 3..]
                .split(['/', ':', '"', '\'', ' ', ',', ']', '}'])
                .next()
                .unwrap_or("");
            if host != "127.0.0.1"
                && !host.is_empty()
                && !host.starts_with('$')
                && !host.starts_with("${")
            {
                v.push(format!("line {}: non-loopback URL host", i + 1));
            }
            r = &r[p + 3..];
        }
    }
    v
}

fn secret_name(k: &str) -> bool {
    let k = k.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "SEED",
        "BEARER",
        "COOKIE",
        "CREDENTIAL",
    ]
    .iter()
    .any(|s| k.contains(s))
}

/// systemd `Environment=` value -> assignments (handles "A=b c" quoting).
fn env_assignments(v: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    let mut push = |s: &mut String| {
        if let Some((k, val)) = s.split_once('=') {
            out.push((k.to_string(), val.to_string()));
        }
        s.clear();
    };
    for c in v.chars() {
        match c {
            '"' => q = !q,
            c if c.is_whitespace() && !q => push(&mut cur),
            c => cur.push(c),
        }
    }
    push(&mut cur);
    out
}

pub struct Unit {
    pub exec: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_files: Vec<String>,
    pub unset: Vec<String>,
}

pub fn parse_unit(text: &str) -> Unit {
    let mut u = Unit {
        exec: vec![],
        env: vec![],
        env_files: vec![],
        unset: vec![],
    };
    let mut in_service = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_service = l == "[Service]";
            continue;
        }
        if !in_service || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        match k.trim() {
            "ExecStart" => {
                u.exec = v.split_whitespace().map(String::from).collect();
                if let Some(f) = u.exec.first_mut() {
                    *f = f.trim_start_matches(['-', '@', '+', '!', ':']).to_string();
                }
            }
            "Environment" => {
                if v.trim().is_empty() {
                    u.env.clear();
                } else {
                    u.env.extend(env_assignments(v));
                }
            }
            "EnvironmentFile" => {
                if !v.trim().is_empty() {
                    u.env_files
                        .push(v.trim().trim_start_matches('-').to_string());
                }
            }
            "UnsetEnvironment" => u.unset.extend(v.split_whitespace().map(String::from)),
            _ => {}
        }
    }
    u
}

fn write(p: &Path, s: &str) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(p, s).map_err(|e| format!("{}: {e}", p.display()))
}

/// Secrets files (dummy values) get the live mode: 0600.
fn write_secret(p: &Path, s: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    write(p, s)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn copy_exec(src: &Path, dst: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(d) = dst.parent() {
        mkdir(d)?;
    }
    std::fs::copy(src, dst)
        .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

pub struct Inputs<'a> {
    pub layout: &'a Path,
    pub run: &'a Path,
    pub candidate: &'a Path,
    pub stack_bin: Option<&'a Path>,
    pub self_exe: &'a Path,
    pub ports: &'a Ports,
    pub nonce: &'a str,
}

pub fn build(i: &Inputs) -> Result<Built, String> {
    let read =
        |n: &str| std::fs::read_to_string(i.layout.join(n)).map_err(|e| format!("layout {n}: {e}"));
    let root = i.run.join("root");
    mkdir(&root)?;
    let m = Mapper {
        root: &root,
        ports: i.ports,
    };
    let mut dummies = BTreeMap::new();
    let mut dummy = |label: &str| {
        let v = format!(
            "dryrun-dummy-{}-{}",
            label.to_ascii_lowercase().replace('_', "-"),
            nonce()
        );
        dummies.insert(v.clone(), label.to_string());
        v
    };
    let mut deviations: Vec<String> = vec![];
    let mut fingerprints = vec![];

    // ---- unit -> env --------------------------------------------------------
    let unit = parse_unit(&read("unit.txt")?);
    if unit.exec.is_empty() {
        return Err("layout unit.txt has no ExecStart".into());
    }
    let live_exec = unit.exec[0].clone();
    let mut env: Vec<(String, String)> = unit.env.clone();
    let env_names: Vec<String> = read("env-names.txt")
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut dot_env =
        String::from("# dry-run copy: every value is a dummy (live values are never read)\n");
    for n in &env_names {
        let v = if n.ends_with("_HOST") {
            "127.0.0.1".to_string()
        } else if n.ends_with("_PORT") {
            if n.contains("MEMORY") || n.contains("TDAI") {
                i.ports.memory
            } else {
                i.ports.sink
            }
            .to_string()
        } else {
            dummy(n)
        };
        dot_env.push_str(&format!("{n}={v}\n"));
        // EnvironmentFile= wins over Environment= in systemd
        env.retain(|(k, _)| k != n);
        env.push((n.clone(), v));
    }
    env.retain(|(k, _)| !unit.unset.contains(k));
    let mut out_env: Vec<(String, String)> = vec![];
    let mut api_key = String::new();
    for (k, v) in env {
        let v2 = if k == "HERMES_RS_HOST" || k == "API_SERVER_HOST" {
            "127.0.0.1".to_string()
        } else if k == "HERMES_RS_PORT" || k == "API_SERVER_PORT" {
            i.ports.gw.to_string()
        } else if secret_name(&k)
            && !v.starts_with('/')
            && !v.starts_with("dryrun-dummy-")
            && !v.starts_with("${")
        {
            dummy(&k)
        } else {
            m.line(&v, "dryrun-dummy-redacted")
        };
        if k == "API_SERVER_KEY" {
            api_key = v2.clone();
        }
        out_env.push((k, v2));
    }
    let set = |e: &mut Vec<(String, String)>, k: &str, v: Option<String>| {
        e.retain(|(x, _)| x != k);
        if let Some(v) = v {
            e.push((k.to_string(), v));
        }
    };
    if api_key.len() < 16 {
        api_key = dummy("API_SERVER_KEY");
        set(&mut out_env, "API_SERVER_KEY", Some(api_key.clone()));
    }
    let getv = |e: &Vec<(String, String)>, k: &str| {
        e.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v.clone())
    };

    // deviations from live, each one recorded
    let mut dev = |e: &mut Vec<(String, String)>, k: &str, v: Option<String>, why: &str| {
        let had = getv(e, k).is_some();
        if had || v.is_some() {
            deviations.push(format!("{k}: {why}"));
            set(e, k, v);
        }
    };
    dev(
        &mut out_env,
        "HERMES_RS_MCP_RUN_AS",
        None,
        "unset (no uid switch possible in an unprivileged namespace; MCP runs as the test user)",
    );
    dev(
        &mut out_env,
        "HERMES_RS_SELF_MODIFY_RUN_AS",
        None,
        "unset (needs the root-only gate CLI; propose_self_change still registers)",
    );
    dev(
        &mut out_env,
        "HERMES_RS_SELF_MODIFY_CLI",
        None,
        "unset (not used without RUN_AS)",
    );
    if getv(&out_env, "HERMES_RS_TOOLS_RUN_AS").is_some() {
        dev(
            &mut out_env,
            "HERMES_RS_TOOLS_RUN_AS",
            Some("gateway".into()),
            "gateway (tools run as the test user; live 65534:65534)",
        );
    }
    dev(
        &mut out_env,
        "HERMES_RS_LEGACY_STATE_DB",
        None,
        "unset (private chat history is not copied)",
    );
    if let Some(b) = getv(&out_env, "HERMES_RS_BROWSER_BIN") {
        if !Path::new(&b).exists() {
            dev(
                &mut out_env,
                "HERMES_RS_BROWSER_BIN",
                None,
                "unset (browser bundle not copied)",
            );
            dev(
                &mut out_env,
                "HERMES_RS_TOOLS_BROWSER",
                Some("0".into()),
                "0 (browser tool off in the copy)",
            );
        }
    }
    dev(
        &mut out_env,
        "HERMES_RS_PROVIDER_LOOPBACK_ONLY",
        Some("1".into()),
        "1 (extra guard; the namespace already blocks egress)",
    );
    let home = i.run.join("home");
    mkdir(&home)?;
    set(&mut out_env, "HOME", Some(home.display().to_string()));
    set(&mut out_env, "PATH", Some("/usr/bin:/bin".into()));
    set(&mut out_env, "LANG", Some("C.UTF-8".into()));
    set(&mut out_env, "TZ", Some("Europe/Zagreb".into()));

    // ---- files in the copy ---------------------------------------------------
    let hermes_home =
        PathBuf::from(getv(&out_env, "HERMES_HOME").ok_or("unit has no HERMES_HOME")?);
    mkdir(&hermes_home)?;
    let env_file = hermes_home.join(".env");
    write_secret(&env_file, &dot_env)?;
    for f in &unit.env_files {
        let mf = PathBuf::from(m.paths(f));
        if mf != env_file {
            write_secret(&mf, &dot_env)?;
        }
    }
    let dummy_cfg = dummy("CONFIG_REDACTED");
    let transform = |text: &str| -> String {
        let mut o = String::new();
        for l in text.lines() {
            o.push_str(&m.line(l, &dummy_cfg));
            o.push('\n');
        }
        o
    };
    // config.yaml
    let stack_conf = root.join("etc/openmind-mcp-stack/stack.conf");
    let add_stack_conf = |s: String| -> String {
        s.lines()
            .map(|l| {
                let t = l.trim_start();
                if !t.starts_with('#')
                    && t.starts_with("args:")
                    && l.contains("--vault")
                    && !l.contains("--stack-conf")
                {
                    if let Some(p) = l.rfind(']') {
                        return format!(
                            "{}, \"--stack-conf\", \"{}\"{}",
                            &l[..p],
                            stack_conf.display(),
                            &l[p..]
                        );
                    }
                }
                l.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    let cfg = add_stack_conf(transform(&read("config.yaml")?));
    let left = leftovers(&cfg);
    if !left.is_empty() {
        return Err(format!(
            "config.yaml copy still points off-box: {}",
            left.join("; ")
        ));
    }
    write(&hermes_home.join("config.yaml"), &cfg)?;
    fingerprints.push(("config.yaml (copy)".into(), sha256_bytes(cfg.as_bytes())));
    if let Ok(s) = read("SOUL.md") {
        write(
            &hermes_home.join("SOUL.md"),
            &s.replace("<IP>", "127.0.0.1"),
        )?;
    }
    // MCP servers file
    let mut mcp_text = String::new();
    if let Some(f) = getv(&out_env, "HERMES_RS_MCP_SERVERS_FILE") {
        mcp_text = add_stack_conf(transform(&read("mcp-servers.yaml")?));
        let left = leftovers(&mcp_text);
        if !left.is_empty() {
            return Err(format!(
                "mcp-servers.yaml copy still points off-box: {}",
                left.join("; ")
            ));
        }
        write(Path::new(&f), &mcp_text)?;
        fingerprints.push((
            "mcp-servers.yaml (copy)".into(),
            sha256_bytes(mcp_text.as_bytes()),
        ));
    }
    if cfg.contains("--stack-conf") || mcp_text.contains("--stack-conf") {
        deviations.push("openmind args: + --stack-conf <copy> (points the stack tools at stubs; live uses built-in defaults)".into());
    }
    // run_as lines: an unprivileged namespace cannot switch uid
    let strip_run_as = |p: &Path| -> Result<bool, String> {
        let t = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
        if !t.lines().any(|l| l.trim_start().starts_with("run_as:")) {
            return Ok(false);
        }
        let n: String = t
            .lines()
            .map(|l| {
                if l.trim_start().starts_with("run_as:") {
                    format!(
                        "{}# dry-run: {}",
                        &l[..l.len() - l.trim_start().len()],
                        l.trim_start()
                    )
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        write(p, &n)?;
        Ok(true)
    };
    let mut stripped = strip_run_as(&hermes_home.join("config.yaml"))?;
    if let Some(f) = getv(&out_env, "HERMES_RS_MCP_SERVERS_FILE") {
        stripped |= strip_run_as(Path::new(&f))?;
    }
    if stripped {
        deviations.push("MCP run_as lines commented out (no uid switch in the namespace)".into());
    }
    // per-server env files: dummies for the names the live files hold
    let mut cur: Option<String> = None;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for l in read("mcp.d.txt").unwrap_or_default().lines() {
        if let Some(n) = l.strip_prefix("## ") {
            cur = Some(n.trim().to_string());
            files.entry(n.trim().to_string()).or_default();
        } else if let (Some(c), n) = (&cur, l.trim()) {
            if !n.is_empty() && n.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                let v = dummy(n);
                files.get_mut(c).unwrap().push_str(&format!("{n}={v}\n"));
            }
        }
    }
    for (n, body) in &files {
        write_secret(&root.join("etc/hermes-rs/mcp.d").join(n), body)?;
    }
    // MCP commands: the openmind stack binary, a zero-tool stub for the rest
    for l in cfg.lines().chain(mcp_text.lines()) {
        let t = l.trim_start();
        if t.starts_with('#') {
            continue;
        }
        let Some(c) = t.strip_prefix("command:") else {
            continue;
        };
        let c = c.trim().trim_matches('"');
        if !c.starts_with(&root.display().to_string()) {
            continue;
        }
        let p = Path::new(c);
        if p.exists() {
            continue;
        }
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        match (name.starts_with("openmind-mcp-stack"), i.stack_bin) {
            (true, Some(sb)) => {
                copy_exec(sb, p)?;
                fingerprints.push((format!("{name} (copy)"), crate::util::sha256_file(p)?.0));
            }
            _ => {
                copy_exec(i.self_exe, p)?;
                deviations.push(format!("MCP command {name}: zero-tool stub"));
            }
        }
    }
    // stack.conf (live file if any, else the built-in defaults), mapped
    let sc = match read("stack.conf") {
        Ok(s) if s.lines().any(|l| l.contains('=')) => s,
        _ => STACK_DEFAULTS
            .iter()
            .map(|(k, v)| format!("{k} = {v}\n"))
            .collect(),
    };
    let sc = transform(&sc);
    write(&stack_conf, &sc)?;
    for l in sc.lines() {
        if let Some((k, v)) = l.split_once('=') {
            let (k, v) = (k.trim(), v.trim());
            if k == "backup_dir" || k == "docs_root" {
                mkdir(Path::new(v))?;
            } else if k.ends_with("_file") {
                if let Some(d) = Path::new(v).parent() {
                    mkdir(d)?;
                }
            }
        }
    }
    // vault + docs fixtures (live notes are never copied)
    let vault_canary = format!("DRYRUN-VAULT-CANARY-{}", i.nonce);
    for l in cfg.lines().chain(mcp_text.lines()) {
        if let Some(p) = l.find("\"--vault\"") {
            let rest = &l[p + 9..];
            if let Some(v) = rest.split('"').nth(1) {
                write(&Path::new(v).join("dryrun-canary.md"), &format!("# Dry-run canary\n\n{vault_canary}: this note exists only in the dry-run copy.\n"))?;
            }
        }
    }
    let docs = root.join("opt/galion/galion-light/docs");
    write(
        &docs.join("dryrun/README.md"),
        "# Dry-run docs fixture\n\nOnly in the copy.\n",
    )?;
    // state dirs and self-modify fixtures
    for (k, v) in out_env.clone() {
        let p = Path::new(&v);
        if !p.is_absolute() || !v.starts_with(&root.display().to_string()) {
            continue;
        }
        match k.as_str() {
            "HERMES_RS_TOOLS_WORKSPACE"
            | "HERMES_RS_SELF_MODIFY_REPO"
            | "HERMES_RS_SELF_MODIFY_QUEUE" => mkdir(p)?,
            "HERMES_RS_SELF_MODIFY_SANDBOX_PUB" => write(p, &format!("{DUMMY_PUB}\n"))?,
            "HERMES_RS_SELF_MODIFY_OWNER_PUB" => {} // missing live too (2026-10-31): leave missing
            "HERMES_RS_SELF_MODIFY_SANDBOX_KEY" => {} // must NOT exist/readable
            "HERMES_RS_SELF_MODIFY_GATE_CMD" => {
                let log = i.run.join("logs/gate-cmd.log");
                write(p, &format!("#!/bin/sh\necho \"$(date +%FT%T%z) gate-cmd called (dry-run stub, refused): $*\" >> '{}'\nexit 1\n", log.display()))?;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
                deviations.push(
                    "HERMES_RS_SELF_MODIFY_GATE_CMD: stub script that logs and refuses".into(),
                );
            }
            _ if k.ends_with("_DB")
                || k.ends_with("_FILE")
                || k.ends_with("_STATE")
                || k.ends_with("_OUTPUT") =>
            {
                if let Some(d) = p.parent() {
                    mkdir(d)?;
                }
            }
            _ => {}
        }
    }
    let workspace = PathBuf::from(
        getv(&out_env, "HERMES_RS_TOOLS_WORKSPACE")
            .unwrap_or_else(|| root.join("srv/hermes-workspace").display().to_string()),
    );
    mkdir(&workspace)?;

    // ---- fail closed on anything unmapped -------------------------------------
    let rs = root.display().to_string();
    let mut unmapped = vec![];
    for (k, v) in &out_env {
        if k == "PATH" {
            continue;
        }
        for tok in v.split([' ', ':', ',', '=']) {
            if tok.starts_with('/')
                && !tok.starts_with(&rs)
                && LIVE_ROOTS.iter().any(|r| tok.starts_with(r))
                && !tok.starts_with(&i.run.display().to_string())
            {
                unmapped.push(format!("{k} -> {tok}"));
            }
        }
        if v.contains("<IP>") || v.contains("<REDACTED>") {
            unmapped.push(format!("{k}: redaction marker"));
        }
        if let Some(p) = v.find("://") {
            let host = v[p + 3..].split(['/', ':']).next().unwrap_or("");
            if host != "127.0.0.1" {
                unmapped.push(format!("{k}: non-loopback URL"));
            }
        }
    }
    if !unmapped.is_empty() {
        return Err(format!(
            "refusing to start: unmapped live paths/addresses in the env: {}",
            unmapped.join("; ")
        ));
    }

    // ---- the candidate, at the mirrored path of the live binary ---------------
    let live_dir = Path::new(&live_exec)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/opt/galion/rust/bin".into());
    let gateway = PathBuf::from(m.paths(&live_dir)).join("hermes-gateway-dryrun-candidate");
    copy_exec(i.candidate, &gateway)?;
    deviations.push(format!(
        "ExecStart {live_exec} -> candidate copy {}",
        gateway.display()
    ));
    let gateway_args = unit.exec[1..]
        .iter()
        .map(|a| m.line(a, &dummy_cfg))
        .collect();

    out_env.sort();
    Ok(Built {
        root,
        gateway,
        gateway_args,
        env: out_env,
        dummies,
        api_key,
        env_file,
        hermes_home,
        workspace,
        deviations,
        fingerprints,
        vault_canary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ports() -> Ports {
        Ports {
            gw: 19001,
            model: 19002,
            memory: 19003,
            nerve: 19004,
            cortex: 19005,
            queue: 19006,
            sink: 19007,
        }
    }
    #[test]
    fn maps_paths_once() {
        let p = ports();
        let root = PathBuf::from("/var/lib/galion-gate/dry/root");
        let m = Mapper {
            root: &root,
            ports: &p,
        };
        assert_eq!(
            m.paths("/opt/galion/hermes"),
            "/var/lib/galion-gate/dry/root/opt/galion/hermes"
        );
        assert_eq!(m.paths("x=/opt/galionx"), "x=/opt/galionx");
        assert_eq!(m.paths("/var/backups-old"), "/var/backups-old");
        assert_eq!(
            m.paths("a /etc/hermes-rs/mcp.d"),
            "a /var/lib/galion-gate/dry/root/etc/hermes-rs/mcp.d"
        );
    }
    #[test]
    fn maps_urls() {
        let p = ports();
        let root = PathBuf::from("/r");
        let m = Mapper {
            root: &root,
            ports: &p,
        };
        assert_eq!(
            m.line("  base_url: https://openrouter.ai/api/v1", "d"),
            "  base_url: http://127.0.0.1:19002/api/v1"
        );
        assert_eq!(
            m.line("NERVE=http://<IP>:8670", "d"),
            "NERVE=http://127.0.0.1:19004"
        );
        assert_eq!(
            m.line("    url: \"https://mcp.box.com\"", "d"),
            "    url: \"http://127.0.0.1:19007/blocked/mcp.box.com\""
        );
        assert_eq!(
            m.line("  api: http://<IP>:11434/v1", "d"),
            "  api: http://127.0.0.1:19002/v1"
        );
        assert!(leftovers("a: https://x.y\n# https://ok\n").len() == 1);
    }
    #[test]
    fn unit_parse() {
        let u = parse_unit("[Service]\nEnvironment=A=1 \"B=x y\"\nExecStart=\nExecStart=/opt/galion/rust/bin/g-1\nUnsetEnvironment=A\n[Install]\nEnvironment=C=3\n");
        assert_eq!(u.exec, vec!["/opt/galion/rust/bin/g-1"]);
        assert_eq!(
            u.env,
            vec![("A".into(), "1".into()), ("B".into(), "x y".into())]
        );
        assert_eq!(u.unset, vec!["A"]);
    }
}
