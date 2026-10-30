// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Full-write mode (opt-in, `HERMES_RS_GUARD_FULL_WRITE=1`, 2026-10-30).
//!
//! Inside Hermes's own workspace the agent works like a coding assistant:
//! it may write, create, delete and rename files, run interpreters, scripts,
//! `make`, `sed -i`, `find -delete`, wrapped commands (`timeout`, `xargs` ...)
//! and local git history edits. The terminal itself is still the
//! `openmind_sandbox` one (no network, read-only system, only the workspace
//! writable, secrets hidden, runs as nobody), so this layer only decides
//! what the deny list lets through; the sandbox stays the hard boundary.
//!
//! What stays refused in every mode (the "never" list):
//! * secrets and tokens (S6-secret: canonical paths, secret names, the prior
//!   floor's secret words) and `/etc`, `/boot`;
//! * the owner-locked files SOUL.md and config.yaml (and the other agent
//!   steering files, the guard itself and the self-modify lock);
//! * other services' files: `/opt`, `/srv` outside the workspace, `/var/lib`,
//!   `/var/backups`, `/root`, `/home`, `/run`, devices;
//! * the vault's existing notes (new `.md` notes only, through a vault
//!   create tool);
//! * service and system control (`systemctl`, `service`, `kill`, packages,
//!   mounts, firewall ...), privilege changers (`sudo`, `su` ...),
//!   container / cluster / database clients, network clients;
//! * flashing devices (`flashrom`, `fastboot`, `adb`, `dd of=/dev/...` ...);
//! * whole-system damage (`rm -rf /`, writes outside the workspace, `rm` of
//!   the workspace root itself);
//! * the undo store (`.hermes-trash`): read and copy back only.
//!
//! Every delete or overwrite keeps a recoverable copy first (see
//! [`super::undo`]); a call whose undo copy can not be made is refused.

use super::rules::*;
use super::*;

/// Opaque text: interpreter code, scripts and shell the strict lexer can
/// not follow (loops, `$VAR`, `$( )`).
mod text;
#[cfg(test)]
pub(crate) use text::OTHER_SERVICE_ROOTS;
pub(crate) use text::{is_fallback, shell_scan};

pub(super) use super::undo::{in_trash, unwrap_wrapper};

/// Wrappers that may run another command in full-write mode: the wrapped
/// command is unwrapped and checked like any other command.
pub const FULL_WRITE_WRAPPERS: &[&str] = &[
    "timeout", "nice", "ionice", "nohup", "stdbuf", "time", "xargs", "command", "env",
];

/// Device flashing / firmware tools: refused in every mode.
pub const FLASH_TOOLS: &[&str] = &[
    "flashrom",
    "fastboot",
    "adb",
    "dfu-util",
    "esptool",
    "esptool.py",
    "avrdude",
    "heimdall",
    "odin4",
    "rkdeveloptool",
    "edl",
    "qdl",
    "nandwrite",
    "flash_erase",
    "flash_eraseall",
    "flashcp",
    "mtd",
    "mtd_debug",
    "ubiformat",
    "bmaptool",
    "balena-etcher",
    "etcher",
    "picotool",
    "stm32flash",
    "st-flash",
    "openocd",
    "uuu",
    "sunxi-fel",
    "mmc",
    "fwupdmgr",
    "fwupdtool",
    "efibootmgr",
    "grub-install",
    "nvme",
];

/// Variables that change how programs load code or find commands: never
/// set by opaque shell text.
const LOADER_VARS: &[&str] = &[
    "PATH",
    "IFS",
    "ENV",
    "BASH_ENV",
    "PS4",
    "PROMPT_COMMAND",
    "SHELLOPTS",
    "BASHOPTS",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PYTHONHOME",
    "PERL5LIB",
    "PERL5OPT",
    "PERLLIB",
    "RUBYLIB",
    "RUBYOPT",
    "NODE_OPTIONS",
    "NODE_PATH",
    "GCONV_PATH",
    "RUSTC_WRAPPER",
    "RUSTC",
    "CARGO_HOME",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "HOME",
    "XDG_CONFIG_HOME",
    "TMPDIR",
    "EDITOR",
    "VISUAL",
    "PAGER",
    "SHELL",
];

