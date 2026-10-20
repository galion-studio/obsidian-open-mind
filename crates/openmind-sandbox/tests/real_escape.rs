// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Real-life adversarial test. Builds the `escape` example into a real binary,
//! runs it INSIDE a sandbox and tries, for real, to break out: reach the
//! network, ptrace / signal a bystander, see other processes, mount, run a
//! setuid-root binary, read a hidden file, write outside the workdir, fork
//! bomb against `pids.max`, and memory bomb against `memory.max` / RLIMIT_AS.
//!
//! Each attack is judged two ways and both must agree it was contained:
//!   1. the payload's own `RESULT ... CONTAINED/ESCAPED` line, and
//!   2. the host side (the listener got no bytes, the bystander is untouched,
//!      the outside file does not exist, no stray sleeper survived).
//!
//! Ignored by default (needs the real build + a reachable localhost). Run it
//! with `GALION_REAL=1 cargo test -p openmind-sandbox --test real_escape --
//!  --ignored`, or via `scripts/real/sandbox.sh`. The cgroup checks only run
//! when `GALION_REAL_CGROUP_PARENT` points at a delegated cgroup v2 dir the
//! caller may create children in (the script sets one up as root).

use std::io::Read;
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use openmind_sandbox::{run, SandboxPolicy, Status};

fn gate() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP real_escape: set GALION_REAL=1 to run the adversarial suite");
        return false;
    }
    true
}

fn escape_bin() -> PathBuf {
    // tests run from target/.../deps/real_escape-*; the example sits in the
    // sibling examples/ dir.
    let exe = std::env::current_exe().unwrap();
    let debug = exe.parent().unwrap().parent().unwrap(); // deps -> debug
    let p = debug.join("examples").join("escape");
    assert!(
        p.exists(),
        "build it first: cargo build -p openmind-sandbox --example escape ({})",
        p.display()
    );
    p
}

fn workdir(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/escape")
        .join(name);
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

/// Copy the escape binary into the (writable, exec-allowed) workdir so it runs
/// even under ro_root + Landlock without opening the whole target tree.
fn place(name: &str) -> (PathBuf, PathBuf) {
    let wd = workdir(name);
    let dst = wd.join("escape");
    std::fs::copy(escape_bin(), &dst).unwrap();
    let mut perm = std::fs::metadata(&dst).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o755);
    std::fs::set_permissions(&dst, perm).unwrap();
    (wd, dst)
}

fn pol(name: &str) -> (SandboxPolicy, PathBuf) {
    let (wd, _bin) = place(name);
    let mut p = SandboxPolicy::new(&wd);
    p.wall_timeout = Duration::from_secs(20);
    (p, wd)
}

fn runsb(p: &SandboxPolicy, args: &[&str]) -> openmind_sandbox::Outcome {
    let mut v = vec!["./escape".to_string()];
    v.extend(args.iter().map(|s| s.to_string()));
    let o = run(&v, p);
    eprintln!(
        "--- {:?} -> {:?}\nstdout: {}\nstderr: {}",
        args,
        o.status,
        o.stdout_str(),
        String::from_utf8_lossy(&o.stderr)
    );
    o
}

fn says(o: &openmind_sandbox::Outcome, attack: &str) -> Option<bool> {
    o.stdout_str().lines().find_map(|l| {
        let mut it = l.split_whitespace();
        if it.next() == Some("RESULT") && it.next() == Some(attack) {
            match it.next() {
                Some("CONTAINED") => Some(false),
                Some("ESCAPED") => Some(true),
                _ => None,
            }
        } else {
            None
        }
    })
}

