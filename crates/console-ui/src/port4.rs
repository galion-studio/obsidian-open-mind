// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! console-port-4 (wave 4 slot 138): the fourth fifth, alphabetical, of the
//! 17 console routes still in TypeScript at 669dc18: `/md/$name`,
//! `/memory`, `/mesh`. Page bodies are in `pages::{md, memory, mesh}`.
//!
//! All three are read-only (GET/HEAD only, no form, nothing sent upstream
//! except `GET /api/mesh` and the header-dot `GET /api/v1/health`).
//!
//! `/memory` and `/md/*` read host files, like the TSX server functions did
//! (Node `fs` on the same machine): the files are named by flags
//! ([`HostFiles`]), never by the URL. `/md/$name` maps only `user` and
//! `persona`; any other name reads nothing.

use crate::client;
use crate::json::{self, Value};
use crate::layout::{self, Origin};
use crate::pages::{md, memory, mesh, p4};
use crate::{Reply, Request, Ui};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The three routes (`/md/user` stands for `/md/$name` in `PORTED`).
pub const ROUTES: &[&str] = &["/md/user", "/memory", "/mesh"];

/// Host files the rooms read (flags of `galion-console-static`). Live
/// values: `--memory-root /var/lib/memory-tencentdb`, `--model-file
/// /opt/galion/galion-light/one/model`, `--user-md
/// /opt/galion/hermes/memories/USER.md`. `None` = the room says it is not
/// configured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostFiles {
    pub memory_root: Option<PathBuf>,
    pub model_file: Option<PathBuf>,
    pub user_md: Option<PathBuf>,
}

/// Whether `/md/...` is ours (any name: unknown names get our 404 page).
pub fn owns(path: &str) -> bool {
    path.starts_with("/md/") && path.len() > 4
}

/// Counts for `/memory` (memory.tsx `readMemory`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemorySnap {
    pub l0: u64,
    pub l1: u64,
    pub l2: u64,
    pub l3_bytes: u64,
    pub scenes: u64,
    pub refs: u64,
    pub canvases: u64,
    pub model: String,
    pub persona_excerpt: String,
    /// The memory root itself could be listed.
    pub root_ok: bool,
    /// A walk hit [`MAX_ENTRIES`] or [`MAX_DEPTH`].
    pub capped: bool,
    /// Why the root could not be read (when `!root_ok`).
    pub error: String,
}

/// Directory entries looked at per page view, and deepest folder.
pub const MAX_ENTRIES: u64 = 250_000;
pub const MAX_DEPTH: u32 = 32;
/// Persona excerpt length (UTF-16 units, the TSX `slice(0, 400)`).
pub const EXCERPT: usize = 400;
/// Largest `/md/*` file text shown.
pub const MAX_MD: u64 = 1 << 20;
/// Largest model-name file read.
const MAX_MODEL: u64 = 256;

struct Walk {
    seen: u64,
    capped: bool,
}

impl Walk {
    /// Directory entries (names), or empty when unreadable (TSX `catch`).
    fn list(&mut self, dir: &Path, depth: u32) -> Vec<(String, PathBuf)> {
        if depth > MAX_DEPTH {
            self.capped = true;
            return Vec::new();
        }
        let Ok(rd) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            if self.seen >= MAX_ENTRIES {
                self.capped = true;
                break;
            }
            self.seen += 1;
            out.push((e.file_name().to_string_lossy().into_owned(), e.path()));
        }
        out
    }

    /// `filesUnder`: files at any depth (stat follows links like Node).
    fn files_under(&mut self, dir: &Path, depth: u32) -> u64 {
        let mut n = 0;
        for (_, p) in self.list(dir, depth) {
            match fs::metadata(&p) {
                Ok(m) if m.is_dir() => n += self.files_under(&p, depth + 1),
                Ok(m) if m.is_file() => n += 1,
                _ => {}
            }
        }
        n
    }

    /// `mdIn`: `*.md` files directly in `dir`.
    fn md_in(&mut self, dir: &Path, depth: u32) -> u64 {
        self.list(dir, depth)
            .into_iter()
            .filter(|(name, p)| name.ends_with(".md") && fs::metadata(p).is_ok_and(|m| m.is_file()))
            .count() as u64
    }

    /// `refsUnder`: `*.md` files directly in every folder named `refs`.
    fn refs_under(&mut self, dir: &Path, depth: u32) -> u64 {
        let mut n = 0;
        for (name, p) in self.list(dir, depth) {
            if !fs::metadata(&p).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            if name == "refs" {
                n += self.md_in(&p, depth + 1);
            }
            n += self.refs_under(&p, depth + 1);
        }
        n
    }

    /// `canvasesUnder`: files named `canvas.mmd` at any depth.
    fn canvases_under(&mut self, dir: &Path, depth: u32) -> u64 {
        let mut n = 0;
        for (name, p) in self.list(dir, depth) {
            match fs::metadata(&p) {
                Ok(m) if m.is_dir() => n += self.canvases_under(&p, depth + 1),
                Ok(m) if m.is_file() && name == "canvas.mmd" => n += 1,
                _ => {}
            }
        }
        n
    }
}

