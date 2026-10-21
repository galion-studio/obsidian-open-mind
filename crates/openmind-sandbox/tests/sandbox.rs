// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
use openmind_sandbox::*;
use std::path::PathBuf;
use std::time::Duration;

fn wd(name: &str) -> PathBuf {
    // GALION_REAL_BASE: run the suite from a throwaway dir on the VPS
    // (sandbox_vps_only) instead of the build tree.
    let p = std::env::var("GALION_REAL_BASE")
        .map(|b| PathBuf::from(b).join("sandbox"))
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/sandbox")
        })
        .join(name);
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}
fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}
fn pol(name: &str) -> SandboxPolicy {
    SandboxPolicy::new(wd(name))
}
/// For tests that observe the command through `/proc/self` (status, fd, net):
/// where no private proc can be mounted (LXC), the default /proc mask has no
/// pid directories, so these keep the host /proc as their observation tool.
fn pol_hostproc(name: &str) -> SandboxPolicy {
    let mut p = pol(name);
    p.proc_fallback = ProcFallback::Host;
    p
}

#[test]
fn a_timeout_kills_and_reaps_own_child() {
    let mut p = pol("a");
    p.wall_timeout = Duration::from_secs(1);
    p.cpu_secs = 60;
    let o = run(&s(&["/bin/sleep", "30"]), &p);
    assert!(o.timed_out);
    assert!(o.wall < Duration::from_secs(3), "{:?}", o.wall);
    assert_eq!(o.status, Status::Signaled(9));
    // Signaled(9) means wait() on the same Child returned: it was reaped.
}

#[test]
fn a2_unrelated_sleep_survives() {
    let mut other = std::process::Command::new("/bin/sleep")
        .arg("20")
        .spawn()
        .unwrap();
    let mut p = pol("a2");
    p.wall_timeout = Duration::from_secs(1);
    let o = run(&s(&["/bin/sleep", "30"]), &p);
    assert!(o.timed_out);
    assert!(
        other.try_wait().unwrap().is_none(),
        "unrelated process was killed"
    );
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn group_kill_leaves_no_grandchild() {
    // A PID namespace hides real pids, so look for a unique argv instead.
    let mut p = pol("grp");
    p.wall_timeout = Duration::from_secs(1);
    let o = run(&s(&["/bin/sh", "-c", "sleep 3171 & wait"]), &p);
    assert!(o.timed_out);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!sleep_alive("3171"), "grandchild still alive");
}

/// Is a non-zombie `sleep <arg>` running anywhere on the host?
fn sleep_alive(arg: &str) -> bool {
    let want = format!("sleep\0{arg}\0");
    std::fs::read_dir("/proc").unwrap().flatten().any(|e| {
        let d = e.path();
        let cmd = std::fs::read(d.join("cmdline")).unwrap_or_default();
        let st = std::fs::read_to_string(d.join("stat")).unwrap_or_default();
        cmd.ends_with(want.as_bytes()) && !st.contains(") Z ")
    })
}

#[test]
fn b_memory_limit() {
    // tail -c N on a pipe must buffer N bytes, so 200 MB cannot fit in 64 MiB AS.
    let script = "head -c 300000000 /dev/zero | tail -c 200000000 >/dev/null";
    let mut p = pol("mem");
    p.wall_timeout = Duration::from_secs(20);
    p.mem_bytes = 64 * 1024 * 1024;
    let o = run(&s(&["/bin/sh", "-c", script]), &p);
    assert_ne!(o.status, Status::Exited(0), "{o:?}");
    // control: a 2 GiB limit lets the same pipeline succeed
    p.mem_bytes = 2 * 1024 * 1024 * 1024;
    let o = run(&s(&["/bin/sh", "-c", script]), &p);
    assert_eq!(o.status, Status::Exited(0), "{:?}", o.stderr);
}

#[test]
fn c_env_cleared() {
    std::env::set_var("SECRET_TEST", "x");
    std::env::set_var("OM_ALLOWED", "yes");
    let mut p = pol("env");
    p.env_allowlist = s(&["OM_ALLOWED", "NOT_SET_ANYWHERE"]);
    let o = run(&s(&["/usr/bin/env"]), &p);
    let out = o.stdout_str();
    assert!(out.contains("OM_ALLOWED=yes"));
    assert!(!out.contains("SECRET_TEST"));
    for line in out.lines() {
        let k = line.split('=').next().unwrap();
        assert!(
            k == "OM_ALLOWED" || k == "PWD" || k == "SHLVL" || k == "_",
            "unexpected {k}"
        );
    }
}

