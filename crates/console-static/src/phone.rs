// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--phone` (console-phone-home, 2026-10-31 CET +01:00): the Android admin
//! first screen, one phone-sized page inside the console.
//!
//! - `GET /phone`: the page. It holds no data (like galion-one's
//!   `/approvals`); everything below comes from gated API calls.
//! - `GET /api/v1/phone/status`: read-only JSON for the top tiles: Hermes
//!   up/down + build id, OpenRouter fresh spend vs the limit (from the
//!   OpenMind no-secrets status snapshot), watchdog (gaia-cortex) alerts
//!   waiting for a decision, health of cortex / nerve / console.
//! - `/api/approvals*`, `/api/owner-key*`: galion-one's approvals module run
//!   in-process ([`galion_one::approvals::route`]), so the cards, the
//!   passkey / key-file signing and the checks are the same code as on
//!   `/approvals`.
//!
//! Every API route here is reached only after [`crate::api::try_handle`]
//! passed the console gate (console host + loopback peer or a paired admin
//! cookie); unpaired phones get its 401. Nothing here restarts or writes
//! anything except what the approvals module itself writes (signed approvals
//! and the staged owner key, as on galion-one).

use galion_one::approvals;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The page path.
pub const PAGE_PATH: &str = "/phone";
/// The status JSON path.
pub const STATUS_PATH: &str = "/api/v1/phone/status";
/// A snapshot older than this is shown as old (the publish timer runs every
/// few minutes; same bound as openmind-mcp-stack `STATUS_STALE_S`).
pub const STALE_S: u64 = 30 * 60;
/// Largest status / unit file read.
const MAX_FILE: u64 = 64 * 1024;
/// Most watchdog items listed on the page.
pub const MAX_ITEMS: usize = 5;
/// Longest item text.
const MAX_TEXT: usize = 140;

/// Spend defaults (owner rulings): $10 limit, $9.50 stop line, $3.449 baseline.
pub const LIMIT_USD: f64 = 10.0;
pub const STOP_LINE_USD: f64 = 9.5;
pub const BASELINE_USD: f64 = 3.449;

/// `--phone` settings. Defaults are the server layout; tests point every
/// path and address at fakes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneCfg {
    /// `--nerve`: openmind-nerve (`/health`, `/api/v1/state`).
    pub nerve: SocketAddr,
    /// `--cortex`: gaia-cortex watchdog (`/health`, `/escalations`, `/approvals`).
    pub cortex: SocketAddr,
    /// `--hermes` (shared flag): hermes-rs (`/health`).
    pub hermes: SocketAddr,
    /// `--status-dir`: OpenMind status snapshots (`spend.json`, `backup.json`).
    pub status_dir: PathBuf,
    /// `--unit-dir` (repeatable): systemd unit dirs for the Hermes build id.
    pub unit_dirs: Vec<PathBuf>,
    /// `--hermes-unit`: the Hermes unit name.
    pub hermes_unit: String,
    /// Approvals (same meaning as galion-one's `ONE_APPROVAL_*`).
    pub queue: PathBuf,
    pub state: PathBuf,
    pub out: Option<PathBuf>,
    pub owner_pub: PathBuf,
    pub sandbox_pub: PathBuf,
    pub stack_pub: PathBuf,
    pub rp_id: String,
    pub origins: Vec<String>,
}

impl PhoneCfg {
    pub fn defaults() -> PhoneCfg {
        let a = approvals::Cfg::defaults();
        let mut origins = vec!["https://console.galion.app".to_string()];
        origins.extend(a.origins);
        PhoneCfg {
            nerve: "127.0.0.1:8670".parse().expect("nerve default"),
            cortex: "127.0.0.1:8674".parse().expect("cortex default"),
            hermes: "127.0.0.1:8642".parse().expect("hermes default"),
            status_dir: "/var/lib/openmind-mcp-stack/status".into(),
            unit_dirs: default_unit_dirs(),
            hermes_unit: "hermes-rs.service".into(),
            queue: a.queue,
            state: a.state,
            out: a.out,
            owner_pub: a.owner_pub,
            sandbox_pub: a.sandbox_pub,
            stack_pub: a.stack_pub,
            rp_id: a.rp_id,
            origins,
        }
    }

