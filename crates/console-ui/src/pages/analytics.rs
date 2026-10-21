// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/analytics` ("Nerve", "This pod"): port of console-ide
//! `src/routes/_console/analytics.tsx` + `src/lib/analytics.ts`
//! `analyticsHealth()`, as in the live tree /opt/galion/console-ide on the VPS
//! (read-only, 2026-10-21 05:06 CEST; sha256 e43bf8c9... for analytics.tsx).
//!
//! Data: `GET /api/v1/analytics` on the console API (it proxies the nerve
//! snapshot at 127.0.0.1:8091 and needs the paired-phone cookie, which the
//! front forwards). Same seven boards, same rows and words.
//!
//! Differences from the TSX, on purpose:
//! - the boards are `?board=<name>` links instead of `useState` buttons, so
//!   they work without JavaScript and can be bookmarked;
//! - no 5 s polling (no JS): the page is a fresh sample on every load;
//! - when the API refuses (401) the page says the nerve answers paired
//!   phones only, instead of only "did not answer"; "Reading the pod." (the
//!   TSX loading state) cannot happen server-side and is not shown.

use super::jsfmt::{finite, num, round};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

pub const BOARDS: &[&str] = &[
    "Overview",
    "Systems",
    "Infrastructure",
    "Services",
    "Agents",
    "Users",
    "Workforce",
];

/// `?board=` value -> board name (case-insensitive; unknown -> Overview).
pub fn board_of(q: Option<&str>) -> &'static str {
    let q = q.unwrap_or("");
    BOARDS
        .iter()
        .find(|b| b.eq_ignore_ascii_case(q))
        .copied()
        .unwrap_or("Overview")
}

/// `analyticsHealth()` of `lib/analytics.ts`.
#[derive(Debug, Clone, PartialEq)]
pub struct Health {
    pub score: f64,
    pub status: String,
    pub online: f64,
    pub offline: f64,
}

/// JS `Number(x)` for the shapes the snapshot uses (missing -> NaN, null -> 0).
fn js_number(v: &Value) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Str(s) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}

fn present(v: &Value) -> bool {
    // `Value::get` gives Null for a missing key; JS `undefined ?? 0` and
    // `Number(null)` both give 0, so Null is "missing" here.
    !v.is_null()
}

pub fn health_of(snap: &Value) -> Health {
    let rows = snap.get("services").as_array();
    let row_down = rows
        .iter()
        .filter(|r| r.get("ok").as_bool() == Some(false))
        .count() as f64;
    let h = snap.get("health");
    let counted = {
        let v = h.get("services").get("offline");
        if present(v) {
            js_number(v)
        } else {
            0.0
        }
    };
    let offline = row_down.max(if counted.is_finite() { counted } else { 0.0 });
    let counted_up = {
        let v = h.get("services").get("online");
        if present(v) {
            js_number(v)
        } else {
            0.0
        }
    };
    let online = if !rows.is_empty() {
        rows.len() as f64 - row_down
    } else if counted_up.is_finite() {
        counted_up
    } else {
        0.0
    };
    let mut score = if present(h.get("score")) {
        js_number(h.get("score"))
    } else {
        f64::NAN
    };
    if !score.is_finite() {
        score = 0.0;
    }
    let mut status = h
        .get("status")
        .as_str()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if status.eq_ignore_ascii_case("undefined") {
        status.clear();
    }
    if offline > 0.0 {
        if status != "critical" {
            status = "degraded".into();
        }
        score = score.min(89.0);
    } else if status.is_empty() {
        status = "—".into();
    }
    Health {
        score,
        status,
        online,
        offline,
    }
}

/// `shown(value, fallback)`.
fn shown(v: &Value, fallback: &str) -> String {
    match v {
        Value::Num(n) if n.is_finite() => num(*n),
        Value::Num(_) => fallback.to_string(),
        Value::Str(s) => {
            let t = s.trim();
            if t.is_empty() || t == "undefined" || t == "NaN" {
                fallback.to_string()
            } else {
                t.to_string()
            }
        }
        _ => fallback.to_string(),
    }
}

/// `plain(value)`.
fn plain(v: &Value) -> String {
    let t = shown(v, "");
    if t == "—" {
        String::new()
    } else {
        t
    }
}

