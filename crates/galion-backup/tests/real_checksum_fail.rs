// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Wave-4 slot 361 real test (GALION_REAL=1): `galion-backup restore-test`
//! must fail HARD (exit 1, nothing after the FAILED lines, one clear
//! `checksum check FAILED ... NOT verified` line naming the archive) on ANY
//! checksum problem in a REAL backup made by the real `galion-backup run`
//! (fake VPS tree, placeholder secrets only), each tampered one way. The live
//! .sh (verbatim, sha256 checked, helper path pointed at this build) runs on
//! the same archives so the record shows which tampers it lets through
//! (F-326-1: exit 0 + `restore test ok`). Also: the staged swap (symlink
//! `galion-restore-test.sh` -> the ELF) behaves exactly like `restore-test`.
//!
//! `GALION_REAL=1 cargo test --release -p galion-backup --test real_checksum_fail -- --nocapture --test-threads=1`
//!
//! VPS half (`real_vps_checksum_fail`, also GALION_VPS_SSH=<alias> and
//! GALION_RESTORE_VPS_BIN=<static x86_64 musl galion-backup>): throwaway
//! 0700 /var/tmp/restore-test-361-<ts>; the Rust tool (and its symlink alias)
//! on EVERY real archive (must pass, stdout identical to the live .sh), then
//! tampered COPIES of the newest real archive inside the throwaway dir (must
//! fail; the .sh result is recorded). Folder removed. Only names, counts,
//! exit codes and ok/FAILED lines are printed, never file content. Nothing
//! live is changed (no unit, no file outside the throwaway dir).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use galion_backup::backup;

const BIN: &str = env!("CARGO_BIN_EXE_galion-backup");
const LIVE_HELPER: &str = "/opt/galion/rust/bin/galion-backup";

/// VPS `/opt/galion/rust/bin/galion-restore-test.sh`, verbatim (same text as
/// tests/real_restore_test.rs; sha256 re-checked live in the VPS half).
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

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn real() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return false;
    }
    true
}

fn sh(cmd: &str) {
    let st = Command::new("bash").arg("-c").arg(cmd).status().unwrap();
    assert!(st.success(), "{cmd}");
}

struct Env {
    dir: PathBuf,
    oracle: PathBuf,
    alias: PathBuf,
    rtmp: PathBuf,
}

fn setup(tag: &str) -> Env {
    assert_eq!(
        sha(ORACLE.as_bytes()),
        ORACLE_SHA256,
        "oracle is the VPS script"
    );
    let dir = common::tmp(&format!("rt361-{tag}"));
    let oracle = dir.join("oracle-restore-test.sh");
    let body = ORACLE.replace(LIVE_HELPER, BIN);
    assert_eq!(body.matches(BIN).count(), 1);
    fs::write(&oracle, body).unwrap();
    // The staged swap: the .sh name becomes a symlink to the ELF.
    let bindir = dir.join("bin");
    fs::create_dir_all(&bindir).unwrap();
    let alias = bindir.join("galion-restore-test.sh");
    std::os::unix::fs::symlink(BIN, &alias).unwrap();
    let rtmp = dir.join("rtmp");
    fs::create_dir_all(&rtmp).unwrap();
    Env {
        dir,
        oracle,
        alias,
        rtmp,
    }
}

fn make_archive(root: &Path, dest: &Path) -> PathBuf {
    fs::create_dir_all(dest).unwrap();
    let o = backup::Opts {
        root: root.to_path_buf(),
        dest: dest.to_path_buf(),
        keep: 50,
        minfree_kb: Some(0),
        gzip_level: 6,
    };
    assert_eq!(backup::run(&o), 0, "backup run");
    let mut a = backup::archives(dest);
    assert_eq!(a.len(), 1);
    a.pop().unwrap()
}

fn run_sh(e: &Env, a: &Path) -> Output {
    Command::new("bash")
        .arg(&e.oracle)
        .arg(a)
        .env("LC_ALL", "C")
        .output()
        .unwrap()
}

