// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Rule tables. Layer P = the prior deny list, kept verbatim. Layers S1..S9 =
//! the structural rules added on 2026-10-29 (see docs/hermes-denylist.md).

use super::ere::{Flags, Regex};
use std::sync::OnceLock;

/// The prior Galion list (`/opt/galion/galion-light/guard/dangerous-patterns.txt`,
/// sha256 4b8e6bef...c17665), byte for byte.
pub const PRIOR_PATTERNS: &str = include_str!("prior/dangerous-patterns.txt");
/// Two rules from the upstream Ondrej pack copy that the Galion list lacks.
pub const PRIOR_ONDREJ_EXTRA: &str = include_str!("prior/ondrej-extra.txt");

/// The prior `command-guard` plugin FLOOR (Python `re`), in the same order,
/// with the same flags. Index 6 (`galion-keys|keys.db|gal_sk_`) is only in the
/// galion-light source copy of the plugin, not in the copy Hermes loaded;
/// both are kept.
/// (id, pattern, ignore-case, dot-matches-newline)
pub const PRIOR_FLOOR: &[(&str, &str, bool, bool)] = &[
    ("P-floor-1", r"hermes\s+gateway\s+stop", true, false),
    (
        "P-floor-2",
        r"\b(pkill|killall)\b.{0,80}\b(hermes|origin\.py|keep\.sh|cloudflared|vite)\b",
        true,
        true,
    ),
    ("P-floor-3", r"\bkill\s+-(STOP|CONT|KILL|9)\b", false, false),
    ("P-floor-4", r"\bldconfig\b", false, false),
    ("P-floor-5", r"libsqlite3", false, false),
    (
        "P-floor-6",
        r"galion-light/secrets|galion-secrets|github\.token|galion-app\.token|hermes\.key|OPENROUTER_API_KEY",
        true,
        false,
    ),
    ("P-floor-6b", r"galion-keys|keys\.db|gal_sk_", true, false),
    (
        "P-floor-7",
        r"\brm\b.{0,60}(/workspace\b|keep\.sh|origin\.py)",
        true,
        true,
    ),
    (
        "P-floor-8",
        r"dangerous-patterns\.txt|guard\.json|command-guard",
        false,
        false,
    ),
    ("P-floor-9", r"/etc/|/boot/|docker\.sock", false, false),
    (
        "P-floor-10",
        r"\bchmod\b.{0,40}\b(666|777|o\+rw|a\+w)\b",
        true,
        true,
    ),
    (
        "P-floor-11",
        r"\b(shutdown|reboot|poweroff)\b",
        false,
        false,
    ),
];

/// Floor rules that apply to every string of every tool (secrets + guard
/// disarm), not only to shell and file tools.
pub const FLOOR_ALL_TOOLS: &[&str] = &["P-floor-6", "P-floor-6b", "P-floor-8"];

pub struct Rule {
    pub id: String,
    pub re: Regex,
}

pub struct PriorRules {
    pub list: Vec<Rule>,
    pub floor: Vec<Rule>,
}

/// Parse a dangerous-patterns.txt body the way the prior adapters did:
/// skip blank and `#` lines, `[:space:]` -> `\s`, multi-line mode.
pub fn parse_list(prefix: &str, body: &str) -> Result<Vec<Rule>, String> {
    let mut out = Vec::new();
    for (n, raw) in body.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let src = line.replace("[:space:]", r"\s");
        let re = Regex::new(
            &src,
            Flags {
                multiline: true,
                ..Default::default()
            },
        )
        .map_err(|e| format!("{prefix} line {}: {e}: {line}", n + 1))?;
        out.push(Rule {
            id: format!("{prefix}-L{}", n + 1),
            re,
        });
    }
    Ok(out)
}

pub fn prior() -> &'static PriorRules {
    static P: OnceLock<PriorRules> = OnceLock::new();
    P.get_or_init(|| {
        let mut list = parse_list("P-list", PRIOR_PATTERNS).expect("prior list compiles");
        list.extend(parse_list("P-ondrej", PRIOR_ONDREJ_EXTRA).expect("ondrej extra compiles"));
        let floor = PRIOR_FLOOR
            .iter()
            .map(|(id, p, i, s)| Rule {
                id: id.to_string(),
                re: Regex::new(
                    p,
                    Flags {
                        icase: *i,
                        dotall: *s,
                        multiline: false,
                    },
                )
                .expect("prior floor compiles"),
            })
            .collect();
        PriorRules { list, floor }
    })
}

