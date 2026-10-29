// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Sandbox deploy: the proposal is applied to a throwaway worktree at its
//! base commit, then every plan stage (build, test, a live-style deploy of
//! the built service on loopback inside the sandbox, smoke probes) runs
//! under `openmind_sandbox::run`: no network, read-only root, Landlock,
//! rlimits, cleared env, live roots hidden. A stage whose required sandbox
//! layers were not enforced fails closed, even if its command exited 0.

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
/// list. `net` = no network (namespace or Landlock TCP deny).
pub const DEFAULT_REQUIRED: &[&str] = &["rlimits", "env", "no_new_privs", "landlock", "net"];

pub fn under_live_root(p: &Path) -> bool {
    LIVE_ROOTS.iter().any(|r| p.starts_with(r))
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
                if LIVE_ROOTS
                    .iter()
                    .filter(|r| **r != "/usr")
                    .any(|r| val.contains(r))
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
    pub ok: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

impl StageResult {
    pub fn to_json(&self) -> Value {
        json!({"name": self.name, "ok": self.ok, "exit": self.exit, "timed_out": self.timed_out,
               "wall_ms": self.wall_ms, "missing_layers": self.missing_layers,
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

/// The sandbox policy for one stage in worktree `wt`.
pub fn policy(wt: &Path, plan: &Plan, st: &StageSpec) -> SandboxPolicy {
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
    p.landlock = true;
    p.fs_read.push(PathBuf::from("/dev/null"));
    p.fs_read.extend(plan.extra_read.iter().cloned());
    // A hidden dir that holds an extra read path (a toolchain under $HOME)
    // stays visible; Landlock still limits reads to the listed paths.
    p.hide_paths
        .retain(|h| !plan.extra_read.iter().any(|r| r.starts_with(h)));
    for (k, v) in &plan.env {
        p.env_set.retain(|(x, _)| x != k);
        p.env_set.push((k.clone(), v.clone()));
    }
    p.fs_write = vec![wt.to_path_buf(), PathBuf::from("/dev/null")];
    for h in ["/opt/galion", "/srv", "/var/lib"] {
        let h = PathBuf::from(h);
        if h.exists() && !p.hide_paths.contains(&h) {
            p.hide_paths.push(h);
        }
    }
    p
}

/// Run one stage in the sandbox and judge it.
pub fn run_stage(wt: &Path, plan: &Plan, st: &StageSpec) -> StageResult {
    let pol = policy(wt, plan, st);
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
        for st in &plan.stages {
            let s = run_stage(&wt, plan, st);
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
    res
}
