// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Local-only bank client: session (refresh-token exchange, transparent
//! re-auth on 401), bounded retry on 429/5xx, and a call log for the audit.
//!
//! [`BankClient::new`] refuses every base URL whose host is not loopback.
//! There is no flag to turn that off: the live Revolut API is unreachable
//! from this crate by construction.

use crate::http::{self, Url};
use crate::mock::is_forbidden;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum BankError {
    /// Base URL is not loopback (e.g. `api.revolut.com`).
    NotLocal(String),
    /// Connect / read / write failed.
    Transport(String),
    /// Refresh-token exchange refused.
    Auth(String),
    /// Non-retryable HTTP error from the bank.
    Http { status: u16, message: String },
    /// 2xx with a body that is not JSON.
    Decode(String),
    /// Retryable errors kept coming.
    RetriesExhausted { status: u16, attempts: u32 },
    /// The client itself refused to send a money-moving request.
    Refused(String),
}

impl std::fmt::Display for BankError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BankError::NotLocal(u) => write!(
                f,
                "refused non-local bank URL {u}: the bench only talks to a loopback mock"
            ),
            BankError::Transport(e) => write!(f, "bank unreachable: {e}"),
            BankError::Auth(e) => write!(f, "bank auth failed: {e}"),
            BankError::Http { status, message } => write!(f, "bank said {status}: {message}"),
            BankError::Decode(e) => write!(f, "bank reply was not JSON: {e}"),
            BankError::RetriesExhausted { status, attempts } => {
                write!(f, "bank kept failing ({status}) after {attempts} attempts")
            }
            BankError::Refused(e) => write!(f, "client refused: {e}"),
        }
    }
}

/// One HTTP exchange, for the audit trail (no headers, no bodies).
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: String,
    pub path: String,
    pub status: u16,
}

impl Call {
    pub fn line(&self) -> String {
        format!("{} {} -> {}", self.method, self.path, self.status)
    }
}

pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

pub struct BankClient {
    url: Url,
    refresh_token: String,
    access: Option<String>,
    expires_in: u64,
    /// Calls not yet drained into the audit trail.
    pub calls: Vec<Call>,
    /// Attempts per request for retryable failures.
    pub max_attempts: u32,
    /// Base backoff; doubled per attempt, capped at 1 s.
    pub backoff: Duration,
    pub logins: u32,
}

const UA: &str = "galion-bench-banking/0.1 (mock only)";

impl BankClient {
    pub fn new(base: &str, refresh_token: &str) -> Result<Self, BankError> {
        let url = http::parse_url(base).map_err(BankError::NotLocal)?;
        if url.scheme != "http" || !is_loopback_host(&url.host) {
            return Err(BankError::NotLocal(base.to_string()));
        }
        Ok(BankClient {
            url,
            refresh_token: refresh_token.to_string(),
            access: None,
            expires_in: 0,
            calls: Vec::new(),
            max_attempts: 3,
            backoff: Duration::from_millis(20),
            logins: 0,
        })
    }

    pub fn has_session(&self) -> bool {
        self.access.is_some()
    }

    pub fn expires_in(&self) -> u64 {
        self.expires_in
    }

    /// Secrets the audit layer must never print (for redaction checks).
    pub fn secrets(&self) -> Vec<String> {
        let mut v = vec![self.refresh_token.clone()];
        if let Some(a) = &self.access {
            v.push(a.clone());
        }
        v
    }

    fn raw(
        &mut self,
        method: &str,
        path: &str,
        auth: Option<&str>,
        body: Option<&Value>,
    ) -> Result<http::Response, BankError> {
        let bytes = body.map(|b| b.to_string().into_bytes());
        let bearer = auth.map(|t| format!("Bearer {t}"));
        let mut headers: Vec<(&str, &str)> =
            vec![("User-Agent", UA), ("Accept", "application/json")];
        if bytes.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        if let Some(b) = &bearer {
            headers.push(("Authorization", b));
        }
        let r = http::send(&self.url, method, path, &headers, bytes.as_deref())
            .map_err(|e| BankError::Transport(e.to_string()))?;
        self.calls.push(Call {
            method: method.into(),
            path: path.split('?').next().unwrap_or(path).into(),
            status: r.status,
        });
        Ok(r)
    }

