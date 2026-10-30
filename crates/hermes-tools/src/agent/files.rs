// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! File tools: `read_file`, `write_file`, `edit_file`, `list_files`,
//! `search_files` (the prior `file` toolset: read/write/search/patch).
//!
//! In-process std::fs, no shell. Path rules on top of the deny list:
//! * no NUL, no `..` component, relative paths resolve in the workspace;
//! * writes: the target must be inside the workspace, no component on the
//!   way (workspace itself included) may be a symlink, the final file is
//!   written to a fresh temp file and renamed over (so a symlink planted at
//!   the target is replaced, never followed), and [`super::NEVER_WRITE_NAMES`]
//!   are refused;
//! * reads: the canonical path must be inside the workspace or a configured
//!   read-only root; bounded by `max_read_bytes`.

use super::{cut, Shared, NEVER_WRITE_NAMES};
use hermes_rs::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt; // mode(); create_new = O_CREAT|O_EXCL never follows a symlink
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Lexical resolution: absolute stays, relative joins the workspace;
/// `..` and NUL are refused outright.
pub fn lexical(ws: &Path, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty path".into());
    }
    if raw.contains('\0') {
        return Err("NUL byte in path".into());
    }
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        ws.join(p)
    };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
            Component::ParentDir => return Err("'..' is not allowed in paths".into()),
            Component::Normal(n) => out.push(n),
        }
    }
    Ok(out)
}

/// A path that may be written: inside the workspace, no symlink on the way.
pub fn write_target(ws: &Path, raw: &str) -> Result<PathBuf, String> {
    let p = lexical(ws, raw)?;
    let rel = p
        .strip_prefix(ws)
        .map_err(|_| {
            format!(
                "{} is outside the Hermes workspace {}",
                p.display(),
                ws.display()
            )
        })?
        .to_path_buf();
    if rel.as_os_str().is_empty() {
        return Err("the workspace root itself is not a file".into());
    }
    let wsm = std::fs::symlink_metadata(ws).map_err(|e| format!("workspace: {e}"))?;
    if wsm.file_type().is_symlink() || !wsm.is_dir() {
        return Err("workspace is not a real directory".into());
    }
    let mut cur = ws.to_path_buf();
    let n = rel.components().count();
    for (i, c) in rel.components().enumerate() {
        let name = c.as_os_str().to_string_lossy().to_string();
        if NEVER_WRITE_NAMES
            .iter()
            .any(|x| x.eq_ignore_ascii_case(&name))
            || name.to_ascii_lowercase().starts_with(".env")
        {
            return Err(format!("{name} may never be written by an agent tool"));
        }
        cur.push(c);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!(
                    "{} is a symlink; refusing to write through it",
                    cur.display()
                ))
            }
            Ok(m) if i + 1 < n && !m.is_dir() => {
                return Err(format!("{} is not a directory", cur.display()))
            }
            Ok(m) if i + 1 == n && m.is_dir() => {
                return Err(format!("{} is a directory", cur.display()))
            }
            _ => {}
        }
    }
    Ok(cur)
}

