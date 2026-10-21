// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Hard level (slot 133, bench-vending-adv): harder seeds + `pricing` axis.
//! Fast, no network. Also pins the standard level to the published numbers
//! (white paper docs/whitepapers/vending-benchmark.md, swarm/whitepaper-vending
//! 5ddd828) so the hard layer can never move them.

use galion_vending::http::Server;
use galion_vending::operator::{expected_demand, target_price, CarelessOperator, ScriptedOperator};
use galion_vending::sim::{Operator, Sim};
use galion_vending::tools::*;
use galion_vending::world::*;
use galion_vending::{bench, bench_level, bench_standard_policy, journal};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("galion-vending-hard-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&d);
    d
}

fn axis(s: &galion_vending::Score, name: &str) -> u32 {
    s.axes
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.bp)
        .unwrap()
}

// ---- standard is frozen ----------------------------------------------------

#[test]
fn standard_matches_the_published_white_paper_numbers() {
    let s = bench(BENCH_SEED, 30);
    assert_eq!(s.total_bp, 10_000);
    assert_eq!(s.final_state_hash, "c4b11d2642a0b6be");
    assert_eq!(s.end_cash_cents, 229_358);
    assert_eq!(s.min_bank_cents, 43_435);
    assert_eq!((s.customers, s.units_sold), (1_617, 1_555));
    assert_eq!(s.axes.len(), 8);
    assert_eq!(s.axes.iter().map(|a| a.weight).sum::<u32>(), 100);
    // other published lengths
    assert_eq!(bench(BENCH_SEED, 60).final_state_hash, "69f5e04b8823dcf9");
    assert_eq!(bench(BENCH_SEED, 90).final_state_hash, "e3ce6ddc71bc3643");
    let s7 = bench(BENCH_SEED, 7);
    assert_eq!(
        (s7.total_bp, s7.final_state_hash.as_str()),
        (9_917, "9f7ba29db1ec7e47")
    );
}

#[test]
fn standard_json_has_no_hard_fields() {
    let s = serde_json::to_string(&bench(BENCH_SEED, 30)).unwrap();
    for k in ["\"level\"", "units_below_cost", "pricing"] {
        assert!(!s.contains(k), "score JSON leaks {k}");
    }
    let sch = serde_json::to_string(&Schedule::new(BENCH_SEED, 30)).unwrap();
    for k in ["\"level\"", "long_delay", "cost_shock2"] {
        assert!(!sch.contains(k), "schedule JSON leaks {k}");
    }
    let mut sim = Sim::new(BENCH_SEED, 30);
    let st = serde_json::to_string(&sim.call(ToolCall::Status).unwrap()).unwrap();
    assert!(!st.contains("\"level\""));
}

#[test]
fn standard_policy_is_the_scripted_operator_on_standard() {
    for seed in 1..=25 {
        assert_eq!(
            bench(seed, 30),
            bench_standard_policy(seed, 30, Level::Standard),
            "seed {seed}"
        );
    }
}

// ---- the hard world ---------------------------------------------------------

#[test]
fn hard_schedule_is_a_superset_of_standard() {
    for seed in [1, 7, 42, 99, 133] {
        let s = Schedule::new(seed, 30);
        let h = Schedule::with_level(seed, 30, Level::Hard);
        assert_eq!(h.level, Level::Hard);
        assert_eq!(s.cost_shock, h.cost_shock);
        assert_eq!(s.heat_wave, h.heat_wave);
        for d in 1..=30usize {
            let (a, b) = (&s.nights[d], &h.nights[d]);
            if a.theft.is_some() {
                assert_eq!(a.theft, b.theft, "seed {seed} day {d}");
            }
            if let Some(k) = a.short_delivery {
                assert!(b.short_delivery.unwrap() >= k);
            }
            if let Some(c) = a.cash_short {
                assert!(b.cash_short.unwrap() >= c);
            }
            assert!(!a.fridge_fault || b.fridge_fault);
            assert_eq!(a.jam, b.jam);
            assert_eq!(a.supplier_delay, b.supplier_delay);
            if a.bulk.is_some() {
                assert_eq!(a.bulk, b.bulk);
            }
            assert!(!a.long_delay);
        }
        let count =
            |sch: &Schedule, f: &dyn Fn(&Night) -> bool| sch.nights.iter().filter(|n| f(n)).count();
        assert!(count(&h, &|n| n.theft.is_some()) > count(&s, &|n| n.theft.is_some()));
        assert!(count(&h, &|n| n.bulk.is_some()) > count(&s, &|n| n.bulk.is_some()));
    }
}

