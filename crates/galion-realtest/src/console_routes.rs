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
        // Slot 395: `--owner-routes` answers retired rows with an explicit 410;
        // a 410 anywhere else is a regression.
        _ if code == 410 && r.owner != Owner::Retired => Err(format!(
            "{path}: 410 but the map says {} owns it ({})",
            r.owner.label(),
            r.note
        )),
        _ if code == 410 => Ok(Live::Gone),
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
        "HTTP 410",
        "answered 410",
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

/// Where the seven console real tests read "the live console API" from
/// (slot 395, D-384-1 option A prep).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiTarget {
    /// Default: the live VPS Rust console :8080 through the test's `ssh -N`
    /// forward (read-only).
    Vps,
    /// A box-local console build (`galion-console-static --ui rust
    /// --local-api --owner-routes`) started by the test on the API port;
    /// galion-one, hermes-rs and galion-desk stay the live VPS services
    /// through the same read-only forward. Nothing on the VPS changes.
    Local(std::path::PathBuf),
}

/// `CONSOLE_REAL_API`: `vps` (default) or `local`.
pub const API_VAR: &str = "CONSOLE_REAL_API";
/// `CONSOLE_REAL_API_BIN`: console binary for `local` (default: the test's
/// own `galion-console-static` when the crate has it).
pub const API_BIN_VAR: &str = "CONSOLE_REAL_API_BIN";

pub fn parse_api_target(
    raw: Option<&str>,
    bin: Option<&str>,
    default_bin: Option<&str>,
) -> Result<ApiTarget, String> {
    match raw.map(str::trim) {
        None | Some("") | Some("vps") => Ok(ApiTarget::Vps),
        Some("local") => bin
            .filter(|b| !b.trim().is_empty())
            .or(default_bin)
            .map(|b| ApiTarget::Local(b.into()))
            .ok_or_else(|| format!("{API_VAR}=local needs {API_BIN_VAR}=<galion-console-static>")),
        Some(o) => Err(format!("{API_VAR}={o:?}: want vps|local")),
    }
}