// ---------------------------------------------------------------- S-layers

/// S4: shells and language interpreters (inline code can not be checked).
pub const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "zsh",
    "ksh",
    "mksh",
    "ash",
    "fish",
    "csh",
    "tcsh",
    "rbash",
    "busybox",
    "toybox",
    "python",
    "python2",
    "python3",
    "pypy",
    "pypy3",
    "ipython",
    "perl",
    "perl5",
    "ruby",
    "irb",
    "node",
    "nodejs",
    "deno",
    "bun",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "expect",
    "rscript",
    "r",
    "julia",
    "osascript",
    "pwsh",
    "powershell",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "jshell",
    "groovy",
    "scala",
    "ghci",
    "runghc",
    "erl",
    "escript",
    "elixir",
    "iex",
    "racket",
    "guile",
    "sbcl",
    "clisp",
    "gdb",
    "lldb",
    "vim",
    "vi",
    "nvim",
    "ex",
    "view",
    "emacs",
    "nano",
    "ed",
    "less",
    "more",
    "most",
    "man",
    "pager",
    "screen",
    "tmux",
    "expect",
    "m4",
    "make",
    "gmake",
    "cmake",
    "ninja",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "pip",
    "pip3",
    "pipx",
    "uv",
    "uvx",
    "poetry",
    "gem",
    "bundle",
    "rake",
    "go",
    "rustc",
    "gcc",
    "cc",
    "clang",
    "tcc",
    "ld",
    "java",
    "javac",
    "dotnet",
    "mono",
    "docker",
    "podman",
    "nerdctl",
    "kubectl",
    "helm",
    "ansible",
    "ansible-playbook",
    "terraform",
    "sqlite3",
    "psql",
    "mysql",
    "redis-cli",
    "mongo",
    "mongosh",
    "jq-shell",
];

/// S4: wrappers that run another program (the wrapped command would escape
/// the per-command checks) and privilege changers.
pub const WRAPPERS: &[&str] = &[
    "env",
    "sudo",
    "su",
    "doas",
    "pkexec",
    "runuser",
    "sg",
    "newgrp",
    "nice",
    "ionice",
    "renice",
    "timeout",
    "nohup",
    "stdbuf",
    "setsid",
    "chroot",
    "unshare",
    "nsenter",
    "xargs",
    "parallel",
    "watch",
    "time",
    "command",
    "builtin",
    "exec",
    "eval",
    "source",
    ".",
    "strace",
    "ltrace",
    "valgrind",
    "flock",
    "chrt",
    "taskset",
    "script",
    "systemd-run",
    "at",
    "batch",
    "cron",
    "crontab",
    "trap",
    "coproc",
    "fakeroot",
    "proot",
    "firejail",
    "bwrap",
    "capsh",
    "setpriv",
    "unbuffer",
    "rlwrap",
    "entr",
    "inotifywait",
    "alias",
    "unalias",
    "enable",
    "hash",
    "type",
    "function",
    "declare",
    "typeset",
    "export",
    "readonly",
    "local",
    "set",
    "unset",
    "shopt",
    "let",
    "read",
    "mapfile",
    "readarray",
    "pushd",
    "popd",
    "disown",
    "wait",
    "jobs",
    "fg",
    "bg",
];

/// S5: environment dumpers (the gateway env holds provider keys).
pub const ENV_DUMPERS: &[&str] = &["printenv", "env", "set", "declare", "export", "compgen"];

/// S7: network clients (exfiltration and loopback service abuse).
pub const NETWORK: &[&str] = &[
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
    "rsync",
    "ftp",
    "lftp",
    "tftp",
    "telnet",
    "dig",
    "nslookup",
    "host",
    "drill",
    "ping",
    "ping6",
    "traceroute",
    "tracepath",
    "mtr",
    "openssl",
    "gnutls-cli",
    "gh",
    "git-remote-http",
    "http",
    "https",
    "xh",
    "httpie",
    "websocat",
    "smbclient",
    "mail",
    "sendmail",
    "mutt",
    "swaks",
    "tailscale",
    "cloudflared",
    "ngrok",
    "nmap",
    "masscan",
    "arp",
    "ip",
    "ifconfig",
    "iw",
];