    /// The approvals module's view of these settings.
    pub fn approvals(&self) -> approvals::Cfg {
        approvals::Cfg {
            queue: self.queue.clone(),
            state: self.state.clone(),
            out: self.out.clone(),
            owner_pub: self.owner_pub.clone(),
            sandbox_pub: self.sandbox_pub.clone(),
            stack_pub: self.stack_pub.clone(),
            rp_id: self.rp_id.clone(),
            origins: self.origins.clone(),
        }
    }
}

/// systemd search order (earlier wins for the same file name).
pub fn default_unit_dirs() -> Vec<PathBuf> {
    [
        "/etc/systemd/system",
        "/run/systemd/system",
        "/usr/lib/systemd/system",
        "/lib/systemd/system",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

/// API paths this module answers (after the gate).
pub fn owns_api(path: &str) -> bool {
    path == STATUS_PATH || path.starts_with("/api/approvals") || path.starts_with("/api/owner-key")
}

/// POSTs whose body this module reads.
pub fn wants_body(method: &str, path: &str) -> bool {
    method == "POST" && path != STATUS_PATH && owns_api(path)
}

/// One answer.
pub struct Out {
    pub status: u16,
    pub kind: &'static str,
    pub body: Vec<u8>,
}

fn json_out(status: u16, v: &Value) -> Out {
    Out {
        status,
        kind: "application/json",
        body: v.to_string().into_bytes(),
    }
}

/// Unix seconds now.
pub fn now_secs() -> i64 {
    SystemTime::now() // repo-check: allow (unix epoch number for the approvals clock and ages; shown in Europe/Zagreb by the page)
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Answer an owned API path. `origin` is the request's Origin header.
pub fn api(cfg: &PhoneCfg, method: &str, path: &str, body: &[u8], origin: &str) -> Out {
    let now = now_secs();
    if path == STATUS_PATH {
        return match method {
            "GET" | "HEAD" => json_out(200, &status(cfg, now)),
            _ => json_out(405, &json!({"ok": false, "error": "use GET"})),
        };
    }
    let r = approvals::route(
        &cfg.approvals(),
        &approvals::Req {
            method,
            path,
            body,
            origin,
            // The console gate already passed (host + loopback or paired cookie).
            paired: true,
            now,
        },
    );
    Out {
        status: r.status,
        kind: r.kind,
        body: r.body,
    }
}

// ---------------------------------------------------------------- page

/// The `/phone` page: galion-one's approvals screen, retitled for the
/// console, with the status tiles above the cards.
pub fn page() -> &'static str {
    static PAGE: OnceLock<String> = OnceLock::new();
    PAGE.get_or_init(build_page)
}

/// Content-Security-Policy for the page (inline sheet + script only, same-origin fetches).
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// (needle in the approvals markup, replacement). Each must match once.
pub const EDITS: &[(&str, &str)] = &[
    (
        r#"<main class="hm ap" id="hm" aria-label="Approvals">"#,
        r#"<main class="hm ap ph" id="hm" aria-label="Console phone home">"#,
    ),
    (
        r#"<p class="k">Galion One · Approvals</p><h1>Hermes self-changes</h1>"#,
        r#"<p class="k">Galion console · Phone</p><h1>Today at a glance</h1>"#,
    ),
    (
        r#"href="/">Back to the mind</a>"#,
        r#"href="/console">Full console</a>"#,
    ),
    (
        r#"This device is not paired. <a href="/">Open Galion One</a> and pair first."#,
        r#"This phone is not paired with the console. <a href="/console">Open the console</a> on this phone, pair it with the admin key, then come back here."#,
    ),
    (
        r#"  <p class="ap-intro">"#,
        concat!(
            include_str!("../assets/phone/status.html"),
            "  <p class=\"ap-intro\">"
        ),
    ),
];

fn build_page() -> String {
    let mut body = approvals::parts::BODY.to_string();
    for (from, to) in EDITS {
        body = body.replacen(from, to, 1);
    }
    let mut s = String::with_capacity(body.len() + approvals::parts::STYLE.len() + 16 * 1024);
    s.push_str(
        "<!doctype html>\n<html lang=\"en\" class=\"g1v2\">\n<head>\n<meta charset=\"utf-8\">\n",
    );
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n");
    s.push_str("<meta name=\"theme-color\" content=\"#05060f\">\n<meta name=\"color-scheme\" content=\"dark\">\n");
    s.push_str("<meta name=\"referrer\" content=\"no-referrer\">\n<title>Phone · Galion console</title>\n<style>\n");
    s.push_str(approvals::parts::STYLE);
    s.push_str(include_str!("../assets/phone/phone.css"));
    s.push_str("</style>\n</head>\n<body>\n");
    s.push_str(&body);
    s.push_str("<script>\n");
    s.push_str(approvals::parts::SCRIPT);
    s.push_str("</script>\n<script>\n");
    s.push_str(include_str!("../assets/phone/phone.js"));
    s.push_str("</script>\n</body>\n</html>\n");
    s
}

// ---------------------------------------------------------------- status

/// The status document. Probes run in parallel; each has the client's
/// own connect / read budget, so a dead service costs at most a few seconds.
pub fn status(cfg: &PhoneCfg, now: i64) -> Value {
    let (h_health, n_health, n_state, c_health, c_esc, c_appr, build, ready) =
        std::thread::scope(|s| {
            let a = s.spawn(|| get_json(cfg.hermes, "/health"));
            let b = s.spawn(|| get_json(cfg.nerve, "/health"));
            let c = s.spawn(|| get_json(cfg.nerve, "/api/v1/state"));
            let d = s.spawn(|| get_json(cfg.cortex, "/health"));
            let e = s.spawn(|| get_json(cfg.cortex, "/escalations"));
            let f = s.spawn(|| get_json(cfg.cortex, "/approvals"));
            let g = s.spawn(|| hermes_build(&cfg.unit_dirs, &cfg.hermes_unit));
            let r = s.spawn(|| approvals::count(&cfg.approvals(), now));
            let j = |h: std::thread::ScopedJoinHandle<'_, Probe>| {
                h.join().unwrap_or(Err("probe failed".into()))
            };
            (
                j(a),
                j(b),
                j(c),
                j(d),
                j(e),
                j(f),
                g.join().unwrap_or(None),
                r.join().unwrap_or(Value::Null),
            )
        });
    let components = n_state
        .as_ref()
        .ok()
        .and_then(|(code, v)| (*code == 200).then_some(v))
        .and_then(|v| v["components"].as_array().cloned())
        .unwrap_or_default();
    let comp = |name: &str| -> Option<String> {
        components
            .iter()
            .find(|c| c["name"] == name)
            .and_then(|c| c["status"].as_str())
            .map(str::to_string)
    };
    let watchdog = watchdog(&c_health, &c_esc, &c_appr, now);
    json!({
        "ok": true,
        "now": now,
        "hermes": hermes(&h_health, comp("hermes-rs"), build),
        "spend": spend(&cfg.status_dir.join("spend.json"), now),
        "backup": backup(&cfg.status_dir.join("backup.json"), now),
        "watchdog": watchdog,
        "health": [
            cortex_row(&c_health),
            nerve_row(&n_health),
            console_row(comp("console")),
        ],
        "approvals": ready,
    })
}

type Probe = Result<(u16, Value), String>;

fn get_json(addr: SocketAddr, path: &str) -> Probe {
    let fwd = console_ui::client::Forward {
        host: addr.to_string(),
        ..Default::default()
    };
    let r = console_ui::client::call(addr, "GET", path, &fwd, None).map_err(|e| short_io(&e))?;
    let v = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    Ok((r.status, v))
}

/// An IO error as a short word (no addresses).
fn short_io(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        ConnectionRefused => "not listening".into(),
        TimedOut | WouldBlock => "timed out".into(),
        ConnectionReset | ConnectionAborted | UnexpectedEof => "connection dropped".into(),
        _ => "unreachable".into(),
    }
}

