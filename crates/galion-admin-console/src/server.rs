// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! HTTP routes (served with `galion_common::http`, loopback only).
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /healthz` | anyone | liveness |
//! | `GET /api/v1/admin` | anyone | `{"paired":bool,"node":..}` for this browser |
//! | `POST /api/v1/admin/pair` | anyone, Origin-checked | spend a one-time code; sets the device cookie |
//! | `POST /api/v1/admin/unpair` | paired, Origin-checked | revoke this device, clear cookie |
//! | `GET /api/v1/admin/status` | paired | nerve + console summary (401 `phone required` otherwise) |
//! | `GET /admin` | anyone | phone page: pair form, or status when paired (no JavaScript) |

use crate::audit;
use crate::pairing::{PairError, Store};
use crate::status;
use galion_common::http::{Request, Response};
use serde_json::{json, Value};
use std::net::SocketAddr;

pub const COOKIE: &str = "galion_admin";

pub struct Config {
    pub store: Store,
    pub nerve: Option<SocketAddr>,
    pub console: Option<SocketAddr>,
    pub console_host: String,
    /// Exact `Origin` accepted on POST (e.g. `https://console.galion.app`).
    pub origin: Option<String>,
    /// `Secure` cookie flag; only loopback tests over plain HTTP turn it off.
    pub secure_cookie: bool,
}

fn harden(r: Response) -> Response {
    r.with_header("cache-control", "no-store")
        .with_header("x-content-type-options", "nosniff")
        .with_header("referrer-policy", "no-referrer")
        .with_header("x-frame-options", "DENY")
        .with_header(
            "content-security-policy",
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        )
}

fn see_other(to: &str) -> Response {
    Response::text(303, "").with_header("location", to)
}

pub fn cookie_token(req: &Request) -> Option<String> {
    req.header("cookie")?.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == COOKIE).then(|| v.to_string())
    })
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit() =>
            {
                let h = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
                out.push(h(b[i + 1]) * 16 + h(b[i + 2]));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn form_field(body: &[u8], name: &str) -> Option<String> {
    std::str::from_utf8(body).ok()?.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (url_decode(k) == name).then(|| url_decode(v))
    })
}

impl Config {
    fn post_allowed(&self, req: &Request) -> bool {
        match req.header("origin") {
            Some(o) => self.origin.as_deref() == Some(o),
            None => req
                .header("sec-fetch-site")
                .is_none_or(|s| s == "same-origin" || s == "none"),
        }
    }

    fn set_cookie(&self, token: &str, max_age: i64) -> String {
        format!(
            "{COOKIE}={token}; Path=/; HttpOnly;{} SameSite=Strict; Max-Age={max_age}",
            if self.secure_cookie { " Secure;" } else { "" }
        )
    }

    fn device(&self, req: &Request) -> Option<crate::pairing::Device> {
        let t = cookie_token(req)?;
        self.store.device(&t).ok().flatten()
    }

    fn audit(&self, action: &str, target: &str, result: &str) {
        // Audit failure must not be silent: it goes to stderr (journald).
        if let Err(e) = audit::append(&self.store.audit_path(), action, target, result) {
            eprintln!("galion-admin-console: audit write failed: {e}");
        }
    }

    pub fn status_json(&self, node: &str) -> Value {
        json!({
            "node": node,
            "nerve": status::nerve(self.nerve),
            "console": status::console(self.console, &self.console_host),
        })
    }

