// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! File tools for Hermes over one shared folder, on the same `galion-files`
//! jail the console's `/files` explorer uses.
//!
//! - Read tools (always on when `HERMES_RS_FILES_ROOT` is set): `files_list`,
//!   `files_read`, `files_info`, `files_kinds`.
//! - Write tools (only with `HERMES_RS_FILES_WRITE=1`): `files_write`,
//!   `files_mkdir`, `files_rename`, `files_move`, `files_extract`,
//!   `files_trash`. Nothing is ever really deleted: `files_trash` moves into
//!   `<root>/.trash/`, edits keep the old copy in `.trash/edits/`, rename /
//!   move / copy refuse to overwrite, extraction goes into a fresh folder.
//!
//! Every call: spend cap -> per-session call cap -> argument allowlist ->
//! audit line (refused calls too) -> galion-files (relative paths only, no
//! `..`, symlinks may not leave the root). The tools run in-process, not in
//! openmind-sandbox: they never start a process. See docs/hermes-files-skill.md.

use crate::audit::{cut, Audit};
use crate::guard;
use galion_files::{kinds, Root};
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{Registry, Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Read-side tool names.
pub const READ_TOOLS: [&str; 4] = ["files_list", "files_read", "files_info", "files_kinds"];
/// Write-side tool names (registered only with `HERMES_RS_FILES_WRITE=1`).
pub const WRITE_TOOLS: [&str; 6] = [
    "files_write",
    "files_mkdir",
    "files_rename",
    "files_move",
    "files_extract",
    "files_trash",
];

#[derive(Debug, Clone)]
pub struct FilesConfig {
    pub root: PathBuf,
    pub write: bool,
    pub audit_path: PathBuf,
    /// Largest text / hex page one `files_read` returns.
    pub max_read_bytes: u64,
    /// Total unpacked bytes one `files_extract` may write.
    pub extract_max_bytes: u64,
    pub max_calls_per_session: u64,
    /// (uid, gid) every entry the write tools make or move is handed to,
    /// with files 0644 and folders 0755 (the gateway's UMask=0077 would
    /// leave them root-only). Same as the agent tools'
    /// `HERMES_RS_TOOLS_RUN_AS`: 65534:65534 under a root gateway, None
    /// (leave as created) otherwise.
    pub owner: Option<(u32, u32)>,
}

impl FilesConfig {
    /// `Ok(None)` when `HERMES_RS_FILES_ROOT` is unset (tools off).
    pub fn from_env(
        env: &dyn Fn(&str) -> Option<String>,
        hermes_home: &Path,
    ) -> Result<Option<FilesConfig>, String> {
        let get = |k: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let Some(root) = get("HERMES_RS_FILES_ROOT") else {
            return Ok(None);
        };
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return Err("HERMES_RS_FILES_ROOT must be an absolute path".into());
        }
        let root = root
            .canonicalize()
            .map_err(|e| format!("HERMES_RS_FILES_ROOT {}: {e}", root.display()))?;
        root_ok(&root)?;
        let num = |k: &str, d: u64| -> Result<u64, String> {
            match get(k) {
                None => Ok(d),
                Some(v) => v
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or(format!("{k} must be a positive integer")),
            }
        };
        let write = match get("HERMES_RS_FILES_WRITE").as_deref() {
            None | Some("0") => false,
            Some("1") => true,
            Some(_) => return Err("HERMES_RS_FILES_WRITE must be 0 or 1".into()),
        };
        let audit_path = match get("HERMES_RS_FILES_AUDIT") {
            Some(p) if Path::new(&p).is_absolute() => PathBuf::from(p),
            Some(_) => return Err("HERMES_RS_FILES_AUDIT must be an absolute path".into()),
            None => hermes_home.join("files-audit.jsonl"),
        };
        if audit_path.starts_with(&root) {
            return Err("HERMES_RS_FILES_AUDIT must be outside HERMES_RS_FILES_ROOT".into());
        }
        Ok(Some(FilesConfig {
            root,
            write,
            audit_path,
            max_read_bytes: num("HERMES_RS_FILES_MAX_READ", 1 << 20)?,
            extract_max_bytes: num("HERMES_RS_FILES_EXTRACT_MAX_MB", 4096)? << 20,
            max_calls_per_session: num("HERMES_RS_FILES_MAX_CALLS", 400)?,
            owner: crate::agent::run_as_from_env(env, openmind_sandbox::effective_uid())?,
        }))
    }
}

