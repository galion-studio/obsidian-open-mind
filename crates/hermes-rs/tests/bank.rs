// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Bank report on fixture data (wave 4 slot 366, F-351-4 + F-351-2).
//! No network: OpenRouter replies are saved fixture bodies.

use hermes_rs::bank::*;
use serde_json::{json, Value};
use std::path::Path;

fn fx(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bank")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn now() -> i64 {
    hermes_rs::time::parse_rfc3339("2026-10-28T05:00:00+01:00").unwrap()
}

fn ok(name: &str) -> Reply {
    Reply::Http(200, Some(fx(name)))
}

fn inputs(key: Reply, credits: Option<Reply>) -> Inputs {
    Inputs {
        openrouter_key: key,
        openrouter_credits: credits,
        river: Reply::Http(200, None),
        survival: None,
        spend_sources: Vec::new(),
        ledger: None,
        now: now(),
    }
}

#[test]
fn parity_key_limit_only_matches_bank_ts() {
    // Same numbers as deploy/staged/console-bank-reader/bank-snap.example.json (slot 351).
    let s = snapshot(&inputs(ok("key-limit-only.json"), None));
    assert_eq!(
        s["purses"][0],
        json!({"name":"OpenRouter","usd":12.5,"measured":true,"note":"key limit left · $1.40 this week · $3.10 this month"})
    );
    assert_eq!(
        s["purses"][1]["note"],
        "key live, balance not published by the API"
    );
    assert_eq!(
        s["purses"][2],
        json!({"name":"Revolut","usd":null,"measured":false,"note":"not connected"})
    );
    assert_eq!(s["purses"].as_array().unwrap().len(), 3);
    assert_eq!(s["totalUsd"], json!(12.5));
    assert!((s["dailyUsd"].as_f64().unwrap() - 0.2).abs() < 1e-12);
    assert!((s["daysLeft"].as_f64().unwrap() - 62.5).abs() < 1e-9);
    assert_eq!(s["note"], "OpenRouter recorded $1.40 this week, about $0.20 a day. The key limit lasts about 63 days at that rate.");
    assert_eq!(s["binding"], "key_limit");
    assert_eq!(s["budget"], Value::Null);
    assert_eq!(s["asOf"], "2026-10-28T05:00:00+01:00");
    // first nine keys in bank.ts order
    let keys: Vec<&String> = s.as_object().unwrap().keys().take(9).collect();
    assert_eq!(
        keys,
        [
            "purses",
            "totalUsd",
            "weekTokensIn",
            "weekTokensOut",
            "weekCostUsd",
            "dailyUsd",
            "daysLeft",
            "riverIfMovedDailyUsd",
            "note"
        ]
    );
    assert_eq!(s["weekTokensIn"].to_string(), "0"); // integer, not 0.0
}

#[test]
fn effective_budget_below_key_limit_binds() {
    let s = snapshot(&inputs(ok("key-budget-binds.json"), None));
    let p = &s["purses"][0];
    assert_eq!(p["usd"], json!(2.75), "budget 2.75 < key limit 12.5");
    assert_eq!(p["note"], "member guardrail weekly budget left ($7.25 of $10.00), resets 2026-11-02 01:00 CET; key limit $12.50 left · $1.40 this week · $3.10 this month");
    assert_eq!(s["totalUsd"], json!(2.75));
    assert_eq!(s["binding"], "budget");
    assert_eq!(s["budget"]["scope"], "member");
    assert_eq!(s["budget"]["resetsAt"], "2026-11-02T01:00:00+01:00");
    assert_eq!(s["note"], "OpenRouter recorded $1.40 this week, about $0.20 a day. The member guardrail weekly budget lasts about 14 days at that rate. It resets 2026-11-02 01:00 CET before that.");
    // The old Node logic would have said $12.50: overstated by $9.75.
    let old = openrouter_purse(&ok("key-limit-only.json"), None);
    assert_eq!(old.purse.usd, Some(12.5));
}

#[test]
fn spent_budget_blocks_and_floors_at_zero() {
    let s = snapshot(&inputs(
        ok("key-budget-spent.json"),
        Some(ok("credits.json")),
    ));
    let p = &s["purses"][0];
    assert_eq!(p["usd"], json!(0));
    assert_eq!(p["measured"], true);
    assert_eq!(p["note"], "blocked: workspace monthly budget spent ($52.00 of $50.00), resets 2026-11-01 01:00 CET; credits $18.00 left · $3.50 this week · $52.00 this month");
    assert_eq!(s["daysLeft"], json!(0));
    assert_eq!(s["budget"]["remainingUsd"], json!(-2));
}