/// `measure(value)`.
fn measure(v: &Value) -> String {
    match finite(v) {
        None => shown(v, "—"),
        Some(n) if n.fract() == 0.0 => num(n),
        Some(n) => num(round(n * 100.0) / 100.0),
    }
}

fn measure_n(n: f64) -> String {
    measure(&Value::Num(n))
}

fn first_finite(g: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| finite(g.get(k)))
}

/// `instanceGpu(gpu)`.
fn instance_gpu(gpu: &Value) -> String {
    const NONE: &str = "no gpu on this instance";
    if !matches!(gpu, Value::Obj(_)) {
        return NONE.into();
    }
    let name = plain(gpu.get("name"));
    let used = first_finite(gpu, &["usedMb", "used_mb", "used"]);
    let total = first_finite(gpu, &["totalMb", "total_mb", "total"]);
    if name.is_empty() && used.is_none() && total.is_none() {
        return NONE.into();
    }
    let unit = if first_finite(gpu, &["usedMb", "totalMb", "used_mb", "total_mb"]).is_some() {
        " MB"
    } else {
        ""
    };
    format!(
        "{} · {} / {}{unit}",
        if name.is_empty() { "—" } else { &name },
        used.map(measure_n).unwrap_or_else(|| "—".into()),
        total.map(measure_n).unwrap_or_else(|| "—".into()),
    )
}

/// `instanceLine(cortex)`.
fn instance_line(c: &Value) -> String {
    let src = match c.get("instance") {
        v @ Value::Obj(_) => v,
        _ => c,
    };
    // JS: `src.gpu !== undefined ? src.gpu : cortex.gpu`; a JSON null is
    // not undefined, but both render "no gpu" so Null may fall through.
    let gpu = match src.get("gpu") {
        Value::Null => c.get("gpu"),
        g => g,
    };
    format!(
        "{} cores · {} load · {}% cpu · {}% memory · {}",
        measure(src.get("cores")),
        measure(src.get("load")),
        measure(src.get("cpu")),
        measure(src.get("memory")),
        instance_gpu(gpu)
    )
}

fn laptop_state(l: &Value) -> String {
    match l.get("up") {
        Value::Bool(b) => return if *b { "up" } else { "down" }.into(),
        v @ Value::Str(_) => {
            let t = plain(v).to_lowercase();
            if t == "up" || t == "down" {
                return t;
            }
        }
        _ => {}
    }
    for k in ["online", "ok"] {
        if let Some(b) = l.get(k).as_bool() {
            return if b { "up" } else { "down" }.into();
        }
    }
    let st = plain(l.get("status")).to_lowercase();
    if st == "up" || st == "down" {
        return st;
    }
    "—".into()
}

fn laptop_gpu(g: &Value) -> String {
    match g {
        Value::Str(_) => {
            let p = plain(g);
            if p.is_empty() {
                "—".into()
            } else {
                p
            }
        }
        Value::Obj(_) => instance_gpu(g),
        _ => "—".into(),
    }
}

/// `laptopLine(cortex)`.
fn laptop_line(c: &Value) -> String {
    let l = c.get("laptop");
    if !matches!(l, Value::Obj(_)) {
        return "—".into();
    }
    let name = plain(l.get("name"));
    let model = {
        let m = plain(l.get("model"));
        if m.is_empty() {
            plain(l.get("modelName"))
        } else {
            m
        }
    };
    let base = format!(
        "{} · {} · {}",
        if name.is_empty() { "—" } else { &name },
        laptop_gpu(l.get("gpu")),
        laptop_state(l)
    );
    if model.is_empty() {
        base
    } else {
        format!("{base} · {model}")
    }
}

