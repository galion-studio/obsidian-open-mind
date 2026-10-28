// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-tools: sandboxed coding tools for hermes-rs.
//!
//! - `fs_read` / `fs_write`: one allowlisted worktree, relative paths only,
//!   no `..`, no symlinks, no `.git`; the actual I/O runs in the sandbox.
//! - `git`: status / diff / commit on one allowlisted branch. Never push.
//! - `cargo`: build / check / test / clippy, `--offline`, `nice -n 19`,
//!   `-j 2` by default.
//!
//! Every process goes through `openmind_sandbox::run` (no network, read-only
//! root, Landlock, rlimits, wall timeout, optional per-run cgroup with
//! memory.max / pids.max / cpu.max, and a total worktree cap: a private
//! size-limited tmpfs, 512 MiB by default, `HERMES_RS_CODING_WORKDIR_MB`).
//! Every decision is written to a JSON Lines audit log before the run. The
//! hermes spend caps are checked before every tool call, on top of the agent
//! loop's check before every model call.
//!
//! Off by default: nothing is registered unless `HERMES_RS_CODING_WORKTREE`
//! is set. See docs/hermes-tools.md.

pub mod audit;
pub mod cgroup_self;
pub mod config;
pub mod exec;
pub mod guard;
pub mod tools;

pub use config::{CodingConfig, NoLandlock};

use hermes_rs::config::SpendCfg;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::Registry;
use std::sync::Arc;

/// Names of the tools `register` adds.
pub const TOOL_NAMES: [&str; 4] = ["fs_read", "fs_write", "git", "cargo"];

/// Startup checks that must hold before any coding tool is offered.
/// Probes the kernel (`openmind_sandbox::features`, forked children that try
/// unshare/landlock): sandbox_vps_only applies to callers.
pub fn preflight(cfg: &mut CodingConfig, spend: &SpendCfg) -> Result<Vec<String>, String> {
    let legacy = std::env::var("HERMES_RS_CODING_ALLOW_NO_LANDLOCK").as_deref() == Ok("1");
    preflight_with(cfg, spend, &openmind_sandbox::features(), legacy)
}

/// The Landlock part of preflight (D-320-1, slot 334), pure: `f` is the
/// kernel feature set. `Ok(Some(note))` = start with that note, `Ok(None)` =
/// Landlock is there, `Err` = refuse to start.
pub fn landlock_gate(
    mode: NoLandlock,
    f: &openmind_sandbox::Features,
    cfg: &CodingConfig,
) -> Result<Option<String>, String> {
    if f.landlock_abi >= 1 {
        return Ok(None);
    }
    match mode {
        NoLandlock::Refuse => Err(
            "the kernel has no Landlock; refusing coding tools (HERMES_RS_CODING_NO_LANDLOCK=allow or =layered, or the legacy HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1, overrides; owner decision D-320-1)"
                .into(),
        ),
        NoLandlock::Allow => Ok(Some(
            "WARNING: no Landlock; relying on the mount namespace only (HERMES_RS_CODING_NO_LANDLOCK=allow, D-320-1 option a)".into(),
        )),
        NoLandlock::Layered => {
            let mut miss: Vec<String> = vec![];
            if !f.userns_full {
                miss.push("kernel: unprivileged user+mount+net+pid namespaces".into());
            }
            if cfg.workdir_quota_mb == 0 {
                miss.push("HERMES_RS_CODING_WORKDIR_MB > 0 (workdir quota)".into());
            }
            for k in [exec::Kind::Fs, exec::Kind::Git, exec::Kind::Cargo] {
                let p = exec::policy(cfg, k);
                if !p.namespaces || !p.ro_root || p.net || !p.require_net_isolation {
                    miss.push(format!("{k:?} policy: namespaces + ro_root + no net"));
                }
                if p.workdir_tmpfs_bytes == 0 {
                    miss.push(format!("{k:?} policy: workdir tmpfs"));
                }
            }
            if miss.is_empty() {
                Ok(Some(format!(
                    "WARNING: no Landlock; layered mode (HERMES_RS_CODING_NO_LANDLOCK=layered, D-320-1 option c): every run must report {} enforced or it fails closed",
                    config::LAYERED_REQUIRED.join("+")
                )))
            } else {
                Err(format!(
                    "the kernel has no Landlock and layered mode (D-320-1 option c) is not satisfied; refusing coding tools: missing {}",
                    miss.join(", ")
                ))
            }
        }
    }
}

