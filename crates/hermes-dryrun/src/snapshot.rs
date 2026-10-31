// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `hermes-dryrun snapshot`: read the live hermes-rs layout READ-ONLY over
//! ssh and keep a redacted copy on this machine.
//!
//! Redaction runs ON THE SERVER (perl, before anything is sent), so no
//! secret value and no public IP ever reaches this machine: IPv4 addresses
//! other than 127.0.0.1 / 0.0.0.0 become `<IP>`, values of secret-looking
//! keys become `<REDACTED>`, env files are sent as variable NAMES only, and
//! token-shaped strings (sk-, ghp_, github_pat_, xox*, AKIA, JWT) are cut.
//! The remote script only runs `systemctl cat`, `cat`, `ls`, `test`,
//! `sha256sum` and `perl -pe` — nothing on the server is written, started
//! or restarted. A second, local scan refuses the snapshot if anything
//! still looks like a secret.

use std::path::Path;
use std::process::{Command, Stdio};

/// Perl redaction program (runs remotely).
pub const REDACT_PERL: &str = r#"s/\b127\.0\.0\.1\b/\x01LO\x01/g; s/\b0\.0\.0\.0\b/\x01ANY\x01/g; s/\b(?:\d{1,3}\.){3}\d{1,3}\b/<IP>/g; s/\x01LO\x01/127.0.0.1/g; s/\x01ANY\x01/0.0.0.0/g; s/(?<![A-Za-z0-9])sk-[A-Za-z0-9_-]{16,}/<REDACTED>/g; s/(?<![A-Za-z0-9])(?:gh[pousr]_|github_pat_|xox[abpr]-|AKIA|eyJ)[A-Za-z0-9_.\/+=-]{12,}/<REDACTED>/g; s/^(\s*-?\s*["']?(?![A-Za-z0-9_]*_env\b)[A-Za-z0-9_]*(?:api_?key|token|secret|passw(?:or)?d|seed|private_?key|authorization|cookie|bearer)[A-Za-z0-9_]*["']?\s*:\s*)(?!["']?\$\{)(?!\/)(?!\d+\s*(?:#|$))(?!<REDACTED>)(\S.*)$/$1<REDACTED>/i; s/((?:^|[\s"])(?:Environment=)?"?[A-Za-z0-9_]*(?:KEY|TOKEN|SECRET|PASSW(?:OR)?D|SEED|AUTH|BEARER|COOKIE)[A-Za-z0-9_]*=)(?!\$\{)(?!\/)[^\s"]+/$1<REDACTED>/gi;"#;

/// Files kept in the layout dir, in order. (name, remote shell command).
fn remote_script() -> String {
    let parts: &[(&str, &str)] = &[
        ("unit.txt", "systemctl cat hermes-rs.service"),
        ("config.yaml", "cat /opt/galion/hermes/config.yaml"),
        ("mcp-servers.yaml", "cat /etc/hermes-rs/mcp-servers.yaml"),
        ("SOUL.md", "cat /opt/galion/hermes/SOUL.md"),
        ("stack.conf", "test -f /etc/openmind-mcp-stack/stack.conf && cat /etc/openmind-mcp-stack/stack.conf"),
        // env files: names only, never values
        ("env-names.txt", "perl -ne 'print \"$1\\n\" if /^\\s*(?:export\\s+)?([A-Za-z_][A-Za-z0-9_]*)=/' /opt/galion/hermes/.env"),
        ("mcp.d.txt", "for f in /etc/hermes-rs/mcp.d/*.env; do test -f \"$f\" || continue; echo \"## $(basename \"$f\")\"; perl -ne 'print \"$1\\n\" if /^\\s*(?:export\\s+)?([A-Za-z_][A-Za-z0-9_]*)=/' \"$f\"; done"),
        ("facts.txt", "B=$(systemctl show -p ExecStart --value hermes-rs.service | perl -ne 'print $1 if /path=(\\S+)/'); echo \"active_binary=$B\"; echo \"active_binary_sha256=$(sha256sum \"$B\" | cut -d' ' -f1)\"; for p in /etc/hermes-self-modify/owner.pub /etc/hermes-self-modify/sandbox.pub /etc/openmind-mcp-stack/stack.conf /opt/galion/hermes/state.db /srv/hermes-workspace; do if test -e \"$p\"; then echo \"exists $p\"; else echo \"missing $p\"; fi; done; ls /opt/galion/rust/bin | grep -E '^(openmind-mcp-stack|github-mcp-rs|hermes-gateway)-[0-9a-f]+$' | sed 's/^/bin /'; echo \"handoff_path_enabled=$(systemctl is-enabled gaia-hermes-handoff.path 2>/dev/null)\"; echo \"hermes_rs_active=$(systemctl is-active hermes-rs.service)\""),
    ];
    let mut s = String::from("set -u\nR='");
    s.push_str(&REDACT_PERL.replace('\'', "'\\''"));
    s.push_str("'\n");
    for (name, cmd) in parts {
        s.push_str(&format!(
            "echo '@@@DRYRUN-FILE {name}@@@'\n( {cmd} ) 2>&1 | perl -pe \"$R\"\n"
        ));
    }
    s
}

