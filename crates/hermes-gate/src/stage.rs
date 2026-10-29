// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Sandbox deploy: the proposal is applied to a throwaway worktree at its
//! base commit, then every plan stage (build, test, a live-style deploy of
//! the built service on loopback inside the sandbox, smoke probes) runs
//! under `openmind_sandbox::run`: user, PID, mount and an empty network
//! namespace (only `lo`), read-only root, the seccomp deny-list, rlimits,
//! cleared env, no capabilities, live roots and key dirs hidden. Landlock
//! is NOT used (the VPS LXC kernel has none; the gate behaves the same on
//! every host). A stage whose required sandbox layers were not enforced
//! fails closed, even if its command exited 0.

use crate::proposal::Proposal;
use openmind_sandbox::{SandboxPolicy, Status};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Roots that are live on the VPS (or the owner's): a scratch dir, an extra
/// read path or an apply target under one of these is refused.
pub const LIVE_ROOTS: &[&str] = &[
    "/opt/galion",
    "/etc",
    "/root",
    "/var/lib",
    "/srv",
    "/usr",
    "/boot",
    "/run",
];

/// Layers every stage must have had enforced unless the plan names its own
/// list. `net` = no network (an empty network namespace).
pub const DEFAULT_REQUIRED: &[&str] = &[
    "rlimits",
    "env",
    "no_new_privs",
    "userns",
    "net",
    "pidns",
    "mountns",
    "ro_root",
    "seccomp",
    "caps",
];

/// The gate's own home on the VPS (`galion-gate` account): its repo copy,
/// scratch and staging dirs live here, so it is not a live root. Its key
/// dir is hidden from every sandbox stage (see [`policy`]).
pub const GATE_HOMES: &[&str] = &["/var/lib/galion-gate"];

/// Paths that are always live, even inside a gate home.
pub const ALWAYS_LIVE: &[&str] = &["/opt/galion/openmind", "/opt/galion/rust/src"];

fn norm(p: &Path) -> std::path::PathBuf {
    // Lexical clean-up first (`..` must not walk out of a gate home), then
    // the real path when it exists (symlinks).
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    std::fs::canonicalize(&out).unwrap_or(out)
}

/// True for a live path (or one that resolves to it). Pure: no process is
/// started, so callers check this before running any command.
pub fn under_live_root(p: &Path) -> bool {
    let n = norm(p);
    let hit = |q: &Path| {
        ALWAYS_LIVE.iter().any(|r| q.starts_with(r))
            || (LIVE_ROOTS.iter().any(|r| q.starts_with(r))
                && !GATE_HOMES.iter().any(|g| q.starts_with(g)))
    };
    hit(p)
        || hit(&n)
        || !p.is_absolute() && hit(&std::env::current_dir().unwrap_or_default().join(p))
}

#[derive(Clone, Debug, PartialEq)]
pub struct StageSpec {
    pub name: String,
    pub argv: Vec<String>,
    pub timeout_s: u64,
    pub mem_mb: u64,
    /// Loopback TCP ports this stage may bind/connect (live-style service
    /// boot). Needs a private netns; >= 1024, at most 8.
    pub loopback_ports: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub stages: Vec<StageSpec>,
    pub require_layers: Vec<String>,
    /// Extra read-only paths (e.g. a Rust toolchain). Never a live root.
    pub extra_read: Vec<PathBuf>,
    /// Extra env for every stage, keys from [`PLAN_ENV_KEYS`] only.
    pub env: Vec<(String, String)>,
}

/// Env keys a plan may set (toolchain location and build knobs only).
pub const PLAN_ENV_KEYS: &[&str] = &[
    "PATH",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_JOBS",
    "CARGO_NET_OFFLINE",
    "CARGO_INCREMENTAL",
];

impl Plan {
    pub fn from_json(v: &Value) -> Result<Plan, String> {
        let mut stages = vec![];
        for s in v["stages"].as_array().ok_or("plan: stages[] missing")? {
            let name = s["name"].as_str().ok_or("plan: stage name")?.to_string();
            if !crate::proposal::id_ok(&name) {
                return Err(format!("plan: bad stage name {name:?}"));
            }
            let argv: Vec<String> = s["argv"]
                .as_array()
                .ok_or("plan: stage argv")?
                .iter()
                .map(|a| {
                    a.as_str()
                        .map(str::to_string)
                        .ok_or("plan: argv must be strings")
                })
                .collect::<Result<_, _>>()?;
            if argv.is_empty() {
                return Err(format!("plan: stage {name} has an empty argv"));
            }
            let mut loopback_ports = vec![];
            for x in s["loopback_ports"].as_array().into_iter().flatten() {
                let port = x
                    .as_u64()
                    .filter(|p| (1024..=65535).contains(p))
                    .ok_or_else(|| format!("plan: stage {name}: loopback port {x} refused"))?;
                loopback_ports.push(port as u16);
            }
            if loopback_ports.len() > 8 {
                return Err(format!("plan: stage {name}: too many loopback ports"));
            }
            stages.push(StageSpec {
                name,
                argv,
                loopback_ports,
                timeout_s: s["timeout_s"].as_u64().unwrap_or(600).clamp(1, 3600),
                mem_mb: s["mem_mb"].as_u64().unwrap_or(1024).clamp(16, 4096),
            });
        }
        if stages.is_empty() {
            return Err("plan: no stages".into());
        }
        let require_layers: Vec<String> = match v["require_layers"].as_array() {
            Some(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            None => DEFAULT_REQUIRED.iter().map(|s| s.to_string()).collect(),
        };
        for l in &require_layers {
            if !openmind_sandbox::LAYERS.contains(&l.as_str()) {
                return Err(format!("plan: unknown sandbox layer {l}"));
            }
            if l == "landlock" {
                return Err("plan: landlock is not used by hermes-gate".into());
            }
        }
        for must in ["seccomp", "net", "no_new_privs"] {
            if !require_layers.iter().any(|l| l == must) {
                return Err(format!("plan: required layer {must} may not be dropped"));
            }
        }
        let mut extra_read = vec![];
        for p in v["extra_read"].as_array().into_iter().flatten() {
            let p = PathBuf::from(p.as_str().ok_or("plan: extra_read must be strings")?);
            if !p.is_absolute() || under_live_root(&p) {
                return Err(format!("plan: extra_read {} refused", p.display()));
            }
            extra_read.push(p);
        }
        let mut env = vec![];
        if let Some(o) = v["env"].as_object() {
            for (k, val) in o {
                let val = val.as_str().ok_or("plan: env values must be strings")?;
                if !PLAN_ENV_KEYS.contains(&k.as_str()) {
                    return Err(format!("plan: env key {k} is not allowed"));
                }
                // Every path-like part (PATH is ':'-separated); the gate's
                // own home is not live, /usr is fine (toolchains, coreutils).
                if val
                    .split(':')
                    .map(str::trim)
                    .filter(|x| x.starts_with('/'))
                    .filter(|x| !(x.starts_with("/usr") || ["/bin", "/sbin"].contains(x)))
                    .any(|x| under_live_root(Path::new(x)))
                {
                    return Err(format!("plan: env {k} points into a live root"));
                }
                env.push((k.clone(), val.to_string()));
            }
        }
        Ok(Plan {
            stages,
            require_layers,
            extra_read,
            env,
        })
    }

    pub fn sha256(&self) -> String {
        let v = json!({
            "stages": self.stages.iter().map(|s| json!({"name": s.name, "argv": s.argv, "timeout_s": s.timeout_s, "mem_mb": s.mem_mb, "loopback_ports": s.loopback_ports})).collect::<Vec<_>>(),
            "require_layers": self.require_layers,
            "extra_read": self.extra_read.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "env": self.env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>(),
        });
        crate::sha256_hex(v.to_string().as_bytes())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StageResult {
    pub name: String,
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub wall_ms: u64,
    pub missing_layers: Vec<String>,
    /// Every sandbox layer as reported by openmind-sandbox: `on` or `off: why`.
    pub layers: Vec<(String, String)>,
    pub ok: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

impl StageResult {
    pub fn to_json(&self) -> Value {
        json!({"name": self.name, "ok": self.ok, "exit": self.exit, "timed_out": self.timed_out,
               "wall_ms": self.wall_ms, "missing_layers": self.missing_layers,
               "layers": self.layers.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
               "stdout_tail": self.stdout_tail, "stderr_tail": self.stderr_tail})
    }
}

#[derive(Clone, Debug)]
pub struct DeployReport {
    /// `git write-tree` of the applied result (what a live apply must equal).
    pub result_tree: String,
    pub applied: bool,
    pub apply_error: Option<String>,
    pub stages: Vec<StageResult>,
}

impl DeployReport {
    pub fn ok(&self) -> bool {
        self.applied && !self.stages.is_empty() && self.stages.iter().all(|s| s.ok)
    }
}

fn tail(b: &[u8], n: usize) -> String {
    let s = String::from_utf8_lossy(b);
    let start = s.len().saturating_sub(n);
    let mut i = start;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    s[i..].to_string()
}

/// Dirs hidden in every stage besides the live roots: secrets of the
/// account the gate runs as (the sandbox key dir is passed in `hide`).
fn secret_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/workspace/swarm/secrets")];
    if let Some(h) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        let h = PathBuf::from(h);
        for d in [
            ".ssh",
            ".config",
            ".gnupg",
            ".docker",
            ".kube",
            ".aws",
            ".local/share/keyrings",
        ] {
            v.push(h.join(d));
        }
    }
    v
}

/// The sandbox policy for one stage in worktree `wt`. `hide` = extra dirs
/// to cover (the sandbox key dir, the source repo's `.git`).
pub fn policy(wt: &Path, plan: &Plan, st: &StageSpec) -> SandboxPolicy {
    policy_with(wt, plan, st, &[])
}

pub fn policy_with(wt: &Path, plan: &Plan, st: &StageSpec, hide: &[PathBuf]) -> SandboxPolicy {
    let mut p = SandboxPolicy::new(wt);
    let home = wt.join(".gate-home");
    let tmp = wt.join(".gate-tmp");
    p.env_allowlist = vec!["LANG".into(), "TZ".into()];
    p.env_set = vec![
        ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ("HOME".into(), home.display().to_string()),
        ("TMPDIR".into(), tmp.display().to_string()),
        ("HERMES_GATE_SANDBOX".into(), "1".into()),
    ];
    p.wall_timeout = Duration::from_secs(st.timeout_s);
    p.cpu_secs = st.timeout_s;
    p.mem_bytes = st.mem_mb << 20;
    p.nofile = 1024;
    p.net = false;
    p.require_net_isolation = true;
    p.loopback_tcp_ports = st.loopback_ports.clone();
    // Landlock is never used (owner rule: VPS kernel has none, error 95).
    p.landlock = false;
    p.seccomp = true;
    p.fs_read.push(PathBuf::from("/dev/null"));
    p.fs_read.extend(plan.extra_read.iter().cloned());
    // A hidden dir that holds an extra read path (a toolchain under $HOME)
    // stays visible; its secret subdirs are hidden below.
    p.hide_paths
        .retain(|h| !plan.extra_read.iter().any(|r| r.starts_with(h)));
    for (k, v) in &plan.env {
        p.env_set.retain(|(x, _)| x != k);
        p.env_set.push((k.clone(), v.clone()));
    }
    p.fs_write = vec![wt.to_path_buf(), PathBuf::from("/dev/null")];
    let mut hides: Vec<PathBuf> = ["/opt/galion", "/srv", "/home", "/root", "/var/backups"]
        .iter()
        .map(PathBuf::from)
        .collect();
    // /var/lib: everything but the gate home (the work tree may live there).
    if let Ok(rd) = std::fs::read_dir("/var/lib") {
        for e in rd.flatten() {
            let path = e.path();
            if !GATE_HOMES.iter().any(|g| Path::new(g) == path) {
                hides.push(path);
            }
        }
    }
    hides.extend(secret_dirs());
    hides.extend(hide.iter().cloned());
    for h in hides {
        // never cover the work tree itself or a path the plan reads
        if wt.starts_with(&h) || plan.extra_read.iter().any(|r| r.starts_with(&h)) {
            continue;
        }
        if h.is_dir() && !p.hide_paths.contains(&h) {
            p.hide_paths.push(h);
        }
    }
    p
}

/// Run one stage in the sandbox and judge it.
pub fn run_stage(wt: &Path, plan: &Plan, st: &StageSpec, hide: &[PathBuf]) -> StageResult {
    let pol = policy_with(wt, plan, st, hide);
    let o = openmind_sandbox::run(&st.argv, &pol);
    let exit = match o.status {
        Status::Exited(c) => Some(c),
        _ => None,
    };
    let missing: Vec<String> = plan
        .require_layers
        .iter()
        .filter(|l| !o.enforced(l))
        .cloned()
        .collect();
    let mut err = tail(&o.stderr, 2000);
    if let Some(e) = &o.error {
        err = format!("{err}\n[sandbox] {e}");
    }
    StageResult {
        name: st.name.clone(),
        exit,
        timed_out: o.timed_out,
        wall_ms: o.wall.as_millis() as u64,
        ok: exit == Some(0) && !o.timed_out && missing.is_empty(),
        missing_layers: missing,
        layers: o
            .enforced
            .iter()
            .map(|(k, e)| {
                (
                    k.clone(),
                    match e {
                        openmind_sandbox::Enforcement::Enforced => "on".to_string(),
                        openmind_sandbox::Enforcement::NotEnforced(w) => format!("off: {w}"),
                    },
                )
            })
            .collect(),
        stdout_tail: tail(&o.stdout, 2000),
        stderr_tail: err,
    }
}

/// Apply `p` to a throwaway worktree of `repo` under `scratch`, run the
/// plan, remove the worktree. Stops at the first failed stage.
pub fn sandbox_deploy(
    repo: &Path,
    scratch: &Path,
    p: &Proposal,
    plan: &Plan,
) -> Result<DeployReport, String> {
    sandbox_deploy_hiding(repo, scratch, p, plan, &[])
}

/// As [`sandbox_deploy`], also hiding `hide` (e.g. the sandbox key dir) from
/// every stage. The source repo's `.git` is always hidden.
pub fn sandbox_deploy_hiding(
    repo: &Path,
    scratch: &Path,
    p: &Proposal,
    plan: &Plan,
    hide: &[PathBuf],
) -> Result<DeployReport, String> {
    if under_live_root(scratch) || under_live_root(repo) {
        return Err("sandbox deploy refuses live roots for repo and scratch".into());
    }
    std::fs::create_dir_all(scratch).map_err(|e| format!("scratch: {e}"))?;
    let mut r = [0u8; 6];
    getrandom::getrandom(&mut r).map_err(|e| e.to_string())?;
    let wt = scratch.join(format!("gate-{}-{}", p.id, crate::hex(&r)));
    let wts = wt.display().to_string();
    crate::git::git(
        repo,
        &["worktree", "add", "--detach", &wts, &p.base_commit],
        None,
    )?;
    let res = (|| {
        let mut rep = DeployReport {
            result_tree: String::new(),
            applied: false,
            apply_error: None,
            stages: vec![],
        };
        let patch = p.patch.as_bytes();
        if let Err(e) = crate::git::git(&wt, &["apply", "--check", "--index", "-"], Some(patch))
            .and_then(|_| crate::git::git(&wt, &["apply", "--index", "-"], Some(patch)))
        {
            rep.apply_error = Some(e);
            return Ok(rep);
        }
        rep.applied = true;
        rep.result_tree = crate::git::git(&wt, &["write-tree"], None)?
            .trim()
            .to_string();
        for d in [".gate-home", ".gate-tmp"] {
            std::fs::create_dir_all(wt.join(d)).map_err(|e| e.to_string())?;
        }
        let mut hide = hide.to_vec();
        if let Ok(gd) = crate::git::git(repo, &["rev-parse", "--absolute-git-dir"], None) {
            hide.push(PathBuf::from(gd.trim()));
        }
        // the work tree's `.git` file points into the hidden repo; drop it
        let _ = std::fs::remove_file(wt.join(".git"));
        for st in &plan.stages {
            let s = run_stage(&wt, plan, st, &hide);
            let stop = !s.ok;
            rep.stages.push(s);
            if stop {
                break;
            }
        }
        Ok(rep)
    })();
    let _ = crate::git::git(repo, &["worktree", "remove", "--force", &wts], None);
    let _ = std::fs::remove_dir_all(&wt);
    let _ = crate::git::git(repo, &["worktree", "prune"], None);
    res
}
