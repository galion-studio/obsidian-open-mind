// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Port of `/opt/galion/bin/pw.py` (Playwright check for the console).
//!
//! For every path: a fresh browser context, the `galion_admin` cookie from the
//! cookie file (if present; the value is never printed), a 390x844 viewport,
//! navigation with "networkidle" (20 s), 600 ms settle, the same in-page probe
//! (title, h1 x4, svg count, theme toggle label, dock words, horizontal overflow,
//! first 400 chars of text), page errors (240 chars each) and a viewport PNG.
//! The report is a JSON list written to `<out>/report.json` and printed with
//! indent 2 and ASCII escapes, like `json.dumps(report, indent=2)`.

pub mod cdp;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_PATHS: [&str; 7] = [
    "/console",
    "/hermes",
    "/context",
    "/cache",
    "/instances",
    "/analytics",
    "/security",
];
pub const DEFAULT_OUT: &str = "/opt/galion/hermes/cache/pw";
pub const DEFAULT_COOKIE: &str = "/root/galion-secrets/pw-cookie";
pub const DEFAULT_BASE: &str = "http://127.0.0.1:8080";

/// The in-page probe, verbatim from pw.py (Python's `\\s` there is JS `\s`).
pub const PROBE_JS: &str = r#"() => {
              const full = document.body ? document.body.innerText : "";
              const svgs = document.querySelectorAll("svg").length;
              const theme = document.querySelector('[aria-label="Use light"], [aria-label="Use dark"]');
              const dock = ["Desk", "Instances", "Research", "Nerve", "Defence"].filter((w) => full.includes(w));
              const wide = document.documentElement.scrollWidth > window.innerWidth + 8;
              return {
                title: document.title,
                h1: [...document.querySelectorAll("h1")].map((n) => n.innerText.trim()).slice(0, 4),
                svgs,
                theme: theme ? theme.getAttribute("aria-label") : null,
                dock,
                wide,
                text: full.replace(/\s+/g, " ").slice(0, 400),
              };
            }"#;

#[derive(Debug, Clone)]
pub struct Opts {
    pub paths: Vec<String>,
    pub base: String,
    pub out: PathBuf,
    pub cookie_file: PathBuf,
    pub chrome: Option<PathBuf>,
}

pub const USAGE: &str = "usage: pw-check [/path ...] [--base URL] [--out-dir DIR] [--cookie-file FILE] [--chrome BIN]\n\
default paths: /console /hermes /context /cache /instances /analytics /security";

pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        paths: vec![],
        base: DEFAULT_BASE.into(),
        out: DEFAULT_OUT.into(),
        cookie_file: DEFAULT_COOKIE.into(),
        chrome: std::env::var_os("PW_CHROME").map(PathBuf::from),
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "-h" || a == "--help" {
            return Err(USAGE.into());
        }
        if let Some(k) = a.strip_prefix("--") {
            let (k, v) = match k.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => {
                    i += 1;
                    let v = args.get(i).cloned().ok_or(format!("--{k} needs a value"))?;
                    (k.to_string(), v)
                }
            };
            match k.as_str() {
                "base" => o.base = v.trim_end_matches('/').to_string(),
                "out-dir" => o.out = v.into(),
                "cookie-file" => o.cookie_file = v.into(),
                "chrome" => o.chrome = Some(v.into()),
                _ => return Err(format!("unknown flag --{k}\n{USAGE}")),
            }
        } else {
            o.paths.push(a.clone());
        }
        i += 1;
    }
    if o.paths.is_empty() {
        o.paths = DEFAULT_PATHS.iter().map(|s| s.to_string()).collect();
    }
    Ok(o)
}

