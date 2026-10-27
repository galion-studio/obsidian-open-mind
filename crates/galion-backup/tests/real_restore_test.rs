// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Wave-4 slot 326 real test (GALION_REAL=1): the VPS
//! `/opt/galion/rust/bin/galion-restore-test.sh` (verbatim below, sha256
//! checked) vs the release-built `galion-backup restore-test`, on the SAME
//! real archives made by the real `galion-backup run` from a fake VPS tree
//! (placeholder secrets only), plus a GNU-tar repack of one. The oracle runs
//! with its helper path pointed at this build's `galion-backup verify` (the
//! only edit; the live .sh calls the pre-t273u helper ELF there, the slot 311
//! blocker). Stdout must be byte-identical and both must agree on pass/fail
//! in every scenario. Rust-only extras: hostile archives (`..`, symlink
//! escape) are refused, the temp dir is always removed (also on SIGTERM).
//!
//! `GALION_REAL=1 cargo test --release -p galion-backup --test real_restore_test -- --nocapture --test-threads=1`
//!
//! VPS half (`real_vps_restore_parity`, needs also GALION_VPS_SSH=<ssh alias>
//! and GALION_RESTORE_VPS_BIN=<static x86_64 musl galion-backup>): copies the
//! binary to a throwaway /var/tmp/restore-test-326-<ts> on the VPS, runs the
//! live .sh and the Rust subcommand on the newest REAL archive, diffs the
//! stdout, then removes the folder. Nothing live is changed; no file
//! content is printed (the outputs are names, counts and ok/FAILED only).

mod common;

use std::fs;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use galion_backup::backup;

const BIN: &str = env!("CARGO_BIN_EXE_galion-backup");
const LIVE_HELPER: &str = "/opt/galion/rust/bin/galion-backup";