fn ok_json(p: &Probe) -> Option<&Value> {
    match p {
        Ok((200, v)) => Some(v),
        _ => None,
    }
}

fn probe_error(p: &Probe) -> String {
    match p {
        Ok((code, _)) => format!("answered HTTP {code}"),
        Err(e) => e.clone(),
    }
}

fn clip(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() <= MAX_TEXT {
        s
    } else {
        let mut t: String = s.chars().take(MAX_TEXT - 1).collect();
        t.push('…');
        t
    }
}

fn hermes(h: &Probe, nerve: Option<String>, build: Option<Build>) -> Value {
    let up = ok_json(h).is_some_and(|v| v["status"] == "ok");
    let mut o = Map::new();
    o.insert("up".into(), up.into());
    if let Some(v) = ok_json(h) {
        if let Some(s) = v["implementation_version"]
            .as_str()
            .or(v["version"].as_str())
        {
            o.insert("version".into(), clip(s).into());
        }
    }
    if !up {
        o.insert("error".into(), probe_error(h).into());
    }
    o.insert(
        "nerve".into(),
        nerve.map(Value::from).unwrap_or(Value::Null),
    );
    match build {
        Some(b) => {
            o.insert("build".into(), b.binary.into());
            o.insert(
                "commit".into(),
                b.commit.map(Value::from).unwrap_or(Value::Null),
            );
            o.insert("build_from".into(), b.from.into());
        }
        None => {
            o.insert("build".into(), Value::Null);
        }
    }
    Value::Object(o)
}

