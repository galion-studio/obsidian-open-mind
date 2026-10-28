// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! galion-tar against GNU tar on a scratch tree.
//!
//! * always: the GNU writer's archive is read back by [`Reader`] and every
//!   member matches the filesystem (types, modes, sizes, data, links).
//! * `GALION_REAL=1`: the real `/usr/bin/tar` (GNU tar 1.35, the same version
//!   as the VPS) archives the same tree; its bytes must equal ours, [`Reader`]
//!   must read its archive, and `tar -tvf` must list galion-tar's ustar output.
//!
//! `GALION_REAL=1 cargo test -p galion-tar --test gnu_oracle -- --nocapture`

use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use galion_tar::gnu::TarWriter;
use galion_tar::read::{read_all, MemberKind};
use galion_tar::ustar;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("galion-tar-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A tree with every member type the backup sees, names over 100 bytes and
/// sizes around the block and record boundaries.
fn make_tree(base: &Path) -> PathBuf {
    let top = base.join("top");
    let deep = top.join("d".repeat(60)).join("e".repeat(60));
    fs::create_dir_all(&deep).unwrap();
    fs::create_dir_all(top.join("empty-dir")).unwrap();
    fs::write(top.join("empty"), b"").unwrap();
    fs::write(top.join("block"), vec![b'b'; 512]).unwrap();
    fs::write(top.join("record"), vec![b'r'; 10241]).unwrap();
    fs::write(deep.join("long-name.json"), b"{\"ok\":true}\n").unwrap();
    let secret = top.join("mode600");
    fs::write(&secret, b"x").unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(top.join("empty-dir"), fs::Permissions::from_mode(0o750)).unwrap();
    symlink("block", top.join("short-link")).unwrap();
    symlink("t".repeat(150), top.join("long-link")).unwrap();
    fs::hard_link(top.join("block"), top.join("hard")).unwrap();
    let fifo = Command::new("mkfifo").arg(top.join("fifo")).status();
    assert!(fifo.map(|s| s.success()).unwrap_or(false), "mkfifo");
    top
}

fn ours(base: &Path) -> Vec<u8> {
    let mut t = TarWriter::new(Vec::new());
    t.add_tree(&base.join("top"), b"top", &|_| false).unwrap();
    t.finish().unwrap()
}

#[test]
fn gnu_writer_reads_back_like_the_filesystem() {
    let base = scratch("rb");
    make_tree(&base);
    let bytes = ours(&base);
    assert_eq!(bytes.len() % 10240, 0);
    let all = read_all(bytes.as_slice()).unwrap();
    let mut kinds = std::collections::BTreeMap::new();
    for (m, data) in &all {
        *kinds.entry(format!("{:?}", m.kind)).or_insert(0) += 1;
        let p = base.join(String::from_utf8(m.name.clone()).unwrap());
        let md = fs::symlink_metadata(&p).unwrap();
        assert_eq!(m.mode, md.mode() & 0o7777, "{}", m.name_lossy());
        assert_eq!(m.mtime, md.mtime());
        match m.kind {
            MemberKind::File => assert_eq!(data, &fs::read(&p).unwrap()),
            MemberKind::Symlink => assert_eq!(
                m.link,
                fs::read_link(&p)
                    .unwrap()
                    .into_os_string()
                    .into_encoded_bytes()
            ),
            MemberKind::Hardlink => {
                let first = base.join(String::from_utf8(m.link.clone()).unwrap());
                assert_eq!(fs::metadata(first).unwrap().ino(), md.ino());
                assert!(data.is_empty());
            }
            MemberKind::Dir | MemberKind::Fifo => assert!(data.is_empty()),
            k => panic!("unexpected {k:?}"),
        }
    }
    assert_eq!(kinds["File"], 5);
    assert_eq!(kinds["Dir"], 4);
    assert_eq!(kinds["Symlink"], 2);
    assert_eq!(kinds["Hardlink"], 1);
    assert_eq!(kinds["Fifo"], 1);
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn real_gnu_tar_oracle() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let v = Command::new("tar").arg("--version").output().expect("tar");
    let v = String::from_utf8_lossy(&v.stdout);
    assert!(v.starts_with("tar (GNU tar)"), "need GNU tar, got {v}");
    println!("oracle: {}", v.lines().next().unwrap());
    let base = scratch("oracle");
    make_tree(&base);

    // 1. Byte identity with GNU tar's default (gnu) format, readdir order.
    let gnu = Command::new("tar")
        .args(["-c", "-f", "-", "-C"])
        .arg(&base)
        .arg("top")
        .output()
        .unwrap();
    assert!(
        gnu.status.success(),
        "{}",
        String::from_utf8_lossy(&gnu.stderr)
    );
    let mine = ours(&base);
    println!(
        "gnu tar {} bytes, galion-tar {} bytes, identical: {}",
        gnu.stdout.len(),
        mine.len(),
        gnu.stdout == mine
    );
    assert!(gnu.stdout == mine, "galion-tar differs from GNU tar");

    // 2. The reader reads GNU tar's own archive, member for member.
    let theirs = read_all(gnu.stdout.as_slice()).unwrap();
    let back = read_all(mine.as_slice()).unwrap();
    assert_eq!(theirs, back);
    println!("reader: {} members from GNU tar's archive", theirs.len());

    // 3. GNU tar lists and extracts galion-tar's ustar output (deploy format).
    let mut u = vec![];
    for (kind, name, data) in [
        (ustar::Kind::Dir, "opt/galion/rust/bin", &b""[..]),
        (
            ustar::Kind::File,
            "opt/galion/rust/bin/galion-nerve",
            b"\x7fELF fake",
        ),
        (ustar::Kind::Symlink, "opt/galion/current", b""),
    ] {
        ustar::write_entry(
            &mut u,
            &ustar::Entry {
                kind,
                name: name.into(),
                mode: if kind == ustar::Kind::File {
                    0o755
                } else {
                    0o750
                },
                uid: 0,
                gid: 0,
                mtime: 1_760_000_000,
                link: if kind == ustar::Kind::Symlink {
                    "rust".into()
                } else {
                    String::new()
                },
                data: data.to_vec(),
            },
        )
        .unwrap();
    }
    ustar::finish(&mut u).unwrap();
    let up = base.join("u.tar");
    fs::File::create(&up).unwrap().write_all(&u).unwrap();
    let list = Command::new("tar").arg("-tvf").arg(&up).output().unwrap();
    assert!(list.status.success());
    let list = String::from_utf8_lossy(&list.stdout);
    println!("tar -tvf ustar:\n{list}");
    assert!(list.contains("opt/galion/rust/bin/galion-nerve"));
    assert!(list.contains("opt/galion/current -> rust"));
    let x = base.join("x");
    fs::create_dir(&x).unwrap();
    let st = Command::new("tar")
        .arg("-xf")
        .arg(&up)
        .arg("-C")
        .arg(&x)
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(
        fs::read(x.join("opt/galion/rust/bin/galion-nerve")).unwrap(),
        b"\x7fELF fake"
    );
    fs::remove_dir_all(&base).unwrap();
}