/// A path that may be read: canonical path inside the workspace or a
/// read-only root.
pub fn read_target(sh: &Shared, raw: &str) -> Result<PathBuf, String> {
    let p = lexical(&sh.cfg.workspace, raw)?;
    let c = p
        .canonicalize()
        .map_err(|e| format!("{}: {e}", p.display()))?;
    let ok = c.starts_with(&sh.cfg.workspace) || sh.cfg.read_roots.iter().any(|r| c.starts_with(r));
    if !ok {
        return Err(format!(
            "{} is outside the workspace and the read-only roots ({})",
            c.display(),
            std::iter::once(&sh.cfg.workspace)
                .chain(sh.cfg.read_roots.iter())
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    // the deny list checked the raw path; check the canonical one too
    let pol = hermes_rs::denylist::Policy::workspace(&sh.cfg.workspace);
    hermes_rs::denylist::check_path(
        &c.display().to_string(),
        hermes_rs::denylist::Access::Read,
        &sh.cfg.workspace,
        &pol,
    )
    .map_err(|d| format!("blocked by the Hermes deny list ({}): {}", d.rule, d.reason))?;
    Ok(c)
}

fn only_keys(args: &Value, keys: &[&str]) -> Result<(), String> {
    match args {
        Value::Object(o) => {
            for k in o.keys() {
                if !keys.contains(&k.as_str()) {
                    return Err(format!("unknown argument {k:?} (allowed: {keys:?})"));
                }
            }
            Ok(())
        }
        Value::Null => Ok(()),
        _ => Err("arguments must be a JSON object".into()),
    }
}

fn s<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str)
}

fn workspace_bytes(ws: &Path, limit: u64) -> u64 {
    crate::exec::footprint(ws, limit, u64::MAX).bytes
}

/// Write `data` to `target` atomically (temp file + rename, 0644).
fn atomic_write(sh: &Shared, target: &Path, data: &[u8]) -> Result<(), String> {
    let used = workspace_bytes(&sh.cfg.workspace, sh.cfg.workspace_quota_bytes + 1);
    if used + data.len() as u64 > sh.cfg.workspace_quota_bytes {
        return Err(format!(
            "workspace quota: {} MiB used of {} MiB",
            used >> 20,
            sh.cfg.workspace_quota_bytes >> 20
        ));
    }
    let dir = target.parent().ok_or("no parent directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    // re-check after mkdir: nothing on the way became a symlink
    write_target(&sh.cfg.workspace, &target.display().to_string())?;
    let tmp = dir.join(format!(".{}", hermes_rs::new_id("tmp_")));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&tmp)
        .map_err(|e| format!("create {}: {e}", tmp.display()))?;
    if let Err(e) = f.write_all(data).and_then(|_| f.sync_all()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write: {e}"));
    }
    sh.cfg.own(&tmp);
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename into place: {e}")
    })?;
    sh.cfg.own_up(dir);
    Ok(())
}

/// Read at most `max` bytes from `offset`.
fn read_bounded(p: &Path, offset: u64, max: u64) -> Result<(Vec<u8>, u64), String> {
    use std::io::Seek;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .open(p)
        .map_err(|e| format!("open {}: {e}", p.display()))?;
    let m = f.metadata().map_err(|e| format!("stat: {e}"))?;
    if !m.is_file() {
        return Err(format!("{} is not a regular file", p.display()));
    }
    f.seek(std::io::SeekFrom::Start(offset))
        .map_err(|e| format!("seek: {e}"))?;
    let mut buf = Vec::new();
    f.take(max)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read: {e}"))?;
    Ok((buf, m.len()))
}

macro_rules! guard {
    ($sh:expr, $ctx:expr, $e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => {
                return ToolOutput::err(format!("refused: {e}"));
            }
        }
    };
}

// ---------------------------------------------------------------- read_file

pub struct ReadFile(pub Arc<Shared>);

impl Tool for ReadFile {
    fn name(&self) -> String {
        "read_file".into()
    }
    fn description(&self) -> String {
        "Read a text file in the Hermes workspace (relative path) or a read-only root. Optional byte offset and max_bytes (capped). Secret, system and Hermes-config paths are refused.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string", "description": "relative to the workspace, or absolute inside a read-only root"},
            "offset": {"type": "integer", "minimum": 0},
            "max_bytes": {"type": "integer", "minimum": 1}
        }, "required": ["path"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        guard!(sh, ctx, sh.gate(ctx));
        guard!(sh, ctx, only_keys(args, &["path", "offset", "max_bytes"]));
        let p = guard!(sh, ctx, read_target(sh, s(args, "path").unwrap_or("")));
        let off = args["offset"].as_u64().unwrap_or(0);
        let max = args["max_bytes"]
            .as_u64()
            .unwrap_or(sh.cfg.max_read_bytes)
            .min(sh.cfg.max_read_bytes);
        let (buf, size) = guard!(sh, ctx, read_bounded(&p, off, max));
        sh.log("read_file", ctx, &format!("{} {}B", p.display(), buf.len()));
        let end = off + buf.len() as u64;
        let text = String::from_utf8_lossy(&buf).to_string();
        ToolOutput::ok(
            json!({
                "path": p.display().to_string(),
                "size": size,
                "offset": off,
                "bytes": buf.len(),
                "truncated": end < size,
                "next_offset": if end < size { json!(end) } else { Value::Null },
                "content": hermes_rs::redact(&text),
            })
            .to_string(),
        )
    }
}

// --------------------------------------------------------------- write_file

pub struct WriteFile(pub Arc<Shared>);

