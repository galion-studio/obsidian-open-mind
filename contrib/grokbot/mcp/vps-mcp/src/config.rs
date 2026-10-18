//! Loads the single KEY=VALUE config file shared with the ~/workflows/vps shell scripts.
//! The file is shell-sourceable (quotes, inline `# comments`, `$HOME`), so we parse a
//! compatible subset and never write to it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ENV_CONFIG_PATH: &str = "VPS_MCP_CONFIG";

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/root"))
}

pub fn default_config_path() -> PathBuf {
    home().join("workflows/vps/config.env")
}

/// --config flag > $VPS_MCP_CONFIG > ~/workflows/vps/config.env
pub fn resolve_config_path(cli: Option<&str>) -> PathBuf {
    if let Some(p) = cli {
        return expand_path(p);
    }
    if let Ok(p) = std::env::var(ENV_CONFIG_PATH) {
        if !p.trim().is_empty() {
            return expand_path(&p);
        }
    }
    default_config_path()
}

pub fn expand_path(p: &str) -> PathBuf {
    let p = expand_vars(p, &HashMap::new());
    if p == "~" {
        home()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(p)
    }
}

/// Expand $NAME / ${NAME} using previously parsed keys, then the process env (HOME etc).
fn expand_vars(s: &str, known: &HashMap<String, String>) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let (name, next) = if i + 1 < b.len() && b[i + 1] == b'{' {
                match s[i + 2..].find('}') {
                    Some(end) => (&s[i + 2..i + 2 + end], i + 3 + end),
                    None => ("", i + 1),
                }
            } else {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                (&s[start..j], j)
            };
            if name.is_empty() {
                out.push('$');
                i += 1;
                continue;
            }
            let val = known
                .get(name)
                .cloned()
                .or_else(|| std::env::var(name).ok())
                .unwrap_or_default();
            out.push_str(&val);
            i = next;
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Parse one shell-ish value: handles "double", 'single', unquoted with trailing `# comment`.
fn parse_value(raw: &str, known: &HashMap<String, String>) -> String {
    let raw = raw.trim();
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    let mut prev_ws = true;
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                for c2 in chars.by_ref() {
                    if c2 == '\'' {
                        break;
                    }
                    out.push(c2);
                }
                prev_ws = false;
            }
            '"' => {
                let mut seg = String::new();
                while let Some(c2) = chars.next() {
                    if c2 == '\\' {
                        if let Some(n) = chars.next() {
                            seg.push(n);
                        }
                        continue;
                    }
                    if c2 == '"' {
                        break;
                    }
                    seg.push(c2);
                }
                out.push_str(&expand_vars(&seg, known));
                prev_ws = false;
            }
            '#' if prev_ws => break,
            c if c.is_whitespace() => {
                // unquoted whitespace ends the value (shell semantics), rest is comment/garbage
                let rest: String = chars.clone().collect();
                if rest.trim_start().starts_with('#') || rest.trim().is_empty() {
                    break;
                }
                out.push(c);
                prev_ws = true;
            }
            '$' => {
                let mut seg = String::from("$");
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '_' || n == '{' || n == '}' {
                        seg.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str(&expand_vars(&seg, known));
                prev_ws = false;
            }
            c => {
                out.push(c);
                prev_ws = false;
            }
        }
    }
    out
}

pub fn parse_env_file(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let mut l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(r) = l.strip_prefix("export ") {
            l = r.trim_start();
        }
        let Some(eq) = l.find('=') else { continue };
        let key = l[..eq].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let val = parse_value(&l[eq + 1..], &map);
        map.insert(key.to_string(), val);
    }
    map
}

pub fn is_placeholder(v: &str) -> bool {
    let t = v.trim();
    let u = t.to_ascii_uppercase();
    t.is_empty()
        || u.contains("CHANGE_ME")
        || u.contains("CHANGEME")
        || u.contains("FILL_ME")
        || u.contains("FILLME")
        || u == "TODO"
        || (t.starts_with('<') && t.ends_with('>'))
        || (t.starts_with("__") && t.ends_with("__"))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub file_found: bool,
    pub host: Option<String>,
    pub user: Option<String>,
    pub raw_host: String,
    pub raw_user: String,
    pub port: u16,
    pub key: PathBuf,
    pub known_hosts: PathBuf,
    pub roots: Vec<String>,
    pub allow_write: bool,
    pub connect_timeout_secs: u64,
    pub exec_timeout_secs: u64,
    pub read_max_bytes: u64,
    pub pull_max_file_bytes: u64,
    pub skills_import_dir: PathBuf,
    pub workflows_import_dir: PathBuf,
}

pub const DEFAULT_ROOTS: &[&str] = &["/opt", "/srv", "/home", "/var/www", "/root", "/etc/systemd", "/usr/local"];

fn first<'a>(m: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a String> {
    keys.iter().find_map(|k| m.get(*k))
}

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

