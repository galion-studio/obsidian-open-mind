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

/// Interpreters that stay refused in full-write mode: they reach other
/// services (containers, clusters, databases), hide the real command
/// (multi-call binaries) or keep processes alive past the call.
pub const FULL_WRITE_STILL_BLOCKED: &[&str] = &[
    "docker",
    "podman",
    "nerdctl",
    "kubectl",
    "helm",
    "ansible",
    "ansible-playbook",
    "terraform",
    "psql",
    "mysql",
    "redis-cli",
    "mongo",
    "mongosh",
    "busybox",
    "toybox",
    "screen",
    "tmux",
    "expect",
    "osascript",
    "pwsh",
    "powershell",
    "jq-shell",
];

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

/// Command words refused when they appear anywhere in code the deny list
/// can not parse (inline interpreter code, scripts, Makefiles): service and
/// system control, privilege changers, other services' clients, flashing.
/// Kept to words that are not ordinary identifiers in code.
pub const OPAQUE_NEVER_WORDS: &[&str] = &[
    "systemctl",
    "journalctl",
    "shutdown",
    "reboot",
    "poweroff",
    "telinit",
    "iptables",
    "ip6tables",
    "nft",
    "ufw",
    "sysctl",
    "modprobe",
    "insmod",
    "rmmod",
    "kexec",
    "mount",
    "umount",
    "useradd",
    "userdel",
    "usermod",
    "groupadd",
    "chpasswd",
    "visudo",
    "apt-get",
    "dpkg",
    "yum",
    "dnf",
    "fdisk",
    "parted",
    "sfdisk",
    "wipefs",
    "blkdiscard",
    "hdparm",
    "mkswap",
    "dmsetup",
    "cryptsetup",
    "losetup",
    "debugfs",
    "loginctl",
    "machinectl",
    "hostnamectl",
    "timedatectl",
    "dbus-send",
    "busctl",
    "fail2ban-client",
    "galion-guard",
    "hermes-gateway",
    "hermes-gate",
    "sudo",
    "doas",
    "pkexec",
    "runuser",
    "nsenter",
    "chroot",
    "unshare",
    "systemd-run",
    "crontab",
    "docker",
    "podman",
    "kubectl",
    "pkill",
    "killall",
    "chattr",
    "setcap",
];

/// Extra words refused in shell text only (ordinary words in other code).
const SHELL_NEVER_WORDS: &[&str] = &[
    "service",
    "su",
    "kill",
    "eval",
    "source",
    "halt",
    "passwd",
    "apt",
    "snap",
    "ldconfig",
    // network clients (the unambiguous names; the rest are caught in
    // command position by the strict check of each piece)
    "curl",
    "wget",
    "wget2",
    "aria2c",
    "nc",
    "ncat",
    "netcat",
    "socat",
    "ssh",
    "scp",
    "sftp",
    "ftp",
    "lftp",
    "tftp",
    "telnet",
    "dig",
    "nslookup",
    "drill",
    "ping",
    "ping6",
    "traceroute",
    "tracepath",
    "mtr",
    "gnutls-cli",
    "websocat",
    "smbclient",
    "sendmail",
    "mutt",
    "swaks",
    "tailscale",
    "cloudflared",
    "ngrok",
    "nmap",
    "masscan",
    "ifconfig",
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

/// Python modules that open servers or reach the network.
const NETWORK_MODULES: &[&str] = &[
    "http.server",
    "socketserver",
    "smtpd",
    "smtplib",
    "ftplib",
    "telnetlib",
    "xmlrpc.server",
    "pip",
    "ensurepip",
    "venv",
    "pydoc",
    "asyncio",
    "aiosmtpd",
    "webbrowser",
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

/// Roots that belong to other services (or the system) and may not be
/// named by code the deny list can not parse. The workspace itself and
/// read-only system code (`/usr`, `/bin`, `/lib`) are fine.
pub(super) const OTHER_SERVICE_ROOTS: &[&str] = &[
    "/opt",
    "/srv",
    "/var/lib",
    "/var/backups",
    "/var/run",
    "/var/spool",
    "/root",
    "/home",
    "/run",
    "/workspace",
    "/sys",
    "/boot",
    "/etc",
    "/mnt",
    "/media",
    "/snap",
];

/// Device files code may name (everything else under /dev is refused).
const DEV_OK: &[&str] = &[
    "/dev/null",
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/stdin",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
];

/// Largest script / program file scanned before it may run.
pub const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;

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

/// The undo store: written only by the guard itself.
pub(super) fn in_trash(p: &Path, pol: &Policy) -> bool {
    pol.trash_dir().is_some_and(|t| under(p, &t))
}

fn word_split(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_ascii_alphanumeric() || "_-.".contains(c)))
        .filter(|t| !t.is_empty())
}

