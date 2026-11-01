// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-join`: add this computer to a Galion console and keep it shown.
//!
//! ```text
//! galion-join ABCD-EFGH --console https://console.example   join over the internet
//! galion-join ABCD-EFGH                                     join a console on this Wi-Fi
//! galion-join                                               keep reporting (already joined)
//! ```
//!
//! After joining, the token is saved (0600) in the state file and the
//! program reports CPU, RAM and GPU every 30 s. If the console address
//! stops answering it looks for the same console (same fleet id) on the
//! local network, so a laptop and a console in one house keep working with
//! the internet down. The code and token are never printed.

use crate::lan;
use crate::metrics::{self, Metrics};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const USAGE: &str = "\
galion-join: add this computer to your Galion console.

  galion-join CODE --console URL   join (CODE is shown once on the Instances page)
  galion-join CODE                 join a console on this same network (found by itself)
  galion-join                      keep reporting (after joining once)

Options:
  --console URL    the console address, e.g. https://console.example or 192.168.1.10:8080
  --name NAME      what to call this computer (default: its hostname)
  --once           send one report and stop
  --every SECS     seconds between reports (default: what the console asks, 30)
  --lan-port PORT  UDP port for finding the console on the network (default 47474)
  --state FILE     where the join is saved (default: ~/.config/galion/join.json)
  --help           this text
";

/// Parsed command line.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Args {
    pub code: Option<String>,
    pub console: Option<String>,
    pub name: String,
    pub once: bool,
    pub every: Option<u64>,
    pub lan_port: u16,
    pub state: Option<PathBuf>,
    pub help: bool,
}

pub fn parse_args<I: IntoIterator<Item = String>>(it: I) -> Result<Args, String> {
    let mut a = Args {
        lan_port: lan::PORT,
        ..Args::default()
    };
    let mut it = it.into_iter();
    while let Some(x) = it.next() {
        let mut val = |flag: &str| it.next().ok_or_else(|| format!("{flag} needs a value"));
        match x.as_str() {
            "--console" => a.console = Some(val("--console")?),
            "--name" => a.name = val("--name")?,
            "--once" => a.once = true,
            "--every" => {
                a.every = Some(
                    val("--every")?
                        .parse()
                        .ok()
                        .filter(|s| (5..=3600).contains(s))
                        .ok_or("--every must be 5 to 3600")?,
                )
            }
            "--lan-port" => {
                a.lan_port = val("--lan-port")?
                    .parse()
                    .ok()
                    .filter(|p| *p > 0)
                    .ok_or("--lan-port must be 1 to 65535")?
            }
            "--state" => a.state = Some(PathBuf::from(val("--state")?)),
            "-h" | "--help" => a.help = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if a.code.is_none() => {
                if crate::code::normalize(s).is_none() {
                    return Err(
                        "that does not look like a code (8 letters and digits, like ABCD-EFGH)"
                            .into(),
                    );
                }
                a.code = Some(s.to_string())
            }
            _ => return Err("only one code, please".into()),
        }
    }
    Ok(a)
}

/// `https://x`, `http://1.2.3.4:8080`, `x:8080` (-> http), `x` (-> https).
pub fn normalize_console(s: &str) -> Option<String> {
    let s = s.trim().trim_end_matches('/');
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(s.to_string());
    }
    if s.contains("://") {
        return None;
    }
    let host_port = s.split('/').next().unwrap_or(s);
    if host_port
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        Some(format!("http://{s}"))
    } else {
        Some(format!("https://{s}"))
    }
}

/// What is saved after joining.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub console: String,
    pub fleet_id: String,
    pub id: String,
    pub name: String,
    pub token: String,
    pub lan_port: u16,
}

pub fn default_state() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("galion"))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/galion")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("join.json")
}

pub fn save(path: &Path, s: &Saved) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = json!({"console": s.console, "fleet_id": s.fleet_id, "id": s.id,
        "name": s.name, "token": s.token, "lan_port": s.lan_port});
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(&tmp)?.write_all(body.to_string().as_bytes())?;
    std::fs::rename(tmp, path)
}

pub fn load(path: &Path) -> Option<Saved> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let saved = Saved {
        console: s("console"),
        fleet_id: s("fleet_id"),
        id: s("id"),
        name: s("name"),
        token: s("token"),
        lan_port: v
            .get("lan_port")
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(lan::PORT),
    };
    (!saved.token.is_empty() && !saved.console.is_empty()).then_some(saved)
}

/// An HTTP answer.
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub json: Value,
}