/// S8: system / service tampering.
pub const SYSTEM: &[&str] = &[
    "systemctl",
    "service",
    "journalctl",
    "shutdown",
    "reboot",
    "poweroff",
    "halt",
    "init",
    "telinit",
    "mount",
    "umount",
    "swapon",
    "swapoff",
    "iptables",
    "ip6tables",
    "nft",
    "ufw",
    "sysctl",
    "modprobe",
    "insmod",
    "rmmod",
    "kexec",
    "chattr",
    "lsattr",
    "setfacl",
    "setcap",
    "useradd",
    "userdel",
    "usermod",
    "groupadd",
    "passwd",
    "chpasswd",
    "visudo",
    "ldconfig",
    "update-alternatives",
    "apt",
    "apt-get",
    "dpkg",
    "snap",
    "yum",
    "dnf",
    "apk",
    "losetup",
    "fdisk",
    "parted",
    "sfdisk",
    "wipefs",
    "blkdiscard",
    "hdparm",
    "mkswap",
    "dmsetup",
    "cryptsetup",
    "debugfs",
    "loginctl",
    "machinectl",
    "hostnamectl",
    "timedatectl",
    "dbus-send",
    "busctl",
    "fail2ban-client",
    "galion-guard",
    "hermes",
    "hermes-gateway",
    "hermes-gate",
    "kill",
    "pkill",
    "killall",
    "pgrep",
    "skill",
    "snice",
    "fuser",
    "pidof",
];

/// S3: decoders whose output is a hidden payload.
pub const DECODERS: &[&str] = &["base64", "base32", "basenc", "xxd", "uudecode", "uuencode"];

/// S6: commands that write (or delete, or re-link) every path argument.
pub const WRITE_ALL_ARGS: &[&str] = &[
    "rm", "rmdir", "mv", "ln", "link", "unlink", "touch", "mkdir", "truncate", "shred", "tee",
    "chmod", "chown", "chgrp", "mkfifo", "mknod", "patch", "split", "csplit", "dd", "gzip",
    "gunzip", "bzip2", "bunzip2", "xz", "unxz", "zstd", "unzstd", "lz4", "sponge",
];
/// S6: commands whose last path argument is the destination.
pub const WRITE_LAST_ARG: &[&str] = &["cp", "install", "rsync"];

/// S5: the only `NAME=value` prefixes and tool `env` keys that may be set.
pub const ENV_ALLOW: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TZ",
    "TERM",
    "NO_COLOR",
    "COLUMNS",
    "RUST_BACKTRACE",
    "RUST_LOG",
    "CARGO_TERM_COLOR",
];

/// S9: shell-type tool names (the command string is checked) — matched on the
/// lower-cased name, also as a suffix after an MCP prefix (`mcp_x_shell_exec`).
pub const SHELL_TOOL_HINTS: &[&str] = &[
    "terminal",
    "bash",
    "shell",
    "exec",
    "run_command",
    "run_shell",
    "process",
    "command",
    "cmd",
    "subprocess",
    "spawn",
    "system",
];
/// S9: code-execution tools: refused outright (inline code is not checkable).
pub const CODE_TOOL_HINTS: &[&str] = &[
    "execute_code",
    "run_code",
    "code_interpreter",
    "python",
    "eval",
    "repl",
    "jupyter",
    "notebook",
    "javascript",
    "js_eval",
    "sandbox_exec",
];
/// S9: tools that write / delete / move through a path argument.
pub const WRITE_TOOL_HINTS: &[&str] = &[
    "write", "edit", "patch", "create", "delete", "remove", "move", "rename", "mkdir", "append",
    "save", "upload", "put", "replace", "insert", "apply", "copy",
];
/// Keys whose string value is a shell command.
pub const COMMAND_KEYS: &[&str] = &[
    "command",
    "cmd",
    "script",
    "commandline",
    "shell",
    "bash",
    "argv",
    "args",
];
/// Keys whose string value is a path.
pub const PATH_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filepath",
    "filename",
    "file",
    "target",
    "dest",
    "destination",
    "src",
    "source",
    "dir",
    "directory",
    "cwd",
    "workdir",
    "working_dir",
    "paths",
    "files",
    "output",
    "out",
    "new_path",
    "old_path",
    "to",
    "from",
];
pub const PATCH_KEYS: &[&str] = &["patch", "diff", "unified_diff", "edits"];
pub const URL_KEYS: &[&str] = &["url", "uri", "href", "endpoint", "link"];
pub const ENV_KEYS: &[&str] = &["env", "environment", "envs", "env_vars"];
/// Keys whose string value is the body written into a file (not a path to
/// touch). A protected path merely mentioned inside such content is text,
/// not access (the file itself is still checked via its path key).
pub const CONTENT_KEYS: &[&str] = &[
    "new_text",
    "old_text",
    "new_str",
    "old_str",
    "new_string",
    "old_string",
    "content",
    "contents",
    "text",
    "body",
    "data",
    "file_text",
    "replacement",
];

