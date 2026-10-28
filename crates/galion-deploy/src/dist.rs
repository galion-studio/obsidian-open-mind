// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-deploy dist-install SRC DEST`: install a static tree (the console
//! `console-dist`, slot 379 / D-356-3) with fixed modes, whatever modes the
//! source or the unpacker produced.
//!
//! Live finding (2026-10-28): `/opt/galion/console-dist` holds 52 files 666 and
//! 6 entries 777 (unpacked without a umask), so any local uid could change what
//! the live console serves. This step never copies a source mode:
//!
//! * directories 0755, files 0644, files with any execute bit 0755;
//!   set-uid/set-gid/sticky are dropped;
//! * relative symlinks that stay inside the tree are kept, absolute or escaping
//!   ones are refused; devices, FIFOs and sockets are refused;
//! * the tree is built next to DEST (`DEST.new-<pid>`), checked with
//!   [`check_tree`], then swapped in with two renames: `DEST -> DEST.old-<stamp>`,
//!   `DEST.new-<pid> -> DEST`. Rollback is the reverse rename
//!   (`dist-rollback DEST DEST.old-<stamp>`). The gap between the two renames is
//!   a few microseconds; a request in it gets a 404, never a mixed tree.

use crate::files::sync_dir;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// Counts for one install.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DistReport {
    pub dirs: usize,
    pub files: usize,
    pub links: usize,
    pub bytes: u64,
    /// source entries whose mode was group/other-writable or set-id (fixed)
    pub fixed: usize,
}