fn watchdog(health: &Probe, esc: &Probe, appr: &Probe, now: i64) -> Value {
    let Some(h) = ok_json(health) else {
        return json!({"ok": false, "error": format!("watchdog {}", probe_error(health))});
    };
    let mut items: Vec<(i64, Value)> = Vec::new();
    let open = ok_json(esc)
        .and_then(|v| v["open"].as_array().cloned())
        .unwrap_or_default();
    let esc_open = h["escalations_open"].as_u64().unwrap_or(open.len() as u64);
    for e in &open {
        let lead = &e["lead_event"];
        let ts = lead["t"].as_i64().unwrap_or(0);
        let what = e["reason"]
            .as_str()
            .or(lead["msg"].as_str())
            .or(e["incident"].as_str())
            .unwrap_or("escalation");
        items.push((
            ts,
            json!({
                "kind": "escalation",
                "id": clip(e["id"].as_str().unwrap_or("")),
                "what": clip(what),
                "unit": lead["unit"].as_str().map(clip),
                "severity": lead["severity"].as_str().map(clip),
                "handoff": e["handoff"].as_str().map(clip),
                "ts": (ts > 0).then_some(ts),
            }),
        ));
    }
    // Cortex action approvals: newer cortex only (older answers 404).
    let pending = ok_json(appr).and_then(|v| v["pending"].as_array().cloned());
    if let Some(p) = &pending {
        for a in p {
            let ts = a["ts"].as_i64().unwrap_or(now);
            let what = format!(
                "{} {}",
                a["action"].as_str().unwrap_or("action"),
                a["unit"].as_str().unwrap_or("")
            );
            items.push((
                ts,
                json!({
                    "kind": "action",
                    "id": clip(a["id"].as_str().unwrap_or("")),
                    "what": clip(what.trim()),
                    "unit": a["unit"].as_str().map(clip),
                    "severity": a["rule"].as_str().map(clip),
                    "reason": a["reason"].as_str().map(clip),
                    "ts": ts,
                }),
            ));
        }
    }
    // Actions first (they expire), then newest escalations.
    items.sort_by(|a, b| {
        let ka = a.1["kind"] == "action";
        let kb = b.1["kind"] == "action";
        kb.cmp(&ka).then(b.0.cmp(&a.0))
    });
    let actions = pending.as_ref().map(|p| p.len() as u64);
    let awaiting = esc_open + actions.unwrap_or(0);
    json!({
        "ok": true,
        "mode": h["mode"].as_str().map(clip),
        "escalations_open": esc_open,
        "actions_pending": actions,
        "incidents_open": h["incidents_open"].as_u64(),
        "awaiting": awaiting,
        "items": items.into_iter().take(MAX_ITEMS).map(|(_, v)| v).collect::<Vec<_>>(),
    })
}

