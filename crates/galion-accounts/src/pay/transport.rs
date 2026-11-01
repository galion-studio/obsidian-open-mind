// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! HTTPS to a payment processor's REST API, behind a small trait so tests
//! can use a local mock and builds can choose the client:
//!
//! * [`CurlTransport`] (default): runs `curl` with the request config on
//!   stdin (`-K -`), so the API key never appears in argv or `ps`. Same
//!   convention as hermes-rs and galion-one; keeps the build free of TLS
//!   crates (RULES.md hard rule 1: no openssl/asm).
//! * `ReqwestTransport` (feature `reqwest`): reqwest blocking + rustls.

use super::PayError;

#[derive(Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub method: &'static str,
    pub url: String,
    /// The API key. Never printed (see the `Debug` impl).
    pub bearer: String,
    pub idempotency_key: Option<String>,
    pub stripe_version: Option<String>,
    /// application/x-www-form-urlencoded pairs (POST body or GET query).
    pub form: Vec<(String, String)>,
}

impl std::fmt::Debug for ApiRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("bearer", &"<redacted>")
            .field("idempotency_key", &self.idempotency_key)
            .field("form", &self.form)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiResponse {
    pub status: u16,
    pub body: String,
}

pub trait Transport: Send + Sync {
    fn send(&self, req: &ApiRequest) -> Result<ApiResponse, PayError>;
}

/// Percent-encode for application/x-www-form-urlencoded.
pub fn form_encode(pairs: &[(String, String)]) -> String {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                    out.push(b as char)
                }
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        enc(k, &mut out);
        out.push('=');
        enc(v, &mut out);
    }
    out
}

/// Escape for a double-quoted curl config value.
pub(crate) fn curl_quote(s: &str) -> String {
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

/// `curl` subprocess transport.
#[derive(Debug, Clone)]
pub struct CurlTransport {
    pub curl: String,
    pub max_time_s: u32,
}

impl Default for CurlTransport {
    fn default() -> Self {
        CurlTransport {
            curl: "curl".into(),
            max_time_s: 30,
        }
    }
}

pub(crate) const STATUS_MARK: &str = "\n__galion_status__:";

impl CurlTransport {
    /// The curl config text (fed on stdin). Public for tests.
    pub fn config(&self, req: &ApiRequest) -> String {
        let body = form_encode(&req.form);
        let url = if req.method == "GET" && !body.is_empty() {
            format!("{}?{}", req.url, body)
        } else {
            req.url.clone()
        };
        let mut c = String::new();
        c.push_str(&format!("url = {}\n", curl_quote(&url)));
        c.push_str(&format!("request = {}\n", curl_quote(req.method)));
        c.push_str(&format!(
            "header = {}\n",
            curl_quote(&format!("Authorization: Bearer {}", req.bearer))
        ));
        if let Some(k) = &req.idempotency_key {
            c.push_str(&format!(
                "header = {}\n",
                curl_quote(&format!("Idempotency-Key: {k}"))
            ));
        }
        if let Some(v) = &req.stripe_version {
            c.push_str(&format!(
                "header = {}\n",
                curl_quote(&format!("Stripe-Version: {v}"))
            ));
        }
        if req.method != "GET" {
            c.push_str("header = \"Content-Type: application/x-www-form-urlencoded\"\n");
            c.push_str(&format!("data-raw = {}\n", curl_quote(&body)));
        }
        c.push_str(&format!("max-time = {}\n", self.max_time_s));
        c.push_str("proto = \"=https,http\"\n");
        c.push_str("silent\nshow-error\n");
        c.push_str(&format!(
            "write-out = {}\n",
            curl_quote(&format!("{STATUS_MARK}%{{http_code}}"))
        ));
        c
    }
}

impl Transport for CurlTransport {
    fn send(&self, req: &ApiRequest) -> Result<ApiResponse, PayError> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&self.curl)
            .args(["-K", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PayError::Transport(format!("cannot start curl: {e}")))?;
        if let Some(mut si) = child.stdin.take() {
            si.write_all(self.config(req).as_bytes())
                .map_err(|e| PayError::Transport(format!("curl stdin: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| PayError::Transport(format!("curl: {e}")))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let Some((body, code)) = text.rsplit_once(STATUS_MARK) else {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(PayError::Transport(
                err.lines().next().unwrap_or("curl failed").to_string(),
            ));
        };
        let status: u16 = code.trim().parse().unwrap_or(0);
        if status == 0 {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(PayError::Transport(
                err.lines().next().unwrap_or("no response").to_string(),
            ));
        }
        Ok(ApiResponse {
            status,
            body: body.to_string(),
        })
    }
}

/// reqwest (blocking, rustls) transport.
#[cfg(feature = "reqwest")]
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    pub(crate) client: reqwest::blocking::Client,
}

