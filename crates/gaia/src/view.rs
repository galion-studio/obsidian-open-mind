// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia's view of a real system, from captured text only.
//!
//! The caller captures `systemctl list-units --type=service ... --no-pager`
//! and `ss -ltn` (or `ss -ltnp`) output, read-only, and hands the text in.
//! This module parses it and maps every unit and listener onto the nine
//! functions and 52 systems. It opens no socket, spawns no process and
//! changes nothing: it is an observer, not a controller.

use gaia_core::ownership::{crates_for, unit_owner};
use gaia_core::{FunctionId, Json, System};

/// One row of `systemctl list-units`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

/// One `LISTEN` row of `ss -ltn[p]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub addr: String,
    pub port: u16,
    /// Process name, when the capture used `-p`.
    pub process: Option<String>,
}

/// Parse `systemctl list-units` output. Header, legend and footer lines are
/// skipped; a leading status bullet is dropped.
pub fn parse_units(text: &str) -> Vec<Unit> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line
            .trim_start()
            .trim_start_matches('\u{25cf}')
            .trim_start();
        let mut it = t.split_whitespace();
        let Some(name) = it.next() else { continue };
        if !name.ends_with(".service") {
            continue;
        }
        let (Some(load), Some(active), Some(sub)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let description = it.collect::<Vec<_>>().join(" ");
        out.push(Unit {
            name: name.to_string(),
            load: load.to_string(),
            active: active.to_string(),
            sub: sub.to_string(),
            description,
        });
    }
    out
}

/// Parse `ss -ltn` / `ss -ltnp` output.
pub fn parse_listeners(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.first() != Some(&"LISTEN") || cols.len() < 4 {
            continue;
        }
        let local = cols[3];
        let Some((addr, port)) = local.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let process = cols[4..].iter().find_map(|c| {
            let rest = c.split_once("((\"")?.1;
            Some(rest.split('"').next()?.to_string())
        });
        out.push(Listener {
            addr: addr.to_string(),
            port,
            process,
        });
    }
    out.sort_by(|a, b| (a.port, &a.addr).cmp(&(b.port, &b.addr)));
    out.dedup_by(|a, b| a.port == b.port && a.addr == b.addr);
    out
}

