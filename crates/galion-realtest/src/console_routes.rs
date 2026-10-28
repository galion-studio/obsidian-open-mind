// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Node-era console API routes -> their Rust owner on the live VPS
//! (slot 384, console-realtests-rust-routes, 2026-10-28 CET +01:00).
//!
//! Since 2026-10-27 19:28 +01:00 the live VPS :8080 is the Rust
//! `galion-console-serve-2d846fb8 --local-api` (D-375-1, F-375-1). Seven
//! console real tests (console-ui `real_port1`, `real_console_ui`;
//! console-static `real_port2..5`, `real_route31`) still read Node-era
//! endpoints from :8080 and fail on the 404s. This module is the single
//! route map those tests consult, plus the switch between the two staged
//! options:
//!
//! * `CONSOLE_LIVE_ROUTES=rust` (option A, default, recommended): a live
//!   read is compared with the room when the live Rust stack answers it;
//!   a 404 on a mapped Node-era route is recorded as "gone on live" and the
//!   room must degrade honestly instead. A 404 on a route the Rust console
//!   serves itself, or on an unmapped route, still fails.
//! * `CONSOLE_LIVE_ROUTES=retire` (option B): every Node-era read (any row
//!   not owned by the live Rust console or desk) is skipped as retired;
//!   only the Rust-owned reads and the box rooms are checked.
//! * `CONSOLE_LIVE_ROUTES=node`: the old strict Node-era asserts (archive;
//!   fails against today's live console).

/// Who answers a Node-era path on the live VPS today (or would after the
/// staged candidate lands).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Live Rust console :8080 (`galion-console-serve-2d846fb8 --local-api`).
    Console,
    /// Live galion-one :8091 (the console's `--pulse`).
    One,
    /// Live hermes-rs :8642 (bearer key needed except `/health`; real tests
    /// never use the key).
    HermesRs,
    /// Live galion-desk :8646.
    Desk,
    /// Not live; served by the staged console candidate 5aa31927
    /// (swarm/console-deploy-candidate 4f763ade) `--local-api` board.
    Candidate,
    /// No Rust provider on any swarm branch (searched 2026-10-28).
    Retired,
}

impl Owner {
    /// VPS loopback port of the owner, if it is a live service.
    pub fn vps_port(self) -> Option<u16> {
        match self {
            Owner::Console | Owner::Candidate | Owner::Retired => Some(8080),
            Owner::One => Some(8091),
            Owner::HermesRs => Some(8642),
            Owner::Desk => Some(8646),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Owner::Console => "rust-console:8080",
            Owner::One => "galion-one:8091",
            Owner::HermesRs => "hermes-rs:8642",
            Owner::Desk => "galion-desk:8646",
            Owner::Candidate => "candidate-5aa31927",
            Owner::Retired => "retired",
        }
    }
}

/// One Node-era read and its Rust equivalent.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    /// Path the Node console served on :8080 (no query).
    pub node: &'static str,
    pub owner: Owner,
    /// Rust equivalent path on `owner` ("" when none).
    pub rust: &'static str,
    /// Status the live owner answered unauthenticated in the read-only
    /// probe of 2026-10-28 11:11 +01:00 (0 = not live).
    pub live: u16,
    pub note: &'static str,
}

const fn r(
    node: &'static str,
    owner: Owner,
    rust: &'static str,
    live: u16,
    note: &'static str,
) -> Route {
    Route {
        node,
        owner,
        rust,
        live,
        note,
    }
}

use Owner::*;

