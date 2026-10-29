// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Wire protocol of `WS /stream`.
//!
//! Client to server:
//! - binary frames: 16 kHz mono PCM16LE, 20-100 ms each (10-250 ms accepted);
//! - text `{"type":"stop"}`: end of audio. The server flushes, sends the last
//!   `final` events, then closes the socket with a normal close (1000).
//!
//! Server to client (text JSON):
//! - `{"type":"partial","line":N,"text":"..."}`: provisional text of line N,
//!   still changing while the speaker talks (about every 300 ms);
//! - `{"type":"final","line":N,"text":"..."}`: line N is complete (pause /
//!   end of speech, or stop) and will not change again;
//! - `{"type":"error","message":"..."}`: a problem; fatal errors are followed
//!   by a close.
//!
//! Line numbers start at 0 and only grow within one connection.

use serde::{Deserialize, Serialize};

/// A message the server sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerMsg {
    Partial { line: u64, text: String },
    Final { line: u64, text: String },
    Error { message: String },
}

impl ServerMsg {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"type":"error","message":"serialization failed"}"#.to_string())
    }
}

/// A control message the client sends as a text frame.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMsg {
    Stop,
}

/// Parse a client text frame.
pub fn parse_client(text: &str) -> Result<ClientMsg, String> {
    serde_json::from_str::<ClientMsg>(text)
        .map_err(|_| "unknown control message; expected {\"type\":\"stop\"}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_shape() {
        let m = ServerMsg::Partial {
            line: 3,
            text: "hello \"world\"".into(),
        };
        assert_eq!(
            m.to_json(),
            r#"{"type":"partial","line":3,"text":"hello \"world\""}"#
        );
    }

    #[test]
    fn final_and_error_json_shape() {
        assert_eq!(
            ServerMsg::Final {
                line: 0,
                text: "ok".into()
            }
            .to_json(),
            r#"{"type":"final","line":0,"text":"ok"}"#
        );
        assert_eq!(
            ServerMsg::Error {
                message: "busy".into()
            }
            .to_json(),
            r#"{"type":"error","message":"busy"}"#
        );
    }

    #[test]
    fn unicode_passes_through() {
        let m = ServerMsg::Final {
            line: 1,
            text: "caf\u{e9}".into(),
        };
        let v: serde_json::Value = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(v["text"], "caf\u{e9}");
    }

    #[test]
    fn stop_parses() {
        assert_eq!(parse_client(r#"{"type":"stop"}"#), Ok(ClientMsg::Stop));
        assert_eq!(
            parse_client(r#" { "type" : "stop" } "#),
            Ok(ClientMsg::Stop)
        );
    }

    #[test]
    fn unknown_control_rejected() {
        assert!(parse_client(r#"{"type":"start"}"#).is_err());
        assert!(parse_client("stop").is_err());
        assert!(parse_client("").is_err());
    }
}
