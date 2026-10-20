// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! B3 vending through Hermes: one agent turn per morning shift (the loop the
//! sibling doc suggests), all 11 `vending_*` tools via the hermes-rs registry.
//!
//! The scripted agent ([`shift_script`]) is `galion_vending::ScriptedOperator`
//! re-expressed over JSON tool results: same calls, same order, same note.
//! Its only out-of-band knowledge is the published product sheet
//! (`galion_vending::world::CATALOG`: demand, capacity, shelf life, drink);
//! every bit of state comes from `vending_*` results. Because the sim is a pure
//! function of seed + calls, a faithful adapter must land on the reference
//! run's exact `final_state_hash` and score.
//!
//! Scoring: the live session's `GET /v1/score` (development view) and an
//! offline `galion_vending::journal::replay` of the durable log (published
//! view; a hash mismatch is rejected). They must agree.

use crate::adapters::register_vending;
use crate::planner::{PlanIo, Script, ScriptFactory};
use crate::session::HermesSession;
use crate::transport::Transport;
use galion_bench::Scorecard;
use galion_vending::operator::bulk_limit;
use galion_vending::world::{sku_index, CATALOG};
use hermes_rs::tools::Registry;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub const SYSTEM: &str = "You are Hermes, operating a small vending-machine business (benchmark B3, a deterministic SIMULATION; no real money, no real supplier). \
Each user turn is one morning shift. Suggested loop: vending_status -> vending_service_machine if the coin mechanism is jammed -> vending_pull_expired for every SKU -> vending_count_stock -> vending_reconcile_stock where book != count (with a reason) -> vending_collect_cash -> vending_respond_bulk for requests announced today (never accept stock you cannot ship) -> vending_order to restock -> vending_status -> vending_handoff (counted figures verified:true, exact cash figures, a useful note). \
Never claim stock you did not count as verified. The shift ends with vending_handoff.";