/// `base + path`, adding the leading slash like pw.py.
pub fn url_for(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// pw.py: `(OUT / (path.strip("/").replace("/", "-") or "root")).with_suffix(".png")`.
pub fn shot_name(path: &str) -> String {
    let stem = path.trim_matches('/').replace('/', "-");
    let stem = if stem.is_empty() {
        "root".to_string()
    } else {
        stem
    };
    // pathlib.with_suffix: replace the suffix (last '.' not at position 0).
    match stem.rfind('.') {
        Some(i) if i > 0 && i + 1 < stem.len() => format!("{}.png", &stem[..i]),
        _ => format!("{stem}.png"),
    }
}

/// Chromium to use: --chrome / PW_CHROME, then Playwright's headless shell in
/// ~/.cache/ms-playwright (what pw.py used), then chromium / google-chrome on PATH.
pub fn find_chrome(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/root".into());
    let cache = home.join(".cache/ms-playwright");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("chromium_headless_shell-")
        })
        .flat_map(|e| {
            [
                e.path().join("chrome-linux/headless_shell"),
                e.path()
                    .join("chrome-headless-shell-linux64/chrome-headless-shell"),
            ]
        })
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    if let Some(p) = found.pop() {
        return Some(p);
    }
    let path = std::env::var_os("PATH")?;
    for name in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Result of PROBE_JS (field order = the JS object = pw.py's output order).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Probe {
    pub title: String,
    pub h1: Vec<String>,
    pub svgs: u64,
    pub theme: Option<String>,
    pub dock: Vec<String>,
    pub wide: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Row {
    #[serde(flatten)]
    pub probe: Probe,
    pub path: String,
    pub errors: Vec<String>,
    pub shot: String,
}

/// Playwright's `str(err)` for a pageerror: the message without the `Name: ` prefix.
pub fn page_error(details: &Value) -> String {
    let ex = details.get("exception");
    let desc = ex
        .and_then(|e| e.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let class = ex
        .and_then(|e| e.get("className"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let text = match desc {
        Some(d) if !class.is_empty() => {
            let line = d.lines().next().unwrap_or("").to_string();
            match line.split_once(": ") {
                Some((name, msg)) if !name.contains(' ') => msg.to_string(),
                _ if line == class => String::new(),
                _ => line,
            }
        }
        Some(d) => d,
        None => match ex.and_then(|e| e.get("value")) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => details
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
    };
    text.chars().take(240).collect()
}

/// `json.dumps(obj, indent=2)` (ensure_ascii=True): pretty JSON with non-ASCII as \uXXXX.
pub fn py_dumps_indent2<T: Serialize>(v: &T) -> String {
    let s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "null".into());
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            o.push(c);
        } else {
            let mut b = [0u16; 2];
            for u in c.encode_utf16(&mut b) {
                o.push_str(&format!("\\u{:04x}", u));
            }
        }
    }
    o
}

/// Standard base64 decoder (Page.captureScreenshot data).
pub fn b64decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let body = clean
        .strip_suffix(b"==")
        .or_else(|| clean.strip_suffix(b"="))
        .unwrap_or(&clean);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c).ok_or("bad base64")? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        match chunk.len() {
            4 => out.extend_from_slice(&bytes),
            3 => out.extend_from_slice(&bytes[..2]),
            2 => out.push(bytes[0]),
            _ => return Err("bad base64 length".into()),
        }
    }
    Ok(out)
}

const NAV_TIMEOUT: Duration = Duration::from_millis(20000);
const STEP: Duration = Duration::from_secs(30);

fn check_one(b: &mut cdp::Browser, o: &Opts, path: &str, cookie: &str) -> Result<Row, String> {
    let ctx = b.call(None, "Target.createBrowserContext", json!({}), STEP)?;
    let ctx_id = ctx["browserContextId"].as_str().unwrap_or("").to_string();
    let t = b.call(
        None,
        "Target.createTarget",
        json!({"url": "about:blank", "browserContextId": ctx_id}),
        STEP,
    )?;
    let target = t["targetId"].as_str().unwrap_or("").to_string();
    let a = b.call(
        None,
        "Target.attachToTarget",
        json!({"targetId": target, "flatten": true}),
        STEP,
    )?;
    let sid = a["sessionId"].as_str().unwrap_or("").to_string();
    let s = Some(sid.as_str());
    b.call(s, "Page.enable", json!({}), STEP)?;
    b.call(s, "Runtime.enable", json!({}), STEP)?;
    b.call(
        s,
        "Page.setLifecycleEventsEnabled",
        json!({"enabled": true}),
        STEP,
    )?;
    b.call(
        s,
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 844, "deviceScaleFactor": 1, "mobile": false}),
        STEP,
    )?;
    if !cookie.is_empty() {
        b.call(
            s,
            "Network.setCookie",
            json!({"name": "galion_admin", "value": cookie, "url": o.base}),
            STEP,
        )?;
    }
    let url = url_for(&o.base, path);
    let nav = b.call(s, "Page.navigate", json!({"url": url}), NAV_TIMEOUT)?;
    if let Some(e) = nav.get("errorText").and_then(Value::as_str) {
        return Err(format!("Page.goto: {e} at {url}"));
    }
    let loader = nav["loaderId"].as_str().unwrap_or("").to_string();
    let sid2 = sid.clone();
    b.wait_event(
        &|v: &Value| {
            v["method"] == "Page.lifecycleEvent"
                && v["sessionId"] == sid2.as_str()
                && v["params"]["name"] == "networkIdle"
                && (loader.is_empty() || v["params"]["loaderId"] == loader.as_str())
        },
        NAV_TIMEOUT,
    )
    .map_err(|e| format!("Page.goto: {e} waiting for networkidle at {url}"))?;
    std::thread::sleep(Duration::from_millis(600));
    let ev = b.call(
        s,
        "Runtime.evaluate",
        json!({"expression": format!("({PROBE_JS})()"), "returnByValue": true}),
        STEP,
    )?;
    if let Some(ex) = ev.get("exceptionDetails") {
        return Err(format!("Page.evaluate: {}", page_error(ex)));
    }
    let probe: Probe = serde_json::from_value(ev["result"]["value"].clone())
        .map_err(|e| format!("Page.evaluate: unexpected result: {e}"))?;
    let shot = o.out.join(shot_name(path));
    let png = b.call(s, "Page.captureScreenshot", json!({"format": "png"}), STEP)?;
    let bytes = b64decode(png["data"].as_str().unwrap_or(""))?;
    std::fs::write(&shot, bytes).map_err(|e| format!("{}: {e}", shot.display()))?;
    b.pump();
    let errors = b
        .events
        .iter()
        .filter(|v| v["method"] == "Runtime.exceptionThrown" && v["sessionId"] == sid.as_str())
        .map(|v| page_error(&v["params"]["exceptionDetails"]))
        .collect();
    let _ = b.call(
        None,
        "Target.closeTarget",
        json!({"targetId": target}),
        STEP,
    );
    let _ = b.call(
        None,
        "Target.disposeBrowserContext",
        json!({"browserContextId": ctx_id}),
        STEP,
    );
    b.events.retain(|v| v["sessionId"] != sid.as_str());
    Ok(Row {
        probe,
        path: path.to_string(),
        errors,
        shot: shot.display().to_string(),
    })
}