#[test]
fn d_workdir() {
    let p = pol("pwd");
    let o = run(&s(&["/bin/pwd"]), &p);
    assert_eq!(o.stdout_str().trim(), p.workdir.to_str().unwrap());
}

#[test]
fn echo_hi() {
    let o = run(&s(&["/bin/echo", "hi"]), &pol("echo"));
    assert_eq!(o.status, Status::Exited(0));
    assert_eq!(o.stdout_str(), "hi\n");
}

#[test]
fn cpu_limit() {
    let mut p = pol("cpu");
    p.cpu_secs = 1;
    p.wall_timeout = Duration::from_secs(15);
    let o = run(&s(&["/bin/sh", "-c", "while :; do :; done"]), &p);
    assert!(!o.timed_out);
    assert!(
        matches!(o.status, Status::Signaled(24) | Status::Signaled(9)),
        "{:?}",
        o.status
    );
}

#[test]
fn nofile_limit() {
    let mut p = pol("nofile");
    p.nofile = 16;
    let script =
        "i=3; while [ $i -lt 60 ]; do eval \"exec $i</dev/null\" || exit 1; i=$((i+1)); done";
    let o = run(&s(&["/bin/sh", "-c", script]), &p);
    assert_ne!(o.status, Status::Exited(0), "{o:?}");
}

#[test]
fn output_capped() {
    let mut p = pol("cap");
    p.wall_timeout = Duration::from_secs(20);
    let o = run(
        &s(&["/bin/sh", "-c", "head -c 10485760 /dev/zero | tr '\\0' a"]),
        &p,
    );
    assert_eq!(o.status, Status::Exited(0));
    assert_eq!(o.stdout.len(), OUTPUT_CAP);
}

#[test]
fn missing_workdir_is_error_outcome() {
    let p = SandboxPolicy::new("/nonexistent/om-sandbox");
    let o = run(&s(&["/bin/true"]), &p);
    assert_eq!(o.status, Status::Error);
    assert!(o.error.is_some());
    let o = run(&[], &pol("empty"));
    assert_eq!(o.status, Status::Error);
}

#[test]
fn e_write_outside_denied_with_landlock() {
    if landlock_abi().is_none() {
        println!("skipped: landlock absent");
        return;
    }
    let p = pol("ll");
    let target = wd("ll_outside").join("x");
    let o = run(
        &s(&["/bin/sh", "-c", &format!("echo a > {}", target.display())]),
        &p,
    );
    assert_ne!(o.status, Status::Exited(0));
    assert!(!target.exists());
}

#[test]
fn f_enforced_report() {
    let o = run(&s(&["/bin/true"]), &pol("rep"));
    assert_eq!(o.layer("rlimits"), Some(&Enforcement::Enforced));
    assert_eq!(o.layer("env"), Some(&Enforcement::Enforced));
    assert_eq!(o.layer("workdir"), Some(&Enforcement::Enforced));
    for l in LAYERS {
        assert!(o.layer(l).is_some(), "missing layer {l}");
    }
    assert_eq!(o.layer("no_new_privs"), Some(&Enforcement::Enforced));
    // net is Enforced only with a netns; otherwise it must say why.
    match o.layer("net") {
        Some(Enforcement::Enforced) => assert!(o.enforced("userns") || o.enforced("net")),
        Some(Enforcement::NotEnforced(why)) => assert!(!why.is_empty()),
        None => unreachable!(),
    }
    match landlock_abi() {
        None => assert!(matches!(
            o.layer("landlock"),
            Some(Enforcement::NotEnforced(_))
        )),
        Some(_) => assert_eq!(o.layer("landlock"), Some(&Enforcement::Enforced)),
    }
}