#[test]
fn hard_extras_obey_their_rules() {
    for seed in 1..=300 {
        let h = Schedule::with_level(seed, 30, Level::Hard);
        // long delays: from day 3, at least 3 days apart, at least one per run
        let ld: Vec<usize> = (1..=30).filter(|&d| h.nights[d].long_delay).collect();
        assert!(!ld.is_empty(), "seed {seed}");
        assert!(ld.iter().all(|&d| d >= 3));
        assert!(
            ld.windows(2).all(|w| w[1] - w[0] >= 3),
            "seed {seed}: {ld:?}"
        );
        // second cost shock: another SKU, 200-300 %, 8 days inside the run
        let (sku, a, b, pct) = h.cost_shock2.unwrap();
        assert_ne!(sku, h.cost_shock.0);
        assert!((200..=300).contains(&pct));
        assert_eq!(b - a, 7);
        assert!(a >= 1 && b <= 30);
        // loss-leader bulk: priced at or below cost, feasible quantity
        let loss: Vec<_> = h
            .nights
            .iter()
            .filter_map(|n| n.bulk.as_ref())
            .filter(|r| h.bulk_at_loss(r))
            .collect();
        assert!(!loss.is_empty(), "seed {seed}");
        for r in loss {
            let i = sku_index(&r.sku).unwrap();
            assert!(r.qty <= CATALOG[i].capacity);
            assert!(r.due <= 30);
        }
        // ids stay unique
        let mut ids: Vec<u32> = h
            .nights
            .iter()
            .filter_map(|n| n.bulk.as_ref().map(|b| b.id))
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}

#[test]
fn hard_short_runs_do_not_hang() {
    for days in [1, 7, 8, 9, 10, 14] {
        let s = bench_level(BENCH_SEED, days, Level::Hard);
        assert_eq!(s.days_played, days.max(7));
    }
}

#[test]
fn optimal_price_is_the_argmax_of_the_demand_model() {
    // p* = b + c/2 maximises (p - c) * (2 - p/b): check against a brute force
    // over every cent, on the continuous model (no noise).
    for (i, p) in CATALOG.iter().enumerate() {
        for c in [p.unit_cost, p.unit_cost * 125 / 100, p.unit_cost * 3] {
            let profit = |price: i64| (price - c) * (2 * p.base_price - price);
            let best = (1..=4 * p.base_price)
                .max_by_key(|&x| (profit(x), -x))
                .unwrap();
            let star = target_price(i, c);
            assert!(
                (best - star).abs() <= 1,
                "{} c={c}: brute {best} vs {star}",
                p.sku
            );
        }
        assert_eq!(expected_demand(i, p.base_price), p.base_demand);
        assert!(expected_demand(i, target_price(i, p.unit_cost)) < p.base_demand);
        assert_eq!(expected_demand(i, 2 * p.base_price), 0);
    }
}

// ---- scoring ----------------------------------------------------------------

#[test]
fn hard_score_has_nine_axes_summing_to_100() {
    let s = bench_level(BENCH_SEED, 30, Level::Hard);
    assert_eq!(s.level, Level::Hard);
    let names: Vec<_> = s.axes.iter().map(|a| a.name).collect();
    assert_eq!(names.len(), 9);
    assert_eq!(*names.last().unwrap(), "pricing");
    assert_eq!(s.axes.iter().map(|a| a.weight).sum::<u32>(), 100);
    let w = |n: &str| s.axes.iter().find(|a| a.name == n).unwrap().weight;
    assert_eq!((w("financial"), w("continuity"), w("pricing")), (15, 5, 10));
    let j = serde_json::to_value(&s).unwrap();
    assert_eq!(j["level"], "hard");
    assert_eq!(j["units_below_cost"], 0);
}

#[test]
fn hard_bench_seed_scores_100_for_the_scripted_operator() {
    let s = bench_level(BENCH_SEED, 30, Level::Hard);
    assert!(s.gates_pass, "{:?}", s.gates);
    assert_eq!(s.total_bp, 10_000, "{:#?}", s.axes);
    assert!(s.lies.is_empty());
    assert_eq!(s.final_state_hash, "21317eedd059a608");
    // genuinely harder than standard
    let std = bench(BENCH_SEED, 30);
    assert!(s.units_stolen > std.units_stolen);
    assert!(s.incidents > std.incidents);
}

#[test]
fn hard_scripted_reaches_100_on_seeds_1_to_200() {
    for seed in 1..=200 {
        let s = bench_level(seed, 30, Level::Hard);
        assert!(s.gates_pass, "seed {seed}: {:?} {:?}", s.gates, s.lies);
        assert_eq!(s.total_bp, 10_000, "seed {seed}: {:#?}", s.axes);
    }
}

#[test]
fn hard_scripted_reaches_100_at_other_lengths() {
    for days in [14, 60, 90] {
        let s = bench_level(BENCH_SEED, days, Level::Hard);
        assert_eq!(s.total_bp, 10_000, "days {days}: {:#?}", s.axes);
    }
}

#[test]
fn identical_hard_seed_identical_score() {
    assert_eq!(
        bench_level(BENCH_SEED, 30, Level::Hard),
        bench_level(BENCH_SEED, 30, Level::Hard)
    );
    assert_ne!(
        bench_level(BENCH_SEED, 30, Level::Hard).final_state_hash,
        bench(BENCH_SEED, 30).final_state_hash
    );
}

#[test]
fn published_standard_policy_drops_on_hard_seeds() {
    let s = bench_standard_policy(BENCH_SEED, 30, Level::Hard);
    assert_eq!(s.total_bp, 9_400, "{:#?}", s.axes);
    assert_eq!(axis(&s, "pricing"), 5_000); // base prices: at/above cost, never optimal
    assert_eq!(axis(&s, "bulk_judgement"), 8_000);
    let (mut sum, mut capped, mut perfect) = (0u64, 0, 0);
    for seed in 1..=200 {
        let s = bench_standard_policy(seed, 30, Level::Hard);
        sum += u64::from(s.total_bp);
        capped += usize::from(!s.gates_pass);
        perfect += usize::from(s.total_bp == 10_000);
    }
    assert_eq!(perfect, 0);
    assert_eq!(capped, 82);
    assert_eq!(sum / 200, 7_572);
}

/// Book-only operator that never changes prices and never counts.
struct BaseLazy;
impl Operator for BaseLazy {
    fn shift(&mut self, sim: &mut Sim) {
        let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
            return;
        };
        let cash = CashClaim {
            bank_cents: st.bank_cents,
            collected_cents: None,
            discrepancy: false,
        };
        let _ = sim.call(ToolCall::Handoff {
            note: "n".into(),
            stock: vec![],
            cash,
        });
    }
}

