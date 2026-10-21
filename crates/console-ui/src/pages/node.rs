// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/node` (Pod): port of console-ide `src/routes/_console/node.tsx` (live
//! tree, 2026-10-20 21:25 CEST). console-port-5, wave 4 slot 139.
//!
//! Differences from the TSX, on purpose:
//! - the SSH line and "Last IP" come from `GET /api/v1/health` `.target`
//!   (`sshDirect`, `sshPort`, `publicIp`), the same `MACHINE` object the
//!   TSX compiled into the bundle (`src/lib/machine.ts`), so a machine
//!   change shows without a rebuild; a value that is missing or does not
//!   look like a user@host / IP is not shown;
//! - Copy buttons (`navigator.clipboard`) are gone: each command is in a
//!   `<pre>` that can be selected. Clipboard needs JS.

use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// `RUNPOD_START_COMMAND` from `src/lib/runpod-start.ts` (verbatim).
pub const RUNPOD_START_COMMAND: &str = "bash -c \"curl -fsSL https://raw.githubusercontent.com/galion-studio/galion-initiative-web/cpu-light/galion-light/flash.sh | bash; exec /start.sh\"";

/// `JUPYTER_ORIGIN_COMMAND` from `src/lib/runpod-start.ts` (verbatim,
/// including its trailing newline).
pub const JUPYTER_ORIGIN_COMMAND: &str = "curl -fsSL https://raw.githubusercontent.com/galion-studio/galion-initiative-web/cpu-light/galion-light/flash.sh | bash\n";

/// `user@host` with a plain user and a host name / IPv4 / bracket-less IPv6.
pub fn ssh_target_ok(s: &str) -> bool {
    let Some((user, host)) = s.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && user.len() <= 32
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        && host_ok(host)
}

/// A host name or IP literal (letters, digits, `.`, `-`, `:`), 1-253 bytes.
pub fn host_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b':')
}

/// TSX `sshCommand()`: `ssh <sshDirect> -p <sshPort> -i ~/.ssh/id_ed25519`.
pub fn ssh_command(target: &Value) -> Option<String> {
    let direct = target
        .get("sshDirect")
        .as_str()
        .filter(|s| ssh_target_ok(s))?;
    let port = target
        .get("sshPort")
        .as_f64()
        .filter(|p| p.fract() == 0.0 && *p >= 1.0 && *p <= 65535.0)?;
    Some(format!(
        "ssh {direct} -p {} -i ~/.ssh/id_ed25519",
        port as u16
    ))
}

fn copy_block(label: &str, value: &str) -> String {
    format!(
        "<div class=\"plain\"><p class=\"k\">{}</p><pre class=\"code\">{}</pre></div>",
        esc(label),
        esc(value)
    )
}

/// The page body. `health` is `GET /api/v1/health` (public).
pub fn body(health: Option<&Value>) -> String {
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Pod</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">Node</span></h1>",
        icon("pod", "i i-md")
    ));
    out.push_str(
        "<p class=\"lede\">Flash this desk onto the pod. Origin + the original galion.studio and galion.app tunnels. DNS stays. 1033 dies when those connectors register.</p></header>",
    );
    out.push_str("<h2>Do this once</h2><ol class=\"steps\">");
    for li in [
        "HTTP ports stay <span class=\"mono\">8888,80,8080,19123</span>. TCP <span class=\"mono\">22</span>.",
        "Paste the start command below. Leave Jupyter on.",
        "Save once. Wait until Running. Do not edit again.",
        "Open <span class=\"mono\">…-8080.proxy.runpod.net/api/v1/health</span>.",
    ] {
        out.push_str(&format!("<li>{li}</li>"));
    }
    out.push_str("</ol>");
    out.push_str(&copy_block("Container start command", RUNPOD_START_COMMAND));
    out.push_str("<h2>Or now, in Jupyter</h2><p class=\"lede\">Jupyter → Terminal. Paste this. Origin starts without editing the pod.</p>");
    out.push_str(&copy_block("Flash onto the pod", JUPYTER_ORIGIN_COMMAND));
    out.push_str("<p class=\"note\">Select the text to copy it (no script on this page).</p>");
    let target = health.map(|h| h.get("target"));
    let ssh = target.and_then(ssh_command);
    let ip = target
        .and_then(|t| t.get("publicIp").as_str())
        .filter(|s| host_ok(s));
    out.push_str("<ul class=\"rows\">");
    for (label, v) in [("SSH", ssh.as_deref()), ("Last IP", ip)] {
        out.push_str(&format!(
            "<li><span>{label}</span><span class=\"muted mono\">{}</span></li>",
            esc(v.unwrap_or("—"))
        ));
    }
    out.push_str("</ul>");
    if health.is_none() {
        out.push_str("<p class=\"note\">The machine target (/api/v1/health) did not load. The console API did not answer.</p>");
    } else if ssh.is_none() || ip.is_none() {
        out.push_str("<p class=\"note\">The health answer has no usable SSH target or IP.</p>");
    }
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn ssh_line_like_the_tsx() {
        let h = parse(br#"{"status":"healthy","target":{"sshDirect":"root@203.0.113.7","sshPort":22,"publicIp":"203.0.113.7"}}"#).unwrap();
        assert_eq!(
            ssh_command(h.get("target")).as_deref(),
            Some("ssh root@203.0.113.7 -p 22 -i ~/.ssh/id_ed25519")
        );
        let b = body(Some(&h));
        assert!(b.contains("ssh root@203.0.113.7 -p 22 -i ~/.ssh/id_ed25519"));
        assert!(b.contains("<span>Last IP</span><span class=\"muted mono\">203.0.113.7</span>"));
        assert!(!b.contains("did not load"));
        assert!(b.contains(&esc(RUNPOD_START_COMMAND)));
        assert!(b.contains("8888,80,8080,19123"));
        assert!(!b.contains("<script"));
    }

    #[test]
    fn odd_targets_are_not_shown() {
        for t in [
            r#"{"target":{"sshDirect":"root@h; rm -rf /","sshPort":22,"publicIp":"<b>x</b>"}}"#,
            r#"{"target":{"sshDirect":"@h","sshPort":22.5,"publicIp":""}}"#,
            r#"{"target":{"sshDirect":"root@h","sshPort":70000}}"#,
            r#"{"target":{}}"#,
        ] {
            let h = parse(t.as_bytes()).unwrap();
            assert!(ssh_command(h.get("target")).is_none(), "{t}");
            let b = body(Some(&h));
            assert!(b.contains("no usable SSH target"), "{t}");
            assert!(!b.contains("rm -rf") && !b.contains("<b>x"), "{t}");
        }
        assert!(body(None).contains("did not load"));
        assert!(ssh_target_ok("root@2001:db8::1"));
        assert!(!ssh_target_ok("root@"));
    }
}
