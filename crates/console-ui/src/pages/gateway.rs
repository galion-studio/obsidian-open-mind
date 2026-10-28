// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/gateway` (console-ide `src/routes/_console/gateway.tsx` +
//! `API_ENDPOINTS` from `src/lib/fleet.ts`, live tree 2026-10-20 21:25
//! CEST): pick a catalog endpoint, send it, read the answer.
//!
//! Without JavaScript:
//! - picking is a link (`/gateway?ep=N`);
//! - a GET endpoint is sent with a GET form (`/gateway?ep=N&send=1`); the
//!   server makes the same GET upstream with the browser's admin cookie;
//! - a POST endpoint is sent with a same-origin-checked `<form method=post>`
//!   carrying only the endpoint number; the method, path and JSON body come
//!   from [`ENDPOINTS`] (fixed in the binary), never from the browser, so
//!   the page is not an open relay;
//! - the answer is pretty-printed like `JSON.stringify(json, null, 2)` (or
//!   shown as text), capped at [`MAX_SHOWN`] characters.

use super::p2::{clip, pretty};
use crate::html::esc;
use crate::icons::icon;
use crate::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    pub method: &'static str,
    pub path: &'static str,
    pub title: &'static str,
    /// JSON body for POST endpoints.
    pub body: Option<&'static str>,
}

const fn get(path: &'static str, title: &'static str) -> Endpoint {
    Endpoint {
        method: "GET",
        path,
        title,
        body: None,
    }
}

/// `API_ENDPOINTS` (fleet.ts), same order.
pub const ENDPOINTS: &[Endpoint] = &[
    get("/api/v1/health", "Health"),
    get("/api/v1/domains", "Domains"),
    get("/api/v1/services", "Services"),
    get("/api/v1/repos", "Repositories"),
    get("/api/v1/markers", "Markers"),
    get("/api/v1/ping", "Ping public edge"),
    get("/api/v1/ledger", "Console ledger"),
    Endpoint {
        method: "POST",
        path: "/api/v1/command",
        title: "Talk",
        body: Some("{\"cmd\":\"ping\"}"),
    },
    get("/api/v1/catalog", "Full API paper trail"),
    get("/api/v1/protocol/snapshot", "GLP snapshot"),
    Endpoint {
        method: "POST",
        path: "/api/v1/boot",
        title: "Boot light stack",
        body: Some("{\"mode\":\"light\"}"),
    },
];

/// Largest answer shown, in characters.
pub const MAX_SHOWN: usize = 200_000;

/// Endpoint number from a query/form value (`None` if out of range).
pub fn pick(v: Option<&str>) -> Option<usize> {
    v.and_then(|s| s.parse::<usize>().ok())
        .filter(|i| *i < ENDPOINTS.len())
}

/// What one send produced: HTTP status (0 = no answer) and the text shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub status: u16,
    pub output: String,
}

impl Sent {
    /// From an upstream answer: pretty JSON when it parses, else the text.
    pub fn from_answer(status: u16, body: &[u8]) -> Sent {
        let output = match json::parse(body) {
            Ok(v) => pretty(&v),
            Err(_) => String::from_utf8_lossy(body).into_owned(),
        };
        Sent {
            status,
            output: clip(&output, MAX_SHOWN),
        }
    }

    pub fn failed(msg: &str) -> Sent {
        Sent {
            status: 0,
            output: msg.to_string(),
        }
    }
}

