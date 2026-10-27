// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `guardian-tick heal` and `guardian-tick classify`.
//!
//! heal: probe -> classify -> repair plan -> apply -> incident store -> nerve.
//!
//! * Dry-run by default. A restart is applied only when its unit has a
//!   `[[repair]]` rule in the probe file AND is named with `--allow UNIT`.
//! * Protected units (hermes, gateway, ssh, cloudflared, memory, ...) are
//!   refused when the probe file is loaded and again when planning.
//! * Reflex cap: at most 3 applied restarts per unit per rolling hour,
//!   counted from the incident store, so it holds across one-shot ticks.
//! * Incident store: `<state>/incidents.jsonl`, append-only, one JSON object
//!   per line, `ts` in Europe/Zagreb RFC 3339 (never `Z`) plus `unix`.
//! * Nerve: one `openmind-nerve` event per incident change, restart,
//!   refusal and cap hit, appended to `--nerve PATH` (default
//!   `<state>/nerve.jsonl`). The file sink stays authoritative (D-T24-3);
//!   each event is also mirrored to the live nerve through `nerve-client`
//!   (`crit` -> kind `error`, else `event`; name = event kind; msg = the JSON
//!   data). No-op unless `NERVE_URL` is set; a down nerve never fails a tick.
//!
//! classify: read a captured galion-health `status.json` and print what heal
//! WOULD do. It cannot apply anything (no `--allow`, no backend is run).
//!
//! Ported from galion-healer/galion-guardian (317d1d4), dropped in 13a296c.

use crate::{run_spec, Spec};
use openmind_survival::heal::{
    classify, count_in_window, is_protected, plan, Class, Classified, PlanItem, Rule, Step,
    REFLEX_CAP, REFLEX_WINDOW_S,
};
use openmind_survival::ProbeResult;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn d_after() -> u32 {
    2
}
fn d_backend() -> String {
    "none".into()
}
fn d_verify() -> u64 {
    1500
}
fn d_timeout() -> u64 {
    15_000
}

/// `[[repair]]` in the probe file: the allow-list.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairSpec {
    pub probe: String,
    pub unit: String,
    #[serde(default = "d_after")]
    pub after_fails: u32,
    /// `systemd` (`systemctl restart UNIT`), `command` (argv, no shell) or `none`.
    #[serde(default = "d_backend")]
    pub backend: String,
    #[serde(default)]
    pub command: Vec<String>,
    /// Re-probe this long after an applied restart.
    #[serde(default = "d_verify")]
    pub verify_after_ms: u64,
    #[serde(default = "d_timeout")]
    pub timeout_ms: u64,
}

