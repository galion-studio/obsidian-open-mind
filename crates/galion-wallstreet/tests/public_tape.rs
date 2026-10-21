// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! B2b REAL-PUBLIC replay tests (slot 174): verbatim ECB reference rates and
//! U.S. Treasury par yields, replayed through the same PAPER broker, risk
//! limits and backtest as the synthetic fixture. No network, no money.

use galion_wallstreet::backtest;
use galion_wallstreet::broker::{OrderRequest, Outcome, Side};
use galion_wallstreet::market::ingest;
use galion_wallstreet::money::apply_bps;
use galion_wallstreet::public::*;
use galion_wallstreet::score;
use serde_json::Value;

fn buy(sym: &str, qty: i64) -> OrderRequest {
    OrderRequest {
        symbol: sym.into(),
        side: Some(Side::Buy),
        qty,
        limit: None,
        rationale: "replay test on public data".into(),
        ticket: "B2B-TEST".into(),
    }
}

#[test]
fn slices_match_recording() {
    let src: Value = serde_json::from_str(SOURCE_JSON).unwrap();
    let files = src["files"].as_array().unwrap();
    assert_eq!(files[0]["file"], "ecb-eurofxref-hist.csv");
    assert_eq!(files[0]["slice_fnv1a64"], fnv1a64(ECB_CSV.as_bytes()));
    assert_eq!(files[1]["file"], "ust-par-yield-curve.csv");
    assert_eq!(files[1]["slice_fnv1a64"], fnv1a64(UST_CSV.as_bytes()));
    assert_eq!(ECB_CSV.lines().count(), 41);
    assert_eq!(UST_CSV.lines().count(), 41);
    assert!(src["fetched_at"].as_str().unwrap().ends_with("+02:00"));
    let e = ecb_table();
    let u = ust_table();
    assert_eq!(e.rows.len(), 40);
    assert_eq!(u.rows.len(), 40);
    assert_eq!(files[0]["first_day"], e.rows[0].0.as_str());
    assert_eq!(files[0]["last_day"], e.rows[39].0.as_str());
    assert_eq!(files[1]["first_day"], u.rows[0].0.as_str());
    assert_eq!(files[1]["last_day"], u.rows[39].0.as_str());
}

/// Values cross-checked against an independent earlier probe (slot 134,
/// 2026-10-21 05:10 +02:00, docs/real-tests/bench-data-licence.md).
#[test]
fn known_published_values() {
    let e = ecb_table();
    assert_eq!(e.raw("2026-10-20", "USD"), Some("1.1215"));
    assert_eq!(e.raw("2026-10-16", "USD"), Some("1.1253"));
    assert_eq!(e.raw("2026-10-15", "USD"), Some("1.1208"));
    assert_eq!(e.raw("2026-10-14", "USD"), Some("1.1195"));
    assert_eq!(e.raw("2026-10-20", "BGN"), None, "N/A kept as missing");
    let u = ust_table();
    assert_eq!(u.raw("2026-10-20", "10 Yr"), Some("5.12"));
}

#[test]
fn ecb_values_unmodified_and_exact() {
    let t = ecb_table();
    let md = Tape::EcbFx.market();
    assert_eq!(verify_ecb_unmodified(&t, &md, &FX_PAIRS), Ok(160));
    assert_eq!(ecb_lot_eur(&t, "USD"), Ok(100));
    assert_eq!(ecb_lot_eur(&t, "GBP"), Ok(1000));
    assert_eq!(ecb_lot_eur(&t, "JPY"), Ok(1));
    let s = md.series("EURUSD").unwrap();
    assert_eq!((s[0].date.as_str(), s[0].close), ("2026-08-26", 11669));
    assert_eq!((s[39].date.as_str(), s[39].close), ("2026-10-20", 11215));
    // a one-pip edit is caught
    let tampered = ECB_CSV.replacen("2026-10-20,1.1215,", "2026-10-20,1.1216,", 1);
    assert_ne!(tampered, ECB_CSV);
    let bad = ingest(&ecb_bars_csv(&parse_ecb(&tampered).unwrap(), &FX_PAIRS).unwrap()).unwrap();
    let err = verify_ecb_unmodified(&t, &bad, &FX_PAIRS).unwrap_err();
    assert!(err.contains("2026-10-20"), "{err}");
}