fn run_rs(e: &Env, a: &Path, alias: bool) -> Output {
    let mut c = if alias {
        Command::new(&e.alias)
    } else {
        let mut c = Command::new(BIN);
        c.arg("restore-test");
        c
    };
    c.arg(a)
        .env("GALION_RESTORE_TMP", &e.rtmp)
        .output()
        .unwrap()
}

/// GNU-tar unpack, edit, repack (no re-sum: the edit is the tamper).
fn repack(a: &Path, out: &Path, edit: &dyn Fn(&Path)) {
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
    sh(&format!("chmod -R u+w '{}'", w.display()));
    fs::remove_dir_all(&w).unwrap();
}

fn flip(p: &Path, at: usize) {
    let mut b = fs::read(p).unwrap();
    let i = at.min(b.len() - 1);
    b[i] ^= 0x01;
    fs::write(p, b).unwrap();
}

/// Rust must fail hard with a clear message (direct and via the alias);
/// returns the .sh verdict on the same archive.
fn expect_hard_fail(name: &str, e: &Env, a: &Path, want_checksum_msg: bool) -> String {
    let base = a.file_name().unwrap().to_string_lossy().into_owned();
    let mut last = String::new();
    for alias in [false, true] {
        let o = run_rs(e, a, alias);
        let (out, err) = (s(&o.stdout), s(&o.stderr));
        assert_eq!(
            o.status.code(),
            Some(1),
            "{name} alias={alias}: exit\n{out}\n{err}"
        );
        assert!(!out.contains("checksums ok"), "{name}: {out}");
        assert!(!out.contains("restore test ok"), "{name}: {out}");
        assert!(out.lines().all(|l| l.contains(": FAILED")), "{name}: {out}");
        let l = err.lines().last().unwrap_or("").to_string();
        assert!(
            l.starts_with("galion-backup: restore-test: "),
            "{name}: {l}"
        );
        assert!(l.contains(&base), "{name}: archive not named: {l}");
        if want_checksum_msg {
            assert!(
                l.contains("checksum check FAILED") && l.contains("NOT verified"),
                "{name}: {l}"
            );
        }
        assert!(
            fs::read_dir(&e.rtmp).unwrap().next().is_none(),
            "{name}: temp left"
        );
        last = l;
    }
    let o = run_sh(e, a);
    let out = s(&o.stdout);
    let verdict = if o.status.success() && out.contains("restore test ok") {
        "sh exit 0 + `restore test ok` (LET THROUGH, F-326-1)".to_string()
    } else {
        format!("sh exit {:?} (caught)", o.status.code())
    };
    say(&format!(
        "PASS {name}: rust exit 1 (direct + alias); {verdict}"
    ));
    say(&format!(
        "     rs: {}",
        last.replace(&e.dir.display().to_string(), "<tmp>")
    ));
    verdict
}

