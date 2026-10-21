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

/// Canonical string of a signed status poll (must match Hermes `status_canonical`).
pub fn status_canonical(device_id: &str, ts: i64, url: &str) -> String {
    format!("omnode-status-v1|{device_id}|{ts}|{url}")
}

/// Canonical string of a tunnel challenge answer (Hermes `tunnel_canonical`).
pub fn tunnel_canonical(
    device_id: &str,
    session: &str,
    id: &str,
    challenge: &str,
    url: &str,
) -> String {
    format!("omnode-tunnel-v1|{device_id}|{session}|{id}|{challenge}|{url}")
}

/// Short code shown next to the QR (Hermes `pairing_code`): 40 bits of
/// SHA-256(`omnode-code-v1|public_key|nonce`), Crockford base32, `XXXX-XXXX`.
pub fn pairing_code(public_key: &str, nonce: &str) -> String {
    use sha2::{Digest, Sha256};
    const A: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let d = Sha256::digest(format!("omnode-code-v1|{public_key}|{nonce}").as_bytes());
    let v = d[..5].iter().fold(0u64, |acc, b| acc << 8 | u64::from(*b));
    let mut s = String::with_capacity(9);
    for i in (0..8).rev() {
        s.push(A[((v >> (i * 5)) & 31) as usize] as char);
        if i == 4 {
            s.push('-');
        }
    }
    s
}