/// POST JSON. `http://` is spoken directly; `https://` goes through the
/// system `curl` with the body on stdin (so no secret is on a command line).
pub fn post(base: &str, path: &str, body: &Value) -> Result<Answer, String> {
    let data = body.to_string();
    if let Some(rest) = base.strip_prefix("http://") {
        post_plain(rest, path, &data)
    } else if base.starts_with("https://") {
        post_curl(&format!("{base}{path}"), &data)
    } else {
        Err("the console address must start with http:// or https://".into())
    }
}

fn post_plain(host_port: &str, path: &str, data: &str) -> Result<Answer, String> {
    let hp = host_port.split('/').next().unwrap_or(host_port);
    let with_port = if hp
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        hp.to_string()
    } else {
        format!("{hp}:80")
    };
    let addrs: Vec<SocketAddr> = with_port
        .to_socket_addrs()
        .map_err(|_| format!("cannot find {hp}"))?
        .collect();
    // "localhost" may resolve to ::1 first while the console listens on
    // 127.0.0.1: try each address.
    let mut s = addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(5)).ok())
        .ok_or_else(|| format!("{hp} does not answer"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {hp}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nUser-Agent: galion-join\r\n\r\n{data}",
        data.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let _ = s.take(1 << 20).read_to_end(&mut raw);
    parse_response(&raw).ok_or_else(|| format!("{hp} gave a broken answer"))
}

/// Status line + body of a `Connection: close` HTTP/1.1 answer.
pub fn parse_response(raw: &[u8]) -> Option<Answer> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    let body = &raw[end + 4..];
    Some(Answer {
        status,
        json: serde_json::from_slice(body).unwrap_or(Value::Null),
    })
}

fn post_curl(url: &str, data: &str) -> Result<Answer, String> {
    use std::process::{Command, Stdio};
    let mut c = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "20",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "-w",
            "\n%{http_code}",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "https needs curl on this computer".to_string())?;
    if let Some(mut i) = c.stdin.take() {
        let _ = i.write_all(data.as_bytes());
    }
    let out = c.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text
        .rsplit_once('\n')
        .ok_or("the console does not answer")?;
    let status: u16 = code
        .trim()
        .parse()
        .map_err(|_| "the console does not answer")?;
    if status == 0 {
        return Err("the console does not answer".into());
    }
    Ok(Answer {
        status,
        json: serde_json::from_str(body).unwrap_or(Value::Null),
    })
}

fn said(a: &Answer) -> String {
    a.json
        .get("error")
        .and_then(Value::as_str)
        .map(|s| metrics::clean(s, 200))
        .unwrap_or_else(|| format!("the console said HTTP {}", a.status))
}

