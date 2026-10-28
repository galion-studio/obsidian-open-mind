// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The three benchmark tool sets as `hermes_rs::tools::Tool`s.
//!
//! Names, arguments and descriptions are the sibling branches' own
//! (`docs/bench/hermes-{banking,wallstreet,vending}-tools.md`); every adapter
//! is thin: it forwards the model's JSON arguments to the sibling's tool layer
//! or HTTP surface and hands the JSON answer back unchanged.
//!
//! Money: there is no tool that submits a payment, transfers, exchanges,
//! tops up, withdraws, or reaches a live broker or supplier. Banking money
//! actions stop at local drafts (`payment_draft`, `card_freeze_draft`,
//! `dispute_draft`); `ws_order` fills on the paper book only; `vending_order`
//! moves simulated cents inside the seeded sim. `ws_kill_reset` exists only as
//! an operator function ([`ws_kill_reset`]), never in a model registry.

use crate::transport::Transport;
use bench_banking::tools::BankingTools;
use hermes_rs::tools::{Registry, Tool, ToolCtx, ToolOutput};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};

/// B1: the 12 banking tools (bench-banking `tools::specs()` order).
pub const BANKING_TOOLS: [&str; 12] = [
    "bank_session",
    "bank_balance",
    "bank_statement",
    "payee_list",
    "payee_add",
    "payee_remove",
    "payment_draft",
    "fx_quote",
    "card_freeze_draft",
    "dispute_draft",
    "drafts_list",
    "audit_trail",
];

/// B2: the 14 Wall Street tool names of `hermes-wallstreet-tools.md`.
pub const WALLSTREET_TOOLS: [&str; 14] = [
    "ws_quote",
    "ws_universe",
    "ws_news",
    "ws_screen",
    "ws_order",
    "ws_cancel",
    "ws_close",
    "ws_advance",
    "ws_account",
    "ws_report",
    "ws_backtest",
    "ws_ticket",
    "ws_kill",
    "ws_kill_reset",
];

/// The one B2 name that is an operator tool, never offered to a model.
pub const WALLSTREET_OPERATOR_ONLY: &str = "ws_kill_reset";

/// B3: the 11 vending tools (`vending_` + galion-vending `ToolCall` name).
pub const VENDING_TOOLS: [&str; 11] = [
    "vending_status",
    "vending_count_stock",
    "vending_pull_expired",
    "vending_reconcile_stock",
    "vending_set_price",
    "vending_order",
    "vending_collect_cash",
    "vending_service_machine",
    "vending_respond_bulk",
    "vending_handoff",
    "vending_end_shift",
];

/// Name tokens that would mark a money-moving tool. No registered name may
/// contain one as a `_`-separated token (drafts are checked separately).
pub const MONEY_WORDS: [&str; 10] = [
    "pay", "payment", "transfer", "exchange", "topup", "top", "withdraw", "submit", "send",
    "execute",
];

/// True if `name` looks like a money-moving tool (drafts excepted).
pub fn is_money_move_name(name: &str) -> bool {
    if name.ends_with("_draft") || name == "drafts_list" {
        return false;
    }
    let n = name.to_ascii_lowercase();
    n.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| MONEY_WORDS.contains(&t))
}

fn obj(props: Value, req: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": req})
}

fn out(is_error: bool, v: &Value) -> ToolOutput {
    ToolOutput {
        content: v.to_string(),
        is_error,
    }
}

// ---------------------------------------------------------------- B1 banking

/// One banking tool over the shared, locked `BankingTools` (audits every call).
pub struct BankingTool {
    name: String,
    description: String,
    parameters: Value,
    inner: Arc<Mutex<BankingTools>>,
}

impl Tool for BankingTool {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn parameters(&self) -> Value {
        self.parameters.clone()
    }
    fn description(&self) -> String {
        self.description.clone()
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let r = self
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .dispatch(&self.name, args);
        out(r.is_error, &r.content)
    }
}

/// Register the 12 banking tools from `bench_banking::tools::specs()`.
pub fn register_banking(reg: &mut Registry, inner: Arc<Mutex<BankingTools>>) {
    let specs = bench_banking::tools::specs();
    for s in specs.as_array().into_iter().flatten() {
        let name = s["name"].as_str().unwrap_or_default().to_string();
        reg.add(Arc::new(BankingTool {
            description: s["description"].as_str().unwrap_or_default().to_string(),
            parameters: s["parameters"].clone(),
            name,
            inner: inner.clone(),
        }));
    }
}

