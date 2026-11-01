// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `Root::with_owner` under the live gateway's `UMask=0077`: every entry a
//! write makes or moves gets the owner, files 0644 (0755 if executable) and
//! folders 0755; links are never followed (an outside file keeps its mode);
//! without an owner nothing changes. As a normal user the owner is the test
//! user itself (chown to self); run as root the owner is 65534:65534, which
//! is the live setup (the VPS harness run).

use galion_files::{owner, Root, TRASH};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

/// The live gateway umask, for the whole test process.
fn umask_0077() {
    // SAFETY: umask(2) only sets the process file-mode mask; no memory.
    unsafe {
        libc::umask(0o077);
    }
}

fn scratch(tag: &str) -> PathBuf {
    umask_0077();
    let d = std::env::temp_dir().join(format!(
        "galion-files-owner-{}-{tag}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&d).unwrap();
    fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
    d.canonicalize().unwrap()
}

/// (uid, gid) files are handed to: 65534:65534 as root, else ourselves.
fn want(d: &Path) -> (u32, u32) {
    let m = fs::metadata(d).unwrap();
    if m.uid() == 0 {
        (65534, 65534)
    } else {
        (m.uid(), m.gid())
    }
}

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().mode() & 0o7777
}

fn owned(p: &Path, w: (u32, u32)) -> bool {
    let m = fs::symlink_metadata(p).unwrap();
    (m.uid(), m.gid()) == w
}

fn check(p: &Path, w: (u32, u32), m: u32) {
    assert_eq!(mode(p), m, "{} mode {:o}", p.display(), mode(p));
    assert!(owned(p, w), "{} owner", p.display());
}

#[test]
fn file_mode_keeps_only_exec_bits() {
    assert_eq!(owner::file_mode(0o600), 0o644);
    assert_eq!(owner::file_mode(0o700), 0o755);
    assert_eq!(owner::file_mode(0o4777), 0o755);
    assert_eq!(owner::file_mode(0o666), 0o644);
    assert_eq!(owner::file_mode(0o744), 0o755);
}

#[test]
fn without_owner_nothing_changes() {
    let d = scratch("none");
    let r = Root::new(&d, true).unwrap();
    assert_eq!(r.owner(), None);
    r.write_text("a.txt", "hi", None, true).unwrap();
    r.mkdir("", "dir").unwrap();
    // Today's behaviour under UMask=0077: root-only.
    assert_eq!(mode(&d.join("a.txt")), 0o600);
    assert_eq!(mode(&d.join("dir")), 0o700);
}

#[test]
fn write_mkdir_edit_upload_get_owner_and_modes() {
    let d = scratch("write");
    let w = want(&d);
    let r = Root::new(&d, true).unwrap().with_owner(Some(w));
    assert_eq!(r.owner(), Some(w));
    let v = r.write_text("a.txt", "hi", None, true).unwrap();
    assert!(v.get("owner_failed").is_none(), "{v}");
    check(&d.join("a.txt"), w, 0o644);
    r.mkdir("", "dir").unwrap();
    check(&d.join("dir"), w, 0o755);
    r.write_text("dir/b.md", "# b", None, true).unwrap();
    check(&d.join("dir/b.md"), w, 0o644);
    // Edit a script someone made 0700: the new copy is 0755 (exec kept) and
    // the backup plus .trash/edits are handed over too.
    fs::write(d.join("run.sh"), "echo 1\n").unwrap();
    fs::set_permissions(d.join("run.sh"), fs::Permissions::from_mode(0o700)).unwrap();
    let ver = galion_files::version_of(&d.join("run.sh")).unwrap();
    let v = r
        .write_text("run.sh", "echo 2\n", Some(&ver), false)
        .unwrap();
    assert!(v.get("owner_failed").is_none(), "{v}");
    check(&d.join("run.sh"), w, 0o755);
    check(&d.join(TRASH), w, 0o755);
    check(&d.join(TRASH).join("edits"), w, 0o755);
    let bak = d.join(v["backup"].as_str().unwrap());
    check(&bak, w, 0o755);
    // Upload through the hidden temp file.
    let v = r.upload("dir", "up.bin", &b"12345"[..], 1 << 20).unwrap();
    check(&d.join(v["path"].as_str().unwrap()), w, 0o644);
}

#[test]
fn copy_move_rename_trash_extract_hand_over_whole_trees() {
    let d = scratch("tree");
    let w = want(&d);
    let r = Root::new(&d, true).unwrap().with_owner(Some(w));
    // A tree made the old way (root-only, as the live gateway left them).
    fs::create_dir_all(d.join("old/sub")).unwrap();
    fs::write(d.join("old/sub/x.json"), "{}").unwrap();
    fs::write(d.join("old/y.md"), "y").unwrap();
    fs::create_dir(d.join("dest")).unwrap();
    assert_eq!(mode(&d.join("old/sub")), 0o700);
    assert_eq!(mode(&d.join("old/sub/x.json")), 0o600);
    // Copy: every new entry handed over.
    r.copy_to("old", "dest").unwrap();
    for (p, m) in [
        ("dest/old", 0o755),
        ("dest/old/sub", 0o755),
        ("dest/old/sub/x.json", 0o644),
        ("dest/old/y.md", 0o644),
    ] {
        check(&d.join(p), w, m);
    }
    // Move: the moved tree too.
    fs::create_dir(d.join("dest2")).unwrap();
    r.move_to("old", "dest2").unwrap();
    check(&d.join("dest2/old/sub"), w, 0o755);
    check(&d.join("dest2/old/sub/x.json"), w, 0o644);
    // Rename in place.
    fs::write(d.join("r.txt"), "r").unwrap();
    r.rename("r.txt", "r2.txt").unwrap();
    check(&d.join("r2.txt"), w, 0o644);
    // Trash.
    fs::write(d.join("t.txt"), "t").unwrap();
    let v = r.trash("t.txt").unwrap();
    check(&d.join(v["path"].as_str().unwrap()), w, 0o644);
    check(&d.join(TRASH), w, 0o755);
    // Extract: members and folders.
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file("s/plain.txt", o).unwrap();
    z.write_all(b"stored bytes").unwrap();
    fs::write(d.join("pack.zip"), z.finish().unwrap().into_inner()).unwrap();
    let v = r.extract("pack.zip", None, 1 << 20).unwrap();
    assert!(v.get("owner_failed").is_none(), "{v}");
    check(&d.join("pack"), w, 0o755);
    check(&d.join("pack/s"), w, 0o755);
    check(&d.join("pack/s/plain.txt"), w, 0o644);
}

#[test]
fn links_are_never_followed() {
    let d = scratch("links");
    let out = scratch("outside");
    let w = want(&d);
    let r = Root::new(&d, true).unwrap().with_owner(Some(w));
    fs::write(out.join("secret.txt"), "outside").unwrap();
    fs::set_permissions(out.join("secret.txt"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).unwrap();
    let before = fs::metadata(out.join("secret.txt")).unwrap();
    // A folder holding links to an outside file and an outside folder.
    fs::create_dir(d.join("l")).unwrap();
    symlink(out.join("secret.txt"), d.join("l/to-file")).unwrap();
    symlink(&out, d.join("l/to-dir")).unwrap();
    fs::create_dir(d.join("into")).unwrap();
    r.move_to("l", "into").unwrap();
    r.rename("into/l", "l2").unwrap();
    r.copy_to("into/l2", "").unwrap();
    // A top-level link moved as itself.
    symlink(out.join("secret.txt"), d.join("top-link")).unwrap();
    r.move_to("top-link", "into").unwrap();
    let after = fs::metadata(out.join("secret.txt")).unwrap();
    assert_eq!(after.mode() & 0o7777, 0o600, "outside file mode changed");
    assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
    assert_eq!(mode(&out), 0o700, "outside folder mode changed");
    // The links themselves are still links (never replaced or chmodded).
    assert!(fs::symlink_metadata(d.join("into/l2/to-file"))
        .unwrap()
        .file_type()
        .is_symlink());
    check(&d.join("into/l2"), w, 0o755);
}

/// As root (the VPS harness run): other users' files are left alone, and
/// new entries really belong to 65534:65534.
#[test]
fn root_hands_to_65534_and_skips_foreign_files() {
    let d = scratch("root");
    if fs::metadata(&d).unwrap().uid() != 0 {
        eprintln!("not root: 65534 hand-over checked on the VPS run");
        return;
    }
    let r = Root::new(&d, true)
        .unwrap()
        .with_owner(Some((65534, 65534)));
    r.write_text("n.txt", "n", None, true).unwrap();
    check(&d.join("n.txt"), (65534, 65534), 0o644);
    fs::create_dir(d.join("mix")).unwrap();
    fs::write(d.join("mix/theirs.txt"), "1000").unwrap();
    std::os::unix::fs::chown(d.join("mix/theirs.txt"), Some(1000), Some(1000)).unwrap();
    fs::create_dir(d.join("to")).unwrap();
    r.move_to("mix", "to").unwrap();
    check(&d.join("to/mix"), (65534, 65534), 0o755);
    let m = fs::metadata(d.join("to/mix/theirs.txt")).unwrap();
    assert_eq!(
        (m.uid(), m.mode() & 0o777),
        (1000, 0o600),
        "foreign file changed"
    );
}