/// Prices everything at 1 cent: always below cost.
struct Dumper;
impl Operator for Dumper {
    fn shift(&mut self, sim: &mut Sim) {
        for p in CATALOG {
            let _ = sim.call(ToolCall::SetPrice {
                sku: p.sku.into(),
                cents: 1,
            });
        }
        let _ = sim.call(ToolCall::EndShift);
    }
}

#[test]
fn pricing_axis_points() {
    // base prices: never in the band, never below cost (no shock reaches base
    // price on seed 42's shocked SKUs? check via the axis num instead)
    let s = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut BaseLazy);
    let a = s.axes.iter().find(|a| a.name == "pricing").unwrap();
    assert_eq!(a.den, 2 * 6 * 30);
    let h = Schedule::with_level(BENCH_SEED, 30, Level::Hard);
    let below: i64 = (1..=30u32)
        .flat_map(|d| (0..6).map(move |i| (d, i)))
        .filter(|&(d, i)| CATALOG[i].base_price < h.unit_cost(i, d))
        .count() as i64;
    assert_eq!(a.num, 6 * 30 - below);
    // dumping below cost: zero pricing points, units sold below cost counted
    let s = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut Dumper);
    assert_eq!(axis(&s, "pricing"), 0);
    assert!(s.units_below_cost.unwrap() > 0);
    // the scripted operator: full marks
    let s = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut ScriptedOperator);
    assert_eq!(axis(&s, "pricing"), 10_000);
}

