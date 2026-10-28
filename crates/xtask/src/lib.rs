// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `cargo xtask test`: the default workspace test recipe (D-320-2).
//!
//! Three crates run real sandboxes or probe the sandbox host in their tests:
//! `openmind-sandbox`, `hermes-tools` and `cgroup-detect`. On a host that
//! lacks the kernel features they need, their suites fail (VPS without
//! Landlock: hermes-tools `sandboxed.rs` 2/8) and every gate had to add
//! `--exclude ...` by hand. On the swarm box they must not run at all
//! (RULES.md `sandbox_vps_only`; the slot-294 slip ran them there).
//!
//! The recipe reads a few `/proc` and `/sys` files (no fork, no unshare, no
//! syscall probes, nothing written), decides per crate, and runs
//! `cargo test --workspace --exclude <crate> ...`.
//!
//! Modes:
//! - `auto` (default): include a sandbox crate only when the host has its
//!   kernel features and is not marked sandbox-VPS-only.
//! - `--sandbox` (explicit opt-in): include all three even where features are
//!   missing (e.g. the VPS with `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`).
//!   Refused on a sandbox-VPS-only host.
//! - `--sandbox=require`: like auto, but fail instead of excluding (CI gate).
//! - `--no-sandbox`: always exclude the three.

use std::fs;
use std::path::{Path, PathBuf};

/// The crates whose tests need sandbox kernel features.
pub const SANDBOX_CRATES: [&str; 3] = ["openmind-sandbox", "hermes-tools", "cgroup-detect"];

/// Marker text in the swarm box rules file (RULES.md `sandbox_vps_only`).
pub const SWARM_RULES: &str = "workspace/swarm/RULES.md";
pub const SWARM_MARKER: &str = "sandbox_vps_only";

/// Kernel/host facts, read from files only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostFacts {
    pub linux: bool,
    /// `landlock` is in `/sys/kernel/security/lsm`. Unreadable = false.
    pub landlock: bool,
    /// Unprivileged user namespaces look allowed (or euid 0).
    pub userns: bool,
    /// A cgroup2 file system is mounted.
    pub cgroup2: bool,
    pub euid: Option<u32>,
    /// The host is marked sandbox-VPS-only (the swarm box).
    pub vps_only: bool,
}

fn read(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join(rel)).ok()
}

fn num(s: Option<String>) -> Option<u64> {
    s?.trim().parse().ok()
}

/// Real uid/effective uid from a `/proc/self/status` text (field 2 of `Uid:`).
pub fn parse_euid(status: &str) -> Option<u32> {
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

/// `landlock` listed in an LSM list such as `capability,landlock,selinux,bpf`.
pub fn lsm_has_landlock(lsm: &str) -> bool {
    lsm.trim().split(',').any(|m| m.trim() == "landlock")
}

/// A cgroup2 mount in a `/proc/self/mounts` text.
pub fn mounts_have_cgroup2(mounts: &str) -> bool {
    mounts
        .lines()
        .any(|l| l.split_whitespace().nth(2) == Some("cgroup2"))
}

/// Probe below `root` (normally `/`). Only reads files.
pub fn probe(root: &Path, env_vps_only: bool) -> HostFacts {
    let linux = cfg!(target_os = "linux") || root != Path::new("/");
    if !linux {
        return HostFacts {
            vps_only: env_vps_only,
            ..HostFacts::default()
        };
    }
    let euid = read(root, "proc/self/status").and_then(|s| parse_euid(&s));
    let landlock = read(root, "sys/kernel/security/lsm")
        .map(|s| lsm_has_landlock(&s))
        .unwrap_or(false);
    let max_ns = num(read(root, "proc/sys/user/max_user_namespaces")).unwrap_or(0);
    // Debian/Ubuntu knob; absent on mainline kernels (= allowed).
    let clone_ok = num(read(root, "proc/sys/kernel/unprivileged_userns_clone")).unwrap_or(1) != 0;
    // Ubuntu 23.10+ AppArmor: unshare works, the uid_map write is refused.
    let aa_ok = num(read(
        root,
        "proc/sys/kernel/apparmor_restrict_unprivileged_userns",
    ))
    .unwrap_or(0)
        == 0;
    let userns = max_ns > 0 && (euid == Some(0) || (clone_ok && aa_ok));
    let cgroup2 = read(root, "proc/self/mounts")
        .map(|m| mounts_have_cgroup2(&m))
        .unwrap_or(false);
    let vps_only = env_vps_only
        || read(root, SWARM_RULES)
            .map(|s| s.contains(SWARM_MARKER))
            .unwrap_or(false);
    HostFacts {
        linux,
        landlock,
        userns,
        cgroup2,
        euid,
        vps_only,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Force,
    Require,
    Off,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "auto" => Some(Mode::Auto),
            "force" | "1" | "on" => Some(Mode::Force),
            "require" => Some(Mode::Require),
            "off" | "0" | "no" => Some(Mode::Off),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Force => "force",
            Mode::Require => "require",
            Mode::Off => "off",
        }
    }
}