/// Installed mode for a source mode.
pub fn mode_for(src_mode: u32, is_dir: bool) -> u32 {
    if is_dir || src_mode & 0o111 != 0 {
        0o755
    } else {
        0o644
    }
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// Does a relative link target at `depth` (dirs below the tree root) stay inside?
fn link_stays_inside(target: &Path, depth: usize) -> bool {
    let mut d = depth as isize;
    for c in target.components() {
        match c {
            Component::Normal(_) => d += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                d -= 1;
                if d < 0 {
                    return false;
                }
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

fn copy_dir(src: &Path, dst: &Path, depth: usize, r: &mut DistReport) -> io::Result<()> {
    fs::create_dir(dst)?;
    fs::set_permissions(dst, fs::Permissions::from_mode(0o755))?;
    r.dirs += 1;
    let mut names: Vec<_> = fs::read_dir(src)?.collect::<Result<_, _>>()?;
    names.sort_by_key(|e| e.file_name());
    for e in names {
        let from = e.path();
        let to = dst.join(e.file_name());
        let md = fs::symlink_metadata(&from)?;
        let ft = md.file_type();
        let mode = md.permissions().mode() & 0o7777;
        if mode & 0o7022 != 0 && !ft.is_symlink() {
            r.fixed += 1;
        }
        if ft.is_dir() {
            copy_dir(&from, &to, depth + 1, r)?;
        } else if ft.is_file() {
            let want = mode_for(mode, false);
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(want)
                .open(&to)?;
            r.bytes += io::copy(&mut File::open(&from)?, &mut out)?;
            out.set_permissions(fs::Permissions::from_mode(want))?;
            out.sync_all()?;
            r.files += 1;
        } else if ft.is_symlink() {
            let t = fs::read_link(&from)?;
            if !link_stays_inside(&t, depth) {
                return Err(bad(format!(
                    "symlink {} -> {} leaves the tree (refused)",
                    from.display(),
                    t.display()
                )));
            }
            std::os::unix::fs::symlink(&t, &to)?;
            r.links += 1;
        } else {
            return Err(bad(format!(
                "{} is not a file, directory or symlink (refused)",
                from.display()
            )));
        }
    }
    Ok(())
}

/// Copy `src` to a new directory `dst` (must not exist) with fixed modes.
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<DistReport> {
    if !fs::symlink_metadata(src)?.is_dir() {
        return Err(bad(format!("{} is not a directory", src.display())));
    }
    let mut r = DistReport::default();
    copy_dir(src, dst, 0, &mut r)?;
    Ok(r)
}

/// Entries of an installed tree that others could change: group/other
/// writable or set-id (symlinks are skipped; their mode is meaningless).
pub fn check_tree(dir: &Path) -> io::Result<Vec<(PathBuf, u32)>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let md = fs::symlink_metadata(&d)?;
        let m = md.permissions().mode() & 0o7777;
        if m & 0o7022 != 0 {
            out.push((d.clone(), m));
        }
        for e in fs::read_dir(&d)? {
            let p = e?.path();
            let md = fs::symlink_metadata(&p)?;
            if md.is_dir() {
                stack.push(p);
            } else if !md.file_type().is_symlink() {
                let m = md.permissions().mode() & 0o7777;
                if m & 0o7022 != 0 {
                    out.push((p, m));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn suffixed(p: &Path, suffix: &str) -> PathBuf {
    crate::files::sibling(p, suffix)
}

/// Result of [`install`].
#[derive(Debug)]
pub struct Installed {
    pub report: DistReport,
    /// where the previous tree went (rollback source), if there was one
    pub old: Option<PathBuf>,
}

/// Build `DEST.new-<pid>`, check it, swap it in. `stamp` names the old tree.
pub fn install(src: &Path, dest: &Path, stamp: &str) -> io::Result<Installed> {
    let src_c = fs::canonicalize(src)?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let dest_c = fs::canonicalize(parent)?.join(
        dest.file_name()
            .ok_or_else(|| bad("DEST has no name".into()))?,
    );
    if dest_c.starts_with(&src_c) || src_c.starts_with(&dest_c) {
        return Err(bad("SRC and DEST must not contain each other".into()));
    }
    let new = suffixed(dest, &format!(".new-{}", std::process::id()));
    if fs::symlink_metadata(&new).is_ok() {
        fs::remove_dir_all(&new)?;
    }
    let report = match copy_tree(src, &new) {
        Ok(r) => r,
        Err(e) => {
            let _ = fs::remove_dir_all(&new);
            return Err(e);
        }
    };
    let left = check_tree(&new)?;
    if let Some((p, m)) = left.first() {
        let _ = fs::remove_dir_all(&new);
        return Err(bad(format!(
            "built tree still has mode {m:o} at {}",
            p.display()
        )));
    }
    let old = suffixed(dest, &format!(".old-{stamp}"));
    let had_old = match fs::symlink_metadata(dest) {
        Ok(md) if md.is_dir() => {
            if fs::symlink_metadata(&old).is_ok() {
                let _ = fs::remove_dir_all(&new);
                return Err(bad(format!("{} already exists", old.display())));
            }
            fs::rename(dest, &old)?;
            true
        }
        Ok(_) => {
            let _ = fs::remove_dir_all(&new);
            return Err(bad(format!(
                "{} exists and is not a directory",
                dest.display()
            )));
        }
        Err(_) => false,
    };
    if let Err(e) = fs::rename(&new, dest) {
        if had_old {
            let _ = fs::rename(&old, dest);
        }
        let _ = fs::remove_dir_all(&new);
        return Err(e);
    }
    sync_dir(dest);
    Ok(Installed {
        report,
        old: had_old.then_some(old),
    })
}

/// Put `old` back at `dest`; the replaced tree moves to `DEST.failed-<stamp>`.
pub fn rollback(dest: &Path, old: &Path, stamp: &str) -> io::Result<PathBuf> {
    if !fs::symlink_metadata(old)?.is_dir() {
        return Err(bad(format!("{} is not a directory", old.display())));
    }
    let failed = suffixed(dest, &format!(".failed-{stamp}"));
    if fs::symlink_metadata(dest).is_ok() {
        fs::rename(dest, &failed)?;
    }
    if let Err(e) = fs::rename(old, dest) {
        let _ = fs::rename(&failed, dest);
        return Err(e);
    }
    sync_dir(dest);
    Ok(failed)
}

const USAGE: &str = "usage: galion-deploy dist-install SRC DEST
       galion-deploy dist-check DIR
       galion-deploy dist-rollback DEST OLD
  dist-install   copy SRC to DEST with dirs 755, files 644 (755 if executable),
                 swap it in; the previous DEST is kept as DEST.old-<stamp>
  dist-check     list group/other-writable or set-id entries (exit 1 if any)
  dist-rollback  move OLD back to DEST (current DEST -> DEST.failed-<stamp>)";

fn stamp() -> String {
    let secs = crate::zagreb::now_ms() / 1000;
    crate::zagreb::compact(secs)
}

/// CLI entry: `args` start after the subcommand name.
pub fn main(cmd: &str, args: &[String]) -> i32 {
    let p = |i: usize| args.get(i).map(PathBuf::from);
    match (cmd, args.len()) {
        ("dist-install", 2) => {
            let (src, dest) = (p(0).unwrap(), p(1).unwrap());
            match install(&src, &dest, &stamp()) {
                Ok(i) => {
                    let r = &i.report;
                    println!(
                        "{} dist-install {} -> {}: {} dirs, {} files, {} links, {} bytes, {} source modes fixed; old {}",
                        crate::zagreb::now(),
                        src.display(),
                        dest.display(),
                        r.dirs,
                        r.files,
                        r.links,
                        r.bytes,
                        r.fixed,
                        i.old.as_ref().map(|o| o.display().to_string()).unwrap_or_else(|| "none".into())
                    );
                    0
                }
                Err(e) => {
                    eprintln!("galion-deploy dist-install: {e}");
                    1
                }
            }
        }
        ("dist-check", 1) => match check_tree(&p(0).unwrap()) {
            Ok(v) => {
                for (path, m) in &v {
                    println!("{m:o} {}", path.display());
                }
                println!(
                    "{} dist-check: {} writable/set-id entries",
                    crate::zagreb::now(),
                    v.len()
                );
                i32::from(!v.is_empty())
            }
            Err(e) => {
                eprintln!("galion-deploy dist-check: {e}");
                2
            }
        },
        ("dist-rollback", 2) => match rollback(&p(0).unwrap(), &p(1).unwrap(), &stamp()) {
            Ok(f) => {
                println!(
                    "{} dist-rollback: restored; replaced tree at {}",
                    crate::zagreb::now(),
                    f.display()
                );
                0
            }
            Err(e) => {
                eprintln!("galion-deploy dist-rollback: {e}");
                1
            }
        },
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gd-dist-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn set(p: &Path, m: u32) {
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }

    fn mode(p: &Path) -> u32 {
        fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
    }

    /// The live console-dist shape: 666 files, 777 dirs, a 777 script.
    fn live_shape(root: &Path) {
        fs::create_dir_all(root.join("static/__grok")).unwrap();
        fs::create_dir_all(root.join("node_modules/srvx/bin")).unwrap();
        fs::write(root.join("static/favicon.svg"), "<svg/>").unwrap();
        fs::write(root.join("static/__grok/styles.css"), "a{}").unwrap();
        fs::write(root.join("node_modules/srvx/bin/srvx.mjs"), "x").unwrap();
        set(&root.join("static/favicon.svg"), 0o666);
        set(&root.join("static/__grok/styles.css"), 0o666);
        set(&root.join("node_modules/srvx/bin/srvx.mjs"), 0o777);
        set(&root.join("node_modules/srvx/bin"), 0o777);
        set(&root.join("node_modules/srvx"), 0o777);
        set(&root.join("static/__grok"), 0o2777);
        std::os::unix::fs::symlink("favicon.svg", root.join("static/icon.svg")).unwrap();
    }

    #[test]
    fn modes() {
        assert_eq!(mode_for(0o666, false), 0o644);
        assert_eq!(mode_for(0o600, false), 0o644);
        assert_eq!(mode_for(0o777, false), 0o755);
        assert_eq!(mode_for(0o4755, false), 0o755);
        assert_eq!(mode_for(0o700, true), 0o755);
        assert_eq!(mode_for(0o1777, true), 0o755);
    }

    #[test]
    fn link_scope() {
        assert!(link_stays_inside(Path::new("a/b"), 0));
        assert!(link_stays_inside(Path::new("../x"), 1));
        assert!(!link_stays_inside(Path::new("../x"), 0));
        assert!(!link_stays_inside(Path::new("a/../../x"), 0));
        assert!(!link_stays_inside(Path::new("/etc/passwd"), 3));
    }

    #[test]
    fn install_fixes_modes_and_swaps() {
        let d = tmp("swap");
        let src = d.join("src");
        live_shape(&src);
        let before = check_tree(&src).unwrap();
        assert_eq!(before.len(), 6, "{before:?}");
        let dest = d.join("console-dist");
        // first install: no old tree
        let i = install(&src, &dest, "20261028-100000+0100").unwrap();
        assert!(i.old.is_none());
        assert_eq!(i.report.files, 3);
        assert_eq!(i.report.links, 1);
        assert_eq!(i.report.fixed, 6);
        assert!(check_tree(&dest).unwrap().is_empty());
        assert_eq!(mode(&dest), 0o755);
        assert_eq!(mode(&dest.join("static/favicon.svg")), 0o644);
        assert_eq!(mode(&dest.join("static/__grok")), 0o755);
        assert_eq!(mode(&dest.join("node_modules/srvx/bin/srvx.mjs")), 0o755);
        assert_eq!(
            fs::read_link(dest.join("static/icon.svg")).unwrap(),
            Path::new("favicon.svg")
        );
        // second install keeps the first as .old-<stamp>
        fs::write(src.join("static/favicon.svg"), "<svg v2/>").unwrap();
        let i2 = install(&src, &dest, "20261028-100100+0100").unwrap();
        let old = i2.old.unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("static/favicon.svg")).unwrap(),
            "<svg v2/>"
        );
        assert_eq!(
            fs::read_to_string(old.join("static/favicon.svg")).unwrap(),
            "<svg/>"
        );
        // same stamp again refuses instead of overwriting the rollback copy
        assert!(install(&src, &dest, "20261028-100100+0100").is_err());
        assert!(!crate::files::sibling(&dest, &format!(".new-{}", std::process::id())).exists());
        // rollback
        let failed = rollback(&dest, &old, "20261028-100200+0100").unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("static/favicon.svg")).unwrap(),
            "<svg/>"
        );
        assert!(failed.join("static/favicon.svg").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn refusals_leave_dest_untouched() {
        let d = tmp("refuse");
        let src = d.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), "a").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.join("bad")).unwrap();
        let dest = d.join("dest");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("keep"), "k").unwrap();
        let e = install(&src, &dest, "s1").unwrap_err();
        assert!(e.to_string().contains("leaves the tree"), "{e}");
        assert_eq!(fs::read_to_string(dest.join("keep")).unwrap(), "k");
        assert!(!crate::files::sibling(&dest, &format!(".new-{}", std::process::id())).exists());
        // nested src/dest
        fs::remove_file(src.join("bad")).unwrap();
        assert!(install(&src, &src.join("inner"), "s2").is_err());
        assert!(install(&d, &dest, "s3").is_err());
        // dest is a file
        let f = d.join("file");
        fs::write(&f, "x").unwrap();
        assert!(install(&src, &f, "s4").is_err());
        assert_eq!(fs::read_to_string(&f).unwrap(), "x");
        let _ = fs::remove_dir_all(&d);
    }
}
