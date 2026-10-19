// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! The probe path reads. It does not spawn systemctl and it does not restart
//! a service.
//!
//! A unit probe may ask the injected runner for `is-active` only. That ask is
//! not a process. `SystemRunner` is the only `Command` in the crate, and the
//! probe functions do not call it.

use openmind_survival::{
    check_cloudflared, check_disk, check_freshness, check_http, check_loadavg, check_meminfo,
    check_unit, ProbeResult, Runner,
};
use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

struct Record {
    calls: RefCell<Vec<(String, Vec<String>)>>,
}

impl Runner for Record {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        self.calls.borrow_mut().push((
            program.to_owned(),
            args.iter().copied().map(str::to_owned).collect(),
        ));
        match program {
            "df" => Ok(include_str!("fixtures/df.txt").to_owned()),
            "systemctl" => {
                if args.len() == 2 && args[0] == "is-active" && args[1] == "demo.service" {
                    Ok("active\n".to_owned())
                } else {
                    let joined = args.join(" ");
                    Err(format!("refused {joined}"))
                }
            }
            other => Err(format!("refused {other}")),
        }
    }
}

fn proc_starttime(pid: u32) -> String {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
    let rest = stat.rsplit_once(')').expect("comm").1;
    rest.split_whitespace()
        .nth(19)
        .expect("starttime")
        .to_owned()
}

fn child_cmdlines() -> Vec<String> {
    let me = std::process::id();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(end) = stat.rfind(')') else {
            continue;
        };
        let mut fields = stat[end + 1..].split_whitespace();
        let _state = fields.next();
        let Some(ppid) = fields.next() else {
            continue;
        };
        if ppid.parse::<u32>().ok() != Some(me) {
            continue;
        }
        let Ok(raw) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let cmd = String::from_utf8_lossy(&raw).replace('\0', " ");
        out.push(format!("{name} {cmd}"));
    }
    out.sort();
    out
}

struct Trap {
    dir: PathBuf,
}