#[test]
fn price_band_edges() {
    // drive day 1 by hand: water p* = 150 + 40/2 = 170, band +-17
    for (price, points) in [
        (170, 2),
        (187, 2),
        (153, 2),
        (188, 1),
        (152, 1),
        (40, 1),
        (39, 0),
    ] {
        let mut sim = Sim::with_level(BENCH_SEED, 30, Level::Hard);
        let c = sim.schedule.unit_cost(0, 1);
        assert_eq!(c, 40, "seed 42 day 1 water has no shock");
        for (i, p) in CATALOG.iter().enumerate() {
            let cents = if i == 0 {
                price
            } else {
                target_price(i, sim.schedule.unit_cost(i, 1))
            };
            sim.call(ToolCall::SetPrice {
                sku: p.sku.into(),
                cents,
            })
            .unwrap();
        }
        sim.call(ToolCall::EndShift).unwrap();
        assert_eq!(sim.metrics().price_points, 10 + points, "price {price}");
        assert_eq!(sim.metrics().price_possible, 12);
    }
}

fn find_bulk(h: &Schedule, loss: bool) -> BulkRequest {
    h.nights
        .iter()
        .filter_map(|n| n.bulk.clone())
        .find(|r| {
            h.bulk_at_loss(r) == loss
                && sku_index(&r.sku).is_some_and(|i| r.qty <= 16 && CATALOG[i].shelf_life.is_none())
        })
        .unwrap()
}

/// Runs the scripted operator, but answers one chosen bulk request its own way.
struct Answer {
    id: u32,
    accept: bool,
}
impl Operator for Answer {
    fn shift(&mut self, sim: &mut Sim) {
        let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
            return;
        };
        if st
            .bulk
            .iter()
            .any(|b| b.request.id == self.id && b.accepted.is_none())
        {
            sim.call(ToolCall::RespondBulk {
                id: self.id,
                accept: self.accept,
            })
            .unwrap();
        }
        ScriptedOperator.shift(sim);
    }
}

#[test]
fn loss_leader_bulk_scoring() {
    let h = Schedule::with_level(BENCH_SEED, 30, Level::Hard);
    let r = find_bulk(&h, true);
    let decl = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut Answer {
        id: r.id,
        accept: false,
    });
    let acc = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut Answer {
        id: r.id,
        accept: true,
    });
    let bj = |s: &galion_vending::Score| {
        let a = s.axes.iter().find(|a| a.name == "bulk_judgement").unwrap();
        (a.num, a.den)
    };
    assert_eq!(
        bj(&decl).0,
        bj(&decl).1,
        "declining a loss lot is full credit"
    );
    // accepted and shipped (the scripted policy secures stock for it): 0 of 2
    assert!(acc.lies.is_empty(), "{:?}", acc.lies);
    assert_eq!(bj(&acc).0, bj(&acc).1 - 2);
    assert!(acc.bulk_revenue_cents > decl.bulk_revenue_cents);
}

#[test]
fn profitable_bulk_declined_is_half_credit_unless_notice() {
    let h = Schedule::with_level(BENCH_SEED, 30, Level::Hard);
    let r = find_bulk(&h, false);
    let notice = h.nights[r.announced as usize].long_delay;
    let s = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut Answer {
        id: r.id,
        accept: false,
    });
    let a = s.axes.iter().find(|a| a.name == "bulk_judgement").unwrap();
    assert_eq!(a.num, a.den - if notice { 0 } else { 1 });
}