#[test]
fn malformed_slices_refused() {
    let lines: Vec<&str> = ECB_CSV.lines().collect();
    let asc = format!("{}\n{}\n{}\n", lines[0], lines[40], lines[1]);
    assert!(parse_ecb(&asc).unwrap_err().contains("newest first"));
    let no_trailing = format!("{}\n{}\n", lines[0], lines[1].trim_end_matches(','));
    assert!(parse_ecb(&no_trailing).is_err());
    let bad_val = format!(
        "{}\n{}\n",
        lines[0],
        lines[1].replacen("1.1215", "1,1215", 1)
    );
    assert!(parse_ecb(&bad_val).is_err());
    let ul: Vec<&str> = UST_CSV.lines().collect();
    assert!(parse_ust(&format!("{}\n2026-10-20{}\n", ul[0], &ul[1][10..])).is_err());
    assert!(parse_ust("").is_err());
}

#[test]
fn attribution_travels_with_the_data() {
    assert!(NOTICE_MD.contains("Source: ECB statistics."));
    assert!(NOTICE_MD.contains("NOT MIT"));
    assert!(NOTICE_MD.contains(ECB_TERMS_URL));
    for u in UST_TERMS_URLS {
        assert!(NOTICE_MD.contains(u), "{u}");
    }
    let b = Tape::EcbFx.broker();
    let q = b.feed.quote("EURUSD").unwrap();
    assert!(q.delayed);
    assert!(q.source.starts_with("REAL-PUBLIC"));
    assert!(q.source.contains("Source: ECB statistics."));
    for s in b.universe.symbols.values() {
        assert!(s.note.contains("Source: ECB statistics."), "{}", s.ticker);
    }
    let i = info(Tape::EcbFx);
    assert_eq!((i.data_class, i.days, i.delay_bars), ("REAL-PUBLIC", 40, 1));
    assert_eq!(i.attribution, "Source: ECB statistics.");
    let i = info(Tape::Ust);
    assert!(i.source.contains("Not endorsed"));
    assert_eq!(info(Tape::Synthetic).data_class, "SYNTHETIC");
}

#[test]
fn real_calendars_differ_and_are_not_mixed() {
    let fx = Tape::EcbFx.market();
    let ust = Tape::Ust.market();
    // US Labor Day / Columbus Day: ECB fixes, Treasury market closed.
    for d in ["2026-09-07", "2026-10-12"] {
        assert!(fx.days.iter().any(|x| x == d), "{d} in ECB");
        assert!(!ust.days.iter().any(|x| x == d), "{d} not in UST");
    }
    let mut merged = ecb_bars_csv(&ecb_table(), &FX_PAIRS[..1]).unwrap();
    let ub = ust_bars_csv(&ust_table(), &UST_TENORS[..1]).unwrap();
    merged.extend(
        ub.lines()
            .filter(|l| l.contains(",UST"))
            .map(|l| format!("{l}\n")),
    );
    let e = ingest(&merged).unwrap_err();
    assert!(e.contains("calendar not aligned"), "{e}");
}

