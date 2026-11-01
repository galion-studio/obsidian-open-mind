// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--instances DIR` (console-instances, 2026-11-01): the device list on
//! `/instances`, the device API, the public `/join` page and the optional
//! LAN discovery answer. The list itself is [`galion_instances::Fleet`].
//!
//! Routes and who may use them:
//! - `GET|POST /instances`: the owner (page gate + paired device; a POST
//!   also needs same-origin). Forms: `op=add` (make a code, shown once),
//!   `op=rename`, `op=forget`.
//! - `GET /api/v1/fleet`: the owner (same checks as the page), the list
//!   as JSON (no tokens, no IP addresses).
//! - `POST /api/v1/fleet/join` + `/api/v1/fleet/beat`: devices. No cookie;
//!   the one-time code or the device token in the JSON body is the proof.
//! - `GET|POST /join`, `POST /join/beat`: a phone or tablet joins in the
//!   browser; its token is kept in an HttpOnly, SameSite=Strict cookie
//!   scoped to `/join`. Only on a console host or from a private address.

use crate::config::InstancesCfg;
use console_ui::pages::fleet::{Fresh, JoinState, Notice};
use galion_instances::metrics::{self, Metrics};
use galion_instances::{fleet, Fleet, API_BEAT, API_JOIN};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use galion_instances::{API_BEAT as BEAT, API_JOIN as JOIN, API_LIST as LIST};

/// The owner page.
pub const PAGE_PATH: &str = "/instances";
/// The public join page.
pub const JOIN_PATH: &str = "/join";
/// The join page's report route.
pub const JOIN_BEAT: &str = "/join/beat";
/// The browser device cookie.
pub const COOKIE: &str = "galion_fleet";

/// One answer from this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub status: u16,
    pub body: String,
}

fn out(status: u16, v: Value) -> Out {
    Out {
        status,
        body: v.to_string(),
    }
}

fn refused(r: &galion_instances::Refused) -> Out {
    out(r.status(), json!({"ok": false, "error": r.words()}))
}

/// Device-facing JSON routes.
pub fn owns_device_api(path: &str) -> bool {
    path == API_JOIN || path == API_BEAT
}

/// Routes that read a request body.
pub fn wants_body(method: &str, path: &str) -> bool {
    method == "POST"
        && (owns_device_api(path) || path == PAGE_PATH || path == JOIN_PATH || path == JOIN_BEAT)
}

/// The fleet plus this console's own reading.
#[derive(Debug)]
pub struct Instances {
    pub fleet: Fleet,
    pub cfg: InstancesCfg,
    me: Mutex<Option<(Instant, Metrics)>>,
}

impl Instances {
    /// Open `DIR/fleet.json`. The LAN responder is started separately
    /// ([`Instances::start_lan`]) once the HTTP port is known.
    pub fn open(cfg: InstancesCfg) -> std::io::Result<Instances> {
        Ok(Instances {
            fleet: Fleet::open(&cfg.dir)?,
            cfg,
            me: Mutex::new(None),
        })
    }

    /// In memory (tests).
    pub fn memory(cfg: InstancesCfg) -> Instances {
        Instances {
            fleet: Fleet::memory(),
            cfg,
            me: Mutex::new(None),
        }
    }

    /// Answer LAN discovery on `--instances-lan` (if set).
    pub fn start_lan(&self, http_port: u16) -> std::io::Result<Option<std::net::SocketAddr>> {
        match self.cfg.lan_port {
            Some(p) => galion_instances::lan::start(
                p,
                http_port,
                self.fleet.fleet_id(),
                self.cfg.name.clone(),
            )
            .map(Some),
            None => Ok(None),
        }
    }

