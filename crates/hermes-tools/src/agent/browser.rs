// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `browser`: open one public http(s) page in the Playwright-installed
//! Chromium headless shell and return its rendered text (+ links) or a PNG
//! screenshot saved in the workspace (for `vision_analyze`). The prior stack
//! had browser_navigate / browser_snapshot on local Chromium (snapshot cap
//! 15000 chars); this keeps the read part, headless, one page per call.
//!
//! Safety:
//! * the deny list checks `url` first (S7: only http(s), no loopback /
//!   private / metadata hosts, no file://);
//! * Chromium runs inside `openmind_sandbox::run`: cleared env, read-only
//!   root, only `<workspace>/.browser` writable, secret dirs hidden, wall
//!   timeout, output cap; a fresh profile per call (no cookies, no logins);
//! * every request the page makes (redirects, sub-resources) goes through a
//!   PAC script that sends loopback, private, link-local, CGNAT and
//!   IPv6-literal hosts, and hosts with no IPv4 address, to a dead proxy, so
//!   a public page can not redirect the browser into local services.

use super::{cut, Shared};
use hermes_rs::tools::{Tool, ToolCtx, ToolOutput};
use openmind_sandbox::Status;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

static ONE_BROWSER: Mutex<()> = Mutex::new(());

/// PAC script: private and local destinations go to a closed port.
pub const PAC: &str = r#"function FindProxyForURL(url, host) {
  var dead = "PROXY 127.0.0.1:9";
  if (host.indexOf(":") >= 0) return dead;
  if (isPlainHostName(host) || host == "localhost" || dnsDomainIs(host, ".localhost") || dnsDomainIs(host, ".local") || dnsDomainIs(host, ".internal")) return dead;
  var ip = dnsResolve(host);
  if (!ip) return dead;
  if (isInNet(ip, "127.0.0.0", "255.0.0.0") || isInNet(ip, "10.0.0.0", "255.0.0.0") ||
      isInNet(ip, "172.16.0.0", "255.240.0.0") || isInNet(ip, "192.168.0.0", "255.255.0.0") ||
      isInNet(ip, "169.254.0.0", "255.255.0.0") || isInNet(ip, "100.64.0.0", "255.192.0.0") ||
      isInNet(ip, "0.0.0.0", "255.0.0.0") || isInNet(ip, "224.0.0.0", "224.0.0.0")) return dead;
  return "DIRECT";
}"#;

pub fn pac_url() -> String {
    format!(
        "data:application/x-ns-proxy-autoconfig;base64,{}",
        super::vision::b64(PAC.as_bytes())
    )
}

/// Chromium arguments for one call.
pub fn chrome_args(
    bin: &str,
    profile: &str,
    url: &str,
    wait_ms: u64,
    shot: Option<&str>,
) -> Vec<String> {
    let mut a = vec![
        bin.to_string(),
        "--headless".into(),
        // the openmind sandbox is the sandbox (Chromium's own needs setuid/userns as root)
        "--no-sandbox".into(),
        "--disable-gpu".into(),
        "--disable-dev-shm-usage".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-extensions".into(),
        "--disable-background-networking".into(),
        "--disable-sync".into(),
        "--disable-component-update".into(),
        "--disable-default-apps".into(),
        "--metrics-recording-only".into(),
        "--mute-audio".into(),
        "--hide-scrollbars".into(),
        "--disable-features=Translate,MediaRouter,OptimizationHints,DialMediaRouteProvider".into(),
        format!("--user-data-dir={profile}"),
        format!("--proxy-pac-url={}", pac_url()),
        "--proxy-bypass-list=<-loopback>".into(),
        format!("--virtual-time-budget={wait_ms}"),
        "--window-size=1280,900".into(),
        "--lang=en-US".into(),
    ];
    match shot {
        Some(p) => a.push(format!("--screenshot={p}")),
        None => a.push("--dump-dom".into()),
    }
    a.push(url.to_string());
    a
}

pub struct Browser(pub Arc<Shared>);

