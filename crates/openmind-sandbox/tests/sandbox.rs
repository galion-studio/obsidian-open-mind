// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
use openmind_sandbox::*;
use std::path::PathBuf;
use std::time::Duration;

fn wd(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/sandbox")
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
    let mut p = pol("grp");
    p.wall_timeout = Duration::from_secs(1);
    let o = run(&s(&["/bin/sh", "-c", "sleep 31 & echo $! ; wait"]), &p);
    assert!(o.timed_out);
    let pid: i32 = o.stdout_str().trim().parse().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // SAFETY: signal 0 probe of a pid we learned from our own child's output.
    let alive = unsafe { libc_kill0(pid) };
    assert!(!alive, "grandchild still alive");
}
unsafe fn libc_kill0(pid: i32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists() && {
        let st = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        !st.contains(") Z ") // zombie awaiting init reaping counts as dead
    }
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
    assert!(matches!(o.layer("net"), Some(Enforcement::NotEnforced(_))));
    match landlock_abi() {
        None => assert!(matches!(
            o.layer("landlock"),
            Some(Enforcement::NotEnforced(_))
        )),
        Some(_) => assert_eq!(o.layer("landlock"), Some(&Enforcement::Enforced)),
    }
}

#[test]
fn g_dev_files_usable_but_dev_dir_not_listable() {
    let o = run(
        &s(&[
            "/bin/sh",
            "-c",
            "echo x >/dev/null && head -c 4 /dev/zero | wc -c",
        ]),
        &pol("dev"),
    );
    assert_eq!(o.status, Status::Exited(0), "{:?}", o.stderr);
    assert_eq!(o.stdout_str().trim(), "4");
    if landlock_abi().is_none() {
        println!("skipped: landlock absent");
        return;
    }
    let o = run(&s(&["/bin/ls", "/dev"]), &pol("devls"));
    assert_ne!(o.status, Status::Exited(0), "{o:?}");
}