fn model_name(item: &Value) -> String {
    match item {
        Value::Str(_) => plain(item),
        Value::Obj(_) => ["name", "model", "id"]
            .iter()
            .map(|k| plain(item.get(k)))
            .find(|s| !s.is_empty())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// `poolLine(pool)`.
fn pool_line(pool: &Value) -> String {
    let list = match pool {
        Value::Str(_) => {
            let p = plain(pool);
            return if p.is_empty() { "—".into() } else { p };
        }
        Value::Arr(a) => a.as_slice(),
        Value::Obj(_) => match pool.get("models") {
            Value::Arr(a) => a.as_slice(),
            Value::Null => match pool.get("names") {
                Value::Arr(a) => a.as_slice(),
                _ => return "—".into(),
            },
            _ => return "—".into(),
        },
        _ => return "—".into(),
    };
    let names: Vec<String> = list
        .iter()
        .map(model_name)
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        "—".into()
    } else {
        names.join(" · ")
    }
}

/// `gb(n)`.
fn gb(v: &Value) -> String {
    match finite(v) {
        Some(n) => format!(
            "{} GB",
            super::jsfmt::to_fixed(n / 1024.0 / 1024.0 / 1024.0, 1)
        ),
        None => "—".into(),
    }
}

/// JSX `{value}` for a number/string child (undefined/null/bool render nothing).
fn child(v: &Value) -> String {
    match v {
        Value::Num(n) => num(*n),
        Value::Str(s) => s.clone(),
        _ => String::new(),
    }
}

fn row(label: &str, value: &str) -> String {
    format!(
        "<li><span>{}</span><span class=\"muted\">{}</span></li>",
        esc(label),
        esc(value)
    )
}

fn tabs(current: &str) -> String {
    let mut out = String::from("<nav class=\"tabs\" aria-label=\"Boards\">");
    for b in BOARDS {
        let cur = if *b == current {
            " aria-current=\"page\""
        } else {
            ""
        };
        out.push_str(&format!(
            "<a href=\"/analytics?board={}\"{cur}>{b}</a>",
            b.to_ascii_lowercase()
        ));
    }
    out.push_str("</nav>");
    out
}

/// Why the snapshot is missing, for the error line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    /// HTTP status from the API (401 = not a paired phone).
    Status(u16),
    /// No answer / not JSON.
    Down,
}

fn overview(snap: &Value, out: &mut String) {
    let h = health_of(snap);
    out.push_str("<ul class=\"rows\">");
    out.push_str(&row("Health", &format!("{} · {}", num(h.score), h.status)));
    let cortex = snap.get("cortex");
    if matches!(cortex, Value::Obj(_)) {
        out.push_str(&row("This instance", &instance_line(cortex)));
        out.push_str(&row("Laptop", &laptop_line(cortex)));
        out.push_str(&row("Pool", &pool_line(cortex.get("pool"))));
    }
    out.push_str(&row(
        "Services",
        &format!("{} up · {} down", num(h.online), num(h.offline)),
    ));
    let avg = snap.get("agents").get("hermes").get("avg");
    let avg_s = match finite(avg) {
        Some(n) if n != 0.0 => format!("{} ms", num(n)),
        _ => "—".into(),
    };
    out.push_str(&row("Hermes", &format!("{avg_s} · mean of the last calls")));
    out.push_str("</ul>");

    let events = snap.get("nerve").get("events").as_array();
    let edge = snap.get("edge").as_array();
    let defence = snap.get("defence").as_array();
    if events.is_empty() && edge.is_empty() && defence.is_empty() {
        return;
    }
    out.push_str("<h2>Changes</h2><ul class=\"rows\">");
    for line in events.iter().take(6) {
        out.push_str(&format!("<li><span>{}</span></li>", esc(&shown(line, ""))));
    }
    if events.is_empty() {
        out.push_str(&row("Changes", "quiet"));
    }
    out.push_str("</ul><h2>Edge</h2><ul class=\"rows\">");
    for e in edge {
        let code = match e.get("code") {
            Value::Num(n) if *n != 0.0 && !n.is_nan() => shown(e.get("code"), "—"),
            Value::Str(s) if !s.is_empty() => shown(e.get("code"), "—"),
            _ => "—".into(),
        };
        out.push_str(&row(
            &child(e.get("host")),
            &format!("{code} · {} ms", shown(e.get("ms"), "—")),
        ));
    }
    if edge.is_empty() {
        out.push_str(&row("Edge", "waiting for the next pass"));
    }
    out.push_str("</ul><h2>Guardian</h2><ul class=\"rows\">");
    for d in defence.iter().take(8) {
        out.push_str(&row(
            &child(d.get("service")),
            &format!(
                "{} · {} ms",
                shown(d.get("status"), "—"),
                shown(d.get("ms"), "—")
            ),
        ));
    }
    if defence.is_empty() {
        out.push_str(&row("Guardian", "no checks yet"));
    }
    out.push_str("</ul>");
}