/// [`preflight`] with the kernel features and the legacy process-env
/// override passed in, so the staged preset can be checked on any host.
pub fn preflight_with(
    cfg: &mut CodingConfig,
    spend: &SpendCfg,
    f: &openmind_sandbox::Features,
    legacy_allow_env: bool,
) -> Result<Vec<String>, String> {
    let mut notes = vec![];
    if spend.session_usd_cap <= 0.0 || spend.daily_usd_cap <= 0.0 {
        return Err(format!(
            "coding tools need finite spend caps (session ${:.2}, daily ${:.2}); set HERMES_RS_SESSION_USD_CAP and HERMES_RS_DAILY_USD_CAP",
            spend.session_usd_cap, spend.daily_usd_cap
        ));
    }
    if !cfg.no_landlock_explicit && legacy_allow_env {
        cfg.no_landlock = NoLandlock::Allow;
    }
    if let Some(n) = landlock_gate(cfg.no_landlock, f, cfg)? {
        notes.push(n);
    }
    if !f.userns {
        notes.push(
            "no unprivileged user namespaces: network isolation needs Landlock ABI >= 4".into(),
        );
        if f.landlock_abi < 4 {
            return Err("neither a network namespace nor Landlock TCP deny is available".into());
        }
    }
    if cfg.workdir_quota_mb > 0 {
        if !f.userns {
            return Err(format!(
                "the worktree cap (HERMES_RS_CODING_WORKDIR_MB={}) needs user, mount and pid namespaces, which this kernel does not allow unprivileged; set HERMES_RS_CODING_WORKDIR_MB=0 to run without a total worktree cap",
                cfg.workdir_quota_mb
            ));
        }
        let limit = cfg.workdir_quota_mb << 20;
        let inodes = exec::policy(cfg, exec::Kind::Fs).workdir_tmpfs_inode_cap();
        let fp = exec::footprint(&cfg.worktree, limit, inodes);
        if fp.bytes > limit || fp.entries > inodes {
            return Err(format!(
                "worktree {} needs at least {} MiB / {} entries on the tmpfs, over the worktree cap {} MiB / {} entries (every tool call would fail closed); raise HERMES_RS_CODING_WORKDIR_MB / _INODES",
                cfg.worktree.display(),
                fp.bytes.div_ceil(1 << 20),
                fp.entries,
                cfg.workdir_quota_mb,
                inodes
            ));
        }
        notes.push(format!(
            "worktree cap {} MiB / {} entries (private tmpfs per run, copied in and back); worktree now ~{} MiB / {} entries",
            cfg.workdir_quota_mb,
            inodes,
            fp.bytes.div_ceil(1 << 20),
            fp.entries
        ));
    } else {
        notes.push(
            "HERMES_RS_CODING_WORKDIR_MB=0: no total worktree cap (RLIMIT_FSIZE caps single files only)"
                .into(),
        );
    }
    if cfg.cgroup_delegate {
        let gw_mem = std::env::var("HERMES_RS_CODING_GATEWAY_MEM_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(64);
        let parent = cgroup_self::enter_delegated(gw_mem << 20)?;
        notes.push(format!(
            "cgroup: gateway moved to {}/gateway (memory.max {gw_mem}M); per-run cgroups under it",
            parent.display()
        ));
        cfg.cgroup_parent = Some(parent);
    }
    if cfg.cgroup_parent.is_none() {
        notes.push(
            "no HERMES_RS_CODING_CGROUP: per-run memory/cpu/pids caps fall back to rlimits (RLIMIT_AS, RLIMIT_CPU) and nice"
                .into(),
        );
    }
    if cfg.toolchain.is_none() {
        notes.push("no HERMES_RS_CODING_TOOLCHAIN: the cargo tool will refuse every call".into());
    }
    cfg.prepare_dirs()?;
    notes.push(format!(
        "landlock_abi={} no_landlock={:?} userns={} worktree={} branch={} audit={}",
        f.landlock_abi,
        cfg.no_landlock,
        f.userns,
        cfg.worktree.display(),
        cfg.branch,
        cfg.audit_path.display()
    ));
    Ok(notes)
}

/// Add the four coding tools to `reg`. Children of `delegate_task` get them
/// too (they share the session spend cap and the per-session call cap).
pub fn register(
    reg: &mut Registry,
    cfg: CodingConfig,
    spend: Option<Arc<SpendTracker>>,
) -> Arc<tools::Shared> {
    let sh = Arc::new(tools::Shared::new(cfg, spend));
    reg.add(Arc::new(tools::FsRead(sh.clone())));
    reg.add(Arc::new(tools::FsWrite(sh.clone())));
    reg.add(Arc::new(tools::Git(sh.clone())));
    reg.add(Arc::new(tools::Cargo(sh.clone())));
    sh
}
