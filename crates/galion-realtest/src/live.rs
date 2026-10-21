// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! READ-ONLY live probe. Over ssh it runs a fixed script of `curl` GETs
//! against loopback health URLs, `systemctl is-active` and `ss -ltn`; from
//! the box it GETs the public endpoints. It never POSTs, never writes a file
//! on the VPS, never restarts anything. The script is built only from the
//! constant tables below, and a unit test pins that it stays GET-only.

use crate::report::Suite;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy)]
pub enum Body {
    Any,
    /// Trimmed body equals this.
    Exact(&'static str),
    Contains(&'static str),
    /// JSON with key == string value.
    JsonStr(&'static str, &'static str),
    /// JSON with key == bool.
    JsonBool(&'static str, bool),
}

#[derive(Debug, Clone, Copy)]
pub struct Probe {
    pub service: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub status: &'static [u16],
    pub body: Body,
    /// Record up to 100 body chars in the report (only for non-sensitive bodies).
    pub show: bool,
}

const fn p(
    service: &'static str,
    name: &'static str,
    url: &'static str,
    status: &'static [u16],
    body: Body,
    show: bool,
) -> Probe {
    Probe {
        service,
        name,
        url,
        status,
        body,
        show,
    }
}

/// Loopback URLs, fetched ON the VPS. Expected contract per
/// docs/next.md, docs/host-decisions.md and the service sources.
pub const VPS: &[Probe] = &[
    p(
        "galion-one",
        "one-health",
        "http://127.0.0.1:8091/health",
        &[200],
        Body::Exact("ok"),
        true,
    ),
    p(
        "galion-one",
        "one-me-unpaired",
        "http://127.0.0.1:8091/me",
        &[200],
        Body::JsonBool("you", false),
        true,
    ),
    p(
        "galion-one",
        "one-session-unpaired-401",
        "http://127.0.0.1:8091/session",
        &[401],
        Body::Any,
        false,
    ),
    p(
        "galion-one-mouth",
        "mouth-health",
        "http://127.0.0.1:8002/health",
        &[200],
        Body::JsonBool("ok", true),
        true,
    ),
    p(
        "openmind-memory",
        "memory-health",
        "http://127.0.0.1:8420/health",
        &[200],
        Body::JsonStr("status", "ok"),
        false,
    ),
    p(
        "openmind-memory",
        "memory-impl",
        "http://127.0.0.1:8420/health",
        &[200],
        Body::JsonStr("implementation", "openmind-memory"),
        false,
    ),
    p(
        "hermes-rs",
        "hermes-health",
        "http://127.0.0.1:8642/health",
        &[200],
        Body::JsonStr("status", "ok"),
        true,
    ),
    p(
        "galion-desk",
        "desk-health",
        "http://127.0.0.1:8646/health",
        &[200],
        Body::Exact("{\"ok\":true}"),
        true,
    ),
    p(
        "tuner",
        "tuner-health",
        "http://127.0.0.1:8650/health",
        &[200],
        Body::Exact("{\"ok\":true}"),
        true,
    ),
    p(
        "galion-health",
        "galion-health-status",
        "http://127.0.0.1:8660/status.json",
        &[200],
        Body::JsonBool("ok", true),
        false,
    ),
    p(
        "galion-nerve",
        "nerve-healthz",
        "http://127.0.0.1:8670/healthz",
        &[200],
        Body::Exact("ok"),
        true,
    ),
    p(
        "galion-nerve",
        "nerve-dashboard",
        "http://127.0.0.1:8670/",
        &[200],
        Body::Contains("galion-nerve"),
        false,
    ),
    p(
        "tanga-serve",
        "tanga-page",
        "http://127.0.0.1:8671/tanga/",
        &[200],
        Body::Contains("<html"),
        false,
    ),
    // next.md: the console's /health stays 404 until the console is switched.
    p(
        "console",
        "console-root",
        "http://127.0.0.1:8080/",
        &[200, 302],
        Body::Any,
        false,
    ),
    p(
        "console",
        "console-health-404-until-switch",
        "http://127.0.0.1:8080/health",
        &[404],
        Body::Any,
        false,
    ),
    // Status only: the cloudflared body carries a connector id, never recorded.
    p(
        "cloudflared",
        "cloudflared-ready",
        "http://127.0.0.1:20241/ready",
        &[200],
        Body::Any,
        false,
    ),
];

/// Public endpoints, fetched from the box through Cloudflare.
pub const PUBLIC: &[Probe] = &[
    p(
        "public",
        "one.galion.app/health",
        "https://one.galion.app/health",
        &[200],
        Body::Exact("ok"),
        true,
    ),
    p(
        "public",
        "one.galion.app/",
        "https://one.galion.app/",
        &[200],
        Body::Contains("<html"),
        false,
    ),
    p(
        "public",
        "one.galion.app/me (unpaired)",
        "https://one.galion.app/me",
        &[200],
        Body::JsonBool("you", false),
        true,
    ),
    p(
        "public",
        "one.galion.app/session (unpaired 401)",
        "https://one.galion.app/session",
        &[401],
        Body::Any,
        false,
    ),
    p(
        "public",
        "one.galion.app/api/v1/analytics (unpaired 401)",
        "https://one.galion.app/api/v1/analytics",
        &[401],
        Body::Any,
        false,
    ),
    p(
        "public",
        "one.galion.app/tanga/",
        "https://one.galion.app/tanga/",
        &[200],
        Body::Contains("<html"),
        false,
    ),
];

/// Units expected `active` (systemctl is-active is read-only).
pub const UNITS: &[&str] = &[
    "galion-one.service",
    "galion-desk.service",
    "tuner.service",
    "openmind-memory.service",
    "hermes-rs.service",
    "galion-nerve.service",
    "galion-health.service",
    "cloudflared.service",
    "console.service",
    "link-watch.service",
];

/// Listening ports expected on the VPS loopback.
pub const PORTS: &[u16] = &[
    8091, 8002, 8420, 8642, 8646, 8650, 8660, 8670, 8671, 8080, 20241,
];

fn curl_line(name: &str, url: &str) -> String {
    // GET only: no -X, no -d, no -o FILE. Headers and the first 4 KiB of the
    // body go to stdout; nothing is written to disk.
    format!("echo '@@probe {name}'; curl -sS -m 6 -D - '{url}' 2>&1 | head -c 4096; echo; echo '@@end'\n")
}

/// The exact script sent to the VPS shell (stdin of `ssh HOST sh -s`).
pub fn vps_script() -> String {
    let mut s = String::from("set +e\n");
    for pr in VPS {
        s.push_str(&curl_line(pr.name, pr.url));
    }
    for u in UNITS {
        s.push_str(&format!(
            "echo '@@unit {u}'; systemctl is-active '{u}' 2>&1; echo '@@end'\n"
        ));
    }
    s.push_str("echo '@@ss'; ss -ltnH 2>&1; echo '@@end'\n");
    s
}

pub fn public_script() -> String {
    let mut s = String::from("set +e\n");
    for pr in PUBLIC {
        s.push_str(&curl_line(pr.name, pr.url));
    }
    s
}

/// Split runner output into (`@@kind arg`, body) blocks.
pub fn blocks(out: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut cur: Option<(String, String)> = None;
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("@@") {
            if head == "end" {
                if let Some(c) = cur.take() {
                    v.push(c);
                }
            } else {
                cur = Some((head.to_string(), String::new()));
            }
        } else if let Some((_, b)) = cur.as_mut() {
            b.push_str(line);
            b.push('\n');
        }
    }
    v
}

#[derive(Debug, Default, Clone)]
pub struct Got {
    pub status: Option<u16>,
    pub content_type: String,
    pub body: String,
    pub error: String,
}

/// Parse `curl -D -` output: last header block wins, body is the rest.
pub fn parse_curl(raw: &str) -> Got {
    let mut g = Got::default();
    let norm = raw.replace("\r\n", "\n");
    if !norm.starts_with("HTTP/") {
        g.error = norm.lines().next().unwrap_or("no response").to_string();
        return g;
    }
    let mut rest = norm.as_str();
    loop {
        let Some((head, body)) = rest.split_once("\n\n") else {
            g.error = "truncated headers".into();
            return g;
        };
        let mut lines = head.lines();
        g.status = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse().ok());
        g.content_type = lines
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default();
        if body.starts_with("HTTP/") {
            rest = body;
            continue;
        }
        g.body = body.trim_end_matches('\n').to_string();
        return g;
    }
}

pub fn judge(pr: &Probe, g: &Got) -> Result<String, String> {
    let code = g.status.ok_or(format!("no HTTP response: {}", g.error))?;
    let mut d = format!("HTTP {code} {}", g.content_type);
    if pr.show {
        let b: String = g.body.chars().take(100).collect();
        d.push_str(&format!(" body {:?}", b.trim()));
    } else {
        d.push_str(&format!(" ({} body bytes, not recorded)", g.body.len()));
    }
    if !pr.status.contains(&code) {
        return Err(format!("want HTTP {:?}; got {d}", pr.status));
    }
    let json =
        || serde_json::from_str::<Value>(g.body.trim()).map_err(|e| format!("not JSON ({e}); {d}"));
    let ok = match pr.body {
        Body::Any => true,
        Body::Exact(x) => g.body.trim() == x,
        Body::Contains(x) => g.body.contains(x),
        Body::JsonStr(k, v) => json()?[k] == v,
        Body::JsonBool(k, v) => json()?[k] == v,
    };
    if ok {
        Ok(d)
    } else {
        Err(format!("body contract {:?} not met; {d}", pr.body))
    }
}

fn run_sh(mut cmd: Command, script: &str) -> Result<String, String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(script.as_bytes())
        .map_err(|e| e.to_string())?;
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    if out.trim().is_empty() {
        return Err(format!(
            "no output (exit {:?}): {}",
            o.status.code(),
            String::from_utf8_lossy(&o.stderr)
                .lines()
                .next()
                .unwrap_or("")
        ));
    }
    Ok(out)
}

