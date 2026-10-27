// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 305 (sandbox-dsb-decision-pack): proves the STAGED hermes-tools
//! preset, i.e. the policy the gateway would build from the exact
//! `Environment=` lines of `deploy/staged/hermes-rs.service.d/coding-tools.conf`.
//!
//! Two layers:
//! * policy tests (no sandbox is started; safe anywhere): /proc Mask,
//!   `/opt/galion` hidden (so the host `link.env`, mode 666 on the VPS, is
//!   out of reach), 512 MiB workdir quota, no network, and unit drop-in
//!   hygiene (no RestrictNamespaces / PrivateUsers / ProtectControlGroups);
//! * `real_staged_*` tests that start openmind-sandbox. They only run with
//!   `GALION_REAL=1` AND `GALION_SANDBOX_ON_VPS=1` (sandbox_vps_only: run on
//!   the VPS as a non-root user in /var/tmp/sandbox-test-<ts>), else they
//!   print a SKIP line and pass vacuously.
//!
//! No secret is ever read or printed: the sandboxed probes only print
//! READABLE/HIDDEN words for the real link.env path, and use a planted
//! canary file for the content check.

use hermes_tools::exec::{self, Kind};
use hermes_tools::CodingConfig;
use openmind_sandbox::{ProcFallback, ProcView, Status};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const MIB: u64 = 1 << 20;
/// Host path of the obsidian link env file (VPS: mode 666 root:root,
/// t287u-C failure #2). Only ever stat'ed or tested with `[ -r ]`.
const LINK_ENV: &str = "/opt/galion/galion-light/publish/obsidian/link.env";
const CANARY: &str = "CANARY-305-NOT-A-SECRET";

/// Repo root; `DSB305_REPO` overrides it when the test binary is copied to
/// the VPS together with the two `deploy/staged` files it reads.
fn repo_root() -> PathBuf {
    std::env::var_os("DSB305_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn read_repo(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// `Environment=K=V` lines of the staged coding-tools drop-in.
fn staged_env() -> Vec<(String, String)> {
    read_repo("deploy/staged/hermes-rs.service.d/coding-tools.conf")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string()))
        .collect()
}

/// Directive keys (`Key=`) set in a unit file, comments ignored.
fn unit_keys(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_string()))
        .collect()
}

fn tmp(name: &str) -> PathBuf {
    let base = std::env::var_os("SANDBOX_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let d = base.join(format!("dsb305-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("wt/.git")).unwrap();
    std::fs::create_dir_all(d.join("toolchain/bin")).unwrap();
    std::fs::write(d.join("toolchain/bin/cargo"), b"#!/bin/sh\n").unwrap();
    d.canonicalize().unwrap()
}

/// The staged config with only the host paths remapped into `d` (worktree,
/// state, toolchain: they do not exist on a box). Everything else is the
/// staged value. `extra` overrides/adds keys; a value of "" removes a key.
fn staged_cfg(d: &Path, extra: &[(&str, &str)]) -> CodingConfig {
    let mut m: HashMap<String, String> = staged_env().into_iter().collect();
    m.insert(
        "HERMES_RS_CODING_WORKTREE".into(),
        d.join("wt").display().to_string(),
    );
    m.insert(
        "HERMES_RS_CODING_STATE".into(),
        d.join("state").display().to_string(),
    );
    m.insert(
        "HERMES_RS_CODING_TOOLCHAIN".into(),
        d.join("toolchain").display().to_string(),
    );
    for (k, v) in extra {
        if v.is_empty() {
            m.remove(*k);
        } else {
            m.insert(k.to_string(), v.to_string());
        }
    }
    CodingConfig::from_env(&move |k| m.get(k).cloned(), &d.join("home"))
        .expect("staged config must be accepted")
        .expect("staged config must turn the tools on")
}

const KINDS: [Kind; 3] = [Kind::Fs, Kind::Git, Kind::Cargo];