/// Render the room with endpoint `active` selected and the last send.
pub fn body(active: usize, sent: Option<&Sent>, note: &str) -> String {
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Gateway</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">Live catalog API</span></h1>\
<p class=\"lede\">These routes are real. No mock. Health, domains, services, repos, and the CPU pack — the API world this boot is built around.</p></header>",
        icon("gateway", "i i-md")
    ));
    out.push_str("<nav class=\"eps\" aria-label=\"Endpoints\">");
    for (i, e) in ENDPOINTS.iter().enumerate() {
        let cur = if i == active {
            " aria-current=\"true\""
        } else {
            ""
        };
        out.push_str(&format!(
            "<a href=\"/gateway?ep={i}\"{cur}><span>{}</span><span class=\"muted\">{}</span></a>",
            e.method,
            esc(e.path.trim_start_matches("/api/v1"))
        ));
    }
    out.push_str("</nav>");
    if let Some(e) = ENDPOINTS.get(active) {
        let status = sent
            .map(|s| format!("<span class=\"muted\">{}</span>", s.status))
            .unwrap_or_else(|| "<span></span>".into());
        let send = if e.method == "GET" {
            format!(
                "<form method=\"get\" action=\"/gateway\"><input type=\"hidden\" name=\"ep\" value=\"{active}\"><input type=\"hidden\" name=\"send\" value=\"1\"><button class=\"btn primary\" type=\"submit\">Send</button></form>"
            )
        } else {
            format!(
                "<form method=\"post\" action=\"/gateway\"><input type=\"hidden\" name=\"ep\" value=\"{active}\"><button class=\"btn primary\" type=\"submit\">Send</button></form>"
            )
        };
        out.push_str(&format!(
            "<ul class=\"rows\"><li><span>{}</span><span class=\"muted mono\">{}</span></li>\
<li><span>{}</span>{status}</li>",
            e.method,
            esc(e.path),
            esc(e.title)
        ));
        if let Some(b) = e.body {
            out.push_str(&format!(
                "<li><span>Body</span><span class=\"muted mono\">{}</span></li>",
                esc(b)
            ));
        }
        out.push_str(&format!("<li><span>Send</span>{send}</li></ul>"));
        if !note.is_empty() {
            out.push_str(&format!("<p class=\"note\">{}</p>", esc(note)));
        }
        let shown = sent
            .map(|s| s.output.as_str())
            .unwrap_or("Select an endpoint and send.");
        out.push_str(&format!(
            "<pre class=\"json\" aria-live=\"polite\">{}</pre>",
            esc(shown)
        ));
    }
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_matches_fleet_ts() {
        assert_eq!(ENDPOINTS.len(), 11);
        assert_eq!(ENDPOINTS.iter().filter(|e| e.method == "POST").count(), 2);
        for e in ENDPOINTS {
            assert!(e.path.starts_with("/api/v1/"));
            assert_eq!(e.method == "POST", e.body.is_some());
            if let Some(b) = e.body {
                assert!(json::parse(b.as_bytes()).is_ok());
            }
        }
        assert_eq!(pick(Some("10")), Some(10));
        assert_eq!(pick(Some("11")), None);
        assert_eq!(pick(Some("-1")), None);
        assert_eq!(pick(Some("x")), None);
        assert_eq!(pick(None), None);
    }

    #[test]
    fn renders_list_and_answer() {
        let b = body(0, None, "");
        assert!(b.contains("<a href=\"/gateway?ep=0\" aria-current=\"true\"><span>GET</span><span class=\"muted\">/health</span></a>"));
        assert!(b.contains("Select an endpoint and send."));
        assert!(b.contains("name=\"send\" value=\"1\""));
        assert!(!b.contains("method=\"post\""));
        let s = Sent::from_answer(200, br#"{"status":"<ok>"}"#);
        let b = body(7, Some(&s), "");
        assert!(b.contains("method=\"post\" action=\"/gateway\""));
        assert!(b.contains("{&quot;cmd&quot;:&quot;ping&quot;}"));
        assert!(b.contains("<span>Talk</span><span class=\"muted\">200</span>"));
        assert!(b.contains("{\n  &quot;status&quot;: &quot;&lt;ok&gt;&quot;\n}"));
        let t = Sent::from_answer(502, b"Bad <gateway>");
        assert_eq!(t.output, "Bad <gateway>");
        assert!(body(1, Some(&t), "n").contains("Bad &lt;gateway&gt;"));
        let big = Sent::from_answer(200, "x".repeat(MAX_SHOWN + 5).as_bytes());
        assert!(big.output.ends_with("(5 more characters not shown)"));
    }
}
