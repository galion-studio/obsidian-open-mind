// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Real-life test of the work dir size cap (`workdir_tmpfs_bytes`, row 151).
//!
//! Every test runs a real `/bin/sh` + coreutils command INSIDE the sandbox and
//! judges the result twice: by what the command printed, and by what the
//! host work dir holds afterwards (the copy back).
//!
//! VPS ONLY (standing rule sandbox_vps_only): ignored by default and gated on
//! `GALION_REAL=1 GALION_REAL_SANDBOX=1` on the VPS (vps-sandbox-gate). Run it as an ordinary non-root temp user with
//! `GALION_REAL_BASE=/var/tmp/sandbox-test-<ts>/w` (work dirs go there),
//! e.g. `GALION_REAL=1 GALION_REAL_SANDBOX=1 GALION_REAL_BASE=... ./real_workdir_quota-<hash> --ignored`.
//! `GALION_REAL_CGROUP_PARENT` (`auto` inside a delegated unit, or a delegated cgroup v2 dir) adds the memory
//! cgroup interplay test.

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

use openmind_sandbox::{run, Outcome, Preset, SandboxPolicy, Status};

const MIB: u64 = 1024 * 1024;

fn gate(name: &str) -> bool {
    // sandbox_vps_only + F-383-10: GALION_REAL=1 alone is not enough.
    if !vps_sandbox_gate::sandbox_real(name) {
        return false;
    }
    // SAFETY: plain id getter.
    assert_ne!(unsafe { libc::geteuid() }, 0, "never run as root");
    true
}

fn wd(name: &str) -> PathBuf {
    let base = std::env::var("GALION_REAL_BASE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/wdq")
        });
    let p = base.join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

/// The shipped read-only-tool preset with this work dir.
fn pol(dir: &std::path::Path) -> SandboxPolicy {
    let mut p = Preset::read_only_tool().policy(dir);
    p.env_allowlist.push("PATH".into());
    p
}

fn sh(p: &SandboxPolicy, script: &str) -> Outcome {
    let o = run(
        &["/bin/sh".to_string(), "-c".to_string(), script.to_string()],
        p,
    );
    eprintln!(
        "--- {script}\nstatus={:?} timed_out={} wall_ms={} error={:?}\nstdout: {}\nstderr: {}\nquota: {:?}\nlayer: {:?}",
        o.status,
        o.timed_out,
        o.wall.as_millis(),
        o.error,
        o.stdout_str(),
        String::from_utf8_lossy(&o.stderr),
        o.workdir_quota,
        o.layer("workdir_quota")
    );
    o
}