fn infrastructure(snap: &Value, out: &mut String) {
    let i = snap.get("infrastructure");
    out.push_str("<ul class=\"rows\">");
    let lines = [
        (
            "CPU",
            i.get("cpu"),
            format!(
                "{} load · {} cores",
                shown(i.get("load"), "—"),
                shown(i.get("cores"), "—")
            ),
        ),
        (
            "Memory",
            i.get("memory"),
            // live tree: "—" unless both memUsed and memTotal are present
            if i.get("memUsed").is_null() || i.get("memTotal").is_null() {
                "—".to_string()
            } else {
                format!("{} of {}", gb(i.get("memUsed")), gb(i.get("memTotal")))
            },
        ),
        (
            "Disk",
            i.get("disk").get("pct"),
            format!("{} of the 100 GB quota", gb(i.get("disk").get("used"))),
        ),
    ];
    for (label, pct, note) in lines {
        out.push_str(&row(label, &format!("{}% · {note}", child(pct))));
    }
    let g = i.get("gpu");
    let gpu = if matches!(g, Value::Obj(_)) {
        format!(
            "{}% · {} · {} / {} MB · {}% busy",
            shown(g.get("pct"), "—"),
            shown(g.get("name"), "—"),
            shown(g.get("usedMb"), "—"),
            shown(g.get("totalMb"), "—"),
            shown(g.get("util"), "—")
        )
    } else {
        "No GPU reading.".into()
    };
    out.push_str(&row("GPU", &gpu));
    out.push_str("</ul>");
}

fn systems(snap: &Value, out: &mut String) {
    let rows = snap.get("systems").as_array();
    out.push_str("<ul class=\"rows\">");
    for r in rows {
        let ok = r.get("ok").as_bool() == Some(true);
        out.push_str(&row(
            &shown(r.get("id"), "—"),
            &format!(
                "{} · {}",
                if ok { "up" } else { "down" },
                shown(r.get("note"), "—")
            ),
        ));
    }
    if rows.is_empty() {
        out.push_str(&row("Systems", "waiting for the next pass"));
    }
    out.push_str("</ul>");
}

fn services(snap: &Value, out: &mut String) {
    out.push_str("<ul class=\"rows\">");
    for s in snap.get("services").as_array() {
        let ok = s.get("ok").as_bool() == Some(true);
        out.push_str(&row(
            &shown(s.get("id"), "—"),
            &format!(
                ":{} · {} · {} ms",
                shown(s.get("port"), "—"),
                if ok { "up" } else { "down" },
                shown(s.get("ms"), "—")
            ),
        ));
    }
    out.push_str("</ul>");
}

fn agents(snap: &Value, out: &mut String) {
    let a = snap.get("agents");
    let last_at = match a.get("lastAt") {
        Value::Str(s) if !s.is_empty() => shown(a.get("lastAt"), "—"),
        Value::Num(n) if *n != 0.0 && !n.is_nan() => shown(a.get("lastAt"), "—"),
        _ => "none yet".into(),
    };
    out.push_str("<ul class=\"rows\">");
    out.push_str(&row(
        "Galion One",
        &format!(
            "{} · voice turns kept · {last_at}",
            shown(a.get("turns"), "0")
        ),
    ));
    let last: Vec<String> = a
        .get("hermes")
        .get("last")
        .as_array()
        .iter()
        .filter_map(finite)
        .map(|n| format!("{} ms", num(n)))
        .collect();
    out.push_str(&row(
        "Last latencies",
        &if last.is_empty() {
            "none".into()
        } else {
            last.join(" · ")
        },
    ));
    out.push_str("</ul>");
}