impl Drop for Trap {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct PathRestore(Option<OsString>);

impl Drop for PathRestore {
    fn drop(&mut self) {
        match self.0.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn write_trap(dir: &Path, log: &Path) {
    let script = dir.join("systemctl");
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 99\n",
        log.display()
    );
    fs::write(&script, body).expect("trap script");
    let mut perm = fs::metadata(&script).expect("trap meta").permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&script, perm).expect("trap mode");
}

fn assert_probe_source_is_read_only() {
    let src = include_str!("../src/probes.rs");
    assert!(
        !src.to_ascii_lowercase().contains("restart"),
        "probes.rs names a restart"
    );
    for banned in [
        "\"restart\"",
        "\"start\"",
        "\"stop\"",
        "\"kill\"",
        "\"enable\"",
        "\"disable\"",
        "daemon-reload",
    ] {
        assert!(!src.contains(banned), "{banned} in probes.rs");
    }
    assert_eq!(src.matches("systemctl").count(), 2);
    assert!(src.contains("runner.run(\"systemctl\", &[\"is-active\", unit])"));
    assert!(src.contains("runner.run(\"df\", &[\"-P\", path])"));
    assert_eq!(src.matches("runner.run(").count(), 2);
    assert_eq!(src.matches("Command::new").count(), 1);
    assert!(src.contains("Command::new(program)"));
    assert!(!src.contains("SystemRunner.run"));
    assert!(!src.contains("check_unit(&SystemRunner"));
    assert!(!src.contains("check_disk(&SystemRunner"));
}

fn assert_detail_is_not_a_command(result: &ProbeResult) {
    let detail = result.detail.to_ascii_lowercase();
    assert!(!detail.contains("systemctl"), "{}", result.detail);
    assert!(!detail.contains("restart"), "{}", result.detail);
}

#[test]
fn probe_path_does_not_spawn_systemctl_or_restart_a_service() {
    assert_probe_source_is_read_only();

    let id = std::process::id();
    let trap_dir = std::env::temp_dir().join(format!("om-surv3-trap-{id}"));
    let _ = fs::remove_dir_all(&trap_dir);
    fs::create_dir_all(&trap_dir).expect("trap dir");
    let log = trap_dir.join("spawned.log");
    write_trap(&trap_dir, &log);
    let trap = Trap { dir: trap_dir };

    let sentinel = Command::new("/bin/sleep")
        .arg("120")
        .spawn()
        .expect("sentinel");
    let pid = sentinel.id();
    let started = proc_starttime(pid);
    let mut reap = Reap(sentinel);

    let old_path = std::env::var_os("PATH");
    let mut joined = trap.dir.as_os_str().to_owned();
    if let Some(path) = &old_path {
        joined.push(":");
        joined.push(path);
    }
    std::env::set_var("PATH", &joined);
    let _path = PathRestore(old_path);

    let before_kids = child_cmdlines();
    assert!(
        before_kids.iter().any(|line| line.contains("sleep")),
        "sentinel missing: {before_kids:?}"
    );
    assert!(
        before_kids
            .iter()
            .all(|line| !line.to_ascii_lowercase().contains("systemctl")),
        "systemctl already a child: {before_kids:?}"
    );

    let record = Record {
        calls: RefCell::new(Vec::new()),
    };

    let http = check_http("closed-port", 1, "/health");
    let mem = check_meminfo(include_str!("fixtures/meminfo.txt"));
    let load = check_loadavg(include_str!("fixtures/loadavg.txt"), 4);
    let disk = check_disk(&record, "/");
    let unit = check_unit(&record, "demo.service");
    let fresh_path = trap.dir.join("fresh.txt");
    fs::write(&fresh_path, "x").expect("fresh file");
    let fresh = check_freshness(&fresh_path, 60);
    let proc_root = trap.dir.join("proc");
    fs::create_dir_all(proc_root.join("10")).expect("fake proc");
    fs::write(proc_root.join("10/comm"), "sleep\n").expect("comm");
    let tunnel = check_cloudflared(&proc_root);

    assert!(!http.ok, "{http:?}");
    assert!(mem.ok, "{mem:?}");
    assert!(load.ok, "{load:?}");
    assert!(disk.ok, "{disk:?}");
    assert!(unit.ok, "{unit:?}");
    assert_eq!(unit.detail, "active");
    assert!(fresh.ok, "{fresh:?}");
    assert!(!tunnel.ok, "{tunnel:?}");
    assert_eq!(tunnel.detail, "not running");
    for result in [&http, &mem, &load, &disk, &unit, &fresh, &tunnel] {
        assert_detail_is_not_a_command(result);
    }

    let calls = record.calls.borrow().clone();
    assert_eq!(
        calls,
        vec![
            ("df".to_owned(), vec!["-P".to_owned(), "/".to_owned()]),
            (
                "systemctl".to_owned(),
                vec!["is-active".to_owned(), "demo.service".to_owned()],
            ),
        ]
    );
    for (program, args) in &calls {
        for word in args {
            assert!(
                !matches!(
                    word.as_str(),
                    "restart" | "start" | "stop" | "kill" | "enable" | "disable" | "isolate"
                ),
                "{program} {word} would change a service"
            );
        }
    }

    let spawned = fs::read_to_string(&log).unwrap_or_default();
    assert!(
        spawned.is_empty(),
        "probe path spawned systemctl: {spawned}"
    );
    assert!(
        reap.0.try_wait().expect("wait").is_none(),
        "probe path killed pid {pid}"
    );
    assert_eq!(
        proc_starttime(pid),
        started,
        "probe path restarted pid {pid}"
    );

    let after_kids = child_cmdlines();
    assert_eq!(before_kids, after_kids, "probe path spawned a process");
    assert!(
        after_kids
            .iter()
            .all(|line| !line.to_ascii_lowercase().contains("systemctl")),
        "systemctl child: {after_kids:?}"
    );
}