#[test]
fn staged_dropin_sets_the_required_keys() {
    let env: HashMap<String, String> = staged_env().into_iter().collect();
    for k in [
        "HERMES_RS_CODING_WORKTREE",
        "HERMES_RS_CODING_BRANCH",
        "HERMES_RS_CODING_STATE",
        "HERMES_RS_CODING_CGROUP",
    ] {
        assert!(env.contains_key(k), "staged drop-in lacks {k}");
    }
    assert_eq!(env["HERMES_RS_CODING_CGROUP"], "delegate");
    // The worktree is a dedicated clone, never the live tree.
    assert!(!env["HERMES_RS_CODING_WORKTREE"].starts_with("/opt/galion"));
    // Slot 318: the drop-in now pins the workdir quota (D-SB pack item 2,
    // flagged as a FINDING by slot 305) to the 512 MiB default.
    assert_eq!(
        env.get("HERMES_RS_CODING_WORKDIR_MB").map(String::as_str),
        Some("512"),
        "staged drop-in must pin HERMES_RS_CODING_WORKDIR_MB=512"
    );
}

#[test]
fn staged_dropin_keeps_namespaces_and_cgroups_usable() {
    let keys = unit_keys(&read_repo(
        "deploy/staged/hermes-rs.service.d/coding-tools.conf",
    ));
    for bad in ["RestrictNamespaces", "PrivateUsers", "ProtectControlGroups"] {
        assert!(
            !keys.iter().any(|k| k == bad),
            "{bad}= would break the sandbox (userns/mountns/cgroup delegation)"
        );
    }
    assert!(keys.iter().any(|k| k == "Delegate"));
    assert!(keys.iter().any(|k| k == "MemorySwapMax"));
}

#[test]
fn staged_unit_user_is_reported_for_d_sb4() {
    let base = unit_keys(&read_repo("deploy/staged/hermes-rs.service"));
    let drop = unit_keys(&read_repo(
        "deploy/staged/hermes-rs.service.d/coding-tools.conf",
    ));
    let has_user = base.iter().chain(drop.iter()).any(|k| k == "User");
    let dynamic = base.iter().chain(drop.iter()).any(|k| k == "DynamicUser");
    // Not an assert on purpose: D-SB4 is an owner decision. The pack records
    // what the staged unit does today.
    eprintln!("D-SB4: staged hermes-rs User= set: {has_user}, DynamicUser= set: {dynamic} (false/false = runs as root)");
}