#[test]
fn ust_proxy_moves_against_yield_every_day() {
    let t = ust_table();
    let md = Tape::Ust.market();
    let mut checked = 0;
    for (tk, col, _) in UST_TENORS {
        let s = md.series(tk).unwrap();
        for w in 1..s.len() {
            let y0 = Dec::parse(t.raw(&s[w - 1].date, col).unwrap())
                .unwrap()
                .mantissa;
            let y1 = Dec::parse(t.raw(&s[w].date, col).unwrap())
                .unwrap()
                .mantissa;
            let (p0, p1) = (s[w - 1].close, s[w].close);
            assert_eq!(
                y1.cmp(&y0),
                p0.cmp(&p1),
                "{tk} {} -> {}",
                s[w - 1].date,
                s[w].date
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 4 * 39);
    let s10 = md.series("UST10Y").unwrap();
    assert_eq!(s10[39].close, zc_price_cents(512, 10));
}

/// Hand-reproducible paper P&L on the real EUR/USD tape.
#[test]
fn paper_broker_replays_ecb_tape() {
    let mut b = Tape::EcbFx.broker();
    assert_eq!(b.today(), "2026-08-26");
    let o = b.place(buy("EURUSD", 100));
    let Outcome::Filled { fill } = o else {
        panic!("{o:?}")
    };
    let px_in = 11669 + apply_bps(11669, 5);
    assert_eq!(fill.price_cents, px_in);
    while !b.feed.at_end() {
        b.advance();
    }
    assert_eq!(b.today(), "2026-10-20");
    assert_eq!(b.feed.quote("EURUSD").unwrap().price, "112.15");
    let o = b.close("EURUSD", "B2B-TEST");
    let Outcome::Filled { fill: out } = o else {
        panic!("{o:?}")
    };
    let px_out = 11215 - apply_bps(11215, 5);
    assert_eq!(out.price_cents, px_out);
    let (n_in, n_out) = (100 * px_in, 100 * px_out);
    let expect = n_out - n_in - apply_bps(n_in, 5) - apply_bps(n_out, 5);
    let a = b.account();
    assert_eq!(a.pnl_cents, expect);
    assert_eq!(a.pnl_cents, -47_744);
    assert_eq!(a.equity_cents - a.starting_cash_cents, a.pnl_cents);
}

#[test]
fn risk_limits_hold_on_real_tapes() {
    let mut b = Tape::EcbFx.broker();
    // 300 lots x 116.69 = 35 007.00 > 25 000.00 max order notional
    let o = b.place(buy("EURUSD", 300));
    assert_eq!(o.code(), "kill_switch_oversized_order");
    assert!(b.kill.engaged);
    assert!(
        b.place(buy("EURUSD", 10)).is_refused(),
        "new risk blocked while engaged"
    );
    assert!(b.reset_kill("test-operator"));
    assert_eq!(b.place(buy("EURUSD", 10)).code(), "filled");
    assert_eq!(b.place(buy("EURGBP", 1)).code(), "restricted_symbol");
    let mut u = Tape::Ust.broker();
    assert_eq!(u.place(buy("UST10Y", 100)).code(), "filled");
    // 1001 x ~21 = ~21 300.00: under max notional, over max position (1000)
    assert_eq!(u.place(buy("UST30Y", 1001)).code(), "max_position");
    assert!(!u.kill.engaged);
}

#[test]
fn real_backtests_are_deterministic_and_pinned() {
    let fx = backtests(Tape::EcbFx);
    assert_eq!(fx.len(), 4);
    let pinned = [
        ("USD", 4, -13_245, "fnv1a64:0ea17fa4f45eaaed"),
        ("GBP", 4, -5_376, "fnv1a64:050a56d418684519"),
        ("CHF", 4, -24_993, "fnv1a64:11bd49f1b903e0d3"),
        ("JPY", 4, -7_665, "fnv1a64:19a03e0446959321"),
    ];
    for (r, (ccy, trades, pnl, fp)) in fx.iter().zip(pinned) {
        assert_eq!(r.data_class, "REAL-PUBLIC");
        assert_eq!(r.account_currency, ccy);
        assert_eq!(r.result.equity_curve.len(), 40);
        assert_eq!(r.result.refusals, 0, "{ccy}");
        assert_eq!(
            (r.result.trades, r.result.pnl_cents),
            (trades, pnl),
            "{ccy}"
        );
        assert_eq!(r.result.fingerprint, fp, "{ccy}");
    }
    let u = backtests(Tape::Ust);
    assert_eq!(u.len(), 1);
    let r = &u[0].result;
    assert_eq!((r.trades, r.refusals, r.pnl_cents), (14, 0, -6_408));
    assert_eq!(r.fingerprint, "fnv1a64:c55c83952c5c599d");
    assert_eq!(r.first_day, "2026-08-24");
    assert_eq!(r.last_day, "2026-10-20");
    // rerun = same fingerprints
    assert_eq!(backtests(Tape::Ust)[0].result.fingerprint, r.fingerprint);
    assert_eq!(
        backtests(Tape::EcbFx)[0].result.fingerprint,
        fx[0].result.fingerprint
    );
}

#[test]
fn synthetic_scoreboard_unchanged() {
    assert_eq!(score::run().percent, 100);
    let r = backtest::reference();
    assert_eq!(r.fingerprint, "fnv1a64:19424da702570a34");
    assert!(Tape::Synthetic
        .broker()
        .feed
        .quote("GLXA")
        .unwrap()
        .source
        .contains("SYNTHETIC"));
}
