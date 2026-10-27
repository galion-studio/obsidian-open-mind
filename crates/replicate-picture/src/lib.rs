// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! One picture from Replicate. Port of `/opt/galion/bin/replicate-picture.py`.
//!
//! Same behaviour as the Python helper:
//! * the token comes from the key file (default `/root/galion-secrets/replicate.key`),
//!   is trimmed and is never printed;
//! * models are tried in order (`flux-dev`, then `flux-schnell`), a failed call falls
//!   through to the next model and the last error becomes the exit message;
//! * `Prefer: wait=60`, 120 s timeout, `starting`/`processing` is polled every 2 s up
//!   to 40 times via `urls.get`; `failed`/`canceled` stops with the error;
//! * the picture is downloaded without the token, the parent dir is created, and
//!   stdout gets the model and the destination path, one per line.
//!
//! HTTPS goes through `curl` (the repo has no TLS stack: no openssl/asm rule). The
//! Authorization header and the request body travel in a curl config on stdin
//! (`-K -`), so the token never shows up in `ps`/`/proc/<pid>/cmdline`.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const MODELS: [&str; 2] = [
    "black-forest-labs/flux-dev",
    "black-forest-labs/flux-schnell",
];
pub const DEFAULT_KEY: &str = "/root/galion-secrets/replicate.key";
pub const DEFAULT_API: &str = "https://api.replicate.com/v1";

#[derive(Debug, Clone)]
pub struct Opts {
    pub prompt: String,
    pub out: PathBuf,
    pub aspect: String,
    pub key_file: PathBuf,
    pub api: String,
    pub curl: String,
    pub poll: Duration,
    pub polls: u32,
}

impl Opts {
    pub fn new(prompt: &str, out: &Path) -> Self {
        Opts {
            prompt: prompt.to_string(),
            out: out.to_path_buf(),
            aspect: "1:1".into(),
            key_file: PathBuf::from(DEFAULT_KEY),
            api: DEFAULT_API.into(),
            curl: "curl".into(),
            poll: Duration::from_secs(2),
            polls: 40,
        }
    }
}

/// Parse argv (without the program name). Accepts `--k v` and `--k=v`.
/// Test/ops knobs beyond the Python flags: `--key-file`, `--api`, `--curl`, `--poll-ms`.
pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::new("", Path::new(""));
    let (mut prompt, mut out) = (None, None);
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "-h" || a == "--help" {
            return Err(USAGE.into());
        }
        let (k, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let v = match inline {
            Some(v) => v,
            None => {
                i += 1;
                args.get(i)
                    .cloned()
                    .ok_or_else(|| format!("argument {k}: expected one argument"))?
            }
        };
        match k.as_str() {
            "--prompt" => prompt = Some(v),
            "--out" => out = Some(v),
            "--aspect" => o.aspect = v,
            "--key-file" => o.key_file = v.into(),
            "--api" => o.api = v.trim_end_matches('/').to_string(),
            "--curl" => o.curl = v,
            "--poll-ms" => o.poll = Duration::from_millis(v.parse().map_err(|_| "bad --poll-ms")?),
            _ => return Err(format!("unrecognized arguments: {a}\n{USAGE}")),
        }
        i += 1;
    }
    o.prompt = prompt.ok_or("the following arguments are required: --prompt")?;
    o.out = out
        .ok_or("the following arguments are required: --out")?
        .into();
    Ok(o)
}

pub const USAGE: &str = "usage: replicate-picture --prompt PROMPT --out OUT [--aspect ASPECT]";

/// Token from the key file, trimmed. Same message as Python when it is missing.
pub fn token_of(key: &Path) -> Result<String, String> {
    if !key.is_file() {
        return Err("missing replicate key file".into());
    }
    std::fs::read_to_string(key)
        .map(|s| s.trim().to_string())
        .map_err(|_| "missing replicate key file".into())
}