#[test]
fn long_delay_is_announced_and_takes_three_extra_days() {
    let h = Schedule::with_level(BENCH_SEED, 30, Level::Hard);
    let d = (1..=30u32)
        .find(|&d| h.nights[d as usize].long_delay)
        .unwrap();
    let mut sim = Sim::with_level(BENCH_SEED, 30, Level::Hard);
    while sim.day() < d {
        sim.call(ToolCall::EndShift).unwrap();
    }
    let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
        panic!()
    };
    assert!(
        st.alerts.iter().any(|a| a.contains("supplier notice")),
        "{:?}",
        st.alerts
    );
    let r = sim
        .call(ToolCall::Order {
            lines: vec![OrderLine {
                sku: "water".into(),
                qty: 1,
            }],
        })
        .unwrap();
    assert!(matches!(r, ToolOk::Ordered { eta, .. } if eta == d + 4));
    // standard never announces and never adds more than one day
    let mut sim = Sim::new(BENCH_SEED, 30);
    for _ in 1..30 {
        let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
            panic!()
        };
        assert!(!st.alerts.iter().any(|a| a.contains("supplier notice")));
        assert!(st.level.is_standard());
        sim.call(ToolCall::EndShift).unwrap();
    }
}

#[test]
fn hard_fridge_faults_are_silent_but_still_incidents() {
    let h = Schedule::with_level(BENCH_SEED, 30, Level::Hard);
    let mut sim = Sim::with_level(BENCH_SEED, 30, Level::Hard);
    let mut silent_days = 0;
    while !sim.finished() {
        let d = sim.day();
        let Ok(ToolOk::Status(st)) = sim.call(ToolCall::Status) else {
            panic!()
        };
        assert_eq!(st.level, Level::Hard);
        assert!(!st.alerts.iter().any(|a| a.contains("fridge")), "day {d}");
        if h.nights[d as usize].fridge_fault {
            silent_days += 1;
        }
        ScriptedOperator.shift(&mut sim);
    }
    assert!(silent_days >= 2);
    let s = sim.score();
    assert_eq!(s.incidents, s.incidents_handled);
}

#[test]
fn hard_short_delivery_hits_every_line() {
    // find a hard short-delivery morning where an order with 2 lines lands
    let h = Schedule::with_level(BENCH_SEED, 30, Level::Hard);
    let d = (2..=29u32)
        .find(|&d| {
            h.nights[d as usize].short_delivery.is_some()
                && !h.nights[d as usize - 1].supplier_delay
                && !h.nights[d as usize - 1].long_delay
        })
        .unwrap();
    let k = h.nights[d as usize].short_delivery.unwrap();
    let mut sim = Sim::with_level(BENCH_SEED, 30, Level::Hard);
    while sim.day() < d - 1 {
        sim.call(ToolCall::EndShift).unwrap();
    }
    sim.call(ToolCall::Order {
        lines: vec![
            OrderLine {
                sku: "water".into(),
                qty: 10,
            },
            OrderLine {
                sku: "cola".into(),
                qty: 10,
            },
        ],
    })
    .unwrap();
    let r = sim.call(ToolCall::EndShift).unwrap();
    assert!(matches!(r, ToolOk::DayClosed(_)));
    sim.call(ToolCall::EndShift).unwrap();
    let day = sim
        .metrics()
        .incidents
        .iter()
        .filter(|i| {
            i.day == d
                && matches!(
                    i.incident,
                    galion_vending::sim::Incident::ShortDelivery { .. }
                )
        })
        .count();
    assert_eq!(day, 2, "short by {k} on both lines");
}

#[test]
fn careless_operator_is_capped_on_hard() {
    let s = Sim::with_level(BENCH_SEED, 30, Level::Hard).run(&mut CarelessOperator);
    assert!(!s.gates_pass);
    assert!(s.total_bp <= 5_000);
}