fn sh(script: &str, p: &SandboxPolicy) -> Outcome {
    run(&s(&["/bin/sh", "-c", script]), p)
}
fn status_field(o: &Outcome, key: &str) -> String {
    o.stdout_str()
        .lines()
        .find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_string()))
        .unwrap_or_default()
}
/// `/proc` must always be readable inside the sandbox. It used to be
/// unreadable about 1 run in 5 under parallel tests: RLIMIT_NOFILE was set
/// before the uid/gid map was written, so in a parent with many open fds
/// `open("/proc/self/uid_map")` failed with EMFILE, the command ran unmapped
/// as the overflow uid and could not read its own /proc entries. Not a
/// machine quirk, so no skip: fail loudly.
fn assert_proc_readable(o: &Outcome, test: &str) {
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        o.status == Status::Exited(0) && !err.contains("Permission denied"),
        "{test}: /proc unreadable inside the sandbox: status {:?}, error {:?}, stderr {:?}, userns {:?}",
        o.status,
        o.error,
        err.trim(),
        o.layer("userns")
    );
}
fn report(o: &Outcome) {
    for (l, e) in &o.enforced {
        println!("{l}: {e:?}");
    }
}

#[test]
fn g_no_new_privs_always() {
    let o = sh("cat /proc/self/status", &pol_hostproc("nnp"));
    assert_proc_readable(&o, "g_no_new_privs_always");
    assert_eq!(status_field(&o, "NoNewPrivs:"), "1", "{o:?}");
    assert!(o.enforced("no_new_privs"));
}

#[test]
fn h_caps_dropped_when_enforced() {
    let o = sh("cat /proc/self/status", &pol_hostproc("caps"));
    report(&o);
    assert_proc_readable(&o, "h_caps_dropped_when_enforced");
    if features().caps {
        assert!(
            o.enforced("caps"),
            "capget/capset usable, so caps must drop"
        );
    }
    if o.enforced("caps") {
        // Every set, not just the effective one. The test process itself may
        // hold a full inheritable set (this box does), so CapInh matters.
        for f in ["CapEff:", "CapPrm:", "CapInh:", "CapAmb:"] {
            assert_eq!(status_field(&o, f), "0000000000000000", "{f} {o:?}");
        }
        if o.enforced("userns") || euid() == 0 {
            assert_eq!(status_field(&o, "CapBnd:"), "0000000000000000");
        }
    }
}

/// Regression for the EMFILE root cause: a parent holding far more open fds
/// than the sandbox's RLIMIT_NOFILE must not break the uid map or /proc.
#[test]
fn h2_many_parent_fds_do_not_break_setup() {
    let files: Vec<std::fs::File> = (0..300)
        .map(|_| std::fs::File::open("/dev/null").unwrap())
        .collect();
    let mut p = pol_hostproc("manyfds");
    p.nofile = 16;
    for _ in 0..5 {
        let o = sh("cat /proc/self/status; cat /proc/net/dev", &p);
        assert_proc_readable(&o, "h2_many_parent_fds_do_not_break_setup");
        if features().userns {
            assert!(o.enforced("userns"), "{o:?}");
            // mapped 1:1, not the overflow uid 65534
            let uid = status_field(&o, "Uid:");
            assert!(!uid.starts_with("65534"), "unmapped uid: {uid}");
        }
    }
    drop(files);
}

/// A descriptor the host leaked (no O_CLOEXEC) must not reach the command.
#[test]
fn h3_leaked_parent_fd_is_not_inherited() {
    let secret = wd("leak").join("secret");
    std::fs::write(&secret, "leaked-secret").unwrap();
    let c = std::ffi::CString::new(secret.to_str().unwrap()).unwrap();
    // SAFETY: open, then F_DUPFD (which never sets FD_CLOEXEC) to a number
    // >= 100 so it cannot be confused with ls's own fd 3; closed below.
    let fd = unsafe {
        let fd0 = libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
        let fd = libc::fcntl(fd0, libc::F_DUPFD, 100);
        libc::close(fd0);
        fd
    };
    assert!(fd >= 100);
    let mut p = pol_hostproc("leakrun");
    p.hide_paths = vec![secret.parent().unwrap().to_path_buf()];
    let o = sh(&format!("cat /proc/self/fd/{fd}; ls /proc/self/fd"), &p);
    // SAFETY: closing the fd opened above.
    unsafe { libc::close(fd) };
    assert!(o.enforced("fds"), "{o:?}");
    assert!(!o.stdout_str().contains("leaked-secret"), "{o:?}");
    let fds: Vec<String> = o.stdout_str().lines().map(str::to_string).collect();
    assert!(fds.contains(&"0".to_string()), "{o:?}");
    assert!(!fds.contains(&fd.to_string()), "fd {fd} inherited: {fds:?}");
}
fn euid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|v| v.split_whitespace().nth(1).map(|x| x.parse().unwrap_or(1)))
        .unwrap_or(1)
}