/// Escape a value for a double-quoted curl config string.
pub fn cfg_quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The curl config (stdin) for one API call. Contains the token: never log it.
pub fn curl_config(url: &str, token: &str, body: Option<&str>) -> String {
    let mut c = String::new();
    c.push_str(&format!("url = {}\n", cfg_quote(url)));
    c.push_str(&format!(
        "header = {}\n",
        cfg_quote(&format!("Authorization: Bearer {token}"))
    ));
    c.push_str("header = \"Content-Type: application/json\"\n");
    c.push_str("header = \"Prefer: wait=60\"\n");
    match body {
        Some(b) => {
            c.push_str("request = \"POST\"\n");
            c.push_str(&format!("data-binary = {}\n", cfg_quote(b)));
        }
        None => c.push_str("request = \"GET\"\n"),
    }
    c
}

/// An HTTP answer: status 0 means curl itself failed (like urllib's URLError).
pub struct Answer {
    pub status: u16,
    pub body: String,
    pub err: String,
}

const MARK: &str = "\n@@galion-status@@";

pub fn call(o: &Opts, url: &str, token: &str, body: Option<&str>) -> Answer {
    let child = Command::new(&o.curl)
        .args(["-sS", "--max-time", "120", "-K", "-", "-w"])
        .arg(format!("{MARK}%{{http_code}}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return Answer {
                status: 0,
                body: String::new(),
                err: format!("curl: {e}"),
            }
        }
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(curl_config(url, token, body).as_bytes());
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return Answer {
                status: 0,
                body: String::new(),
                err: format!("curl: {e}"),
            }
        }
    };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = match text.rfind(MARK) {
        Some(i) => (
            text[..i].to_string(),
            text[i + MARK.len()..].trim().to_string(),
        ),
        None => (text, "000".into()),
    };
    let err = String::from_utf8_lossy(&out.stderr)
        .trim()
        .replace(token, "<token>");
    Answer {
        status: code.parse().unwrap_or(0),
        body,
        err,
    }
}

/// Python's error text: `<code> <body[:180] with newlines as spaces>` for HTTP errors,
/// the exception class name for transport errors.
pub fn error_text(a: &Answer) -> String {
    if a.status == 0 {
        return if a.err.is_empty() {
            "URLError".into()
        } else {
            format!("URLError {}", first(&a.err, 180))
        };
    }
    format!("{} {}", a.status, first(&a.body, 180).replace('\n', " "))
}

fn first(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn out_of(doc: &Value) -> Option<String> {
    match doc.get("output") {
        Some(Value::String(s)) if s.starts_with("http") => Some(s.clone()),
        Some(Value::Array(a)) => a.first().and_then(|v| v.as_str()).map(str::to_string),
        _ => None,
    }
}

fn str_field(doc: &Value, k: &str) -> Option<String> {
    doc.get(k)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Picture URL of a prediction; polls while it is starting/processing.
pub fn output_url(o: &Opts, mut doc: Value, token: &str) -> Result<String, String> {
    if let Some(u) = out_of(&doc) {
        return Ok(u);
    }
    let status = str_field(&doc, "status").unwrap_or_default();
    let get = doc
        .pointer("/urls/get")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if matches!(status.as_str(), "starting" | "processing") {
        if let Some(get) = get {
            for _ in 0..o.polls {
                std::thread::sleep(o.poll);
                let a = call(o, &get, token, None);
                if !(200..300).contains(&a.status) {
                    return Err(error_text(&a));
                }
                doc = serde_json::from_str(&a.body).map_err(|e| format!("JSONDecodeError {e}"))?;
                if let Some(u) = out_of(&doc) {
                    return Ok(u);
                }
                if matches!(
                    doc.get("status").and_then(|v| v.as_str()),
                    Some("failed" | "canceled")
                ) {
                    return Err(str_field(&doc, "error").unwrap_or(status));
                }
            }
        }
    }
    Err(str_field(&doc, "error")
        .or_else(|| str_field(&doc, "status"))
        .unwrap_or_else(|| "no output".into()))
}

/// Download the picture (no Authorization header, like urlretrieve).
pub fn download(o: &Opts, url: &str, dest: &Path) -> Result<(), String> {
    if let Some(p) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let st = Command::new(&o.curl)
        .args(["-sSfL", "--max-time", "120", "-o"])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!(
            "download failed: curl exit {}",
            st.code().unwrap_or(-1)
        ))
    }
}

pub fn body_for(o: &Opts) -> String {
    json!({"input": {"prompt": o.prompt, "aspect_ratio": o.aspect,
        "output_format": "png", "num_outputs": 1}})
    .to_string()
}

