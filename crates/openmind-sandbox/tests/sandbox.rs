// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
use openmind_sandbox::*;
use std::path::PathBuf;
use std::time::Duration;

fn wd(name: &str) -> PathBuf {
    // Per target dir (CARGO_TARGET_TMPDIR), not per source tree: several
    // copies of this suite with their own CARGO_TARGET_DIR may run from one
    // checkout at once and must not share (and delete) each other's files.
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("test-out/sandbox")
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
    // A PID namespace hides real pids, so the grandchild is found by a
    // unique argv instead: `sleep 3171<our pid>`. Several agents/copies of
    // this suite run at once on one box; a fixed `sleep 3171` matched their
    // grandchildren too. The pid is unique among live processes, so a match
    // can only be the sleeper this test started (even after it was
    // reparented away from our process tree, which a tree walk would miss).
    let secs = marker_secs(3171);
    let mut p = pol("grp");
    p.wall_timeout = Duration::from_secs(1);
    let o = run(&s(&["/bin/sh", "-c", &format!("sleep {secs} & wait")]), &p);
    assert!(o.timed_out);
    assert!(
        gone_within(&secs, Duration::from_secs(5)),
        "grandchild still alive"
    );
}

/// `<tag><pid of this test process>`: a sleep argument no other process uses.
fn marker_secs(tag: u32) -> String {
    format!("{tag}{}", std::process::id())
}

/// Wait (bounded) until no live `sleep <arg>` exists. SIGKILL is delivered
/// before `run` returns, but the kernel finishes the victim's exit
/// asynchronously, so poll for the end state instead of guessing a delay.
fn gone_within(arg: &str, limit: Duration) -> bool {
    let t = std::time::Instant::now();
    loop {
        if !sleep_alive(arg) {
            return true;
        }
        if t.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Is a non-zombie `sleep <arg>` running? `arg` must be a unique marker.
fn sleep_alive(arg: &str) -> bool {
    let want = format!("sleep\0{arg}\0");
    std::fs::read_dir("/proc").unwrap().flatten().any(|e| {
        let d = e.path();
        let cmd = std::fs::read(d.join("cmdline")).unwrap_or_default();
        let st = std::fs::read_to_string(d.join("stat")).unwrap_or_default();
        // state is the first field after the last ')' (comm may hold ')')
        let state = st.rsplit_once(')').map(|(_, r)| r.trim_start());
        cmd.ends_with(want.as_bytes())
            && !matches!(state.and_then(|r| r.chars().next()), Some('Z' | 'X') | None)
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
    let o = sh("cat /proc/self/status", &pol("nnp"));
    assert_proc_readable(&o, "g_no_new_privs_always");
    assert_eq!(status_field(&o, "NoNewPrivs:"), "1", "{o:?}");
    assert!(o.enforced("no_new_privs"));
}

#[test]
fn h_caps_dropped_when_enforced() {
    let o = sh("cat /proc/self/status", &pol("caps"));
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
    let mut p = pol("manyfds");
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
    let mut p = pol("leakrun");
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
    let o = sh("cat /proc/net/dev", &pol("netns"));
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