#[test]
fn real_tampered_backups_fail_hard() {
    if !real() {
        return;
    }
    let e = setup("main");
    let root = e.dir.join("root");
    common::build_tree(&root);
    fs::write(
        root.join("var/lib/memory-tencentdb/persona.md"),
        b"# placeholder persona\n",
    )
    .unwrap();
    let dest = e.dir.join("dest");
    let a = make_archive(&root, &dest);
    say(&format!(
        "real archive: {} ({} B)",
        a.file_name().unwrap().to_string_lossy(),
        fs::metadata(&a).unwrap().len()
    ));

    // T0 clean: Rust (direct + alias) passes, stdout identical to the .sh.
    let sh0 = run_sh(&e, &a);
    assert!(sh0.status.success(), "{}", s(&sh0.stderr));
    for alias in [false, true] {
        let o = run_rs(&e, &a, alias);
        assert!(o.status.success(), "clean alias={alias}: {}", s(&o.stderr));
        assert_eq!(s(&o.stdout), s(&sh0.stdout), "clean stdout alias={alias}");
    }
    say(&format!(
        "PASS T0 clean archive: rust ok direct + via galion-restore-test.sh symlink, stdout identical to .sh ({} lines)",
        s(&sh0.stdout).lines().count()
    ));

    let t = e.dir.join("tampered");
    fs::create_dir_all(&t).unwrap();
    let ext = e.dir.join("outside-map");
    let ext_name = ext.display().to_string();
    let empty_sha = sha(b"");
    let sums = |top: &Path| top.join("SHA256SUMS");
    type Edit<'a> = Box<dyn Fn(&Path) + 'a>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "T1 one bit flipped in db/memory-vectors.db",
            Box::new(|top: &Path| flip(&top.join("db/memory-vectors.db"), 5000)),
        ),
        (
            "T2 one bit flipped in files/files.tar",
            Box::new(|top: &Path| flip(&top.join("files/files.tar"), 700)),
        ),
        (
            "T3 db/MAP truncated to 0 bytes",
            Box::new(|top: &Path| fs::write(top.join("db/MAP"), b"").unwrap()),
        ),
        (
            "T4 listed file deleted (db/MAP)",
            Box::new(|top: &Path| fs::remove_file(top.join("db/MAP")).unwrap()),
        ),
        (
            "T5 SHA256SUMS line for db/MAP removed",
            Box::new(move |top: &Path| {
                let k: String = fs::read_to_string(sums(top))
                    .unwrap()
                    .lines()
                    .filter(|l| !l.ends_with("./db/MAP"))
                    .map(|l| format!("{l}\n"))
                    .collect();
                fs::write(sums(top), k).unwrap();
            }),
        ),
        (
            "T6 unlisted file injected (db/notes.txt)",
            Box::new(|top: &Path| fs::write(top.join("db/notes.txt"), b"injected\n").unwrap()),
        ),
        (
            "T7 malformed line appended to SHA256SUMS",
            Box::new(move |top: &Path| {
                let mut x = fs::read_to_string(sums(top)).unwrap();
                x.push_str("not a checksum line\n");
                fs::write(sums(top), x).unwrap();
            }),
        ),
        (
            "T8 one digest digit changed in SHA256SUMS",
            Box::new(move |top: &Path| {
                let mut b = fs::read(sums(top)).unwrap();
                b[0] = if b[0] == b'0' { b'1' } else { b'0' };
                fs::write(sums(top), b).unwrap();
            }),
        ),
        (
            "T9 absolute name with valid digest appended",
            Box::new(|top: &Path| {
                fs::write(&ext, b"").unwrap();
                let mut x = fs::read_to_string(sums(top)).unwrap();
                x.push_str(&format!("{empty_sha}  {ext_name}\n"));
                fs::write(sums(top), x).unwrap();
            }),
        ),
        (
            "T10 db/MAP swapped for symlink to outside copy (same digest)",
            Box::new(|top: &Path| {
                let m = top.join("db/MAP");
                fs::write(&ext, fs::read(&m).unwrap()).unwrap();
                fs::remove_file(&m).unwrap();
                std::os::unix::fs::symlink(&ext, &m).unwrap();
            }),
        ),
        (
            "T11 three tampers at once",
            Box::new(|top: &Path| {
                flip(&top.join("db/memory-vectors.db"), 9000);
                fs::remove_file(top.join("db/MAP")).unwrap();
                fs::write(top.join("extra.bin"), b"x").unwrap();
            }),
        ),
    ];
    let mut rows: Vec<(String, String)> = Vec::new();
    for (i, (name, edit)) in cases.iter().enumerate() {
        let p = t.join(format!("galion-t{}.tar.gz", i + 1));
        repack(&a, &p, edit.as_ref());
        rows.push((name.to_string(), expect_hard_fail(name, &e, &p, true)));
    }

    // Container-level checksums: gzip CRC32 / ISIZE trailer, tar header checksum.
    let raw = fs::read(&a).unwrap();
    let n = raw.len();
    for (name, at, file) in [
        ("G1 gzip CRC32 trailer bit flipped", n - 6, "g1"),
        ("G2 gzip ISIZE trailer bit flipped", n - 2, "g2"),
    ] {
        let p = t.join(format!("galion-{file}.tar.gz"));
        let mut b = raw.clone();
        b[at] ^= 0x01;
        fs::write(&p, &b).unwrap();
        rows.push((name.into(), expect_hard_fail(name, &e, &p, false)));
    }
    let g3 = t.join("galion-g3.tar.gz");
    let plain = t.join("g3.tar");
    sh(&format!(
        "gzip -dc '{}' > '{}'",
        a.display(),
        plain.display()
    ));
    let mut tb = fs::read(&plain).unwrap();
    let size = |h: &[u8]| -> usize {
        let f = String::from_utf8_lossy(&h[124..136])
            .trim_matches(|c: char| c == '\0' || c == ' ')
            .to_string();
        usize::from_str_radix(&f, 8).unwrap_or(0)
    };
    // Second header (first member after the top dir), a byte of its name.
    let off = 512 + size(&tb[0..512]).div_ceil(512) * 512;
    tb[off + 10] ^= 0x01;
    fs::write(&plain, &tb).unwrap();
    sh(&format!(
        "gzip -c '{}' > '{}'",
        plain.display(),
        g3.display()
    ));
    let name = "G3 tar header checksum broken";
    rows.push((name.into(), expect_hard_fail(name, &e, &g3, false)));

    let through = rows
        .iter()
        .filter(|(_, v)| v.contains("LET THROUGH"))
        .count();
    say(&format!(
        "SUMMARY: {} tamper cases; rust failed hard on {}/{}; live .sh let {through}/{} through with exit 0 + `restore test ok`",
        rows.len(),
        rows.len(),
        rows.len(),
        rows.len()
    ));
    for (n, v) in &rows {
        if v.contains("LET THROUGH") {
            say(&format!("     .sh let through: {n}"));
        }
    }
    let _ = fs::remove_dir_all(&e.dir);
}

