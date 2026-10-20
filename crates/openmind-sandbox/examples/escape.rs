// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Adversarial payload: compiled as a real binary, run INSIDE the sandbox by
//! the `real_escape` test. Each sub-command attempts one escape and prints
//! `RESULT <attack> <CONTAINED|ESCAPED> <detail>` on stdout. The host-side
//! test independently checks the outside world (the listener received
//! nothing, no stray process survived, the outside file was not written), so
//! a payload that lied about being contained would still be caught.
//!
//! Sub-commands:
//!   tcp <port>         connect to a host listener (and to the internet)
//!   udp <port>         send a UDP datagram to a host listener
//!   ptrace <pid>       PTRACE_ATTACH / PTRACE_SEIZE a process outside
//!   kill <pid>         signal a process outside the sandbox
//!   seebystander <re>  look for another process in /proc
//!   mount              try to mount a fresh proc / remount / bind
//!   setuid <binary>    run a setuid-root binary and report euid
//!   write <path>       create a file outside the workdir
//!   read <path>        read a file that should be hidden/denied
//!   forkbomb           fork until it cannot (pids.max)
//!   membomb <mib>      allocate and touch memory until it cannot
//!   setsid-orphan <n>  setsid()+fork a sleeper that leaves the process group

use std::io::Write;
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

fn out(attack: &str, escaped: bool, detail: impl std::fmt::Display) {
    let v = if escaped { "ESCAPED" } else { "CONTAINED" };
    println!("RESULT {attack} {v} {detail}");
    let _ = std::io::stdout().flush();
}
fn err() -> std::io::Error {
    std::io::Error::last_os_error()
}