/// The shared root may not be `/` or a system folder, and may not hold
/// hermes's own state (`SOUL.md`, `config.yaml` stay out of reach).
pub fn root_ok(root: &Path) -> Result<(), String> {
    const SYSTEM: [&str; 12] = [
        "/", "/bin", "/boot", "/dev", "/etc", "/lib", "/proc", "/root", "/sbin", "/sys", "/usr",
        "/var",
    ];
    if SYSTEM.iter().any(|s| root == Path::new(s)) {
        return Err(format!("refusing {} as the files root", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    for locked in ["SOUL.md", "config.yaml"] {
        if root.join(locked).exists() {
            return Err(format!(
                "{} holds {locked} (hermes home?); pick a plain shared folder",
                root.display()
            ));
        }
    }
    Ok(())
}

/// State shared by the file tools.
pub struct FilesShared {
    pub cfg: FilesConfig,
    pub root: Root,
    pub audit: Audit,
    pub spend: Option<Arc<SpendTracker>>,
    calls: Mutex<HashMap<String, u64>>,
}

impl FilesShared {
    pub fn new(cfg: FilesConfig, spend: Option<Arc<SpendTracker>>) -> Result<Self, String> {
        let root = Root::new(&cfg.root, cfg.write)
            .map_err(|e| format!("files root {}: {e}", cfg.root.display()))?
            .with_owner(cfg.owner);
        Ok(FilesShared {
            audit: Audit::new(cfg.audit_path.clone()),
            cfg,
            root,
            spend,
            calls: Mutex::new(HashMap::new()),
        })
    }

    fn run(
        &self,
        tool: &str,
        keys: &[&str],
        args: &Value,
        ctx: &ToolCtx,
        f: impl FnOnce(&Value) -> galion_files::Result<Value>,
    ) -> ToolOutput {
        if let Err(e) = self.gate(ctx) {
            return self.refuse(tool, ctx, args, &e);
        }
        if let Err(e) = guard::only_keys(args, keys) {
            return self.refuse(tool, ctx, args, &e);
        }
        if !self.audit.record(json!({
            "tool": tool, "session": ctx.session_id, "decision": "allowed", "args": log_args(args),
        })) {
            return ToolOutput::err(format!(
                "audit log {} is not writable; refusing to run unaudited",
                self.audit.path().display()
            ));
        }
        match f(args) {
            Ok(v) => {
                self.audit.record(json!({
                    "tool": tool, "session": ctx.session_id, "decision": "ran",
                    "result": json!(cut(&summary(&v), 300)),
                }));
                ToolOutput::ok(v.to_string())
            }
            Err(e) => {
                let m = e.message();
                self.audit.record(json!({
                    "tool": tool, "session": ctx.session_id, "decision": "failed", "reason": m,
                }));
                ToolOutput::err(m)
            }
        }
    }

    fn gate(&self, ctx: &ToolCtx) -> Result<(), String> {
        if let Some(s) = &self.spend {
            s.check(ctx.spend_session).map_err(|b| b.message())?;
        }
        let mut c = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let n = c.entry(ctx.spend_session.to_string()).or_insert(0);
        if *n >= self.cfg.max_calls_per_session {
            return Err(format!(
                "files tool call cap reached for this session ({} calls)",
                self.cfg.max_calls_per_session
            ));
        }
        *n += 1;
        Ok(())
    }

    fn refuse(&self, tool: &str, ctx: &ToolCtx, args: &Value, why: &str) -> ToolOutput {
        self.audit.record(json!({
            "tool": tool, "session": ctx.session_id, "decision": "refused",
            "reason": why, "args": log_args(args),
        }));
        ToolOutput::err(format!("refused: {why}"))
    }
}

fn log_args(args: &Value) -> Value {
    let mut a = args.clone();
    if let Some(c) = a.get("content").and_then(Value::as_str) {
        a["content"] = json!(format!("<{} bytes>", c.len()));
    }
    json!(cut(&a.to_string(), 600))
}

/// Result for the audit log: big strings become their size.
fn summary(v: &Value) -> String {
    let mut r = v.clone();
    for k in ["text", "dump"] {
        if let Some(s) = r.get(k).and_then(Value::as_str) {
            r[k] = json!(format!("<{} bytes>", s.len()));
        }
    }
    if let Some(a) = r.get("entries").and_then(Value::as_array) {
        r["entries"] = json!(format!("<{} entries>", a.len()));
    }
    if let Some(a) = r.get("members").and_then(Value::as_array) {
        r["members"] = json!(format!("<{} members>", a.len()));
    }
    r.to_string()
}

/// Names that usually hold secrets. `files_read` refuses them (list and info
/// still show they exist) so keys never land in a model context or a log.
pub fn secret_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || [".pem", ".key", ".p12", ".pfx", ".kdbx", ".keystore"]
            .iter()
            .any(|x| name.ends_with(x))
        || [
            ".netrc",
            ".npmrc",
            ".pypirc",
            "credentials",
            "credentials.json",
            ".htpasswd",
        ]
        .contains(&name.as_str())
}

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Add the file tools; write tools only when the config allows writing.
/// Returns the names added.
pub fn register_files(
    reg: &mut Registry,
    cfg: FilesConfig,
    spend: Option<Arc<SpendTracker>>,
) -> Result<Vec<&'static str>, String> {
    let write = cfg.write;
    let sh = Arc::new(FilesShared::new(cfg, spend)?);
    let mut names: Vec<&'static str> = READ_TOOLS.to_vec();
    if write {
        names.extend(WRITE_TOOLS);
    }
    for n in &names {
        reg.add(Arc::new(FileTool {
            name: n,
            sh: sh.clone(),
        }));
    }
    Ok(names)
}

/// One of the file tools, picked by name.
pub struct FileTool {
    pub name: &'static str,
    pub sh: Arc<FilesShared>,
}

const PATH: &str = "path relative to the shared root, '/' separated; '' is the root";

impl Tool for FileTool {
    fn name(&self) -> String {
        self.name.into()
    }

    fn description(&self) -> String {
        match self.name {
            "files_list" => "List a folder in the shared files root: name, kind (image, video, audio, pdf, text, code, markdown, archive, binary...), size, mtime (Europe/Zagreb), hidden flag. Dotfiles are listed only with hidden=true.",
            "files_read" => "Read a file in the shared root. Text and code come back as text (with a version token for files_write); anything else, or mode='hex', comes back as a hex dump page (offset, max_bytes). Archives (.rar, .zip, .tar, .tar.gz): mode='archive' lists the members.",
            "files_info" => "Facts about one file or folder in the shared root: kind, MIME, size, mtime, permissions, how the console previews it.",
            "files_kinds" => "The file-type table: every known extension with its kind, MIME type, editor language and preview mode.",
            "files_write" => "Save a text file in the shared root. To change an existing file pass the version from files_read (refused with a conflict if the file changed since); the old copy is kept in .trash/edits/. create=true makes a new file and refuses an existing name.",
            "files_mkdir" => "Make a new empty folder (dir = parent folder, name = new folder name). Refuses an existing name.",
            "files_rename" => "Rename a file or folder in place (new_name is a plain name, no '/'). Refuses to overwrite.",
            "files_move" => "Move (or with copy=true, copy) a file or folder into another folder of the shared root. Refuses to overwrite; copies get a ' (1)' style name when taken.",
            "files_extract" => "Unpack a .rar, .zip, .tar or .tar.gz into a fresh folder next to it (or into dest_dir). Unsafe member names are cleaned or skipped and the total size is capped.",
            "files_trash" => "Move a file or folder into .trash/ of the shared root (nothing is ever really deleted; a human empties the trash).",
            _ => "",
        }
        .into()
    }

    fn parameters(&self) -> Value {
        let p = json!({"type": "string", "description": PATH});
        match self.name {
            "files_list" => json!({"type": "object", "properties": {
                "path": p, "hidden": {"type": "boolean"}}, "required": []}),
            "files_read" => json!({"type": "object", "properties": {
                "path": p,
                "mode": {"type": "string", "enum": ["auto", "text", "hex", "archive"]},
                "offset": {"type": "integer", "minimum": 0},
                "max_bytes": {"type": "integer", "minimum": 1}}, "required": ["path"]}),
            "files_info" => {
                json!({"type": "object", "properties": {"path": p}, "required": ["path"]})
            }
            "files_kinds" => json!({"type": "object", "properties": {}, "required": []}),
            "files_write" => json!({"type": "object", "properties": {
                "path": p, "content": {"type": "string"},
                "version": {"type": "string", "description": "version token from files_read"},
                "create": {"type": "boolean"}}, "required": ["path", "content"]}),
            "files_mkdir" => json!({"type": "object", "properties": {
                "dir": p, "name": {"type": "string"}}, "required": ["dir", "name"]}),
            "files_rename" => json!({"type": "object", "properties": {
                "path": p, "new_name": {"type": "string"}}, "required": ["path", "new_name"]}),
            "files_move" => json!({"type": "object", "properties": {
                "path": p, "dest_dir": p, "copy": {"type": "boolean"}}, "required": ["path", "dest_dir"]}),
            "files_extract" => json!({"type": "object", "properties": {
                "path": p, "dest_dir": p}, "required": ["path"]}),
            "files_trash" => {
                json!({"type": "object", "properties": {"path": p}, "required": ["path"]})
            }
            _ => json!({"type": "object", "properties": {}}),
        }
    }

    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.sh;
        let r = &sh.root;
        let max_read = sh.cfg.max_read_bytes;
        match self.name {
            "files_list" => sh.run(self.name, &["path", "hidden"], args, ctx, |a| {
                r.list(s(a, "path"), a["hidden"].as_bool().unwrap_or(false))
            }),
            "files_read" => sh.run(
                self.name,
                &["path", "mode", "offset", "max_bytes"],
                args,
                ctx,
                |a| {
                    let path = s(a, "path");
                    if secret_name(path) {
                        return Err(galion_files::Error::Bad(
                            "this file name usually holds secrets; ask a human to open it".into(),
                        ));
                    }
                    let max = a["max_bytes"].as_u64().unwrap_or(max_read).min(max_read);
                    let off = a["offset"].as_u64().unwrap_or(0);
                    match a.get("mode").and_then(Value::as_str).unwrap_or("auto") {
                        "text" => r.read_text(path, max),
                        "hex" => r.hex(path, off, max.min(64 << 10)),
                        "archive" => r.archive_list(path),
                        "auto" => match r.read_text(path, max) {
                            Err(galion_files::Error::Unsupported(_)) => {
                                r.hex(path, off, max.min(4096))
                            }
                            other => other,
                        },
                        m => Err(galion_files::Error::Bad(format!("unknown mode {m}"))),
                    }
                },
            ),
            "files_info" => sh.run(self.name, &["path"], args, ctx, |a| r.info(s(a, "path"))),
            "files_kinds" => sh.run(self.name, &[], args, ctx, |_| Ok(kinds::table_json())),
            "files_write" => sh.run(
                self.name,
                &["path", "content", "version", "create"],
                args,
                ctx,
                |a| {
                    let Some(content) = a.get("content").and_then(Value::as_str) else {
                        return Err(galion_files::Error::Bad("content must be a string".into()));
                    };
                    let create = a["create"].as_bool().unwrap_or(false);
                    let version = a.get("version").and_then(Value::as_str);
                    if !create && version.is_none() {
                        return Err(galion_files::Error::Bad(
                            "pass the version from files_read (or create=true for a new file)"
                                .into(),
                        ));
                    }
                    r.write_text(s(a, "path"), content, version, create)
                },
            ),
            "files_mkdir" => sh.run(self.name, &["dir", "name"], args, ctx, |a| {
                r.mkdir(s(a, "dir"), s(a, "name"))
            }),
            "files_rename" => sh.run(self.name, &["path", "new_name"], args, ctx, |a| {
                r.rename(s(a, "path"), s(a, "new_name"))
            }),
            "files_move" => sh.run(self.name, &["path", "dest_dir", "copy"], args, ctx, |a| {
                if a["copy"].as_bool().unwrap_or(false) {
                    r.copy_to(s(a, "path"), s(a, "dest_dir"))
                } else {
                    r.move_to(s(a, "path"), s(a, "dest_dir"))
                }
            }),
            "files_extract" => sh.run(self.name, &["path", "dest_dir"], args, ctx, |a| {
                let dest = a.get("dest_dir").and_then(Value::as_str);
                r.extract(s(a, "path"), dest, sh.cfg.extract_max_bytes)
            }),
            "files_trash" => sh.run(self.name, &["path"], args, ctx, |a| r.trash(s(a, "path"))),
            _ => ToolOutput::err("unknown files tool"),
        }
    }
}
