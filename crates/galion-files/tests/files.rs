// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-files against a scratch folder: the jail, read-only mode, edits
//! with backups, never-overwrite rules, delete-to-trash, uploads, and
//! zip/tar extraction (including hostile member names). The real RAR test
//! runs when GALION_FILES_SAMPLE_RAR points at a .rar file.

use galion_files::{Error, Root, TRASH};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "galion-files-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(d.join("docs")).unwrap();
    fs::write(d.join("docs/a.md"), "# Title\n\nhello\n").unwrap();
    fs::write(d.join("docs/.env"), "KEY=value\n").unwrap();
    fs::write(d.join("pic.png"), b"\x89PNG\r\n\x1a\n0000").unwrap();
    fs::write(d.join("blob"), [0u8, 1, 2, 3, 0xff]).unwrap();
    d
}

fn outside() -> PathBuf {
    let d = std::env::temp_dir().join(format!("galion-files-outside-{}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("secret.txt"), "outside").unwrap();
    d
}

#[test]
fn traversal_and_symlink_escape_refused() {
    let d = scratch();
    let o = outside();
    std::os::unix::fs::symlink(&o, d.join("escape")).unwrap();
    std::os::unix::fs::symlink(d.join("docs"), d.join("inside-link")).unwrap();
    let r = Root::new(&d, true).unwrap();
    for bad in [
        "../",
        "docs/../../etc/passwd",
        "/../x",
        "escape/secret.txt",
        "escape",
        "docs\\a.md",
    ] {
        let e = r.info(bad).unwrap_err();
        assert!(matches!(e, Error::Bad(_) | Error::NotFound), "{bad}: {e:?}");
    }
    assert!(r.read_text("escape/secret.txt", 1 << 20).is_err());
    assert!(r.write_text("escape/new.txt", "x", None, true).is_err());
    assert!(r.move_to("pic.png", "escape").is_err());
    assert!(r.copy_to("docs", "escape").is_err());
    assert!(r.upload("escape", "x.bin", &b"x"[..], 10).is_err());
    assert!(!o.join("new.txt").exists() && !o.join("x.bin").exists());
    // A link that stays inside works.
    assert!(r.read_text("inside-link/a.md", 1 << 20).is_ok());
    // The listing shows the escaping link as broken, not its target.
    let l = r.list("", true).unwrap();
    let esc = l["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "escape")
        .unwrap();
    assert_eq!(esc["broken"], true);
}

#[test]
fn listing_types_and_hidden() {
    let d = scratch();
    let r = Root::new(&d, false).unwrap();
    let l = r.list("docs", false).unwrap();
    let names: Vec<&str> = l["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["a.md"]);
    let l = r.list("docs", true).unwrap();
    assert_eq!(l["entries"].as_array().unwrap().len(), 2);
    assert_eq!(l["parent"], "");
    let top = r.list("", false).unwrap();
    let first = &top["entries"][0];
    assert_eq!(first["name"], "docs");
    assert_eq!(first["dir"], true);
    assert_eq!(r.info("pic.png").unwrap()["kind"], "image");
    assert_eq!(r.info("docs/.env").unwrap()["lang"], "ini");
    assert_eq!(r.info("blob").unwrap()["preview"], "hex");
    assert_eq!(r.info("docs/a.md").unwrap()["preview"], "markdown");
    let h = r.hex("blob", 0, 64).unwrap();
    assert!(h["dump"]
        .as_str()
        .unwrap()
        .starts_with("00000000  00 01 02 03 ff"));
    assert_eq!(h["more"], false);
}

#[test]
fn listing_has_short_local_times() {
    let d = scratch();
    let r = Root::new(&d, false).unwrap();
    let top = r.list("", false).unwrap();
    for e in top["entries"].as_array().unwrap() {
        let short = e["mtime_short"].as_str().unwrap();
        // Just written: today on the Zagreb clock ("yesterday" only if the
        // test straddles local midnight).
        assert!(
            (short.starts_with("today ") && short.len() == 11) || short.starts_with("yesterday "),
            "{e}"
        );
        let exact = e["mtime_local"].as_str().unwrap();
        assert!(
            exact.ends_with("+01:00") || exact.ends_with("+02:00"),
            "{exact}"
        );
    }
    let i = r.info("docs/a.md").unwrap();
    let s = i["mtime_short"].as_str().unwrap();
    assert!(
        s.starts_with("today ") || s.starts_with("yesterday "),
        "{s}"
    );
    assert_eq!(
        galion_files::short_stamp(1_793_533_200 - 2 * 86400, 1_793_533_200),
        "30 Oct 12:40"
    );
}

#[test]
fn read_only_refuses_every_write() {
    let d = scratch();
    let r = Root::new(&d, false).unwrap();
    assert_eq!(
        r.write_text("docs/a.md", "x", None, false).unwrap_err(),
        Error::ReadOnly
    );
    assert_eq!(
        r.write_text("new.txt", "x", None, true).unwrap_err(),
        Error::ReadOnly
    );
    assert_eq!(r.mkdir("", "x").unwrap_err(), Error::ReadOnly);
    assert_eq!(r.rename("pic.png", "q.png").unwrap_err(), Error::ReadOnly);
    assert_eq!(r.move_to("pic.png", "docs").unwrap_err(), Error::ReadOnly);
    assert_eq!(r.copy_to("pic.png", "docs").unwrap_err(), Error::ReadOnly);
    assert_eq!(r.trash("pic.png").unwrap_err(), Error::ReadOnly);
    assert_eq!(
        r.upload("", "u.bin", &b"x"[..], 10).unwrap_err(),
        Error::ReadOnly
    );
    assert_eq!(
        r.extract("pic.png", None, 1 << 20).unwrap_err(),
        Error::ReadOnly
    );
    assert_eq!(
        fs::read_to_string(d.join("docs/a.md")).unwrap(),
        "# Title\n\nhello\n"
    );
}

#[test]
fn edit_keeps_backup_and_detects_conflicts() {
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    let t = r.read_text("docs/a.md", 1 << 20).unwrap();
    let m = t["version"].as_str().unwrap().to_string();
    let saved = r
        .write_text("docs/a.md", "# New\n", Some(&m), false)
        .unwrap();
    assert_eq!(fs::read_to_string(d.join("docs/a.md")).unwrap(), "# New\n");
    let bak = saved["backup"].as_str().unwrap();
    assert!(
        bak.starts_with(".trash/edits/") && bak.ends_with("-a.md.bak"),
        "{bak}"
    );
    assert_eq!(
        fs::read_to_string(d.join(bak)).unwrap(),
        "# Title\n\nhello\n"
    );
    // The old mtime no longer matches.
    assert_eq!(
        r.write_text("docs/a.md", "x", Some(&m), false).unwrap_err(),
        Error::Conflict
    );
    // New files never overwrite.
    assert!(matches!(
        r.write_text("docs/a.md", "x", None, true).unwrap_err(),
        Error::Exists(_)
    ));
    r.write_text("docs/b.rs", "fn main() {}\n", None, true)
        .unwrap();
    assert_eq!(r.info("docs/b.rs").unwrap()["lang"], "rust");
    // No temp files left behind.
    let leftovers: Vec<_> = fs::read_dir(d.join("docs"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("galion-save"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn rename_move_copy_never_overwrite() {
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    fs::write(d.join("docs/pic.png"), "other").unwrap();
    assert!(matches!(
        r.move_to("pic.png", "docs").unwrap_err(),
        Error::Exists(_)
    ));
    assert!(matches!(
        r.rename("blob", "pic.png").unwrap_err(),
        Error::Exists(_)
    ));
    assert!(r.rename("blob", "a/b").is_err());
    assert_eq!(r.rename("blob", "blob.bin").unwrap()["path"], "blob.bin");
    let c = r.copy_to("pic.png", "docs").unwrap();
    assert_eq!(c["path"], "docs/pic (1).png");
    assert_eq!(fs::read_to_string(d.join("docs/pic.png")).unwrap(), "other");
    assert!(r.move_to("docs", "docs").is_err(), "folder into itself");
    assert!(r.copy_to("docs", "docs").is_err(), "copy into itself");
    let c = r.copy_to("docs", "").unwrap();
    assert_eq!(c["path"], "docs (1)");
    assert!(d.join("docs (1)/a.md").is_file());
    r.mkdir("", "new folder").unwrap();
    assert_eq!(
        r.move_to("pic.png", "new folder").unwrap()["path"],
        "new folder/pic.png"
    );
    assert!(r.rename("", "x").is_err(), "root cannot be renamed");
}

#[test]
fn delete_goes_to_trash_and_nothing_is_removed() {
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    let t = r.trash("docs/a.md").unwrap();
    let p = t["path"].as_str().unwrap();
    assert!(p.starts_with(".trash/") && p.ends_with("-a.md"), "{p}");
    assert!(!d.join("docs/a.md").exists());
    assert_eq!(fs::read_to_string(d.join(p)).unwrap(), "# Title\n\nhello\n");
    // Trashing again from the trash is refused; the trash itself cannot move.
    assert!(r.trash(p).is_err());
    assert!(r.trash(TRASH).is_err());
    assert!(r.rename(TRASH, "x").is_err());
    // Nothing new can be written into the trash directly.
    assert!(r.write_text(".trash/x.txt", "x", None, true).is_err());
    assert!(r.upload(".trash", "x", &b"x"[..], 10).is_err());
    // Restore = move back out.
    assert_eq!(r.move_to(p, "docs").unwrap()["from"], p);
    // Whole folders go too.
    r.trash("docs").unwrap();
    assert!(!d.join("docs").exists());
    assert_eq!(fs::read_dir(d.join(TRASH)).unwrap().count(), 1);
}

#[test]
fn upload_streams_unique_names_and_cap_keeps_off_file() {
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    let u = r.upload("docs", "a.md", &b"uploaded"[..], 100).unwrap();
    assert_eq!(u["path"], "docs/a (1).md");
    assert_eq!(
        fs::read_to_string(d.join("docs/a (1).md")).unwrap(),
        "uploaded"
    );
    let big = vec![7u8; 1000];
    assert!(matches!(
        r.upload("docs", "big.bin", &big[..], 100).unwrap_err(),
        Error::TooLarge(_)
    ));
    assert!(!d.join("docs/big.bin").exists());
    let off: Vec<_> = fs::read_dir(d.join("docs"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".off"))
        .collect();
    assert_eq!(
        off.len(),
        1,
        "the partial upload is kept as .off, not deleted"
    );
}

/// A zip with deflated members, written by hand (local headers, central
/// directory, end record) so the tests need no zip writer with deflate.
fn make_zip(p: &Path, entries: &[(&str, &[u8])]) {
    fs::write(p, deflated_zip(entries)).unwrap();
}

fn deflated_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in entries {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = galion_files::inflate::crc32(data);
        let off = out.len() as u32;
        le32(&mut out, 0x0403_4b50);
        for x in [20u16, 0, 8, 0, 0x21] {
            le16(&mut out, x);
        }
        for x in [crc, packed.len() as u32, data.len() as u32] {
            le32(&mut out, x);
        }
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);
        le32(&mut cd, 0x0201_4b50);
        for x in [20u16, 20, 0, 8, 0, 0x21] {
            le16(&mut cd, x);
        }
        for x in [crc, packed.len() as u32, data.len() as u32] {
            le32(&mut cd, x);
        }
        for x in [name.len() as u16, 0, 0, 0, 0] {
            le16(&mut cd, x);
        }
        le32(&mut cd, 0);
        le32(&mut cd, off);
        cd.extend_from_slice(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&cd);
    le32(&mut out, 0x0605_4b50);
    for x in [0u16, 0, entries.len() as u16, entries.len() as u16] {
        le16(&mut out, x);
    }
    le32(&mut out, cd.len() as u32);
    le32(&mut out, cd_off);
    le16(&mut out, 0);
    out
}

#[test]
fn zip_stored_members_and_crc_check() {
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file("s/plain.txt", o).unwrap();
    z.write_all(b"stored bytes").unwrap();
    fs::write(d.join("stored.zip"), z.finish().unwrap().into_inner()).unwrap();
    let x = r.extract("stored.zip", None, 1 << 20).unwrap();
    assert_eq!(
        fs::read_to_string(d.join("stored/s/plain.txt")).unwrap(),
        "stored bytes"
    );
    assert_eq!(x["files"], 1);
    // Flip one packed byte: the CRC check refuses, the staging is kept as .off.
    let mut bad = deflated_zip(&[("a.txt", &b"hello hello hello hello"[..])]);
    let at = 30 + "a.txt".len() + 2;
    bad[at] ^= 0x55;
    fs::write(d.join("bad.zip"), bad).unwrap();
    assert!(r.extract("bad.zip", None, 1 << 20).is_err());
    assert!(!d.join("bad").exists());
}

#[test]
fn zip_list_and_extract_with_hostile_names() {
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    make_zip(
        &d.join("bundle.zip"),
        &[
            ("ok/one.txt", b"one"),
            ("two.txt", b"two"),
            ("../evil.txt", b"evil"),
            ("ok/../../evil2.txt", b"evil"),
        ],
    );
    let l = r.archive_list("bundle.zip").unwrap();
    assert_eq!(l["format"], "zip");
    assert_eq!(l["count"], 4);
    assert_eq!(l["members"][2]["safe"], false);
    let x = r.extract("bundle.zip", None, 1 << 20).unwrap();
    assert_eq!(x["path"], "bundle");
    assert_eq!(x["files"], 2);
    assert_eq!(x["skipped"].as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(d.join("bundle/ok/one.txt")).unwrap(),
        "one"
    );
    assert!(!d.join("evil.txt").exists() && !d.parent().unwrap().join("evil.txt").exists());
    // A second extract never overwrites: new folder.
    assert_eq!(
        r.extract("bundle.zip", None, 1 << 20).unwrap()["path"],
        "bundle (1)"
    );
    // Over the cap: refused, staging kept as .off, no final folder.
    make_zip(&d.join("bomb.zip"), &[("zeros.bin", &vec![0u8; 200_000])]);
    assert!(matches!(
        r.extract("bomb.zip", None, 10_000).unwrap_err(),
        Error::TooLarge(_)
    ));
    assert!(!d.join("bomb").exists());
    // Not an archive.
    assert!(matches!(
        r.archive_list("pic.png").unwrap_err(),
        Error::Unsupported(_)
    ));
}

#[test]
fn tar_and_tar_gz_extract() {
    use galion_tar::ustar::{finish, write_entry, Entry, Kind};
    let d = scratch();
    let r = Root::new(&d, true).unwrap();
    let mut buf = Vec::new();
    let e = |name: &str, data: &[u8]| Entry {
        kind: Kind::File,
        name: name.into(),
        mode: 0o644,
        uid: 0,
        gid: 0,
        mtime: 0,
        link: String::new(),
        data: data.to_vec(),
    };
    write_entry(&mut buf, &e("t/a.txt", b"tar-a")).unwrap();
    write_entry(
        &mut buf,
        &Entry {
            kind: Kind::Symlink,
            link: "/etc/passwd".into(),
            ..e("t/link", b"")
        },
    )
    .unwrap();
    finish(&mut buf).unwrap();
    fs::write(d.join("pack.tar"), &buf).unwrap();
    let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    gz.extend(miniz_oxide::deflate::compress_to_vec(&buf, 6));
    gz.extend(galion_files::inflate::crc32(&buf).to_le_bytes());
    gz.extend((buf.len() as u32).to_le_bytes());
    fs::write(d.join("pack2.tar.gz"), gz).unwrap();
    assert_eq!(r.archive_list("pack.tar").unwrap()["count"], 2);
    assert_eq!(r.archive_list("pack2.tar.gz").unwrap()["format"], "tar.gz");
    let x = r.extract("pack2.tar.gz", None, 1 << 20).unwrap();
    assert_eq!(x["path"], "pack2");
    assert_eq!(
        fs::read_to_string(d.join("pack2/t/a.txt")).unwrap(),
        "tar-a"
    );
    assert!(
        fs::symlink_metadata(d.join("pack2/t/link")).is_err(),
        "links are skipped"
    );
}

/// GALION_FILES_SAMPLE_ZIP=/path/to.zip: a real (deflated) zip extracts and
/// every member passes its CRC-32.
#[test]
fn real_zip_sample() {
    let Ok(zp) = std::env::var("GALION_FILES_SAMPLE_ZIP") else {
        eprintln!("skip: GALION_FILES_SAMPLE_ZIP not set");
        return;
    };
    let d = scratch();
    fs::copy(&zp, d.join("sample.zip")).unwrap();
    let r = Root::new(&d, true).unwrap();
    let l = r.archive_list("sample.zip").unwrap();
    let x = r.extract("sample.zip", None, 8 << 30).unwrap();
    let files = l["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["dir"] == false)
        .count() as u64;
    assert_eq!(x["files"].as_u64().unwrap(), files, "{x}");
}

/// GALION_FILES_SAMPLE_RAR=/path/to.rar: list and extract a real RAR5 into
/// a scratch root and compare every member with the folder next to it
/// (GALION_FILES_SAMPLE_DIR), byte for byte.
#[test]
fn real_rar_sample() {
    let Ok(rar) = std::env::var("GALION_FILES_SAMPLE_RAR") else {
        eprintln!("skip: GALION_FILES_SAMPLE_RAR not set");
        return;
    };
    let d = scratch();
    let name = Path::new(&rar)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    std::os::unix::fs::symlink(&rar, d.join("sample-link.rar")).unwrap_or(());
    // Copy instead of link: the jail refuses links that leave the root.
    fs::copy(&rar, d.join(&name)).unwrap();
    let r = Root::new(&d, true).unwrap();
    assert!(
        r.archive_list("sample-link.rar").is_err(),
        "a link out of the root is refused"
    );
    let l = r.archive_list(&name).unwrap();
    assert_eq!(l["format"], "rar");
    let n = l["count"].as_u64().unwrap();
    assert!(n > 0);
    let x = r.extract(&name, None, 8 << 30).unwrap();
    assert_eq!(x["files"].as_u64().unwrap(), n);
    if let Ok(dir) = std::env::var("GALION_FILES_SAMPLE_DIR") {
        let out = d.join(x["path"].as_str().unwrap());
        for m in l["members"].as_array().unwrap() {
            let m = m["name"].as_str().unwrap();
            let a = fs::read(out.join(m)).unwrap();
            let b = fs::read(Path::new(&dir).join(m)).unwrap();
            assert!(a == b, "{m} differs from the sample folder");
        }
    }
}
