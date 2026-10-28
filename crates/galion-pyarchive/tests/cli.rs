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
            "# t314u 2026-10-27: cut over live galion-desk to Rust desk-hermes.\n# Rollback: rm this drop-in; systemctl daemon-reload; systemctl restart galion-desk\n# (old /opt/galion/bin/galion-desk + desk-hermes.py left untouched).\n[Service]\nExecStart=\nExecStart=/opt/galion/rust/bin/galion-desk\nEnvironment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes\n",
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

// ---- wave-4 slot 345: overridden-base units get an exact safe rollback ----

const DROPIN: &str = "/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf";

fn pos(t: &str, needle: &str) -> usize {
    t.find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in:\n{t}"))
}

#[test]
fn overridden_base_is_flagged_with_exact_safe_rollback() {
    let f = Fx::new("ovb");
    let before = f.snapshot();
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 0, "{j}");
    let ob = j["overridden_base"].as_array().unwrap();
    assert_eq!(ob.len(), 1, "{j}");
    let o = &ob[0];
    assert_eq!(o["unit"], "galion-desk.service");
    assert_eq!(o["base_file"], "/etc/systemd/system/galion-desk.service");
    assert_eq!(o["base_line"], 4);
    assert_eq!(o["section"], "[Service]");
    assert_eq!(o["key"], "ExecStart");
    assert_eq!(o["target"], "/opt/galion/bin/galion-desk");
    assert_eq!(o["by_file"], DROPIN);
    assert_eq!(o["by_line"], 6);
    assert_eq!(o["by_text"], "ExecStart=/opt/galion/rust/bin/galion-desk");
    assert_eq!(
        o["restore_together"],
        serde_json::json!([
            "/opt/galion/bin/galion-desk",
            "/opt/galion/bin/desk-hermes.py"
        ])
    );
    let notes = o["stale_rollback_notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["line"], 2);
    assert!(notes[0]["text"]
        .as_str()
        .unwrap()
        .contains("rm this drop-in"));
    let steps: Vec<&str> = o["safe_rollback"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    let t = steps.join("\n");
    // Order: restore both files, prove bytes, test -x, THEN drop-in + reload.
    let a = pos(&t, "mv -n /opt/galion/archive/py-bin-20261103/opt__galion__bin__galion-desk /opt/galion/bin/galion-desk");
    let b = pos(&t, "mv -n /opt/galion/archive/py-bin-20261103/opt__galion__bin__desk-hermes.py /opt/galion/bin/desk-hermes.py");
    let c2 = pos(&t, "sha256sum -c -   # expect 2 OK");
    let d = pos(&t, "test -x /opt/galion/bin/galion-desk");
    let e = pos(&t, &format!("remove {DROPIN}"));
    let g = pos(&t, "systemctl daemon-reload");
    let h = pos(&t, "systemctl show -p ExecStart galion-desk.service");
    assert!(a < b && b < c2 && c2 < d && d < e && e < g && g < h, "{t}");
    assert!(
        t.contains(r"/opt/galion/bin/galion-desk$|  /opt/galion/bin/desk-hermes\.py$"),
        "{t}"
    );
    assert!(t.contains("STALE rollback note"));
    // Text report and proposed commands carry the same flag.
    let (c, txt) = f.run(&["plan"]);
    assert_eq!(c, 0);
    assert!(
        txt.contains(
            "OVERRIDDEN-BASE galion-desk.service /etc/systemd/system/galion-desk.service:4"
        ),
        "{txt}"
    );
    assert!(
        txt.contains(&format!("STALE ROLLBACK NOTE {DROPIN}:2")),
        "{txt}"
    );
    assert!(txt.contains("# OVERRIDDEN-BASE UNITS (1)"), "{txt}");
    assert!(txt.contains("# ROLLBACK of Phase A (file level"), "{txt}");
    // Archive mv and rollback mv are exact inverses for every file.
    for p in [
        "/opt/galion/bin/desk-hermes.py",
        "/opt/galion/bin/galion-desk",
        "/opt/galion/bin/pw.py",
        "/opt/galion/bin/__pycache__/pw.cpython-312.pyc",
        "/opt/galion/bin/replicate-picture.py",
    ] {
        let arch = format!(
            "/opt/galion/archive/py-bin-20261103/{}",
            p.trim_start_matches('/').replace('/', "__")
        );
        assert!(txt.contains(&format!("mv -n {p} {arch}\n")), "{p}");
        assert!(txt.contains(&format!("mv -n {arch} {p}\n")), "{p}");
    }
    assert_eq!(before, f.snapshot(), "plan changed the scanned tree");
}

#[test]
fn override_without_note_and_bare_reset() {
    let f = Fx::new("ovb2");
    // Drop-in without a rollback comment.
    f.w(
        DROPIN,
        "[Service]\nExecStart=\nExecStart=/opt/galion/rust/bin/galion-desk\n",
    );
    // A second unit whose base names pw.py and whose drop-in only resets it.
    f.w(
        "/etc/systemd/system/pw-old.service",
        "[Service]\nExecStart=/opt/galion/bin/pw.py --x\n",
    );
    f.w(
        "/etc/systemd/system/pw-old.service.d/off.conf",
        "# rollback: delete this file\n[Service]\nExecStart=\n",
    );
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 0, "{j}");
    let ob = j["overridden_base"].as_array().unwrap();
    assert_eq!(ob.len(), 2, "{j}");
    let desk = ob
        .iter()
        .find(|o| o["unit"] == "galion-desk.service")
        .unwrap();
    assert!(desk["stale_rollback_notes"].as_array().unwrap().is_empty());
    let pw = ob.iter().find(|o| o["unit"] == "pw-old.service").unwrap();
    assert_eq!(
        pw["by_file"],
        "/etc/systemd/system/pw-old.service.d/off.conf"
    );
    assert_eq!(pw["by_line"], 3);
    assert_eq!(pw["by_text"], "ExecStart=");
    assert_eq!(
        pw["restore_together"],
        serde_json::json!(["/opt/galion/bin/pw.py"])
    );
    assert_eq!(pw["stale_rollback_notes"].as_array().unwrap().len(), 1);
}

#[test]
fn no_override_means_no_unit_rollback_section() {
    let f = Fx::new("ovb3");
    f.w(
        "/etc/systemd/system/galion-desk.service",
        "[Service]\nExecStart=/opt/galion/rust/bin/galion-desk\n",
    );
    f.w(DROPIN, "[Service]\nEnvironment=DESK_BIND=127.0.0.1:8646\n");
    let (c, j) = f.json(&["plan"]);
    assert_eq!(c, 0, "{j}");
    assert!(j["overridden_base"].as_array().unwrap().is_empty());
    let (_, txt) = f.run(&["plan"]);
    assert!(!txt.contains("OVERRIDDEN-BASE"), "{txt}");
    assert!(txt.contains("# ROLLBACK of Phase A (file level"));
}