#[test]
fn no_key_limit_budget_vs_credits() {
    // credits 18 > budget 10 -> budget binds
    let s = snapshot(&inputs(
        ok("key-no-limit-budget.json"),
        Some(ok("credits.json")),
    ));
    assert_eq!(s["purses"][0]["usd"], json!(10));
    assert_eq!(s["purses"][0]["note"], "key guardrail monthly budget left ($15.00 of $25.00), resets 2026-11-01 01:00 CET; credits $18.00 left · $0.00 this week · $6.00 this month");
    assert_eq!(s["note"], "OpenRouter recorded $0 this week and $6.00 this month. At that monthly rate, about $0.20 a day, the key guardrail monthly budget lasts about 50 days. It resets 2026-11-01 01:00 CET before that.");
    // credits not fetched: budget alone still gives a figure
    let r = openrouter_purse(&ok("key-no-limit-budget.json"), None);
    assert_eq!(r.purse.usd, Some(10.0));
    assert!(!r.purse.note.contains("credits"));
    // credits lower than budget -> credits bind, bank.ts wording
    let low = Reply::Http(
        200,
        Some(json!({"data":{"total_credits":10,"total_usage":6}})),
    );
    let r = openrouter_purse(&ok("key-no-limit-budget.json"), Some(&low));
    assert_eq!((r.purse.usd, r.binding), (Some(4.0), Binding::Credits));
    assert!(r.purse.note.starts_with("credits minus usage"));
}

#[test]
fn legacy_reply_without_budget_fields_keeps_bank_ts_paths() {
    let key = Reply::Http(
        200,
        Some(
            json!({"data":{"limit":null,"limit_remaining":null,"usage_weekly":1,"usage_monthly":2}}),
        ),
    );
    let r = openrouter_purse(&key, Some(&ok("credits.json")));
    assert_eq!(
        (r.purse.usd, r.purse.note.as_str()),
        (Some(18.0), "credits minus usage")
    );
    let r = openrouter_purse(&key, Some(&Reply::Http(500, None)));
    assert_eq!(
        (r.purse.measured, r.purse.note.as_str()),
        (false, "HTTP 500")
    );
    let r = openrouter_purse(&key, Some(&Reply::Http(200, Some(json!({"data":{}})))));
    assert_eq!(r.purse.note, "balance not published");
    let r = openrouter_purse(&key, None);
    assert_eq!(r.purse.note, "credits not read");
}

#[test]
fn budgets_without_effective_uses_least_remaining() {
    let r = openrouter_purse(&ok("key-budgets-no-effective.json"), None);
    assert_eq!(r.purse.usd, Some(5.0));
    assert_eq!(r.purse.note, "workspace lifetime budget left ($15.00 of $20.00); key limit $22.00 left · $7.00 this week · $7.00 this month");
    // effective_budget: null means no budget applies, even with budgets[]
    let r = openrouter_purse(&ok("key-effective-null.json"), None);
    assert_eq!((r.purse.usd, r.binding), (Some(22.0), Binding::KeyLimit));
    assert_eq!(r.budget, None);
}

#[test]
fn survival_fallback_and_runpod() {
    let mut inp = inputs(Reply::Failed, None);
    inp.survival = Some(fx("survival.json"));
    let s = snapshot(&inp);
    assert_eq!(
        s["purses"][0],
        json!({"name":"OpenRouter","usd":11.75,"measured":true,"note":"from survival probe 2026-10-27 18:19 CET (10.7 h old)"})
    );
    assert_eq!(s["binding"], "survival");
    assert_eq!(
        s["note"],
        "OpenRouter was not read live, so its balance is the last survival probe figure and its spend is unknown."
    );
    assert_eq!(
        s["purses"][3],
        json!({"name":"Runpod","usd":null,"measured":false,"note":"no API key on this host"})
    );
    // live read wins over survival
    let mut inp = inputs(ok("key-limit-only.json"), None);
    inp.survival = Some(fx("survival.json"));
    assert_eq!(snapshot(&inp)["purses"][0]["usd"], json!(12.5));
    // runpod money nested
    let sv = json!({"runpod":{"balance":{"USD":4.5}}});
    assert_eq!(runpod_purse(Some(&sv)).unwrap().usd, Some(4.5));
}

#[test]
fn today_ledger_only_counts_current_day() {
    let t = today_spend(Some(&fx("rs-spend.json")), now()).unwrap();
    assert_eq!((t.day.as_str(), t.usd, t.calls), ("2026-10-28", 0.4755, 20));
    let t = today_spend(Some(&fx("rs-spend-yesterday.json")), now()).unwrap();
    assert_eq!((t.usd, t.calls), (0.0, 0));
    assert_eq!(t.ledger_day.as_deref(), Some("2026-10-27"));
    assert!(today_spend(None, now()).is_none());
}

// ---- spend DBs (F-351-2) ----