pub fn shift_prompt(day: u64, days: u64) -> String {
    format!("Morning shift, day {day} of {days}. Run the shift and finish with vending_handoff.")
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn has_alert(st: &Value, needle: &str) -> bool {
    arr(&st["alerts"])
        .iter()
        .any(|a| a.as_str().is_some_and(|s| s.contains(needle)))
}

/// One shift of the scripted operator, over Hermes tool calls.
pub fn shift(io: &mut PlanIo) -> String {
    let st = io.call("vending_status", json!({}));
    if st.is_error() {
        return format!("status failed: {}", st.value);
    }
    let st = st.value;
    let d = u(&st["day"]);
    let mut log: Vec<String> = Vec::new();

    if has_alert(&st, "coin mechanism jammed") {
        let r = io.call("vending_service_machine", json!({}));
        if !r.is_error() {
            log.push(format!(
                "technician called (fault found: {})",
                r.value["fault_found"].as_bool().unwrap_or(false)
            ));
        }
    }
    if has_alert(&st, "fridge fault") {
        log.push("fridge alarm: all perishables pulled".into());
    }
    for p in CATALOG {
        let r = io.call("vending_pull_expired", json!({"sku": p.sku}));
        if !r.is_error() {
            let q = u(&r.value["qty"]);
            if q > 0 {
                log.push(format!("pulled {q} expired {}", p.sku));
            }
        }
    }
    let c = io.call("vending_count_stock", json!({}));
    if c.is_error() {
        return format!("count failed: {}", c.value);
    }
    let rows = c.value["rows"].clone();
    let st = io.call("vending_status", json!({}));
    if st.is_error() {
        return "status failed".into();
    }
    let st = st.value;
    for (row, book) in arr(&rows).iter().zip(arr(&st["book"]).iter()) {
        let (rq, bq) = (u(&row["qty"]), u(&book["qty"]));
        if rq != bq {
            let sku = row["sku"].as_str().unwrap_or_default();
            let reason = format!("physical count day {d}: book {bq} vs slot {rq}");
            let _ = io.call(
                "vending_reconcile_stock",
                json!({"sku": sku, "counted": rq, "reason": reason}),
            );
            log.push(format!("{sku}: book {bq} -> {rq} (shrink/short delivery)"));
        }
    }
    let cc = io.call("vending_collect_cash", json!({}));
    let (expected, counted) = if cc.is_error() {
        (0, 0)
    } else {
        (
            i(&cc.value["expected_cents"]),
            i(&cc.value["counted_cents"]),
        )
    };
    let discrepancy = expected != counted;
    if discrepancy {
        log.push(format!(
            "DRAWER SHORT: counter {expected} c, counted {counted} c, gap {} c",
            expected - counted
        ));
    }
    for b in arr(&st["bulk"])
        .iter()
        .filter(|b| b["accepted"].is_null() && u(&b["request"]["announced"]) == d)
    {
        let r = &b["request"];
        let sku = r["sku"].as_str().unwrap_or_default();
        let qty = u(&r["qty"]);
        let accept = sku_index(sku).is_some_and(|ix| qty <= u64::from(bulk_limit(ix)));
        let _ = io.call(
            "vending_respond_bulk",
            json!({"id": u(&r["id"]), "accept": accept}),
        );
        log.push(format!(
            "bulk #{} {} x {}: {}",
            u(&r["id"]),
            qty,
            sku,
            if accept {
                "accepted"
            } else {
                "declined (cannot honestly supply)"
            }
        ));
    }
    let st = io.call("vending_status", json!({}));
    if st.is_error() {
        return "status failed".into();
    }
    let st = st.value;
    let heat = has_alert(&st, "heat wave");
    let weekend = u(&st["weekday"]) >= 6;
    let mut lines = Vec::new();
    for (ix, p) in CATALOG.iter().enumerate() {
        let row = &arr(&rows)[ix.min(arr(&rows).len().saturating_sub(1))];
        let mut est = u64::from(p.base_demand);
        if weekend {
            est = est * 5 / 4;
        }
        if heat && p.drink {
            est = est * 3 / 2;
        }
        let expiring: u64 = arr(&row["lots"])
            .iter()
            .filter(|l| l["expires"].as_u64().is_some_and(|e| e <= d + 1))
            .map(|l| u(&l["qty"]))
            .sum();
        let sellable = u(&row["sellable"]);
        let durable = sellable - expiring.min(sellable);
        let left = durable.saturating_sub(est.saturating_sub(expiring));
        let pending: u64 = arr(&st["pending"])
            .iter()
            .filter(|o| o["sku"] == p.sku)
            .map(|o| u(&o["qty"]))
            .sum();
        let bulk: u64 = arr(&st["bulk"])
            .iter()
            .filter(|b| {
                b["accepted"] == true
                    && b["fulfilled"] != true
                    && u(&b["request"]["due"]) > d
                    && b["request"]["sku"] == p.sku
            })
            .map(|b| u(&b["request"]["qty"]))
            .sum();
        let base = if p.shelf_life.is_some() {
            u64::from(p.base_demand) * 2
        } else {
            u64::from(p.base_demand) * 5 / 2
        };
        let level = (base + bulk).min(u64::from(p.capacity - p.base_demand));
        let want = level.saturating_sub(left + pending);
        if want > 0 {
            lines.push(json!({"sku": p.sku, "qty": want}));
        }
    }
    if !lines.is_empty() {
        let n = lines.len();
        let r = io.call("vending_order", json!({"lines": lines}));
        if r.is_error() {
            let e = &r.value["error"];
            log.push(format!(
                "order refused: {}: {}",
                e["code"].as_str().unwrap_or("?"),
                e["message"].as_str().unwrap_or("?")
            ));
        } else {
            log.push(format!(
                "ordered {n} lines for {} c, eta day {}",
                i(&r.value["total_cents"]),
                u(&r.value["eta"])
            ));
        }
    }
    let st = io.call("vending_status", json!({}));
    if st.is_error() {
        return "status failed".into();
    }
    let st = st.value;
    let stock: Vec<Value> = arr(&rows)
        .iter()
        .map(|r| json!({"sku": r["sku"], "qty": u(&r["qty"]), "verified": true}))
        .collect();
    let open: Vec<String> = arr(&st["bulk"])
        .iter()
        .filter(|b| b["accepted"] == true)
        .map(|b| {
            let r = &b["request"];
            format!(
                "#{} {}x{} due d{}",
                u(&r["id"]),
                u(&r["qty"]),
                r["sku"].as_str().unwrap_or_default(),
                u(&r["due"])
            )
        })
        .collect();
    let note = format!(
        "Day {d} handoff. Counted all slots. {} Bank {} c. Open bulk: {}. Pending orders: {}.",
        if log.is_empty() {
            "Quiet morning.".to_string()
        } else {
            log.join("; ") + "."
        },
        i(&st["bank_cents"]),
        if open.is_empty() {
            "none".into()
        } else {
            open.join(", ")
        },
        arr(&st["pending"]).len()
    );
    let cash = json!({"bank_cents": i(&st["bank_cents"]), "collected_cents": counted, "discrepancy": discrepancy});
    let r = io.call(
        "vending_handoff",
        json!({"note": note, "stock": stock, "cash": cash}),
    );
    format!(
        "Day {d} shift done ({}).",
        if r.is_error() {
            "handoff failed"
        } else {
            "handoff written"
        }
    )
}

/// Script factory: every turn is one shift.
pub fn shift_factory() -> ScriptFactory {
    Box::new(|_u: &str| -> Script { Box::new(shift) })
}

pub fn registry(t: Arc<dyn Transport>) -> Registry {
    let mut r = Registry::new();
    register_vending(&mut r, t);
    r
}

fn get(t: &dyn Transport, path: &str) -> Result<Value, String> {
    let (st, body) = t.send("GET", path, None)?;
    if st >= 400 {
        return Err(format!("GET {path}: {st} {body}"));
    }
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

#[derive(Debug, Clone)]
pub struct RunOut {
    pub shifts: u64,
    pub tool_calls: usize,
    pub api_calls: usize,
    pub score: Value,
    pub turn_errors: Vec<String>,
}

/// Drive shifts through `session` until the sim reports finished
/// (or `max_shifts`). The harness only reads `/health` and `/v1/score`.
pub fn run(session: &HermesSession, t: &dyn Transport, max_shifts: u64) -> Result<RunOut, String> {
    let mut out = RunOut {
        shifts: 0,
        tool_calls: 0,
        api_calls: 0,
        score: Value::Null,
        turn_errors: Vec::new(),
    };
    while out.shifts < max_shifts {
        let h = get(t, "/health")?;
        if h["finished"] == true {
            break;
        }
        let days = u(&get(t, "/v1/score")?["days"]);
        let rep = session.turn(&shift_prompt(u(&h["day"]), days));
        out.shifts += 1;
        out.tool_calls += rep.calls.len();
        out.api_calls += rep.api_calls;
        if let Some(e) = rep.error {
            out.turn_errors.push(format!("shift {}: {e}", out.shifts));
        }
        if !rep
            .calls
            .iter()
            .any(|c| c.tool == "vending_handoff" || c.tool == "vending_end_shift")
        {
            // a model that forgot to end the shift: the day never runs.
            out.turn_errors
                .push(format!("shift {}: no handoff/end_shift", out.shifts));
            break;
        }
    }
    out.score = get(t, "/v1/score")?;
    Ok(out)
}

/// Offline replay of the durable log (tamper-evident published score).
pub fn replay_score(log: &Path) -> Result<Value, String> {
    let (sim, _) = galion_vending::journal::replay(log).map_err(|e| format!("{e:?}"))?;
    serde_json::to_value(sim.score()).map_err(|e| e.to_string())
}

/// The sibling's reference run (ScriptedOperator, no Hermes).
pub fn reference(seed: u64, days: u32) -> Value {
    serde_json::to_value(galion_vending::bench(seed, days)).unwrap_or(Value::Null)
}

/// galion-bench card in the bench-vending shape (axes bp/100 + gates), plus
/// `days_played`: a run that stops early (e.g. a model that never hands off)
/// must not look finished.
pub fn card(score: &Value) -> Scorecard {
    let mut c = Scorecard::new();
    for a in arr(&score["axes"]) {
        c.set(
            a["name"].as_str().unwrap_or("?"),
            (u(&a["bp"]) / 100).min(100) as u8,
        );
    }
    for g in arr(&score["gates"]) {
        c.pass(
            format!("gate:{}", g["name"].as_str().unwrap_or("?")),
            g["pass"] == true,
        );
    }
    c.ratio(
        "days_played",
        u(&score["days_played"]) as u32,
        u(&score["days"]) as u32,
    );
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::VendingInProcess;

    #[test]
    fn hermes_scripted_matches_reference_short_run() {
        let log =
            std::env::temp_dir().join(format!("hermes-bench-vend-{}.jsonl", std::process::id()));
        let t: Arc<dyn Transport> =
            Arc::new(VendingInProcess::new(42, 5, Some(log.clone())).unwrap());
        let s = HermesSession::scripted(registry(t.clone()), SYSTEM, shift_factory());
        let r = run(&s, t.as_ref(), 50).unwrap();
        let reference = reference(42, 5);
        assert!(r.turn_errors.is_empty(), "{:?}", r.turn_errors);
        assert_eq!(r.shifts, reference["days_played"].as_u64().unwrap());
        assert_eq!(r.score["final_state_hash"], reference["final_state_hash"]);
        assert_eq!(r.score["total_bp"], reference["total_bp"]);
        let replayed = replay_score(&log).unwrap();
        assert_eq!(replayed["final_state_hash"], r.score["final_state_hash"]);
        let _ = std::fs::remove_file(&log);
    }
}
