//! Read-only exec allowlist, agent-file classification and secret detection.

pub const READONLY_COMMANDS: &[&str] = &[
    "ls", "cat", "head", "tail", "grep", "egrep", "fgrep", "find", "stat", "file", "wc", "du", "df",
    "free", "uptime", "uname", "whoami", "id", "groups", "hostname", "ps", "pgrep", "ss", "netstat",
    "lsof", "readlink", "realpath", "pwd", "echo", "printf", "date", "which", "lsblk", "findmnt",
    "sort", "uniq", "cut", "tr", "column", "nl", "tree", "sha256sum", "md5sum", "lscpu", "nproc",
    "basename", "dirname", "top", "systemctl", "journalctl", "docker", "docker-compose", "git", "ip",
    "printenv", "getent", "w", "who", "last", "crontab", "test", "true", "vmstat", "iostat", "cal",
];

fn tokenize(seg: &str) -> Result<Vec<String>, String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut in_tok = false;
    let mut chars = seg.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' => {
                in_tok = true;
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == c { closed = true; break; }
                    cur.push(c2);
                }
                if !closed { return Err("unbalanced quotes".into()); }
            }
            c if c.is_whitespace() => {
                if in_tok { toks.push(std::mem::take(&mut cur)); in_tok = false; }
            }
            c => { cur.push(c); in_tok = true; }
        }
    }
    if in_tok { toks.push(cur); }
    Ok(toks)
}

fn positional(args: &[String]) -> Vec<&String> {
    args.iter().filter(|a| !a.starts_with('-')).collect()
}

fn check_segment(toks: &[String]) -> Result<(), String> {
    let Some(cmd) = toks.first() else { return Err("empty command segment".into()) };
    let args = &toks[1..];
    if cmd.contains('/') {
        return Err(format!("'{cmd}': use bare command names in read-only mode"));
    }
    if cmd.contains('=') {
        return Err("environment assignments are not allowed in read-only mode".into());
    }
    if !READONLY_COMMANDS.contains(&cmd.as_str()) {
        return Err(format!("'{cmd}' is not in the read-only allowlist"));
    }
    let has = |names: &[&str]| args.iter().any(|a| names.iter().any(|n| a == n || a.starts_with(&format!("{n}="))));
    let deny = |names: &[&str]| -> Result<(), String> {
        if has(names) { Err(format!("'{cmd}' with {names:?} is not allowed in read-only mode")) } else { Ok(()) }
    };
    let sub = || positional(args).first().map(|s| s.as_str()).unwrap_or("");
    match cmd.as_str() {
        "find" => deny(&["-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf", "-fls"])?,
        "sort" => deny(&["-o", "--output"])?,
        "tree" => deny(&["-o"])?,
        "date" => deny(&["-s", "--set"])?,
        "uniq" => if positional(args).len() > 1 { return Err("uniq with an output file is not allowed".into()) },
        "hostname" => if !positional(args).is_empty() { return Err("hostname with arguments (sets hostname) is not allowed".into()) },
        "crontab" => if args.iter().any(|a| a != "-l") || args.is_empty() { return Err("only 'crontab -l' is allowed".into()) },
        "top" => if !args.iter().any(|a| a.starts_with("-b")) { return Err("top requires -b (batch) mode".into()) },
        "journalctl" => deny(&["--vacuum-size", "--vacuum-time", "--vacuum-files", "--rotate", "--flush", "--sync",
            "--relinquish-var", "--smart-relinquish-var", "--setup-keys", "--update-catalog", "-f", "--follow"])?,
        "systemctl" => {
            let ok = ["status", "list-units", "list-unit-files", "list-timers", "list-sockets", "list-dependencies",
                      "cat", "show", "is-active", "is-enabled", "is-failed", "", "--version"];
            if !ok.contains(&sub()) { return Err(format!("systemctl '{}' is not read-only", sub())); }
        }
        "docker" => {
            let p = positional(args);
            let s0 = p.first().map(|s| s.as_str()).unwrap_or("");
            let s1 = p.get(1).map(|s| s.as_str()).unwrap_or("");
            let ok = match s0 {
                "ps" | "images" | "inspect" | "logs" | "version" | "info" | "top" | "port" | "stats" => true,
                "compose" => matches!(s1, "ls" | "ps" | "config" | "images" | "logs" | "top" | "version"),
                "container" => matches!(s1, "ls" | "list" | "inspect" | "logs" | "top" | "port"),
                "image" => matches!(s1, "ls" | "list" | "inspect" | "history"),
                "network" | "volume" => matches!(s1, "ls" | "list" | "inspect"),
                _ => false,
            };
            if !ok { return Err(format!("docker '{s0} {s1}' is not read-only")); }
            deny(&["-f", "--follow"]).or_else(|e| if s0 == "ps" || s0 == "images" || s1 == "ls" || s1 == "ps" { Ok(()) } else { Err(e) })?;
        }
        "docker-compose" => {
            if !matches!(sub(), "ps" | "config" | "images" | "logs" | "top" | "version" | "ls") {
                return Err(format!("docker-compose '{}' is not read-only", sub()));
            }
        }
        "git" => {
            let first = args.first().map(|s| s.as_str()).unwrap_or("");
            if !matches!(first, "status" | "log" | "show" | "diff" | "branch" | "remote" | "ls-files" | "rev-parse" | "describe" | "shortlog" | "blame") {
                return Err(format!("git '{first}' is not allowed in read-only mode (no global options)"));
            }
            deny(&["--output", "--ext-diff", "-D", "-d", "--delete", "-m", "-M", "--move", "add", "rm", "remove", "rename", "set-url", "prune"])?;
        }
        "ip" => deny(&["add", "del", "delete", "change", "replace", "set", "flush", "append", "exec", "-b", "-batch", "-force"])?,
        _ => {}
    }
    Ok(())
}