/// Owner-locked and agent-steering file names: never written in any mode,
/// and never named by code the deny list can not parse.
pub const LOCKED_NAMES: &[&str] = &[
    "SOUL.md",
    "config.yaml",
    "AGENTS.md",
    "USER.md",
    "guard.json",
    "dangerous-patterns.txt",
    "hermes-self-modify.json",
];

fn is_flash(n: &str) -> bool {
    FLASH_TOOLS.contains(&n)
}

/// S8-flash: refused in every mode.
pub(super) fn flash_check(n: &str) -> R {
    if is_flash(n) {
        return Err(deny(
            "S8-flash",
            format!("{n} flashes or re-partitions devices"),
        ));
    }
    Ok(())
}

/// Write targets the owner locked or that steer the agent (full-write mode
/// adds config.yaml and the self-modify lock to the strict list).
pub(super) fn locked_name(name: &str) -> bool {
    LOCKED_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Maximum nesting of scripts, wrappers and `find -exec` checked.
pub const MAX_NEST: usize = 4;

/// Extra git subcommands in full-write mode (local history and files;
/// push / fetch / pull / clone / config / submodule / gc / prune / reflog /
/// filter-branch / worktree stay refused, as do `-c`, `--exec`, hooks).
pub const FULL_WRITE_GIT: &[&str] = &[
    "init",
    "checkout",
    "reset",
    "rm",
    "mv",
    "apply",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "clean",
    "am",
];

/// Git subcommands whose path operands are files git deletes, moves or
/// overwrites (write-checked).
pub const GIT_WRITES_PATHS: &[&str] = &["rm", "mv", "checkout", "restore", "clean", "reset"];

/// Extra cargo subcommands in full-write mode.
pub const FULL_WRITE_CARGO: &[&str] = &["run", "new", "init", "clean", "fix"];

/// A tool name that creates a vault note (and does not edit, move or
/// delete one).
pub(super) fn vault_create_tool(tool: &str) -> bool {
    let toks = tokens(tool);
    let creates = toks
        .iter()
        .any(|t| matches!(t.as_str(), "create" | "new" | "add" | "save" | "write"));
    let changes = toks.iter().any(|t| {
        matches!(
            t.as_str(),
            "edit"
                | "delete"
                | "remove"
                | "rm"
                | "move"
                | "mv"
                | "rename"
                | "append"
                | "prepend"
                | "replace"
                | "patch"
                | "update"
                | "overwrite"
                | "upsert"
                | "set"
                | "put"
                | "modify"
                | "change"
                | "merge"
                | "trash"
                | "archive"
        )
    });
    creates && !changes
}

/// Full-write: a vault create call must name the new note by an absolute
/// path inside a vault root (a bare title or a relative path could land on
/// an existing note the guard can not see).
pub(super) fn vault_create_args(args: &Value, pol: &Policy) -> R {
    let mut paths = Vec::new();
    collect_paths(args, &mut paths, 0);
    let in_vault: Vec<&String> = paths
        .iter()
        .filter(|p| {
            p.starts_with('/')
                && pol.vaults().iter().any(|v| {
                    under(&physical(Path::new(p.as_str())), v) || under(Path::new(p.as_str()), v)
                })
        })
        .collect();
    if in_vault.is_empty() {
        return Err(deny(
            "S9-vault-write",
            "a new vault note must be named by its absolute path inside the vault, so the guard can check it does not exist yet",
        ));
    }
    Ok(())
}

fn collect_paths(v: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if let (true, Value::String(s)) = (key_in(k, PATH_KEYS), x) {
                    out.push(s.clone());
                } else {
                    collect_paths(x, out, depth + 1);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_paths(x, out, depth + 1)),
        _ => {}
    }
}