fn bytes_of(p: &Path) -> u64 {
    fs::metadata(p)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Read at most `cap` bytes of a regular file.
fn read_capped(p: &Path, cap: u64) -> std::io::Result<(Vec<u8>, u64, Option<i64>)> {
    let f = fs::File::open(p)?;
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf)?;
    Ok((buf, m.len(), mtime))
}

/// Short, path-free reason for an I/O error (pages never show host paths).
pub fn why(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "file not found".into(),
        std::io::ErrorKind::PermissionDenied => {
            "permission denied (the console user cannot read it)".into()
        }
        _ if e.to_string() == "not a regular file" => "not a regular file".into(),
        _ => "read error".into(),
    }
}

/// The `/memory` snapshot for `files` (`None` when no root is configured).
pub fn memory_snap(files: &HostFiles) -> Option<MemorySnap> {
    let root = files.memory_root.as_deref()?;
    let mut s = MemorySnap::default();
    match fs::read_dir(root) {
        Ok(_) => s.root_ok = true,
        Err(e) => s.error = why(&e).replace("file not found", "folder not found"),
    }
    let mut w = Walk {
        seen: 0,
        capped: false,
    };
    if s.root_ok {
        let pyr = root.join("pyramid");
        s.l0 = w.files_under(&pyr.join("l0"), 0);
        s.l1 = w.files_under(&pyr.join("l1"), 0);
        s.l2 = w.files_under(&pyr.join("l2"), 0);
        s.l3_bytes = bytes_of(&pyr.join("l3").join("persona.md"));
        s.scenes = w.md_in(&root.join("scene_blocks"), 0);
        let off = root.join("offload");
        s.refs = w.refs_under(&off, 0);
        s.canvases = w.canvases_under(&off, 0);
        if let Ok((b, _, _)) = read_capped(&root.join("persona.md"), (EXCERPT as u64) * 4) {
            let t = String::from_utf8_lossy(&b);
            s.persona_excerpt = p4::js_slice(&t, EXCERPT).to_string();
        }
    }
    s.capped = w.capped;
    if let Some(mf) = files.model_file.as_deref() {
        if let Ok((b, _, _)) = read_capped(mf, MAX_MODEL) {
            s.model = String::from_utf8_lossy(&b).trim().to_string();
        }
    }
    Some(s)
}

/// Read `/md/$name` (`name` already checked against [`md::NAMES`]).
pub fn md_read(files: &HostFiles, name: &str) -> Result<md::MdFile, String> {
    let path = match name {
        "user" => files.user_md.clone(),
        "persona" => files.memory_root.as_ref().map(|r| r.join("persona.md")),
        _ => None,
    }
    .ok_or_else(|| "not configured on this console".to_string())?;
    let (b, bytes, mtime) = read_capped(&path, MAX_MD).map_err(|e| why(&e))?;
    Ok(md::MdFile {
        bytes,
        mtime,
        text: String::from_utf8_lossy(&b).into_owned(),
        cut: bytes > MAX_MD,
    })
}

impl Ui {
    pub(crate) fn port4_room(&self, req: &Request<'_>) -> Reply {
        match req.path {
            "/memory" => self.p4_memory(req),
            "/mesh" => self.p4_mesh(req),
            _ => self.p4_md(req),
        }
    }