fn probe_suite(
    title: &str,
    service_of: &str,
    probes: &[Probe],
    out: &Result<String, String>,
) -> Vec<Suite> {
    let mut suites: Vec<Suite> = Vec::new();
    let found = out.as_ref().map(|o| blocks(o)).unwrap_or_default();
    for pr in probes {
        let svc = if service_of.is_empty() {
            pr.service
        } else {
            service_of
        };
        let idx = match suites.iter().position(|s| s.service == svc) {
            Some(i) => i,
            None => {
                suites.push(Suite::new(&format!("{title}:{svc}"), svc));
                suites.len() - 1
            }
        };
        let raw = found
            .iter()
            .find(|(h, _)| h == &format!("probe {}", pr.name))
            .map(|(_, b)| b.clone());
        suites[idx].check(pr.name, || match (&raw, out) {
            (_, Err(e)) => Err(format!("runner failed: {e}")),
            (None, _) => Err("probe produced no output".into()),
            (Some(r), _) => judge(pr, &parse_curl(r)),
        });
    }
    suites
}

/// Run the live probe. `ssh_host` None = public endpoints only.
pub fn run(ssh_host: Option<&str>, public: bool) -> (Vec<Suite>, String) {
    let mut suites = Vec::new();
    let mut notes = String::new();
    if let Some(host) = ssh_host {
        let mut c = Command::new("ssh");
        c.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            host,
            "sh",
            "-s",
        ]);
        let t = std::time::Instant::now();
        let out = run_sh(c, &vps_script());
        notes.push_str(&format!(
            "- VPS probes over `ssh {host} sh -s` (script: GET-only curl, `systemctl is-active`, `ss -ltnH`), one batch in {} ms.\n",
            t.elapsed().as_millis()
        ));
        suites.extend(probe_suite("live-vps", "", VPS, &out));
        let found = out.as_ref().map(|o| blocks(o)).unwrap_or_default();
        let mut units = Suite::new("live-vps:units", "systemd");
        for u in UNITS {
            let state = found
                .iter()
                .find(|(h, _)| h == &format!("unit {u}"))
                .map(|(_, b)| b.trim().to_string());
            units.check(u, || match &state {
                Some(s) if s == "active" => Ok("active".into()),
                Some(s) => Err(format!("is-active: {s}")),
                None => Err("no answer".into()),
            });
        }
        suites.push(units);
        let ss = found
            .iter()
            .find(|(h, _)| h == "ss")
            .map(|(_, b)| b.clone())
            .unwrap_or_default();
        let mut ports = Suite::new("live-vps:ports", "listeners");
        for port in PORTS {
            let needle = format!(":{port} ");
            ports.check(&format!("listen:{port}"), || {
                ss.lines()
                    .find(|l| l.contains(&needle))
                    .map(|l| l.split_whitespace().nth(3).unwrap_or("").to_string())
                    .ok_or("not listening".to_string())
            });
        }
        suites.push(ports);
    }
    if public {
        let t = std::time::Instant::now();
        let out = run_sh(Command::new("sh"), &public_script());
        notes.push_str(&format!(
            "- Public probes from the box with `curl` GET (no cookies, so they also check that One fails closed to strangers), one batch in {} ms.\n",
            t.elapsed().as_millis()
        ));
        suites.extend(probe_suite("live-public", "public", PUBLIC, &out));
    }
    (suites, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_is_get_only_and_writes_nothing() {
        for s in [vps_script(), public_script()] {
            for bad in [
                " -X", " -d ", "--data", "POST", " -o ", "--output", "restart", " stop", " start ",
                "kill", " > ", ">>", "rm ", "tee", "secrets", ".env",
            ] {
                assert!(!s.contains(bad), "live script contains {bad:?}");
            }
            for line in s.lines().filter(|l| l.contains("curl ")) {
                assert!(line.contains("curl -sS -m 6 -D - '"), "{line}");
            }
        }
        assert!(VPS.iter().all(|p| p.url.starts_with("http://127.0.0.1:")));
        assert!(PUBLIC
            .iter()
            .all(|p| p.url.starts_with("https://one.galion.app/")));
    }

    #[test]
    fn parses_curl_blocks_and_redirect_chains() {
        let out = "@@probe a\nHTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok\n@@end\n@@unit x.service\nactive\n@@end\n";
        let b = blocks(out);
        assert_eq!(b.len(), 2);
        let g = parse_curl(&b[0].1);
        assert_eq!(
            (g.status, g.content_type.as_str(), g.body.as_str()),
            (Some(200), "text/plain", "ok")
        );
        let pr = p(
            "s",
            "a",
            "http://127.0.0.1:1/",
            &[200],
            Body::Exact("ok"),
            true,
        );
        assert!(judge(&pr, &g).is_ok());
        let two = parse_curl("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 503 X\r\nContent-Type: application/json\r\n\r\n{\"ok\":false}");
        assert_eq!(two.status, Some(503));
        let pj = p(
            "s",
            "b",
            "http://127.0.0.1:1/",
            &[200],
            Body::JsonBool("ok", true),
            true,
        );
        assert!(judge(&pj, &two).is_err());
        assert!(parse_curl("curl: (7) Failed to connect").status.is_none());
    }

    #[test]
    fn hidden_bodies_are_not_recorded() {
        let g = parse_curl("HTTP/1.1 200 OK\r\n\r\n{\"connectorId\":\"abc\"}");
        let pr = p(
            "cloudflared",
            "r",
            "http://127.0.0.1:20241/ready",
            &[200],
            Body::Any,
            false,
        );
        let d = judge(&pr, &g).unwrap();
        assert!(!d.contains("abc"));
    }
}