/// Full-write: a new note in the vault. Allowed only for a `.md` file that
/// does not exist yet, under a vault root, with no hidden or protected
/// directory between the root and the note; anything else in the vault
/// stays read-only.
pub(super) fn vault_new_note(raw: &str, cwd: &Path, pol: &Policy) -> R {
    let lex = lexical(raw, cwd, &pol.home)?;
    let phys = physical(&lex);
    let in_vault = |p: &Path| pol.vaults().iter().any(|v| under(p, v) && p != v.as_path());
    if !in_vault(&lex) && !in_vault(&phys) {
        // not the vault: an ordinary write
        return check_path_x(raw, Access::Write, cwd, pol, true);
    }
    if !in_vault(&lex) || !in_vault(&phys) {
        return Err(deny(
            "S9-vault-write",
            format!("{} leaves the vault through a link", lex.display()),
        ));
    }
    check_path_x(raw, Access::Read, cwd, pol, true)?;
    for p in [&lex, &phys] {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.to_ascii_lowercase().ends_with(".md") || name.len() <= 3 {
            return Err(deny(
                "S9-vault-write",
                format!("{name}: only new .md notes may be added to the vault"),
            ));
        }
        if locked_name(&name) || NO_WRITE_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            return Err(deny(
                "S6-no-write",
                format!("{name} is locked by the owner"),
            ));
        }
        // only the part below the vault root: the root itself may sit under
        // a hidden directory (a temp dir, `~/.local/...`)
        let below = pol
            .vaults()
            .iter()
            .filter(|v| under(p, v))
            .filter_map(|v| p.strip_prefix(v).ok().map(Path::to_path_buf))
            .collect::<Vec<_>>();
        for c in below.iter().flat_map(|r| r.components()) {
            if let Component::Normal(n) = c {
                let n = n.to_string_lossy();
                if n.starts_with('.') || NO_WRITE_DIRS.iter().any(|d| *d == n) {
                    return Err(deny(
                        "S9-vault-write",
                        format!(
                            "{} is inside a hidden or protected vault folder",
                            p.display()
                        ),
                    ));
                }
            }
        }
        if p.symlink_metadata().is_ok() {
            return Err(deny(
                "S9-vault-write",
                format!(
                    "{} already exists; existing vault notes are read-only (new notes only)",
                    p.display()
                ),
            ));
        }
    }
    Ok(())
}

/// Simple glob match of one path component (`*`, `?`, `[...]`).
fn glob_match(pat: &[char], name: &[char]) -> bool {
    match (pat.first(), name.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            glob_match(&pat[1..], name) || (!name.is_empty() && glob_match(pat, &name[1..]))
        }
        (Some('?'), Some(_)) => glob_match(&pat[1..], &name[1..]),
        (Some('['), Some(c)) => {
            let Some(end) = pat.iter().skip(1).position(|x| *x == ']').map(|e| e + 1) else {
                return false;
            };
            let set = &pat[1..end];
            let (neg, set) = match set.first() {
                Some('!') | Some('^') => (true, &set[1..]),
                _ => (false, set),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == '-' {
                    if set[i] <= *c && *c <= set[i + 2] {
                        hit = true;
                    }
                    i += 3;
                } else {
                    if set[i] == *c {
                        hit = true;
                    }
                    i += 1;
                }
            }
            hit != neg && glob_match(&pat[end + 1..], &name[1..])
        }
        (Some(p), Some(n)) if p == n => glob_match(&pat[1..], &name[1..]),
        _ => false,
    }
}

