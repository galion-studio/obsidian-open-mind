// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia S3.4: read-only **live classification**.
//!
//! `gaia classify` runs Gaia's own live probes (the `status --live` probes
//! plus the nerve, cortex, desk, mouth, galion-health and unit-only
//! services, [`gaia_live::classify_probes`]) and puts Gaia's per-unit
//! verdict next to the nerve's (`GET /state` components, incidents and
//! per-function view) and the cortex's (`GET /health`, `GET /incidents`).
//! Every disagreement becomes a finding. For each unit that is not healthy
//! it also says what `heal-tick` would do in **plan-only** mode: the real
//! scope guard ([`gaia_live::heal::check_target`]), the nerve's restarts
//! that count toward the 3/h cap (D-L2), and the HADES plan. Nothing is
//! applied, no token is read or used, and nothing is POSTed: the only
//! requests are HTTP GETs, so a remote host can be classified through an
//! ssh `-L` tunnel with unit states and file ages from a read-only capture
//! ([`gaia_live::Remote`]).

use gaia_core::Json;
use gaia_live::heal::{check_target, TestTarget, MAX_RESTARTS_PER_HOUR, TEST_PORTS};
use gaia_live::{ProbeResult, State};
use serde_json::Value;
use std::collections::BTreeMap;

/// A verdict on one common scale, so Gaia, nerve and cortex compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Ok,
    Warn,
    Bad,
    /// The nerve lists it but says it is not deployed.
    Absent,
    /// No verdict (not watched, unreadable, unknown word).
    Unknown,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Ok => "ok",
            Class::Warn => "warn",
            Class::Bad => "bad",
            Class::Absent => "absent",
            Class::Unknown => "unknown",
        }
    }
    /// Gaia's probe state.
    pub fn of_gaia(s: &State) -> Class {
        match s {
            State::Up => Class::Ok,
            State::Degraded => Class::Warn,
            State::Down => Class::Bad,
        }
    }
    /// The nerve's component / function status words.
    pub fn of_nerve(s: &str) -> Class {
        match s {
            "healthy" | "ok" | "up" => Class::Ok,
            "degrading" | "degraded" | "failing-soon" | "warn" => Class::Warn,
            "failing" | "down" | "dead" | "critical" | "error" => Class::Bad,
            "absent" | "no-live-systems" => Class::Absent,
            _ => Class::Unknown,
        }
    }
}

/// Everything the classifier looks at. Missing documents are findings, not
/// errors: a classifier that cannot read the nerve must say so loudly.
pub struct Inputs {
    pub probes: Vec<ProbeResult>,
    /// The probes' HTTP ports by service name (for the heal scope check).
    pub ports: BTreeMap<String, (u16, String)>,
    pub nerve_state: Result<Value, String>,
    pub cortex_health: Result<Value, String>,
    pub cortex_incidents: Result<Value, String>,
}

struct Finding {
    severity: &'static str,
    kind: &'static str,
    subject: String,
    detail: String,
}

impl Finding {
    fn json(&self) -> Json {
        Json::obj()
            .with("severity", self.severity)
            .with("kind", self.kind)
            .with("subject", self.subject.as_str())
            .with("detail", self.detail.as_str())
    }
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The nerve component a Gaia probe is about: same unit first, then same
/// name (the mouth and the nerve itself have no unit in `/state`).
fn nerve_component<'a>(comps: &'a [Value], p: &ProbeResult) -> Option<&'a Value> {
    let unit = p.unit.as_ref().map(|(u, _)| u.as_str());
    comps
        .iter()
        .find(|c| unit.is_some() && s(c, "unit") == unit)
        .or_else(|| comps.iter().find(|c| s(c, "name") == Some(p.name.as_str())))
}

/// A component name as the cortex writes it, without its source prefix
/// (`unit:x.service` -> `x.service`, `nerve-incident:edge` -> `edge`).
fn bare(c: &str) -> &str {
    c.strip_prefix("unit:")
        .or_else(|| c.strip_prefix("nerve-incident:"))
        .unwrap_or(c)
}