fn size(p: PathBuf) -> u64 {
    std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0)
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q1_256_mib_cap_write_320_enospc_at_limit() {
    if !gate("q1") {
        return;
    }
    let d = wd("q1");
    let mut p = pol(&d);
    p.workdir_tmpfs_bytes = 256 * MIB;
    p.fsize_bytes = 512 * MIB;
    p.cpu_secs = 60;
    p.wall_timeout = Duration::from_secs(90);
    let o = sh(
        &p,
        "dd if=/dev/zero of=big bs=1M count=320 2>&1; echo rc=$?; df -k . | tail -1",
    );
    let out = o.stdout_str();
    assert_eq!(o.status, Status::Exited(0));
    assert!(o.enforced("workdir_quota"));
    assert!(out.contains("No space left on device"), "{out}");
    assert!(out.contains("rc=1"), "{out}");
    let q = o.workdir_quota.clone().unwrap();
    assert_eq!(q.limit_bytes, 256 * MIB);
    assert!((250 * MIB..=256 * MIB).contains(&q.used_bytes), "{q:?}");
    let host = size(d.join("big"));
    eprintln!(
        "RESULT q1 asked=320MiB host_bytes={host} used={}",
        q.used_bytes
    );
    assert!(q.copied_out && q.out_complete, "{q:?}");
    assert!((250 * MIB..=256 * MIB).contains(&host), "host {host}");
    assert_eq!(q.out_bytes, host);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q2_shipped_preset_caps_many_files_at_64_mib() {
    if !gate("q2") {
        return;
    }
    let d = wd("q2");
    let mut p = pol(&d);
    p.fsize_bytes = 16 * MIB;
    p.cpu_secs = 60;
    p.wall_timeout = Duration::from_secs(60);
    assert_eq!(p.workdir_tmpfs_bytes, 64 * MIB, "shipped preset default");
    let o = sh(
        &p,
        "i=0; while [ $i -lt 32 ]; do dd if=/dev/zero of=f$i bs=1M count=8 2>/dev/null || { echo stop=$i; break; }; i=$((i+1)); done; echo done=$i",
    );
    let out = o.stdout_str();
    assert!(out.contains("stop="), "{out}");
    let host: u64 = (0..32).map(|i| size(d.join(format!("f{i}")))).sum();
    eprintln!("RESULT q2 asked=256MiB host_bytes={host}");
    assert!((60 * MIB..=64 * MIB).contains(&host), "host {host}");
    assert!(o.workdir_quota.unwrap().out_complete);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q3_inputs_copied_in_changes_copied_back_deletions_not() {
    if !gate("q3") {
        return;
    }
    let d = wd("q3");
    std::fs::write(d.join("input.txt"), b"journal line 1\njournal line 2\n").unwrap();
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("sub/state"), b"v1\n").unwrap();
    std::fs::write(d.join("keep.bin"), vec![3u8; 200_000]).unwrap();
    std::os::unix::fs::symlink("input.txt", d.join("link")).unwrap();
    let keep_mtime = std::fs::metadata(d.join("keep.bin")).unwrap().mtime_nsec();
    let p = pol(&d);
    let o = sh(
        &p,
        "wc -l < link > out.txt && echo v2 > sub/state && mkdir -p new/deep && echo hi > new/deep/x && rm input.txt && ls -A | sort | tr '\\n' ' '",
    );
    assert_eq!(o.status, Status::Exited(0));
    assert_eq!(o.stdout_str().trim(), "keep.bin link new out.txt sub");
    let q = o.workdir_quota.clone().unwrap();
    eprintln!("RESULT q3 {q:?}");
    assert_eq!(q.in_entries, 5); // input.txt sub sub/state keep.bin link
    assert_eq!(std::fs::read(d.join("out.txt")).unwrap(), b"2\n");
    assert_eq!(std::fs::read(d.join("sub/state")).unwrap(), b"v2\n");
    assert_eq!(std::fs::read(d.join("new/deep/x")).unwrap(), b"hi\n");
    assert!(
        d.join("input.txt").exists(),
        "deletions are not copied back"
    );
    assert_eq!(
        std::fs::metadata(d.join("keep.bin")).unwrap().mtime_nsec(),
        keep_mtime,
        "unchanged file not rewritten"
    );
    assert!(q.out_unchanged >= 2, "{q:?}"); // keep.bin, link
    assert!(q.out_complete);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q4_workdir_bigger_than_cap_fails_closed() {
    if !gate("q4") {
        return;
    }
    let d = wd("q4");
    std::fs::write(d.join("big-input"), vec![1u8; 3 * MIB as usize]).unwrap();
    let mut p = pol(&d);
    p.workdir_tmpfs_bytes = 2 * MIB;
    let o = sh(&p, "touch ran-anyway");
    assert_eq!(o.status, Status::Error);
    let e = o.error.clone().unwrap_or_default();
    eprintln!("RESULT q4 error={e}");
    assert!(
        e.contains("work dir tmpfs") && e.contains("No space left"),
        "{e}"
    );
    assert!(!d.join("ran-anyway").exists(), "the command ran");
    assert_eq!(size(d.join("big-input")), 3 * MIB, "host input untouched");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q5_timeout_copies_nothing_back() {
    if !gate("q5") {
        return;
    }
    let d = wd("q5");
    let mut p = pol(&d);
    p.wall_timeout = Duration::from_secs(2);
    let o = sh(&p, "echo partial > half-done; sleep 30");
    assert!(o.timed_out);
    let q = o.workdir_quota.clone().unwrap();
    eprintln!("RESULT q5 {q:?}");
    assert!(!q.copied_out);
    assert!(!d.join("half-done").exists(), "copied back after a timeout");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q6_sparse_file_never_becomes_host_disk() {
    if !gate("q6") {
        return;
    }
    let d = wd("q6");
    let mut p = pol(&d);
    p.fsize_bytes = 1024 * MIB;
    let o = sh(
        &p,
        "dd if=/dev/zero of=sparse bs=1 count=1 seek=943718399 2>&1; ls -ls sparse; df -k . | tail -1",
    );
    assert_eq!(o.status, Status::Exited(0));
    let q = o.workdir_quota.clone().unwrap();
    eprintln!("RESULT q6 {q:?} host_exists={}", d.join("sparse").exists());
    assert!(q.used_bytes < MIB, "sparse file uses ~no tmpfs: {q:?}");
    assert!(
        q.copied_out && !q.out_complete && q.errno == libc::EDQUOT,
        "{q:?}"
    );
    assert!(!d.join("sparse").exists(), "900 MiB file reached the host");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q7_no_host_fd_no_umount_cap_holds() {
    if !gate("q7") {
        return;
    }
    let d = wd("q7");
    let mut p = pol(&d);
    p.fsize_bytes = 128 * MIB;
    // This test looks at the command through /proc/self (fd list, and
    // umount(8) needs /proc/self/mountinfo); on LXC the default /proc mask
    // has neither, so keep the host /proc here (slot 191). The mask with the
    // work dir cap is covered by sandbox.rs r5.
    p.proc_fallback = openmind_sandbox::ProcFallback::Host;
    let wdp = d.display().to_string();
    let o = sh(
        &p,
        &format!(
            "echo fds=$(ls /proc/self/fd | sort -n | tr '\\n' ,); umount {wdp} 2>&1; echo umount_rc=$?; umount -l {wdp} 2>&1; echo lazy_rc=$?; grep -c ' {wdp} ' /proc/self/mountinfo; dd if=/dev/zero of=x bs=1M count=100 2>&1 | grep -E 'No space|copied'"
        ),
    );
    let out = o.stdout_str();
    assert!(out.contains("fds=0,1,2,3,"), "{out}");
    assert!(
        !out.contains("umount_rc=0") && !out.contains("lazy_rc=0"),
        "{out}"
    );
    assert!(out.contains("No space left on device"), "{out}");
    let host = size(d.join("x"));
    eprintln!("RESULT q7 host_bytes={host}");
    assert!(
        host <= 64 * MIB,
        "cap escaped after umount attempts: {host}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q8_inode_cap_and_off_switch() {
    if !gate("q8") {
        return;
    }
    let d = wd("q8");
    let mut p = pol(&d);
    p.workdir_tmpfs_inodes = 1024;
    let o = sh(
        &p,
        "i=0; while [ $i -lt 2000 ]; do true > n$i 2>/dev/null || { echo stop=$i; break; }; i=$((i+1)); done",
    );
    let out = o.stdout_str();
    let stop: u64 = out
        .trim()
        .strip_prefix("stop=")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let host = std::fs::read_dir(&d).unwrap().count() as u64;
    eprintln!("RESULT q8 stop={stop} host_entries={host}");
    assert!((1000..1024).contains(&stop), "{out}");
    assert_eq!(host, stop);
    std::fs::remove_dir_all(&d).unwrap();
    // off switch: bare policy reports the gap honestly
    let d = wd("q8-off");
    let o = sh(&SandboxPolicy::new(&d), "echo ok");
    assert_eq!(o.status, Status::Exited(0));
    assert!(o.workdir_quota.is_none());
    assert!(!o.enforced("workdir_quota"));
    std::fs::remove_dir_all(&d).unwrap();
}

/// `GALION_REAL_CGROUP_PARENT=auto`: inside a delegated systemd unit
/// (`systemd-run -p Delegate=yes`), move this process into `<unit>/sup` and
/// enable cpu, memory and pids for the unit's children, then use the unit
/// cgroup as the per-run parent (the same dance as tools/sandbox-real-problems).
/// Any other value is used as the parent path as it is.
fn cgroup_parent() -> Option<PathBuf> {
    let v = std::env::var("GALION_REAL_CGROUP_PARENT").ok()?;
    if v != "auto" {
        return Some(PathBuf::from(v));
    }
    let s = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = s.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
    let cg = PathBuf::from("/sys/fs/cgroup").join(rel.trim_start_matches('/'));
    if !cg.join("sup").exists() {
        std::fs::create_dir_all(cg.join("sup")).ok()?;
    }
    std::fs::write(cg.join("sup/cgroup.procs"), std::process::id().to_string()).ok()?;
    std::fs::write(cg.join("cgroup.subtree_control"), "+cpu +memory +pids").ok()?;
    eprintln!(
        "CGROUP parent {} subtree_control={}",
        cg.display(),
        std::fs::read_to_string(cg.join("cgroup.subtree_control"))
            .unwrap_or_default()
            .trim()
    );
    Some(cg)
}

#[test]
#[ignore = "VPS only; set GALION_REAL=1 GALION_REAL_SANDBOX=1"]
fn q9_cap_bigger_than_memory_cgroup() {
    if !gate("q9") {
        return;
    }
    let Some(parent) = cgroup_parent() else {
        eprintln!("NOTE q9 skipped: set GALION_REAL_CGROUP_PARENT (auto or a delegated dir)");
        return;
    };
    let d = wd("q9");
    let mut p = pol(&d);
    p.workdir_tmpfs_bytes = 256 * MIB;
    p.fsize_bytes = 512 * MIB;
    p.cpu_secs = 60;
    p.wall_timeout = Duration::from_secs(60);
    p.cgroup_parent = Some(parent);
    p.cgroup_mem_bytes = 128 * MIB;
    let o = sh(
        &p,
        "dd if=/dev/zero of=big bs=1M count=320 2>&1; echo rc=$?",
    );
    let host = size(d.join("big"));
    eprintln!(
        "RESULT q9 status={:?} cgroup={:?} quota={:?} host_bytes={host}",
        o.status, o.cgroup, o.workdir_quota
    );
    assert!(o.error.is_none(), "run did not start: {:?}", o.error);
    let cg = o.cgroup.clone().expect("per-run cgroup used");
    assert!(cg.removed, "per-run cgroup removed");
    // tmpfs pages are charged to the run: memory.max (128 MiB), not the
    // 256 MiB tmpfs size, is the binding limit here.
    assert!(
        cg.oom_kills > 0 || cg.mem_max_hits > 0,
        "memory cap not reached: {cg:?}"
    );
    assert!(host <= 128 * MIB, "host {host}");
    std::fs::remove_dir_all(&d).unwrap();
}