#[test]
fn staged_policy_masks_proc_for_every_kind() {
    let d = tmp("mask");
    let c = staged_cfg(&d, &[]);
    for k in KINDS {
        let p = exec::policy(&c, k);
        assert_eq!(p.proc_fallback, ProcFallback::Mask, "{k:?}");
        assert!(p.namespaces, "{k:?}: Mask needs the mount/pid namespaces");
        assert!(p.ro_root, "{k:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn staged_policy_hides_opt_galion_and_link_env() {
    let d = tmp("hide");
    let c = staged_cfg(&d, &[]);
    for k in KINDS {
        let p = exec::policy(&c, k);
        for must in ["/root", "/home", "/run/secrets", "/opt/galion"] {
            assert!(
                p.hide_paths.iter().any(|h| h == Path::new(must)),
                "{k:?}: {must} not hidden"
            );
        }
        assert!(
            p.hide_paths
                .iter()
                .any(|h| Path::new(LINK_ENV).starts_with(h)),
            "{k:?}: link.env is not under any hidden path"
        );
        // HERMES_HOME (the state parent) is hidden too.
        assert!(p.hide_paths.iter().any(|h| h == &d.join("home")), "{k:?}");
        // Nothing hermes writes to may be hidden away from the run itself.
        for w in &p.fs_write {
            assert!(!w.starts_with("/opt/galion"), "{k:?}: writes {w:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn staged_policy_has_workdir_quota_and_no_net() {
    let d = tmp("wdq");
    let c = staged_cfg(&d, &[]);
    assert_eq!(c.workdir_quota_mb, 512);
    assert!(c.cgroup_delegate, "staged drop-in uses delegate");
    for k in KINDS {
        let p = exec::policy(&c, k);
        assert_eq!(p.workdir_tmpfs_bytes, 512 * MIB, "{k:?}");
        assert_eq!(p.workdir_tmpfs_inode_cap(), 32768, "{k:?}");
        assert!(p.cgroup_mem_bytes > p.workdir_tmpfs_bytes, "{k:?}");
        assert!(!p.net, "{k:?}");
        assert!(p.require_net_isolation, "{k:?}");
    }
    let cargo = exec::policy(&c, Kind::Cargo);
    assert_eq!(cargo.cgroup_mem_bytes, (2048 + 512) * MIB);
    // Unit MemoryMax=2304M must still cover one cargo run's own budget
    // (the tmpfs part is only charged when the worktree is actually full).
    let mm: u64 = read_repo("deploy/staged/hermes-rs.service.d/coding-tools.conf")
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("MemoryMax=")?
                .strip_suffix('M')?
                .parse()
                .ok()
        })
        .expect("staged MemoryMax=<n>M");
    assert!(mm * MIB >= (c.cargo_mem_mb + 64) * MIB, "MemoryMax={mm}M");
    let _ = std::fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------- real

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1")
        && std::env::var("GALION_SANDBOX_ON_VPS").as_deref() == Ok("1");
    if !on {
        eprintln!(
            "SKIP: real_staged_* need GALION_REAL=1 GALION_SANDBOX_ON_VPS=1 (sandbox_vps_only)"
        );
    }
    on
}

/// Staged config for a real run as a plain user: no delegated cgroup (an
/// ordinary user has none), everything else staged.
fn real_cfg(d: &Path, extra: &[(&str, &str)]) -> CodingConfig {
    let mut e: Vec<(&str, &str)> = vec![
        ("HERMES_RS_CODING_CGROUP", ""),
        ("HERMES_RS_CODING_REQUIRE_CGROUP", "0"),
    ];
    e.extend_from_slice(extra);
    let c = staged_cfg(d, &e);
    c.prepare_dirs().unwrap();
    c
}

fn sh(script: &str) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), script.into()]
}

fn field<'a>(out: &'a str, key: &str) -> Option<&'a str> {
    out.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