#[test]
#[ignore = "real-life adversarial test; set GALION_REAL=1"]
fn real_escape() {
    if !gate() {
        return;
    }
    let mut failures: Vec<String> = Vec::new();
    let mut check = |cond: bool, msg: String| {
        if !cond {
            failures.push(msg);
        }
    };

    // ---- network: empty netns, so even localhost is gone ----
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            if let Some(Ok(mut s)) = listener.incoming().next() {
                s.set_read_timeout(Some(Duration::from_secs(1))).ok();
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf);
                tx.send(buf).ok();
            }
        });
        let (p, _) = pol("net");
        let o = runsb(&p, &["tcp", &port.to_string()]);
        check(p_net_enforced(&o), "net layer not enforced".into());
        check(
            says(&o, "tcp") == Some(false),
            "payload reached localhost TCP".into(),
        );
        check(
            says(&o, "tcp-internet") == Some(false),
            "payload reached the internet".into(),
        );
        std::thread::sleep(Duration::from_millis(300));
        check(
            rx.try_recv().is_err(),
            "the host TCP listener received bytes from inside the sandbox".into(),
        );
    }
    {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let port = sock.local_addr().unwrap().port();
        let (p, _) = pol("udp");
        let o = runsb(&p, &["udp", &port.to_string()]);
        check(
            says(&o, "udp") == Some(false),
            "payload sent UDP out of the netns".into(),
        );
        let mut buf = [0u8; 64];
        check(
            sock.recv_from(&mut buf).is_err(),
            "host UDP socket received a datagram".into(),
        );
    }

    // ---- ptrace / signal / visibility of a bystander outside ----
    {
        // Unique argv (30<our pid>) so another agent's `sleep 30...` on the
        // same box can never be mistaken for this bystander.
        let by_secs = marker_secs(30);
        let mut bystander = std::process::Command::new("/bin/sleep")
            .arg(&by_secs)
            .spawn()
            .unwrap();
        let pid = bystander.id();
        let (p, _) = pol("ptrace");
        let o = runsb(&p, &["ptrace", &pid.to_string()]);
        check(
            says(&o, "ptrace") == Some(false),
            "payload ptraced a process outside".into(),
        );
        let o = runsb(&p, &["kill", &pid.to_string()]);
        check(
            says(&o, "kill") == Some(false),
            "payload could signal a process outside".into(),
        );
        let o = runsb(&p, &["seebystander", &format!("sleep {by_secs} ")]);
        check(
            says(&o, "seebystander") == Some(false),
            "payload saw a process outside in /proc".into(),
        );
        check(
            bystander.try_wait().unwrap().is_none(),
            "bystander died".into(),
        );
        bystander.kill().ok();
        bystander.wait().ok();
    }

    // ---- mount / remount ----
    {
        let (p, _) = pol("mount");
        let o = runsb(&p, &["mount"]);
        check(
            says(&o, "mount") == Some(false),
            "payload mounted a filesystem".into(),
        );
        check(
            says(&o, "remount-root") == Some(false),
            "payload remounted /".into(),
        );
    }

    // ---- setuid-root binary must not elevate under no_new_privs ----
    // Only meaningful when the sandboxed process is NOT already root (the
    // crate maps uid 1:1, so a root caller stays root and a setuid bit would
    // add nothing to prove). The harness, running as root, creates a
    // setuid-root helper and then runs this suite as an unprivileged user,
    // passing the helper path in GALION_SETUID_HELPER.
    // SAFETY: geteuid() has no preconditions.
    let host_root = unsafe { libc::geteuid() } == 0;
    match (host_root, std::env::var("GALION_SETUID_HELPER")) {
        (false, Ok(helper)) if Path::new(&helper).exists() => {
            use std::os::unix::fs::MetadataExt;
            use std::os::unix::fs::PermissionsExt;
            // Run the ORIGINAL root-owned setuid binary (copying it would make
            // it box-owned and defeat the test); grant read+exec on its dir.
            let meta = std::fs::metadata(&helper).unwrap();
            check(
                meta.uid() == 0 && meta.permissions().mode() & 0o4000 != 0,
                format!("GALION_SETUID_HELPER is not a setuid-root file (uid {}, mode {:o})", meta.uid(), meta.permissions().mode()),
            );
            let dir = Path::new(&helper).parent().unwrap().to_path_buf();
            let (_, _) = place("setuid");
            let mut p = SandboxPolicy::new(workdir("setuid"));
            p.wall_timeout = Duration::from_secs(10);
            p.fs_read.push(dir);
            let o = runsb(&p, &["setuid", helper.as_str()]);
            check(
                says(&o, "setuid") == Some(false),
                "a setuid-root binary elevated inside the sandbox (no_new_privs did not hold)".into(),
            );
        }
        (true, _) => eprintln!(
            "NOTE setuid sub-check skipped: running as root, so the sandboxed process is root too; the no_new_privs + empty-caps proof is in h_caps_dropped_when_enforced"
        ),
        _ => eprintln!(
            "NOTE setuid sub-check skipped: set GALION_SETUID_HELPER to a setuid-root binary and run this suite as a non-root user"
        ),
    }

    // ---- filesystem: read a hidden file, write outside the workdir ----
    {
        let secret_dir = workdir("secretdir");
        std::fs::write(secret_dir.join("key"), "top-secret-value").unwrap();
        let mut p = SandboxPolicy::new(workdir("fs"));
        p.wall_timeout = Duration::from_secs(10);
        let (_wd, _bin) = place("fs");
        p.hide_paths = vec![secret_dir.clone()];
        let o = runsb(&p, &["read", secret_dir.join("key").to_str().unwrap()]);
        check(
            says(&o, "read") == Some(false),
            "payload read a hidden file".into(),
        );
        check(
            !o.stdout_str().contains("top-secret-value"),
            "hidden secret leaked".into(),
        );

        let outside = workdir("fs").parent().unwrap().join("escaped-marker");
        let _ = std::fs::remove_file(&outside);
        let o = runsb(&p, &["write", outside.to_str().unwrap()]);
        check(
            says(&o, "write") == Some(false),
            "payload wrote outside the workdir".into(),
        );
        check(
            !outside.exists(),
            "a file was created outside the workdir".into(),
        );
    }

    // ---- fork bomb against pids.max (needs a delegated cgroup) ----
    // ---- memory bomb against memory.max / RLIMIT_AS ----
    if let Some(parent) = cgroup_parent() {
        let (mut p, _) = pol("forkbomb");
        p.cgroup_parent = Some(parent.clone());
        p.cgroup_pids_max = 24;
        p.wall_timeout = Duration::from_secs(20);
        let o = runsb(&p, &["forkbomb"]);
        check(
            o.cgroup.is_some(),
            "no cgroup stats for the fork bomb run".into(),
        );
        let forkbomb_capped = says(&o, "forkbomb") == Some(false)
            || o.cgroup
                .as_ref()
                .map(|c| c.pids_max_hits > 0)
                .unwrap_or(false);
        check(
            forkbomb_capped,
            format!("fork bomb was not capped by pids.max: {:?}", o.cgroup),
        );
        check(
            o.cgroup.as_ref().map(|c| c.removed).unwrap_or(false),
            "per-run cgroup was not empty/removed after the fork bomb".into(),
        );

        let (mut p, _) = pol("membomb");
        p.cgroup_parent = Some(parent.clone());
        p.cgroup_mem_bytes = 128 * 1024 * 1024;
        p.mem_bytes = 2 * 1024 * 1024 * 1024; // let the cgroup, not RLIMIT_AS, catch it
        p.wall_timeout = Duration::from_secs(25);
        let o = runsb(&p, &["membomb", "1024"]);
        let killed = o.status != Status::Exited(0) && says(&o, "membomb") != Some(true);
        let oom = o
            .cgroup
            .as_ref()
            .map(|c| c.oom_kills > 0 || c.mem_max_hits > 0)
            .unwrap_or(false);
        check(
            killed || oom,
            format!(
                "memory bomb not contained: status {:?}, cgroup {:?}",
                o.status, o.cgroup
            ),
        );

        // a process that leaves the group with setsid() still dies via cgroup.kill
        let (mut p, _) = pol("orphan");
        p.cgroup_parent = Some(parent);
        p.wall_timeout = Duration::from_secs(3);
        let orphan = marker_secs(3133);
        let _ = runsb(&p, &["setsid-orphan", &orphan]);
        check(
            gone_within(&orphan, Duration::from_secs(5)),
            "a setsid orphan survived the sandbox teardown".into(),
        );
    } else {
        // Without a delegated cgroup, prove at least RLIMIT_AS contains a
        // single-process memory bomb.
        let (mut p, _) = pol("membomb_rlimit");
        p.mem_bytes = 128 * 1024 * 1024;
        p.wall_timeout = Duration::from_secs(25);
        let o = runsb(&p, &["membomb", "1024"]);
        check(
            o.status != Status::Exited(0) && says(&o, "membomb") != Some(true),
            format!(
                "single-process memory bomb not contained by RLIMIT_AS: {:?}",
                o.status
            ),
        );
        eprintln!("NOTE cgroup checks skipped: set GALION_REAL_CGROUP_PARENT to a delegated cgroup v2 dir");
    }

    assert!(
        failures.is_empty(),
        "containment failures:\n  {}",
        failures.join("\n  ")
    );
}

fn p_net_enforced(o: &openmind_sandbox::Outcome) -> bool {
    o.enforced("net")
}

fn cgroup_parent() -> Option<PathBuf> {
    let p = std::env::var("GALION_REAL_CGROUP_PARENT").ok()?;
    let path = PathBuf::from(p);
    path.join("cgroup.subtree_control").exists().then_some(path)
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