#[cfg(feature = "reqwest")]
impl ReqwestTransport {
    pub fn new() -> Result<Self, PayError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| PayError::Transport(e.to_string()))?;
        Ok(ReqwestTransport { client })
    }
}

#[cfg(feature = "reqwest")]
impl Transport for ReqwestTransport {
    fn send(&self, req: &ApiRequest) -> Result<ApiResponse, PayError> {
        let body = form_encode(&req.form);
        let mut rb = if req.method == "GET" {
            let url = if body.is_empty() {
                req.url.clone()
            } else {
                format!("{}?{}", req.url, body)
            };
            self.client.get(url)
        } else {
            self.client
                .post(&req.url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(body)
        };
        rb = rb.bearer_auth(&req.bearer);
        if let Some(k) = &req.idempotency_key {
            rb = rb.header("Idempotency-Key", k);
        }
        if let Some(v) = &req.stripe_version {
            rb = rb.header("Stripe-Version", v);
        }
        let resp = rb
            .send()
            .map_err(|e| PayError::Transport(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .map_err(|e| PayError::Transport(e.to_string()))?;
        Ok(ApiResponse { status, body })
    }
}

/// In-memory transport for tests: records requests, replays canned answers.
#[derive(Default)]
pub struct MockTransport {
    pub seen: std::sync::Mutex<Vec<ApiRequest>>,
    #[allow(clippy::type_complexity)]
    pub reply: Option<Box<dyn Fn(&ApiRequest) -> ApiResponse + Send + Sync>>,
}

impl Transport for MockTransport {
    fn send(&self, req: &ApiRequest) -> Result<ApiResponse, PayError> {
        self.seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(req.clone());
        match &self.reply {
            Some(f) => Ok(f(req)),
            None => Ok(ApiResponse {
                status: 500,
                body: "{}".into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encoding() {
        let f = vec![
            ("line_items[0][price]".to_string(), "price_1 x".to_string()),
            (
                "success_url".into(),
                "https://galion.app/account?checkout=done&x=é".into(),
            ),
        ];
        assert_eq!(
            form_encode(&f),
            "line_items%5B0%5D%5Bprice%5D=price_1+x&success_url=https%3A%2F%2Fgalion.app%2Faccount%3Fcheckout%3Ddone%26x%3D%C3%A9"
        );
    }

    #[test]
    fn curl_config_keeps_key_off_argv_and_escapes() {
        let t = CurlTransport::default();
        let c = t.config(&ApiRequest {
            method: "POST",
            url: "https://api.stripe.com/v1/checkout/sessions".into(),
            bearer: "sk_test_\"x".into(),
            idempotency_key: Some("galion-gp_1".into()),
            stripe_version: None,
            form: vec![("mode".into(), "subscription".into())],
        });
        assert!(c.contains("header = \"Authorization: Bearer sk_test_\\\"x\""));
        assert!(c.contains("Idempotency-Key: galion-gp_1"));
        assert!(c.contains("data-raw = \"mode=subscription\""));
        let dbg = format!(
            "{:?}",
            ApiRequest {
                method: "GET",
                url: "u".into(),
                bearer: "sk_live_secret".into(),
                idempotency_key: None,
                stripe_version: None,
                form: vec![],
            }
        );
        assert!(!dbg.contains("sk_live_secret"));
    }
}
