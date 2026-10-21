// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Device side of Hermes enrollment (docs/hermes-enroll.md). The device
//! always dials out; nothing here listens.
//!
//! * `request`: POST the signed pairing document to Hermes (202 = waiting for
//!   the owner, who compares the fingerprint with `omnode identity show`);
//! * `claim`: after approval, prove key possession with a fresh signed claim
//!   and store the scoped credential (`hermes-credential.json`, mode 600);
//! * `status`: ask Hermes whether this credential is still accepted.
//!
//! The credential is never printed. Plain `http://` is only used for a
//! loopback Hermes; `https://` goes through the system `curl` (no TLS stack
//! in omnode), with the bearer header fed on stdin, never on argv.

use crate::boot::write_atomic;
use crate::identity::Identity;
use crate::util::{hex, random_bytes};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const ENROLL_PATH: &str = "/openmind/enroll";
const MAX_RESPONSE: u64 = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

pub fn request_file(dir: &Path) -> PathBuf {
    dir.join("enroll.json")
}
pub fn credential_file(dir: &Path) -> PathBuf {
    dir.join("hermes-credential.json")
}

/// Canonical string of a credential claim (must match Hermes `claim_canonical`).
pub fn claim_canonical(device_id: &str, nonce: &str, ts: i64, url: &str) -> String {
    format!("omnode-claim-v1|{device_id}|{nonce}|{ts}|{url}")
}