/// Join with `code`. `console` None = look on the local network.
pub fn join(args: &Args, out: &mut dyn Write) -> Result<Saved, String> {
    let code = args.code.as_deref().ok_or("no code given")?;
    let console = match &args.console {
        Some(c) => normalize_console(c).ok_or("that console address does not look right")?,
        None => {
            let _ = writeln!(out, "Looking for a Galion console on this network...");
            let found =
                lan::discover(args.lan_port, Duration::from_secs(2)).map_err(|e| e.to_string())?;
            match found.as_slice() {
                [] => return Err("no console answered on this network. Add --console with the address shown on the Instances page.".into()),
                [one] => {
                    let _ = writeln!(out, "Found \"{}\" at {}.", one.name, one.addr);
                    format!("http://{}", one.addr)
                }
                many => {
                    return Err(format!(
                        "{} consoles answered; pick one with --console ({})",
                        many.len(),
                        many.iter().map(|f| f.addr.to_string()).collect::<Vec<_>>().join(", ")
                    ))
                }
            }
        }
    };
    let m = metrics::probe(Duration::from_millis(500));
    let a = post(
        &console,
        crate::API_JOIN,
        &json!({"code": code, "name": args.name, "metrics": m.to_json()}),
    )?;
    if a.status != 200 {
        return Err(said(&a));
    }
    let s = |k: &str| {
        a.json
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let saved = Saved {
        console,
        fleet_id: s("fleet_id"),
        id: s("id"),
        name: s("name"),
        token: s("token"),
        lan_port: args.lan_port,
    };
    if saved.token.len() != 64 {
        return Err("the console's answer had no device key".into());
    }
    Ok(saved)
}

/// One report. On a network failure, try the same console on the LAN.
pub fn report(s: &Saved, m: &Metrics) -> Result<u64, String> {
    let body = json!({"token": s.token, "metrics": m.to_json()});
    let a = match post(&s.console, crate::API_BEAT, &body) {
        Ok(a) => a,
        Err(e) => {
            let near = lan::discover(s.lan_port, Duration::from_secs(1)).unwrap_or_default();
            let Some(f) = near
                .iter()
                .find(|f| !s.fleet_id.is_empty() && f.fleet_id == s.fleet_id)
            else {
                return Err(e);
            };
            post(&format!("http://{}", f.addr), crate::API_BEAT, &body)?
        }
    };
    match a.status {
        200 => Ok(a
            .json
            .get("every")
            .and_then(Value::as_u64)
            .unwrap_or(crate::fleet::BEAT_EVERY)),
        401 => Err(format!("REMOVED: {}", said(&a))),
        _ => Err(said(&a)),
    }
}

/// The program. Returns the exit code.
pub fn main_with(argv: Vec<String>, out: &mut dyn Write) -> i32 {
    let args = match parse_args(argv) {
        Ok(a) if a.help => {
            let _ = write!(out, "{USAGE}");
            return 0;
        }
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(out, "{e} (run with --help)");
            return 2;
        }
    };
    let state = args.state.clone().unwrap_or_else(default_state);
    let saved = if args.code.is_some() {
        match join(&args, out) {
            Ok(s) => {
                if let Err(e) = save(&state, &s) {
                    let _ = writeln!(
                        out,
                        "Joined, but could not save to {}: {e}",
                        state.display()
                    );
                    return 1;
                }
                let _ = writeln!(
                    out,
                    "Joined as \"{}\". This computer now shows on the Instances page.",
                    s.name
                );
                s
            }
            Err(e) => {
                let _ = writeln!(out, "Could not join: {e}");
                return 1;
            }
        }
    } else {
        match load(&state) {
            Some(s) => s,
            None => {
                let _ = writeln!(out, "Not joined yet. Get a code on the console's Instances page, then run: galion-join CODE");
                return 2;
            }
        }
    };
    loop {
        let m = metrics::probe(Duration::from_millis(500));
        let wait = match report(&saved, &m) {
            Ok(every) => {
                if args.once {
                    let _ = writeln!(out, "Reported to the console.");
                    return 0;
                }
                args.every.unwrap_or(every).clamp(5, 3600)
            }
            Err(e) if e.starts_with("REMOVED: ") => {
                let _ = writeln!(out, "{}", &e[9..]);
                return 3;
            }
            Err(e) => {
                let _ = writeln!(out, "Could not report ({e}); trying again soon.");
                if args.once {
                    return 1;
                }
                args.every.unwrap_or(crate::fleet::BEAT_EVERY)
            }
        };
        let _ = out.flush();
        std::thread::sleep(Duration::from_secs(wait));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn args() {
        let a = parse_args(v(&["abcd-efgh", "--console", "c.example", "--once"])).unwrap();
        assert_eq!(a.code.as_deref(), Some("abcd-efgh"));
        assert!(a.once);
        assert_eq!(a.lan_port, lan::PORT);
        assert!(parse_args(v(&["nope"])).is_err());
        assert!(parse_args(v(&["--every", "1"])).is_err());
        assert!(parse_args(v(&["--bogus"])).is_err());
        assert!(parse_args(v(&["ABCD-EFGH", "ABCD-EFGH"])).is_err());
        assert!(parse_args(v(&["--help"])).unwrap().help);
    }

    #[test]
    fn console_addresses() {
        assert_eq!(
            normalize_console("console.example/").as_deref(),
            Some("https://console.example")
        );
        assert_eq!(
            normalize_console("192.168.1.10:8080").as_deref(),
            Some("http://192.168.1.10:8080")
        );
        assert_eq!(
            normalize_console("http://x:1").as_deref(),
            Some("http://x:1")
        );
        assert_eq!(normalize_console("ftp://x"), None);
        assert_eq!(normalize_console("a b"), None);
    }

    #[test]
    fn response_and_state_file() {
        let a =
            parse_response(b"HTTP/1.1 401 Unauthorized\r\nX: y\r\n\r\n{\"error\":\"no\"}").unwrap();
        assert_eq!(a.status, 401);
        assert_eq!(said(&a), "no");
        assert!(parse_response(b"garbage").is_none());
        let p = std::env::temp_dir().join(format!("galion-join-{}/join.json", std::process::id()));
        let s = Saved {
            console: "http://127.0.0.1:1".into(),
            fleet_id: "f".into(),
            id: "i".into(),
            name: "n".into(),
            token: "t".repeat(64),
            lan_port: 5,
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), Some(s));
    }

    #[test]
    fn friendly_errors() {
        let mut o = Vec::new();
        assert_eq!(main_with(v(&["--bogus"]), &mut o), 2);
        let mut o = Vec::new();
        let st = std::env::temp_dir().join(format!("galion-join-none-{}.json", std::process::id()));
        assert_eq!(main_with(v(&["--state", st.to_str().unwrap()]), &mut o), 2);
        assert!(String::from_utf8_lossy(&o).contains("Not joined yet"));
    }
}