/// Every Node-era console API path the console rooms and real tests read.
pub const MAP: &[Route] = &[
    r(
        "/api/v1/health",
        Console,
        "/api/v1/health",
        200,
        "Rust health; no `target` (pod/publicIp/podId) any more",
    ),
    r(
        "/api/v1/session",
        Console,
        "/api/v1/session",
        401,
        "unpaired 401, same as Node",
    ),
    r(
        "/api/v1/session-pack",
        Console,
        "/api/v1/session-pack",
        401,
        "unpaired 401, same as Node",
    ),
    r(
        "/md/user",
        Console,
        "/md/user",
        404,
        "Rust room; 404 'not configured' without --user-md",
    ),
    r("/desk", Desk, "/desk", 200, "galion-desk, unchanged"),
    r(
        "/api/v1/analytics",
        One,
        "/api/v1/analytics",
        401,
        "galion-one answers {\"error\":\"pair\"}",
    ),
    r(
        "/api/v1/awake",
        One,
        "/api/v1/awake",
        200,
        "galion-one pulse",
    ),
    r(
        "/api/hermes/health",
        HermesRs,
        "/health",
        200,
        "hermes-rs /health (unauthenticated)",
    ),
    r(
        "/api/hermes/models",
        HermesRs,
        "/v1/models",
        401,
        "bearer key; tests never use it",
    ),
    r(
        "/api/hermes/history",
        HermesRs,
        "/api/sessions",
        401,
        "thread list = hermes-rs sessions; bearer key; tests never use it",
    ),
    r(
        "/api/v1/services",
        Candidate,
        "/api/v1/services",
        0,
        "board; galion-one :8091 has a different pulse-only /api/v1/services",
    ),
    r(
        "/api/v1/catalog",
        Candidate,
        "/api/v1/catalog",
        0,
        "board; galion-one :8091 has its own route catalog",
    ),
    r("/api/v1/domains", Candidate, "/api/v1/domains", 0, "board"),
    r("/api/v1/repos", Candidate, "/api/v1/repos", 0, "board"),
    r("/api/v1/ledger", Candidate, "/api/v1/ledger", 0, "board"),
    r("/api/v1/docs", Candidate, "/api/v1/docs", 0, "board"),
    r(
        "/api/v1/protocol",
        Candidate,
        "/api/v1/protocol",
        0,
        "board",
    ),
    r(
        "/api/v1/markers",
        Candidate,
        "/api/v1/markers",
        0,
        "board marker ring (GET; POST held)",
    ),
    r(
        "/api/v1/admin",
        Candidate,
        "/api/v1/admin",
        0,
        "pairing store (--admin-store)",
    ),
    r(
        "/api/arena",
        Candidate,
        "/api/arena",
        0,
        "--arena-root reader",
    ),
    r(
        "/api/v1/bank",
        Retired,
        "",
        0,
        "candidate only proxies it via --data-upstream; no Rust source",
    ),
    r("/api/v1/instances", Retired, "", 0, "Node-only"),
    r("/api/v1/laptop", Retired, "", 0, "Node-only"),
    r(
        "/api/v1/keys",
        Retired,
        "",
        0,
        "Node-only (candidate test asserts not served)",
    ),
    r("/api/v1/security", Retired, "", 0, "Node-only"),
    r("/api/v3/guardian/status", Retired, "", 0, "Node-only"),
    r("/api/v3/guardian/incidents", Retired, "", 0, "Node-only"),
    r("/api/mesh", Retired, "", 0, "Node-only"),
    r(
        "/api/v1/cloudflare",
        Retired,
        "",
        0,
        "Node-only (candidate catalog lists it held)",
    ),
    r("/api/v1/pack", Retired, "", 0, "Node-only"),
    r(
        "/api/hermes/guard",
        Retired,
        "",
        0,
        "no hermes-rs equivalent",
    ),
    r(
        "/api/hermes/modes",
        Retired,
        "",
        0,
        "no hermes-rs equivalent",
    ),
];

/// The map row for `path` (query string ignored).
pub fn lookup(path: &str) -> Option<&'static Route> {
    let p = path.split('?').next().unwrap_or(path);
    MAP.iter().find(|r| r.node == p)
}

/// Which staged option the console real tests run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Option A (default): compare when the live Rust stack answers.
    Rust,
    /// Option B: skip every Node-era read.
    Retire,
    /// Old strict Node-era asserts.
    Node,
}

pub const MODE_VAR: &str = "CONSOLE_LIVE_ROUTES";

pub fn parse_mode(raw: Option<&str>) -> Result<Mode, String> {
    match raw.map(str::trim) {
        None | Some("") | Some("rust") | Some("a") | Some("A") => Ok(Mode::Rust),
        Some("retire") | Some("b") | Some("B") => Ok(Mode::Retire),
        Some("node") => Ok(Mode::Node),
        Some(o) => Err(format!("{MODE_VAR}={o:?}: want rust|retire|node")),
    }
}

/// `parse_mode` on the env; panics naming the var on a bad value.
pub fn mode() -> Mode {
    parse_mode(std::env::var(MODE_VAR).ok().as_deref()).unwrap_or_else(|e| panic!("{e}"))
}

/// What a test should do with one live read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    /// Compare the room with the live answer (old behaviour).
    Compare,
    /// The read is gone on live (A: 404 on a mapped Node-era route) or
    /// retired (B); the room must degrade honestly, no comparison.
    Gone,
}

/// Pure decision: `mode`, Node-era `path`, live status `code`.
/// Errors are real failures (regression on a Rust-owned route, unmapped
/// drift).
pub fn decide_in(mode: Mode, path: &str, code: u16) -> Result<Live, String> {
    if mode == Mode::Node {
        return Ok(Live::Compare);
    }
    let Some(r) = lookup(path) else {
        return Err(format!(
            "{path}: not in console_routes::MAP (unmapped live read, code {code})"
        ));
    };
    let rust_owned = matches!(r.owner, Owner::Console | Owner::Desk);
    match mode {
        Mode::Retire if !rust_owned => Ok(Live::Gone),
        _ if code == 404 && rust_owned && r.live != 404 => Err(format!(
            "{path}: 404 but {} serves it ({})",
            r.owner.label(),
            r.note
        )),
        _ if code == 404 => Ok(Live::Gone),
        _ => Ok(Live::Compare),
    }
}