/// Ports named in a unit description such as `text 127.0.0.1:8091, mouth
/// 127.0.0.1:8002` or `(127.0.0.1:8660)`.
pub fn ports_in(description: &str) -> Vec<u16> {
    let mut out = Vec::new();
    for (i, _) in description.match_indices(':') {
        let digits: String = description[i + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(p) = digits.parse::<u16>() {
            if p > 0 && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// The well-known ports of systems whose unit description does not name one.
/// Process names from `ss -p` override this when present.
fn known_port(port: u16) -> Option<(System, &'static str)> {
    Some(match port {
        8080 => (System::Console, "console"),
        8420 => (System::MemoryGateway, "openmind-memory"),
        8642 => (System::Messaging, "hermes-rs API"),
        8646 => (System::GalionDesk, "galion-desk"),
        8650 => (System::Tuner, "tuner"),
        20241 | 20242 => (System::Tunnels, "cloudflared metrics"),
        _ => return None,
    })
}

/// Process name (from `ss -p`) to system.
fn process_system(name: &str) -> Option<System> {
    unit_owner(name).map(|u| u.system).or(match name {
        "cloudflared" => Some(System::Tunnels),
        "node" => Some(System::Console),
        _ => None,
    })
}

fn is_running(u: &Unit) -> bool {
    u.active == "active" && u.sub == "running"
}

/// Build Gaia's view from the two captures.
pub fn view(units_text: &str, listen_text: Option<&str>) -> Json {
    let units = parse_units(units_text);
    let listeners = listen_text.map(parse_listeners).unwrap_or_default();

    // Which system each listener belongs to, and why.
    let mut claimed: Vec<(u16, System, String)> = Vec::new();
    for u in &units {
        if let Some(o) = unit_owner(&u.name) {
            if is_running(u) {
                for p in ports_in(&u.description) {
                    claimed.push((p, o.system, format!("unit {}", u.name)));
                }
            }
        }
    }
    let mut stray = Vec::new();
    let mut host = Vec::new();
    let mut listener_rows = Vec::new();
    for l in &listeners {
        let by_proc = l
            .process
            .as_deref()
            .and_then(|p| process_system(p).map(|s| (s, format!("process {p}"))));
        let by_unit = claimed
            .iter()
            .find(|(p, _, _)| *p == l.port)
            .map(|(_, s, why)| (*s, why.clone()));
        let by_known = known_port(l.port).map(|(s, n)| (s, format!("known port ({n})")));
        let owner = by_proc.or(by_unit).or(by_known);
        let loopback = l.addr.starts_with("127.") || l.addr == "[::1]" || l.addr.contains("%lo");
        let mut row = Json::obj()
            .with("addr", l.addr.as_str())
            .with("port", u64::from(l.port))
            .with("loopback", loopback)
            .with("process", l.process.clone());
        match &owner {
            Some((s, why)) => {
                row = row
                    .with("system", s.name())
                    .with("function", s.function().name())
                    .with("by", why.as_str());
            }
            None if matches!(l.port, 22 | 25 | 53) => {
                row = row
                    .with("system", Json::Null)
                    .with("by", "host service (not stack)");
                host.push(u64::from(l.port));
            }
            None => {
                row = row.with("system", Json::Null).with("by", "unmapped");
                stray.push(Json::from(u64::from(l.port)));
            }
        }
        listener_rows.push((owner.map(|o| o.0), l.port, row));
    }

    let mut unmapped_units = Vec::new();
    let mut functions = Vec::new();
    let mut totals = (0u64, 0u64, 0u64); // systems with a running unit, with any unit, with a crate
    for f in FunctionId::ALL {
        let mut systems = Vec::new();
        for s in f.systems() {
            let su: Vec<&Unit> = units
                .iter()
                .filter(|u| unit_owner(&u.name).map(|o| o.system) == Some(s))
                .collect();
            let running = su.iter().any(|u| is_running(u));
            let crates = crates_for(s);
            let ports: Vec<Json> = listener_rows
                .iter()
                .filter(|(o, _, _)| *o == Some(s))
                .map(|(_, p, _)| Json::from(u64::from(*p)))
                .collect();
            let observed = if running {
                "live"
            } else if !su.is_empty() {
                "unit-not-running"
            } else if !crates.is_empty() {
                "crate-only"
            } else {
                "design-only"
            };
            totals.0 += u64::from(running);
            totals.1 += u64::from(!su.is_empty());
            totals.2 += u64::from(!crates.is_empty());
            systems.push(
                Json::obj()
                    .with("system", s.name())
                    .with("observed", observed)
                    .with(
                        "units",
                        Json::Arr(
                            su.iter()
                                .map(|u| {
                                    Json::obj()
                                        .with("unit", u.name.as_str())
                                        .with("active", u.active.as_str())
                                        .with("sub", u.sub.as_str())
                                        .with(
                                            "legacy",
                                            unit_owner(&u.name).is_some_and(|o| o.legacy),
                                        )
                                })
                                .collect(),
                        ),
                    )
                    .with("ports", Json::Arr(ports))
                    .with(
                        "crates",
                        Json::Arr(crates.into_iter().map(Json::from).collect()),
                    ),
            );
        }
        functions.push(
            Json::obj()
                .with("function", f.name())
                .with("systems", Json::Arr(systems)),
        );
    }
    for u in &units {
        if unit_owner(&u.name).is_none() {
            unmapped_units.push(
                Json::obj()
                    .with("unit", u.name.as_str())
                    .with("active", u.active.as_str())
                    .with("description", u.description.as_str()),
            );
        }
    }
    let running_units = units.iter().filter(|u| is_running(u)).count();
    Json::obj()
        .with(
            "mode",
            "observe-only (parsed captures; nothing was changed)",
        )
        .with(
            "summary",
            Json::obj()
                .with("units_seen", units.len())
                .with("units_running", running_units)
                .with("units_unmapped", unmapped_units.len())
                .with("listeners", listeners.len())
                .with("listeners_unmapped", stray.len())
                .with("systems_total", System::ALL.len())
                .with("systems_live", totals.0)
                .with("systems_with_unit", totals.1)
                .with("systems_with_crate", totals.2),
        )
        .with("functions", Json::Arr(functions))
        .with(
            "listeners",
            Json::Arr(listener_rows.into_iter().map(|(_, _, r)| r).collect()),
        )
        .with("unmapped_units", Json::Arr(unmapped_units))
        .with("unmapped_listeners", Json::Arr(stray))
        .with(
            "host_listeners",
            Json::Arr(host.into_iter().map(Json::from).collect()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNITS: &str = "  UNIT                    LOAD   ACTIVE SUB     DESCRIPTION
  galion-one.service      loaded active running Galion One: text 127.0.0.1:8091, mouth 127.0.0.1:8002
\u{25cf} console-screen.service  not-found inactive dead console-screen.service
  hermes.service          loaded inactive dead    Hermes agent gateway

Legend: LOAD   -> Reflects whether the unit definition was properly loaded.
3 loaded units listed.";

    const SS: &str = "State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess
LISTEN 0      128        127.0.0.1:8091       0.0.0.0:*
LISTEN 0      128        127.0.0.1:8002       0.0.0.0:*
LISTEN 0      128        127.0.0.1:8653       0.0.0.0:*    users:((\"mystery\",pid=9,fd=3))
LISTEN 0      4096               *:22               *:*
LISTEN 0      100            [::1]:25            [::]:*";

    #[test]
    fn parses_units() {
        let u = parse_units(UNITS);
        assert_eq!(u.len(), 3);
        assert_eq!(u[0].name, "galion-one.service");
        assert_eq!(u[1].load, "not-found");
        assert_eq!(u[2].active, "inactive");
        assert_eq!(ports_in(&u[0].description), vec![8091, 8002]);
    }

    #[test]
    fn parses_listeners() {
        let l = parse_listeners(SS);
        assert_eq!(l.len(), 5);
        assert_eq!(l[0].port, 22);
        assert_eq!(l[1].addr, "[::1]");
        let m = l.iter().find(|x| x.port == 8653).expect("8653");
        assert_eq!(m.process.as_deref(), Some("mystery"));
    }

    #[test]
    fn view_maps_live_and_flags_unknown() {
        let v = view(UNITS, Some(SS));
        let s = v.get("summary").expect("summary");
        assert_eq!(s.get("units_seen").and_then(Json::as_i64), Some(3));
        assert_eq!(s.get("units_running").and_then(Json::as_i64), Some(1));
        assert_eq!(s.get("units_unmapped").and_then(Json::as_i64), Some(1));
        assert_eq!(s.get("listeners_unmapped").and_then(Json::as_i64), Some(1));
        assert_eq!(s.get("systems_live").and_then(Json::as_i64), Some(1));
        let out = v.compact();
        assert!(out.contains("\"system\":\"galion-one\",\"observed\":\"live\""));
        assert!(out.contains("\"system\":\"hermes-agents\",\"observed\":\"unit-not-running\""));
        assert!(out.contains("\"unmapped_listeners\":[8653]"));
        assert!(out.contains("\"host_listeners\":[22,25]"));
    }
}