    fn p4_memory(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let (origin, snap) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let m = s.spawn(|| memory_snap(&self.cfg.files));
            (o.join().unwrap_or(Origin::Down), m.join().ok().flatten())
        });
        let body = memory::body(snap.as_ref());
        let f = self.frame(req, "Galion Console · Memory");
        Reply::html(200, layout::console(&f, origin, &body))
    }

    fn p4_mesh(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let (origin, got) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let g = s.spawn(|| client::call(self.cfg.upstream, "GET", "/api/mesh", &fwd, None));
            (
                o.join().unwrap_or(Origin::Down),
                g.join()
                    .unwrap_or_else(|_| Err(std::io::Error::other("panic"))),
            )
        });
        let (board, miss) = match got {
            Ok(r) => {
                let v = json::parse(&r.body).ok();
                if r.status == 200 && v.is_some() {
                    (v, String::new())
                } else {
                    let err = v
                        .as_ref()
                        .and_then(|v| v.get("error").as_str())
                        .map(|e| format!(" ({})", e.chars().take(200).collect::<String>()))
                        .unwrap_or_default();
                    (None, format!("HTTP {}{err}", r.status))
                }
            }
            Err(_) => (None, "the console API did not answer".into()),
        };
        let body = mesh::body(board.as_ref().filter(|v| !matches!(v, Value::Null)), &miss);
        let f = self.frame(req, "Galion Console · Scope");
        Reply::html(200, layout::console(&f, origin, &body))
    }

    fn p4_md(&self, req: &Request<'_>) -> Reply {
        let name = req.path.trim_start_matches("/md/");
        let (status, body, title) = match md::title_of(name) {
            None => (404, md::body(name, Err("")), "Not found".to_string()),
            Some(t) => match md_read(&self.cfg.files, name) {
                Ok(file) => (200, md::body(name, Ok(&file)), t.to_string()),
                Err(why) => {
                    let code = if why.starts_with("not configured") || why == "file not found" {
                        404
                    } else {
                        500
                    };
                    (code, md::body(name, Err(&why)), t.to_string())
                }
            },
        };
        let f = self.frame(req, &title);
        Reply::html(status, layout::document(&f, "", &body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(tag: &str) -> Tmp {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("p4-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    fn touch(p: &Path, body: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::File::create(p).unwrap().write_all(body).unwrap();
    }

    /// A tree with every rule of memory.tsx exercised.
    fn tree(r: &Path) {
        touch(&r.join("pyramid/l0/a.json"), b"1");
        touch(&r.join("pyramid/l0/sub/b.json"), b"1");
        touch(&r.join("pyramid/l0/sub/deeper/c"), b"1");
        touch(&r.join("pyramid/l1/x.md"), b"1");
        fs::create_dir_all(r.join("pyramid/l2")).unwrap();
        touch(&r.join("pyramid/l3/persona.md"), &[b'p'; 1234]);
        touch(&r.join("scene_blocks/one.md"), b"s");
        touch(&r.join("scene_blocks/two.md"), b"s");
        touch(&r.join("scene_blocks/not.txt"), b"s");
        fs::create_dir_all(r.join("scene_blocks/dir.md")).unwrap();
        touch(&r.join("scene_blocks/nested/three.md"), b"s");
        touch(&r.join("offload/s1/refs/r1.md"), b"r");
        touch(&r.join("offload/s1/refs/r2.md"), b"r");
        touch(&r.join("offload/s1/refs/r3.txt"), b"r");
        touch(&r.join("offload/s1/refs/deep/refs/r4.md"), b"r");
        touch(&r.join("offload/s1/refs/deep/r5.md"), b"r");
        touch(&r.join("offload/s2/canvas.mmd"), b"c");
        touch(&r.join("offload/s2/x/canvas.mmd"), b"c");
        touch(&r.join("offload/s2/refs.md"), b"c");
        fs::create_dir_all(r.join("offload/s3/canvas.mmd")).unwrap();
        touch(
            &r.join("persona.md"),
            "# Persona ž\n".repeat(100).as_bytes(),
        );
    }

    #[test]
    fn counts_like_memory_tsx() {
        let t = tmp("count");
        tree(&t.0);
        touch(&t.0.join("model"), b"  galion \n");
        let files = HostFiles {
            memory_root: Some(t.0.clone()),
            model_file: Some(t.0.join("model")),
            user_md: None,
        };
        let s = memory_snap(&files).unwrap();
        assert!(s.root_ok && !s.capped, "{s:?}");
        assert_eq!((s.l0, s.l1, s.l2, s.l3_bytes), (3, 1, 0, 1234));
        assert_eq!(s.scenes, 2, "direct *.md files only");
        assert_eq!(s.refs, 3, "r1, r2 and deep/refs/r4");
        assert_eq!(s.canvases, 2);
        assert_eq!(s.model, "galion");
        assert_eq!(s.persona_excerpt.encode_utf16().count(), 400);
        assert!(s.persona_excerpt.starts_with("# Persona ž\n# Persona"));
    }

    #[test]
    fn missing_and_unconfigured() {
        assert!(memory_snap(&HostFiles::default()).is_none());
        let t = tmp("missing");
        let files = HostFiles {
            memory_root: Some(t.0.join("nope")),
            ..HostFiles::default()
        };
        let s = memory_snap(&files).unwrap();
        assert!(!s.root_ok);
        assert_eq!(s.error, "folder not found");
        assert_eq!(s.l0, 0);
        // An empty root: zeros like the TSX, no excerpt.
        let files = HostFiles {
            memory_root: Some(t.0.clone()),
            ..HostFiles::default()
        };
        let s = memory_snap(&files).unwrap();
        assert!(s.root_ok);
        assert_eq!(
            (
                s.l0,
                s.scenes,
                s.refs,
                s.canvases,
                s.persona_excerpt.as_str()
            ),
            (0, 0, 0, 0, "")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_is_capped() {
        let t = tmp("loop");
        touch(&t.0.join("pyramid/l0/a"), b"1");
        std::os::unix::fs::symlink(t.0.join("pyramid/l0"), t.0.join("pyramid/l0/me")).unwrap();
        let files = HostFiles {
            memory_root: Some(t.0.clone()),
            ..HostFiles::default()
        };
        let s = memory_snap(&files).unwrap();
        assert!(s.capped);
        assert_eq!(s.l0, (MAX_DEPTH + 1) as u64);
    }

    #[test]
    fn md_reads_only_fixed_files() {
        let t = tmp("md");
        touch(&t.0.join("persona.md"), b"# P <b>");
        touch(&t.0.join("USER.md"), b"user facts");
        let files = HostFiles {
            memory_root: Some(t.0.clone()),
            model_file: None,
            user_md: Some(t.0.join("USER.md")),
        };
        let p = md_read(&files, "persona").unwrap();
        assert_eq!((p.bytes, p.text.as_str(), p.cut), (7, "# P <b>", false));
        assert!(p.mtime.is_some());
        assert_eq!(md_read(&files, "user").unwrap().text, "user facts");
        assert_eq!(
            md_read(&files, "../persona").unwrap_err(),
            "not configured on this console"
        );
        let none = HostFiles::default();
        assert_eq!(
            md_read(&none, "user").unwrap_err(),
            "not configured on this console"
        );
        let gone = HostFiles {
            user_md: Some(t.0.join("gone.md")),
            ..HostFiles::default()
        };
        assert_eq!(md_read(&gone, "user").unwrap_err(), "file not found");
        let dir = HostFiles {
            user_md: Some(t.0.clone()),
            ..HostFiles::default()
        };
        assert_eq!(md_read(&dir, "user").unwrap_err(), "not a regular file");
    }

    #[test]
    fn md_cap() {
        let t = tmp("cap");
        touch(&t.0.join("persona.md"), &vec![b'a'; (MAX_MD + 10) as usize]);
        let files = HostFiles {
            memory_root: Some(t.0.clone()),
            ..HostFiles::default()
        };
        let p = md_read(&files, "persona").unwrap();
        assert!(p.cut);
        assert_eq!(p.text.len() as u64, MAX_MD);
        assert_eq!(p.bytes, MAX_MD + 10);
    }

    #[test]
    fn every_port4_class_is_styled() {
        let defined = crate::css::defined_classes();
        let snap = MemorySnap {
            root_ok: true,
            capped: true,
            ..MemorySnap::default()
        };
        let board = json::parse(br#"{"phase":"idle","score":["SCOPE x"]}"#).unwrap();
        let file = md::MdFile {
            bytes: 1,
            mtime: Some(0),
            text: "x".into(),
            cut: true,
        };
        let all = [
            memory::body(Some(&snap)),
            memory::body(None),
            mesh::body(Some(&board), "HTTP 500"),
            mesh::body(None, ""),
            md::body("persona", Ok(&file)),
            md::body("user", Err("x")),
            md::body("x", Err("")),
        ]
        .concat();
        let mut missing = Vec::new();
        for cap in all.split("class=\"").skip(1) {
            for c in cap.split('"').next().unwrap_or("").split_whitespace() {
                if !defined.iter().any(|d| d == c) {
                    missing.push(c.to_string());
                }
            }
        }
        assert!(missing.is_empty(), "unstyled: {missing:?}");
        assert!(!all.contains("<script") && !all.contains("<form"));
    }

    #[test]
    fn owns_md() {
        assert!(owns("/md/user") && owns("/md/x"));
        assert!(!owns("/md/") && !owns("/md") && !owns("/mdx"));
    }
}