/// The page body. `snap` is the `/api/v1/analytics` JSON when it answered.
pub fn body(snap: Result<&Value, Miss>, board: &str) -> String {
    let board = board_of(Some(board));
    let mut out = String::from("<div class=\"page w46\">");
    out.push_str(&format!(
        "<header><h1>{}<span class=\"ell\">This pod</span></h1>\
<p class=\"lede\">One sample every five seconds, from the nerve. No second probe.</p></header>",
        icon("nerve", "i i-md")
    ));
    out.push_str(&tabs(board));
    let snap = match snap {
        Ok(s) => s,
        Err(m) => {
            out.push_str("<p class=\"lede\">The analytics API did not answer.</p>");
            if m == Miss::Status(401) {
                out.push_str(
                    "<p class=\"note\">HTTP 401: the nerve answers paired phones only. Pair this phone on the Desk, then reload.</p>",
                );
            } else if let Miss::Status(c) = m {
                out.push_str(&format!(
                    "<p class=\"note\">HTTP {c} from /api/v1/analytics.</p>"
                ));
            }
            out.push_str("</div>");
            return out;
        }
    };
    match board {
        "Overview" => overview(snap, &mut out),
        "Infrastructure" => infrastructure(snap, &mut out),
        "Systems" => systems(snap, &mut out),
        "Services" => services(snap, &mut out),
        "Agents" => agents(snap, &mut out),
        "Users" => {
            out.push_str("<ul class=\"rows\">");
            out.push_str(&row(
                "Paired phones",
                &child(snap.get("users").get("phones")),
            ));
            out.push_str("</ul><p class=\"lede\">Count only. No names, no hashes.</p>");
        }
        _ => {
            out.push_str("<ul class=\"rows\">");
            out.push_str(&row("Workforce", "held"));
            out.push_str(&format!(
                "</ul><p class=\"lede\">The neural workforce stays in the fridge. The live agent on this pod is {}.</p>",
                esc(&child(snap.get("workforce").get("live")))
            ));
        }
    }
    out.push_str("</div>");
    out
}