/// Missing kernel features for one sandbox crate (empty = supported).
/// `allow_no_landlock` mirrors `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`.
pub fn missing(krate: &str, f: &HostFacts, allow_no_landlock: bool) -> Vec<&'static str> {
    let mut m = Vec::new();
    if !f.linux {
        m.push("linux");
        return m;
    }
    match krate {
        // sandbox.rs adapts to what it finds but needs Landlock or userns.
        "openmind-sandbox" if !f.landlock && !f.userns => m.push("landlock-or-userns"),
        // sandboxed.rs: the coding preset refuses without Landlock (F-320-1).
        "hermes-tools" => {
            if !f.landlock && !allow_no_landlock {
                m.push("landlock");
            }
            if !f.userns {
                m.push("userns");
            }
        }
        "cgroup-detect" if !f.cgroup2 => m.push("cgroup2"),
        _ => {}
    }
    m
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub include: Vec<&'static str>,
    /// (crate, reason)
    pub exclude: Vec<(&'static str, String)>,
}

/// Decide which sandbox crates the recipe runs.
pub fn plan(f: &HostFacts, mode: Mode, allow_no_landlock: bool) -> Result<Plan, String> {
    let mut p = Plan {
        include: Vec::new(),
        exclude: Vec::new(),
    };
    if f.vps_only && matches!(mode, Mode::Force | Mode::Require) {
        return Err(format!(
            "refused: --sandbox={} on a sandbox-VPS-only host (RULES.md {SWARM_MARKER}); \
             run the sandbox suites on the VPS as non-root in /var/tmp/sandbox-test-<ts>",
            mode.name()
        ));
    }
    for k in SANDBOX_CRATES {
        let why = match mode {
            Mode::Off => Some("--no-sandbox".to_string()),
            Mode::Force => None,
            Mode::Auto | Mode::Require => {
                if f.vps_only {
                    Some(format!("host is sandbox-VPS-only ({SWARM_MARKER})"))
                } else {
                    let m = missing(k, f, allow_no_landlock);
                    if m.is_empty() {
                        None
                    } else if mode == Mode::Require {
                        return Err(format!(
                            "--sandbox=require: {k} needs {} on this host",
                            m.join("+")
                        ));
                    } else {
                        Some(format!("host lacks {}", m.join("+")))
                    }
                }
            }
        };
        match why {
            None => p.include.push(k),
            Some(r) => p.exclude.push((k, r)),
        }
    }
    Ok(p)
}

/// `cargo` arguments for the plan: `test --workspace [--exclude k]... <extra>`.
pub fn cargo_args(p: &Plan, extra: &[String]) -> Vec<String> {
    let mut a = vec!["test".to_string(), "--workspace".to_string()];
    for (k, _) in &p.exclude {
        a.push("--exclude".to_string());
        a.push((*k).to_string());
    }
    a.extend(extra.iter().cloned());
    a
}

/// One summary line about the host (no paths, no secrets).
pub fn facts_line(f: &HostFacts) -> String {
    let yn = |b: bool| if b { "yes" } else { "no" };
    format!(
        "xtask: host linux={} landlock={} userns={} cgroup2={} euid={} vps_only={}",
        yn(f.linux),
        yn(f.landlock),
        yn(f.userns),
        yn(f.cgroup2),
        f.euid.map(|u| u.to_string()).unwrap_or_else(|| "?".into()),
        yn(f.vps_only)
    )
}