/// Open incidents (nerve or cortex shape: `open: [{id, components, ...}]`)
/// whose `components` (prefix-stripped, see [`bare`]) name any of `names`.
/// Not `cause` on the nerve side: the nerve groups the mouth under cause
/// `galion-one`, which is itself healthy. Returns `(id, cause)` pairs.
fn open_incidents(doc: &Value, names: &[&str]) -> Vec<(String, String)> {
    doc.get("open")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|i| {
                    let comps = strs(i.get("components"));
                    names
                        .iter()
                        .any(|n| !n.is_empty() && comps.iter().any(|c| bare(c) == *n))
                })
                .filter_map(|i| {
                    s(i, "id").map(|id| (id.to_string(), s(i, "cause").unwrap_or("").to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn open_count(doc: &Value) -> usize {
    doc.get("open")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

/// What `heal-tick` would do for one unit, plan-only (no apply, no token,
/// no nerve event). The scope check is the real guard of the apply path.
fn heal_plan(p: &ProbeResult, port: Option<&(u16, String)>, nerve: Option<&Value>) -> Json {
    let scope = match port {
        Some((port, path)) => check_target(
            &TestTarget {
                name: p.name.clone(),
                port: *port,
                health_path: path.clone(),
                argv: Vec::new(),
            },
            &TEST_PORTS,
        )
        .err()
        .unwrap_or_else(|| {
            "inside the test range; would still need a start command and a hades-gate token".into()
        }),
        None => "no HTTP port: heal-tick only heals loopback HTTP test services".into(),
    };
    let unit = p.unit.as_ref().map_or(p.name.as_str(), |(u, _)| u.as_str());
    let restarts = nerve.map(|st| gaia_live::nerve::restarts_for(st, unit));
    let (used, left, rerr) = match restarts {
        Some(Ok(r)) => (
            Some(r.count as u64),
            Some(MAX_RESTARTS_PER_HOUR.saturating_sub(r.count) as u64),
            None,
        ),
        Some(Err(e)) => (None, Some(0), Some(e)),
        None => (
            None,
            Some(0),
            Some("nerve /state unreadable; failing closed".into()),
        ),
    };
    Json::obj()
        .with("mode", "plan-only")
        .with("verdict", "refused")
        .with("stage", "scope")
        .with("reason", scope)
        .with("applied", false)
        .with("token_used", false)
        .with("nerve_event_posted", false)
        .with("nerve_restarts_1h", used)
        .with("cap_left_for_gaia", left)
        .with("cap_error", rerr)
        .with(
            "hades_plan",
            Json::Arr(
                [
                    format!("inspect: journalctl -u {unit} -n 100 (read-only)"),
                    format!("restart (operator, after an Alpha's approval): systemctl restart {unit}"),
                    "if the restart does not help: the recorded rollback for the last deploy (operator, after approval)".to_string(),
                ]
                .into_iter()
                .map(Json::from)
                .collect(),
            ),
        )
}

/// Classify. Pure: all evidence is in `inp`.
#[allow(clippy::too_many_lines)]
pub fn classify(inp: &Inputs) -> Json {
    let mut findings: Vec<Finding> = Vec::new();
    let nerve = inp.nerve_state.as_ref().ok();
    let comps: Vec<Value> = nerve
        .and_then(|n| n.get("components"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Err(e) = &inp.nerve_state {
        findings.push(Finding {
            severity: "error",
            kind: "nerve-unreadable",
            subject: "galion-nerve".into(),
            detail: format!("GET /state failed: {e}; no nerve verdicts"),
        });
    } else if comps.is_empty() {
        findings.push(Finding {
            severity: "error",
            kind: "nerve-unreadable",
            subject: "galion-nerve".into(),
            detail: "/state has no components list".into(),
        });
    }
    let cortex_inc = inp.cortex_incidents.as_ref().ok();
    if let Err(e) = &inp.cortex_incidents {
        findings.push(Finding {
            severity: "warn",
            kind: "cortex-unreadable",
            subject: "gaia-cortex".into(),
            detail: format!("GET /incidents failed: {e}; no cortex verdicts"),
        });
    }
    let nerve_inc = nerve.and_then(|n| n.get("incidents"));

    let mut units = Vec::new();
    let mut matched: Vec<String> = Vec::new();
    let mut cortex_matched: Vec<String> = Vec::new();
    let (mut agree, mut disagree) = (0u64, 0u64);
    // Gaia per-function worst class (for the function view).
    let mut gaia_fn: BTreeMap<&'static str, Class> = BTreeMap::new();

    for p in &inp.probes {
        let g = Class::of_gaia(&p.state);
        let f = p.system.function().name();
        let e = gaia_fn.entry(f).or_insert(Class::Ok);
        *e = (*e).max(g);
        let unit = p.unit.as_ref().map_or("", |(u, _)| u.as_str());
        let comp = nerve_component(&comps, p);
        let cname = comp.and_then(|c| s(c, "name")).unwrap_or("");
        if !cname.is_empty() {
            matched.push(cname.to_string());
        }
        let nclass = comp
            .and_then(|c| s(c, "status"))
            .map_or(Class::Unknown, Class::of_nerve);
        let names = [cname, p.name.as_str(), unit];
        let n_inc = nerve_inc
            .map(|d| open_incidents(d, &names))
            .unwrap_or_default();
        // The cortex mirrors a nerve incident as `nerve-incident:<cause>`,
        // so the causes of this unit's nerve incidents also name it there.
        let mut cnames: Vec<&str> = names.to_vec();
        cnames.extend(n_inc.iter().map(|(_, c)| c.as_str()));
        let c_inc = cortex_inc
            .map(|d| open_incidents(d, &cnames))
            .unwrap_or_default();
        cortex_matched.extend(c_inc.iter().map(|(id, _)| id.clone()));
        let n_inc: Vec<String> = n_inc.into_iter().map(|(id, _)| id).collect();
        let c_inc: Vec<String> = c_inc.into_iter().map(|(id, _)| id).collect();
        let c_unit = cortex_inc
            .and_then(|d| d.get("units"))
            .and_then(|u| u.get(unit))
            .is_some_and(|u| {
                u.get("circuit_open").and_then(Value::as_bool) == Some(true)
                    || u.get("failures_window")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        > 0
            });
        let cclass = match cortex_inc {
            None => Class::Unknown,
            Some(_) if !c_inc.is_empty() || c_unit => Class::Warn,
            Some(_) => Class::Ok,
        };
        let mut unit_findings = 0usize;
        let mut push = |fs: &mut Vec<Finding>, sev, kind, detail: String| {
            fs.push(Finding {
                severity: sev,
                kind,
                subject: p.name.clone(),
                detail,
            });
            unit_findings += 1;
        };
        if comp.is_none() && nerve.is_some() {
            push(
                &mut findings,
                "warn",
                "nerve-blind",
                format!(
                    "Gaia sees {} {} ({}) but the nerve has no component for it",
                    p.name,
                    g.name(),
                    if unit.is_empty() { "no unit" } else { unit }
                ),
            );
        } else if comp.is_some() && nclass != g {
            let sev = if matches!(
                (g, nclass),
                (Class::Ok, Class::Bad) | (Class::Bad, Class::Ok)
            ) {
                "error"
            } else {
                "warn"
            };
            let reasons = strs(comp.and_then(|c| c.get("reasons"))).join("; ");
            push(
                &mut findings,
                sev,
                "gaia-nerve-disagree",
                format!(
                    "Gaia says {} [{}], nerve says {} ({}) [{}]",
                    g.name(),
                    p.notes.join("; "),
                    nclass.name(),
                    comp.and_then(|c| s(c, "status")).unwrap_or("?"),
                    reasons
                ),
            );
        }
        if let Some(c) = comp {
            let nf = c.pointer("/gaia/function").and_then(Value::as_str);
            let ns = c.pointer("/gaia/system").and_then(Value::as_str);
            if nf.is_some_and(|nf| nf != f) {
                push(
                    &mut findings,
                    "info",
                    "ownership-disagree",
                    format!(
                        "Gaia files it under {f}/{}, the nerve under {}/{}",
                        p.system.name(),
                        nf.unwrap_or("?"),
                        ns.unwrap_or("?")
                    ),
                );
            }
        }
        if !n_inc.is_empty() && cortex_inc.is_some() && c_inc.is_empty() {
            push(
                &mut findings,
                "warn",
                "cortex-missed",
                format!(
                    "nerve has open incident(s) {} for it; the cortex has none open",
                    n_inc.join(", ")
                ),
            );
        }
        if g == Class::Ok && nclass == Class::Ok && !n_inc.is_empty() {
            push(
                &mut findings,
                "info",
                "nerve-incident-on-healthy",
                format!(
                    "the nerve has open incident(s) {} for it while its component row and Gaia say healthy",
                    n_inc.join(", ")
                ),
            );
        }
        if cclass == Class::Warn
            && g == Class::Ok
            && matches!(nclass, Class::Ok | Class::Unknown)
            && n_inc.is_empty()
        {
            push(
                &mut findings,
                "info",
                "cortex-disagree",
                format!(
                    "the cortex has open incident(s) {} for it; Gaia and the nerve see it healthy",
                    if c_inc.is_empty() {
                        "(unit circuit/failures)".to_string()
                    } else {
                        c_inc.join(", ")
                    }
                ),
            );
        }
        let worst = g.max(if nclass == Class::Unknown {
            Class::Ok
        } else {
            nclass
        });
        let heal = (worst != Class::Ok && worst != Class::Absent)
            .then(|| heal_plan(p, inp.ports.get(&p.name), nerve));
        if unit_findings == 0 {
            agree += 1;
        } else {
            disagree += 1;
        }
        let mut row = Json::obj()
            .with("service", p.name.as_str())
            .with("unit", (!unit.is_empty()).then_some(unit))
            .with("system", p.system.name())
            .with("function", f)
            .with(
                "gaia",
                Json::obj()
                    .with("verdict", g.name())
                    .with("state", p.state.name())
                    .with(
                        "notes",
                        Json::Arr(p.notes.iter().map(|n| Json::from(n.as_str())).collect()),
                    ),
            )
            .with(
                "nerve",
                Json::obj()
                    .with("component", (!cname.is_empty()).then_some(cname))
                    .with("verdict", nclass.name())
                    .with("status", comp.and_then(|c| s(c, "status")))
                    .with(
                        "open_incidents",
                        Json::Arr(n_inc.iter().map(|x| Json::from(x.as_str())).collect()),
                    ),
            )
            .with(
                "cortex",
                Json::obj().with("verdict", cclass.name()).with(
                    "open_incidents",
                    Json::Arr(c_inc.iter().map(|x| Json::from(x.as_str())).collect()),
                ),
            )
            .with("agree", unit_findings == 0);
        if let Some(h) = heal {
            row = row.with("heal_tick", h);
        }
        units.push(row);
    }

    // Nerve components Gaia does not probe.
    for c in &comps {
        let name = s(c, "name").unwrap_or("?");
        if matched.iter().any(|m| m == name) {
            continue;
        }
        let st = s(c, "status").unwrap_or("?");
        findings.push(Finding {
            severity: if Class::of_nerve(st) == Class::Ok || Class::of_nerve(st) == Class::Absent {
                "info"
            } else {
                "warn"
            },
            kind: "gaia-blind",
            subject: name.to_string(),
            detail: format!(
                "the nerve watches {name} ({st}; {}) but Gaia has no probe for it",
                strs(c.get("reasons")).join("; ")
            ),
        });
    }

    // Cortex incidents about things Gaia does not probe.
    if let Some(open) = cortex_inc
        .and_then(|d| d.get("open"))
        .and_then(Value::as_array)
    {
        for i in open {
            let id = s(i, "id").unwrap_or("?");
            if cortex_matched.iter().any(|m| m == id) {
                continue;
            }
            let comps = strs(i.get("components"));
            let subject = comps.first().map_or("?", |c| bare(c)).to_string();
            findings.push(Finding {
                severity: "info",
                kind: "gaia-blind",
                subject,
                detail: format!(
                    "the cortex has open incident {id} ({}; {}) on something Gaia has no probe for",
                    s(i, "severity").unwrap_or("?"),
                    s(i, "title").unwrap_or("")
                ),
            });
        }
    }
    // Function view: Gaia's worst class per function vs the nerve's.
    let mut functions = Json::obj();
    let nfn = nerve.and_then(|n| n.get("gaia")).and_then(Value::as_object);
    let mut fnames: Vec<&str> = gaia_fn.keys().copied().collect();
    if let Some(m) = nfn {
        for k in m.keys() {
            if !fnames.contains(&k.as_str()) {
                fnames.push(k.as_str());
            }
        }
    }
    for fname in fnames {
        let gc = gaia_fn.get(fname).copied();
        let ns = nfn.and_then(|m| m.get(fname)).and_then(|v| s(v, "status"));
        let nc = ns.map(Class::of_nerve);
        let agree_f = match (gc, nc) {
            (Some(g), Some(n)) => g == n,
            (None, Some(Class::Absent)) | (None, None) => true,
            _ => false,
        };
        if !agree_f && nerve.is_some() {
            findings.push(Finding {
                severity: "info",
                kind: "function-disagree",
                subject: fname.to_string(),
                detail: format!(
                    "Gaia (live probes) says {}, nerve says {}",
                    gc.map_or("no probed systems", Class::name),
                    ns.unwrap_or("nothing")
                ),
            });
        }
        functions = functions.with(
            fname,
            Json::obj()
                .with("gaia", gc.map(Class::name))
                .with("nerve", ns)
                .with("agree", agree_f),
        );
    }

    // Whole-system and cortex-vs-nerve.
    let n_sys = nerve.and_then(|n| n.get("system"));
    let n_open = nerve_inc.map_or(0, open_count);
    let c_open = cortex_inc.map(open_count);
    if let (Some(c), true) = (c_open, n_open > 0) {
        if c == 0 {
            findings.push(Finding {
                severity: "warn",
                kind: "cortex-nerve-incident-gap",
                subject: "gaia-cortex".into(),
                detail: format!(
                    "the nerve has {n_open} open incident(s), the cortex 0 (it only opens incidents from nerve incident events and its own rules)"
                ),
            });
        }
    }
    let ch = inp.cortex_health.as_ref().ok();
    let c_mode = ch.and_then(|h| s(h, "mode"));
    if c_mode == Some("dry-run") {
        findings.push(Finding {
            severity: "info",
            kind: "cortex-mode",
            subject: "gaia-cortex".into(),
            detail: "cortex /health reports mode dry-run (its fixes are simulated)".into(),
        });
    }

    let mut sev = BTreeMap::<&str, u64>::new();
    for f in &findings {
        *sev.entry(f.severity).or_default() += 1;
    }
    let mut sev_j = Json::obj();
    for (k, v) in sev {
        sev_j = sev_j.with(k, v);
    }
    let gaia_overall = {
        let worst = inp
            .probes
            .iter()
            .map(|p| Class::of_gaia(&p.state))
            .max()
            .unwrap_or(Class::Unknown);
        match worst {
            Class::Ok => "healthy",
            Class::Warn | Class::Bad => "degraded",
            _ => "unknown",
        }
    };
    Json::obj()
        .with("mode", "live-classify-read-only")
        .with(
            "safety",
            "HTTP GET only (probes, nerve /state, cortex /health and /incidents); unit states and file ages from systemctl is-active / stat; heal-tick plan-only: no apply, no token, no nerve event",
        )
        .with(
            "summary",
            Json::obj()
                .with("units", inp.probes.len())
                .with("agree", agree)
                .with("disagree", disagree)
                .with("findings", findings.len())
                .with("by_severity", sev_j),
        )
        .with(
            "overall",
            Json::obj()
                .with("gaia", gaia_overall)
                .with("nerve_status", n_sys.and_then(|v| s(v, "status")))
                .with(
                    "nerve_score",
                    n_sys.and_then(|v| v.get("score")).and_then(Value::as_u64),
                )
                .with("nerve_open_incidents", n_open)
                .with("cortex_mode", c_mode)
                .with("cortex_open_incidents", c_open)
                .with(
                    "cortex_escalations_open",
                    ch.and_then(|h| h.get("escalations_open")).and_then(Value::as_u64),
                )
                .with(
                    "cortex_hermes_handoff",
                    ch.and_then(|h| h.get("hermes_handoff")).and_then(Value::as_bool),
                ),
        )
        .with("units", Json::Arr(units))
        .with("functions", functions)
        .with("findings", Json::Arr(findings.iter().map(Finding::json).collect()))
}

/// Parse `8420=20940,8642=20941`.
pub fn parse_port_map(s: &str) -> Result<BTreeMap<u16, u16>, String> {
    let mut m = BTreeMap::new();
    for pair in s.split(',').filter(|p| !p.is_empty()) {
        let (r, l) = pair
            .split_once('=')
            .ok_or_else(|| format!("port map entry {pair:?} is not REMOTE=LOCAL"))?;
        let r: u16 = r.trim().parse().map_err(|_| format!("bad port {r:?}"))?;
        let l: u16 = l.trim().parse().map_err(|_| format!("bad port {l:?}"))?;
        m.insert(r, l);
    }
    Ok(m)
}

fn read_json(path: &str) -> Result<Value, String> {
    let t = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&t).map_err(|e| format!("{path}: not JSON ({e})"))
}

/// `gaia classify ...` (see the usage text in main.rs).
pub fn cli(args: &[&str]) -> Result<(Json, bool), String> {
    let opt = |name: &str| -> Option<&str> {
        args.iter()
            .position(|a| *a == name)
            .and_then(|i| args.get(i + 1).copied())
    };
    let mut o = gaia_live::Opts::default();
    if let Some(h) = opt("--host") {
        o.host = h.to_string();
    }
    if args.contains(&"--no-systemd") {
        o.systemd = false;
    }
    if let Some(ms) = opt("--timeout-ms") {
        let ms: u64 = ms.parse().map_err(|_| "--timeout-ms needs a number")?;
        o.connect_timeout = std::time::Duration::from_millis(ms.max(50));
        o.read_timeout = std::time::Duration::from_millis(ms.max(50) * 2);
    }
    let mut r = gaia_live::Remote::default();
    if let Some(m) = opt("--port-map") {
        r.port_map = parse_port_map(m)?;
    }
    if let Some(ev) = opt("--evidence") {
        let t = std::fs::read_to_string(ev).map_err(|e| format!("{ev}: {e}"))?;
        r.parse_evidence(&t)?;
    }
    let local = |remote: u16| r.port_map.get(&remote).copied().unwrap_or(remote);
    let addr = |remote: u16| -> Result<std::net::SocketAddr, String> {
        use std::net::ToSocketAddrs;
        (o.host.as_str(), local(remote))
            .to_socket_addrs()
            .map_err(|e| format!("resolve: {e}"))?
            .next()
            .ok_or_else(|| "no address".to_string())
    };
    let nerve_port: u16 = opt("--nerve-port")
        .map_or(Ok(8670), str::parse)
        .map_err(|_| "bad --nerve-port")?;
    let cortex_port: u16 = opt("--cortex-port")
        .map_or(Ok(8674), str::parse)
        .map_err(|_| "bad --cortex-port")?;
    let probes = gaia_live::classify_probes();
    let ports = probes
        .iter()
        .filter_map(|p| {
            p.http
                .as_ref()
                .map(|h| (p.name.clone(), (h.port, h.path.clone())))
        })
        .collect();
    let results = gaia_live::probe_all_with(&probes, &o, &r);
    let nerve_state = match opt("--nerve-state") {
        Some(f) => read_json(f),
        None => addr(nerve_port).and_then(gaia_live::nerve::state),
    };
    let cortex_health = match opt("--cortex-health") {
        Some(f) => read_json(f),
        None => addr(cortex_port).and_then(|a| gaia_live::nerve::get_json(a, "/health")),
    };
    let cortex_incidents = match opt("--cortex-incidents") {
        Some(f) => read_json(f),
        None => addr(cortex_port).and_then(|a| gaia_live::nerve::get_json(a, "/incidents")),
    };
    let j = classify(&Inputs {
        probes: results,
        ports,
        nerve_state,
        cortex_health,
        cortex_incidents,
    });
    let disagree = j
        .get("summary")
        .and_then(|s| s.get("disagree"))
        .and_then(Json::as_i64)
        .unwrap_or(0)
        > 0;
    Ok((j, disagree))
}
