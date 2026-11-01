// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! JSON over HTTP(S) for the crypto backends: BTCPay Server's Greenfield API
//! (`Authorization: token <key>`) and monero-wallet-rpc's JSON-RPC (HTTP
//! digest auth from `--rpc-login`, or none on a loopback-only wallet).
//!
//! Same rules as [`super::transport`]: the default build runs `curl` with the
//! request config on stdin, so keys and passwords never reach argv or `ps`;
//! `reqwest` is an optional feature. Tests use [`MockJsonTransport`] or a
//! loopback server with the real curl transport.

use super::transport::{curl_quote, ApiResponse, CurlTransport, STATUS_MARK};
use super::PayError;

/// How a request authenticates.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    /// BTCPay Greenfield: `Authorization: token <api key>`.
    Token(String),
    /// HTTP digest (monero-wallet-rpc `--rpc-login user:pass`).
    Digest {
        user: String,
        pass: String,
    },
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Auth::None => "None",
            Auth::Token(_) => "Token(<redacted>)",
            Auth::Digest { .. } => "Digest(<redacted>)",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRequest {
    pub method: &'static str,
    /// Full URL, query string included.
    pub url: String,
    pub auth: Auth,
    /// JSON body (POST/PUT).
    pub body: Option<String>,
}

pub trait JsonTransport: Send + Sync {
    fn send_json(&self, req: &JsonRequest) -> Result<ApiResponse, PayError>;
}

impl CurlTransport {
    /// The curl config text for a JSON request (fed on stdin). Public for tests.
    pub fn json_config(&self, req: &JsonRequest) -> String {
        let mut c = String::new();
        c.push_str(&format!("url = {}\n", curl_quote(&req.url)));
        c.push_str(&format!("request = {}\n", curl_quote(req.method)));
        c.push_str("header = \"Accept: application/json\"\n");
        match &req.auth {
            Auth::None => {}
            Auth::Token(k) => c.push_str(&format!(
                "header = {}\n",
                curl_quote(&format!("Authorization: token {k}"))
            )),
            Auth::Digest { user, pass } => {
                c.push_str("digest\n");
                c.push_str(&format!(
                    "user = {}\n",
                    curl_quote(&format!("{user}:{pass}"))
                ));
            }
        }
        if let Some(b) = &req.body {
            c.push_str("header = \"Content-Type: application/json\"\n");
            c.push_str(&format!("data-raw = {}\n", curl_quote(b)));
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

impl JsonTransport for CurlTransport {
    fn send_json(&self, req: &JsonRequest) -> Result<ApiResponse, PayError> {
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
            si.write_all(self.json_config(req).as_bytes())
                .map_err(|e| PayError::Transport(format!("curl stdin: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| PayError::Transport(format!("curl: {e}")))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let first_err = || {
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("curl failed")
                .to_string()
        };
        let Some((body, code)) = text.rsplit_once(STATUS_MARK) else {
            return Err(PayError::Transport(first_err()));
        };
        let status: u16 = code.trim().parse().unwrap_or(0);
        if status == 0 {
            return Err(PayError::Transport(first_err()));
        }
        Ok(ApiResponse {
            status,
            body: body.to_string(),
        })
    }
}

#[cfg(feature = "reqwest")]
impl JsonTransport for super::transport::ReqwestTransport {
    fn send_json(&self, req: &JsonRequest) -> Result<ApiResponse, PayError> {
        let mut rb = self.client.request(
            reqwest::Method::from_bytes(req.method.as_bytes())
                .map_err(|e| PayError::Transport(e.to_string()))?,
            &req.url,
        );
        rb = rb.header("Accept", "application/json");
        match &req.auth {
            Auth::None => {}
            Auth::Token(k) => rb = rb.header("Authorization", format!("token {k}")),
            Auth::Digest { .. } => {
                return Err(PayError::Transport(
                    "digest auth needs the default curl transport".into(),
                ))
            }
        }
        if let Some(b) = &req.body {
            rb = rb
                .header("Content-Type", "application/json")
                .body(b.clone());
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

/// In-memory JSON transport for tests: records requests, replays answers.
#[derive(Default)]
pub struct MockJsonTransport {
    pub seen: std::sync::Mutex<Vec<JsonRequest>>,
    #[allow(clippy::type_complexity)]
    pub reply: Option<Box<dyn Fn(&JsonRequest) -> ApiResponse + Send + Sync>>,
}

impl JsonTransport for MockJsonTransport {
    fn send_json(&self, req: &JsonRequest) -> Result<ApiResponse, PayError> {
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
    fn curl_json_config_keeps_secrets_off_argv() {
        let t = CurlTransport::default();
        let c = t.json_config(&JsonRequest {
            method: "POST",
            url: "https://pay.example/api/v1/stores/S/invoices".into(),
            auth: Auth::Token("k\"ey".into()),
            body: Some(r#"{"amount":"20.00"}"#.into()),
        });
        assert!(c.contains("header = \"Authorization: token k\\\"ey\""));
        assert!(c.contains("data-raw = \"{\\\"amount\\\":\\\"20.00\\\"}\""));
        assert!(c.contains("Content-Type: application/json"));
        let d = t.json_config(&JsonRequest {
            method: "POST",
            url: "http://127.0.0.1:18083/json_rpc".into(),
            auth: Auth::Digest {
                user: "galion".into(),
                pass: "pw".into(),
            },
            body: None,
        });
        assert!(d.contains("\ndigest\n"));
        assert!(d.contains("user = \"galion:pw\""));
        let dbg = format!(
            "{:?}",
            JsonRequest {
                method: "GET",
                url: "u".into(),
                auth: Auth::Token("secret-key".into()),
                body: None
            }
        );
        assert!(!dbg.contains("secret-key"));
    }
}