fn cortex_row(h: &Probe) -> Value {
    match ok_json(h) {
        Some(v) => {
            let ok = v["ok"] == true;
            let mut d = Vec::new();
            if let Some(n) = v["incidents_open"].as_u64() {
                d.push(format!(
                    "{n} incident{} open",
                    if n == 1 { "" } else { "s" }
                ));
            }
            if let Some(c) = v["commit"].as_str() {
                d.push(format!("build {}", &c[..c.len().min(8)]));
            }
            json!({"name": "cortex", "ok": ok,
                "status": v["mode"].as_str().map(clip).unwrap_or_else(|| if ok {"up".into()} else {"not ok".into()}),
                "detail": d.join(" · ")})
        }
        None => json!({"name": "cortex", "ok": false, "status": "down", "detail": probe_error(h)}),
    }
}

fn nerve_row(h: &Probe) -> Value {
    match ok_json(h) {
        Some(v) => {
            let ok = v["status"] == "ok" && v["state_fresh"] != false;
            let mut d = Vec::new();
            if let Some(s) = v["system"].as_str() {
                d.push(format!("system {}", clip(s)));
            }
            if let Some(s) = v["score"].as_u64() {
                d.push(format!("score {s}"));
            }
            if let Some(a) = v["state_age_s"].as_u64() {
                d.push(format!("state {} min old", a / 60));
            }
            json!({"name": "nerve", "ok": ok,
                "status": if ok { "up" } else { "not ok" },
                "detail": d.join(" · ")})
        }
        None => json!({"name": "nerve", "ok": false, "status": "down", "detail": probe_error(h)}),
    }
}

fn console_row(nerve_view: Option<String>) -> Value {
    // This answer itself proves the console is up; nerve adds its view.
    let detail = match &nerve_view {
        Some(s) => format!("nerve sees: {s}"),
        None => "nerve view unavailable".into(),
    };
    json!({"name": "console", "ok": nerve_view.as_deref().is_none_or(|s| s == "healthy"),
        "status": "answering", "detail": detail})
}

// ---------------------------------------------------------------- snapshots

/// Read a small regular file (no symlinks), with its age in seconds.
fn read_small(p: &Path) -> Result<(Vec<u8>, u64), String> {
    let m = std::fs::symlink_metadata(p).map_err(|e| file_err(&e))?;
    if !m.file_type().is_file() {
        return Err("not a regular file".into());
    }
    if m.len() > MAX_FILE {
        return Err("too large".into());
    }
    let age = m
        .modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok()) // repo-check: allow (file age in seconds only)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let mut buf = Vec::new();
    std::fs::File::open(p)
        .map_err(|e| file_err(&e))?
        .take(MAX_FILE)
        .read_to_end(&mut buf)
        .map_err(|e| file_err(&e))?;
    Ok((buf, age))
}

fn file_err(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no snapshot yet".into(),
        std::io::ErrorKind::PermissionDenied => "snapshot not readable (permission)".into(),
        _ => "snapshot unreadable".into(),
    }
}

