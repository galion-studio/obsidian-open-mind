// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! CLI tests on fixture trees shaped like the VPS (wave-4 slot 330).

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_galion-pyarchive");

struct Fx(PathBuf);

impl Fx {
    fn new(tag: &str) -> Fx {
        let d = std::env::temp_dir().join(format!(
            "pyarchive-cli-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        let f = Fx(d);
        f.w(
            "/opt/galion/bin/desk-hermes.py",
            "#!/usr/bin/env python3\nprint('desk')\n",
        );
        f.w("/opt/galion/bin/galion-desk", "ELF-old");
        f.w("/opt/galion/bin/pw.py", "abc");
        f.w("/opt/galion/bin/__pycache__/pw.cpython-312.pyc", "pyc");
        f.w("/opt/galion/bin/replicate-picture.py", "rp");
        f.w("/etc/hostname", "fixture-host\n");
        f.w(
            "/etc/systemd/system/galion-desk.service",
            "[Unit]\nDescription=Galion desk\n[Service]\nExecStart=/opt/galion/bin/galion-desk\nRestart=on-failure\n",
        );
        f.w(
            "/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf",
            "# (old /opt/galion/bin/galion-desk + desk-hermes.py left untouched).\n[Service]\nExecStart=\nExecStart=/opt/galion/rust/bin/galion-desk\nEnvironment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes\n",
        );
        f.w(
            "/lib/systemd/system/guardian-tick.timer",
            "[Timer]\nOnCalendar=*:*:00\nUnit=guardian-tick.service\n",
        );
        f.w(
            "/etc/crontab",
            "# m h dom mon dow user command\n17 * * * * root cd / && run-parts --report /etc/cron.hourly\n",
        );
        f.w(
            "/etc/cron.d/sysstat",
            "5-55/10 * * * * root command -v debian-sa1 > /dev/null && debian-sa1 1 1\n",
        );
        fs::create_dir_all(f.p("/proc")).unwrap();
        f
    }
    fn p(&self, l: &str) -> PathBuf {
        self.0.join(l.trim_start_matches('/'))
    }
    fn w(&self, l: &str, body: &str) {
        let p = self.p(l);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    fn run(&self, args: &[&str]) -> (i32, String) {
        let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        a.push("--root".into());
        a.push(self.0.display().to_string());
        let o = Command::new(BIN).args(&a).output().unwrap();
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr),
        )
    }
    fn json(&self, args: &[&str]) -> (i32, serde_json::Value) {
        let mut a = args.to_vec();
        a.push("--json");
        let (c, out) = self.run(&a);
        (
            c,
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}")),
        )
    }
    fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut v = Vec::new();
        walk(&self.0, &self.0, &mut v);
        v.sort();
        v
    }
}

fn walk(base: &Path, d: &Path, v: &mut Vec<(String, Vec<u8>)>) {
    for e in fs::read_dir(d).unwrap() {
        let e = e.unwrap();
        let ft = e.file_type().unwrap();
        let p = e.path();
        if ft.is_dir() {
            walk(base, &p, v);
        } else if ft.is_file() {
            v.push((
                p.strip_prefix(base).unwrap().display().to_string(),
                fs::read(&p).unwrap(),
            ));
        } else {
            v.push((p.strip_prefix(base).unwrap().display().to_string(), vec![]));
        }
    }
}

