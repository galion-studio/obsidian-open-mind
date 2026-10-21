// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Worktree cap of the coding tools (D-WDQ2, accepted 2026-10-21): pure
//! policy, config and reporting checks. Nothing here starts the sandbox, but
//! the crate's tests run on the VPS only (sandbox_vps_only), like the rest of
//! hermes-tools.

use hermes_tools::config::{quota_mb, WORKDIR_QUOTA_MB_DEFAULT};
use hermes_tools::exec::{self, Kind};
use hermes_tools::CodingConfig;
use openmind_sandbox::{Outcome, Preset, Status, WorkdirQuota};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MIB: u64 = 1 << 20;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-wdq-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("wt/.git")).unwrap();
    d.canonicalize().unwrap()
}

fn cfg(d: &Path, extra: &[(&str, &str)]) -> Result<CodingConfig, String> {
    let mut m: HashMap<String, String> = HashMap::from([
        (
            "HERMES_RS_CODING_WORKTREE".to_string(),
            d.join("wt").display().to_string(),
        ),
        ("HERMES_RS_CODING_BRANCH".to_string(), "swarm/x".to_string()),
        (
            "HERMES_RS_CODING_STATE".to_string(),
            d.join("state").display().to_string(),
        ),
    ]);
    for (k, v) in extra {
        m.insert(k.to_string(), v.to_string());
    }
    CodingConfig::from_env(&move |k| m.get(k).cloned(), &d.join("home")).map(|c| c.unwrap())
}