    /// This machine, read at most every 10 s.
    pub fn console_metrics(&self) -> Metrics {
        let mut g = self.me.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, m)) = g.as_ref() {
            if at.elapsed() < Duration::from_secs(10) {
                return m.clone();
            }
        }
        let m = metrics::probe(Duration::from_millis(200));
        *g = Some((Instant::now(), m.clone()));
        m
    }

    /// The page's view: the device list plus this console.
    pub fn view(&self) -> Value {
        let mut v = self.fleet.snapshot(Fleet::now());
        v["console"] = json!({"name": self.cfg.name, "metrics": self.console_metrics().to_json()});
        v["lan"] = json!(self.cfg.lan_port.is_some());
        v
    }

    /// `GET /api/v1/fleet` for the owner.
    pub fn owner_list(&self, method: &str) -> Out {
        match method {
            "GET" | "HEAD" => out(200, self.view()),
            _ => out(405, json!({"ok": false, "error": "use GET"})),
        }
    }

    /// `POST /api/v1/fleet/join` and `/beat` for devices.
    pub fn device_api(&self, method: &str, path: &str, body: &[u8], ip: &str) -> Out {
        if method != "POST" {
            return out(405, json!({"ok": false, "error": "use POST"}));
        }
        let Ok(v) = serde_json::from_slice::<Value>(body) else {
            return out(400, json!({"ok": false, "error": "send JSON"}));
        };
        let m = Metrics::from_json(v.get("metrics").unwrap_or(&Value::Null));
        let now = Fleet::now();
        if path == API_JOIN {
            let code = v.get("code").and_then(Value::as_str).unwrap_or("");
            let name = v.get("name").and_then(Value::as_str).unwrap_or("");
            match self.fleet.join(code, name, "agent", m, ip, now) {
                Ok(j) => out(
                    200,
                    json!({"ok": true, "id": j.id, "name": j.name, "token": j.token,
                        "fleet_id": j.fleet_id, "every": fleet::BEAT_EVERY}),
                ),
                Err(r) => refused(&r),
            }
        } else {
            let token = v.get("token").and_then(Value::as_str).unwrap_or("");
            match self.fleet.beat(token, m, ip, now) {
                Ok((id, name)) => out(
                    200,
                    json!({"ok": true, "id": id, "name": name, "every": fleet::BEAT_EVERY}),
                ),
                Err(r) => refused(&r),
            }
        }
    }

    /// An owner form post on `/instances` (after the page checks passed).
    /// Returns the code to show once, if one was made, and a notice.
    pub fn owner_form(&self, body: &[u8]) -> (Option<String>, Notice) {
        let form = console_ui::html::form_pairs(&String::from_utf8_lossy(body));
        let get = |k: &str| console_ui::html::form_get(&form, k).unwrap_or("");
        let say = |ok: bool, t: &str| Notice {
            ok,
            text: t.to_string(),
        };
        match get("op") {
            "add" => match self.fleet.mint_code(get("name"), Fleet::now()) {
                Ok(c) => (
                    Some(c),
                    say(
                        true,
                        "Here is your code. Use it on the device you want to add.",
                    ),
                ),
                Err(r) => (None, say(false, r.words())),
            },
            "rename" => match self.fleet.rename(get("id"), get("name")) {
                Some(n) => (None, say(true, &format!("Renamed to \"{n}\"."))),
                None => (
                    None,
                    say(false, "That device was not found, or the name was empty."),
                ),
            },
            "forget" => match self.fleet.forget(get("id")) {
                Some(n) => (
                    None,
                    say(
                        true,
                        &format!("\"{n}\" was removed. It can no longer report."),
                    ),
                ),
                None => (
                    None,
                    say(false, "That device was not found or is already removed."),
                ),
            },
            _ => (None, say(false, "Unknown action.")),
        }
    }

    /// The `/instances` body for the owner.
    pub fn page_body(
        &self,
        code: Option<String>,
        notice: Option<&Notice>,
        console_url: &str,
    ) -> String {
        let view = console_ui::json::parse(self.view().to_string().as_bytes())
            .unwrap_or(console_ui::json::Value::Null);
        let fresh = code.map(|code| Fresh {
            code,
            console_url: console_url.to_string(),
            minutes: fleet::CODE_TTL / 60,
            lan: self.cfg.lan_port.is_some(),
        });
        console_ui::pages::fleet::body(&view, fresh.as_ref(), notice)
    }

    /// `/join` (GET shows the form or the joined state; POST joins).
    /// Returns (status, body, Set-Cookie value).
    pub fn join_page(
        &self,
        method: &str,
        body: &[u8],
        cookie: Option<&str>,
        ip: &str,
        secure: bool,
    ) -> (u16, String, Option<String>) {
        let known = console_ui::gate::cookie(cookie, COOKIE).and_then(|t| self.name_of(&t));
        if method != "POST" {
            let st = match known {
                Some(name) => JoinState::Joined { name, fresh: false },
                None => JoinState::Ask { error: None },
            };
            return (200, console_ui::pages::fleet::join_body(&st), None);
        }
        let form = console_ui::html::form_pairs(&String::from_utf8_lossy(body));
        let get = |k: &str| console_ui::html::form_get(&form, k).unwrap_or("");
        let m = serde_json::from_str::<Value>(get("metrics"))
            .map(|v| Metrics::from_json(&v))
            .unwrap_or_default();
        match self
            .fleet
            .join(get("code"), get("name"), "browser", m, ip, Fleet::now())
        {
            Ok(j) => {
                let c = format!(
                    "{COOKIE}={}; Path={JOIN_PATH}; Max-Age=31536000; HttpOnly; SameSite=Strict{}",
                    j.token,
                    if secure { "; Secure" } else { "" }
                );
                (
                    200,
                    console_ui::pages::fleet::join_body(&JoinState::Joined {
                        name: j.name,
                        fresh: true,
                    }),
                    Some(c),
                )
            }
            Err(r) => (
                r.status(),
                console_ui::pages::fleet::join_body(&JoinState::Ask {
                    error: Some(r.words().into()),
                }),
                None,
            ),
        }
    }

    fn name_of(&self, token: &str) -> Option<String> {
        self.fleet.name_for_token(token)
    }

    /// `POST /join/beat`: the join page reports (cookie token).
    pub fn join_beat(&self, method: &str, body: &[u8], cookie: Option<&str>, ip: &str) -> Out {
        if method != "POST" {
            return out(405, json!({"ok": false, "error": "use POST"}));
        }
        let Some(token) = console_ui::gate::cookie(cookie, COOKIE) else {
            return refused(&galion_instances::Refused::Unknown);
        };
        let m = serde_json::from_slice::<Value>(body)
            .map(|v| Metrics::from_json(&v))
            .unwrap_or_default();
        match self.fleet.beat(&token, m, ip, Fleet::now()) {
            Ok((id, name)) => out(
                200,
                json!({"ok": true, "id": id, "name": name, "every": fleet::BEAT_EVERY}),
            ),
            Err(r) => refused(&r),
        }
    }
}