impl Config {
    pub fn load(path: &Path) -> Config {
        let (file_found, map) = match std::fs::read_to_string(path) {
            Ok(t) => (true, parse_env_file(&t)),
            Err(_) => (false, HashMap::new()),
        };
        let raw_host = first(&map, &["VPS_HOST", "SSH_HOST", "HOST"]).cloned().unwrap_or_default();
        let raw_user = first(&map, &["VPS_USER", "SSH_USER", "VPS_USERNAME"]).cloned().unwrap_or_default();
        let num = |keys: &[&str], d: u64| -> u64 {
            first(&map, keys).and_then(|v| v.trim().parse().ok()).unwrap_or(d)
        };
        let key = first(&map, &["VPS_KEY", "VPS_SSH_KEY", "VPS_KEY_PATH", "VPS_IDENTITY_FILE", "SSH_KEY"])
            .filter(|v| !is_placeholder(v))
            .map(|v| expand_path(v))
            .unwrap_or_else(|| home().join(".ssh/id_ed25519_vps"));
        let known_hosts = first(&map, &["VPS_KNOWN_HOSTS", "VPS_MCP_KNOWN_HOSTS"])
            .filter(|v| !v.trim().is_empty())
            .map(|v| expand_path(v))
            .unwrap_or_else(|| home().join(".config/vps-mcp/known_hosts"));
        let roots: Vec<String> = first(&map, &["VPS_PROJECT_ROOTS"])
            .map(|v| {
                v.split(|c: char| c == ',' || c == ':' || c.is_whitespace())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_ROOTS.iter().map(|s| s.to_string()).collect());
        Config {
            path: path.to_path_buf(),
            file_found,
            host: (!is_placeholder(&raw_host)).then(|| raw_host.trim().to_string()),
            user: (!is_placeholder(&raw_user)).then(|| raw_user.trim().to_string()),
            raw_host,
            raw_user,
            port: num(&["VPS_PORT", "SSH_PORT"], 22) as u16,
            key,
            known_hosts,
            roots,
            allow_write: first(&map, &["VPS_ALLOW_WRITE", "VPS_MCP_ALLOW_WRITE"]).map(|v| truthy(v)).unwrap_or(false),
            connect_timeout_secs: num(&["VPS_CONNECT_TIMEOUT"], 15).max(1),
            exec_timeout_secs: num(&["VPS_EXEC_TIMEOUT"], 60).max(1),
            read_max_bytes: num(&["VPS_READ_MAX_BYTES"], 512 * 1024),
            pull_max_file_bytes: num(&["VPS_PULL_MAX_FILE_BYTES"], 5 * 1024 * 1024),
            skills_import_dir: first(&map, &["VPS_SKILLS_IMPORT_DIR"])
                .map(|v| expand_path(v))
                .unwrap_or_else(|| home().join("skills/imported-from-vps")),
            workflows_import_dir: first(&map, &["VPS_WORKFLOWS_IMPORT_DIR"])
                .map(|v| expand_path(v))
                .unwrap_or_else(|| home().join("workflows/imported-from-vps")),
        }
    }

    /// Returns (host, user) or the clear "not configured" error.
    pub fn target(&self) -> Result<(String, String), String> {
        if !self.file_found {
            return Err(format!(
                "VPS config file not found at {} (set {} or run `vps-mcp init-config`)",
                self.path.display(),
                ENV_CONFIG_PATH
            ));
        }
        match (&self.host, &self.user) {
            (Some(h), Some(u)) => Ok((h.clone(), u.clone())),
            _ => Err(format!(
                "VPS host/user not configured in {} (VPS_HOST={:?}, VPS_USER={:?}). Fill in real values (e.g. ~/workflows/vps/bin/set-target.sh <host> <user>) and retry.",
                self.path.display(),
                self.raw_host,
                self.raw_user
            )),
        }
    }

    /// Directory-safe host label used under imported-from-vps/.
    pub fn host_label(&self) -> String {
        self.host
            .clone()
            .unwrap_or_else(|| "unconfigured".into())
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
            .collect()
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "config_path": self.path,
            "config_file_found": self.file_found,
            "host": self.raw_host,
            "user": self.raw_user,
            "configured": self.host.is_some() && self.user.is_some(),
            "port": self.port,
            "key": self.key,
            "key_exists": self.key.exists(),
            "known_hosts": self.known_hosts,
            "project_roots": self.roots,
            "allow_write": self.allow_write,
            "connect_timeout_secs": self.connect_timeout_secs,
            "exec_timeout_secs": self.exec_timeout_secs,
            "read_max_bytes": self.read_max_bytes,
            "pull_max_file_bytes": self.pull_max_file_bytes,
            "skills_import_dir": self.skills_import_dir.join(self.host_label()),
            "workflows_import_dir": self.workflows_import_dir.join(self.host_label()),
        })
    }
}

pub const TEMPLATE: &str = "# vps-mcp / ~/workflows/vps config (KEY=VALUE, shell-sourceable)\nVPS_HOST=CHANGE_ME\nVPS_USER=CHANGE_ME\nVPS_PORT=22\nVPS_KEY=~/.ssh/id_ed25519_vps\n# Optional:\n# VPS_PROJECT_ROOTS=/opt,/srv,/home,/var/www,/root,/etc/systemd\n# VPS_KNOWN_HOSTS=~/.config/vps-mcp/known_hosts\n# VPS_ALLOW_WRITE=false\n";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_shell_style() {
        let t = "VPS_HOST=\"__FILL_ME_HOST__\"      # e.g. x\nVPS_PORT=\"2222\"\nVPS_KEY=\"$HOME/.ssh/k\"   # c\nexport A='x y' # z\nB=plain # c\nC=${B}/d\n";
        let m = parse_env_file(t);
        assert_eq!(m["VPS_HOST"], "__FILL_ME_HOST__");
        assert!(is_placeholder(&m["VPS_HOST"]));
        assert_eq!(m["VPS_PORT"], "2222");
        assert_eq!(m["VPS_KEY"], format!("{}/.ssh/k", home().display()));
        assert_eq!(m["A"], "x y");
        assert_eq!(m["B"], "plain");
        assert_eq!(m["C"], "plain/d");
        assert!(is_placeholder("CHANGE_ME"));
        assert!(!is_placeholder("203.0.113.10"));
    }
}