impl Tool for Browser {
    fn name(&self) -> String {
        "browser".into()
    }
    fn description(&self) -> String {
        "Open a public http(s) URL in headless Chromium (Playwright install) and return the rendered page text, title and links (mode 'text', capped at 15000 chars), or save a 1280x900 PNG screenshot in the workspace (mode 'screenshot') to look at with vision_analyze. Local, private and file URLs are refused. One page per call, fresh profile, no logins.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "url": {"type": "string"},
            "mode": {"type": "string", "enum": ["text", "screenshot"], "default": "text"},
            "wait_ms": {"type": "integer", "minimum": 0, "maximum": 15000, "default": 3000}
        }, "required": ["url"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        if let Err(e) = sh.gate(ctx) {
            return ToolOutput::err(format!("refused: {e}"));
        }
        if let Value::Object(o) = args {
            if let Some(k) = o
                .keys()
                .find(|k| !["url", "mode", "wait_ms"].contains(&k.as_str()))
            {
                return ToolOutput::err(format!("refused: unknown argument {k:?}"));
            }
        }
        let url = args["url"].as_str().unwrap_or("").trim().to_string();
        let lu = url.to_ascii_lowercase();
        if !(lu.starts_with("http://") || lu.starts_with("https://"))
            || url.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return ToolOutput::err("refused: only plain http(s) URLs");
        }
        let pol0 = hermes_rs::denylist::Policy::workspace(&sh.cfg.workspace);
        if let Err(d) = hermes_rs::denylist::check_url(&url, &pol0, false) {
            return ToolOutput::err(hermes_rs::denylist::refusal_text(&d));
        }
        let mode = args["mode"].as_str().unwrap_or("text");
        if mode != "text" && mode != "screenshot" {
            return ToolOutput::err("refused: mode must be 'text' or 'screenshot'");
        }
        let wait = args["wait_ms"].as_u64().unwrap_or(3000).min(15_000);
        let Some(bin) = sh.cfg.browser_bin.clone() else {
            return ToolOutput::err("browser is not available on this host");
        };
        let ws = &sh.cfg.workspace;
        let bdir = ws.join(".browser");
        let id = hermes_rs::new_id("b_");
        let profile = bdir.join(format!("profile-{id}"));
        let home = bdir.join("home");
        let tmp = bdir.join("tmp");
        let shots = ws.join("browser");
        for d in [&profile, &home, &tmp, &shots] {
            if let Err(e) = std::fs::create_dir_all(d) {
                return ToolOutput::err(format!("browser dirs: {e}"));
            }
            sh.cfg.own_up(d);
        }
        let shot = (mode == "screenshot").then(|| shots.join(format!("shot-{id}.png")));
        let timeout = Duration::from_secs(sh.cfg.browser_timeout_s);
        let mut p = super::shell::policy(&sh.cfg, timeout);
        p.net = true;
        p.require_net_isolation = false;
        p.fs_write = vec![bdir.clone(), shots.clone(), PathBuf::from("/dev/null")];
        p.fs_read.push(bin.parent().unwrap_or(&bin).to_path_buf());
        p.env_set = vec![
            ("HOME".into(), home.display().to_string()),
            ("TMPDIR".into(), tmp.display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ];
        // V8 / Oilpan reserve a huge virtual cage (an RLIMIT_AS of even
        // 64 GiB aborts Chromium), so no address-space limit here: the wall
        // timeout, and memory.max when HERMES_RS_TOOLS_CGROUP is set, bound it
        p.mem_bytes = u64::MAX;
        p.cgroup_mem_bytes = 1536 << 20;
        p.cpu_secs = sh.cfg.browser_timeout_s * 2;
        p.nofile = 1024;
        p.cgroup_pids_max = 256;
        p.fsize_bytes = 64 << 20;
        p.output_cap = 4 << 20;
        // Chromium needs /proc/self. Where no private /proc can be mounted
        // (LXC + lxcfs) it would crash under the mask; the host /proc is
        // allowed only when the run drops to another, non-root user (then it
        // can not read the environ or memory of the gateway or any root
        // process), else the mask stays.
        p.proc_fallback = match sh.cfg.run_as {
            Some((u, _)) if u != 0 && u != openmind_sandbox::effective_uid() => {
                openmind_sandbox::ProcFallback::Host
            }
            _ => openmind_sandbox::ProcFallback::Mask,
        };
        let argv = chrome_args(
            &bin.display().to_string(),
            &profile.display().to_string(),
            &url,
            wait,
            shot.as_ref().map(|s| s.to_str().unwrap_or("")),
        );
        sh.log("browser", ctx, &format!("{mode} {url}"));
        let out = {
            let _one = ONE_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
            openmind_sandbox::run(&argv, &p)
        };
        let _ = std::fs::remove_dir_all(&profile);
        if out.status == Status::Error {
            return ToolOutput::err(format!(
                "browser sandbox could not start: {}",
                out.error.clone().unwrap_or_default()
            ));
        }
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        if out.timed_out {
            let tail: String = stderr.lines().rev().take(6).collect::<Vec<_>>().join(" / ");
            return ToolOutput::err(format!(
                "browser timed out after {} s; browser said: {}",
                sh.cfg.browser_timeout_s,
                hermes_rs::redact(&cut(&tail, 600))
            ));
        }
        if let Some(s) = shot {
            return match std::fs::metadata(&s) {
                Ok(m) if m.len() > 0 => {
                    let rel = s.strip_prefix(ws).unwrap_or(&s).display().to_string();
                    ToolOutput::ok(json!({"url": url, "screenshot": rel, "bytes": m.len(), "size": "1280x900", "hint": "call vision_analyze with this path to look at it"}).to_string())
                }
                _ => ToolOutput::err(format!(
                    "no screenshot was produced; browser said: {}",
                    hermes_rs::redact(&cut(stderr.trim(), 600))
                )),
            };
        }
        let html = String::from_utf8_lossy(&out.stdout).to_string();
        if html.trim().is_empty() {
            let how = match out.status {
                Status::Exited(c) => format!("exit {c}"),
                Status::Signaled(s) => format!("signal {s}"),
                Status::Error => "error".into(),
            };
            return ToolOutput::err(format!(
                "the page did not load (blocked, private address, or network error); browser {how} after {} ms; browser said: {}",
                out.wall.as_millis(),
                hermes_rs::redact(&cut(stderr.trim(), 600))
            ));
        }
        let page = super::html::to_text(&html, &url, 40);
        if page.text.is_empty() && page.title.is_empty() && page.links.is_empty() {
            return ToolOutput::err(
                "the page is empty: it did not load (private or local address blocked, DNS or network error) or rendered nothing",
            );
        }
        let cap = sh.cfg.browser_text_cap;
        let truncated = page.text.len() > cap;
        ToolOutput::ok(
            json!({
                "url": url,
                "title": page.title,
                "text": hermes_rs::redact(&cut(&page.text, cap)),
                "text_truncated": truncated,
                "links": page.links.iter().map(|(t, h)| json!({"text": cut(t, 120), "href": h})).collect::<Vec<_>>(),
                "html_bytes": html.len(),
            })
            .to_string(),
        )
    }
}