/// Run the helper: Ok((model, dest)) or the exit message.
pub fn run(o: &Opts) -> Result<(String, PathBuf), String> {
    let token = token_of(&o.key_file)?;
    let body = body_for(o);
    let mut last = "no model".to_string();
    for model in MODELS {
        let url = format!("{}/models/{model}/predictions", o.api);
        let a = call(o, &url, &token, Some(&body));
        if !(200..300).contains(&a.status) {
            last = error_text(&a);
            continue;
        }
        let doc: Value = match serde_json::from_str(&a.body) {
            Ok(d) => d,
            Err(_) => {
                last = "JSONDecodeError".into();
                continue;
            }
        };
        let pic = output_url(o, doc, &token)?;
        download(o, &pic, &o.out)?;
        return Ok((model.to_string(), o.out.clone()));
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn args_like_argparse() {
        let o = parse_args(&s(&["--prompt", "a cat", "--out=/tmp/x.png"])).unwrap();
        assert_eq!(o.prompt, "a cat");
        assert_eq!(o.out, PathBuf::from("/tmp/x.png"));
        assert_eq!(o.aspect, "1:1");
        assert_eq!(o.key_file, PathBuf::from(DEFAULT_KEY));
        assert!(parse_args(&s(&["--out", "x"]))
            .unwrap_err()
            .contains("--prompt"));
        assert!(parse_args(&s(&["--prompt", "x"]))
            .unwrap_err()
            .contains("--out"));
        assert!(parse_args(&s(&["--prompt"])).is_err());
        assert!(parse_args(&s(&["--bogus", "1"]))
            .unwrap_err()
            .contains("unrecognized"));
    }

    #[test]
    fn config_quotes_and_holds_token_only_in_header() {
        let c = curl_config("https://h/p", "tok\"en", Some("{\"a\":\"b\\n\"}"));
        assert!(c.contains("header = \"Authorization: Bearer tok\\\"en\"\n"));
        assert!(c.contains("data-binary = \"{\\\"a\\\":\\\"b\\\\n\\\"}\"\n"));
        assert!(c.contains("request = \"POST\""));
        assert_eq!(c.matches("tok").count(), 1);
        assert!(curl_config("u", "t", None).contains("request = \"GET\""));
    }

    #[test]
    fn body_matches_python_payload() {
        let o = Opts::new("p \"q\"", Path::new("o"));
        let v: Value = serde_json::from_str(&body_for(&o)).unwrap();
        assert_eq!(v["input"]["prompt"], "p \"q\"");
        assert_eq!(v["input"]["aspect_ratio"], "1:1");
        assert_eq!(v["input"]["output_format"], "png");
        assert_eq!(v["input"]["num_outputs"], 1);
    }

    #[test]
    fn output_shapes() {
        let o = Opts::new("p", Path::new("o"));
        let ok = |v: Value| output_url(&o, v, "t");
        assert_eq!(
            ok(json!({"output": "https://x/a.png"})).unwrap(),
            "https://x/a.png"
        );
        assert_eq!(
            ok(json!({"output": ["https://x/b.png"]})).unwrap(),
            "https://x/b.png"
        );
        assert_eq!(
            ok(json!({"status": "failed", "error": "nsfw"})).unwrap_err(),
            "nsfw"
        );
        assert_eq!(ok(json!({"status": "succeeded"})).unwrap_err(), "succeeded");
        assert_eq!(ok(json!({})).unwrap_err(), "no output");
        // a non-http string output is not a picture (Python: startswith("http"))
        assert_eq!(
            ok(json!({"output": "data:x", "status": "succeeded"})).unwrap_err(),
            "succeeded"
        );
    }

    #[test]
    fn missing_key_message() {
        assert_eq!(
            token_of(Path::new("/nonexistent/replicate.key")).unwrap_err(),
            "missing replicate key file"
        );
    }

    #[test]
    fn error_text_like_python() {
        let a = Answer {
            status: 422,
            body: "line1\nline2".into(),
            err: String::new(),
        };
        assert_eq!(error_text(&a), "422 line1 line2");
        let long = Answer {
            status: 500,
            body: "x".repeat(400),
            err: String::new(),
        };
        assert_eq!(error_text(&long).len(), 4 + 180);
    }
}