    /// Exchange the refresh token for a fresh access token.
    pub fn login(&mut self) -> Result<(), BankError> {
        let body =
            serde_json::json!({"grant_type": "refresh_token", "refresh_token": self.refresh_token});
        let r = self.raw("POST", "/api/1.0/auth/token", None, Some(&body))?;
        self.logins += 1;
        if r.status != 200 {
            self.access = None;
            return Err(BankError::Auth(format!(
                "token endpoint returned {}",
                r.status
            )));
        }
        let v: Value =
            serde_json::from_slice(&r.body).map_err(|e| BankError::Decode(e.to_string()))?;
        let t = v["access_token"]
            .as_str()
            .ok_or_else(|| BankError::Auth("no access_token in reply".into()))?;
        self.access = Some(t.to_string());
        self.expires_in = v["expires_in"].as_u64().unwrap_or(0);
        Ok(())
    }

    pub fn get(&mut self, path: &str) -> Result<Value, BankError> {
        self.request("GET", path, None)
    }

    pub fn post(&mut self, path: &str, body: &Value) -> Result<Value, BankError> {
        self.request("POST", path, Some(body))
    }

    pub fn delete(&mut self, path: &str) -> Result<Value, BankError> {
        self.request("DELETE", path, None)
    }

    /// Authenticated request with re-auth and retry.
    ///
    /// GET/DELETE are idempotent: retried on 429, 5xx and garbled bodies.
    /// POST is retried on 429 only (the server refused before doing
    /// anything); a POST that hit a 5xx is surfaced, never re-sent blind.
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, BankError> {
        let bare = path.split('?').next().unwrap_or(path);
        if is_forbidden(method, bare) {
            return Err(BankError::Refused(format!(
                "{method} {bare} moves money or changes bank state; Hermes drafts only"
            )));
        }
        if self.access.is_none() {
            self.login()?;
        }
        let idempotent = method != "POST";
        let mut reauthed = false;
        let mut attempt = 0;
        let mut last = 0;
        while attempt < self.max_attempts {
            attempt += 1;
            let token = self.access.clone();
            let r = self.raw(method, path, token.as_deref(), body)?;
            last = r.status;
            match r.status {
                401 if !reauthed => {
                    reauthed = true;
                    attempt -= 1; // re-auth is not a failed attempt
                    self.access = None;
                    self.login()?;
                }
                200..=299 => {
                    if r.body.is_empty() {
                        return Ok(Value::Null);
                    }
                    match serde_json::from_slice::<Value>(&r.body) {
                        Ok(v) => return Ok(v),
                        Err(e) if idempotent && attempt < self.max_attempts => {
                            let _ = e;
                            self.pause(attempt, None);
                        }
                        Err(e) => return Err(BankError::Decode(e.to_string())),
                    }
                }
                429 => {
                    let ra = r.header("retry-after").and_then(|v| v.parse::<u64>().ok());
                    self.pause(attempt, ra);
                }
                500..=599 if idempotent => self.pause(attempt, None),
                _ => {
                    let msg = serde_json::from_slice::<Value>(&r.body)
                        .ok()
                        .and_then(|v| v["message"].as_str().map(str::to_string))
                        .unwrap_or_else(|| {
                            String::from_utf8_lossy(&r.body).chars().take(200).collect()
                        });
                    return Err(BankError::Http {
                        status: r.status,
                        message: msg,
                    });
                }
            }
        }
        Err(BankError::RetriesExhausted {
            status: last,
            attempts: attempt,
        })
    }

    fn pause(&self, attempt: u32, retry_after: Option<u64>) {
        let backoff = self.backoff * 2u32.saturating_pow(attempt.saturating_sub(1));
        let d = match retry_after {
            Some(s) => Duration::from_secs(s).max(backoff),
            None => backoff,
        };
        std::thread::sleep(d.min(Duration::from_secs(1)));
    }

    pub fn drain_calls(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.calls)
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn refuses_live_banks() {
        for u in [
            "https://api.revolut.com",
            "https://b2b.revolut.com",
            "http://api.revolut.com",
            "https://sandbox-b2b.revolut.com",
            "http://10.0.0.5:80",
            "http://127.0.0.1.revolut.com:80",
            "https://127.0.0.1:19510",
        ] {
            assert!(
                matches!(BankClient::new(u, "x"), Err(BankError::NotLocal(_))),
                "{u}"
            );
        }
        assert!(BankClient::new("http://127.0.0.1:19510", "x").is_ok());
        assert!(BankClient::new("http://localhost:19510", "x").is_ok());
    }

    #[test]
    fn refuses_forbidden_before_network() {
        // Port 9 (discard) is never contacted: refusal happens first.
        let mut c = BankClient::new("http://127.0.0.1:9", "x").unwrap();
        let e = c.post("/api/1.0/pay", &Value::Null).unwrap_err();
        assert!(matches!(e, BankError::Refused(_)));
        assert!(c.calls.is_empty());
    }
}