#[test]
fn vps_shape_passes_with_overridden_rollback_warning() {
    let f = Fx::new("pass");
    let before = f.snapshot();
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 0, "{j}");
    assert_eq!(j["verdict"], "PASS");
    assert_eq!(j["host"], "fixture-host");
    assert_eq!(j["files"].as_array().unwrap().len(), 5);
    assert_eq!(j["counts"]["active"], 0);
    assert_eq!(j["counts"]["overridden"], 1);
    assert!(j["counts"]["comment"].as_u64().unwrap() >= 2);
    let pw = j["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["path"] == "/opt/galion/bin/pw.py")
        .unwrap();
    assert_eq!(
        pw["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(!j["generated"].as_str().unwrap().ends_with('Z'));
    let (c, t) = f.run(&["plan"]);
    assert_eq!(c, 0);
    assert!(t.contains("WARNING rollback dependency"), "{t}");
    assert!(t.contains("NOT EXECUTED"));
    assert!(t.contains("verdict: PASS"));
    assert_eq!(before, f.snapshot(), "plan changed the scanned tree");
}

#[test]
fn live_unit_reference_fails() {
    let f = Fx::new("unit");
    f.w(
        "/etc/systemd/system/pw.service",
        "[Service]\nExecStart=/opt/galion/venv/bin/python /opt/galion/bin/pw.py\n",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 1);
    assert_eq!(j["verdict"], "FAIL");
    assert_eq!(j["counts"]["active"], 1);
}

#[test]
fn drop_in_without_reset_keeps_base_exec_active() {
    let f = Fx::new("noreset");
    f.w(
        "/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf",
        "[Service]\nEnvironment=X=1\n",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 1, "{j}");
    assert_eq!(j["counts"]["active"], 1);
}

#[test]
fn timer_and_continuation_line_reference_fails() {
    let f = Fx::new("timer");
    f.w(
        "/run/systemd/system/rp.service",
        "[Service]\nExecStart=/usr/bin/env \\\n  /opt/galion/bin/replicate-picture.py --once\n",
    );
    f.w(
        "/run/systemd/system/rp.timer",
        "[Timer]\nOnCalendar=hourly\n",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 1);
    assert_eq!(j["counts"]["active"], 1);
    assert_eq!(j["refs"][0]["line"], 2);
}

#[test]
fn masked_and_shadowed_do_not_count() {
    let f = Fx::new("mask");
    f.w(
        "/lib/systemd/system/hermes.service",
        "[Service]\nExecStart=/usr/bin/python3 /opt/galion/bin/desk-hermes.py\n",
    );
    fs::create_dir_all(f.p("/etc/systemd/system")).unwrap();
    symlink("/dev/null", f.p("/etc/systemd/system/hermes.service")).unwrap();
    f.w(
        "/lib/systemd/system/galion-desk.service",
        "[Service]\nExecStart=/opt/galion/bin/galion-desk\n",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 0, "{j}");
    assert_eq!(j["counts"]["masked"], 1);
    assert_eq!(j["counts"]["shadowed"], 1);
}

#[test]
fn cron_basename_reference_fails_comment_does_not() {
    let f = Fx::new("cron");
    f.w(
        "/var/spool/cron/crontabs/root",
        "# 0 * * * * python3 /opt/galion/bin/pw.py\n*/5 * * * * cd /opt/galion/bin && python3 pw.py\n",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 1);
    assert_eq!(j["counts"]["active"], 1);
    let a = &j["refs"][0];
    assert_eq!(a["kind"], "cron");
    assert_eq!(a["line"], 2);
}

#[test]
fn running_process_fails_unless_no_procs() {
    let f = Fx::new("proc");
    f.w(
        "/proc/4242/cmdline",
        "python3\0/opt/galion/bin/desk-hermes.py\0",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 1);
    assert_eq!(j["refs"][0]["kind"], "process");
    let (c, _) = f.json(&["plan", "--no-procs"]);
    assert_eq!(c, 0);
}

#[test]
fn credential_lines_are_redacted() {
    let f = Fx::new("redact");
    f.w(
        "/etc/cron.d/x",
        "* * * * * root API_TOKEN=sekrit123 /opt/galion/bin/pw.py\n",
    );
    let (c, out) = f.run(&["plan"]);
    assert_eq!(c, 1);
    assert!(!out.contains("sekrit123"));
    assert!(out.contains("redacted"));
}

#[test]
fn missing_file_fails() {
    let f = Fx::new("missing");
    fs::remove_file(f.p("/opt/galion/bin/pw.py")).unwrap();
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 1);
    assert_eq!(j["missing"][0], "/opt/galion/bin/pw.py");
}

#[test]
fn out_then_verify_then_tamper() {
    let f = Fx::new("verify");
    let out = f.0.with_extension("out");
    let (c, _) = f.run(&["plan", "--out", out.to_str().unwrap()]);
    assert_eq!(c, 0);
    for n in [
        "phase-a.plan.txt",
        "phase-a.sha256",
        "phase-a.files",
        "phase-a.json",
    ] {
        assert!(out.join(n).is_file(), "{n}");
    }
    let files = fs::read_to_string(out.join("phase-a.files")).unwrap();
    assert_eq!(files.lines().count(), 5);
    // never overwrite
    let (c, t) = f.run(&["plan", "--out", out.to_str().unwrap()]);
    assert_eq!(c, 2, "{t}");
    let man = out.join("phase-a.sha256");
    let (c, t) = f.run(&["verify", "--manifest", man.to_str().unwrap()]);
    assert_eq!(c, 0, "{t}");
    assert!(t.contains("all sha256 match"));
    f.w("/opt/galion/bin/replicate-picture.py", "changed");
    let (c, t) = f.run(&["verify", "--manifest", man.to_str().unwrap()]);
    assert_eq!(c, 1);
    assert!(t.contains("MISMATCH /opt/galion/bin/replicate-picture.py"));
}

#[test]
fn refuses_out_in_live_tree_and_bad_usage() {
    let f = Fx::new("usage");
    let (c, t) = f.run(&["plan", "--out", "/opt/galion/archive/x"]);
    assert_eq!(c, 2);
    assert!(t.contains("refusing"));
    assert!(!Path::new("/opt/galion/archive/x").exists());
    assert_eq!(f.run(&["verify"]).0, 2);
    assert_eq!(f.run(&["delete"]).0, 2);
    assert_eq!(f.run(&["plan", "--bogus"]).0, 2);
}