/// The address a device should use, from the request: the forwarded
/// scheme (trusted only from a loopback proxy) or http, and the Host.
pub fn console_url(host: &str, forwarded_proto: Option<&str>) -> String {
    let scheme = match forwarded_proto {
        Some(p) if p.eq_ignore_ascii_case("https") => "https",
        _ => "http",
    };
    let host: String = host
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
        .take(200)
        .collect();
    format!("{scheme}://{host}")
}

/// The device's address: the peer, or (behind a loopback proxy) the
/// first X-Forwarded-For / CF-Connecting-IP entry.
pub fn client_ip(
    peer: Option<std::net::SocketAddr>,
    xff: Option<&str>,
    cf: Option<&str>,
) -> String {
    let peer_ip = peer.map(|p| p.ip());
    if peer_ip.is_some_and(|ip| ip.is_loopback()) {
        let fwd = cf
            .or_else(|| xff.and_then(|x| x.split(',').next()))
            .map(str::trim)
            .and_then(|s| s.parse::<std::net::IpAddr>().ok());
        if let Some(ip) = fwd {
            return ip.to_string();
        }
    }
    peer_ip.map(|i| i.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inst(lan: bool) -> Instances {
        Instances::memory(InstancesCfg {
            dir: PathBuf::from("/nonexistent"),
            lan_port: lan.then_some(47474),
            name: "Test console".into(),
        })
    }

    #[test]
    fn owner_flow_add_join_beat_forget() {
        let i = inst(true);
        let (code, n) = i.owner_form(b"op=add&name=Studio+laptop");
        let code = code.unwrap();
        assert!(n.ok);
        let h = i.page_body(Some(code.clone()), Some(&n), "https://console.example");
        assert!(h.contains(&format!(
            "galion-join {code} --console https://console.example"
        )));
        let body = json!({"code": code, "name": "x", "metrics": {"cores": 8, "ram_total_mb": 8192, "gpus": [{"name": "RTX 3060", "vram_total_mb": 12288}]}});
        let r = i.device_api("POST", API_JOIN, body.to_string().as_bytes(), "192.168.1.4");
        assert_eq!(r.status, 200, "{}", r.body);
        let v: Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(v["name"], "Studio laptop");
        let token = v["token"].as_str().unwrap().to_string();
        let again = i.device_api("POST", API_JOIN, body.to_string().as_bytes(), "1.1.1.1");
        assert_eq!(again.status, 401);
        let beat = json!({"token": token, "metrics": {"cpu_pct": 55}});
        assert_eq!(
            i.device_api("POST", API_BEAT, beat.to_string().as_bytes(), "192.168.1.4")
                .status,
            200
        );
        let list = i.owner_list("GET");
        assert!(list.body.contains("Studio laptop"));
        assert!(!list.body.contains(&token));
        assert!(!list.body.contains("192.168.1.4"));
        assert!(list.body.contains("\"console\""));
        let page = i.page_body(None, None, "x");
        assert!(page.contains("55% busy"));
        assert!(!page.contains(&code), "a code is shown once");
        let id = v["id"].as_str().unwrap();
        let (_, n) = i.owner_form(format!("op=forget&id={id}").as_bytes());
        assert!(n.ok, "{}", n.text);
        assert_eq!(
            i.device_api("POST", API_BEAT, beat.to_string().as_bytes(), "x")
                .status,
            401
        );
        assert_eq!(i.device_api("GET", API_BEAT, b"", "x").status, 405);
        assert_eq!(i.device_api("POST", API_BEAT, b"nope", "x").status, 400);
        assert!(!i.owner_form(b"op=zap").1.ok);
    }

    #[test]
    fn browser_join_sets_a_scoped_cookie() {
        let i = inst(false);
        let (code, _) = i.owner_form(b"op=add");
        let code = code.unwrap();
        let (st, body, c) = i.join_page("GET", b"", None, "10.0.0.5", true);
        assert_eq!(st, 200);
        assert!(body.contains("Add this device"));
        assert!(c.is_none());
        let (st, body, _) = i.join_page("POST", b"code=ZZZZ-ZZZZ", None, "10.0.0.5", true);
        assert_eq!(st, 401);
        assert!(body.contains("That code is wrong"));
        let form = format!(
            "code={}&name=My+phone&metrics=%7B%22cores%22%3A6%7D",
            code.to_lowercase()
        );
        let (st, body, c) = i.join_page("POST", form.as_bytes(), None, "10.0.0.5", true);
        assert_eq!(st, 200, "{body}");
        let c = c.unwrap();
        assert!(
            c.contains("Path=/join")
                && c.contains("HttpOnly")
                && c.contains("SameSite=Strict")
                && c.contains("Secure")
        );
        let cookie = c.split(';').next().unwrap().to_string();
        let (_, body, _) = i.join_page("GET", b"", Some(&cookie), "10.0.0.5", true);
        assert!(body.contains("My phone"));
        assert_eq!(
            i.join_beat("POST", br#"{"cores":6}"#, Some(&cookie), "10.0.0.5")
                .status,
            200
        );
        assert_eq!(i.join_beat("POST", b"{}", None, "10.0.0.5").status, 401);
        assert!(i.owner_list("GET").body.contains("\"via\":\"browser\""));
    }

    #[test]
    fn urls_and_ips() {
        assert_eq!(
            console_url("console.example", Some("https")),
            "https://console.example"
        );
        assert_eq!(
            console_url("192.168.1.2:8090", None),
            "http://192.168.1.2:8090"
        );
        assert_eq!(console_url("a\"<b>", None), "http://ab");
        let lb = Some("127.0.0.1:5000".parse().unwrap());
        let ext = Some("203.0.113.9:5000".parse().unwrap());
        assert_eq!(
            client_ip(lb, Some("198.51.100.1, 10.0.0.1"), None),
            "198.51.100.1"
        );
        assert_eq!(
            client_ip(lb, Some("x"), Some("198.51.100.2")),
            "198.51.100.2"
        );
        assert_eq!(client_ip(lb, None, None), "127.0.0.1");
        assert_eq!(
            client_ip(ext, Some("10.0.0.1"), None),
            "203.0.113.9",
            "only a loopback proxy is trusted"
        );
    }
}