impl Tool for WriteFile {
    fn name(&self) -> String {
        "write_file".into()
    }
    fn description(&self) -> String {
        "Create or overwrite (or append to) a file inside the Hermes workspace. Parent folders are created. Only the workspace is writable; Hermes config, SOUL.md, secrets and system paths never are.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string", "description": "relative to the workspace"},
            "content": {"type": "string"},
            "append": {"type": "boolean", "default": false}
        }, "required": ["path", "content"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        guard!(sh, ctx, sh.gate(ctx));
        guard!(sh, ctx, only_keys(args, &["path", "content", "append"]));
        let t = guard!(
            sh,
            ctx,
            write_target(&sh.cfg.workspace, s(args, "path").unwrap_or(""))
        );
        let content = s(args, "content").unwrap_or("");
        let append = args["append"].as_bool().unwrap_or(false);
        let mut data = Vec::new();
        if append && t.exists() {
            let (old, size) = guard!(sh, ctx, read_bounded(&t, 0, sh.cfg.max_write_bytes + 1));
            if size > old.len() as u64 {
                return ToolOutput::err("refused: file too large to append to");
            }
            data = old;
        }
        data.extend_from_slice(content.as_bytes());
        if data.len() as u64 > sh.cfg.max_write_bytes {
            return ToolOutput::err(format!(
                "refused: {} bytes is over the {} byte write cap",
                data.len(),
                sh.cfg.max_write_bytes
            ));
        }
        guard!(sh, ctx, atomic_write(sh, &t, &data));
        sh.log(
            "write_file",
            ctx,
            &format!("{} {}B", t.display(), data.len()),
        );
        ToolOutput::ok(
            json!({"path": t.display().to_string(), "bytes": data.len(), "appended": append})
                .to_string(),
        )
    }
}

// ---------------------------------------------------------------- edit_file

pub struct EditFile(pub Arc<Shared>);

impl Tool for EditFile {
    fn name(&self) -> String {
        "edit_file".into()
    }
    fn description(&self) -> String {
        "Replace text in a workspace file: old_text must occur exactly once (or set replace_all). The smallest-change way to fix code.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string"},
            "old_text": {"type": "string"},
            "new_text": {"type": "string"},
            "replace_all": {"type": "boolean", "default": false}
        }, "required": ["path", "old_text", "new_text"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        guard!(sh, ctx, sh.gate(ctx));
        guard!(
            sh,
            ctx,
            only_keys(args, &["path", "old_text", "new_text", "replace_all"])
        );
        let t = guard!(
            sh,
            ctx,
            write_target(&sh.cfg.workspace, s(args, "path").unwrap_or(""))
        );
        let old = s(args, "old_text").unwrap_or("");
        let new = s(args, "new_text").unwrap_or("");
        if old.is_empty() {
            return ToolOutput::err("refused: old_text is empty");
        }
        let (buf, size) = guard!(sh, ctx, read_bounded(&t, 0, sh.cfg.max_write_bytes + 1));
        if size > buf.len() as u64 {
            return ToolOutput::err("refused: file over the write cap");
        }
        let Ok(text) = String::from_utf8(buf) else {
            return ToolOutput::err("refused: not a UTF-8 text file");
        };
        let n = text.matches(old).count();
        let all = args["replace_all"].as_bool().unwrap_or(false);
        if n == 0 {
            return ToolOutput::err("old_text not found");
        }
        if n > 1 && !all {
            return ToolOutput::err(format!(
                "old_text occurs {n} times; make it unique or set replace_all"
            ));
        }
        let out = if all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        if out.len() as u64 > sh.cfg.max_write_bytes {
            return ToolOutput::err("refused: result over the write cap");
        }
        guard!(sh, ctx, atomic_write(sh, &t, out.as_bytes()));
        sh.log(
            "edit_file",
            ctx,
            &format!("{} {n} replacement(s)", t.display()),
        );
        ToolOutput::ok(
            json!({"path": t.display().to_string(), "replacements": if all { n } else { 1 }, "bytes": out.len()})
                .to_string(),
        )
    }
}

// --------------------------------------------------------------- list_files

pub struct ListFiles(pub Arc<Shared>);

const LIST_CAP: usize = 500;