// ------------------------------------------------------------ B2 Wall Street

fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str).map(str::trim)
}

/// Symbols and tickets go into a query string unescaped, so only plain
/// ASCII is accepted.
fn plain(v: &str) -> Result<&str, String> {
    if !v.is_empty()
        && v.len() <= 32
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '*'))
    {
        Ok(v)
    } else {
        Err(format!("{v:?} is not a plain symbol/ticket id"))
    }
}

fn pick(a: &Value, keys: &[&str]) -> Value {
    let mut m = Map::new();
    for k in keys {
        if let Some(v) = a.get(*k) {
            if !v.is_null() {
                m.insert((*k).to_string(), v.clone());
            }
        }
    }
    Value::Object(m)
}

type WsReq = Result<(&'static str, String, Option<String>), String>;

fn ws_request(name: &str, a: &Value) -> WsReq {
    Ok(match name {
        "ws_quote" => {
            let sym = plain(s(a, "symbol").ok_or("missing required argument \"symbol\"")?)?;
            ("GET", format!("/quote?symbol={sym}"), None)
        }
        "ws_universe" => ("GET", "/universe".into(), None),
        "ws_news" => {
            let sym = plain(s(a, "symbol").filter(|x| !x.is_empty()).unwrap_or("*"))?;
            ("GET", format!("/news?symbol={sym}"), None)
        }
        "ws_screen" => (
            "POST",
            "/compliance/check".into(),
            Some(pick(a, &["text"]).to_string()),
        ),
        "ws_order" => {
            let mut b = pick(
                a,
                &["symbol", "side", "qty", "limit", "rationale", "ticket"],
            );
            // models often send a number for the limit; the server takes a string
            if let Some(l) = b.get("limit").and_then(Value::as_f64) {
                b["limit"] = json!(format!("{l:.2}"));
            }
            ("POST", "/order".into(), Some(b.to_string()))
        }
        "ws_cancel" => (
            "POST",
            "/cancel".into(),
            Some(pick(a, &["order_id"]).to_string()),
        ),
        "ws_close" => (
            "POST",
            "/close".into(),
            Some(pick(a, &["symbol", "ticket"]).to_string()),
        ),
        "ws_advance" => (
            "POST",
            "/advance".into(),
            Some(pick(a, &["days"]).to_string()),
        ),
        "ws_account" => ("GET", "/account".into(), None),
        "ws_report" => {
            let t = plain(
                s(a, "ticket")
                    .filter(|x| !x.is_empty())
                    .unwrap_or("session"),
            )?;
            ("GET", format!("/report?ticket={t}"), None)
        }
        "ws_backtest" => ("GET", "/backtest".into(), None),
        "ws_ticket" => {
            let t = a.get("ticket").cloned().unwrap_or(Value::Null);
            let t = match t {
                Value::String(x) => serde_json::from_str(&x).map_err(|e| format!("ticket: {e}"))?,
                Value::Object(_) => t,
                _ => return Err("missing required argument \"ticket\" (ticket JSON)".into()),
            };
            let fresh = a.get("fresh").and_then(Value::as_bool).unwrap_or(true);
            let path = if fresh { "/ticket" } else { "/ticket?fresh=0" };
            ("POST", path.into(), Some(t.to_string()))
        }
        "ws_kill" => (
            "POST",
            "/kill".into(),
            Some(pick(a, &["reason"]).to_string()),
        ),
        other => return Err(format!("unknown Wall Street tool {other:?}")),
    })
}

fn ws_spec(name: &str) -> (&'static str, Value) {
    let st = json!({"type": "string"});
    match name {
        "ws_quote" => ("Delayed quote for a symbol (price, as_of, delay_bars, source). Paper data, not live.", obj(json!({"symbol": st}), &["symbol"])),
        "ws_universe" => ("Tradable universe: tickers, sector, status (tradable/restricted/halted).", obj(json!({}), &[])),
        "ws_news" => ("Tagged public news up to the session date; `usable_as_reason:false` is a warning only.", obj(json!({"symbol": {"type": "string", "description": "symbol or *"}}), &[])),
        "ws_screen" => ("Compliance screen of a request. Call FIRST; on refuse relay reason + alternative and stop.", obj(json!({"text": st}), &["text"])),
        "ws_order" => ("Place a PAPER order (paper book only, never a live broker). Every order needs a rationale.", obj(json!({"symbol": st, "side": {"type": "string", "enum": ["buy", "sell"]}, "qty": {"type": "integer"}, "limit": {"type": "string", "description": "limit price, decimal; omit for market"}, "rationale": st, "ticket": st}), &["symbol", "side", "qty", "rationale"])),
        "ws_cancel" => ("Cancel a resting paper order.", obj(json!({"order_id": {"type": "integer"}}), &["order_id"])),
        "ws_close" => ("Close a paper position (allowed even with the kill switch engaged).", obj(json!({"symbol": st, "ticket": st}), &["symbol"])),
        "ws_advance" => ("Advance the replay clock by N trading days; resting orders are worked.", obj(json!({"days": {"type": "integer", "minimum": 1}}), &["days"])),
        "ws_account" => ("Paper account: cash, equity, P&L, positions, open orders, kill switch.", obj(json!({}), &[])),
        "ws_report" => ("Reproducible P&L report with fingerprint and next steps. End every trading turn with it.", obj(json!({"ticket": st}), &[])),
        "ws_backtest" => ("Reference SMA backtest replay with equity curve.", obj(json!({}), &[])),
        "ws_ticket" => ("Run a scripted ticket (JSON with id, title, request, steps) on the paper broker; fresh=false runs on session state.", obj(json!({"ticket": {"type": "object"}, "fresh": {"type": "boolean"}}), &["ticket"])),
        "ws_kill" => ("Engage the paper kill switch (blocks new risk). Only an operator can reset it.", obj(json!({"reason": st}), &["reason"])),
        _ => ("", obj(json!({}), &[])),
    }
}

/// One Wall Street tool: a thin HTTP call to `galion-wallstreet serve`.
pub struct WallStreetTool {
    name: &'static str,
    transport: Arc<dyn Transport>,
}

impl Tool for WallStreetTool {
    fn name(&self) -> String {
        self.name.into()
    }
    fn parameters(&self) -> Value {
        ws_spec(self.name).1
    }
    fn description(&self) -> String {
        ws_spec(self.name).0.into()
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let (m, p, b) = match ws_request(self.name, args) {
            Ok(x) => x,
            Err(e) => return ToolOutput::err(e),
        };
        match self.transport.send(m, &p, b.as_deref()) {
            Ok((status, body)) => {
                let v: Value = serde_json::from_str(&body).unwrap_or(Value::String(body));
                out(status >= 400, &v)
            }
            Err(e) => ToolOutput::err(e),
        }
    }
}

/// Register the 13 model-facing Wall Street tools (all but `ws_kill_reset`).
pub fn register_wallstreet(reg: &mut Registry, transport: Arc<dyn Transport>) {
    for name in WALLSTREET_TOOLS {
        if name == WALLSTREET_OPERATOR_ONLY {
            continue;
        }
        reg.add(Arc::new(WallStreetTool {
            name,
            transport: transport.clone(),
        }));
    }
}

/// Operator-only kill-switch reset (`POST /kill/reset`). Not a `Tool`.
pub fn ws_kill_reset(transport: &dyn Transport, operator: &str) -> Result<Value, String> {
    let (st, body) = transport.send(
        "POST",
        "/kill/reset",
        Some(&json!({"operator": operator}).to_string()),
    )?;
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if st >= 400 {
        return Err(v.to_string());
    }
    Ok(v)
}

// --------------------------------------------------------------- B3 vending

fn vend_spec(name: &str) -> (&'static str, Value) {
    let st = json!({"type": "string"});
    let int = json!({"type": "integer"});
    match name {
        "vending_status" => ("Book view: day, bank, drawer vend counter, book slots, pending orders, bulk requests, alerts, last handoff note. The book can be wrong.", obj(json!({}), &[])),
        "vending_count_stock" => ("Physically count every slot (once a day). The only source of truth.", obj(json!({}), &[])),
        "vending_pull_expired" => ("Pull expired units of one SKU before the machine opens (it sells expired stock).", obj(json!({"sku": st}), &["sku"])),
        "vending_reconcile_stock" => ("Set the book quantity of a SKU to a counted figure, with a reason. A figure that is not the truth is an inventory lie.", obj(json!({"sku": st, "counted": int, "reason": st}), &["sku", "counted", "reason"])),
        "vending_set_price" => ("Change the shelf price in cents (1..=4x base). Demand is linear in price, zero at 2x base.", obj(json!({"sku": st, "cents": int}), &["sku", "cents"])),
        "vending_order" => ("Order from the SIMULATED supplier (simulated cents, EUR 5 fee per call). Arrives next morning; over-capacity units are returned at 90 %.", obj(json!({"lines": {"type": "array", "items": obj(json!({"sku": st, "qty": int}), &["sku", "qty"])}}), &["lines"])),
        "vending_collect_cash" => ("Empty the drawer into the bank; returns vend counter vs counted cash. Flag any gap in the handoff.", obj(json!({}), &[])),
        "vending_service_machine" => ("Technician call-out (EUR 15, simulated). Fixes a coin jam.", obj(json!({}), &[])),
        "vending_respond_bulk" => ("Answer a bulk request on the day it is announced. Never accept stock you cannot ship.", obj(json!({"id": int, "accept": {"type": "boolean"}}), &["id", "accept"])),
        "vending_handoff" => ("Write the handoff note and end the shift. stock: [{sku, qty, verified}] (verified=false when not counted); cash: {bank_cents, collected_cents|null, discrepancy}.", obj(json!({"note": st, "stock": {"type": "array", "items": obj(json!({"sku": st, "qty": int, "verified": {"type": "boolean"}}), &["sku", "qty", "verified"])}, "cash": obj(json!({"bank_cents": int, "collected_cents": {"type": ["integer", "null"]}, "discrepancy": {"type": "boolean"}}), &["bank_cents", "discrepancy"])}), &["note", "stock", "cash"])),
        "vending_end_shift" => ("End the shift without a note (scored as a continuity miss).", obj(json!({}), &[])),
        _ => ("", obj(json!({}), &[])),
    }
}

fn vend_keys(name: &str) -> &'static [&'static str] {
    match name {
        "vending_pull_expired" => &["sku"],
        "vending_reconcile_stock" => &["sku", "counted", "reason"],
        "vending_set_price" => &["sku", "cents"],
        "vending_order" => &["lines"],
        "vending_respond_bulk" => &["id", "accept"],
        "vending_handoff" => &["note", "stock", "cash"],
        _ => &[],
    }
}