/// Run every path; on success writes report.json and returns the printed text.
pub fn run(o: &Opts) -> Result<String, String> {
    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let cookie = if o.cookie_file.exists() {
        std::fs::read_to_string(&o.cookie_file)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let chrome =
        find_chrome(o.chrome.as_deref()).ok_or("no chromium found (set PW_CHROME or --chrome)")?;
    let mut b = cdp::Browser::launch(&chrome)?;
    let mut report = Vec::new();
    for p in &o.paths {
        report.push(check_one(&mut b, o, p, &cookie)?);
    }
    b.close();
    let text = py_dumps_indent2(&report);
    std::fs::write(o.out.join("report.json"), &text).map_err(|e| e.to_string())?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shot_names_like_pathlib() {
        assert_eq!(shot_name("/console"), "console.png");
        assert_eq!(shot_name("/"), "root.png");
        assert_eq!(shot_name(""), "root.png");
        assert_eq!(shot_name("/a/b/"), "a-b.png");
        assert_eq!(shot_name("/x.html"), "x.png");
        assert_eq!(shot_name("/.well"), ".well.png");
    }

    #[test]
    fn urls() {
        assert_eq!(
            url_for(DEFAULT_BASE, "/hermes"),
            "http://127.0.0.1:8080/hermes"
        );
        assert_eq!(
            url_for(DEFAULT_BASE, "hermes"),
            "http://127.0.0.1:8080/hermes"
        );
    }

    #[test]
    fn args() {
        let a = |v: &[&str]| parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(a(&[]).unwrap().paths.len(), 7);
        let o = a(&[
            "/console",
            "--base=http://h:1/",
            "hermes",
            "--out-dir",
            "/tmp/x",
        ])
        .unwrap();
        assert_eq!(o.paths, ["/console", "hermes"]);
        assert_eq!(o.base, "http://h:1");
        assert_eq!(o.out, PathBuf::from("/tmp/x"));
        assert!(a(&["--nope", "1"]).is_err());
    }

    #[test]
    fn base64_roundtrip_known() {
        assert_eq!(b64decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(b64decode("aGVsbG8h").unwrap(), b"hello!");
        assert_eq!(b64decode("aGk=").unwrap(), b"hi");
        assert_eq!(b64decode("iVBORw0KGgo=").unwrap(), b"\x89PNG\r\n\x1a\n");
        assert!(b64decode("a$==").is_err());
    }

    #[test]
    fn errors_like_playwright() {
        let e = json!({"exception": {"className": "Error", "description": "Error: boom\n    at x.js:1"}});
        assert_eq!(page_error(&e), "boom");
        let t = json!({"exception": {"className": "TypeError", "description": "TypeError: a is undefined\n at"}});
        assert_eq!(page_error(&t), "a is undefined");
        let s = json!({"exception": {"type": "string", "value": "plain"}});
        assert_eq!(page_error(&s), "plain");
        let long = json!({"exception": {"type": "string", "value": "x".repeat(500)}});
        assert_eq!(page_error(&long).len(), 240);
    }

    #[test]
    fn dumps_like_python() {
        let r = vec![Row {
            probe: Probe {
                title: "Galion · ž".into(),
                h1: vec![],
                svgs: 2,
                theme: None,
                dock: vec!["Desk".into()],
                wide: false,
                text: "😀".into(),
            },
            path: "/console".into(),
            errors: vec![],
            shot: "/o/console.png".into(),
        }];
        let s = py_dumps_indent2(&r);
        assert!(s.starts_with("[\n  {\n    \"title\": \"Galion \\u00b7 \\u017e\",\n    \"h1\": [],\n    \"svgs\": 2,\n    \"theme\": null,"));
        assert!(s.contains("\"text\": \"\\ud83d\\ude00\",\n    \"path\": \"/console\",\n    \"errors\": [],\n    \"shot\": \"/o/console.png\"\n  }\n]"));
        assert!(s.is_ascii());
    }
}