#[test]
fn i_pid_namespace_hides_and_protects_others() {
    let mut bystander = std::process::Command::new("/bin/sleep")
        .arg("3172")
        .spawn()
        .unwrap();
    let script = format!("echo $$; kill -0 {} && echo REACHED", bystander.id());
    let o = sh(&script, &pol("pidns"));
    report(&o);
    if o.enforced("pidns") {
        assert_eq!(o.stdout_str().lines().next(), Some("1"), "{o:?}");
        assert!(!o.stdout_str().contains("REACHED"), "{o:?}");
    } else {
        println!("skipped: pidns not enforced here");
    }
    assert!(bystander.try_wait().unwrap().is_none());
    bystander.kill().ok();
    bystander.wait().ok();
}

#[test]
fn j_net_namespace_has_only_lo() {
    let o = sh("cat /proc/net/dev", &pol_hostproc("netns"));
    assert_proc_readable(&o, "j_net_namespace_has_only_lo");
    if o.enforced("net") {
        let ifs: Vec<String> = o
            .stdout_str()
            .lines()
            .skip(2)
            .filter_map(|l| l.split(':').next().map(|x| x.trim().to_string()))
            .collect();
        assert_eq!(ifs, vec!["lo".to_string()], "{o:?}");
    } else {
        println!("skipped: net not enforced: {:?}", o.layer("net"));
    }
}

#[test]
fn k_ro_root_keeps_only_workdir_writable() {
    let p = pol("ro");
    let outside = wd("ro_outside").join("x");
    let _ = std::fs::remove_file(&outside);
    let script = format!(
        "echo a > {} 2>/dev/null && echo OUT; echo b > inside && echo IN",
        outside.display()
    );
    let o = sh(&script, &p);
    report(&o);
    assert!(
        o.stdout_str().contains("IN"),
        "workdir must stay writable: {o:?}"
    );
    if o.enforced("ro_root") {
        assert!(!o.stdout_str().contains("OUT"), "{o:?}");
        assert!(!outside.exists());
    } else {
        println!("skipped: ro_root not enforced: {:?}", o.layer("ro_root"));
    }
}

#[test]
fn l_hide_paths_masks_secrets() {
    let secret_dir = wd("hide_me");
    std::fs::write(secret_dir.join("key"), "top-secret").unwrap();
    let mut p = pol("hide");
    p.hide_paths = vec![secret_dir.clone()];
    let o = sh(&format!("cat {}/key", secret_dir.display()), &p);
    if o.enforced("hide_home") {
        assert!(!o.stdout_str().contains("top-secret"), "{o:?}");
    } else {
        println!("skipped: hide not enforced: {:?}", o.layer("hide_home"));
    }
    // hiding a dir that contains the work dir is refused, not applied
    let mut p = pol("hide2");
    p.hide_paths = vec![p.workdir.parent().unwrap().to_path_buf()];
    let o = sh("echo ok > f && cat f", &p);
    assert_eq!(o.stdout_str().trim(), "ok", "{o:?}");
    assert!(!o.enforced("hide_home"));
}

#[test]
fn m_namespaces_off_is_reported() {
    let mut p = pol("nons");
    p.namespaces = false;
    let o = run(&s(&["/bin/true"]), &p);
    assert_eq!(o.status, Status::Exited(0));
    for l in ["userns", "pidns", "mountns", "ro_root"] {
        assert!(!o.enforced(l), "{l} must not be enforced");
    }
    assert!(matches!(o.layer("userns"), Some(Enforcement::NotEnforced(w)) if w.contains("policy")));
}