impl Cmd<'_> {
    /// Full-write: a relative write glob (`rm *.o`, `mv src/*.rs old/`): the
    /// glob may only be in the last component, may not start with a dot,
    /// and every file it matches now is write-checked (so it can not reach
    /// a locked file or the undo store).
    pub(super) fn fw_write_glob(&self, t: &str) -> R {
        let (dir, pat) = match t.rsplit_once('/') {
            Some((d, p)) => (if d.is_empty() { "/" } else { d }, p),
            None => (".", t),
        };
        if dir.contains(['*', '?', '[']) {
            return Err(deny(
                "S2-glob-path",
                format!("write glob with a wildcard directory: {t}"),
            ));
        }
        if pat.starts_with('.') || pat.starts_with('[') {
            return Err(deny(
                "S2-glob-path",
                format!("write glob that can match hidden files (.git, .hermes-trash): {t}"),
            ));
        }
        self.path(dir, Access::Write)?;
        let lex = lexical(dir, &self.cwd, &self.pol.home)?;
        let pc: Vec<char> = pat.chars().collect();
        let mut n = 0;
        if let Ok(rd) = std::fs::read_dir(physical(&lex)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let nc: Vec<char> = name.chars().collect();
                if glob_match(&pc, &nc) {
                    n += 1;
                    if n > 10_000 {
                        return Err(deny("S9-size", format!("{t} matches too many files")));
                    }
                    check_path_x(
                        &lex.join(&name).to_string_lossy(),
                        Access::Write,
                        &self.cwd,
                        self.pol,
                        true,
                    )?;
                    if self
                        .pol
                        .write_roots
                        .iter()
                        .any(|w| under(w, &lex.join(&name)))
                    {
                        return Err(deny("S6-rm-root", "write glob matches the workspace root"));
                    }
                }
            }
        }
        Ok(())
    }

    fn sub(&self) -> Result<Cmd<'_>, Deny> {
        if self.depth >= MAX_NEST {
            return Err(deny("S9-depth", "commands nested too deeply to check"));
        }
        Ok(Cmd {
            pol: self.pol,
            cwd: self.cwd.clone(),
            depth: self.depth + 1,
        })
    }

    /// Full-write: `timeout`, `nice`, `xargs`, `env` ... run the wrapped
    /// command, which gets the same checks as if it ran on its own.
    pub(super) fn fw_wrapper(&mut self, n: &str, args: &[&Word]) -> R {
        let inner = unwrap_wrapper(n, args)?;
        let Some(inner) = inner else {
            if n == "env" {
                return Err(deny(
                    "S5-env-dump",
                    "env with no command prints the environment (provider keys live there)",
                ));
            }
            return Ok(());
        };
        let mut sub = self.sub()?;
        let seg = Segment {
            words: inner.iter().map(|w| (*w).clone()).collect(),
            redirs: Vec::new(),
        };
        sub.segment(&seg, false)
            .map_err(|e| deny(&e.rule, format!("{n} runs: {}", e.reason)))
    }

    /// Full-write: `find` may `-delete` and `-exec` inside the workspace.
    /// Start paths are write-checked when find deletes or runs commands;
    /// each `-exec` command gets the full check; `-fprint` files are writes.
    pub(super) fn fw_find(&mut self, args: &[&Word]) -> R {
        let acts = args.iter().any(|a| {
            matches!(
                a.text.as_str(),
                "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir"
            )
        });
        let starts: Vec<&&Word> = args
            .iter()
            .take_while(|a| !a.text.starts_with('-') && a.text != "(" && a.text != "!")
            .collect();
        let access = if acts { Access::Write } else { Access::Read };
        if starts.is_empty() {
            self.path(".", access)?;
        }
        for s in &starts {
            self.word_path(s, access)?;
        }
        let mut i = starts.len();
        while i < args.len() {
            let t = args[i].text.as_str();
            match t {
                "-exec" | "-execdir" | "-ok" | "-okdir" => {
                    let mut j = i + 1;
                    let mut inner: Vec<Word> = Vec::new();
                    while j < args.len() && args[j].text != ";" && args[j].text != "+" {
                        if args[j].text != "{}" {
                            inner.push(args[j].clone());
                        }
                        j += 1;
                    }
                    if inner.is_empty() {
                        return Err(deny("S4-find-exec", "find -exec with no command"));
                    }
                    let mut sub = self.sub()?;
                    sub.segment(
                        &Segment {
                            words: inner,
                            redirs: Vec::new(),
                        },
                        false,
                    )
                    .map_err(|e| deny(&e.rule, format!("find {t} runs: {}", e.reason)))?;
                    i = j + 1;
                    continue;
                }
                "-fprint" | "-fprint0" | "-fls" | "-fprintf" => {
                    let Some(f) = args.get(i + 1) else {
                        return Err(deny("S4-find-exec", format!("find {t} with no file")));
                    };
                    self.word_path(f, Access::Write)?;
                    i += if t == "-fprintf" { 3 } else { 2 };
                    continue;
                }
                _ => {
                    if !t.starts_with('-') && (looks_like_path(t) || args[i].glob) && !args[i].glob
                    {
                        self.word_path(args[i], Access::Read)?;
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }
}

pub(super) fn loader_var(name: &str) -> bool {
    LOADER_VARS.contains(&name)
        || name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || name.starts_with("GIT_")
        || name.starts_with("BASH_FUNC")
}