pub fn validate(rs: &[RepairSpec]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for r in rs {
        if is_protected(&r.unit) {
            return Err(format!("repair unit {} is protected", r.unit));
        }
        if !seen.insert(r.probe.clone()) {
            return Err(format!("two repair rules for probe {}", r.probe));
        }
        match r.backend.as_str() {
            "systemd" | "none" => {}
            "command" => {
                let ok = r
                    .command
                    .first()
                    .is_some_and(|p| Path::new(p).is_absolute());
                if !ok {
                    return Err(format!(
                        "repair {}: backend command needs an absolute argv[0]",
                        r.unit
                    ));
                }
            }
            o => return Err(format!("repair {}: unknown backend {o}", r.unit)),
        }
    }
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Append-only JSONL incident store.
pub struct Store {
    pub path: PathBuf,
}

impl Store {
    pub fn append(&self, mut v: Value) -> Result<(), String> {
        let unix = now_unix();
        v["ts"] = json!(zagreb_time::from_unix(unix));
        v["unix"] = json!(unix);
        if let Some(d) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        f.write_all(format!("{v}\n").as_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn read(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Services with an open incident (replay of opened/resolved).
    pub fn open_services(rows: &[Value]) -> BTreeSet<String> {
        let mut open = BTreeSet::new();
        for r in rows {
            let s = r["service"].as_str().unwrap_or("").to_string();
            match r["kind"].as_str() {
                Some("opened") => {
                    open.insert(s);
                }
                Some("resolved") => {
                    open.remove(&s);
                }
                _ => {}
            }
        }
        open
    }

    /// Unix stamps of applied restarts of `unit`.
    pub fn applied_restarts(rows: &[Value], unit: &str) -> Vec<i64> {
        rows.iter()
            .filter(|r| {
                r["kind"] == "restart" && r["mode"] == "apply" && r["unit"].as_str() == Some(unit)
            })
            .filter_map(|r| r["unix"].as_i64())
            .collect()
    }
}

struct Nerve(Option<openmind_nerve::Writer>);

impl Nerve {
    fn open(path: &Path) -> Result<Self, String> {
        if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        openmind_nerve::Writer::open(path, 1 << 20, 3)
            .map(|w| Nerve(Some(w)))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
    fn send(&mut self, kind: &str, severity: &str, data: Value) -> Result<(), String> {
        if let Some(w) = self.0.as_mut() {
            let ev = openmind_nerve::Event::new("guardian-tick", kind, severity, &data.to_string())
                .map_err(|e| e.to_string())?;
            w.append(&ev).map_err(|e| e.to_string())?;
        }
        // Mirror to the live nerve (no-op without NERVE_URL; never fails).
        nerve_client::send(
            "guardian-tick",
            live_kind(severity),
            kind,
            None,
            &data.to_string(),
        );
        Ok(())
    }
}

/// Live nerve kind for a file-sink severity: `crit` is an error, the rest
/// (info/warn) are events.
fn live_kind(severity: &str) -> &'static str {
    if severity == "crit" {
        "error"
    } else {
        "event"
    }
}

fn mode(apply: bool) -> &'static str {
    if apply {
        "apply"
    } else {
        "dry-run"
    }
}

/// Run one restart through its backend. Only called with apply = true.
fn run_backend(r: &RepairSpec) -> Result<String, String> {
    let (prog, args): (String, Vec<String>) = match r.backend.as_str() {
        "systemd" => ("systemctl".into(), vec!["restart".into(), r.unit.clone()]),
        "command" => (r.command[0].clone(), r.command[1..].to_vec()),
        _ => return Ok("skip: backend none".into()),
    };
    let mut child = Command::new(&prog)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {prog}: {e}"))?;
    let deadline = Instant::now() + Duration::from_millis(r.timeout_ms);
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            return if st.success() {
                Ok(format!("restarted via {}", r.backend))
            } else {
                Err(format!("{} exited {st}", r.backend))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{} timed out", r.backend));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn load_streak(p: &Path) -> BTreeMap<String, u32> {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_atomic(p: &Path, body: &str) -> Result<(), String> {
    if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("tmp~");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

fn rules_of(rs: &[RepairSpec]) -> Vec<Rule> {
    rs.iter()
        .map(|r| Rule {
            probe: r.probe.clone(),
            unit: r.unit.clone(),
            after_fails: r.after_fails,
        })
        .collect()
}

fn plan_json(c: &Classified, p: &PlanItem) -> Value {
    json!({
        "service": c.name, "class": c.class.as_str(), "detail": c.detail, "text": c.text,
        "unit": p.unit, "step": p.step.token(), "reason": p.step.reason(),
    })
}

pub struct HealArgs {
    pub state: PathBuf,
    pub nerve: Option<PathBuf>,
    pub allow: BTreeSet<String>,
    pub cap: u32,
    pub out: Option<PathBuf>,
}

/// One heal tick. Returns the report JSON.
pub fn heal(specs: &[Spec], repairs: &[RepairSpec], a: &HealArgs) -> Result<Value, String> {
    std::fs::create_dir_all(&a.state).map_err(|e| format!("{}: {e}", a.state.display()))?;
    let store = Store {
        path: a.state.join("incidents.jsonl"),
    };
    let nerve_path = a
        .nerve
        .clone()
        .unwrap_or_else(|| a.state.join("nerve.jsonl"));
    let mut nerve = Nerve::open(&nerve_path)?;
    let stamp = zagreb_time::now_rfc3339();

    // 1. probe
    let results: Vec<ProbeResult> = specs.iter().map(run_spec).collect();
    // 2. classify
    let classes = classify(&results);

    // streaks (consecutive non-healthy ticks)
    let sp = a.state.join("streak.json");
    let mut streak = load_streak(&sp);
    for c in &classes {
        let n = if c.class == Class::Healthy {
            0
        } else {
            streak.get(&c.name).copied().unwrap_or(0) + 1
        };
        streak.insert(c.name.clone(), n);
    }
    write_atomic(
        &sp,
        &serde_json::to_string(&streak).map_err(|e| e.to_string())?,
    )?;

    // incidents: open on first non-healthy tick, resolve on healthy
    let rows = store.read();
    let open = Store::open_services(&rows);
    for c in &classes {
        let is_open = open.contains(&c.name);
        let sev = if c.class == Class::Down {
            "high"
        } else {
            "low"
        };
        if c.class != Class::Healthy && !is_open {
            store.append(json!({"kind":"opened","service":c.name,"class":c.class.as_str(),"severity":sev,"detail":c.detail}))?;
            nerve.send(
                "incident-opened",
                if c.class == Class::Down {
                    "crit"
                } else {
                    "warn"
                },
                json!({"service":c.name,"class":c.class.as_str(),"detail":c.detail}),
            )?;
        } else if c.class == Class::Healthy && is_open {
            store
                .append(json!({"kind":"resolved","service":c.name,"resolution":"probe healthy"}))?;
            nerve.send("incident-resolved", "info", json!({"service":c.name}))?;
        }
    }

    // 3. plan
    let rules = rules_of(repairs);
    let rows = store.read();
    let now = now_unix();
    let used = |u: &str| count_in_window(&Store::applied_restarts(&rows, u), now, REFLEX_WINDOW_S);
    let plan_items = plan(
        &classes,
        &rules,
        &|n| streak.get(n).copied().unwrap_or(0),
        &used,
        a.cap,
    );

    // 4. apply (allow-listed only) + 5. store + 6. nerve
    let mut out_plan = Vec::new();
    let mut actions = Vec::new();
    for ((c, p), (spec, _r)) in classes
        .iter()
        .zip(&plan_items)
        .zip(specs.iter().zip(&results))
    {
        out_plan.push(plan_json(c, p));
        let unit = p.unit.clone().unwrap_or_default();
        match &p.step {
            Step::Restart { used, cap } => {
                let rs = repairs.iter().find(|r| r.probe == c.name).expect("rule");
                let apply = a.allow.contains(&rs.unit);
                let (ok, result) = if apply {
                    match run_backend(rs) {
                        Ok(t) => (true, t),
                        Err(e) => (false, format!("error: {e}")),
                    }
                } else {
                    (
                        true,
                        format!("would restart via {} (not in --allow)", rs.backend),
                    )
                };
                store.append(
                    json!({"kind":"restart","service":c.name,"unit":unit,"mode":mode(apply),
                    "n": used + 1, "cap": cap, "result": result}),
                )?;
                nerve.send(
                    "heal-restart",
                    if ok { "warn" } else { "crit" },
                    json!({"service":c.name,"unit":unit,"mode":mode(apply),"n":used+1,"cap":cap,"result":result}),
                )?;
                let mut act = json!({"service":c.name,"unit":unit,"mode":mode(apply),"n":used+1,"cap":cap,"result":result});
                if apply && ok {
                    std::thread::sleep(Duration::from_millis(rs.verify_after_ms));
                    let v = run_spec(spec);
                    store.append(json!({"kind":"verify","service":c.name,"unit":unit,"ok":v.ok,"detail":v.detail}))?;
                    act["verify"] = json!({"ok": v.ok, "detail": v.detail});
                }
                actions.push(act);
            }
            Step::CapReached { used, cap } => {
                store.append(
                    json!({"kind":"cap-hit","service":c.name,"unit":unit,"used":used,"cap":cap}),
                )?;
                nerve.send(
                    "heal-cap-hit",
                    "crit",
                    json!({"service":c.name,"unit":unit,"used":used,"cap":cap,"text":"operator needed"}),
                )?;
                actions.push(
                    json!({"service":c.name,"unit":unit,"mode":"held","result":p.step.reason()}),
                );
            }
            Step::RefuseProtected => {
                nerve.send(
                    "heal-refused",
                    "warn",
                    json!({"service":c.name,"unit":unit}),
                )?;
            }
            _ => {}
        }
    }
    let rep = json!({
        "ts": stamp,
        "mode": if a.allow.is_empty() { "dry-run" } else { "apply-allowlisted" },
        "allow": a.allow,
        "cap": a.cap.min(REFLEX_CAP),
        "window_s": REFLEX_WINDOW_S,
        "plan": out_plan,
        "actions": actions,
        "store": store.path,
        "nerve": nerve_path,
    });
    let out = a
        .out
        .clone()
        .unwrap_or_else(|| a.state.join("heal-report.json"));
    write_atomic(
        &out,
        &(serde_json::to_string_pretty(&rep).map_err(|e| e.to_string())? + "\n"),
    )?;
    Ok(rep)
}

/// Print a heal/classify report as plain lines.
pub fn print_report(rep: &Value, head: &str) {
    println!("{head}");
    for p in rep["plan"].as_array().into_iter().flatten() {
        println!(
            "  classify {:<22} {:<8} {}",
            p["service"].as_str().unwrap_or(""),
            p["class"].as_str().unwrap_or(""),
            p["detail"].as_str().unwrap_or("")
        );
    }
    for p in rep["plan"].as_array().into_iter().flatten() {
        println!(
            "  plan     {:<22} {:<16} unit={} reason={}",
            p["service"].as_str().unwrap_or(""),
            p["step"].as_str().unwrap_or(""),
            p["unit"].as_str().unwrap_or("-"),
            p["reason"].as_str().unwrap_or("")
        );
    }
    for a in rep["actions"].as_array().into_iter().flatten() {
        println!(
            "  action   {:<22} mode={} unit={} result={}{}",
            a["service"].as_str().unwrap_or(""),
            a["mode"].as_str().unwrap_or(""),
            a["unit"].as_str().unwrap_or(""),
            a["result"].as_str().unwrap_or(""),
            a.get("verify")
                .map(|v| format!(
                    " verify_ok={} ({})",
                    v["ok"],
                    v["detail"].as_str().unwrap_or("")
                ))
                .unwrap_or_default()
        );
    }
}

/// galion-health status.json service row.
#[derive(Debug, Deserialize)]
struct HealthRow {
    name: String,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    unit_state: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    http: String,
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct HealthDoc {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    checked_at: String,
    services: Vec<HealthRow>,
}

/// Classify a captured galion-health status.json and plan WITHOUT applying.
/// Rules: the probe file's `[[repair]]` set when given, otherwise one
/// hypothetical rule per row that names a unit (after 1 fail), so the plan
/// shows which units heal would restart and which it would refuse.
pub fn classify_status(text: &str, repairs: Option<&[RepairSpec]>) -> Result<Value, String> {
    let doc: HealthDoc = serde_json::from_str(text).map_err(|e| format!("status.json: {e}"))?;
    let results: Vec<ProbeResult> = doc
        .services
        .iter()
        .map(|s| {
            let mut r = ProbeResult::new(
                &s.name,
                s.ok,
                format!(
                    "unit={} state={} port={} http={}",
                    s.unit, s.unit_state, s.port, s.http
                ),
            );
            r.ts = doc.checked_at.clone();
            r
        })
        .collect();
    let classes = classify(&results);
    let rules: Vec<Rule> = match repairs {
        Some(rs) => rules_of(rs),
        None => doc
            .services
            .iter()
            .filter(|s| !s.unit.is_empty() && s.unit != "-")
            .map(|s| Rule {
                probe: s.name.clone(),
                unit: s.unit.clone(),
                after_fails: 1,
            })
            .collect(),
    };
    let p = plan(&classes, &rules, &|_| 1, &|_| 0, REFLEX_CAP);
    let would: Vec<Value> = classes
        .iter()
        .zip(&p)
        .filter(|(_, p)| !matches!(p.step, Step::Observe))
        .map(|(c, p)| json!({"service":c.name,"unit":p.unit,"mode":"would",
            "result": match p.step { Step::Restart{..} => format!("WOULD restart {} (dry-run; classify never applies)", p.unit.clone().unwrap_or_default()), _ => p.step.reason() }}))
        .collect();
    Ok(json!({
        "ts": zagreb_time::now_rfc3339(),
        "source_checked_at": doc.checked_at,
        "source_ok": doc.ok,
        "mode": "classify-only",
        "plan": classes.iter().zip(&p).map(|(c, p)| plan_json(c, p)).collect::<Vec<_>>(),
        "actions": would,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_kind_maps_crit_to_error() {
        assert_eq!(live_kind("crit"), "error");
        assert_eq!(live_kind("warn"), "event");
        assert_eq!(live_kind("info"), "event");
    }

    #[test]
    fn validate_rejects() {
        let base = RepairSpec {
            probe: "a".into(),
            unit: "a.service".into(),
            after_fails: 1,
            backend: "command".into(),
            command: vec!["rel/path".into()],
            verify_after_ms: 0,
            timeout_ms: 100,
        };
        assert!(validate(std::slice::from_ref(&base)).is_err());
        let mut p = base.clone();
        p.command = vec!["/bin/true".into()];
        assert!(validate(std::slice::from_ref(&p)).is_ok());
        p.unit = "hermes-rs.service".into();
        assert!(validate(&[p]).unwrap_err().contains("protected"));
        let mut b = base.clone();
        b.backend = "kill".into();
        assert!(validate(&[b]).is_err());
    }

    #[test]
    fn store_replay() {
        let rows: Vec<Value> = vec![
            json!({"kind":"opened","service":"a"}),
            json!({"kind":"opened","service":"b"}),
            json!({"kind":"resolved","service":"a"}),
            json!({"kind":"restart","unit":"b.service","mode":"apply","unix":10}),
            json!({"kind":"restart","unit":"b.service","mode":"dry-run","unix":11}),
        ];
        let open = Store::open_services(&rows);
        assert_eq!(open.into_iter().collect::<Vec<_>>(), ["b"]);
        assert_eq!(Store::applied_restarts(&rows, "b.service"), [10]);
    }

    #[test]
    fn classify_status_doc() {
        let t = r#"{"ok":false,"checked_at":"2026-10-20T18:51:23+02:00","services":[
          {"name":"tuner","unit":"tuner.service","unit_state":"failed","port":8650,"http":"-","ok":false},
          {"name":"hermes-rs","unit":"hermes-rs.service","unit_state":"failed","port":8642,"http":"-","ok":false},
          {"name":"one","unit":"-","unit_state":"n/a","port":8091,"http":"503","ok":false},
          {"name":"ssh","unit":"ssh.service","unit_state":"active","port":22,"http":"tcp-open","ok":true}]}"#;
        let v = classify_status(t, None).unwrap();
        let steps: Vec<&str> = v["plan"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["step"].as_str().unwrap())
            .collect();
        assert_eq!(steps, ["restart", "refuse-protected", "notify", "observe"]);
        assert!(v["actions"][0]["result"]
            .as_str()
            .unwrap()
            .starts_with("WOULD restart tuner.service"));
        assert!(!v["ts"].as_str().unwrap().ends_with('Z'));
    }
}