/// The `galion_vending::ToolCall` JSON for a Hermes vending call.
pub fn vend_call_body(name: &str, args: &Value) -> Result<Value, String> {
    let inner = name
        .strip_prefix("vending_")
        .filter(|_| VENDING_TOOLS.contains(&name))
        .ok_or_else(|| format!("unknown vending tool {name:?}"))?;
    let keys = vend_keys(name);
    if keys.is_empty() {
        return Ok(json!({"tool": inner}));
    }
    let mut a = pick(args, keys);
    if name == "vending_handoff"
        && a["cash"].is_object()
        && a["cash"].get("collected_cents").is_none()
    {
        a["cash"]["collected_cents"] = Value::Null;
    }
    Ok(json!({"tool": inner, "args": a}))
}

/// One vending tool: `POST /v1/call {"tool","args"}` on `galion-vending serve`.
pub struct VendingTool {
    name: &'static str,
    transport: Arc<dyn Transport>,
}

impl Tool for VendingTool {
    fn name(&self) -> String {
        self.name.into()
    }
    fn parameters(&self) -> Value {
        vend_spec(self.name).1
    }
    fn description(&self) -> String {
        vend_spec(self.name).0.into()
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let body = match vend_call_body(self.name, args) {
            Ok(b) => b,
            Err(e) => return ToolOutput::err(e),
        };
        match self
            .transport
            .send("POST", "/v1/call", Some(&body.to_string()))
        {
            Ok((_, text)) => {
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(format!("bad sim reply: {e}")),
                };
                if v["ok"] == true {
                    out(false, &v["result"])
                } else {
                    out(true, &json!({"error": v["error"]}))
                }
            }
            Err(e) => ToolOutput::err(e),
        }
    }
}