#[cfg(feature = "sqlite")]
mod db {
    use super::*;
    use std::path::PathBuf;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmpdir(tag: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("hermes-bank-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    /// (id, last_activity_at as SQL literal, in, out, cost)
    fn make_db(path: &Path, rows: &[(&str, &str, i64, i64, f64)]) {
        let c = rusqlite::Connection::open(path).unwrap();
        c.execute_batch(hermes_rs::session_sqlite::SCHEMA).unwrap();
        for (id, at, i, o, cost) in rows {
            c.execute_batch(&format!(
            "INSERT INTO sessions (id, source, started_at, last_activity_at, input_tokens, output_tokens, estimated_cost_usd) VALUES ('{id}', 'api', 1700000000, {at}, {i}, {o}, {cost});"
        ))
        .unwrap();
        }
    }

    pub fn fixture_dbs(dir: &Path, now: i64) -> (PathBuf, PathBuf) {
        let own = dir.join("rs-state.db");
        let legacy = dir.join("state.db");
        let h = |x: i64| (now - x * 3600).to_string();
        make_db(
            &own,
            &[
                ("s-own-1", &h(1), 1000, 100, 0.9),
                ("s-own-2", &h(48), 2000, 200, 0.3),
                ("s-own-old", &h(8 * 24), 500000, 50000, 9.0),
                ("s-dup", &h(1), 10, 1, 0.05),
            ],
        );
        make_db(
            &legacy,
            &[
                ("s-dup", &h(1), 99999, 9999, 5.0),
                // frozen legacy DB: last write 2026-10-18 21:12 CEST, outside the week
                ("s-legacy-1", "'2026-10-18 21:12:00+02:00'", 7000, 700, 2.0),
                (
                    "s-legacy-ms",
                    &((now - 3 * 24 * 3600) * 1000).to_string(),
                    0,
                    0,
                    0.0,
                ),
            ],
        );
        (own, legacy)
    }

    #[test]
    fn week_spend_reads_own_db_first_and_dedupes() {
        let t = tmpdir("dbs");
        let (own, legacy) = fixture_dbs(&t.0, now());
        let own_rows = read_spend_db(&own).unwrap();
        let leg_rows = read_spend_db(&legacy).unwrap();
        assert_eq!((own_rows.len(), leg_rows.len()), (4, 3));
        let w = week_spend(&[own_rows.clone(), leg_rows.clone()], now());
        assert_eq!((w.tokens_in, w.tokens_out), (3010.0, 301.0));
        assert!((w.cost_usd - 1.25).abs() < 1e-12);
        assert_eq!((w.sessions, w.duplicates), (4, 1)); // 3 own + legacy ms row (cost 0)
                                                        // F-351-2: the legacy DB alone (what bank.ts read) shows the dup only
        let only_legacy = week_spend(&[leg_rows], now());
        assert!((only_legacy.cost_usd - 5.0).abs() < 1e-12);
        // missing file = empty, not an error
        assert!(read_spend_db(&t.0.join("nope.db")).unwrap().is_empty());
        // not a DB -> short error without OS text
        std::fs::write(t.0.join("bad.db"), b"not sqlite at all, just text").unwrap();
        let e = read_spend_db(&t.0.join("bad.db")).unwrap_err();
        assert!(!e.contains('/'), "{e}");
    }

    #[test]
    fn snapshot_with_local_spend() {
        let t = tmpdir("snap");
        let (own, legacy) = fixture_dbs(&t.0, now());
        let mut inp = inputs(ok("key-effective-null.json"), None);
        inp.spend_sources = vec![
            read_spend_db(&own).unwrap(),
            read_spend_db(&legacy).unwrap(),
        ];
        inp.ledger = Some(fx("rs-spend.json"));
        let s = snapshot(&inp);
        assert_eq!(s["weekTokensIn"], json!(3010));
        assert_eq!(s["weekTokensOut"], json!(301));
        assert!((s["weekCostUsd"].as_f64().unwrap() - 1.25).abs() < 1e-12);
        // OpenRouter shows no spend, local sessions do -> local rate
        assert!((s["dailyUsd"].as_f64().unwrap() - 1.25 / 7.0).abs() < 1e-12);
        assert_eq!(s["localSpend"]["rateSource"], "local_week");
        assert_eq!(s["localSpend"]["todayCalls"], json!(20));
        assert_eq!(s["note"], "OpenRouter recorded no spend this week or this month, so no runway is estimated. hermes-rs sessions recorded $1.25 in the last 7 days and $0.48 today (20 calls). At that local rate the balance lasts about 123 days.");
        let river = (3010.0 / 1e6 * 0.12 + 301.0 / 1e6 * 0.48) / 7.0;
        assert!((s["riverIfMovedDailyUsd"].as_f64().unwrap() - river).abs() < 1e-15);
        // provider unreadable: no runway, local figures still reported
        let mut inp2 = inp.clone();
        inp2.openrouter_key = Reply::NotAsked;
        let s = snapshot(&inp2);
        assert_eq!(s["totalUsd"], Value::Null);
        assert_eq!(s["daysLeft"], Value::Null);
        assert_eq!(s["note"], "No provider balance could be read, so there is no runway figure. hermes-rs sessions recorded $1.25 in the last 7 days and $0.48 today (20 calls).");
    }
}

#[test]
fn snapshot_never_echoes_key_material() {
    // A key-looking label in the provider body must not reach the snapshot.
    // built at run time so the repo secret scanner does not see a literal
    let label = format!("sk-{}-{}", "or-v1", "0".repeat(16));
    let key = Reply::Http(
        200,
        Some(json!({"data":{"label": label,"limit_remaining":3,"usage_weekly":1}})),
    );
    let s = snapshot(&inputs(key, None)).to_string();
    assert!(!s.contains("sk-or"), "{s}");
}