fn ssh(host: &str, cmd: &str) -> Output {
    Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", host, cmd])
        .output()
        .expect("ssh")
}

#[test]
fn real_vps_checksum_fail() {
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
    let o = ssh(
        &host,
        "sha256sum /opt/galion/rust/bin/galion-restore-test.sh",
    );
    assert!(o.status.success());
    assert!(s(&o.stdout).starts_with(ORACLE_SHA256), "live .sh changed");
    let ts = backup::stamp_from_rfc3339(&backup::now_iso());
    let dir = format!("/var/tmp/restore-test-361-{ts}");
    let o = ssh(
        &host,
        &format!("umask 077 && mkdir {dir} && mkdir {dir}/t {dir}/bin"),
    );
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
    // Part 1: every real archive must still pass (no false positives from
    // the strict checks), direct and via the symlink alias, stdout == .sh.
    // Part 2: tampered COPIES of the newest archive must fail.
    let script = format!(
        r#"set -u; umask 077; cd {dir}; ln -s {dir}/galion-backup {dir}/bin/galion-restore-test.sh
for A in $(ls -1t /var/backups/galion/galion-*.tar.gz); do echo "archive=$(basename $A)";
 /opt/galion/rust/bin/galion-restore-test.sh "$A" > sh.out 2>/dev/null; echo sh_rc=$?;
 GALION_RESTORE_TMP={dir}/t nice -n 10 ./galion-backup restore-test "$A" > rs.out 2> rs.err; echo rs_rc=$?;
 GALION_RESTORE_TMP={dir}/t nice -n 10 {dir}/bin/galion-restore-test.sh "$A" > al.out 2>/dev/null; echo al_rc=$?;
 if cmp -s sh.out rs.out && cmp -s sh.out al.out; then echo STDOUT_IDENTICAL; else echo STDOUT_DIFF; tail -2 rs.err | sed 's|{dir}|<dir>|g'; fi;
 echo t_left=$(ls -A {dir}/t | wc -l); done
N=$(ls -1t /var/backups/galion/galion-*.tar.gz | head -1); echo "tamper_base=$(basename $N)"
mkdir w && tar -C w -xzf "$N" && TOP=$(ls w)
tamper() {{ name=$1; out={dir}/galion-tamper-$name.tar.gz; ( cd w && tar -czf "$out" "$TOP" );
 /opt/galion/rust/bin/galion-restore-test.sh "$out" > sh.out 2>/dev/null; src=$?; shok=$(grep -c 'restore test ok' sh.out);
 GALION_RESTORE_TMP={dir}/t nice -n 10 {dir}/bin/galion-restore-test.sh "$out" > rs.out 2> rs.err; rrc=$?;
 echo "tamper=$name sh_rc=$src sh_says_ok=$shok rs_rc=$rrc rs_ok_lines=$(grep -c 'ok' rs.out) t_left=$(ls -A {dir}/t | wc -l)";
 echo "  rs_msg=$(tail -1 rs.err | sed 's|{dir}|<dir>|g')"; rm -f "$out"; }}
cp -p "w/$TOP/files/files.tar" files.keep; printf '\001' | dd of="w/$TOP/files/files.tar" bs=1 seek=700 conv=notrunc status=none; tamper files_tar_byte; cp -p files.keep "w/$TOP/files/files.tar"; rm -f files.keep
cp -p "w/$TOP/SHA256SUMS" sums.keep; grep -v '  ./db/MAP$' sums.keep > "w/$TOP/SHA256SUMS"; tamper sums_line_dropped; cp -p sums.keep "w/$TOP/SHA256SUMS"
echo x > "w/$TOP/db/notes.txt"; tamper unlisted_file; rm -f "w/$TOP/db/notes.txt"
echo 'garbage' >> "w/$TOP/SHA256SUMS"; tamper malformed_line; cp -p sums.keep "w/$TOP/SHA256SUMS"; rm -f sums.keep
cp "$N" g.tar.gz; S=$(stat -c %s g.tar.gz); printf '\377' | dd of=g.tar.gz bs=1 seek=$((S-6)) conv=notrunc status=none
/opt/galion/rust/bin/galion-restore-test.sh g.tar.gz >/dev/null 2>&1; src=$?; GALION_RESTORE_TMP={dir}/t ./galion-backup restore-test g.tar.gz > /dev/null 2> rs.err; echo "tamper=gzip_crc sh_rc=$src rs_rc=$? t_left=$(ls -A {dir}/t | wc -l)"; echo "  rs_msg=$(tail -1 rs.err | sed 's|{dir}|<dir>|g')"
chmod -R u+w w; cd / && rm -rf {dir} && echo cleaned={dir}; echo tmp_left=$(ls -d /tmp/galion-restore-* 2>/dev/null | wc -l)"#
    );
    let o = ssh(&host, &script);
    let out = s(&o.stdout);
    for l in out.lines() {
        say(&format!("vps | {l}"));
    }
    assert!(o.status.success(), "ssh script: {}", s(&o.stderr));
    let n = out.matches("archive=").count();
    assert!(n >= 1);
    assert_eq!(
        out.matches("rs_rc=0").count(),
        n,
        "every real archive passes rust"
    );
    assert_eq!(
        out.matches("al_rc=0").count(),
        n,
        "every real archive passes via alias"
    );
    assert_eq!(out.matches("STDOUT_IDENTICAL").count(), n);
    let tampers: Vec<&str> = out.lines().filter(|l| l.starts_with("tamper=")).collect();
    assert_eq!(tampers.len(), 5, "{out}");
    for l in &tampers {
        assert!(l.contains("rs_rc=1"), "{l}");
        assert!(l.contains("t_left=0"), "{l}");
        assert!(!l.contains("rs_ok_lines=1"), "{l}");
    }
    assert!(out.contains("cleaned="));
    assert!(out.contains("tmp_left=0"));
}