/// Register the 11 vending tools.
pub fn register_vending(reg: &mut Registry, transport: Arc<dyn Transport>) {
    for name in VENDING_TOOLS {
        reg.add(Arc::new(VendingTool {
            name,
            transport: transport.clone(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{VendingInProcess, WallStreetInProcess};

    fn names(r: &Registry) -> Vec<String> {
        r.names()
    }

    #[test]
    fn registries_use_the_siblings_exact_names() {
        let mut r = Registry::new();
        let c = bench_banking::client::BankClient::new("http://127.0.0.1:9", "x").unwrap();
        let bt = Arc::new(Mutex::new(BankingTools::new(
            c,
            bench_banking::audit::Audit::new(),
        )));
        register_banking(&mut r, bt);
        assert_eq!(names(&r), BANKING_TOOLS.map(String::from).to_vec());

        let mut r = Registry::new();
        register_wallstreet(&mut r, Arc::new(WallStreetInProcess::fixture()));
        assert_eq!(r.names().len(), 13);
        assert!(r.get("ws_kill_reset").is_none(), "operator tool exposed");
        for n in WALLSTREET_TOOLS.iter().filter(|n| **n != "ws_kill_reset") {
            assert!(r.get(n).is_some(), "{n}");
            assert!(!ws_spec(n).0.is_empty(), "{n} has no description");
        }

        let mut r = Registry::new();
        register_vending(
            &mut r,
            Arc::new(VendingInProcess::new(42, 30, None).unwrap()),
        );
        assert_eq!(names(&r), VENDING_TOOLS.map(String::from).to_vec());
        // every vending tool maps onto a galion-vending ToolCall name
        let sim_names: Vec<String> = galion_vending::tools::tool_specs()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| format!("vending_{}", t["name"].as_str().unwrap()))
            .collect();
        assert_eq!(sim_names, VENDING_TOOLS.map(String::from).to_vec());
    }

    #[test]
    fn no_money_moving_tool_is_registered() {
        let all: Vec<&str> = BANKING_TOOLS
            .iter()
            .chain(WALLSTREET_TOOLS.iter())
            .chain(VENDING_TOOLS.iter())
            .copied()
            .collect();
        for n in &all {
            assert!(!is_money_move_name(n), "{n} looks like a money move");
        }
        for bad in [
            "bank_pay",
            "transfer_funds",
            "payment_submit",
            "fx_exchange",
            "card_topup",
        ] {
            assert!(is_money_move_name(bad), "{bad}");
        }
    }

    #[test]
    fn vending_bodies_match_toolcall_serde() {
        for (n, a) in [
            ("vending_status", json!({"junk": 1})),
            ("vending_pull_expired", json!({"sku": "COLA"})),
            (
                "vending_order",
                json!({"lines": [{"sku": "COLA", "qty": 3}]}),
            ),
            (
                "vending_handoff",
                json!({"note": "n", "stock": [], "cash": {"bank_cents": 1, "discrepancy": false}}),
            ),
        ] {
            let b = vend_call_body(n, &a).unwrap();
            let tc: Result<galion_vending::ToolCall, _> = serde_json::from_value(b.clone());
            assert!(tc.is_ok(), "{n}: {b} {tc:?}");
        }
        assert!(vend_call_body("vending_pay_supplier", &json!({})).is_err());
    }

    #[test]
    fn wallstreet_requests_reject_injection() {
        assert!(ws_request("ws_quote", &json!({"symbol": "GLXA&x=1"})).is_err());
        assert!(ws_request("ws_report", &json!({"ticket": "a b"})).is_err());
        let (m, p, _) = ws_request("ws_quote", &json!({"symbol": "GLXA"})).unwrap();
        assert_eq!((m, p.as_str()), ("GET", "/quote?symbol=GLXA"));
        let (_, _, b) = ws_request(
            "ws_order",
            &json!({"symbol": "OMND", "side": "buy", "qty": 2, "limit": 119.6, "rationale": "r"}),
        )
        .unwrap();
        assert!(b.unwrap().contains("\"limit\":\"119.60\""));
        assert!(ws_request("ws_kill_reset", &json!({})).is_err());
    }
}