impl Tool for ListFiles {
    fn name(&self) -> String {
        "list_files".into()
    }
    fn description(&self) -> String {
        "List a directory in the workspace or a read-only root (default: workspace root), up to depth 4 and 500 entries. Directories end with '/'.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string", "default": "."},
            "depth": {"type": "integer", "minimum": 1, "maximum": 4, "default": 1}
        }})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        guard!(sh, ctx, sh.gate(ctx));
        guard!(sh, ctx, only_keys(args, &["path", "depth"]));
        let root = guard!(sh, ctx, read_target(sh, s(args, "path").unwrap_or(".")));
        if !root.is_dir() {
            return ToolOutput::err(format!("{} is not a directory", root.display()));
        }
        let depth = args["depth"].as_u64().unwrap_or(1).clamp(1, 4) as usize;
        let mut out = vec![];
        let mut truncated = false;
        let mut stack = vec![(root.clone(), 1usize)];
        while let Some((d, lvl)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut ents: Vec<_> = rd.flatten().collect();
            ents.sort_by_key(|e| e.file_name());
            for e in ents {
                if out.len() >= LIST_CAP {
                    truncated = true;
                    break;
                }
                let Ok(m) = std::fs::symlink_metadata(e.path()) else {
                    continue;
                };
                let rel = e
                    .path()
                    .strip_prefix(&root)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let ft = m.file_type();
                if ft.is_dir() {
                    out.push(json!({"name": format!("{rel}/")}));
                    if lvl < depth && e.file_name() != ".git" {
                        stack.push((e.path(), lvl + 1));
                    }
                } else if ft.is_symlink() {
                    out.push(json!({"name": rel, "symlink": true}));
                } else {
                    out.push(json!({"name": rel, "bytes": m.len()}));
                }
            }
        }
        sh.log(
            "list_files",
            ctx,
            &format!("{} {} entries", root.display(), out.len()),
        );
        ToolOutput::ok(
            json!({"path": root.display().to_string(), "entries": out, "truncated": truncated})
                .to_string(),
        )
    }
}

// ------------------------------------------------------------- search_files

pub struct SearchFiles(pub Arc<Shared>);

const SEARCH_HITS: usize = 100;
const SEARCH_FILES: usize = 5000;

impl Tool for SearchFiles {
    fn name(&self) -> String {
        "search_files".into()
    }
    fn description(&self) -> String {
        "Search for a literal text (case-insensitive) in files under a workspace or read-only folder; returns up to 100 'file:line: text' hits. Optional name_contains filters file names.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "query": {"type": "string"},
            "path": {"type": "string", "default": "."},
            "name_contains": {"type": "string"}
        }, "required": ["query"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        guard!(sh, ctx, sh.gate(ctx));
        guard!(
            sh,
            ctx,
            only_keys(args, &["query", "path", "name_contains"])
        );
        let q = s(args, "query").unwrap_or("").to_lowercase();
        if q.is_empty() {
            return ToolOutput::err("empty query");
        }
        let filt = s(args, "name_contains").map(str::to_lowercase);
        let root = guard!(sh, ctx, read_target(sh, s(args, "path").unwrap_or(".")));
        let mut hits = vec![];
        let mut files = 0usize;
        let mut stack = vec![root.clone()];
        'outer: while let Some(d) = stack.pop() {
            let ents: Vec<_> = if d.is_dir() {
                match std::fs::read_dir(&d) {
                    Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
                    Err(_) => continue,
                }
            } else {
                vec![d.clone()]
            };
            for p in ents {
                let Ok(m) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if m.is_dir() {
                    if p.file_name().is_some_and(|n| n != ".git" && n != "target") {
                        stack.push(p);
                    }
                    continue;
                }
                if !m.is_file() || m.len() > sh.cfg.max_read_bytes * 4 {
                    continue;
                }
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if filt.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
                    continue;
                }
                files += 1;
                if files > SEARCH_FILES {
                    break 'outer;
                }
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for (i, line) in text.lines().enumerate() {
                    if line.to_lowercase().contains(&q) {
                        let rel = p
                            .strip_prefix(&root)
                            .map(|x| x.display().to_string())
                            .unwrap_or_else(|_| p.display().to_string());
                        hits.push(format!("{rel}:{}: {}", i + 1, cut(line.trim(), 200)));
                        if hits.len() >= SEARCH_HITS {
                            break 'outer;
                        }
                    }
                }
            }
        }
        sh.log(
            "search_files",
            ctx,
            &format!("{} {} hits", root.display(), hits.len()),
        );
        ToolOutput::ok(
            json!({
                "path": root.display().to_string(),
                "hits": hits.iter().map(|h| hermes_rs::redact(h)).collect::<Vec<_>>(),
                "files_scanned": files.min(SEARCH_FILES),
                "truncated": hits.len() >= SEARCH_HITS || files > SEARCH_FILES,
            })
            .to_string(),
        )
    }
}