/// Parsed `cargo xtask test` command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCli {
    pub mode: Mode,
    pub dry_run: bool,
    pub extra: Vec<String>,
}

/// Parse the arguments after `test`. `env_mode` is `GALION_SANDBOX_TESTS`.
pub fn parse_test(args: &[String], env_mode: Option<&str>) -> Result<TestCli, String> {
    let mut mode = match env_mode {
        None | Some("") => Mode::Auto,
        Some(v) => Mode::parse(v).ok_or_else(|| format!("GALION_SANDBOX_TESTS={v}?"))?,
    };
    let mut dry_run = false;
    let mut extra = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--" => {
                extra.extend(it.by_ref().cloned());
                break;
            }
            "--sandbox" => mode = Mode::Force,
            "--no-sandbox" => mode = Mode::Off,
            "--dry-run" => dry_run = true,
            s if s.starts_with("--sandbox=") => {
                let v = &s["--sandbox=".len()..];
                mode = Mode::parse(v).ok_or_else(|| format!("unknown --sandbox={v}"))?;
            }
            other => return Err(format!("unknown argument {other} (cargo args go after --)")),
        }
    }
    Ok(TestCli {
        mode,
        dry_run,
        extra,
    })
}

/// Root for the probe. `GALION_XTASK_HOST_ROOT` points at a captured host
/// tree (tests, simulations); such a run never executes cargo.
pub fn host_root(env: Option<String>) -> (PathBuf, bool) {
    match env {
        Some(r) if !r.is_empty() => (PathBuf::from(r), true),
        _ => (PathBuf::from("/"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(landlock: bool, userns: bool, cgroup2: bool, vps_only: bool) -> HostFacts {
        HostFacts {
            linux: true,
            landlock,
            userns,
            cgroup2,
            euid: Some(1000),
            vps_only,
        }
    }

    fn names(p: &Plan) -> Vec<&str> {
        p.exclude.iter().map(|(k, _)| *k).collect()
    }

    #[test]
    fn parsers() {
        assert!(lsm_has_landlock("capability,landlock,selinux,bpf\n"));
        assert!(!lsm_has_landlock("lockdown,capability,yama,apparmor"));
        assert!(!lsm_has_landlock("landlockx"));
        assert_eq!(parse_euid("Name:\tx\nUid:\t1000\t0\t0\t0\n"), Some(0));
        assert_eq!(parse_euid("Uid:\t1000\t1000\t1000\t1000"), Some(1000));
        assert_eq!(parse_euid("nothing"), None);
        assert!(mounts_have_cgroup2(
            "none /sys/fs/cgroup cgroup2 rw,nosuid 0 0\n"
        ));
        assert!(!mounts_have_cgroup2(
            "cgroup /sys/fs/cgroup/cpu cgroup rw 0 0"
        ));
    }

    #[test]
    fn auto_full_host_includes_all() {
        let p = plan(&host(true, true, true, false), Mode::Auto, false).unwrap();
        assert_eq!(p.include, SANDBOX_CRATES.to_vec());
        assert!(p.exclude.is_empty());
        assert_eq!(cargo_args(&p, &[]), ["test", "--workspace"]);
    }

    #[test]
    fn auto_vps_without_landlock() {
        // Live VPS 2026-10-28 01:15 CET: lsm lockdown,capability,yama,apparmor.
        let f = host(false, true, true, false);
        let p = plan(&f, Mode::Auto, false).unwrap();
        assert_eq!(p.include, ["openmind-sandbox", "cgroup-detect"]);
        assert_eq!(names(&p), ["hermes-tools"]);
        assert_eq!(p.exclude[0].1, "host lacks landlock");
        // With the documented override hermes-tools is in again.
        let p = plan(&f, Mode::Auto, true).unwrap();
        assert!(p.exclude.is_empty());
    }

    #[test]
    fn auto_bare_host_excludes_all() {
        let p = plan(&host(false, false, false, false), Mode::Auto, false).unwrap();
        assert!(p.include.is_empty());
        assert_eq!(
            cargo_args(&p, &["--locked".into()]),
            [
                "test",
                "--workspace",
                "--exclude",
                "openmind-sandbox",
                "--exclude",
                "hermes-tools",
                "--exclude",
                "cgroup-detect",
                "--locked"
            ]
        );
        let reasons: Vec<_> = p.exclude.iter().map(|(_, r)| r.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "host lacks landlock-or-userns",
                "host lacks landlock+userns",
                "host lacks cgroup2"
            ]
        );
    }

    #[test]
    fn vps_only_host_never_runs_them() {
        let f = host(true, true, true, true);
        let p = plan(&f, Mode::Auto, false).unwrap();
        assert!(p.include.is_empty());
        assert_eq!(names(&p), SANDBOX_CRATES.to_vec());
        for m in [Mode::Force, Mode::Require] {
            let e = plan(&f, m, false).unwrap_err();
            assert!(e.contains("refused") && e.contains(SWARM_MARKER), "{e}");
        }
        assert_eq!(plan(&f, Mode::Off, false).unwrap().include.len(), 0);
    }

    #[test]
    fn force_require_off() {
        let bare = host(false, false, false, false);
        assert_eq!(
            plan(&bare, Mode::Force, false).unwrap().include,
            SANDBOX_CRATES.to_vec()
        );
        let e = plan(&bare, Mode::Require, false).unwrap_err();
        assert!(
            e.contains("openmind-sandbox needs landlock-or-userns"),
            "{e}"
        );
        let full = host(true, true, true, false);
        assert_eq!(plan(&full, Mode::Require, false).unwrap().exclude, []);
        assert_eq!(
            names(&plan(&full, Mode::Off, false).unwrap()),
            SANDBOX_CRATES.to_vec()
        );
    }

    #[test]
    fn non_linux() {
        let f = HostFacts::default();
        let p = plan(&f, Mode::Auto, true).unwrap();
        assert!(p.exclude.iter().all(|(_, r)| r == "host lacks linux"));
    }

    #[test]
    fn cli() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let c = parse_test(&s(&[]), None).unwrap();
        assert_eq!((c.mode, c.dry_run, c.extra.len()), (Mode::Auto, false, 0));
        let c = parse_test(
            &s(&["--sandbox", "--dry-run", "--", "--locked", "--", "-q"]),
            None,
        )
        .unwrap();
        assert_eq!(c.mode, Mode::Force);
        assert!(c.dry_run);
        assert_eq!(c.extra, s(&["--locked", "--", "-q"]));
        assert_eq!(
            parse_test(&s(&["--sandbox=require"]), None).unwrap().mode,
            Mode::Require
        );
        assert_eq!(parse_test(&s(&[]), Some("off")).unwrap().mode, Mode::Off);
        // A flag beats the env.
        assert_eq!(
            parse_test(&s(&["--no-sandbox"]), Some("1")).unwrap().mode,
            Mode::Off
        );
        assert!(parse_test(&s(&["--sandbox=maybe"]), None).is_err());
        assert!(parse_test(&s(&["-p", "x"]), None).is_err());
        assert!(parse_test(&s(&[]), Some("bogus")).is_err());
    }

    #[test]
    fn probe_tree() {
        let d = std::env::temp_dir().join(format!("xtask-probe-{}", std::process::id()));
        let w = |rel: &str, body: &str| {
            let p = d.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        };
        w("proc/self/status", "Uid:\t1000\t1000\t1000\t1000\n");
        w(
            "sys/kernel/security/lsm",
            "lockdown,capability,yama,apparmor",
        );
        w("proc/sys/user/max_user_namespaces", "2147483647\n");
        w("proc/sys/kernel/unprivileged_userns_clone", "1\n");
        w(
            "proc/sys/kernel/apparmor_restrict_unprivileged_userns",
            "1\n",
        );
        w("proc/self/mounts", "none /sys/fs/cgroup cgroup2 rw 0 0\n");
        let f = probe(&d, false);
        assert_eq!(f, host(false, false, true, false));
        w(
            "proc/sys/kernel/apparmor_restrict_unprivileged_userns",
            "0\n",
        );
        w(SWARM_RULES, "## Standing rule: sandbox_vps_only\n");
        let f = probe(&d, false);
        assert_eq!(f, host(false, true, true, true));
        fs::remove_dir_all(&d).unwrap();
        // Empty tree: nothing.
        let f = probe(&d, true);
        assert_eq!(
            f,
            HostFacts {
                linux: true,
                vps_only: true,
                ..HostFacts::default()
            }
        );
    }
}