#[test]
fn default_cap_is_the_coding_preset_512_mib() {
    assert_eq!(WORKDIR_QUOTA_MB_DEFAULT, 512);
    // The coding (build-tool) preset's own cap: the two must stay equal.
    let b = Preset::build_tool()
        .policy(Path::new("/x"))
        .workdir_tmpfs_bytes;
    assert_eq!(b, WORKDIR_QUOTA_MB_DEFAULT * MIB);
    let d = tmp("default");
    let c = cfg(&d, &[]).unwrap();
    assert_eq!(c.workdir_quota_mb, 512);
    assert_eq!(c.workdir_quota_inodes, 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn every_kind_gets_the_same_cap_and_memory_on_top() {
    let d = tmp("kinds");
    let c = cfg(&d, &[]).unwrap();
    for (kind, own_mem) in [
        (Kind::Fs, 128 * MIB),
        (Kind::Git, 256 * MIB),
        (Kind::Cargo, 2048 * MIB),
    ] {
        let p = exec::policy(&c, kind);
        assert_eq!(p.workdir, c.worktree, "{kind:?}");
        assert_eq!(p.workdir_tmpfs_bytes, 512 * MIB, "{kind:?}");
        // 0 = sandbox default: one inode per 16 KiB.
        assert_eq!(p.workdir_tmpfs_inode_cap(), 32768, "{kind:?}");
        // tmpfs pages are charged to the run cgroup: the cap comes on top of
        // the kind's own budget, so a full worktree is ENOSPC, not OOM.
        assert_eq!(p.cgroup_mem_bytes, own_mem + 512 * MIB, "{kind:?}");
        assert!(p.cgroup_mem_bytes > p.workdir_tmpfs_bytes);
        // per-file caps stay as they were
        assert!(p.fsize_bytes <= 512 * MIB);
    }
    // RLIMIT_AS of cargo is not raised (tmpfs pages are not address space).
    assert_eq!(exec::policy(&c, Kind::Cargo).mem_bytes, 2048 * MIB);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cap_can_be_set_or_turned_off() {
    let d = tmp("env");
    let c = cfg(&d, &[("HERMES_RS_CODING_WORKDIR_MB", "64")]).unwrap();
    let p = exec::policy(&c, Kind::Fs);
    assert_eq!(p.workdir_tmpfs_bytes, 64 * MIB);
    assert_eq!(p.cgroup_mem_bytes, 128 * MIB + 64 * MIB);
    let c = cfg(
        &d,
        &[
            ("HERMES_RS_CODING_WORKDIR_MB", "off"),
            ("HERMES_RS_CODING_WORKDIR_INODES", "5000"),
        ],
    )
    .unwrap();
    let p = exec::policy(&c, Kind::Git);
    assert_eq!(p.workdir_tmpfs_bytes, 0);
    assert_eq!(p.cgroup_mem_bytes, 256 * MIB, "no extra memory when off");
    let c = cfg(&d, &[("HERMES_RS_CODING_WORKDIR_INODES", "5000")]).unwrap();
    assert_eq!(
        exec::policy(&c, Kind::Cargo).workdir_tmpfs_inode_cap(),
        5000
    );
    for bad in ["15", "4097", "-1", "lots", "512M"] {
        let e = cfg(&d, &[("HERMES_RS_CODING_WORKDIR_MB", bad)]).unwrap_err();
        assert!(e.contains("HERMES_RS_CODING_WORKDIR_MB"), "{bad}: {e}");
    }
    assert_eq!(quota_mb(None).unwrap(), 512);
    assert_eq!(quota_mb(Some("0".into())).unwrap(), 0);
    assert_eq!(quota_mb(Some("16".into())).unwrap(), 16);
    assert_eq!(quota_mb(Some("4096".into())).unwrap(), 4096);
    let e = cfg(&d, &[("HERMES_RS_CODING_WORKDIR_INODES", "x")]).unwrap_err();
    assert!(e.contains("WORKDIR_INODES"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

fn outcome(q: Option<WorkdirQuota>, status: Status, timed_out: bool) -> Outcome {
    Outcome {
        status,
        stdout: vec![],
        stderr: b"install: error writing 'big': No space left on device\n".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
        timed_out,
        wall: Duration::from_millis(5),
        error: None,
        enforced: vec![],
        cgroup: None,
        workdir_quota: q,
        proc_view: openmind_sandbox::ProcView::Host,
    }
}

fn quota(copied_out: bool, out_complete: bool, errno: i32) -> WorkdirQuota {
    WorkdirQuota {
        limit_bytes: 512 * MIB,
        limit_inodes: 32768,
        in_bytes: 1000,
        in_entries: 5,
        used_bytes: 8192,
        copied_out,
        out_complete,
        out_bytes: 3 * MIB,
        out_entries: 2,
        errno,
        ..Default::default()
    }
}

#[test]
fn partial_copy_back_is_kept_but_reported_d_wdq3() {
    // Complete copy back: an ordinary good run.
    let o = outcome(Some(quota(true, true, 0)), Status::Exited(0), false);
    assert!(exec::ok(&o));
    assert!(exec::copy_back_problem(&o).is_none());
    let s = exec::summary(&o);
    assert_eq!(s["workdir_quota"]["limit_mib"], 512);
    assert_eq!(s["workdir_quota"]["out_complete"], true);
    // No cap: nothing to report, ok as before.
    let o = outcome(None, Status::Exited(0), false);
    assert!(exec::ok(&o));
    assert!(exec::summary(&o).get("workdir_quota").is_none());
    // Host fs full during copy back: exit 0 is NOT ok, the text says what
    // reached the host and that partial files may be left.
    let o = outcome(Some(quota(true, false, 28)), Status::Exited(0), false);
    assert!(!exec::ok(&o));
    let p = exec::copy_back_problem(&o).unwrap();
    assert!(
        p.contains("INCOMPLETE") && p.contains("host file system is full"),
        "{p}"
    );
    assert!(p.contains("partial files may be left"), "{p}");
    assert!(p.contains(&format!("{} bytes", 3 * MIB)), "{p}");
    let f = exec::failure(&o, 2000);
    assert!(f.starts_with("worktree copy back is INCOMPLETE"), "{f}");
    assert!(f.contains("No space left on device"), "{f}");
    // Budget (sparse file) refusal.
    let o = outcome(Some(quota(true, false, 122)), Status::Exited(0), false);
    assert!(exec::copy_back_problem(&o)
        .unwrap()
        .contains("budget (512 MiB"));
    // Timeout: nothing copied back, host as before.
    let o = outcome(Some(quota(false, false, 0)), Status::Signaled(9), true);
    let p = exec::copy_back_problem(&o).unwrap();
    assert!(
        p.contains("NOT copied back") && p.contains("wall timeout"),
        "{p}"
    );
    // A tool that failed inside the cap (ENOSPC) but whose copy back was
    // complete: plain stderr, no copy-back note.
    let o = outcome(Some(quota(true, true, 0)), Status::Exited(1), false);
    assert!(!exec::ok(&o));
    assert!(exec::failure(&o, 2000).starts_with("install: error writing"));
}

#[test]
fn footprint_counts_pages_and_entries_without_following_links() {
    let d = tmp("fp");
    let wt = d.join("wt");
    std::fs::write(wt.join("a"), vec![1u8; 5000]).unwrap(); // 2 pages
    std::fs::write(wt.join("b"), b"x").unwrap(); // 1 page
    std::fs::write(wt.join("empty"), b"").unwrap(); // 0 pages
    std::fs::create_dir_all(wt.join("sub/deeper")).unwrap();
    std::fs::write(wt.join("sub/deeper/c"), vec![0u8; 4096]).unwrap(); // 1 page
    std::os::unix::fs::symlink("/usr", wt.join("link")).unwrap();
    let fp = exec::footprint(&wt, u64::MAX, u64::MAX);
    // root + .git + a + b + empty + sub + deeper + c + link
    assert_eq!(fp.entries, 9);
    assert_eq!(fp.bytes, 4 * 4096, "symlink to /usr not followed");
    // Early stop once a limit is passed.
    let fp = exec::footprint(&wt, 4096, u64::MAX);
    assert!(fp.bytes > 4096);
    let fp = exec::footprint(&wt, u64::MAX, 2);
    assert_eq!(fp.entries, 3);
    let _ = std::fs::remove_dir_all(&d);
}