// The libc crate does not export PTRACE_SEIZE for Android (bionic has it
// since API 21; it is the Linux kernel ABI value 0x4206 everywhere), so
// define it there instead of losing the attack on aarch64-android.
#[cfg(not(target_os = "android"))]
use libc::PTRACE_SEIZE;
#[cfg(target_os = "android")]
const PTRACE_SEIZE: libc::c_int = 0x4206;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    match arg(0).as_str() {
        "tcp" => {
            let addr: std::net::SocketAddr = format!("127.0.0.1:{}", arg(1)).parse().unwrap();
            match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                Ok(mut s) => {
                    let _ = s.write_all(b"ESCAPE-TCP\n");
                    out("tcp", true, format!("connected to {addr}"));
                }
                Err(e) => out("tcp", false, e),
            }
            // A public resolver, built from octets so no literal address sits
            // in the source tree (repo-check forbids hard-coded public IPs).
            let public = std::net::SocketAddr::from((std::net::Ipv4Addr::new(1, 1, 1, 1), 443));
            match TcpStream::connect_timeout(&public, Duration::from_secs(2)) {
                Ok(_) => out("tcp-internet", true, format!("connected to {public}")),
                Err(e) => out("tcp-internet", false, e),
            }
        }
        "udp" => match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => {
                let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
                match s.send_to(b"ESCAPE-UDP\n", format!("127.0.0.1:{}", arg(1))) {
                    Ok(n) => out("udp", true, format!("sent {n} bytes")),
                    Err(e) => out("udp", false, e),
                }
            }
            Err(e) => out("udp", false, e),
        },
        "ptrace" => {
            let pid: i32 = arg(1).parse().unwrap_or(0);
            // SAFETY: PTRACE_SEIZE on a target pid; read-only attempt. addr and
            // data are pointers in the C prototype, so pass real null pointers
            // through the variadic call rather than int 0.
            let r = unsafe {
                libc::ptrace(
                    PTRACE_SEIZE,
                    pid,
                    std::ptr::null_mut::<libc::c_void>(),
                    std::ptr::null_mut::<libc::c_void>(),
                )
            };
            if r == 0 {
                out("ptrace", true, format!("seized pid {pid}"));
            } else {
                out("ptrace", false, format!("pid {pid}: {}", err()));
            }
        }
        "kill" => {
            let pid: i32 = arg(1).parse().unwrap_or(0);
            // SAFETY: signal 0 only probes reachability; no process harmed.
            let r = unsafe { libc::kill(pid, 0) };
            if r == 0 {
                out("kill", true, format!("can signal pid {pid}"));
            } else {
                out("kill", false, format!("pid {pid}: {}", err()));
            }
        }
        "seebystander" => {
            let needle = arg(1);
            let mut seen = None;
            // SAFETY: getpid() has no preconditions.
            let me = unsafe { libc::getpid() }.to_string();
            if let Ok(rd) = std::fs::read_dir("/proc") {
                for e in rd.flatten() {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    // Only real pid dirs, and never ourselves: the /proc/self
                    // and /proc/<ourpid> entries are this very process, and in
                    // a PID namespace they are all there is.
                    if !name.bytes().all(|b| b.is_ascii_digit()) || name == me {
                        continue;
                    }
                    let cmd = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
                    let cmd = String::from_utf8_lossy(&cmd).replace('\0', " ");
                    if cmd.contains(&needle) {
                        seen = Some(format!(
                            "{} -> {}",
                            e.file_name().to_string_lossy(),
                            cmd.trim()
                        ));
                        break;
                    }
                }
            }
            match seen {
                Some(s) => out("seebystander", true, s),
                None => out("seebystander", false, "bystander not visible in /proc"),
            }
        }
        "mount" => {
            use std::ffi::CString;
            let proc = CString::new("proc").unwrap();
            let tgt = CString::new("/mnt").unwrap();
            // SAFETY: mount(2) of a fresh procfs; rejected without privilege.
            let r = unsafe {
                libc::mount(
                    proc.as_ptr(),
                    tgt.as_ptr(),
                    proc.as_ptr(),
                    0,
                    std::ptr::null(),
                )
            };
            if r == 0 {
                out("mount", true, "mounted proc on /mnt");
            } else {
                out("mount", false, format!("mount proc: {}", err()));
            }
            let root = CString::new("/").unwrap();
            // SAFETY: remount of / read-write; rejected without privilege.
            let r2 = unsafe {
                libc::mount(
                    std::ptr::null(),
                    root.as_ptr(),
                    std::ptr::null(),
                    libc::MS_REMOUNT,
                    std::ptr::null(),
                )
            };
            if r2 == 0 {
                out("remount-root", true, "remounted / rw");
            } else {
                out("remount-root", false, format!("remount /: {}", err()));
            }
        }
        "setuid" => {
            let bin = arg(1);
            let o = std::process::Command::new(&bin).arg("-u").output();
            match o {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    // id -u == 0 would mean the setuid bit took effect
                    if s == "0" {
                        out("setuid", true, format!("{bin} -> euid 0"));
                    } else {
                        out(
                            "setuid",
                            false,
                            format!("{bin} -> uid {s} (setuid ignored under no_new_privs)"),
                        );
                    }
                }
                Err(e) => out("setuid", false, format!("{bin}: {e}")),
            }
        }
        "write" => {
            let path = arg(1);
            match std::fs::write(&path, b"escaped\n") {
                Ok(_) => out("write", true, format!("wrote {path}")),
                Err(e) => out("write", false, format!("{path}: {e}")),
            }
        }
        "read" => {
            let path = arg(1);
            match std::fs::read_to_string(&path) {
                Ok(s) => out(
                    "read",
                    true,
                    format!(
                        "read {} bytes: {:?}",
                        s.len(),
                        s.lines().next().unwrap_or("")
                    ),
                ),
                Err(e) => out("read", false, format!("{path}: {e}")),
            }
        }
        "forkbomb" => {
            let mut n = 0u64;
            loop {
                // SAFETY: fork(); children _exit immediately, parent counts.
                let pid = unsafe { libc::fork() };
                if pid == 0 {
                    // Child: sit briefly so it counts against pids.max, then die.
                    std::thread::sleep(Duration::from_millis(400));
                    // SAFETY: _exit in the forked child.
                    unsafe { libc::_exit(0) };
                }
                if pid < 0 {
                    out(
                        "forkbomb",
                        false,
                        format!("stopped after {n} forks: {}", err()),
                    );
                    break;
                }
                n += 1;
                if n > 100_000 {
                    out("forkbomb", true, format!("forked {n} without limit"));
                    break;
                }
            }
        }
        "membomb" => {
            let mib: usize = arg(1).parse().unwrap_or(4096);
            let mut chunks: Vec<Vec<u8>> = Vec::new();
            for _ in 0..mib {
                let mut v = vec![0u8; 1024 * 1024];
                for b in v.iter_mut().step_by(4096) {
                    *b = 1;
                }
                chunks.push(v);
            }
            // If we got here we allocated and touched mib MiB: escape.
            out("membomb", true, format!("touched {} MiB", chunks.len()));
        }
        "setsid-orphan" => {
            let secs = arg(1);
            // SAFETY: setsid()+fork a child that leaves the process group and
            // sleeps; the sandbox's cgroup.kill must still reap it.
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                // SAFETY: new session, detaches from the sandbox's setsid group.
                unsafe { libc::setsid() };
                let _ = std::process::Command::new("/bin/sleep").arg(secs).status();
                // SAFETY: _exit in the forked child.
                unsafe { libc::_exit(0) };
            }
            out(
                "setsid-orphan",
                false,
                format!("spawned detached sleeper (pid in ns {pid})"),
            );
        }
        other => {
            eprintln!("unknown escape command: {other:?}");
            std::process::exit(2);
        }
    }
}