/// `{"snapshot_of": kind, "written": ..., "data": {...}}` → (data, written, age).
fn snapshot(p: &Path, kind: &str) -> Result<(Value, Value, u64), String> {
    let (buf, age) = read_small(p)?;
    let v: Value = serde_json::from_slice(&buf).map_err(|_| "snapshot is not JSON".to_string())?;
    if v["snapshot_of"] != kind || !v["data"].is_object() {
        return Err(format!("not a {kind} snapshot"));
    }
    Ok((v["data"].clone(), v["written"].clone(), age))
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v[k].as_f64().filter(|x| x.is_finite())
}

fn round(x: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (x * m).round() / m
}

/// Spend level from the numbers (not the snapshot's own word):
/// over (>= limit), stop (>= stop line), near (within $0.50 of it), ok.
pub fn spend_level(fresh: f64, stop: f64, limit: f64) -> &'static str {
    if fresh >= limit {
        "over"
    } else if fresh >= stop {
        "stop"
    } else if fresh >= stop - 0.5 {
        "near"
    } else {
        "ok"
    }
}

/// Whitelisted spend fields (never `top_sessions` / `sessions`).
pub fn spend(p: &Path, _now: i64) -> Value {
    let (d, written, age) = match snapshot(p, "spend") {
        Ok(x) => x,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let Some(fresh) = num(&d, "fresh_usd") else {
        return json!({"ok": false, "error": "snapshot has no fresh spend"});
    };
    let limit = num(&d, "limit_usd").unwrap_or(LIMIT_USD);
    let stop = num(&d, "stop_line_usd").unwrap_or(STOP_LINE_USD);
    let baseline = num(&d, "baseline_usd").unwrap_or(BASELINE_USD);
    json!({
        "ok": true,
        "fresh_usd": round(fresh, 4),
        "limit_usd": limit,
        "stop_line_usd": stop,
        "baseline_usd": baseline,
        "left_to_stop_line_usd": round((stop - fresh).max(0.0), 4),
        "left_to_limit_usd": round((limit - fresh).max(0.0), 4),
        "level": spend_level(fresh, stop, limit),
        "state": d["state"].as_str().map(clip),
        "day": d["day"].as_str().map(clip),
        "day_usd": num(&d, "day_usd").map(|x| round(x, 4)),
        "day_calls": d["day_calls"].as_u64(),
        "ledger_updated": d["ledger_updated"].as_str().map(clip),
        "written": written.as_str().map(clip),
        "age_s": age,
        "stale": age > STALE_S,
    })
}

/// Whitelisted backup fields.
pub fn backup(p: &Path, _now: i64) -> Value {
    let (d, written, age) = match snapshot(p, "backup") {
        Ok(x) => x,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let problems: Vec<String> = d["problems"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .take(5)
                .map(clip)
                .collect()
        })
        .unwrap_or_default();
    json!({
        "ok": true,
        "state": d["state"].as_str().map(clip).unwrap_or_else(|| "unknown".into()),
        "archives": d["archives"].as_u64(),
        "newest_age_h": num(&d, "newest_age_h"),
        "offsite_copies": d["offsite_copies"].as_u64(),
        "problems": problems,
        "written": written.as_str().map(clip),
        "age_s": age,
        "stale": age > STALE_S,
    })
}

// ---------------------------------------------------------------- build id

/// The Hermes binary systemd would start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// Binary file name, e.g. `hermes-gateway-6da98fdc`.
    pub binary: String,
    /// Trailing hex commit in the name, when there is one.
    pub commit: Option<String>,
    /// Unit or drop-in file name the line came from.
    pub from: String,
}