/// `decide_in(mode(), ..)` that panics on an error and prints one
/// `real: route` line either way.
pub fn decide(path: &str, code: u16) -> Live {
    let m = mode();
    let d = decide_in(m, path, code).unwrap_or_else(|e| panic!("console route map: {e}"));
    let r = lookup(path);
    eprintln!(
        "real: route {path} live {code} mode {m:?} -> {d:?} (owner {}, rust {:?})",
        r.map(|r| r.owner.label()).unwrap_or("-"),
        r.map(|r| r.rust).unwrap_or("")
    );
    d
}

/// True when a rendered room states that a read failed instead of showing
/// data (the honest-degrade check for `Live::Gone`).
pub fn page_says_unavailable(html: &str) -> bool {
    const MARKS: &[&str] = &[
        "did not load",
        "did not answer",
        "not configured",
        "HTTP 404",
        "answered 404",
        "404",
        "not available",
        "unavailable",
        "no /api/",
    ];
    MARKS.iter().any(|m| html.contains(m))
}

/// Text of the console-ui fail-closed gate page (`pages::gate::unavailable`),
/// served 503 when the pairing check `GET /api/v1/admin` does not answer 200.
pub const GATE_CLOSED: &str = "Console API is not answering";

/// The pairing gate on a remote console host, unpaired: with a live
/// `/api/v1/admin` the page asks to pair ("This phone"); while that route
/// is gone on live it fails closed (503, [`GATE_CLOSED`]). Either way no data.
pub fn gate_ok(status: u16, html: &str, admin_gone: bool) -> bool {
    if admin_gone {
        status == 503 && html.contains(GATE_CLOSED)
    } else {
        html.contains("This phone")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_has_unique_paths_and_sane_rows() {
        for (i, a) in MAP.iter().enumerate() {
            assert!(a.node.starts_with('/'), "{}", a.node);
            assert!(!a.node.contains('?'), "{}", a.node);
            for b in &MAP[i + 1..] {
                assert_ne!(a.node, b.node, "duplicate row");
            }
            match a.owner {
                Owner::Retired => assert!(a.rust.is_empty() && a.live == 0, "{}", a.node),
                Owner::Candidate => assert!(!a.rust.is_empty() && a.live == 0, "{}", a.node),
                _ => assert!(!a.rust.is_empty() && a.live != 0, "{}", a.node),
            }
        }
    }

    #[test]
    fn mode_parse() {
        assert_eq!(parse_mode(None), Ok(Mode::Rust));
        assert_eq!(parse_mode(Some("rust")), Ok(Mode::Rust));
        assert_eq!(parse_mode(Some("retire")), Ok(Mode::Retire));
        assert_eq!(parse_mode(Some("node")), Ok(Mode::Node));
        assert!(parse_mode(Some("spa")).is_err());
    }

    #[test]
    fn option_a_compares_live_and_degrades_on_gone() {
        let a = Mode::Rust;
        assert_eq!(decide_in(a, "/api/v1/health", 200), Ok(Live::Compare));
        assert_eq!(decide_in(a, "/api/hermes/history", 404), Ok(Live::Gone));
        assert_eq!(
            decide_in(a, "/api/hermes/history?id=x", 404),
            Ok(Live::Gone)
        );
        assert_eq!(decide_in(a, "/api/v1/markers", 404), Ok(Live::Gone));
        // Comes back after the candidate lands: compared again, no edit.
        assert_eq!(decide_in(a, "/api/v1/markers", 200), Ok(Live::Compare));
        assert_eq!(decide_in(a, "/md/user", 404), Ok(Live::Gone));
        // Regression on a route the live Rust console serves -> error.
        assert!(decide_in(a, "/api/v1/health", 404).is_err());
        assert!(decide_in(a, "/desk", 404).is_err());
        // Unmapped drift -> error.
        assert!(decide_in(a, "/api/v1/new-thing", 404).is_err());
    }

    #[test]
    fn option_b_retires_node_era_reads() {
        let b = Mode::Retire;
        assert_eq!(decide_in(b, "/api/v1/markers", 200), Ok(Live::Gone));
        assert_eq!(decide_in(b, "/api/v1/analytics", 401), Ok(Live::Gone));
        assert_eq!(decide_in(b, "/api/v1/health", 200), Ok(Live::Compare));
        assert!(decide_in(b, "/api/v1/health", 404).is_err());
    }

    #[test]
    fn node_mode_keeps_old_asserts() {
        assert_eq!(
            decide_in(Mode::Node, "/api/hermes/history", 404),
            Ok(Live::Compare)
        );
        assert_eq!(decide_in(Mode::Node, "/unmapped", 404), Ok(Live::Compare));
    }

    #[test]
    fn gate_both_states() {
        assert!(gate_ok(503, "x Console API is not answering x", true));
        assert!(!gate_ok(200, "This phone", true));
        assert!(gate_ok(200, "This phone", false));
        assert!(!gate_ok(503, GATE_CLOSED, false));
    }

    #[test]
    fn owners_have_ports() {
        assert_eq!(Owner::One.vps_port(), Some(8091));
        assert_eq!(Owner::HermesRs.vps_port(), Some(8642));
        assert_eq!(lookup("/api/hermes/health").unwrap().rust, "/health");
    }
}