/// S6: file names that steer an agent or run code on their own (shell rc
/// files, git hooks, build config, the guard itself). Refused for writes even
/// inside the workspace.
pub const NO_WRITE_NAMES: &[&str] = &[
    "SOUL.md",
    "AGENTS.md",
    "CLAUDE.md",
    "USER.md",
    "DESK.md",
    "GEMINI.md",
    ".cursorrules",
    ".envrc",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".inputrc",
    ".netrc",
    ".gitconfig",
    ".gitmodules",
    ".gitattributes",
    "guard.json",
    "dangerous-patterns.txt",
    "rust-toolchain",
    "rust-toolchain.toml",
    "authorized_keys",
    "ld.so.preload",
];
pub const NO_WRITE_DIRS: &[&str] = &[
    ".git",
    ".cargo",
    ".github",
    ".ssh",
    ".config",
    "command-guard",
    "hooks",
];

/// S6: secret material, refused for reads too (after canonical resolution).
pub const SECRET_ROOTS: &[&str] = &[
    "/root/.ssh",
    "/root/galion-secrets",
    "/root/.config",
    "/root/.memory-tencentdb",
    "/root/.hermes",
    "/opt/galion/hermes",
    "/opt/galion/galion-light/secrets",
    "/opt/galion/gate",
    "/var/lib/hermes-rs",
    "/var/backups",
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/sudoers",
    "/etc/sudoers.d",
    "/etc/ssh",
    "/proc/kcore",
    "/dev/mem",
    "/dev/kmem",
    "/dev/port",
    "/sys/firmware",
];
/// File-name shapes that are secrets anywhere.
pub const SECRET_NAMES: &[&str] = &[
    ".env",
    "auth.json",
    ".netrc",
    ".git-credentials",
    ".pgpass",
    "credentials",
    "credentials.json",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    "known_hosts",
    "authorized_keys",
    "keys.db",
    "rs-spend.json",
    "token",
    "github.token",
    "hermes.key",
    "galion-app.token",
    ".vault.lock",
];
pub const SECRET_SUFFIXES: &[&str] = &[
    ".pem", ".key", ".token", ".p12", ".pfx", ".kdbx", ".age", ".gpg", ".asc", "_rsa", "_ed25519",
    "_ecdsa",
];

/// S6: aliases of `/` and of other processes' views; never resolvable
/// statically, so refused.
pub const ALIAS_PREFIXES: &[&str] = &[
    "/proc/self/root",
    "/proc/thread-self/root",
    "/proc/self/cwd",
    "/proc/self/fd",
    "/proc/self/mem",
    "/dev/fd",
    "/dev/tcp",
    "/dev/udp",
    "/proc/self/environ",
    "/proc/self/map_files",
];

/// S8: the only `git` subcommands (read-only research plus local commit).
pub const GIT_ALLOW: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "blame",
    "grep",
    "ls-files",
    "ls-tree",
    "rev-parse",
    "rev-list",
    "shortlog",
    "describe",
    "cat-file",
    "branch",
    "tag",
    "remote",
    "add",
    "commit",
    "stash",
    "switch",
    "restore",
    "merge-base",
    "name-rev",
    "whatchanged",
    "count-objects",
    "check-ignore",
    "help",
    "version",
];
/// S8: the only `cargo` subcommands; all must run with --offline / --frozen.
pub const CARGO_ALLOW: &[&str] = &[
    "build",
    "check",
    "test",
    "clippy",
    "tree",
    "metadata",
    "fmt",
    "doc",
    "bench",
    "version",
    "--version",
    "-V",
];