/// Validate a command for read-only mode. Pipes between allowlisted commands are OK;
/// sequencing, redirection, substitution and variable expansion are rejected.
pub fn check_readonly(cmd: &str) -> Result<(), String> {
    for bad in [";", "&", ">", "<", "`", "$", "\n", "\r", "\\"] {
        if cmd.contains(bad) {
            return Err(format!("character {bad:?} is not allowed in read-only mode (no sequencing, redirection, substitution or variables)"));
        }
    }
    for seg in cmd.split('|') {
        let toks = tokenize(seg)?;
        check_segment(&toks)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category { Persona, Skill, Workflow, Other }

pub fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

pub fn classify(path: &str) -> Category {
    let b = basename(path).to_ascii_lowercase();
    let lp = path.to_ascii_lowercase();
    if b.starts_with("persona") || b.contains(".persona.") || b.ends_with(".persona") || b.starts_with("soul")
        || b.contains("system_prompt") || b.contains("system-prompt") || b.contains("systemprompt")
        || lp.contains("/personas/") || lp.contains("/persona/")
    {
        Category::Persona
    } else if b == "skill.md" || b.contains(".skill.") || lp.contains("/skills/") {
        Category::Skill
    } else if lp.contains("/workflows/") || b.contains(".workflow.") || b.ends_with(".workflow") {
        Category::Workflow
    } else {
        Category::Other
    }
}

/// find(1) expression (without roots/prune) matching persona/skill/workflow files.
pub const AGENT_FIND_EXPR: &str = "-type f \\( -iname 'persona*' -o -iname '*.persona.*' -o -iname '*.persona' -o -iname 'soul*' \
-o -iname '*system_prompt*' -o -iname '*system-prompt*' -o -iname '*systemprompt*' -o -ipath '*/personas/*' \
-o -iname 'SKILL.md' -o -iname '*.skill.*' -o -path '*/skills/*' \
-o -path '*/workflows/*' -o -iname '*.workflow.*' -o -iname '*.workflow' \\)";

/// Directories never descended into.
pub const PRUNE_EXPR: &str = "-type d \\( -name node_modules -o -name .git -o -name __pycache__ -o -name .venv -o -name venv \
-o -name site-packages -o -name dist-packages -o -name .cache -o -name .npm -o -name .cargo -o -name .rustup \
-o -name imported-from-vps -o -name overlay2 -o -name proc \\) -prune";

/// Name-based secret detection (on the basename).
pub fn secret_reason(path: &str) -> Option<&'static str> {
    let b = basename(path).to_ascii_lowercase();
    if b == ".env" || b.starts_with(".env.") || b.ends_with(".env") {
        return Some("dotenv/env file");
    }
    if (b.starts_with("id_rsa") || b.starts_with("id_dsa") || b.starts_with("id_ecdsa") || b.starts_with("id_ed25519")) && !b.ends_with(".pub") {
        return Some("SSH private key name");
    }
    for ext in [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore", ".kdbx", ".ppk", ".gpg"] {
        if b.ends_with(ext) {
            return Some("key/certificate/keystore extension");
        }
    }
    for exact in [".netrc", ".pgpass", "authorized_keys", "known_hosts", "shadow", ".htpasswd", ".npmrc", ".pypirc", ".git-credentials"] {
        if b == exact {
            return Some("credential file");
        }
    }
    for word in ["secret", "credential", "token", "password", "passwd", "apikey", "api_key", "api-key", "private", "service-account", "serviceaccount"] {
        if b.contains(word) {
            return Some("credential-like file name");
        }
    }
    None
}

/// Content-based secret detection on downloaded bytes.
pub fn secret_in_content(data: &[u8]) -> Option<&'static str> {
    let s = String::from_utf8_lossy(&data[..data.len().min(256 * 1024)]);
    if s.contains("PRIVATE KEY-----") {
        return Some("contains a PEM private key block");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readonly() {
        assert!(check_readonly("ls -la /opt").is_ok());
        assert!(check_readonly("ps aux | grep galleon").is_ok());
        assert!(check_readonly("systemctl status nginx").is_ok());
        assert!(check_readonly("docker compose ps").is_ok());
        assert!(check_readonly("rm -rf /").is_err());
        assert!(check_readonly("ls; rm x").is_err());
        assert!(check_readonly("cat x > y").is_err());
        assert!(check_readonly("echo $(id)").is_err());
        assert!(check_readonly("find / -delete").is_err());
        assert!(check_readonly("systemctl restart nginx").is_err());
        assert!(check_readonly("docker rm x").is_err());
        assert!(check_readonly("/bin/rm x").is_err());
        assert!(check_readonly("git -c core.pager=x log").is_err());
    }
    #[test]
    fn classes() {
        assert_eq!(classify("/opt/galleon/persona.md"), Category::Persona);
        assert_eq!(classify("/opt/galleon/SOUL.md"), Category::Persona);
        assert_eq!(classify("/opt/galleon/skills/x/SKILL.md"), Category::Skill);
        assert_eq!(classify("/opt/galleon/workflows/y.yaml"), Category::Workflow);
        assert!(secret_reason("/opt/x/.env").is_some());
        assert!(secret_reason("/opt/x/skills/id_ed25519").is_some());
        assert!(secret_reason("/opt/x/skills/SKILL.md").is_none());
    }
}