#[test]
fn n_require_net_isolation_fails_closed() {
    let mut p = pol("reqnet");
    p.namespaces = false;
    p.landlock = false;
    p.require_net_isolation = true;
    let o = run(&s(&["/bin/echo", "should-not-run"]), &p);
    assert_eq!(o.status, Status::Error, "{o:?}");
    assert!(!o.stdout_str().contains("should-not-run"));
    // with namespaces allowed it runs if the platform can isolate
    p.namespaces = true;
    let o = run(&s(&["/bin/echo", "ran"]), &p);
    if o.enforced("net") {
        assert_eq!(o.stdout_str(), "ran\n");
    } else {
        assert_eq!(o.status, Status::Error);
    }
}

#[test]
fn o_exit_code_passes_through_supervisor() {
    let o = sh("exit 7", &pol("exit7"));
    assert_eq!(o.status, Status::Exited(7));
}

#[test]
fn p_features_probe_is_stable() {
    let a = features();
    let b = features();
    assert_eq!(a, b);
    println!("{a:?}");
}

#[test]
fn q_output_cap_configurable() {
    let mut p = pol("cap2");
    p.output_cap = 100;
    let o = sh("head -c 5000 /dev/zero | tr '\\0' a; echo err >&2", &p);
    assert_eq!(o.stdout.len(), 100);
    assert!(o.stdout_truncated);
    assert!(!o.stderr_truncated);
}

// ---- /proc view (slot 191: the LXC "seebystander" gap) ----
// Markers are matched with a bracket regex ('318[1]') so the command's own
// argv, which holds the pattern text, never matches itself.

/// A bystander outside the sandbox with a marker argv and a canary variable
/// in its environment (a stand-in for a service's secrets).
fn bystander(marker: &str) -> std::process::Child {
    std::process::Command::new("/bin/sleep")
        .arg(marker)
        .env("R191_CANARY", format!("c191-{}", std::process::id()))
        .spawn()
        .unwrap()
}
const PEEK: &str = "n=0; e=0; for d in /proc/[0-9]*; do tr '\\0' ' ' < $d/cmdline 2>/dev/null | grep -q 'sleep 318[1]' && n=$((n+1)); tr '\\0' '\\n' < $d/environ 2>/dev/null | grep -q '^R191_CANARY=c19[1]' && e=$((e+1)); done; echo seen=$n; echo env_seen=$e; echo entries=$(ls /proc | wc -l); [ -e /proc/self ] && echo self=yes || echo self=no";
fn num(o: &Outcome, key: &str) -> i64 {
    status_field(o, key).parse().unwrap_or(-1)
}

#[test]
fn r1_proc_never_shows_host_processes_by_default() {
    let mut b = bystander("3181");
    let o = sh(PEEK, &pol("r1"));
    report(&o);
    println!("proc_view {:?} stdout {}", o.proc_view, o.stdout_str());
    b.kill().ok();
    b.wait().ok();
    assert_eq!(o.status, Status::Exited(0), "{o:?}");
    if !o.enforced("pidns") || !o.enforced("mountns") {
        println!("skipped: no pid + mount namespaces here");
        return;
    }
    assert!(o.enforced("proc"), "{:?}", o.layer("proc"));
    assert_ne!(o.proc_view, ProcView::Host);
    assert_eq!(num(&o, "seen="), 0, "host process visible: {o:?}");
    assert_eq!(num(&o, "env_seen="), 0, "host environ readable: {o:?}");
    if let ProcView::Masked { static_files } = o.proc_view {
        assert_eq!(num(&o, "entries="), static_files as i64, "{o:?}");
        assert_eq!(status_field(&o, "self="), "no");
    }
}

/// Control: with `ProcFallback::Host` on a kernel that refuses a private
/// proc, the same probe must see the bystander, or r1 proves nothing.
#[test]
fn r2_proc_host_fallback_control_sees_the_bystander() {
    let mut b = bystander("3181");
    let o = sh(PEEK, &pol_hostproc("r2"));
    report(&o);
    println!("proc_view {:?} stdout {}", o.proc_view, o.stdout_str());
    b.kill().ok();
    b.wait().ok();
    assert_eq!(o.status, Status::Exited(0), "{o:?}");
    match o.proc_view {
        ProcView::Host => {
            assert!(!o.enforced("proc"));
            assert!(
                num(&o, "seen=") >= 1,
                "control cannot see the bystander: {o:?}"
            );
            println!(
                "MEASURED host-proc leak: same-uid environ canary readable = {} (landlock {:?})",
                num(&o, "env_seen="),
                o.layer("landlock")
            );
        }
        v => {
            assert!(o.enforced("proc"));
            assert_eq!(num(&o, "seen="), 0, "{v:?} {o:?}");
        }
    }
}