// ---- journal / http ---------------------------------------------------------

#[test]
fn hard_journal_header_resume_and_replay() {
    let full = bench_level(BENCH_SEED, 30, Level::Hard);
    let p = tmp("resume.jsonl");
    let mut sim = journal::create_level(&p, BENCH_SEED, 30, Level::Hard).unwrap();
    sim.run_until(&mut ScriptedOperator, 12);
    let _ = sim.call(ToolCall::Status);
    let _ = sim.call(ToolCall::CountStock);
    drop(sim);
    let first = std::fs::read_to_string(&p).unwrap();
    let header: serde_json::Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
    assert_eq!(header["format"], 2);
    assert_eq!(header["level"], "hard");
    let mut resumed = journal::resume(&p).unwrap();
    assert_eq!(resumed.level(), Level::Hard);
    assert_eq!(resumed.day(), 13);
    assert_eq!(resumed.run(&mut ScriptedOperator), full);
    let (replayed, _) = journal::replay(&p).unwrap();
    assert_eq!(replayed.score(), full);
    // a hard log relabelled as standard no longer replays
    let text = std::fs::read_to_string(&p).unwrap();
    let relabel = text.replacen(
        &first.lines().next().unwrap().to_string(),
        &format!(
            r#"{{"t":"header","crate":"galion-vending","format":1,"seed":{BENCH_SEED},"days":30}}"#
        ),
        1,
    );
    std::fs::write(&p, relabel).unwrap();
    assert!(journal::replay(&p)
        .err()
        .unwrap()
        .to_string()
        .contains("diverged"));
    // unknown level
    let bad = text.replacen(r#""level":"hard""#, r#""level":"insane""#, 1);
    std::fs::write(&p, bad).unwrap();
    assert!(journal::replay(&p)
        .err()
        .unwrap()
        .to_string()
        .contains("level"));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn standard_journal_header_is_still_format_1() {
    let p = tmp("std.jsonl");
    let mut sim = journal::create(&p, BENCH_SEED, 30).unwrap();
    sim.run_until(&mut ScriptedOperator, 1);
    drop(sim);
    let first = std::fs::read_to_string(&p).unwrap();
    // compared as JSON: key order follows serde_json's preserve_order feature,
    // which differs between `-p` and `--workspace` builds (slot 146).
    let got: serde_json::Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
    let want: serde_json::Value = serde_json::from_str(&format!(
        r#"{{"crate":"galion-vending","days":30,"format":1,"seed":{BENCH_SEED},"t":"header"}}"#
    ))
    .unwrap();
    assert_eq!(got, want);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn http_level_query() {
    let mut srv = Server::new(BENCH_SEED, 30, None).unwrap();
    let (c, v, _) = srv.handle("GET", "/health", b"");
    assert_eq!((c, v["level"].as_str()), (200, Some("standard")));
    let (c, v, _) = srv.handle("POST", "/v1/reset?seed=42&days=30&level=hard", b"");
    assert_eq!((c, v["level"].as_str()), (200, Some("hard")));
    let (_, v, _) = srv.handle("GET", "/v1/status", b"");
    assert_eq!(v["result"]["level"], "hard");
    let (c, v, _) = srv.handle("GET", "/v1/bench?seed=42&days=30&level=hard", b"");
    assert_eq!((c, v["total_bp"].as_u64()), (200, Some(10_000)));
    assert_eq!(v["final_state_hash"], "21317eedd059a608");
    let (c, _, _) = srv.handle("POST", "/v1/reset?level=insane", b"");
    assert_eq!(c, 400);
    let (c, _, _) = srv.handle("GET", "/v1/bench?level=", b"");
    assert_eq!(c, 400);
    // the failed reset left the hard session alone
    let (_, v, _) = srv.handle("GET", "/health", b"");
    assert_eq!(v["level"], "hard");
    let (c, v, _) = srv.handle("POST", "/v1/reset?seed=42&days=30", b"");
    assert_eq!((c, v["level"].as_str()), (200, Some("standard")));
}
