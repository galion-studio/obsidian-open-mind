// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The one call other services make (console, Galion One, Hermes): "is the
//! person behind this `galion_acct` cookie entitled, and to which plan?"
//! Std-only, over loopback, with the internal bearer token.

use std::net::SocketAddr;
use std::time::Duration;

use galion_common::http::{send, ClientRequest};

use crate::plans::Plan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entitled {
    pub plan: Plan,
    pub active: bool,
    pub paid_until: Option<i64>,
}

impl Entitled {
    /// True when the plan in force is at least `min`.
    pub fn at_least(&self, min: Plan) -> bool {
        (self.active || min == Plan::Free) && self.plan.rank() >= min.rank()
    }
}

/// Ask the accounts service about a session cookie value. Any failure
/// (service down, bad token) answers Free/inactive: fail closed.
pub fn check_session(addr: SocketAddr, token: &str, session: &str) -> Entitled {
    let body = serde_json::json!({ "session": session }).to_string();
    let auth = format!("Bearer {token}");
    let mut req = ClientRequest::post(
        "/internal/v1/entitlement",
        &addr.to_string(),
        "application/json",
        body.as_bytes(),
    );
    req.headers.insert(0, ("Authorization".into(), auth));
    let free = Entitled {
        plan: Plan::Free,
        active: false,
        paid_until: None,
    };
    let Ok(resp) = send(addr, &req, Duration::from_secs(3), 64 * 1024) else {
        return free;
    };
    if resp.status != 200 {
        return free;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp.text()) else {
        return free;
    };
    let plan = v
        .get("plan")
        .and_then(|p| p.as_str())
        .and_then(Plan::parse)
        .unwrap_or(Plan::Free);
    Entitled {
        plan,
        active: v.get("active").and_then(|a| a.as_bool()).unwrap_or(false),
        paid_until: v.get("paid_until").and_then(|p| p.as_i64()),
    }
}