/// Allowlist mode (opt-in, HERMES_RS_GUARD_MODE=allowlist): the only command
/// words the terminal may run at all.
pub const RESEARCH_ALLOW: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "rg",
    "grep",
    "egrep",
    "fgrep",
    "find",
    "sed",
    "sort",
    "uniq",
    "cut",
    "tr",
    "jq",
    "diff",
    "cmp",
    "comm",
    "file",
    "stat",
    "du",
    "df",
    "pwd",
    "echo",
    "printf",
    "date",
    "tree",
    "realpath",
    "readlink",
    "basename",
    "dirname",
    "true",
    "false",
    "test",
    "[",
    "which",
    "git",
    "cargo",
    "mkdir",
    "touch",
    "cp",
    "mv",
    "rm",
    "tee",
    "column",
    "nl",
    "fold",
    "fmt",
    "paste",
    "join",
    "expand",
    "unexpand",
    "rev",
    "tac",
    "sha256sum",
    "md5sum",
    "b2sum",
    "cd",
    "seq",
    "yes",
    "sleep",
    "uname",
    "id",
    "whoami",
    "nproc",
    "free",
    "uptime",
    "zcat",
    "gzip",
    "gunzip",
    "tar",
    "unzip",
    "zip",
];

/// S9: the Obsidian vault and other knowledge stores OpenMind serves
/// read-only. Writes to them are refused through every tool, including
/// delegated children, the coding agent and MCP tools (2026-10-29: a live
/// test had Hermes offer to route a vault write through its coding agent).
pub const VAULT_ROOTS: &[&str] = &[
    "/opt/galion/galion-light/docs/build-session",
    "/opt/galion/hermes/vault",
    "/opt/galion/openmind",
];

/// S9: roots no tool may write to unless a configured workspace lies inside
/// them (then only the workspace itself is writable). Used for the clear
/// reason text and for scanning delegated task text.
pub const PROTECTED_WRITE_ROOTS: &[&str] = &[
    "/opt",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/boot",
    "/root",
    "/home",
    "/srv",
    "/var",
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/workspace",
    "/tmp",
    "/mnt",
    "/media",
    "/snap",
];

/// S9: tool-name tokens that hand work to another agent.
pub const DELEGATE_TOOL_HINTS: &[&str] = &[
    "delegate",
    "subagent",
    "sub_agent",
    "handoff",
    "spawn_agent",
    "coding_agent",
    "agent",
];

/// S9: verbs that make a delegated task a write request.
pub const WRITE_VERBS: &[&str] = &[
    "write",
    "writes",
    "writing",
    "save",
    "saving",
    "edit",
    "editing",
    "modify",
    "append",
    "overwrite",
    "patch",
    "delete",
    "remove",
    "rename",
    "move",
    "create",
    "update",
    "store",
    "put",
    "insert",
    "replace",
    "commit",
    "add a note",
    "add note",
    "new note",
    "fs_write",
];

/// Script mode: shell variables a workspace script may not set (they change
/// how the shell or programs find and load code, or where output goes).
/// `LD_*`, `BASH_FUNC*`, `HERMES_*`, `OPENROUTER*` and `GIT_*` prefixes are
/// refused as well.
pub const SCRIPT_VAR_DENY: &[&str] = &[
    "PATH",
    "IFS",
    "ENV",
    "BASH_ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "PS4",
    "PROMPT_COMMAND",
    "CDPATH",
    "GCONV_PATH",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PERL5LIB",
    "PERL5OPT",
    "RUBYOPT",
    "NODE_OPTIONS",
    "RUSTC_WRAPPER",
    "RUSTC",
    "RUSTFLAGS",
    "CARGO_HOME",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "HOME",
    "TMPDIR",
    "SHELL",
    "EDITOR",
    "VISUAL",
    "PAGER",
    "MANPAGER",
    "LESSOPEN",
    "HISTFILE",
];