/// VPS `/opt/galion/rust/bin/galion-restore-test.sh`, verbatim (read-only
/// `cat` over ssh, 2026-10-28 00:16 CET +01:00).
const ORACLE: &str = r#"#!/bin/bash
# Restore the newest (or given) backup into a temp dir and verify it. Touches nothing live.
set -euo pipefail
umask 077
DEST=${GALION_BACKUP_DEST:-/var/backups/galion}
A=${1:-$(ls -1t "$DEST"/galion-*.tar.gz | head -1)}
T=$(mktemp -d /tmp/galion-restore-XXXXXX); trap 'rm -rf "$T"' EXIT
tar -C "$T" -xzf "$A"
cd "$T"/galion-*
sha256sum --quiet -c SHA256SUMS && echo "checksums ok"
for d in db/*.db; do /opt/galion/rust/bin/galion-backup verify "$d" | sed "s|$T/||"; done
mkdir -p restored && tar -C restored -xf files/files.tar
echo "files restored: $(find restored -type f | wc -l)"
for p in var/lib/memory-tencentdb/persona.md var/lib/hermes-rs root/galion-secrets/admin-device.json etc/openmind-memory.env; do
  [ -e "restored/$p" ] && echo "present: /$p" || echo "MISSING: /$p"
done
echo "restore test ok: $(basename "$A")"
"#;
const ORACLE_SHA256: &str = "264397750a31083298e8e926934d02ce84418c2b3501a12bb0af74ada8170f14";

fn say(m: &str) {
    println!("[{}] {m}", backup::now_iso());
}

fn sha(b: &[u8]) -> String {
    let mut h = galion_backup::sha256::Sha256::new();
    h.update(b);
    galion_backup::sha256::hex(&h.finish())
}

fn real() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return false;
    }
    true
}

struct Env {
    dir: PathBuf,
    oracle: PathBuf,
    rtmp: PathBuf,
}

fn setup(tag: &str) -> Env {
    assert_eq!(
        sha(ORACLE.as_bytes()),
        ORACLE_SHA256,
        "oracle is the VPS script"
    );
    let dir = common::tmp(&format!("rt326-{tag}"));
    let oracle = dir.join("galion-restore-test.sh");
    let body = ORACLE.replace(LIVE_HELPER, BIN);
    assert_eq!(
        body.matches(BIN).count(),
        1,
        "exactly one helper call rewired"
    );
    fs::write(&oracle, body).unwrap();
    let rtmp = dir.join("rtmp");
    fs::create_dir_all(&rtmp).unwrap();
    Env { dir, oracle, rtmp }
}

/// A real archive: the real `galion-backup run` (library, root = fake tree).
fn make_archive(root: &Path, dest: &Path) -> PathBuf {
    fs::create_dir_all(dest).unwrap();
    let before = backup::archives(dest);
    let o = backup::Opts {
        root: root.to_path_buf(),
        dest: dest.to_path_buf(),
        keep: 50,
        minfree_kb: Some(0),
        gzip_level: 6,
    };
    assert_eq!(backup::run(&o), 0, "backup run");
    let after = backup::archives(dest);
    let new: Vec<_> = after.into_iter().filter(|a| !before.contains(a)).collect();
    assert_eq!(new.len(), 1, "one new archive");
    new[0].clone()
}

fn fake_root(e: &Env, persona: bool, big_mb: usize) -> PathBuf {
    let root = e.dir.join("root");
    common::build_tree(&root);
    if persona {
        let p = root.join("var/lib/memory-tencentdb/persona.md");
        fs::write(&p, b"# placeholder persona\n").unwrap();
    }
    if big_mb > 0 {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let big: Vec<u8> = (0..big_mb << 20)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        fs::write(root.join("var/lib/hermes-rs/big.bin"), big).unwrap();
    }
    root
}

fn run_sh(e: &Env, dest: &Path, arg: Option<&Path>) -> Output {
    let mut c = Command::new("bash");
    c.arg(&e.oracle)
        .env("GALION_BACKUP_DEST", dest)
        .env("LC_ALL", "C");
    if let Some(a) = arg {
        c.arg(a);
    }
    c.output().unwrap()
}

fn run_rs(e: &Env, dest: &Path, arg: Option<&Path>) -> Output {
    let mut c = Command::new(BIN);
    c.arg("restore-test")
        .env("GALION_BACKUP_DEST", dest)
        .env("GALION_RESTORE_TMP", &e.rtmp);
    if let Some(a) = arg {
        c.arg(a);
    }
    c.output().unwrap()
}

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Both must agree on pass/fail and print the same stdout; temp dirs gone.
fn compare(name: &str, e: &Env, dest: &Path, arg: Option<&Path>, want_ok: bool) -> String {
    let t0 = Instant::now();
    let a = run_sh(e, dest, arg);
    let t_sh = t0.elapsed();
    let t1 = Instant::now();
    let b = run_rs(e, dest, arg);
    let t_rs = t1.elapsed();
    let (ao, bo) = (s(&a.stdout), s(&b.stdout));
    if ao != bo || a.status.success() != want_ok || b.status.success() != want_ok {
        panic!(
            "{name}: MISMATCH\n--- sh (exit {:?}) stdout:\n{ao}\n--- sh stderr:\n{}\n--- rs (exit {:?}) stdout:\n{bo}\n--- rs stderr:\n{}",
            a.status.code(),
            s(&a.stderr),
            b.status.code(),
            s(&b.stderr)
        );
    }
    let left: Vec<_> = fs::read_dir(&e.rtmp).unwrap().flatten().collect();
    assert!(left.is_empty(), "{name}: rust temp dir left behind");
    say(&format!(
        "PASS {name}: stdout identical ({} lines), sh exit {:?} {:.0} ms, rs exit {:?} {:.0} ms",
        bo.lines().count(),
        a.status.code(),
        t_sh.as_secs_f64() * 1e3,
        b.status.code(),
        t_rs.as_secs_f64() * 1e3
    ));
    if !want_ok {
        say(&format!(
            "     sh stderr: {}",
            s(&a.stderr).lines().last().unwrap_or("")
        ));
        say(&format!(
            "     rs stderr: {}",
            s(&b.stderr).lines().last().unwrap_or("")
        ));
    }
    bo
}

/// Live-.sh bug F-326-1: `sha256sum --quiet -c SHA256SUMS && echo "checksums ok"`
/// is an `&&` list, so `set -e` does NOT stop on a mismatch: the .sh prints
/// `FAILED`, carries on and ends `restore test ok` with exit 0. The Rust port
/// stops there with exit 1. Asserts exactly that: same FAILED lines first,
/// .sh exit 0 + "restore test ok", Rust exit 1 and nothing after the FAILED lines.
fn compare_sh_bug(name: &str, e: &Env, dest: &Path, arg: &Path) -> String {
    let a = run_sh(e, dest, Some(arg));
    let b = run_rs(e, dest, Some(arg));
    let (ao, bo) = (s(&a.stdout), s(&b.stdout));
    let ok = a.status.success()
        && !b.status.success()
        && ao.starts_with(&bo)
        && !bo.is_empty()
        && bo.lines().all(|l| l.contains(": FAILED"))
        && !ao.contains("checksums ok")
        && ao.contains("restore test ok");
    assert!(
        ok,
        "{name}: unexpected\n--- sh (exit {:?}):\n{ao}\n--- rs (exit {:?}):\n{bo}\n{}",
        a.status.code(),
        b.status.code(),
        s(&b.stderr)
    );
    assert!(
        fs::read_dir(&e.rtmp).unwrap().next().is_none(),
        "{name}: temp left"
    );
    say(&format!(
        "PASS {name}: F-326-1 reproduced: .sh exit 0 + `{}` despite `{}`; rust exit {:?} (`{}`)",
        ao.lines().last().unwrap_or(""),
        bo.trim(),
        b.status.code(),
        s(&b.stderr).lines().last().unwrap_or("")
    ));
    bo
}

fn sh(cmd: &str) {
    let st = Command::new("bash").arg("-c").arg(cmd).status().unwrap();
    assert!(st.success(), "{cmd}");
}

/// Unpack `a` with GNU tar, let `edit` change the tree, repack with GNU tar
/// (`tar -czf`, GNU format) as `out`.
fn repack(a: &Path, out: &Path, edit: impl FnOnce(&Path)) {
    let w = out.with_extension("unpack");
    let _ = fs::remove_dir_all(&w);
    fs::create_dir_all(&w).unwrap();
    sh(&format!("tar -C '{}' -xzf '{}'", w.display(), a.display()));
    let top = fs::read_dir(&w).unwrap().flatten().next().unwrap().path();
    edit(&top);
    sh(&format!(
        "cd '{}' && tar -czf '{}' '{}'",
        w.display(),
        out.display(),
        top.file_name().unwrap().to_string_lossy()
    ));
    fs::remove_dir_all(&w).unwrap();
}

fn resum(top: &Path) {
    sh(&format!(
        "cd '{}' && find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > SHA256SUMS",
        top.display()
    ));
}

#[test]
fn real_sh_vs_rust_restore_test() {
    if !real() {
        return;
    }
    let e = setup("main");
    let root = fake_root(&e, true, 8);
    let dest = e.dir.join("dest");
    let a1 = make_archive(&root, &dest);
    sh(&format!(
        "touch -d '2026-10-27 10:00:00' '{}'",
        a1.display()
    ));
    // A second backup a second later (stamp has 1 s resolution).
    std::thread::sleep(Duration::from_millis(1100));
    fs::write(root.join("var/lib/hermes-rs/second.txt"), b"second run\n").unwrap();
    let a2 = make_archive(&root, &dest);
    say(&format!(
        "archives: {} ({} B), {} ({} B)",
        a1.file_name().unwrap().to_string_lossy(),
        fs::metadata(&a1).unwrap().len(),
        a2.file_name().unwrap().to_string_lossy(),
        fs::metadata(&a2).unwrap().len()
    ));

    // S1 newest by mtime (no argument).
    let out = compare("S1 newest (no arg)", &e, &dest, None, true);
    let base2 = a2.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        out.ends_with(&format!("restore test ok: {base2}\n")),
        "{out}"
    );
    assert!(out.starts_with("checksums ok\nverify db/hermes-legacy-state.db integrity=ok"));
    for l in out.lines() {
        println!("      | {l}");
    }
    // S1b mtime decides, not the name: make the older-named one newest.
    sh(&format!(
        "touch -d '2030-01-01 00:00:00' '{}'",
        a1.display()
    ));
    let out = compare("S1b newest is by mtime", &e, &dest, None, true);
    assert!(out.contains(&format!(
        "restore test ok: {}",
        a1.file_name().unwrap().to_string_lossy()
    )));
    sh(&format!(
        "touch -d '2026-10-27 10:00:00' '{}'",
        a1.display()
    ));

    // S2 explicit archive argument.
    compare("S2 explicit arg", &e, &dest, Some(&a1), true);

    // S3 GNU tar repack of a Rust archive (GNU format, @LongLink names).
    let other = e.dir.join("other");
    fs::create_dir_all(&other).unwrap();
    let gnu = other.join("galion-gnu-repack.tar.gz");
    repack(&a2, &gnu, |_| {});
    compare("S3 GNU-tar repacked archive", &e, &dest, Some(&gnu), true);

    // S4 checksum mismatch.
    let bad = other.join("galion-badsum.tar.gz");
    repack(&a2, &bad, |t| {
        fs::write(t.join("db/MAP"), b"tampered\n").unwrap();
    });
    let out = compare_sh_bug("S4 checksum mismatch", &e, &dest, &bad);
    assert_eq!(out, "./db/MAP: FAILED\n");

    // S5 listed file missing.
    let miss = other.join("galion-missing.tar.gz");
    repack(&a2, &miss, |t| {
        fs::remove_file(t.join("db/MAP")).unwrap();
    });
    let out = compare_sh_bug("S5 listed file missing", &e, &dest, &miss);
    assert_eq!(out, "./db/MAP: FAILED open or read\n");

    // S6 corrupt DB with consistent SHA256SUMS (integrity_check must catch it).
    let cdb = other.join("galion-corruptdb.tar.gz");
    repack(&a2, &cdb, |t| {
        let p = t.join("db/memory-vectors.db");
        let mut b = fs::read(&p).unwrap();
        let n = b.len();
        assert!(n > 3 * 4096, "db big enough to corrupt a page");
        for x in &mut b[n - 2 * 4096 + 100..n - 4096 - 100] {
            *x = 0x5a;
        }
        fs::write(&p, b).unwrap();
        resum(t);
    });
    let out = compare(
        "S6 corrupt DB, sums consistent",
        &e,
        &dest,
        Some(&cdb),
        false,
    );
    assert!(out.starts_with("checksums ok\n"), "{out}");
    assert!(!out.contains("restore test ok"));

    // S7 truncated archive.
    let tr = other.join("galion-truncated.tar.gz");
    let b = fs::read(&a2).unwrap();
    fs::write(&tr, &b[..b.len() * 2 / 3]).unwrap();
    let out = compare("S7 truncated gzip", &e, &dest, Some(&tr), false);
    assert_eq!(out, "");

    // S8 no archives in DEST.
    let empty = e.dir.join("empty-dest");
    fs::create_dir_all(&empty).unwrap();
    compare("S8 no archives", &e, &empty, None, false);

    // S9 nonexistent explicit archive.
    compare(
        "S9 missing archive arg",
        &e,
        &dest,
        Some(&other.join("nope.tar.gz")),
        false,
    );

    // S10 persona.md missing -> MISSING line, still ok (the .sh only reports).
    fs::remove_file(root.join("var/lib/memory-tencentdb/persona.md")).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let a3 = make_archive(&root, &dest);
    let out = compare("S10 persona missing", &e, &dest, Some(&a3), true);
    assert!(out.contains("MISSING: /var/lib/memory-tencentdb/persona.md\n"));
    assert!(out.contains("present: /root/galion-secrets/admin-device.json\n"));

    // S11 an archive with two galion-* top dirs must fail both (`cd` gets 2 args).
    let two = other.join("galion-twotop.tar.gz");
    let w = e.dir.join("twotop");
    fs::create_dir_all(&w).unwrap();
    sh(&format!("tar -C '{}' -xzf '{}'", w.display(), a2.display()));
    fs::create_dir_all(w.join("galion-extra")).unwrap();
    sh(&format!(
        "cd '{}' && tar -czf '{}' galion-*",
        w.display(),
        two.display()
    ));
    compare("S11 two galion-* dirs", &e, &dest, Some(&two), false);

    let _ = fs::remove_dir_all(&e.dir);
}

fn gz_tar(entries: &[galion_tar::ustar::Entry], out: &Path) {
    let mut t = Vec::new();
    for en in entries {
        galion_tar::ustar::write_entry(&mut t, en).unwrap();
    }
    galion_tar::ustar::finish(&mut t).unwrap();
    let mut g = galion_backup::gzip::GzWriter::new(Vec::new(), 6).unwrap();
    g.write_all(&t).unwrap();
    fs::write(out, g.finish().unwrap()).unwrap();
}

fn ent(
    kind: galion_tar::ustar::Kind,
    name: &str,
    link: &str,
    data: &[u8],
) -> galion_tar::ustar::Entry {
    galion_tar::ustar::Entry {
        kind,
        name: name.into(),
        mode: if kind == galion_tar::ustar::Kind::Dir {
            0o755
        } else {
            0o644
        },
        uid: 0,
        gid: 0,
        mtime: 1_761_600_000,
        link: link.into(),
        data: data.to_vec(),
    }
}

#[test]
fn real_rust_refuses_hostile_archives() {
    if !real() {
        return;
    }
    use galion_tar::ustar::Kind::*;
    let e = setup("hostile");
    let dest = e.dir.join("dest");
    fs::create_dir_all(&dest).unwrap();
    let outside = e.dir.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let cases: Vec<(&str, Vec<galion_tar::ustar::Entry>)> = vec![
        (
            "dotdot",
            vec![
                ent(Dir, "galion-x", "", b""),
                ent(File, "galion-x/../../../outside/pwn", "", b"pwned\n"),
            ],
        ),
        (
            "symlink-escape",
            vec![
                ent(Dir, "galion-x", "", b""),
                ent(Symlink, "galion-x/esc", outside.to_str().unwrap(), b""),
                ent(File, "galion-x/esc/pwn", "", b"pwned\n"),
            ],
        ),
    ];
    for (name, entries) in cases {
        let a = dest.join(format!("galion-{name}.tar.gz"));
        gz_tar(&entries, &a);
        let o = run_rs(&e, &dest, Some(&a));
        assert!(!o.status.success(), "{name}: must fail");
        let err = s(&o.stderr);
        assert!(err.contains("refusing"), "{name}: {err}");
        assert!(!outside.join("pwn").exists(), "{name}: escaped!");
        assert!(
            fs::read_dir(&e.rtmp).unwrap().next().is_none(),
            "{name}: temp left"
        );
        say(&format!("PASS hostile {name}: refused ({})", err.trim()));
    }
    let _ = fs::remove_dir_all(&e.dir);
}

#[test]
fn real_sigterm_cleans_temp() {
    if !real() {
        return;
    }
    let e = setup("sigterm");
    let root = fake_root(&e, true, 96);
    let dest = e.dir.join("dest");
    let a = make_archive(&root, &dest);
    let mut killed = 0;
    for delay_ms in [30u64, 80, 150] {
        let mut ch = Command::new(BIN)
            .arg("restore-test")
            .arg(&a)
            .env("GALION_RESTORE_TMP", &e.rtmp)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(delay_ms));
        // SAFETY: plain kill(2) on our own child.
        unsafe {
            libc::kill(ch.id() as i32, libc::SIGTERM);
        }
        let st = ch.wait().unwrap();
        let left = fs::read_dir(&e.rtmp).unwrap().count();
        assert_eq!(left, 0, "temp dir left after SIGTERM at {delay_ms} ms");
        match (st.code(), st.signal()) {
            (Some(143), _) => killed += 1,
            (Some(0), _) => {}
            other => panic!("unexpected exit {other:?} at {delay_ms} ms"),
        }
        say(&format!(
            "SIGTERM at {delay_ms} ms -> exit {:?}, temp dir removed",
            st.code()
        ));
    }
    assert!(killed > 0, "no run was still busy when SIGTERM arrived");
    let _ = fs::remove_dir_all(&e.dir);
}

fn ssh(host: &str, cmd: &str) -> Output {
    Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", host, cmd])
        .output()
        .expect("ssh")
}

#[test]
fn real_vps_restore_parity() {
    if !real() {
        return;
    }
    let (Ok(host), Ok(bin)) = (
        std::env::var("GALION_VPS_SSH"),
        std::env::var("GALION_RESTORE_VPS_BIN"),
    ) else {
        eprintln!("skipped (set GALION_VPS_SSH and GALION_RESTORE_VPS_BIN)");
        return;
    };
    // The live .sh must still be the one we ported.
    let o = ssh(
        &host,
        "sha256sum /opt/galion/rust/bin/galion-restore-test.sh",
    );
    assert!(o.status.success());
    assert!(
        s(&o.stdout).starts_with(ORACLE_SHA256),
        "live .sh changed: {}",
        s(&o.stdout)
    );
    let ts = backup::stamp_from_rfc3339(&backup::now_iso());
    let dir = format!("/var/tmp/restore-test-326-{ts}");
    let o = ssh(&host, &format!("umask 077 && mkdir {dir} && mkdir {dir}/t"));
    assert!(o.status.success(), "mkdir: {}", s(&o.stderr));
    let st = Command::new("scp")
        .args([
            "-q",
            "-o",
            "BatchMode=yes",
            &bin,
            &format!("{host}:{dir}/galion-backup"),
        ])
        .status()
        .unwrap();
    assert!(st.success(), "scp");
    // Every archive on the VPS (newest first): Rust-made (t273u+) and older ones.
    let script = format!(
        "set -u; cd {dir}; for A in $(ls -1t /var/backups/galion/galion-*.tar.gz); do echo \"archive=$(basename $A) size=$(stat -c %s $A)\"; \
         s0=$(date +%s%N); /opt/galion/rust/bin/galion-restore-test.sh \"$A\" > sh.out 2> sh.err; echo sh_rc=$?; s1=$(date +%s%N); \
         GALION_RESTORE_TMP={dir}/t nice -n 10 ./galion-backup restore-test \"$A\" > rs.out 2> rs.err; echo rs_rc=$?; s2=$(date +%s%N); \
         echo sh_ms=$(( (s1-s0)/1000000 )) rs_ms=$(( (s2-s1)/1000000 )); \
         if cmp -s sh.out rs.out; then echo STDOUT_IDENTICAL; else echo STDOUT_DIFF; diff sh.out rs.out | head -20; fi; \
         echo '--- rs.out'; cat rs.out; echo '--- sh.err'; head -5 sh.err; echo '--- rs.err'; head -5 rs.err; \
         echo t_left=$(ls -A {dir}/t | wc -l) tmp_left=$(ls -d /tmp/galion-restore-* 2>/dev/null | wc -l); \
         done; cd / && rm -rf {dir} && echo cleaned={dir}"
    );
    let o = ssh(&host, &script);
    let out = s(&o.stdout);
    for l in out.lines() {
        say(&format!("vps | {l}"));
    }
    assert!(o.status.success(), "ssh script: {}", s(&o.stderr));
    let n = out.matches("archive=").count();
    assert!(n >= 1, "no archives on the VPS");
    assert_eq!(out.matches("sh_rc=0").count(), n, "every .sh run passes");
    assert_eq!(out.matches("rs_rc=0").count(), n, "every rust run passes");
    assert_eq!(out.matches("STDOUT_IDENTICAL").count(), n);
    assert!(!out.contains("STDOUT_DIFF"));
    assert_eq!(out.matches("t_left=0").count(), n);
    assert!(out.contains("cleaned="));
}