/// Quote and escape characters removed: `sys''tem\ctl` -> `systemctl`,
/// and string concatenation folded: `'sys' 'temctl'`, `"/e" + "tc"`.
fn dequoted(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        if "'\"`".contains(c[i]) {
            // a closing quote, then only spaces / `+` / `.` / `,`?, then an
            // opening quote: one string
            let mut j = i + 1;
            while j < c.len() && (c[j] == ' ' || c[j] == '+' || c[j] == '\t') {
                j += 1;
            }
            if j > i + 1 && j < c.len() && "'\"".contains(c[j]) {
                i = j + 1;
                continue;
            }
            i += 1;
            continue;
        }
        if c[i] == '\\' {
            i += 1;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

fn never_words(text: &str, shell: bool) -> R {
    let folded = dequoted(text);
    for src in [text, folded.as_str()] {
        for t in word_split(src) {
            let l = t.to_ascii_lowercase();
            let l = l.trim_matches(|c| c == '.' || c == '-');
            if OPAQUE_NEVER_WORDS.contains(&l)
                || is_flash(l)
                || (l.starts_with("mkfs") && (l.len() == 4 || l[4..].starts_with('.')))
                || (l.starts_with("systemd-") && l != "systemd-")
                || (shell && SHELL_NEVER_WORDS.contains(&l))
                || (shell && DECODERS.contains(&l))
            {
                return Err(deny(
                    "S8-system",
                    format!("code names {l}, which controls services, privileges, devices or the system"),
                ));
            }
        }
    }
    Ok(())
}

/// String literals in code ('...' or "..."), escapes kept as written.
fn literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let c: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let q = c[i];
        if q == '\'' || q == '"' {
            let mut j = i + 1;
            let mut s = String::new();
            while j < c.len() && c[j] != q && c[j] != '\n' {
                if c[j] == '\\' && j + 1 < c.len() {
                    s.push(c[j]);
                    j += 1;
                }
                s.push(c[j]);
                j += 1;
            }
            out.push(s);
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// One path named by opaque code: refused when it is a secret, /etc,
/// /boot, an alias, a device, the vault or another service's files.
fn opaque_path(raw: &str, cwd: &Path, pol: &Policy) -> R {
    let t = raw.trim_end_matches(['.', ':', '!', '?', ',']);
    let t = t.strip_prefix("file://").unwrap_or(t);
    if t.is_empty() || t.chars().all(|c| c == '/') || t.starts_with("//") {
        return Ok(());
    }
    // a glob or regex: judge its fixed prefix
    let fixed: &str = t
        .split(['*', '?', '[', '$', '{', '(', '^'])
        .next()
        .unwrap_or("");
    if fixed.is_empty() || fixed.chars().all(|c| c == '/') {
        return Ok(());
    }
    let lex = lexical(fixed, cwd, &pol.home)?;
    let phys = physical(&lex);
    for p in [&lex, &phys] {
        if pol.in_write_roots(p) {
            if in_trash(p, pol) {
                // reading undo copies is fine; code may not name them
                // (it could not tell reads from writes)
                return Err(deny(
                    "S6-trash",
                    format!(
                        "{} is the undo store; copy files back with cp, do not touch it from code",
                        p.display()
                    ),
                ));
            }
            continue;
        }
        if is_alias(p) {
            return Err(deny(
                "S6-alias",
                format!("{} is an alias of / or of a process view", p.display()),
            ));
        }
        if let Some(why) = secret_reason(p) {
            return Err(deny("S6-secret", format!("{}: {why}", p.display())));
        }
        if let Some(v) = pol.vaults().iter().find(|v| under(p, v)) {
            return Err(deny(
                "S9-vault-write",
                format!(
                    "{} is inside the vault {} (code can not be checked for writes)",
                    p.display(),
                    v.display()
                ),
            ));
        }
        let ps = p.to_string_lossy();
        if under(p, Path::new("/dev")) && !DEV_OK.contains(&ps.as_ref()) {
            return Err(deny(
                "S8-flash",
                format!("{} is a device file", p.display()),
            ));
        }
        if let Some(r) = OTHER_SERVICE_ROOTS.iter().find(|r| under(p, Path::new(r))) {
            return Err(deny(
                "S6-outside",
                format!(
                    "{} is outside the workspace, under {r} (other services' files)",
                    p.display()
                ),
            ));
        }
    }
    Ok(())
}

fn name_check(tok: &str) -> R {
    let base = tok.rsplit('/').next().unwrap_or(tok);
    if base.is_empty() {
        return Ok(());
    }
    if tok.split('/').any(|c| c == super::undo::TRASH_NAME) {
        return Err(deny(
            "S6-trash",
            "code that names the undo store can not be checked; copy files back with cp",
        ));
    }
    if locked_name(base) {
        return Err(deny(
            "S6-no-write",
            format!("{base} is locked by the owner (code that names it can not be checked)"),
        ));
    }
    let p = Path::new(base);
    if let Some(why) = secret_reason(p) {
        return Err(deny("S6-secret", why));
    }
    Ok(())
}

/// Scan code the deny list can not parse (inline interpreter code, a script
/// or program file, a Makefile). `bare` = also judge unquoted tokens as file
/// names (shell-like text); otherwise only string literals are names.
pub(super) fn code_scan(text: &str, cwd: &Path, pol: &Policy, shell: bool, bare: bool) -> R {
    if text.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(deny("S9-size", "code too long to check"));
    }
    if let Some(c) = text
        .chars()
        .find(|c| shell::suspicious_char(*c) && *c != '\u{feff}')
    {
        return Err(deny(
            "S2-unicode",
            format!("invisible or look-alike character U+{:04X}", c as u32),
        ));
    }
    // the prior floor always; the prior shell list on shell text
    prior_check(text, true, shell)?;
    never_words(text, shell)?;
    for p in text_paths(text) {
        opaque_path(&p, cwd, pol)
            .map_err(|e| deny(&e.rule, format!("path in code: {}", e.reason)))?;
    }
    for lit in literals(text) {
        if (lit.starts_with('/') || lit.starts_with("~/") || lit.contains('/'))
            && !lit.contains(char::is_whitespace)
            && !lit.contains("://")
        {
            opaque_path(&lit, cwd, pol)
                .map_err(|e| deny(&e.rule, format!("path in code: {}", e.reason)))?;
        }
        if !lit.contains(char::is_whitespace) && !lit.is_empty() {
            name_check(&lit)?;
        }
        if lit.contains("://") {
            for t in lit.split_whitespace() {
                if t.contains("://") {
                    check_url(t, pol, false)?;
                }
            }
        }
    }
    if bare {
        for t in text.split(|c: char| c.is_whitespace() || "\"'`;|&()<>=,".contains(c)) {
            if t.contains('.') || t.contains('/') {
                name_check(t)?;
            }
        }
    }
    for d in decoded_variants_all(text) {
        if code_scan_inner(&d, cwd, pol).is_err() {
            return Err(deny("S3-encoded", "decoded payload in code is refused"));
        }
    }
    Ok(())
}

/// Embedded base64 / hex blobs in code, decoded.
fn decoded_variants_all(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in text.split(|c: char| c.is_whitespace() || "\"'`;|&()<>,".contains(c)) {
        if t.len() >= 12 {
            out.extend(decoded_variants(t));
        }
        if out.len() > 64 {
            break;
        }
    }
    out
}

fn code_scan_inner(d: &str, cwd: &Path, pol: &Policy) -> R {
    prior_check(d, true, true)?;
    never_words(d, true)?;
    for p in text_paths(d) {
        opaque_path(&p, cwd, pol)?;
    }
    Ok(())
}

/// Shell text the lexer refuses (loops, `$VAR`, `$( )`, here-docs) or a
/// shell script: run as opaque code, plus the shell-only rules that keep
/// encoding and dynamic-command escapes out: no ANSI-C `$'..'`, no `\x`/
/// octal escapes, no command word built from an expansion, no `eval`, no
/// `-c "$VAR"`, no pipe into a shell.
pub(super) fn shell_scan(text: &str, cwd: &Path, pol: &Policy, depth: usize) -> R {
    if depth > MAX_NEST {
        return Err(deny("S9-depth", "scripts nested too deeply to check"));
    }
    code_scan(text, cwd, pol, true, true)?;
    for p in text_paths(&dequoted(text)) {
        opaque_path(&p, cwd, pol)
            .map_err(|e| deny(&e.rule, format!("path in code: {}", e.reason)))?;
    }
    shell_words(text, cwd, pol)?;
    if text.contains("$'") {
        return Err(deny("S3-escape", "ANSI-C quoting $'..' builds hidden text"));
    }
    let b = text.as_bytes();
    for i in 0..b.len().saturating_sub(1) {
        if b[i] == b'\\' {
            let n = b[i + 1];
            let octal = n.is_ascii_digit()
                && b.get(i + 2).is_some_and(|c| c.is_ascii_digit())
                && b.get(i + 3).is_some_and(|c| c.is_ascii_digit());
            if n == b'x' || n == b'u' || n == b'U' || octal {
                return Err(deny(
                    "S3-escape",
                    "hex / unicode / octal escapes build hidden text",
                ));
            }
        }
    }
    for line in text.split(['\n', ';', '|', '&']) {
        let mut toks = line.split_whitespace().peekable();
        // skip keywords, assignments and wrappers to the command word
        let mut cmd: Option<&str> = None;
        let mut prev: Option<&str> = None;
        for t in toks.by_ref() {
            let tl = t.trim_start_matches(['(', '{', '!']);
            if tl.is_empty()
                || matches!(
                    tl,
                    "then"
                        | "do"
                        | "else"
                        | "elif"
                        | "if"
                        | "while"
                        | "until"
                        | "time"
                        | "exec"
                        | "command"
                        | "builtin"
                        | "nohup"
                        | "nice"
                        | "xargs"
                        | "env"
                        | "timeout"
                        | "stdbuf"
                )
                || (tl.contains('=') && !tl.starts_with('=') && !tl.starts_with('-'))
                || (prev == Some("timeout")
                    && tl.chars().next().is_some_and(|c| c.is_ascii_digit()))
                || (tl.starts_with('-') && prev.is_some_and(|p| FULL_WRITE_WRAPPERS.contains(&p)))
            {
                if !tl.is_empty() && !tl.contains('=') {
                    prev = Some(tl);
                }
                continue;
            }
            cmd = Some(tl);
            break;
        }
        let Some(c) = cmd else { continue };
        if c.starts_with('$') || c.starts_with("\"$") || c.starts_with("'$") || c.starts_with('`') {
            return Err(deny(
                "S1-dynamic-cmd",
                format!("command word {c} is built at run time, the deny list can not see it"),
            ));
        }
        let rest: Vec<&str> = toks.collect();
        for w in rest.windows(2) {
            if matches!(w[0], "-c" | "-e" | "-E" | "-r" | "--eval")
                && (w[1].starts_with('$') || w[1].starts_with("\"$") || w[1].starts_with('`'))
            {
                return Err(deny(
                    "S1-dynamic-cmd",
                    format!("{c} {} runs code held in a variable", w[0]),
                ));
            }
        }
    }
    // a pipe straight into a shell or interpreter reads an unseen script
    for seg in text.split('|').skip(1) {
        let first = seg
            .split_whitespace()
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("");
        if interp_base(first) && !matches!(first, "awk" | "gawk" | "mawk" | "nawk") {
            return Err(deny(
                "S4-interpreter",
                format!("piping into {first} runs a script the deny list can not see"),
            ));
        }
    }
    loader_vars(text)?;
    fork_bomb(text)?;
    pieces_strict(text, cwd, pol, depth)
}

/// `PATH=...`, `export LD_PRELOAD=...` and friends in opaque shell.
fn loader_vars(text: &str) -> R {
    for t in text.split(|c: char| c.is_whitespace() || ";|&()`\"'".contains(c)) {
        let Some((name, _)) = t.split_once('=') else {
            continue;
        };
        let name = name.trim_end_matches('+');
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        if loader_var(name) {
            return Err(deny(
                "S5-env-assign",
                format!("{name}= changes how programs load or find code"),
            ));
        }
    }
    Ok(())
}

/// A shell function that pipes into itself in the background.
fn fork_bomb(text: &str) -> R {
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut rest = squeezed.as_str();
    while let Some(i) = rest.find("(){") {
        let name: String = rest[..i]
            .chars()
            .rev()
            .take_while(|c| !";&|{}()\n".contains(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let name = name.trim_start_matches("function");
        if !name.is_empty()
            && (squeezed.contains(&format!("{name}|{name}"))
                || squeezed.contains(&format!("{name}&{name}")))
        {
            return Err(deny(
                "S8-system",
                "fork bomb (a function that pipes into itself)",
            ));
        }
        rest = &rest[i + 3..];
    }
    Ok(())
}

/// The command a wrapper runs, with its own options skipped. `None` when
/// the wrapper runs nothing (`command -v x`). Refuses options that change
/// directory, split strings into commands or read commands from files.
pub(super) fn unwrap_wrapper<'w>(
    n: &str,
    args: &'w [&'w Word],
) -> Result<Option<&'w [&'w Word]>, Deny> {
    let mut i = 0;
    let opt_with_value: &[&str] = match n {
        "timeout" => &["-s", "--signal", "-k", "--kill-after"],
        "nice" => &["-n", "--adjustment"],
        "ionice" => &["-c", "-n", "--class", "--classdata"],
        "stdbuf" => &["-i", "-o", "-e", "--input", "--output", "--error"],
        "time" => &["-f", "--format"],
        "xargs" => &[
            "-I",
            "-n",
            "-P",
            "-d",
            "-L",
            "-s",
            "-E",
            "-e",
            "--max-args",
            "--max-procs",
            "--delimiter",
            "--max-lines",
            "--max-chars",
            "--eof",
        ],
        "env" => &["-u", "--unset"],
        _ => &[],
    };
    while i < args.len() {
        let t = args[i].text.as_str();
        if t == "--" {
            i += 1;
            break;
        }
        if n == "env" && is_assignment(args[i]).is_some() {
            break;
        }
        if !t.starts_with('-') || t == "-" {
            break;
        }
        match (n, t) {
            ("env", "-S") | ("env", "-C") | ("env", "--chdir") => {
                return Err(deny("S4-wrapper", format!("env {t} runs a hidden command")))
            }
            (_, t) if t.starts_with("--split-string") || t.starts_with("--chdir") => {
                return Err(deny("S4-wrapper", format!("{n} {t} runs a hidden command")))
            }
            ("xargs", "-a") | ("xargs", "--arg-file") => {
                return Err(deny(
                    "S4-wrapper",
                    "xargs -a reads its arguments from a file the deny list can not see",
                ))
            }
            ("ionice", "-p") | ("ionice", "-P") | ("ionice", "-u") => {
                return Err(deny("S8-system", "ionice on other processes"))
            }
            ("time", "-o") | ("time", "--output") | ("time", "-a") => {
                return Err(deny(
                    "S6-no-write",
                    "time -o writes a file; redirect instead",
                ))
            }
            ("command", "-v") | ("command", "-V") => return Ok(None),
            _ => {}
        }
        if opt_with_value.contains(&t) {
            i += 2;
        } else {
            i += 1;
        }
    }
    if n == "timeout" {
        // the duration
        i += 1;
    }
    Ok(args.get(i..).filter(|r| !r.is_empty()))
}

/// Maximum nesting of scripts, wrappers and `find -exec` checked.
pub const MAX_NEST: usize = 4;

/// Strict-check refusals that only mean "the strict lexer can not follow
/// this shell" (not "this does something refused"): in full-write mode such
/// text gets the script scan instead.
const LEX_FALLBACK: &[&str] = &[
    "S1-subst",
    "S5-expand",
    "S2-brace",
    "S1-heredoc",
    "S1-subshell",
    "S1-compound",
];

pub(super) fn is_fallback(rule: &str) -> bool {
    LEX_FALLBACK.contains(&rule)
}

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

/// Interactive programs: refused (they would hang the terminal or open a
/// shell escape).
const INTERACTIVE: &[&str] = &[
    "vim", "vi", "nvim", "view", "emacs", "nano", "less", "more", "most", "man", "pager", "gdb",
    "lldb", "ghci", "irb", "ipython", "iex", "jshell", "erl",
];

const SHELLS: &[&str] = &[
    "sh", "bash", "dash", "ash", "ksh", "mksh", "zsh", "fish", "csh", "tcsh", "rbash",
];

const COMPILERS: &[&str] = &[
    "gcc", "cc", "clang", "tcc", "rustc", "go", "javac", "java", "dotnet", "ld", "c++", "g++",
    "clang++",
];

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
/// directory on the way; anything else in the vault stays read-only.
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
        for c in p.components() {
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

/// Shell-only word rules for opaque shell text: `cd` / `pushd` must stay in
/// the workspace and may not be built at run time; `ln` may not point at
/// `/` or outside the workspace; relative paths are resolved (existing
/// symlinks followed) and checked like absolute ones.
fn shell_words(text: &str, cwd: &Path, pol: &Policy) -> R {
    for line in text.split(['\n', ';', '|', '&']) {
        let toks: Vec<&str> = line.split_whitespace().collect();
        for (i, t) in toks.iter().enumerate() {
            let t = t.trim_matches(|c| c == '"' || c == '\'');
            if matches!(t, "cd" | "pushd" | "popd") {
                let target = toks
                    .get(i + 1)
                    .map(|x| x.trim_matches(|c| c == '"' || c == '\''))
                    .filter(|x| !matches!(*x, "&&" | "||" | ";" | "-P" | "-L"));
                match target {
                    None if t != "popd" => {
                        return Err(deny("S6-cd", "cd with no directory goes to $HOME"))
                    }
                    Some(d) if d.contains('$') || d.contains('`') || d.contains('*') => {
                        return Err(deny(
                            "S6-cd",
                            format!("cd {d}: a directory built at run time can not be checked"),
                        ))
                    }
                    Some(d) if t != "popd" => {
                        let lex = lexical(d, cwd, &pol.home)?;
                        if !pol.in_write_roots(&lex) || !pol.in_write_roots(&physical(&lex)) {
                            return Err(deny(
                                "S6-cd",
                                format!("cd to {} leaves the workspace", lex.display()),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            if t == "ln" {
                for a in toks[i + 1..].iter().take_while(|a| !a.contains(['$', '`'])) {
                    let a = a.trim_matches(|c| c == '"' || c == '\'');
                    if a.starts_with('-') {
                        continue;
                    }
                    if a.chars().all(|c| c == '/') || a == ".." || a.starts_with("../") {
                        return Err(deny(
                            "S6-outside",
                            format!("ln to {a} links out of the workspace"),
                        ));
                    }
                    opaque_path(a, cwd, pol)?;
                }
            }
            // a relative path with a directory part: resolve it (an
            // existing symlink in the workspace may point outside)
            let tt = t.trim_end_matches([';', ',']);
            if tt.contains('/')
                && !tt.starts_with('/')
                && !tt.starts_with('-')
                && !tt.contains("://")
                && !tt.starts_with('$')
                && !tt.contains('=')
            {
                opaque_path(tt, cwd, pol)?;
            }
        }
    }
    Ok(())
}

fn strip_keywords(piece: &str) -> &str {
    let mut p = piece.trim();
    loop {
        let before = p;
        for kw in [
            "then ", "do ", "else ", "elif ", "if ", "while ", "until ", "! ", "{ ", "time ",
        ] {
            if let Some(r) = p.strip_prefix(kw) {
                p = r.trim_start();
            }
        }
        if p == before {
            return p;
        }
    }
}

/// Every piece of opaque shell text that the strict lexer can parse gets
/// the full strict check (in full-write mode), so the S7 network and S8
/// git / system refusals hold inside loops and scripts too.
fn pieces_strict(text: &str, cwd: &Path, pol: &Policy, depth: usize) -> R {
    // literal values of simple assignments (`x=/`, `d="build"`), used for
    // `$x` below; anything else is an unknown run-time value
    let mut vars: std::collections::BTreeMap<String, Option<String>> = Default::default();
    for raw in text.split(['\n', ';', '|', '&', '(', ')', '`']) {
        for t in strip_keywords(raw).split_whitespace() {
            let t = t.trim_start_matches("export").trim();
            let Some((n, v)) = t.split_once('=') else {
                continue;
            };
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let v = v.trim_matches(|c| c == '"' || c == '\'');
            let known = !v.contains(['$', '`', '\\', '\'', '"', '*', '?']) && !v.is_empty();
            let prev = vars.get(n).cloned().flatten();
            let val = known.then(|| v.to_string());
            // assigned twice with different values: unknown
            let val = match (vars.contains_key(n), prev, val) {
                (false, _, v) => v,
                (true, Some(a), Some(b)) if a == b => Some(a),
                _ => None,
            };
            vars.insert(n.to_string(), val);
        }
    }
    // `( )` and backticks split too, so `$(cmd)` and `(cmd)` are checked
    for raw in text.split(['\n', ';', '|', '&', '(', ')', '`']) {
        let piece = strip_keywords(raw);
        let first = piece.split_whitespace().next().unwrap_or("");
        if piece.is_empty()
            || piece.starts_with('#')
            || matches!(
                first,
                "for"
                    | "case"
                    | "esac"
                    | "select"
                    | "function"
                    | "done"
                    | "fi"
                    | "in"
                    | "}"
                    | "{"
                    | "then"
                    | "do"
                    | "else"
                    | "[["
                    | "]]"
                    | "))"
                    | "(("
            )
            || first.ends_with(')')
            || first.ends_with("()")
        {
            continue;
        }
        let segs = match shell::lex(piece) {
            Ok(s) => s,
            // run-time values ($x, ${x}, "$@") become a plain placeholder
            // word, so the command around them still gets the strict check
            Err(_) => match shell::lex(&placeholders(piece, &vars)) {
                Ok(s) => s,
                Err(_) => continue,
            },
        };
        let mut c = Cmd {
            pol,
            cwd: cwd.to_path_buf(),
            depth: depth + 1,
        };
        for s in &segs {
            match c.segment(s, false) {
                Ok(()) => {}
                Err(e) if is_fallback(&e.rule) => {}
                Err(e) => return Err(deny(&e.rule, format!("in script: {}", e.reason))),
            }
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

    /// Read a file that must resolve inside the workspace (lexically and
    /// after symlinks), up to [`MAX_SCRIPT_BYTES`]. `None` = binary.
    pub(super) fn read_ws_file(&self, w: &Word) -> Result<(PathBuf, Option<String>), Deny> {
        if w.glob {
            return Err(deny(
                "S4-interpreter",
                "a glob script path can not be scanned",
            ));
        }
        let lex = lexical(&w.text, &self.cwd, &self.pol.home)?;
        let phys = physical(&lex);
        for p in [&lex, &phys] {
            if let Some(why) = secret_reason(p) {
                return Err(deny("S6-secret", format!("{}: {why}", p.display())));
            }
            if !self.pol.in_write_roots(p) {
                return Err(deny(
                    "S4-interpreter",
                    format!(
                        "{} is outside the workspace; only scripts and programs inside the workspace are scanned and run",
                        p.display()
                    ),
                ));
            }
            if in_trash(p, self.pol) {
                return Err(deny(
                    "S6-trash",
                    "scripts are not run from the undo store; copy them back first",
                ));
            }
        }
        let md = std::fs::metadata(&phys).map_err(|_| {
            deny(
                "S4-interpreter",
                format!("can not read {} to check it", phys.display()),
            )
        })?;
        if !md.is_file() {
            return Err(deny(
                "S4-interpreter",
                format!("{} is not a file", phys.display()),
            ));
        }
        if md.len() > MAX_SCRIPT_BYTES {
            return Err(deny("S9-size", "script or program too long to check"));
        }
        let bytes = std::fs::read(&phys).map_err(|_| {
            deny(
                "S4-interpreter",
                format!("can not read {} to check it", phys.display()),
            )
        })?;
        if bytes.contains(&0) {
            return Ok((phys, None));
        }
        Ok((phys, Some(String::from_utf8_lossy(&bytes).into_owned())))
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

    /// Shell code from `sh -c` or a script: the strict check first; shell
    /// the strict lexer can not follow gets the script scan.
    pub(super) fn fw_shell_text(&self, text: &str) -> R {
        let mut sub = self.sub()?;
        if text.len() > MAX_COMMAND_BYTES {
            return shell_scan(text, &self.cwd, self.pol, self.depth + 1);
        }
        match sub.all(text) {
            Ok(()) => Ok(()),
            Err(e) if is_fallback(&e.rule) => shell_scan(text, &self.cwd, self.pol, self.depth + 1),
            Err(e) => Err(e),
        }
    }

    /// Path-like arguments of a program are read-checked (secrets, /etc,
    /// aliases); `-o FILE` is a write.
    fn fw_arg_paths(&self, args: &[&Word]) -> R {
        let mut out_next = false;
        for a in args {
            let t = a.text.as_str();
            if out_next {
                self.word_path(a, Access::Write)?;
                out_next = false;
                continue;
            }
            if t == "-o" || t == "--output" || t == "--out-dir" {
                out_next = true;
                continue;
            }
            if let Some(v) = t
                .strip_prefix("--")
                .and_then(|x| x.split_once('='))
                .map(|x| x.1)
            {
                if looks_like_path(v) {
                    self.path(v, Access::Read)?;
                }
                continue;
            }
            if t.starts_with('-') {
                continue;
            }
            if looks_like_path(t) || a.glob {
                self.word_path(a, Access::Read)?;
            } else if !t.is_empty() && !t.contains("://") && (t.contains('.') || t.contains('_')) {
                let p = lexical(t, &self.cwd, &self.pol.home)?;
                if let Some(why) = secret_reason(&p) {
                    return Err(deny("S6-secret", why));
                }
            }
        }
        Ok(())
    }

    /// Full-write: an interpreter, build tool or compiler. Inline code is
    /// scanned; a script / program / Makefile / package.json inside the
    /// workspace is read and scanned before it runs; reading the program
    /// from stdin stays refused (it can not be seen).
    pub(super) fn fw_interp(&self, n: &str, args: &[&Word]) -> R {
        if FULL_WRITE_STILL_BLOCKED.contains(&n) || INTERACTIVE.contains(&n) {
            return Err(deny(
                "S4-interpreter",
                format!("{n} stays blocked in full-write mode (other services, multi-call binaries, interactive or session programs)"),
            ));
        }
        if self.depth >= MAX_NEST {
            return Err(deny("S9-depth", "commands nested too deeply to check"));
        }
        let joined: String = args
            .iter()
            .map(|a| a.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if SHELLS.contains(&n) {
            // -c CODE (also clusters like -ec, -xc)
            if let Some(i) = args.iter().position(|a| {
                let t = a.text.as_str();
                t == "-c"
                    || (t.starts_with('-')
                        && !t.starts_with("--")
                        && t.len() > 1
                        && t.ends_with('c'))
            }) {
                let Some(code) = args.get(i + 1) else {
                    return Err(deny("S4-interpreter", format!("{n} -c with no code")));
                };
                self.fw_shell_text(&code.text)?;
                return self.fw_arg_paths(&args[i + 2..]);
            }
            let Some(script) = shell_script_arg(args) else {
                return Err(deny(
                    "S4-interpreter",
                    format!("{n} with no script file reads its commands from stdin, which the deny list can not see"),
                ));
            };
            let (_p, content) = self.read_ws_file(script)?;
            let Some(content) = content else {
                return Err(deny(
                    "S4-interpreter",
                    "a binary file run as a shell script",
                ));
            };
            self.fw_shell_text(&content)
                .map_err(|e| deny(&e.rule, format!("{n} script {}: {}", script.text, e.reason)))?;
            let at = args
                .iter()
                .position(|a| std::ptr::eq(*a, script))
                .unwrap_or(0);
            return self.fw_arg_paths(&args[at + 1..]);
        }
        if matches!(n, "awk" | "gawk" | "mawk" | "nawk") {
            match self.run_awk(args) {
                Ok(()) => return Ok(()),
                Err(e) if e.rule != "S4-interpreter" => return Err(e),
                Err(_) => {}
            }
            // system() / pipes / redirects: the program is opaque code
            let mut prog = joined.clone();
            let mut it = args.iter();
            while let Some(a) = it.next() {
                if a.text == "-f" {
                    if let Some(f) = it.next() {
                        let (_p, c) = self.read_ws_file(f)?;
                        prog.push('\n');
                        prog.push_str(&c.unwrap_or_default());
                    }
                }
            }
            code_scan(&prog, &self.cwd, self.pol, true, false)?;
            return self.fw_arg_paths(args);
        }
        code_scan(&joined, &self.cwd, self.pol, false, false)?;
        let build_file = |names: &[&str], dir: &Path| -> R {
            for f in names {
                let p = dir.join(f);
                if p.is_file() {
                    let w = Word {
                        text: p.to_string_lossy().to_string(),
                        ..Default::default()
                    };
                    let (_p, c) = self.read_ws_file(&w)?;
                    code_scan(&c.unwrap_or_default(), dir, self.pol, true, true)
                        .map_err(|e| deny(&e.rule, format!("{f}: {}", e.reason)))?;
                }
            }
            Ok(())
        };
        match n {
            "make" | "gmake" => {
                let mut dir = self.cwd.clone();
                let mut files: Vec<&Word> = Vec::new();
                let mut it = args.iter();
                while let Some(a) = it.next() {
                    let t = a.text.as_str();
                    if t == "-C" || t == "--directory" {
                        let Some(d) = it.next() else { break };
                        let lex = lexical(&d.text, &self.cwd, &self.pol.home)?;
                        if !self.pol.in_write_roots(&lex)
                            || !self.pol.in_write_roots(&physical(&lex))
                        {
                            return Err(deny("S6-cd", "make -C outside the workspace"));
                        }
                        dir = physical(&lex);
                    } else if t == "-f" || t == "--file" || t == "--makefile" {
                        if let Some(f) = it.next() {
                            files.push(f);
                        }
                    } else if t == "-I"
                        || t == "--include-dir"
                        || t.starts_with("--eval")
                        || t == "-E"
                    {
                        return Err(deny(
                            "S4-interpreter",
                            format!("make {t} pulls in unseen rules"),
                        ));
                    }
                }
                if files.is_empty() {
                    build_file(&["GNUmakefile", "makefile", "Makefile"], &dir)?;
                }
                for f in files {
                    let (_p, c) = self.read_ws_file(f)?;
                    code_scan(&c.unwrap_or_default(), &dir, self.pol, true, true)?;
                }
                // included makefiles next to it
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.ends_with(".mk") {
                            build_file(&[name.as_str()], &dir)?;
                        }
                    }
                }
                return self.fw_arg_paths(args);
            }
            "cmake" => build_file(&["CMakeLists.txt"], &self.cwd)?,
            "ninja" => build_file(&["build.ninja"], &self.cwd)?,
            "npm" | "npx" | "pnpm" | "yarn" | "bun" | "deno" => {
                build_file(&["package.json", "deno.json"], &self.cwd)?
            }
            "cargo" => {}
            _ => {}
        }
        if COMPILERS.contains(&n)
            || matches!(
                n,
                "cmake"
                    | "ninja"
                    | "npm"
                    | "npx"
                    | "pnpm"
                    | "yarn"
                    | "m4"
                    | "uv"
                    | "uvx"
                    | "pip"
                    | "pip3"
                    | "pipx"
                    | "poetry"
                    | "gem"
                    | "bundle"
                    | "rake"
            )
        {
            // source files given to a compiler / build tool are scanned
            let mut scanned = 0;
            for a in args.iter().filter(|a| !a.text.starts_with('-') && !a.glob) {
                let lex = lexical(&a.text, &self.cwd, &self.pol.home)?;
                if lex.is_file() && self.pol.in_write_roots(&lex) && scanned < 64 {
                    let (_p, c) = self.read_ws_file(a)?;
                    if let Some(c) = c {
                        code_scan(&c, &self.cwd, self.pol, false, false)
                            .map_err(|e| deny(&e.rule, format!("{}: {}", a.text, e.reason)))?;
                    }
                    scanned += 1;
                }
            }
            return self.fw_arg_paths(args);
        }
        // a language interpreter: inline code (already scanned) or a script
        let inline = args.iter().any(|a| {
            let t = a.text.as_str();
            matches!(
                t,
                "-c" | "-e" | "-E" | "-r" | "-p" | "--eval" | "--print" | "-x"
            ) || (t.starts_with('-')
                && !t.starts_with("--")
                && t.len() > 2
                && (t.ends_with('e') || t.ends_with('c')))
        });
        let module = args
            .iter()
            .any(|a| a.text == "-m" || (a.text.starts_with("-m") && !a.text.starts_with("--")));
        for w in args.windows(2) {
            let m = if w[0].text == "-m" {
                Some(w[1].text.as_str())
            } else {
                None
            };
            if let Some(m) = m.or_else(|| {
                w[0].text
                    .strip_prefix("-m")
                    .filter(|x| !x.is_empty() && !x.starts_with('-'))
            }) {
                if NETWORK_MODULES.contains(&m) || m.starts_with("http.") || m.starts_with("pip.") {
                    return Err(deny(
                        "S7-network",
                        format!("{n} -m {m} serves or reaches the network"),
                    ));
                }
            }
        }
        let info_only = !args.is_empty()
            && args.iter().all(|a| {
                matches!(
                    a.text.as_str(),
                    "--version" | "-V" | "-v" | "--help" | "-h" | "-VV"
                )
            });
        if !inline && !module {
            let script = args
                .iter()
                .find(|a| !a.text.starts_with('-') || a.text == "-");
            match script {
                Some(s) if s.text == "-" => {
                    return Err(deny(
                        "S4-interpreter",
                        format!("{n} - reads its program from stdin, which the deny list can not see"),
                    ))
                }
                Some(s) => {
                    let (_p, c) = self.read_ws_file(s)?;
                    let Some(c) = c else {
                        return Err(deny("S4-interpreter", format!("{} is a binary file", s.text)));
                    };
                    code_scan(&c, &self.cwd, self.pol, c.starts_with("#!") && c.lines().next().is_some_and(|l| l.contains("sh")), false)
                        .map_err(|e| deny(&e.rule, format!("{n} script {}: {}", s.text, e.reason)))?;
                }
                None if !info_only => {
                    return Err(deny(
                        "S4-interpreter",
                        format!("{n} with no script reads its program from stdin, which the deny list can not see"),
                    ))
                }
                None => {}
            }
        }
        self.fw_arg_paths(args)
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

    /// Full-write: run a file by path. It must be inside the workspace; a
    /// `#!` script is checked as `<interpreter> <script>`, a text file
    /// without one as a shell script; a compiled program runs as is (the
    /// sandbox is its boundary).
    pub(super) fn fw_path_exec(&mut self, cmd: &Word, args: &[&Word]) -> R {
        let (_p, content) = self.read_ws_file(cmd).map_err(|e| {
            if e.rule == "S4-interpreter" {
                deny(
                    "S4-path-exec",
                    format!("running {} by path: {}", cmd.text, e.reason),
                )
            } else {
                e
            }
        })?;
        let Some(c) = content else {
            // a compiled program built in the workspace
            return self.fw_arg_paths(args);
        };
        if let Some(line) = c.lines().next().and_then(|l| l.strip_prefix("#!")) {
            let mut toks = line.split_whitespace();
            let first = toks.next().unwrap_or("");
            let mut interp = first.rsplit('/').next().unwrap_or("").to_string();
            let mut extra: Vec<String> = toks.map(String::from).collect();
            if interp == "env" {
                if extra.first().is_some_and(|x| x.starts_with('-')) {
                    return Err(deny(
                        "S4-wrapper",
                        "#!/usr/bin/env with options runs a hidden command",
                    ));
                }
                if extra.is_empty() {
                    return Err(deny("S4-path-exec", "#!/usr/bin/env with no program"));
                }
                interp = extra.remove(0);
            }
            let mut words: Vec<Word> = vec![Word {
                text: interp,
                ..Default::default()
            }];
            words.extend(extra.into_iter().map(|t| Word {
                text: t,
                ..Default::default()
            }));
            words.push(cmd.clone());
            words.extend(args.iter().map(|w| (*w).clone()));
            let mut sub = self.sub()?;
            return sub
                .segment(
                    &Segment {
                        words,
                        redirs: Vec::new(),
                    },
                    false,
                )
                .map_err(|e| deny(&e.rule, format!("{}: {}", cmd.text, e.reason)));
        }
        self.fw_shell_text(&c)
            .map_err(|e| deny(&e.rule, format!("{}: {}", cmd.text, e.reason)))?;
        self.fw_arg_paths(args)
    }
}

pub(super) fn loader_var(name: &str) -> bool {
    LOADER_VARS.contains(&name)
        || name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || name.starts_with("GIT_")
        || name.starts_with("BASH_FUNC")
}

/// `$x`, `${x}`, `$1`, `$@` -> `_v` (for the strict check of loop bodies).
fn placeholders(piece: &str, vars: &std::collections::BTreeMap<String, Option<String>>) -> String {
    let val = |n: &str| -> String {
        vars.get(n)
            .cloned()
            .flatten()
            .unwrap_or_else(|| "_v".to_string())
    };
    let c: Vec<char> = piece.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' {
            match c.get(i + 1) {
                Some('{') => {
                    let mut j = i + 2;
                    while j < c.len() && c[j] != '}' {
                        j += 1;
                    }
                    let inner: String = c[i + 2..j.min(c.len())].iter().collect();
                    if inner.chars().all(|x| x.is_ascii_alphanumeric() || x == '_') {
                        out.push_str(&val(&inner));
                    } else {
                        out.push_str("_v");
                    }
                    i = j + 1;
                    continue;
                }
                Some(n) if n.is_ascii_alphabetic() || *n == '_' => {
                    let mut j = i + 1;
                    while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                        j += 1;
                    }
                    let name: String = c[i + 1..j].iter().collect();
                    out.push_str(&val(&name));
                    i = j;
                    continue;
                }
                Some(n) if n.is_ascii_digit() || "@*#?!$-".contains(*n) => {
                    out.push_str("_v");
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}