/// Effective `ExecStart` of `unit` from the unit file and its `.d/*.conf`
/// drop-ins, systemd style: drop-ins sorted by file name (earlier dir wins a
/// name), an empty `ExecStart=` resets, the last one wins. Reads files only.
pub fn hermes_build(dirs: &[PathBuf], unit: &str) -> Option<Build> {
    if unit.is_empty() || unit.contains('/') || unit.starts_with('.') {
        return None;
    }
    let main = dirs.iter().map(|d| d.join(unit)).find(|p| p.is_file())?;
    let mut drops: Vec<(String, PathBuf)> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d.join(format!("{unit}.d"))) else {
            continue;
        };
        let mut here: Vec<(String, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                (n.ends_with(".conf") && !n.starts_with('.')).then(|| (n, e.path()))
            })
            .collect();
        here.sort();
        for (n, p) in here {
            if !drops.iter().any(|(m, _)| *m == n) {
                drops.push((n, p));
            }
        }
    }
    drops.sort_by(|a, b| a.0.cmp(&b.0));
    let mut cur: Option<(String, String)> = None;
    let main_name = unit.to_string();
    for (name, path) in std::iter::once((main_name, main)).chain(drops) {
        let Ok((buf, _)) = read_small(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&buf);
        let mut in_service = false;
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                in_service = l == "[Service]";
                continue;
            }
            if !in_service {
                continue;
            }
            let Some(v) = l.strip_prefix("ExecStart=") else {
                continue;
            };
            let v = v.trim();
            if v.is_empty() {
                cur = None;
                continue;
            }
            let cmd = v
                .trim_start_matches(['-', '@', ':', '+', '!'])
                .split_whitespace()
                .next()
                .unwrap_or("");
            let bin = cmd.rsplit('/').next().unwrap_or("");
            if !bin.is_empty() {
                cur = Some((bin.to_string(), name.clone()));
            }
        }
    }
    let (binary, from) = cur?;
    let binary = clip(&binary);
    let commit = binary.rsplit_once('-').and_then(|(_, tail)| {
        (tail.len() >= 7 && tail.len() <= 40 && tail.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| tail.to_ascii_lowercase())
    });
    Some(Build {
        binary,
        commit,
        from,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("phone-unit-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn page_edits_each_match_once() {
        for (from, _) in EDITS {
            assert_eq!(
                approvals::parts::BODY.matches(from).count(),
                1,
                "approvals markup changed; fix EDITS for {from:?}"
            );
        }
        let p = page();
        for id in [
            "ph-hermes-v",
            "ph-spend-v",
            "ph-wd-list",
            "ph-health",
            "ap-list",
            "st-key",
        ] {
            assert!(p.contains(&format!("id=\"{id}\"")), "{id}");
        }
        assert!(p.contains("Full console"));
        assert!(!p.contains("Back to the mind"));
        assert!(p.contains(STATUS_PATH));
        assert!(p.find("ph-top").unwrap() < p.find("id=\"ap-list\"").unwrap());
    }

    #[test]
    fn build_from_drop_ins() {
        let etc = tmp("etc");
        let lib = tmp("lib");
        fs::write(
            lib.join("hermes-rs.service"),
            "[Unit]\nDescription=x\n[Service]\nExecStart=/opt/galion/rust/bin/hermes-gateway --x\n",
        )
        .unwrap();
        let d = etc.join("hermes-rs.service.d");
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("50-rust-bin.conf"),
            "[Service]\nExecStart=\nExecStart=/opt/galion/rust/bin/hermes-gateway-aa77864\n",
        )
        .unwrap();
        fs::write(d.join("99i-self-modify.conf"), "[Service]\nExecStart=\nExecStart=-/opt/galion/rust/bin/hermes-gateway-6da98fdc --flag\n").unwrap();
        fs::write(d.join("99c-limits.conf"), "[Service]\nMemoryMax=1G\n").unwrap();
        // Not a [Service] line: ignored.
        fs::write(d.join("99z-other.conf"), "[Unit]\nExecStart=/bin/evil\n").unwrap();
        let b = hermes_build(&[etc.clone(), lib.clone()], "hermes-rs.service").unwrap();
        assert_eq!(b.binary, "hermes-gateway-6da98fdc");
        assert_eq!(b.commit.as_deref(), Some("6da98fdc"));
        assert_eq!(b.from, "99i-self-modify.conf");
        // A later reset with nothing after it: no ExecStart.
        fs::write(d.join("99zz-reset.conf"), "[Service]\nExecStart=\n").unwrap();
        assert_eq!(
            hermes_build(&[etc.clone(), lib.clone()], "hermes-rs.service"),
            None
        );
        fs::remove_file(d.join("99zz-reset.conf")).unwrap();
        // No drop-ins: the main unit, no commit in the name.
        fs::remove_dir_all(&d).unwrap();
        let b = hermes_build(&[etc, lib], "hermes-rs.service").unwrap();
        assert_eq!((b.binary.as_str(), b.commit), ("hermes-gateway", None));
        assert_eq!(hermes_build(&[], "../x"), None);
    }

    #[test]
    fn spend_whitelist_and_level() {
        let d = tmp("spend");
        let p = d.join("spend.json");
        fs::write(&p, r#"{"snapshot_of":"spend","written":"2026-10-31T09:33:40+01:00","data":{"fresh_usd":5.1501,"limit_usd":10.0,"stop_line_usd":9.5,"baseline_usd":3.449,"state":"ok","day_usd":0.0229,"top_sessions":[{"session":"secret-session-name"}],"sessions":7}}"#).unwrap();
        let v = spend(&p, 0);
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["fresh_usd"], 5.1501);
        assert_eq!(v["left_to_stop_line_usd"], 4.3499);
        assert_eq!(v["level"], "ok");
        assert_eq!(v["stale"], false);
        let s = v.to_string();
        assert!(
            !s.contains("secret-session-name") && !s.contains("sessions"),
            "{s}"
        );
        assert_eq!(spend_level(9.1, 9.5, 10.0), "near");
        assert_eq!(spend_level(9.5, 9.5, 10.0), "stop");
        assert_eq!(spend_level(10.2, 9.5, 10.0), "over");
        // Wrong kind, missing file, symlink.
        fs::write(&p, r#"{"snapshot_of":"backup","data":{}}"#).unwrap();
        assert_eq!(spend(&p, 0)["ok"], false);
        assert_eq!(spend(&d.join("nope.json"), 0)["error"], "no snapshot yet");
        let l = d.join("link.json");
        std::os::unix::fs::symlink("/etc/hostname", &l).unwrap();
        assert_eq!(spend(&l, 0)["error"], "not a regular file");
    }

    #[test]
    fn watchdog_lists_actions_then_newest() {
        let h: Probe = Ok((
            200,
            json!({"ok": true, "mode": "live", "escalations_open": 2, "incidents_open": 1}),
        ));
        let e: Probe = Ok((
            200,
            json!({"open": [
                {"id": "esc-1", "reason": "old one", "lead_event": {"t": 100, "unit": "a.service", "severity": "low"}},
                {"id": "esc-2", "reason": "new one\u{7}", "lead_event": {"t": 200, "unit": "b.service", "severity": "high"}}
            ]}),
        ));
        let a: Probe = Ok((404, json!({"ok": false})));
        let w = watchdog(&h, &e, &a, 300);
        assert_eq!(w["awaiting"], 2);
        assert_eq!(w["actions_pending"], Value::Null);
        assert_eq!(w["items"][0]["what"], "new one");
        let a: Probe = Ok((
            200,
            json!({"pending": [{"id": "ap-1", "action": "restart", "unit": "x.service", "rule": "r1", "ts": 50}]}),
        ));
        let w = watchdog(&h, &e, &a, 300);
        assert_eq!(w["awaiting"], 3);
        assert_eq!(w["items"][0]["kind"], "action");
        assert_eq!(w["items"][0]["what"], "restart x.service");
        let down: Probe = Err("not listening".into());
        assert_eq!(watchdog(&down, &e, &a, 0)["ok"], false);
    }
}