/// Every row text a board shows, in order (used by the real-life test to
/// compare the page with the live payload).
pub fn health_line(snap: &Value) -> String {
    let h = health_of(snap);
    format!("{} · {}", num(h.score), h.status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    const SNAP: &str = r#"{"at":"2026-10-21T04:50:00+02:00",
      "health":{"score":97,"status":"healthy","anomalies":0,"services":{"online":5,"offline":0}},
      "infrastructure":{"cpu":12.5,"load":0.42,"cores":8,"memory":61,"memUsed":5368709120,"memTotal":16106127360,
        "disk":{"pct":38,"used":40802189312,"total":107374182400},"gpu":null},
      "services":[{"id":"console","port":8080,"ok":true,"ms":3},{"id":"hermes","port":8642,"ok":false,"ms":1500}],
      "agents":{"turns":12,"lastAt":"2026-10-21T04:40:00+02:00","hermes":{"last":[120,null,95.5],"avg":107.75}},
      "users":{"phones":2},"workforce":{"zone":"fridge","live":"Hermes"},
      "defence":[{"service":"guard","ok":true,"status":"ok","ms":4}],
      "edge":[{"host":"galion.app","code":200,"ms":88},{"host":"<b>x</b>","code":0,"ms":12}],
      "systems":[{"id":"nerve","ok":true,"note":"fresh"}],
      "nerve":{"events":["a","b","c","d","e","f","g"]},
      "cortex":{"instance":{"cores":8,"load":0.4166,"cpu":12,"memory":61.0,"gpu":{"name":"L4","usedMb":100,"totalMb":24000}},
        "laptop":{"name":"mate-lt","gpu":"RTX 4060","online":true,"modelName":"qwen"},
        "pool":{"models":["a",{"model":"b"},{"id":""},7]}}}"#;

    fn snap() -> Value {
        parse(SNAP.as_bytes()).unwrap()
    }

    #[test]
    fn health_degrades_on_a_down_row() {
        let h = health_of(&snap());
        assert_eq!(h.status, "degraded");
        assert_eq!(h.score, 89.0);
        assert_eq!((h.online, h.offline), (1.0, 1.0));
        let ok =
            parse(br#"{"health":{"score":"93","status":" undefined "},"services":[]}"#).unwrap();
        let h = health_of(&ok);
        assert_eq!((h.score, h.status.as_str()), (93.0, "—"));
        let crit = parse(
            br#"{"health":{"score":40,"status":"critical","services":{"offline":2,"online":3}}}"#,
        )
        .unwrap();
        let h = health_of(&crit);
        assert_eq!(
            (h.score, h.status.as_str(), h.online, h.offline),
            (40.0, "critical", 3.0, 2.0)
        );
        let none = parse(b"{}").unwrap();
        assert_eq!(health_of(&none).status, "—");
    }

    #[test]
    fn overview_rows_match_the_tsx() {
        let b = body(Ok(&snap()), "");
        assert!(b.contains("<span>Health</span><span class=\"muted\">89 · degraded</span>"));
        assert!(b.contains("8 cores · 0.42 load · 12% cpu · 61% memory · L4 · 100 / 24000 MB"));
        assert!(b.contains("mate-lt · RTX 4060 · up · qwen"));
        assert!(b.contains("<span>Pool</span><span class=\"muted\">a · b</span>"));
        assert!(b.contains("1 up · 1 down"));
        assert!(b.contains("107.75 ms · mean of the last calls"));
        // six events only
        assert!(b.contains("<li><span>f</span></li>") && !b.contains("<li><span>g</span></li>"));
        assert!(b.contains("&lt;b&gt;x&lt;/b&gt;</span><span class=\"muted\">— · 12 ms"));
        assert!(b.contains("galion.app</span><span class=\"muted\">200 · 88 ms"));
        assert!(b.contains("guard</span><span class=\"muted\">ok · 4 ms"));
        assert!(b.contains("aria-current=\"page\">Overview</a>"));
        assert!(!b.contains("<script"));
    }

    #[test]
    fn other_boards() {
        let s = snap();
        let b = body(Ok(&s), "infrastructure");
        assert!(b.contains("12.5% · 0.42 load · 8 cores"));
        assert!(b.contains("61% · 5.0 GB of 15.0 GB"));
        assert!(b.contains("38% · 38.0 GB of the 100 GB quota"));
        assert!(b.contains("No GPU reading."));
        let b = body(Ok(&s), "Systems");
        assert!(b.contains("nerve</span><span class=\"muted\">up · fresh"));
        let b = body(Ok(&s), "services");
        assert!(b.contains("hermes</span><span class=\"muted\">:8642 · down · 1500 ms"));
        let b = body(Ok(&s), "agents");
        assert!(b.contains("12 · voice turns kept · 2026-10-21T04:40:00+02:00"));
        assert!(b.contains("120 ms · 95.5 ms"));
        let b = body(Ok(&s), "users");
        assert!(b.contains("Paired phones</span><span class=\"muted\">2"));
        let b = body(Ok(&s), "workforce");
        assert!(b.contains("The live agent on this pod is Hermes."));
        assert_eq!(board_of(Some("nope")), "Overview");
    }

    #[test]
    fn empty_and_refused() {
        let e = parse(br#"{"health":{"score":100,"status":"healthy"},"services":[],"agents":{"hermes":{"last":[],"avg":0}},"infrastructure":{"disk":{}}}"#).unwrap();
        let b = body(Ok(&e), "Overview");
        assert!(b.contains("— · mean of the last calls"));
        assert!(!b.contains("<h2>Changes</h2>"));
        assert!(!b.contains("This instance"));
        let b = body(Ok(&e), "Infrastructure");
        assert!(b.contains("<span>Memory</span><span class=\"muted\">% · —</span>"));
        let b = body(Ok(&e), "Systems");
        assert!(b.contains("waiting for the next pass"));
        let b = body(Ok(&e), "Agents");
        assert!(b.contains("0 · voice turns kept · none yet") && b.contains(">none<"));
        let b = body(Err(Miss::Status(401)), "Overview");
        assert!(b.contains("The analytics API did not answer."));
        assert!(b.contains("paired phones only"));
        let b = body(Err(Miss::Down), "Overview");
        assert!(b.contains("did not answer") && !b.contains("HTTP"));
    }

    #[test]
    fn cortex_variants() {
        let c = parse(br#"{"cores":4,"load":1,"cpu":5,"memory":7,"gpu":{"name":"T4","used":1,"total":2},"laptop":{"up":"DOWN","gpu":{"usedMb":3}},"pool":["x"]}"#).unwrap();
        assert_eq!(
            instance_line(&c),
            "4 cores · 1 load · 5% cpu · 7% memory · T4 · 1 / 2"
        );
        assert_eq!(laptop_line(&c), "— · — · 3 / — MB · down");
        assert_eq!(pool_line(c.get("pool")), "x");
        let d = parse(br#"{"laptop":{"status":"Up"},"pool":{"names":[]}}"#).unwrap();
        assert_eq!(laptop_line(&d), "— · — · up");
        assert_eq!(pool_line(d.get("pool")), "—");
        assert_eq!(
            instance_line(&d),
            "— cores · — load · —% cpu · —% memory · no gpu on this instance"
        );
    }
}