/// Strings that must never appear in a snapshot (local second check).
pub fn secret_hits(text: &str) -> Vec<String> {
    let mut hits = vec![];
    for (i, line) in text.lines().enumerate() {
        let l = line.trim();
        for pat in [
            "sk-or-",
            "sk-ant-",
            "ghp_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
            "AKIA",
        ] {
            for (p, _) in l.match_indices(pat) {
                let tail = l[p + pat.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .count();
                if tail >= 12 {
                    hits.push(format!("line {}: {pat} token", i + 1));
                }
            }
        }
        if l.contains("PRIVATE KEY-----") || l.contains("BEGIN OPENSSH") {
            hits.push(format!("line {}: private key block", i + 1));
        }
        // long base64-ish token that is not a hex digest
        for w in l.split(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+' || c == '/')
        }) {
            let hexy = w.chars().all(|c| c.is_ascii_hexdigit());
            let mixed = w.chars().any(|c| c.is_ascii_uppercase())
                && w.chars().any(|c| c.is_ascii_lowercase())
                && w.chars().any(|c| c.is_ascii_digit());
            let name = w
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            if w.len() >= 32 && !hexy && !name && mixed && !w.contains('/') {
                hits.push(format!(
                    "line {}: token-shaped string ({} chars)",
                    i + 1,
                    w.len()
                ));
            }
        }
        // a dotted quad that is not loopback/any
        for w in l.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
            let parts: Vec<&str> = w.split('.').collect();
            if parts.len() == 4
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u8>().is_ok())
                && w != "127.0.0.1"
                && w != "0.0.0.0"
            {
                hits.push(format!("line {}: IPv4 address", i + 1));
            }
        }
    }
    hits
}

pub fn run(host: &str, out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut child = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            host,
            "sh",
            "-s",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    {
        use std::io::Write;
        let mut si = child.stdin.take().ok_or("ssh stdin")?;
        si.write_all(remote_script().as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() && o.stdout.is_empty() {
        return Err(format!(
            "ssh {host} failed: {}",
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join(" / ")
        ));
    }
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let hits = secret_hits(&text);
    if !hits.is_empty() {
        return Err(format!(
            "refusing to keep the snapshot: {} secret-looking item(s) survived remote redaction ({}); nothing written",
            hits.len(),
            hits.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    let mut cur: Option<(String, String)> = None;
    let mut files = vec![];
    for line in text.lines() {
        if let Some(n) = line
            .strip_prefix("@@@DRYRUN-FILE ")
            .and_then(|l| l.strip_suffix("@@@"))
        {
            if let Some(c) = cur.take() {
                files.push(c);
            }
            cur = Some((n.to_string(), String::new()));
        } else if let Some((_, b)) = cur.as_mut() {
            b.push_str(line);
            b.push('\n');
        }
    }
    if let Some(c) = cur.take() {
        files.push(c);
    }
    if files.len() < 3 {
        return Err("snapshot incomplete (ssh output had too few sections)".into());
    }
    for (n, b) in &files {
        std::fs::write(out.join(n), b).map_err(|e| e.to_string())?;
    }
    let meta = serde_json::json!({
        "version": "hermes-dryrun/layout/v1",
        "taken_at": crate::util::zagreb(crate::util::now_secs()),
        "host_alias": host,
        "redaction": "remote perl (IPs, secret keys, token shapes); env files as names only",
        "files": files.iter().map(|(n, b)| serde_json::json!({"name": n, "sha256": crate::util::sha256_bytes(b.as_bytes()), "bytes": b.len()})).collect::<Vec<_>>(),
    });
    std::fs::write(out.join("layout.json"), format!("{meta:#}\n")).map_err(|e| e.to_string())?;
    eprintln!("snapshot: {} files -> {}", files.len(), out.display());
    Ok(())
}