#[test]
fn real_staged_link_env_and_proc_are_contained() {
    if !real() {
        return;
    }
    let d = tmp("real-hide");
    // Host-side facts (metadata only, never the content).
    match std::fs::metadata(LINK_ENV) {
        Ok(m) => eprintln!(
            "HOST link.env mode {:o} (exists)",
            m.permissions().mode() & 0o7777
        ),
        Err(e) => eprintln!("HOST link.env: {e}"),
    }
    // Planted canary: a world-readable 0666 "link.env" in a dir we hide via
    // HERMES_RS_CODING_HIDE, same shape as the real one.
    let cdir = d.join("hostside/obsidian");
    std::fs::create_dir_all(&cdir).unwrap();
    let canary = cdir.join("link.env");
    std::fs::write(&canary, CANARY).unwrap();
    std::fs::set_permissions(&canary, std::fs::Permissions::from_mode(0o666)).unwrap();
    let hide = d.join("hostside").display().to_string();
    let c = real_cfg(&d, &[("HERMES_RS_CODING_HIDE", hide.as_str())]);
    let script = format!(
        "L={LINK_ENV}; C={c}; \
         if [ -r \"$L\" ]; then echo LINKENV=READABLE; elif [ -e \"$L\" ]; then echo LINKENV=EXISTS_UNREADABLE; else echo LINKENV=HIDDEN; fi; \
         echo OPTGALION=$(ls -A /opt/galion 2>/dev/null | wc -l); \
         cat \"$C\" 2>/dev/null; echo; \
         if [ -r \"$C\" ]; then echo CANARY=READABLE; else echo CANARY=HIDDEN; fi; \
         echo PIDDIRS=$(ls /proc 2>/dev/null | grep -c '^[0-9]'); \
         if [ -e /proc/self ]; then echo SELF=yes; else echo SELF=no; fi; \
         if [ -r /proc/1/cmdline ]; then echo P1=readable; else echo P1=no; fi; \
         echo PROCFILES=$(ls /proc 2>/dev/null | tr '\\n' ',')",
        c = canary.display()
    );
    for k in KINDS {
        let p = exec::policy(&c, k);
        let o = openmind_sandbox::run(&sh(&script), &p);
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        let layers: Vec<String> = o
            .enforced
            .iter()
            .map(|(n, e)| format!("{n}={e:?}"))
            .collect();
        eprintln!(
            "{k:?}: status={:?} proc_view={:?} err={:?}\n{out}LAYERS {}",
            o.status,
            o.proc_view,
            o.error,
            layers.join(" ")
        );
        assert!(matches!(o.status, Status::Exited(0)), "{k:?}: {o:?}");
        assert_ne!(field(&out, "LINKENV"), Some("READABLE"), "{k:?}");
        assert_eq!(field(&out, "OPTGALION"), Some("0"), "{k:?}");
        assert_eq!(field(&out, "CANARY"), Some("HIDDEN"), "{k:?}");
        assert!(!out.contains(CANARY), "{k:?}: canary content leaked");
        assert_ne!(o.proc_view, ProcView::Host, "{k:?}: host /proc visible");
        if matches!(o.proc_view, ProcView::Masked { .. }) {
            assert_eq!(field(&out, "PIDDIRS"), Some("0"), "{k:?}");
            assert_eq!(field(&out, "SELF"), Some("no"), "{k:?}");
            assert_eq!(field(&out, "P1"), Some("no"), "{k:?}");
        }
    }
    // Control: the same canary without the hide entry IS readable, so the
    // HIDDEN above is the hide layer, not a broken probe.
    let c2 = real_cfg(&d, &[]);
    let mut p = exec::policy(&c2, Kind::Fs);
    p.fs_read.push(cdir.clone());
    let o = openmind_sandbox::run(
        &sh(&format!(
            "if [ -r {} ]; then echo CANARY=READABLE; else echo CANARY=HIDDEN; fi",
            canary.display()
        )),
        &p,
    );
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    eprintln!("CONTROL (no hide): {out}");
    assert_eq!(field(&out, "CANARY"), Some("READABLE"), "control: {o:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn real_staged_workdir_quota_is_enospc_not_host_fill() {
    if !real() {
        return;
    }
    let d = tmp("real-wdq");
    // Smallest allowed cap so the fill is quick; the mechanism is the one the
    // staged 512 MiB default uses.
    let c = real_cfg(&d, &[("HERMES_RS_CODING_WORKDIR_MB", "16")]);
    let p = exec::policy(&c, Kind::Fs);
    assert_eq!(p.workdir_tmpfs_bytes, 16 * MIB);
    let o = openmind_sandbox::run(
        &sh("i=0; while [ $i -lt 200 ] && dd if=/dev/zero of=f$i bs=262144 count=1 2>/dev/null; do i=$((i+1)); done; echo WROTE=$i; df -k . | tail -1"),
        &p,
    );
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    eprintln!(
        "WDQ status={:?} proc_view={:?} quota={:?}\n{out}",
        o.status, o.proc_view, o.workdir_quota
    );
    let wrote: u32 = field(&out, "WROTE").unwrap().parse().unwrap();
    assert!(wrote < 200, "the 16 MiB cap did not stop a 50 MiB fill");
    assert!(wrote >= 32, "cap hit far too early: {wrote}");
    let q = o.workdir_quota.as_ref().expect("workdir quota report");
    assert_eq!(q.limit_bytes, 16 * MIB);
    assert!(q.used_bytes <= 16 * MIB);
    // What came back to the host worktree is bounded by the cap.
    let host: u64 = std::fs::read_dir(&c.worktree)
        .unwrap()
        .filter_map(|e| e.ok()?.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    eprintln!("HOST worktree bytes after copy back: {host}");
    assert!(host <= 16 * MIB, "host got {host} bytes");
    let _ = std::fs::remove_dir_all(&d);
}