/// Signed status poll while waiting for the owner. Returns Hermes' HTTP
/// status and JSON: 202 pending, 200 approved, 403 rejected/revoked,
/// 410 expired. Hermes writes nothing for a poll and never sends a token.
pub fn poll(url: &str, id: &Identity, net: Net) -> Result<(u16, Value), String> {
    let ts = crate::zagreb::now_unix();
    let dev = &id.state.device_id;
    let sig = id.sign(status_canonical(dev, ts, url).as_bytes());
    let body = json!({"device_id": dev, "ts": ts, "signature": sig});
    let (code, text) = http(
        "POST",
        &format!("{url}/status"),
        Some(&body.to_string()),
        None,
        net,
    )?;
    Ok((code, json_of(&text)))
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

/// How omnode reaches Hermes. `https://` always goes through the system
/// `curl` (omnode carries no TLS stack). Plain `http://` is used only for a
/// loopback Hermes unless `allow_http` (`--insecure-http`, a LAN test on a
/// network you own). `via_curl` (`--via-curl`) sends plain http through curl
/// too, so the curl code path that a phone uses for https can be tested
/// against a local server.
#[derive(Debug, Clone, Copy, Default)]
pub struct Net {
    pub allow_http: bool,
    pub via_curl: bool,
}

impl Net {
    pub fn new(allow_http: bool, via_curl: bool) -> Net {
        Net {
            allow_http,
            via_curl,
        }
    }
}

/// Refuse plain http to a non-loopback host unless allowed.
fn check_plain(u: &Url, net: Net) -> Result<(), String> {
    if !u.https && !is_loopback(&u.host) && !net.allow_http {
        return Err(format!(
            "refusing plain http to {} (not loopback); use https or --insecure-http on a network you own",
            u.host
        ));
    }
    Ok(())
}

fn connect(u: &Url, timeout: Duration) -> Result<TcpStream, String> {
    let addr = (u.host.as_str(), u.port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {}: {e}", u.host))?
        .next()
        .ok_or("host did not resolve")?;
    let s =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect {addr}: {e}"))?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    Ok(s)
}

fn request_head(method: &str, u: &Url, bearer: Option<&str>, accept: &str) -> String {
    let host_hdr = if u.host.contains(':') {
        format!("[{}]:{}", u.host, u.port)
    } else {
        format!("{}:{}", u.host, u.port)
    };
    let mut head = format!(
        "{method} {} HTTP/1.1\r\nHost: {host_hdr}\r\nUser-Agent: omnode/{}\r\nAccept: {accept}\r\nConnection: close\r\n",
        u.path,
        env!("CARGO_PKG_VERSION")
    );
    if let Some(t) = bearer {
        head.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    head
}

/// One HTTP exchange; returns (status, body). `bearer` is a secret: it goes
/// into the request head (http) or curl's stdin config (curl), never argv
/// or logs.
pub fn http(
    method: &str,
    url: &str,
    body: Option<&str>,
    bearer: Option<&str>,
    net: Net,
) -> Result<(u16, String), String> {
    let u = parse_url(url)?;
    check_plain(&u, net)?;
    if u.https || net.via_curl {
        return via_curl(method, url, u.https, body, bearer);
    }
    let mut s = connect(&u, TIMEOUT)?;
    let mut head = request_head(method, &u, bearer, "application/json");
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

/// A value inside a double-quoted curl config string.
fn curl_quote(s: &str) -> Result<String, String> {
    if s.chars().any(|c| c.is_control() && c != '\t') {
        return Err("internal: control character in a curl config value".into());
    }
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// curl config (`-K -`, read from stdin): headers, bearer and body never
/// appear on argv, where other local users could read them in /proc.
fn curl_config(
    method: &str,
    accept: &str,
    body: Option<&str>,
    bearer: Option<&str>,
) -> Result<String, String> {
    let mut c = String::new();
    c.push_str(&format!("request = {}\n", curl_quote(method)?));
    c.push_str(&format!(
        "header = {}\n",
        curl_quote(&format!("Accept: {accept}"))?
    ));
    c.push_str(&format!(
        "header = {}\n",
        curl_quote(&format!("User-Agent: omnode/{}", env!("CARGO_PKG_VERSION")))?
    ));
    if let Some(t) = bearer {
        c.push_str(&format!(
            "header = {}\n",
            curl_quote(&format!("Authorization: Bearer {t}"))?
        ));
    }
    if let Some(b) = body {
        c.push_str("header = \"Content-Type: application/json\"\n");
        c.push_str(&format!("data-binary = {}\n", curl_quote(b)?));
    }
    Ok(c)
}

fn curl_proto(https: bool) -> &'static str {
    if https {
        "=https"
    } else {
        "=http"
    }
}

fn via_curl(
    method: &str,
    url: &str,
    https: bool,
    body: Option<&str>,
    bearer: Option<&str>,
) -> Result<(u16, String), String> {
    let config = curl_config(method, "application/json", body, bearer)?;
    let args: Vec<&str> = vec![
        "-sS",
        "--proto",
        curl_proto(https),
        "--max-time",
        "15",
        "--max-filesize",
        "65536",
        "-w",
        "\n%{http_code}",
        "-K",
        "-",
        url,
    ];
    let mut child = Command::new("curl")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("https needs the system curl (Termux: pkg install curl): {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("curl stdin")?;
        stdin
            .write_all(config.as_bytes())
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

/// A long-lived GET whose body is read line by line (the node tunnel).
pub enum Stream {
    Tcp(std::io::BufReader<TcpStream>),
    Curl(
        std::process::Child,
        std::io::BufReader<std::process::ChildStdout>,
    ),
}

impl Stream {
    /// Next line without the line ending; `Ok(None)` at end of stream.
    pub fn line(&mut self) -> std::io::Result<Option<String>> {
        use std::io::BufRead;
        let mut s = String::new();
        let n = match self {
            Stream::Tcp(r) => Read::take(&mut *r, 16 * 1024).read_line(&mut s)?,
            Stream::Curl(_, r) => Read::take(&mut *r, 16 * 1024).read_line(&mut s)?,
        };
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(s.trim_end_matches(['\r', '\n']).to_string()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Stream::Curl(c, _) = self {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Open `GET url` (Bearer) and return (status, stream positioned at the
/// body). `idle` = give up when nothing arrives for that long (Hermes pings
/// well inside it); `max` = hard stop for the whole connection.
pub fn open_stream(
    url: &str,
    bearer: &str,
    net: Net,
    idle: Duration,
    max: Option<Duration>,
) -> Result<(u16, Stream), String> {
    let u = parse_url(url)?;
    check_plain(&u, net)?;
    let mut st = if u.https || net.via_curl {
        let config = curl_config("GET", "text/event-stream", None, Some(bearer))?;
        let idle_s = idle.as_secs().max(1).to_string();
        let max_s = max.map(|m| m.as_secs().max(1).to_string());
        let mut args: Vec<&str> = vec![
            "-sS",
            "-N",
            "-i",
            "--proto",
            curl_proto(u.https),
            "--connect-timeout",
            "15",
            "--speed-limit",
            "1",
            "--speed-time",
            &idle_s,
        ];
        if let Some(m) = &max_s {
            args.extend(["--max-time", m]);
        }
        args.extend(["-K", "-", url]);
        let mut child = Command::new("curl")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("https needs the system curl (Termux: pkg install curl): {e}"))?;
        {
            let mut stdin = child.stdin.take().ok_or("curl stdin")?;
            stdin
                .write_all(config.as_bytes())
                .map_err(|e| format!("curl stdin: {e}"))?;
        }
        let out = child.stdout.take().ok_or("curl stdout")?;
        Stream::Curl(child, std::io::BufReader::new(out))
    } else {
        let mut s = connect(&u, TIMEOUT)?;
        let head = request_head("GET", &u, Some(bearer), "text/event-stream") + "\r\n";
        s.write_all(head.as_bytes())
            .map_err(|e| format!("send: {e}"))?;
        s.set_read_timeout(Some(idle)).ok();
        Stream::Tcp(std::io::BufReader::new(s))
    };
    // Status line, then headers up to the blank line (curl -i prints them too;
    // a 1xx/redirect head is not expected from Hermes and is treated as an error).
    let first = st
        .line()
        .map_err(|e| format!("read: {e}"))?
        .ok_or("connection closed before a response")?;
    let code: u16 = first
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| {
            format!(
                "malformed status line: {}",
                first.chars().take(80).collect::<String>()
            )
        })?;
    let mut n = 0;
    loop {
        let l = st
            .line()
            .map_err(|e| format!("read: {e}"))?
            .ok_or("connection closed in the response head")?;
        if l.is_empty() {
            break;
        }
        n += 1;
        if n > 100 {
            return Err("response head too long".into());
        }
    }
    Ok((code, st))
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
pub fn request(dir: &Path, url: &str, doc: &Value, net: Net) -> Result<Outcome, String> {
    if !url.ends_with(ENROLL_PATH) {
        return Err(format!("enroll URL must end with {ENROLL_PATH}"));
    }
    let (code, body) = http("POST", url, Some(&doc.to_string()), None, net)?;
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
pub fn claim(dir: &Path, id: &Identity, net: Net) -> Result<Outcome, String> {
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
        net,
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
pub fn status(dir: &Path, scope: Option<&str>, net: Net) -> Result<Outcome, String> {
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
    let (code, text) = http("GET", &target, None, Some(token), net)?;
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
        let e = http("GET", "http://192.0.2.1:9/x", None, None, Net::default()).unwrap_err();
        assert!(e.contains("refusing plain http"), "{e}");
    }

    #[test]
    fn code_matches_hermes() {
        // Same vector as crates/hermes-rs enroll tests (pairing_code_shape_and_normalising).
        assert_eq!(
            pairing_code(&"11".repeat(32), &"22".repeat(12)),
            "J820-7XVQ"
        );
        assert_eq!(
            status_canonical("omn-0123456789ab", 5, "http://127.0.0.1:1/openmind/enroll"),
            "omnode-status-v1|omn-0123456789ab|5|http://127.0.0.1:1/openmind/enroll"
        );
        assert_eq!(
            tunnel_canonical("omn-0123456789ab", "s", "hello", "c", "u"),
            "omnode-tunnel-v1|omn-0123456789ab|s|hello|c|u"
        );
    }

    #[test]
    fn curl_config_quotes_and_keeps_secrets_off_argv() {
        let c = curl_config(
            "POST",
            "application/json",
            Some(r#"{"a":"b\\"c"}"#),
            Some("omn1_x"),
        )
        .unwrap();
        assert!(
            c.contains("header = \"Authorization: Bearer omn1_x\"\n"),
            "{c}"
        );
        assert!(c.contains(r#"data-binary = "{\"a\":\"b\\\\\"c\"}""#), "{c}");
        assert!(curl_config("GET", "x", Some("a\nb"), None).is_err());
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