/// `parse_api_target` on the env; panics naming the var on a bad value.
pub fn api_target(default_bin: Option<&str>) -> ApiTarget {
    parse_api_target(
        std::env::var(API_VAR).ok().as_deref(),
        std::env::var(API_BIN_VAR).ok().as_deref(),
        default_bin,
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

/// Arguments for the box-local console API (the live :8080 command line
/// `--ui rust --local-api` plus `--owner-routes`). `admin_store` and
/// `arena_root` are throwaway paths (never the live store or arena; slot
/// 413: `--owner-routes` refuses to start without an arena reader);
/// `pulse`, `hermes`, `desk` are the box ends of the read-only forwards.
pub fn local_api_args(
    bind: &str,
    admin_store: &std::path::Path,
    arena_root: &std::path::Path,
    pulse: Option<&str>,
    hermes: Option<&str>,
    desk: Option<&str>,
) -> Vec<String> {
    let mut a: Vec<String> = [
        "--ui",
        "rust",
        "--local-api",
        "--owner-routes",
        "--bind",
        bind,
        "--admin-store",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(admin_store.display().to_string());
    a.push("--arena-root".into());
    a.push(arena_root.display().to_string());
    for (flag, v) in [("--pulse", pulse), ("--hermes", hermes), ("--desk", desk)] {
        if let Some(v) = v {
            a.push(flag.into());
            a.push(v.into());
        }
    }
    a.push("--quiet".into());
    a
}

/// A started box-local console API; killed (and its throwaway admin dir
/// removed) on drop.
pub struct LocalApi {
    child: std::process::Child,
    dir: std::path::PathBuf,
}

impl LocalApi {
    /// Start `bin` with [`local_api_args`] and wait until `GET
    /// /api/v1/health` answers 200 on `bind`.
    pub fn start(
        bin: &std::path::Path,
        bind: &str,
        pulse: Option<&str>,
        hermes: Option<&str>,
        desk: Option<&str>,
    ) -> LocalApi {
        let dir = std::env::temp_dir().join(format!(
            "console-local-api-{}-{}",
            std::process::id(),
            bind.replace([':', '.'], "-")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("local api temp dir");
        let store = dir.join("admin-device.json");
        let arena = dir.join("arena");
        std::fs::create_dir_all(&arena).expect("local api arena dir");
        let args = local_api_args(bind, &store, &arena, pulse, hermes, desk);
        let child = std::process::Command::new(bin)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("start local console {}: {e}", bin.display()));
        let api = LocalApi { child, dir };
        let t0 = std::time::Instant::now();
        loop {
            if health_ok(bind) {
                break;
            }
            assert!(
                t0.elapsed() < std::time::Duration::from_secs(15),
                "local console API on {bind} did not come up"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        eprintln!(
            "real: local console API {} {} (CONSOLE_REAL_API=local)",
            bin.display(),
            args.join(" ")
        );
        api
    }
}

fn health_ok(bind: &str) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect(bind) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let _ = write!(
        s,
        "GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    );
    let mut b = Vec::new();
    let _ = s.read_to_end(&mut b);
    b.starts_with(b"HTTP/1.1 200")
}

impl Drop for LocalApi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
        eprintln!("real: stopped local console API");
    }
}

/// `ssh -L` arguments for the console API: the live :8080 for
/// [`ApiTarget::Vps`], nothing for [`ApiTarget::Local`] (the local build
/// binds `api_at` itself).
pub fn api_forward_args(target: &ApiTarget, api_at: &str) -> Vec<String> {
    match target {
        ApiTarget::Vps => vec!["-L".into(), format!("{api_at}:127.0.0.1:8080")],
        ApiTarget::Local(_) => Vec::new(),
    }
}

/// What answers the console API port, for the `real: forward` log line
/// (F-395-4, slot 413): `ADDR->8080` only when the port really is the ssh
/// forward of the live :8080; a local build names itself instead.
pub fn api_forward_label(target: &ApiTarget, api_at: &str) -> String {
    match target {
        ApiTarget::Vps => format!("{api_at}->8080"),
        ApiTarget::Local(bin) => format!(
            "{api_at}=local {} ({API_VAR}=local)",
            bin.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bin.display().to_string())
        ),
    }
}

/// Extra `ssh -L` arguments the local build needs for its owner rows
/// (galion-one :8091, hermes-rs :8642). `None` = the test forwards it
/// already (or does not need it). Empty for [`ApiTarget::Vps`].
pub fn owner_forward_args(
    target: &ApiTarget,
    pulse_at: Option<&str>,
    hermes_at: Option<&str>,
) -> Vec<String> {
    let mut a = Vec::new();
    if let ApiTarget::Local(_) = target {
        if let Some(p) = pulse_at {
            a.push("-L".into());
            a.push(format!("{p}:127.0.0.1:8091"));
        }
        if let Some(h) = hermes_at {
            a.push("-L".into());
            a.push(format!("{h}:127.0.0.1:8642"));
        }
    }
    a
}

/// For [`ApiTarget::Local`]: wait until the ssh forward listens on
/// `probe_at` (any forwarded port), then start the local console API on
/// `api_at` with the owner forwards. `None` for [`ApiTarget::Vps`].
pub fn start_local_api(
    target: &ApiTarget,
    api_at: &str,
    probe_at: &str,
    pulse_at: Option<&str>,
    hermes_at: Option<&str>,
    desk_at: Option<&str>,
) -> Option<LocalApi> {
    let ApiTarget::Local(bin) = target else {
        return None;
    };
    let t0 = std::time::Instant::now();
    while std::net::TcpStream::connect(probe_at).is_err() {
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(25),
            "ssh forward {probe_at} did not come up"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Some(LocalApi::start(bin, api_at, pulse_at, hermes_at, desk_at))
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
        // Slot 395: explicit 410 only for retired rows.
        assert_eq!(decide_in(a, "/api/mesh", 410), Ok(Live::Gone));
        assert_eq!(decide_in(a, "/api/v1/keys?x=1", 410), Ok(Live::Gone));
        assert!(decide_in(a, "/api/v1/markers", 410).is_err());
        assert!(decide_in(a, "/api/v1/health", 410).is_err());
        assert!(decide_in(a, "/api/v1/analytics", 410).is_err());
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
    fn api_target_parse() {
        assert_eq!(parse_api_target(None, None, None), Ok(ApiTarget::Vps));
        assert_eq!(
            parse_api_target(Some("vps"), Some("/x"), None),
            Ok(ApiTarget::Vps)
        );
        assert_eq!(
            parse_api_target(Some("local"), None, Some("/own")),
            Ok(ApiTarget::Local("/own".into()))
        );
        assert_eq!(
            parse_api_target(Some("local"), Some("/b"), Some("/own")),
            Ok(ApiTarget::Local("/b".into()))
        );
        assert!(parse_api_target(Some("local"), None, None).is_err());
        assert!(parse_api_target(Some("live"), None, None).is_err());
    }

    #[test]
    fn local_api_args_shape() {
        let a = local_api_args(
            "127.0.0.1:1",
            std::path::Path::new("/tmp/s.json"),
            std::path::Path::new("/tmp/arena"),
            Some("127.0.0.1:2"),
            None,
            Some("127.0.0.1:3"),
        );
        let s = a.join(" ");
        assert!(s.starts_with(
            "--ui rust --local-api --owner-routes --bind 127.0.0.1:1 --admin-store /tmp/s.json --arena-root /tmp/arena"
        ));
        assert!(s.contains("--pulse 127.0.0.1:2") && s.contains("--desk 127.0.0.1:3"));
        assert!(!s.contains("--hermes"));
    }

    #[test]
    fn forward_label_names_real_upstream() {
        assert_eq!(
            api_forward_label(&ApiTarget::Vps, "127.0.0.1:22830"),
            "127.0.0.1:22830->8080"
        );
        let l = api_forward_label(
            &ApiTarget::Local("/t/release/galion-console-static".into()),
            "127.0.0.1:22830",
        );
        assert_eq!(
            l,
            "127.0.0.1:22830=local galion-console-static (CONSOLE_REAL_API=local)"
        );
        assert!(!l.contains("8080"), "F-395-4: {l}");
    }

    #[test]
    fn owners_have_ports() {
        assert_eq!(Owner::One.vps_port(), Some(8091));
        assert_eq!(Owner::HermesRs.vps_port(), Some(8642));
        assert_eq!(lookup("/api/hermes/health").unwrap().rust, "/health");
    }
}