    /// console-ui gate contract (`crates/console-ui` `pair`): `POST /api/v1/admin`
    /// with JSON `{"code": ".."}` -> 200 + `Set-Cookie: galion_admin=..`, 429 when
    /// locked, 401 otherwise.
    fn pair_json(&self, req: &Request) -> Response {
        if !self.post_allowed(req) {
            self.audit("pair.refused", "-", "origin");
            return Response::json(403, r#"{"error":"origin"}"#);
        }
        let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let code = v.get("code").and_then(Value::as_str).unwrap_or("");
        let label = v.get("label").and_then(Value::as_str).unwrap_or("");
        match self.store.pair(code, label, crate::now_secs()) {
            Ok(Ok((token, node))) => {
                self.audit("pair.ok", &node, "paired");
                Response::json(
                    200,
                    json!({"ok": true, "you": true, "node": node}).to_string(),
                )
                .with_header("set-cookie", &self.set_cookie(&token, 31_536_000))
            }
            Ok(Err(PairError::Bad)) => {
                self.audit("pair.fail", "-", "bad-code");
                Response::json(401, r#"{"error":"code refused"}"#)
            }
            Ok(Err(PairError::Locked)) => {
                self.audit("pair.fail", "-", "locked");
                Response::json(429, r#"{"error":"too many attempts"}"#)
                    .with_header("retry-after", "900")
            }
            Err(_) => Response::json(500, r#"{"error":"state"}"#),
        }
    }

    pub fn handle(&self, req: &Request) -> Response {
        harden(self.route(req))
    }

    fn route(&self, req: &Request) -> Response {
        let m = req.method();
        match (m, req.path()) {
            ("GET" | "HEAD", "/healthz") => Response::json(
                200,
                r#"{"status":"ok","service":"galion-admin-console","implementation":"rust"}"#,
            ),
            ("GET" | "HEAD", "/api/v1/admin") => {
                let body = match self.device(req) {
                    Some(d) => {
                        json!({"you": true, "paired": true, "node": d.node, "label": d.label})
                    }
                    None => json!({"you": false, "paired": false}),
                };
                Response::json(200, body.to_string())
            }
            ("GET" | "HEAD", "/api/v1/admin/status") => match self.device(req) {
                Some(d) => Response::json(200, self.status_json(&d.node).to_string()),
                None => Response::json(401, r#"{"error":"phone required"}"#),
            },
            ("POST", "/api/v1/admin") => self.pair_json(req),
            ("POST", "/api/v1/admin/pair") => {
                if !self.post_allowed(req) {
                    self.audit("pair.refused", "-", "origin");
                    return Response::json(403, r#"{"error":"origin"}"#);
                }
                let code = form_field(&req.body, "code").unwrap_or_default();
                let label = form_field(&req.body, "label").unwrap_or_default();
                match self.store.pair(&code, &label, crate::now_secs()) {
                    Ok(Ok((token, node))) => {
                        self.audit("pair.ok", &node, "paired");
                        see_other("/admin")
                            .with_header("set-cookie", &self.set_cookie(&token, 31_536_000))
                    }
                    Ok(Err(PairError::Bad)) => {
                        self.audit("pair.fail", "-", "bad-code");
                        see_other("/admin?e=bad")
                    }
                    Ok(Err(PairError::Locked)) => {
                        self.audit("pair.fail", "-", "locked");
                        see_other("/admin?e=locked")
                    }
                    Err(_) => Response::json(500, r#"{"error":"state"}"#),
                }
            }
            ("POST", "/api/v1/admin/unpair") => {
                if !self.post_allowed(req) {
                    return Response::json(403, r#"{"error":"origin"}"#);
                }
                let Some(d) = self.device(req) else {
                    return Response::json(401, r#"{"error":"phone required"}"#);
                };
                let _ = self.store.revoke(&d.node);
                self.audit("unpair", &d.node, "revoked");
                see_other("/admin").with_header("set-cookie", &self.set_cookie("", 0))
            }
            ("GET" | "HEAD", "/admin") => {
                let html = match self.device(req) {
                    Some(d) => page_paired(&d, &self.status_json(&d.node)),
                    None => page_pair(req.query()),
                };
                Response::new(200, "text/html; charset=utf-8", html)
            }
            (_, "/api/v1/admin") => {
                Response::text(405, "method not allowed\n").with_header("allow", "GET, HEAD, POST")
            }
            (_, "/healthz" | "/api/v1/admin/status" | "/admin") => {
                Response::text(405, "method not allowed\n").with_header("allow", "GET, HEAD")
            }
            (_, "/api/v1/admin/pair" | "/api/v1/admin/unpair") => {
                Response::text(405, "method not allowed\n").with_header("allow", "POST")
            }
            _ => Response::text(404, "not found\n"),
        }
    }
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

const STYLE: &str = "body{margin:0;font:16px/1.4 system-ui,sans-serif;background:#0b0d10;color:#e8e6e3}\
main{max-width:30rem;margin:0 auto;padding:1rem}h1{font-size:1.2rem}\
.card{background:#151a20;border-radius:12px;padding:1rem;margin:.75rem 0}\
input,button{font:inherit;width:100%;box-sizing:border-box;padding:.8rem;margin:.3rem 0;border-radius:10px;border:1px solid #333;background:#0b0d10;color:inherit}\
button{background:#c9a227;color:#111;border:0;font-weight:600}.warn{color:#f0a35e}.bad{color:#ef6b6b}.ok{color:#7ad17a}\
.big{font-size:2.4rem;font-weight:700}small{color:#9aa}";

fn shell(body: &str) -> String {
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8>\
<meta name=viewport content=\"width=device-width,initial-scale=1\">\
<meta name=robots content=noindex><title>Galion admin</title><style>{STYLE}</style></head>\
<body><main><h1>Galion admin</h1>{body}</main></body></html>"
    )
}

fn page_pair(query: &str) -> String {
    let note = if query.contains("e=locked") {
        "<p class=bad>Too many wrong codes. Wait fifteen minutes, then try a fresh code.</p>"
    } else if query.contains("e=bad") {
        "<p class=bad>That code didn't work. Codes are single use and last five minutes.</p>"
    } else {
        ""
    };
    shell(&format!(
        "<div class=card><p>This phone isn't paired yet.</p>{note}\
<form method=post action=/api/v1/admin/pair>\
<input name=code inputmode=text autocomplete=one-time-code autocapitalize=characters placeholder=\"XXXX-XXXX\" maxlength=16 required>\
<input name=label placeholder=\"Name for this phone (optional)\" maxlength=32>\
<button>Pair this phone</button></form>\
<small>Get a code on the server: <code>galion-admin-console issue-code</code>.</small></div>"
    ))
}

fn grade_class(g: &str) -> &'static str {
    match g {
        "A" | "B" => "ok",
        "C" | "D" => "warn",
        _ => "bad",
    }
}

fn page_paired(d: &crate::pairing::Device, s: &Value) -> String {
    let n = &s["nerve"];
    let mut body = format!(
        "<div class=card><small>Paired as {} ({})</small></div>",
        esc(&d.node),
        esc(&d.label)
    );
    if n["ok"] == true {
        let grade = n["grade"].as_str().unwrap_or("?");
        body.push_str(&format!(
            "<div class=card><div class=\"big {}\">{} <small>{}</small></div><small>nerve cycle {} · {} · {}</small><p>",
            grade_class(grade),
            esc(grade),
            esc(&n["score"].to_string()),
            esc(&n["cycle"].to_string()),
            esc(n["mode"].as_str().unwrap_or("?")),
            esc(n["ts"].as_str().unwrap_or("?")),
        ));
        if let Some(c) = n["components"].as_object() {
            let parts: Vec<String> = c.iter().map(|(k, v)| format!("{} {}", esc(k), v)).collect();
            body.push_str(&parts.join(" · "));
        }
        body.push_str("</p>");
        for a in n["attention"].as_array().into_iter().flatten() {
            body.push_str(&format!(
                "<div class=warn>{} — {}</div>",
                esc(a["name"].as_str().unwrap_or("?")),
                esc(a["status"].as_str().unwrap_or("?"))
            ));
        }
        body.push_str("</div><div class=card><b>Open incidents</b>");
        let inc = n["incidents_open"].as_array().cloned().unwrap_or_default();
        if inc.is_empty() {
            body.push_str("<p class=ok>None.</p>");
        }
        for i in inc {
            body.push_str(&format!(
                "<div class=bad>{} · {} · {}</div>",
                esc(i["severity"].as_str().unwrap_or("?")),
                esc(i["cause"].as_str().unwrap_or("?")),
                esc(i["id"].as_str().unwrap_or("?"))
            ));
        }
        body.push_str("</div>");
    } else {
        body.push_str(&format!(
            "<div class=card><p class=bad>Nerve not answering: {}</p></div>",
            esc(n["error"].as_str().unwrap_or("?"))
        ));
    }
    let c = &s["console"];
    body.push_str(&format!(
        "<div class=card>Console: <span class={}>{}</span> <small>{}</small></div>",
        if c["ok"] == true { "ok" } else { "bad" },
        esc(&c["http"].to_string()),
        esc(c["implementation"].as_str().unwrap_or(""))
    ));
    body.push_str(
        "<form method=post action=/api/v1/admin/unpair><button>Unpair this phone</button></form>",
    );
    shell(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use galion_common::http::Head;

    fn req(method: &str, target: &str, headers: &[(&str, &str)], body: &[u8]) -> Request {
        Request {
            head: Head {
                method: method.into(),
                target: target.into(),
                minor: 1,
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                content_length: Some(body.len() as u64),
                chunked: false,
                len: 0,
            },
            raw_head: vec![],
            body: body.to_vec(),
            peer: None,
        }
    }

    fn cfg(name: &str) -> Config {
        let d = std::env::temp_dir().join(format!("gac-srv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        Config {
            store: Store::open(&d).unwrap(),
            nerve: None,
            console: None,
            console_host: "console.galion.app".into(),
            origin: Some("https://console.galion.app".into()),
            secure_cookie: true,
        }
    }

    fn hdr<'a>(r: &'a Response, k: &str) -> Option<&'a str> {
        r.headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn pair_flow_and_origin() {
        let c = cfg("flow");
        let code = c.store.issue_code(300, crate::now_secs()).unwrap();
        let body = format!("code={}&label=My+Redmi", code.to_lowercase());
        let evil = req(
            "POST",
            "/api/v1/admin/pair",
            &[("Origin", "https://evil.example")],
            body.as_bytes(),
        );
        assert_eq!(c.handle(&evil).status, 403);
        let ok = req(
            "POST",
            "/api/v1/admin/pair",
            &[("Origin", "https://console.galion.app")],
            body.as_bytes(),
        );
        let r = c.handle(&ok);
        assert_eq!(r.status, 303);
        let sc = hdr(&r, "set-cookie").unwrap();
        assert!(sc.contains("HttpOnly") && sc.contains("Secure") && sc.contains("SameSite=Strict"));
        let tok = sc
            .split(';')
            .next()
            .unwrap()
            .trim_start_matches("galion_admin=");
        let cookie = format!("a=b; {COOKIE}={tok}");
        let me = c.handle(&req("GET", "/api/v1/admin", &[("Cookie", &cookie)], b""));
        let v: Value = serde_json::from_slice(&me.body).unwrap();
        assert_eq!(v["paired"], true);
        assert_eq!(v["label"], "My Redmi");
        let st = c.handle(&req(
            "GET",
            "/api/v1/admin/status",
            &[("Cookie", &cookie)],
            b"",
        ));
        assert_eq!(st.status, 200);
        let page = c.handle(&req("GET", "/admin", &[("Cookie", &cookie)], b""));
        assert!(String::from_utf8_lossy(&page.body).contains("Unpair"));
        // reuse of the code fails
        let again = c.handle(&ok);
        assert_eq!(hdr(&again, "location"), Some("/admin?e=bad"));
        // unpair
        let un = c.handle(&req(
            "POST",
            "/api/v1/admin/unpair",
            &[("Cookie", &cookie), ("Sec-Fetch-Site", "same-origin")],
            b"",
        ));
        assert_eq!(un.status, 303);
        let st = c.handle(&req(
            "GET",
            "/api/v1/admin/status",
            &[("Cookie", &cookie)],
            b"",
        ));
        assert_eq!(st.status, 401);
        assert!(matches!(
            audit::verify(&c.store.audit_path()).unwrap(),
            audit::Verify::Ok { records: 4, .. }
        ));
        let log = std::fs::read_to_string(c.store.audit_path()).unwrap();
        assert!(!log.contains(tok) && !log.contains(&code));
    }

    #[test]
    fn unpaired_and_headers() {
        let c = cfg("hdr");
        let r = c.handle(&req("GET", "/api/v1/admin/status", &[], b""));
        assert_eq!(r.status, 401);
        assert_eq!(r.body, br#"{"error":"phone required"}"#);
        assert_eq!(hdr(&r, "cache-control"), Some("no-store"));
        assert!(hdr(&r, "content-security-policy")
            .unwrap()
            .contains("default-src 'none'"));
        assert_eq!(c.handle(&req("GET", "/nope", &[], b"")).status, 404);
        assert_eq!(c.handle(&req("POST", "/admin", &[], b"")).status, 405);
        assert_eq!(
            c.handle(&req("GET", "/api/v1/admin/pair", &[], b"")).status,
            405
        );
        let cross = req(
            "POST",
            "/api/v1/admin/pair",
            &[("Sec-Fetch-Site", "cross-site")],
            b"code=x",
        );
        assert_eq!(c.handle(&cross).status, 403);
        let bogus = c.handle(&req(
            "GET",
            "/api/v1/admin",
            &[("Cookie", "galion_admin=forged")],
            b"",
        ));
        assert_eq!(bogus.body, br#"{"you":false,"paired":false}"#);
    }

    #[test]
    fn form_and_escape() {
        assert_eq!(
            form_field(b"code=ab%2Dcd&label=a+b", "code").as_deref(),
            Some("ab-cd")
        );
        assert_eq!(form_field(b"label=%", "label").as_deref(), Some("%"));
        assert_eq!(form_field(b"x=1", "code"), None);
        assert_eq!(esc("<a href=\"x\">'"), "&lt;a href=&quot;x&quot;&gt;&#39;");
        let d = crate::pairing::Device {
            node: "node-1".into(),
            paired: 0,
            revoked: false,
            label: "<script>".into(),
        };
        let html = page_paired(
            &d,
            &json!({"nerve": {"ok": false, "error": "<x>"}, "console": {}}),
        );
        assert!(!html.contains("<script>") && !html.contains("<x>"));
    }
    #[test]
    fn console_ui_gate_contract() {
        // crates/console-ui: GET /api/v1/admin -> {"you":bool}; POST /api/v1/admin
        // JSON {"code"} -> 200 + galion_admin cookie, 429 locked, else refused.
        let c = cfg("contract");
        let me = c.handle(&req("GET", "/api/v1/admin", &[], b""));
        assert_eq!(me.body, br#"{"you":false,"paired":false}"#);
        let r = c.handle(&req(
            "POST",
            "/api/v1/admin",
            &[],
            br#"{"code":"0000-0000"}"#,
        ));
        assert_eq!(r.status, 401);
        let code = c.store.issue_code(300, crate::now_secs()).unwrap();
        let body = format!(r#"{{"code":"{code}"}}"#);
        let r = c.handle(&req("POST", "/api/v1/admin", &[], body.as_bytes()));
        assert_eq!(r.status, 200);
        let sc = hdr(&r, "set-cookie").unwrap();
        assert!(sc.starts_with("galion_admin="));
        let cookie = sc.split(';').next().unwrap().to_string();
        let me = c.handle(&req("GET", "/api/v1/admin", &[("Cookie", &cookie)], b""));
        let v: Value = serde_json::from_slice(&me.body).unwrap();
        assert_eq!(v["you"], true);
        for _ in 0..crate::pairing::MAX_FAILS {
            c.handle(&req("POST", "/api/v1/admin", &[], br#"{"code":"x"}"#));
        }
        let r = c.handle(&req("POST", "/api/v1/admin", &[], br#"{"code":"x"}"#));
        assert_eq!(r.status, 429);
        assert_eq!(c.handle(&req("PUT", "/api/v1/admin", &[], b"")).status, 405);
    }
}