#[test]
fn r3_proc_refuse_fails_closed_or_runs_private() {
    let mut p = pol("r3");
    p.proc_fallback = ProcFallback::Refuse;
    let o = sh("echo ran", &p);
    report(&o);
    if o.status == Status::Error {
        let e = o.error.clone().unwrap_or_default();
        assert!(e.contains("Refuse"), "{e}");
        assert!(o.stdout.is_empty());
    } else {
        assert_eq!(o.stdout_str(), "ran\n");
        assert!(o.enforced("proc"));
        assert!(matches!(o.proc_view, ProcView::Private | ProcView::PidOnly));
    }
}

/// The mask is a pivot_root into a copy of the tree: the work dir, read-only
/// root and hidden paths must behave exactly as without it.
#[test]
fn r4_mask_keeps_workdir_ro_root_and_hide() {
    let hidden = wd("r4-hidden");
    std::fs::write(hidden.join("secret"), "s").unwrap();
    let mut p = pol("r4");
    p.hide_paths = vec![hidden.clone()];
    let _ = std::fs::remove_file(p.workdir.join("out.txt"));
    let script = format!(
        "echo hi > out.txt && cat out.txt; touch /usr/r191-ro 2>/dev/null && echo RO_BROKEN; echo hidden=$(ls {} | wc -l); echo pwd=$(pwd); echo meminfo=$(head -c 9 /proc/meminfo 2>/dev/null)",
        hidden.display()
    );
    let o = sh(&script, &p);
    report(&o);
    println!("proc_view {:?} stdout {}", o.proc_view, o.stdout_str());
    assert_eq!(o.status, Status::Exited(0), "{o:?}");
    assert!(o.stdout_str().starts_with("hi\n"), "{o:?}");
    assert_eq!(
        std::fs::read_to_string(p.workdir.join("out.txt")).unwrap(),
        "hi\n"
    );
    assert!(!o.stdout_str().contains("RO_BROKEN"));
    assert!(!std::path::Path::new("/usr/r191-ro").exists());
    if o.enforced("hide_home") {
        assert_eq!(status_field(&o, "hidden="), "0", "{o:?}");
    }
    assert_eq!(status_field(&o, "pwd="), p.workdir.to_str().unwrap());
    if let ProcView::Masked { static_files } = o.proc_view {
        if static_files == 6 {
            assert_eq!(status_field(&o, "meminfo="), "MemTotal:", "{o:?}");
        }
    }
}

/// Mask + work dir tmpfs: the copy back still reaches the host work dir and
/// the size cap still holds.
#[test]
fn r5_mask_with_workdir_quota() {
    let mut p = pol("r5");
    let _ = std::fs::remove_file(p.workdir.join("small"));
    let _ = std::fs::remove_file(p.workdir.join("big"));
    p.workdir_tmpfs_bytes = 8 << 20;
    p.fsize_bytes = 0;
    p.wall_timeout = Duration::from_secs(20);
    let o = sh(
        "echo ok > small; dd if=/dev/zero of=big bs=1M count=16 2>&1 | grep -c 'No space'",
        &p,
    );
    report(&o);
    println!("proc_view {:?} wdq {:?}", o.proc_view, o.workdir_quota);
    if !o.enforced("workdir_quota") {
        println!("skipped: no work dir tmpfs here");
        return;
    }
    assert_eq!(o.stdout_str().trim(), "1", "{o:?}");
    assert_eq!(
        std::fs::read_to_string(p.workdir.join("small")).unwrap(),
        "ok\n"
    );
    let big = std::fs::metadata(p.workdir.join("big"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(big <= 8 << 20, "big = {big}");
    assert!(
        o.workdir_quota.as_ref().is_some_and(|q| q.out_complete),
        "{o:?}"
    );
}