#[derive(Debug, PartialEq, Eq)]
struct Url {
    https: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<Url, String> {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("URL must start with http:// or https://".into());
    };
    let (auth, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = if let Some(v6) = auth.strip_prefix('[') {
        let (h, tail) = v6.split_once(']').ok_or("bad IPv6 host")?;
        (h.to_string(), tail.strip_prefix(':'))
    } else {
        match auth.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (auth.to_string(), None),
        }
    };
    if host.is_empty() {
        return Err("URL has no host".into());
    }
    let port = match port {
        Some(p) => p.parse().map_err(|_| format!("bad port in URL: {p}"))?,
        None if https => 443,
        None => 80,
    };
    Ok(Url {
        https,
        host,
        port,
        path: path.to_string(),
    })
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// One HTTP exchange; returns (status, body). `bearer` is a secret: it goes
/// into the request head (http) or curl's stdin (https), never argv or logs.
pub fn http(
    method: &str,
    url: &str,
    body: Option<&str>,
    bearer: Option<&str>,
    allow_http: bool,
) -> Result<(u16, String), String> {
    let u = parse_url(url)?;
    if u.https {
        return via_curl(method, url, body, bearer);
    }
    if !is_loopback(&u.host) && !allow_http {
        return Err(format!(
            "refusing plain http to {} (not loopback); use https or --insecure-http on a network you own",
            u.host
        ));
    }
    let addr = (u.host.as_str(), u.port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {}: {e}", u.host))?
        .next()
        .ok_or("host did not resolve")?;
    let mut s =
        TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| format!("connect {addr}: {e}"))?;
    s.set_read_timeout(Some(TIMEOUT)).ok();
    s.set_write_timeout(Some(TIMEOUT)).ok();
    let host_hdr = if u.host.contains(':') {
        format!("[{}]:{}", u.host, u.port)
    } else {
        format!("{}:{}", u.host, u.port)
    };
    let mut head = format!(
        "{method} {} HTTP/1.1\r\nHost: {host_hdr}\r\nUser-Agent: omnode/{}\r\nAccept: application/json\r\nConnection: close\r\n",
        u.path,
        env!("CARGO_PKG_VERSION")
    );
    if let Some(t) = bearer {
        head.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    let b = body.unwrap_or("");
    if body.is_some() {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())
        .and_then(|_| s.write_all(b.as_bytes()))
        .map_err(|e| format!("send: {e}"))?;
    let mut raw = Vec::new();
    s.take(MAX_RESPONSE)
        .read_to_end(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or("malformed HTTP response")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("malformed HTTP status line")?;
    Ok((status, body.to_string()))
}

fn via_curl(
    method: &str,
    url: &str,
    body: Option<&str>,
    bearer: Option<&str>,
) -> Result<(u16, String), String> {
    if body.is_some() && bearer.is_some() {
        return Err("internal: https request with both a body and a bearer".into());
    }
    let mut args: Vec<&str> = vec![
        "-sS",
        "--proto",
        "=https",
        "--max-time",
        "15",
        "--max-filesize",
        "65536",
        "-X",
        method,
        "-H",
        "Accept: application/json",
        "-w",
        "\n%{http_code}",
    ];
    if body.is_some() {
        args.extend([
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
        ]);
    }
    if bearer.is_some() {
        args.extend(["-H", "@-"]);
    }
    args.push(url);
    let mut child = Command::new("curl")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("https needs the system curl: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("curl stdin")?;
        let feed = match (body, bearer) {
            (Some(b), _) => b.to_string(),
            (None, Some(t)) => format!("Authorization: Bearer {t}\n"),
            _ => String::new(),
        };
        stdin
            .write_all(feed.as_bytes())
            .map_err(|e| format!("curl stdin: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = text.rsplit_once('\n').ok_or("curl: no status")?;
    let code = code.trim().parse().map_err(|_| "curl: bad status")?;
    Ok((code, body.to_string()))
}

fn json_of(body: &str) -> Value {
    serde_json::from_str(body)
        .unwrap_or_else(|_| json!({"raw": body.chars().take(200).collect::<String>()}))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))
}

/// Node routes live next to the enroll route: `<base>/openmind/node/<what>`.
pub fn node_url(enroll_url: &str, what: &str) -> Result<String, String> {
    let base = enroll_url
        .strip_suffix(ENROLL_PATH)
        .ok_or_else(|| format!("enroll URL must end with {ENROLL_PATH}"))?;
    Ok(format!("{base}/openmind/node/{what}"))
}

/// Outcome codes for the CLI: 0 done, 3 waiting for the owner, else error.
pub enum Outcome {
    Done(Value),
    Pending(Value),
}

/// `omnode enroll request URL`: POST the signed document produced by
/// `pair::build`. `doc` is what `omnode pair` would print.
pub fn request(dir: &Path, url: &str, doc: &Value, allow_http: bool) -> Result<Outcome, String> {
    if !url.ends_with(ENROLL_PATH) {
        return Err(format!("enroll URL must end with {ENROLL_PATH}"));
    }
    let (code, body) = http("POST", url, Some(&doc.to_string()), None, allow_http)?;
    let resp = json_of(&body);
    match code {
        202 => {
            let rec = json!({
                "url": url,
                "device_id": doc["payload"]["device_id"],
                "fingerprint": doc["payload"]["fingerprint"],
                "requested_at": crate::zagreb::now_rfc3339(),
            });
            write_atomic(
                &request_file(dir),
                serde_json::to_string_pretty(&rec)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            Ok(Outcome::Pending(
                json!({"http": code, "hermes": resp, "next": "owner approves the fingerprint, then `omnode enroll claim`"}),
            ))
        }
        _ => Err(format!(
            "Hermes refused the enrollment (HTTP {code}): {}",
            resp["error"].as_str().unwrap_or(&body)
        )),
    }
}

/// `omnode enroll claim`: fresh signed claim; on 200 store the credential.
pub fn claim(dir: &Path, id: &Identity, allow_http: bool) -> Result<Outcome, String> {
    let rec =
        read_json(&request_file(dir)).map_err(|e| format!("no enrollment request yet ({e})"))?;
    let url = rec["url"]
        .as_str()
        .ok_or("enroll.json has no url")?
        .to_string();
    let nonce = hex(&random_bytes::<12>().map_err(|e| e.to_string())?);
    let ts = crate::zagreb::now_unix();
    let dev = &id.state.device_id;
    let sig = id.sign(claim_canonical(dev, &nonce, ts, &url).as_bytes());
    let body = json!({"device_id": dev, "nonce": nonce, "ts": ts, "signature": sig});
    let (code, text) = http(
        "POST",
        &format!("{url}/claim"),
        Some(&body.to_string()),
        None,
        allow_http,
    )?;
    let mut resp = json_of(&text);
    match code {
        200 => {
            let token = resp["token"]
                .as_str()
                .filter(|t| t.starts_with("omn1_"))
                .ok_or("Hermes answered 200 without a credential")?
                .to_string();
            if let Some(o) = resp.as_object_mut() {
                o.remove("token");
            }
            let cred = json!({
                "url": url, "device_id": dev, "token": token,
                "scopes": resp["scopes"], "expires": resp["expires"],
                "issued_at": crate::zagreb::now_rfc3339(),
            });
            write_atomic(
                &credential_file(dir),
                serde_json::to_string_pretty(&cred)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            Ok(Outcome::Done(json!({"http": code, "hermes": resp,
                "credential_file": credential_file(dir).display().to_string()})))
        }
        202 => Ok(Outcome::Pending(json!({"http": code, "hermes": resp}))),
        _ => Err(format!(
            "Hermes refused the claim (HTTP {code}): {}",
            resp["error"].as_str().unwrap_or(&text)
        )),
    }
}

/// `omnode enroll status [--scope S]`: is this credential accepted right now?
pub fn status(dir: &Path, scope: Option<&str>, allow_http: bool) -> Result<Outcome, String> {
    let cred = read_json(&credential_file(dir)).map_err(|e| format!("no credential ({e})"))?;
    let url = cred["url"].as_str().ok_or("credential has no url")?;
    let token = cred["token"].as_str().ok_or("credential has no token")?;
    let target = match scope {
        Some(s) => {
            if !s
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
            {
                return Err("scope: only a-z 0-9 - _ .".into());
            }
            format!("{}?scope={s}", node_url(url, "allowed")?)
        }
        None => node_url(url, "whoami")?,
    };
    let (code, text) = http("GET", &target, None, Some(token), allow_http)?;
    let resp = json_of(&text);
    if code == 200 {
        Ok(Outcome::Done(json!({"http": code, "hermes": resp})))
    } else {
        Err(format!(
            "Hermes does not accept this node (HTTP {code}): {}",
            resp["error"].as_str().unwrap_or(&text)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            parse_url("http://127.0.0.1:19320/openmind/enroll").unwrap(),
            Url {
                https: false,
                host: "127.0.0.1".into(),
                port: 19320,
                path: "/openmind/enroll".into()
            }
        );
        assert_eq!(parse_url("https://h.example.invalid/x").unwrap().port, 443);
        assert_eq!(parse_url("http://[::1]:8/x").unwrap().host, "::1");
        assert!(parse_url("ftp://x").is_err());
        assert!(parse_url("http://:80/").is_err());
        assert_eq!(
            node_url("https://h.example.invalid/openmind/enroll", "whoami").unwrap(),
            "https://h.example.invalid/openmind/node/whoami"
        );
        assert!(node_url("https://h.example.invalid/other", "whoami").is_err());
    }

    #[test]
    fn plain_http_only_to_loopback() {
        let e = http("GET", "http://192.0.2.1:9/x", None, None, false).unwrap_err();
        assert!(e.contains("refusing plain http"), "{e}");
    }

    #[test]
    fn claim_string_is_stable() {
        assert_eq!(
            claim_canonical(
                "omn-0123456789ab",
                "00",
                5,
                "http://127.0.0.1:1/openmind/enroll"
            ),
            "omnode-claim-v1|omn-0123456789ab|00|5|http://127.0.0.1:1/openmind/enroll"
        );
    }
}
