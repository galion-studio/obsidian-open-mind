// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Accounts, sessions, rate limiting, CSRF, cash vouchers and the internal
//! entitlement API through the HTTP handler.

mod common;

use common::*;
use serde_json::json;

const DAY: i64 = 86_400;

fn internal(
    t: &TestApp,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> galion_common::http::Response {
    let auth = format!("Bearer {token}");
    t.app.handle_internal(&req(
        "POST",
        path,
        &[
            ("Authorization", &auth),
            ("Content-Type", "application/json"),
        ],
        body.to_string().as_bytes(),
    ))
}

#[test]
fn page_is_served_with_strict_headers() {
    let t = app(false, false, mock_transport());
    let r = t.app.handle(&req("GET", "/account", &[], b""));
    assert_eq!(r.status, 200);
    let csp = header(&r, "content-security-policy").unwrap();
    assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"));
    let html = String::from_utf8_lossy(&r.body);
    assert!(html.contains("/account/account.js") && !html.contains("<script>"));
    assert!(!html.contains("style=\""), "no inline styles under the CSP");
    for p in ["/account/account.css", "/account/account.js"] {
        assert_eq!(t.app.handle(&req("GET", p, &[], b"")).status, 200, "{p}");
    }
    let plans = json(&t.app.handle(&req("GET", "/api/account/plans", &[], b"")));
    let cents: Vec<i64> = plans["plans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["eur_cents"].as_i64().unwrap())
        .collect();
    assert_eq!(cents, vec![0, 2000, 22000]);
}

#[test]
fn create_login_logout_and_cookie() {
    let t = app(false, false, mock_transport());
    let r = t.app.handle(&post_json("/api/account/create", None, "{}"));
    assert_eq!(r.status, 200);
    let number = json(&r)["number"].as_str().unwrap().to_string();
    // Shown grouped: 4 groups of 4 digits, first digit not 0.
    let groups: Vec<&str> = number.split(' ').collect();
    assert_eq!(groups.len(), 4);
    assert!(groups
        .iter()
        .all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_digit())));
    assert!(!number.starts_with('0'));
    let sc = header(&r, "set-cookie").unwrap();
    for part in [
        "galion_acct=",
        "HttpOnly",
        "SameSite=Lax",
        "Secure",
        "Path=/",
    ] {
        assert!(sc.contains(part), "{sc}");
    }
    let cookie = cookie_of(&r);
    let me = json(
        &t.app
            .handle(&req("GET", "/api/account/me", &[("Cookie", &cookie)], b"")),
    );
    assert_eq!(me["plan"], "free");
    assert_eq!(me["active"], false);

    // The number is the login, with or without separators.
    for form in [
        number.clone(),
        number.replace(' ', ""),
        number.replace(' ', "-"),
    ] {
        let li = t.app.handle(&post_json(
            "/api/account/login",
            None,
            &json!({ "number": form }).to_string(),
        ));
        assert_eq!(li.status, 200, "{form}");
    }
    // The DB never holds the digits.
    let digits = number.replace(' ', "");
    assert!(t
        .app
        .store
        .debug_dump()
        .iter()
        .all(|row| !row.contains(&digits)));

    // Logout ends the server-side session.
    let lo = t
        .app
        .handle(&post_json("/api/account/logout", Some(&cookie), "{}"));
    assert!(header(&lo, "set-cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(
        t.app
            .handle(&req("GET", "/api/account/me", &[("Cookie", &cookie)], b""))
            .status,
        401
    );
}

#[test]
fn login_lockout_and_csrf() {
    let t = app(false, false, mock_transport());
    for i in 0..10 {
        let r = t.app.handle(&post_json(
            "/api/account/login",
            None,
            r#"{"number":"1234 5678 9012 3456"}"#,
        ));
        assert_eq!(r.status, 401, "attempt {i}");
    }
    let r = t.app.handle(&post_json(
        "/api/account/login",
        None,
        r#"{"number":"1234 5678 9012 3456"}"#,
    ));
    assert_eq!(r.status, 429);
    assert!(header(&r, "retry-after").is_some());
    // The lockout lifts after 15 minutes.
    t.set_now(T0 + 15 * 60 + 1);
    let r = t.app.handle(&post_json(
        "/api/account/login",
        None,
        r#"{"number":"1234 5678 9012 3456"}"#,
    ));
    assert_eq!(r.status, 401);

    // CSRF: form posts and foreign origins are refused.
    let form = req(
        "POST",
        "/api/account/create",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        b"x=1",
    );
    assert_eq!(t.app.handle(&form).status, 415);
    let foreign = req(
        "POST",
        "/api/account/create",
        &[
            ("Content-Type", "application/json"),
            ("Origin", "https://evil.example"),
        ],
        b"{}",
    );
    assert_eq!(t.app.handle(&foreign).status, 415);
}

#[test]
fn cash_voucher_flow() {
    // Disabled without a mailing address.
    let off = app(false, false, mock_transport());
    let c = cookie_of(
        &off.app
            .handle(&post_json("/api/account/create", None, "{}")),
    );
    let r = off
        .app
        .handle(&post_json("/api/pay/cash", Some(&c), r#"{"plan":"plus"}"#));
    assert_eq!(r.status, 503);

    let t = app(false, true, mock_transport());
    let cr = t.app.handle(&post_json("/api/account/create", None, "{}"));
    let cookie = cookie_of(&cr);
    let number = json(&cr)["number"].as_str().unwrap().to_string();
    assert_eq!(
        t.app
            .handle(&post_json(
                "/api/pay/cash",
                Some(&cookie),
                r#"{"plan":"free"}"#
            ))
            .status,
        400
    );
    let r = t.app.handle(&post_json(
        "/api/pay/cash",
        Some(&cookie),
        r#"{"plan":"plus"}"#,
    ));
    assert_eq!(r.status, 200);
    let v = json(&r);
    let reference = v["reference"].as_str().unwrap().to_string();
    assert!(reference.starts_with("GC-") && reference.len() == 17);
    let text = v["instructions"].as_str().unwrap();
    assert!(
        text.contains("€20")
            && text.contains("Zagreb")
            && text.contains("Do not write your account number")
    );
    assert!(!text.contains(&number));

    // Operator credit: needs the admin token (not the internal one).
    let body =
        json!({"reference": reference.to_lowercase().replace('-', " "), "amount_cents": 4000});
    assert_eq!(
        internal(
            &t,
            "/internal/v1/cash/credit",
            "internal-test-token",
            body.clone()
        )
        .status,
        401
    );
    let ok = internal(
        &t,
        "/internal/v1/cash/credit",
        "admin-test-token",
        body.clone(),
    );
    assert_eq!(ok.status, 200, "{}", String::from_utf8_lossy(&ok.body));
    assert_eq!(json(&ok)["days_added"], 60, "€40 of Plus = 60 days");
    // Used once only.
    assert_eq!(
        internal(&t, "/internal/v1/cash/credit", "admin-test-token", body).status,
        404
    );
    // Non-EUR is refused for a human decision.
    let r2 = json(&t.app.handle(&post_json(
        "/api/pay/cash",
        Some(&cookie),
        r#"{"plan":"plus"}"#,
    )));
    let usd = internal(
        &t,
        "/internal/v1/cash/credit",
        "admin-test-token",
        json!({"reference": r2["reference"], "amount_cents": 2000, "currency": "usd"}),
    );
    assert_eq!(json(&usd)["error"], "currency");

    let me = json(
        &t.app
            .handle(&req("GET", "/api/account/me", &[("Cookie", &cookie)], b"")),
    );
    assert_eq!(me["plan"], "plus");
    assert_eq!(me["active"], true);
    assert_eq!(me["paid_until"], T0 + 60 * DAY);
    assert_eq!(me["renewing"], false);
    assert!(me["paid_until_label"].as_str().unwrap().contains("2025"));
    // Payment metadata is gone: the used reference no longer points anywhere.
    assert!(t.app.store.peek_pay_ref(&reference, T0).unwrap().is_none());
    assert_eq!(t.app.store.ledger_count().unwrap(), 1);

    // After the paid time the account is Free again.
    t.set_now(T0 + 60 * DAY + 1);
    let e = internal(
        &t,
        "/internal/v1/entitlement",
        "internal-test-token",
        json!({ "number": number }),
    );
    assert_eq!(json(&e)["plan"], "free");
    assert_eq!(json(&e)["active"], false);
}

#[test]
fn entitlement_api_auth_and_unknowns() {
    let t = app(false, false, mock_transport());
    let cr = t.app.handle(&post_json("/api/account/create", None, "{}"));
    let number = json(&cr)["number"].as_str().unwrap().to_string();
    let session = cookie_of(&cr).split_once('=').unwrap().1.to_string();
    assert_eq!(
        internal(
            &t,
            "/internal/v1/entitlement",
            "wrong",
            json!({ "session": session })
        )
        .status,
        401
    );
    let no_auth = t
        .app
        .handle_internal(&req("POST", "/internal/v1/entitlement", &[], b"{}"));
    assert_eq!(no_auth.status, 401);
    let ok = json(&internal(
        &t,
        "/internal/v1/entitlement",
        "internal-test-token",
        json!({ "session": session }),
    ));
    assert_eq!(ok["plan"], "free");
    assert_eq!(ok["ok"], true);
    let byn = json(&internal(
        &t,
        "/internal/v1/entitlement",
        "internal-test-token",
        json!({ "number": number }),
    ));
    assert_eq!(byn["ok"], true);
    assert!(byn.get("known").is_none());
    let unknown = json(&internal(
        &t,
        "/internal/v1/entitlement",
        "internal-test-token",
        json!({"session": "nope"}),
    ));
    assert_eq!(unknown["known"], false);
    // The internal routes are not on the public handler.
    assert_eq!(
        t.app
            .handle(&req("POST", "/internal/v1/entitlement", &[], b"{}"))
            .status,
        404
    );
    let h = t.app.handle_internal(&req(
        "GET",
        "/internal/v1/health",
        &[("Authorization", "Bearer internal-test-token")],
        b"",
    ));
    assert_eq!(json(&h)["accounts"], 1);
}
