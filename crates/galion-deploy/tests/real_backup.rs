// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-deploy backup` on the box, against a fake root laid out like the
//! VPS (docs/ops/vps-baseline.md + the 2026-10-20 23:13 CEST read-only
//! inventory): tuner and galion-nerve binaries, unit files, state dir, static
//! files, the bash galion-deploy, plus a secrets tree and a `.env` that must
//! never be read. Checks the manifest, the tar (with GNU tar too), verify,
//! tamper detection, and the delegation of the data part to galion-backup.
//! With GALION_REAL=1 the real binaries are used as file contents (nerve-old
//! is byte-identical to the live galion-nerve 3b150dea…; the bash script is
//! the live 86207af5…) and the real galion-backup (swarm/rust-ports 26a2a8e)
//! is run for the failure path.

use std::path::{Path, PathBuf};
use std::process::Command;

fn ex(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join(name)
}

fn sim_bin() -> PathBuf {
    PathBuf::from(
        std::env::var("CUTOVER_SIM_BIN")
            .unwrap_or_else(|_| "/workspace/swarm/sim/cutover-prep/bin".into()),
    )
}

fn put(root: &Path, abs: &str, data: &[u8], mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let p = root.join(abs.trim_start_matches('/'));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, data).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn fake_root(root: &Path, real: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let read = |n: &str, fallback: &[u8]| {
        if real {
            std::fs::read(sim_bin().join(n)).unwrap_or_else(|_| panic!("missing {n} in sim bin"))
        } else {
            fallback.to_vec()
        }
    };
    let tuner = read("tuner-old", b"\x7fELFtuner-stand-in");
    let nerve = read("nerve-old", b"\x7fELFnerve-stand-in");
    let deploy = if real {
        Command::new("git")
            .args(["show", "e43d7ab:deploy/foundation/galion-deploy"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap()
            .stdout
    } else {
        b"#!/bin/bash\n# galion-deploy stand-in\n".to_vec()
    };
    put(root, "/opt/galion/tuner/bin/tuner", &tuner, 0o755);
    put(
        root,
        "/opt/galion/tuner/src/main.rs",
        b"fn main() {}\n",
        0o644,
    );
    for (f, n) in [
        ("index.html", 2356usize),
        ("style.css", 2737),
        ("app.js", 4570),
        ("pitch.js", 4266),
    ] {
        put(
            root,
            &format!("/opt/galion/tuner/www/{f}"),
            &vec![b'x'; n],
            0o644,
        );
    }
    put(root, "/opt/galion/tuner/test/test.js", b"// test\n", 0o644);
    put(
        root,
        "/etc/systemd/system/tuner.service",
        b"[Service]\nExecStart=/opt/galion/tuner/bin/tuner\nRestart=always\nRestartSec=2\n",
        0o644,
    );
    put(root, "/run/systemd/transient/tuner-quick.service", b"[Service]\nExecStart=/usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650\n", 0o644);
    put(root, "/opt/galion/rust/bin/galion-nerve", &nerve, 0o755);
    put(root, "/etc/systemd/system/galion-nerve.service", b"[Unit]\nStartLimitIntervalSec=600\nStartLimitBurst=5\n[Service]\nExecStart=/opt/galion/rust/bin/galion-nerve serve\n", 0o644);
    put(
        root,
        "/var/lib/galion-nerve/state.json",
        &vec![b'{'; 13089],
        0o644,
    );
    put(root, "/var/lib/galion-nerve/.probe", b"probe\n", 0o644);
    put(
        root,
        "/var/lib/galion-nerve/self-eval/latest.md",
        b"# self-eval\n",
        0o644,
    );
    put(
        root,
        "/var/lib/galion-nerve/self-eval/latest.json",
        b"{}\n",
        0o644,
    );
    // must never be read or archived
    put(
        root,
        "/var/lib/galion-nerve/stray.env",
        b"NOT_FOR_BACKUP=1\n",
        0o600,
    );
    put(
        root,
        "/opt/galion/galion-light/secrets/canary",
        b"CANARY-SECRET\n",
        0o600,
    );
    put(root, "/opt/galion/rust/bin/galion-deploy", &deploy, 0o755);
    (tuner, nerve, deploy)
}

fn run(args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_galion-deploy"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr),
    )
}

fn only_dir(d: &Path) -> PathBuf {
    let v: Vec<PathBuf> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(v.len(), 1, "{v:?}");
    v[0].clone()
}

#[test]
fn pre_cutover_backup_overlay_manifest_verify_and_data_delegation() {
    let real = std::env::var("GALION_REAL").as_deref() == Ok("1");
    let w = std::env::temp_dir().join(format!("gd-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&w);
    let root = w.join("root");
    let (tuner, nerve, deploy) = fake_root(&root, real);
    let rs = root.to_str().unwrap();

    // 1) overlay + data part via a galion-backup stand-in that succeeds
    let dest = w.join("dest-ok");
    let fake = ex("fake_galion_backup");
    assert!(
        fake.exists(),
        "build the examples (plain `cargo test -p galion-deploy`)"
    );
    let (code, out) = run(&[
        "backup",
        "--root",
        rs,
        "--dest",
        dest.to_str().unwrap(),
        "--galion-backup",
        fake.to_str().unwrap(),
    ]);
    eprintln!("{out}");
    assert_eq!(code, 0, "{out}");
    let snap = only_dir(&dest);
    let name = snap.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.len() == 20 && (name.ends_with("+0200") || name.ends_with("+0100")),
        "Zagreb stamp: {name}"
    );
    let manifest = std::fs::read_to_string(snap.join("MANIFEST.tsv")).unwrap();
    eprintln!("{manifest}");
    let sums = std::fs::read_to_string(snap.join("SHA256SUMS")).unwrap();
    assert!(
        sums.contains("  data/galion-20261020-231900+0200.tar.gz"),
        "{sums}"
    );
    assert!(manifest
        .contains("# data: galion-backup run exit 0, data/galion-20261020-231900+0200.tar.gz"));
    let sha = |b: &[u8]| galion_deploy::sha256::hex(b);
    for (path, bytes) in [
        ("/opt/galion/tuner/bin/tuner", &tuner),
        ("/opt/galion/rust/bin/galion-nerve", &nerve),
        ("/opt/galion/rust/bin/galion-deploy", &deploy),
    ] {
        let line = manifest
            .lines()
            .find(|l| l.ends_with(&format!("\t{path}")))
            .unwrap_or_else(|| panic!("{path} not in manifest"));
        assert!(
            line.contains(&sha(bytes)) && line.contains(&format!("\t{}\t", bytes.len())),
            "{line}"
        );
    }
    for p in [
        "/etc/systemd/system/tuner.service",
        "/etc/systemd/system/galion-nerve.service",
        "/var/lib/galion-nerve/state.json",
        "/opt/galion/tuner/www/index.html",
        "/run/systemd/transient/tuner-quick.service",
    ] {
        assert!(manifest.contains(&format!("\t{p}\n")), "{p} missing");
    }
    assert!(manifest.contains("absent (optional) /etc/systemd/system/tuner.service.d"));
    assert!(manifest.contains("skipped /var/lib/galion-nerve/stray.env"));
    let tar_bytes = std::fs::read(snap.join("backup.tar")).unwrap();
    assert!(!tar_bytes.windows(13).any(|x| x == b"CANARY-SECRET"));
    assert!(!tar_bytes.windows(14).any(|x| x == b"NOT_FOR_BACKUP"));
    if real {
        assert_eq!(
            sha(&nerve),
            "3b150dea692ffac098945e9413c4fe1caf95ee3a50d3f6c937e09df3598d6d3d"
        );
        assert_eq!(
            sha(&deploy),
            "86207af5617720098c820dee4f6d994e4b4dc57d5a46903b76ef9d85e83aea0b"
        );
    }
    // GNU tar reads it and restores one path byte-identically
    let x = w.join("extract");
    std::fs::create_dir_all(&x).unwrap();
    let st = Command::new("tar")
        .args([
            "-xpf",
            snap.join("backup.tar").to_str().unwrap(),
            "-C",
            x.to_str().unwrap(),
            "opt/galion/rust/bin/galion-nerve",
        ])
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(
        std::fs::read(x.join("opt/galion/rust/bin/galion-nerve")).unwrap(),
        nerve
    );
    // perms survive
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(snap.join("backup.tar"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&snap).unwrap().permissions().mode() & 0o777,
        0o700
    );

    // 2) verify: clean, then live drift, then tamper
    let (c, o) = run(&["backup-verify", snap.to_str().unwrap(), "--live-root", rs]);
    assert_eq!(c, 0, "{o}");
    assert!(o.contains("OK") && !o.contains("live drift"), "{o}");
    put(
        &root,
        "/var/lib/galion-nerve/state.json",
        b"{\"changed\":1}",
        0o644,
    );
    let (c, o) = run(&["backup-verify", snap.to_str().unwrap(), "--live-root", rs]);
    assert_eq!(c, 0, "drift is informational: {o}");
    assert!(
        o.contains("live drift: /var/lib/galion-nerve/state.json"),
        "{o}"
    );
    let mut t = tar_bytes.clone();
    let pos = t.windows(4).position(|x| x == b"\x7fELF").unwrap();
    t[pos + 5] ^= 0xff; // inside the stand-in too (18 B), not block padding
    std::fs::write(snap.join("backup.tar"), &t).unwrap();
    let (c, o) = run(&["backup-verify", snap.to_str().unwrap()]);
    assert_eq!(c, 1, "{o}");
    assert!(
        o.contains("backup.tar: sha256 does not match SHA256SUMS")
            && o.contains("size or sha256 differs"),
        "{o}"
    );

    // 3) a secrets path as a capture target is refused outright; unknown unit refused
    let (c, o) = run(&[
        "backup",
        "--root",
        rs,
        "--dest",
        w.join("d3").to_str().unwrap(),
        "--unit",
        "hermes-rs",
        "--no-data",
    ]);
    assert_eq!(c, 1, "{o}");
    assert!(o.contains("no backup plan for unit hermes-rs"));

    // 4) required path missing -> refused, nothing half-written
    std::fs::remove_file(root.join("opt/galion/rust/bin/galion-nerve")).unwrap();
    let d4 = w.join("d4");
    let (c, o) = run(&[
        "backup",
        "--root",
        rs,
        "--dest",
        d4.to_str().unwrap(),
        "--no-data",
    ]);
    assert_eq!(c, 1, "{o}");
    assert!(
        o.contains("required path missing: /opt/galion/rust/bin/galion-nerve"),
        "{o}"
    );
    assert!(!d4.exists());
    put(&root, "/opt/galion/rust/bin/galion-nerve", &nerve, 0o755);

    // 5) the real galion-backup on the box has no production paths: overlay is
    //    still written and verified, and the command fails loudly (exit 1).
    let gb = sim_bin().join("galion-backup");
    if real && gb.exists() {
        let d5 = w.join("d5");
        let (c, o) = run(&[
            "backup",
            "--root",
            rs,
            "--dest",
            d5.to_str().unwrap(),
            "--galion-backup",
            gb.to_str().unwrap(),
        ]);
        eprintln!("{o}");
        assert_eq!(c, 1, "{o}");
        assert!(
            o.contains("overlay written and verified") && o.contains("galion-backup run exited 1"),
            "{o}"
        );
        let s5 = only_dir(&d5);
        assert!(std::fs::read_to_string(s5.join("data/galion-backup.log"))
            .unwrap()
            .contains("no files to archive"));
        assert!(std::fs::read_to_string(s5.join("VERIFY.txt"))
            .unwrap()
            .contains("OK"));
    }
    let _ = std::fs::remove_dir_all(&w);
}
