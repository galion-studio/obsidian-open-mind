// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Component table and runtime config (env only, loopback defaults).

use std::net::SocketAddr;
use std::path::PathBuf;

/// The nine Gaia functions (plus GAIA core), as in ws/gaia `gaia-design.md`
/// section 4.2. The nerve itself is an AETHER system.
pub const GAIA_FUNCTIONS: [&str; 10] = [
    "GAIA",
    "HADES",
    "MINERVA",
    "APOLLO",
    "HEPHAESTUS",
    "ARTEMIS",
    "POSEIDON",
    "AETHER",
    "DEMETER",
    "ELEUTHIA",
];

/// What kind of thing a component is; decides which probes run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// systemd unit, optional HTTP health port.
    Service,
    /// cloudflared tunnel: unit + metrics port.
    Tunnel,
    /// The LXC itself: disk and RAM.
    Host,
    /// The `gaia` CLI (ws/gaia), if installed.
    Gaia,
    /// galion-nerve itself.
    SelfNerve,
    /// A systemd timer (`x.timer`) and the oneshot service it starts
    /// (`x.service`): the timer must be active, the last run must have
    /// succeeded and finished within `max_age_s`. Observe-only: a timer is
    /// never hard-down (no reflex acts on it).
    Timer,
}

/// One monitored component.
#[derive(Debug, Clone)]
pub struct Spec {
    pub name: &'static str,
    pub unit: Option<&'static str>,
    /// Loopback HTTP health port and path.
    pub port: Option<u16>,
    pub path: &'static str,
    /// cloudflared metrics port.
    pub metrics_port: Option<u16>,
    pub kind: Kind,
    /// Gaia function and `System` name.
    pub gaia: &'static str,
    pub gaia_system: &'static str,
    /// Max reflex restarts per hour; 0 = never restarted by the nerve.
    pub restarts_per_hour: u32,
    /// Failing probe on a non-critical component only degrades it.
    pub critical: bool,
    /// `false`: a missing unit/port is "absent", not "failing" (not deployed yet).
    pub expected: bool,
    pub note: &'static str,
    /// Correlation group: signals of one group are one incident.
    pub group: &'static str,
    /// Timer kind: the last run must have finished within this many seconds
    /// (0 = no age check).
    pub max_age_s: u64,
}

const fn svc(
    name: &'static str,
    unit: &'static str,
    port: Option<u16>,
    path: &'static str,
    gaia: &'static str,
    sys: &'static str,
    rph: u32,
) -> Spec {
    Spec {
        name,
        unit: Some(unit),
        port,
        path,
        metrics_port: None,
        kind: Kind::Service,
        gaia,
        gaia_system: sys,
        restarts_per_hour: rph,
        critical: true,
        expected: true,
        note: "",
        group: name,
        max_age_s: 0,
    }
}

/// The Galion stack on the LXC (2026-10-18). Ports from `ss -ltnp`.
pub fn default_specs(voice_unit: Option<String>, voice_port: Option<u16>) -> Vec<Spec> {
    let mut v = vec![
        svc("openmind-memory", "openmind-memory.service", Some(8420), "/health", "APOLLO", "memory-gateway", 3),
        svc("hermes-rs", "hermes-rs.service", Some(8642), "/health", "ARTEMIS", "hermes-agents", 3),
        svc("link-watch-rs", "link-watch-rs.service", None, "", "APOLLO", "link-watch", 3),
        svc("galion-desk", "galion-desk.service", Some(8646), "/health", "HEPHAESTUS", "galion-desk", 3),
        svc("tuner", "tuner.service", Some(8650), "/health", "DEMETER", "tuner", 3),
        svc("galion-one", "galion-one.service", Some(8091), "/health", "ELEUTHIA", "galion-one", 3),
        Spec {
            note: "Galion One mouth (TTS); 503 while the model loads",
            group: "galion-one",
            critical: false,
            unit: None,
            ..svc("galion-one-mouth", "", Some(8002), "/health", "AETHER", "speech-io", 0)
        },
        svc("console", "console.service", Some(8080), "/", "HEPHAESTUS", "console", 3),
        svc("galion-health", "galion-health.service", Some(8660), "/status.json", "DEMETER", "guardian-tick", 3),
        Spec {
            kind: Kind::Tunnel,
            metrics_port: Some(20241),
            note: "named tunnel; config and DNS are never touched; max 1 restart/h",
            group: "edge",
            ..svc("cloudflared", "cloudflared.service", None, "", "AETHER", "tunnels", 1)
        },
        Spec {
            kind: Kind::Tunnel,
            metrics_port: Some(20242),
            critical: false,
            note: "tuner quick tunnel (transient unit); never restarted: its trycloudflare URL would change",
            group: "edge",
            ..svc("tuner-quick", "tuner-quick.service", None, "", "AETHER", "tunnels", 0)
        },
        // T2.3 (slot 200): observe-only specs for the rest of the live VPS
        // stack (`ss -ltn` + `systemctl` read-only, 2026-10-21 17:55 +02:00).
        // restarts_per_hour 0 and critical false: the nerve reports them and
        // never restarts or rolls them back; none of these units is in the
        // reflex allowlist (reflex::ALLOW is unchanged).
        Spec {
            critical: false,
            note: "POSEIDON work queue (leases, DLQ, spend caps); observe-only",
            ..svc("openmind-queue", "openmind-queue.service", Some(19120), "/healthz", "POSEIDON", "queues", 0)
        },
        Spec {
            critical: false,
            note: "Galion One growth prototype (static, mock data), routed at one.galion.app/tanga/; observe-only",
            ..svc("tanga-proto", "tanga-proto.service", Some(8671), "/tanga/", "ELEUTHIA", "galion-one", 0)
        },
        Spec {
            critical: false,
            note: "galion-nerve SHADOW (observe-only build, reflex off); never restarted by the nerve",
            ..svc("galion-nerve-shadow", "galion-nerve-shadow.service", Some(8672), "/healthz", "AETHER", "nerve", 0)
        },
        Spec {
            kind: Kind::Timer,
            critical: false,
            // OnCalendar 00/6:17 + RandomizedDelaySec 5 min: two missed runs + 1 h.
            max_age_s: 13 * 3600,
            note: "backup every 6 h (galion-backup.timer -> galion-backup.service oneshot); observe-only",
            ..svc("galion-backup", "galion-backup.timer", None, "", "DEMETER", "survival", 0)
        },
        Spec {
            expected: false,
            critical: false,
            note: "HADES admin approval gate (dry-run, never executes); not installed on the VPS yet",
            ..svc("hades-gate", "hades-gate.service", None, "", "HADES", "teardown", 0)
        },
    ];
    let vu: &'static str = Box::leak(
        voice_unit
            .unwrap_or_else(|| "galion-voice.service".into())
            .into_boxed_str(),
    );
    v.push(Spec {
        expected: false,
        critical: false,
        note: "ws/voice KittenTTS worker; not deployed yet",
        ..svc(
            "voice-worker",
            vu,
            voice_port,
            "/health",
            "AETHER",
            "voice-relay",
            3,
        )
    });
    v.push(Spec {
        kind: Kind::Gaia,
        unit: None,
        expected: false,
        critical: false,
        note: "ws/gaia `gaia status`; absent until the gaia CLI is installed",
        ..svc("gaia", "", None, "", "GAIA", "orchestration", 0)
    });
    v.push(Spec {
        kind: Kind::Host,
        unit: None,
        note: "LXC disk (/) and RAM; single host, no scaling beyond restarts",
        ..svc("lxc-host", "", None, "", "POSEIDON", "quotas", 0)
    });
    v.push(Spec {
        kind: Kind::SelfNerve,
        unit: None,
        note: "the nerve itself",
        ..svc("galion-nerve", "", None, "", "AETHER", "nerve", 0)
    });
    v
}

/// Runtime config, from env.
#[derive(Debug, Clone)]
pub struct Config {
    pub addr: SocketAddr,
    pub interval_s: u64,
    pub data_dir: PathBuf,
    /// NERVE_DRY_RUN=1, or the file `<data_dir>/dry-run` exists (runtime switch).
    pub dry_run_env: bool,
    /// Consecutive failing cycles before a restart reflex.
    pub fail_cycles: u32,
    /// Write a self-eval report every N cycles (and on the first).
    pub report_every: u32,
    pub health_url_port: Option<u16>,
    pub gaia_bin: Option<String>,
    pub rollback: bool,
    pub deploy_bin: PathBuf,
    pub deploy_backups: PathBuf,
    pub switch_dir: PathBuf,
    pub telegram_env: Option<PathBuf>,
    pub ext_alerts_per_hour: u32,
    pub voice_unit: Option<String>,
    pub voice_port: Option<u16>,
    pub disk_path: String,
    /// NERVE_COMPONENTS: JSON file replacing the built-in VPS component table.
    pub components_file: Option<PathBuf>,
    /// NERVE_REFLEX=0 turns the reflex layer off (observe only).
    pub reflex: bool,
    /// NERVE_JOURNAL=0 skips journald (hosts without systemd).
    pub journal: bool,
    /// NERVE_SYSTEMD=0 skips `systemctl show` (hosts without systemd).
    pub systemd: bool,
    /// NERVE_ACCESS_LOGS=name=path[@component],...
    pub access_logs: Vec<AccessLog>,
    /// Listener ports that are expected but not a component's health port.
    pub known_ports: Vec<u16>,
    /// Public (non-loopback) listener ports that are intended (ssh).
    pub public_ok_ports: Vec<u16>,
    pub ddos_min_rps: f64,
    pub ddos_min_requests: u64,
    /// A 503 "loading" for longer than this is "stuck", seconds.
    pub loading_grace_s: u64,
    /// NERVE_LISTEN_SCOPE=lo-hi: judge only listeners in this port range
    /// (a shared box); unset judges every listener (the VPS).
    pub listen_scope: Option<(u16, u16)>,
    /// NERVE_PORT_OWNERS=port=name[@GAIA],...: extra port owners.
    pub port_owners: Vec<(u16, crate::owners::Entry)>,
    /// NERVE_UNOWNED_PORTS: listeners reported as "unknown owner" (default
    /// the five VPS ports 8431,8432,8433,8652,8653; empty string = none).
    pub unowned_ports: Vec<u16>,
}

/// One tailed access log.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessLog {
    pub name: String,
    pub path: PathBuf,
    pub component: Option<String>,
}

/// Parse `name=path[@component],...`.
pub fn parse_access_logs(s: &str) -> Vec<AccessLog> {
    s.split(',')
        .filter_map(|x| {
            let (n, rest) = x.trim().split_once('=')?;
            let (p, c) = match rest.rsplit_once('@') {
                Some((p, c)) => (p, Some(c.trim().to_string())),
                None => (rest, None),
            };
            (!n.trim().is_empty() && !p.trim().is_empty()).then(|| AccessLog {
                name: n.trim().to_string(),
                path: PathBuf::from(p.trim()),
                component: c,
            })
        })
        .collect()
}

/// Default `NERVE_KNOWN_PORTS`: listeners on the VPS that are expected but
/// are not (or not only) a component health port. Matches the live VPS
/// `ss -ltn` of 2026-10-21 17:55 +02:00 together with the component table.
pub const KNOWN_PORTS_VPS: &str = "25,53,8670,8671,8672,18800,20241,20242";

fn ports(s: &str) -> Vec<u16> {
    s.split([',', ' '])
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

impl Config {
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        let addr: SocketAddr = get("NERVE_ADDR")
            .unwrap_or_else(|| "127.0.0.1:8670".into())
            .parse()
            .map_err(|e| format!("NERVE_ADDR: {e}"))?;
        if !addr.ip().is_loopback() {
            return Err(format!("NERVE_ADDR {addr} is not loopback; refusing"));
        }
        let num = |k: &str, d: u64| get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
        let flag = |k: &str, d: bool| {
            get(k)
                .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(d)
        };
        Ok(Config {
            addr,
            interval_s: num("NERVE_INTERVAL", 30).clamp(5, 3600),
            data_dir: PathBuf::from(
                get("NERVE_DATA_DIR").unwrap_or_else(|| "/var/lib/galion-nerve".into()),
            ),
            dry_run_env: flag("NERVE_DRY_RUN", true),
            fail_cycles: num("NERVE_FAIL_CYCLES", 3).clamp(1, 100) as u32,
            report_every: num("NERVE_REPORT_EVERY", 20).max(1) as u32,
            health_url_port: match num("NERVE_GALION_HEALTH_PORT", 8660) {
                0 => None,
                p => Some(p as u16),
            },
            gaia_bin: get("NERVE_GAIA_BIN"),
            rollback: flag("NERVE_ROLLBACK", true),
            deploy_bin: PathBuf::from(
                get("NERVE_DEPLOY_BIN")
                    .unwrap_or_else(|| "/opt/galion/rust/bin/galion-deploy".into()),
            ),
            deploy_backups: PathBuf::from(
                get("NERVE_DEPLOY_BACKUPS").unwrap_or_else(|| "/var/backups/galion/deploy".into()),
            ),
            switch_dir: PathBuf::from(
                get("NERVE_SWITCH_DIR")
                    .unwrap_or_else(|| "/root/galion-switch-20261018-2140".into()),
            ),
            telegram_env: get("NERVE_TELEGRAM_ENV").map(PathBuf::from),
            ext_alerts_per_hour: num("NERVE_EXT_ALERTS_PER_HOUR", 4).min(6) as u32,
            voice_unit: get("NERVE_VOICE_UNIT"),
            voice_port: get("NERVE_VOICE_PORT").and_then(|v| v.parse().ok()),
            disk_path: get("NERVE_DISK_PATH").unwrap_or_else(|| "/".into()),
            components_file: get("NERVE_COMPONENTS").map(PathBuf::from),
            reflex: flag("NERVE_REFLEX", true),
            journal: flag("NERVE_JOURNAL", true),
            systemd: flag("NERVE_SYSTEMD", true),
            access_logs: parse_access_logs(&get("NERVE_ACCESS_LOGS").unwrap_or_default()),
            // 25 postfix, 53 resolved, 8670 nerve, 8671 tanga-serve, 8672 nerve
            // shadow, 18800 forge-link (box ssh reverse tunnel behind
            // forge.galion.studio), 20241/20242 cloudflared metrics.
            known_ports: ports(&get("NERVE_KNOWN_PORTS").unwrap_or_else(|| KNOWN_PORTS_VPS.into())),
            public_ok_ports: ports(&get("NERVE_PUBLIC_OK_PORTS").unwrap_or_else(|| "22".into())),
            ddos_min_rps: get("NERVE_DDOS_MIN_RPS")
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| *v > 0.0)
                .unwrap_or(10.0),
            ddos_min_requests: num("NERVE_DDOS_MIN_REQUESTS", 300).max(1),
            loading_grace_s: num("NERVE_LOADING_GRACE", 900),
            listen_scope: get("NERVE_LISTEN_SCOPE").and_then(|s| {
                let (a, b) = s.split_once('-')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            }),
            port_owners: crate::owners::parse_owners(&get("NERVE_PORT_OWNERS").unwrap_or_default()),
            unowned_ports: match get("NERVE_UNOWNED_PORTS") {
                Some(s) => ports(&s),
                None => crate::owners::VPS_UNOWNED_PORTS.to_vec(),
            },
        })
    }
}

fn leak(s: &str) -> &'static str {
    // Loaded once at start-up; the table lives for the whole process.
    Box::leak(s.to_string().into_boxed_str())
}

/// Load a component table from JSON (`NERVE_COMPONENTS`), e.g. for the box
/// test stack or another host:
///
/// ```json
/// [{"name":"tuner","port":19101,"path":"/health","gaia":"DEMETER","system":"tuner"},
///  {"name":"edge","kind":"tunnel","metrics_port":20241,"group":"edge","unit":"cloudflared.service"}]
/// ```
///
/// Keys: name (required), unit, port, path ("/health"), metrics_port, kind
/// (service|tunnel|timer), gaia ("AETHER"), system, group (name), critical (true),
/// expected (true), restarts_per_hour (0: file-defined components are never
/// restarted unless asked), note, max_age_s (timer: last-run age
/// limit in seconds, 0 = none). The host and the nerve itself are added
/// when the file does not list them.
pub fn parse_specs(text: &str) -> Result<Vec<Spec>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("components: {e}"))?;
    let arr = v.as_array().ok_or("components: expected a JSON array")?;
    let mut out: Vec<Spec> = vec![];
    for (i, c) in arr.iter().enumerate() {
        let s = |k: &str| c.get(k).and_then(|x| x.as_str());
        let name = s("name").ok_or(format!("components[{i}]: missing name"))?;
        if name.is_empty() || out.iter().any(|o| o.name == name) {
            return Err(format!("components[{i}]: empty or duplicate name {name:?}"));
        }
        let gaia = s("gaia").unwrap_or("AETHER");
        if !GAIA_FUNCTIONS.contains(&gaia) {
            return Err(format!("components[{i}]: unknown Gaia function {gaia:?}"));
        }
        let port = c.get("port").and_then(|x| x.as_u64()).map(|p| p as u16);
        let kind = match s("kind").unwrap_or("service") {
            "service" => Kind::Service,
            "tunnel" => Kind::Tunnel,
            "timer" => Kind::Timer,
            other => {
                return Err(format!(
                    "components[{i}]: kind {other:?} (service|tunnel|timer)"
                ))
            }
        };
        let b = |k: &str, d: bool| c.get(k).and_then(|x| x.as_bool()).unwrap_or(d);
        out.push(Spec {
            name: leak(name),
            unit: s("unit").map(leak),
            port,
            path: leak(s("path").unwrap_or("/health")),
            metrics_port: c
                .get("metrics_port")
                .and_then(|x| x.as_u64())
                .map(|p| p as u16),
            kind,
            gaia: leak(gaia),
            gaia_system: leak(s("system").unwrap_or(name)),
            restarts_per_hour: c
                .get("restarts_per_hour")
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32,
            critical: b("critical", true),
            expected: b("expected", true),
            note: leak(s("note").unwrap_or("")),
            group: leak(s("group").unwrap_or(name)),
            max_age_s: c.get("max_age_s").and_then(|x| x.as_u64()).unwrap_or(0),
        });
    }
    let base = default_specs(None, None);
    for k in [Kind::Host, Kind::SelfNerve] {
        if !out.iter().any(|o| o.kind == k) {
            if let Some(sp) = base.iter().find(|b| b.kind == k) {
                out.push(sp.clone());
            }
        }
    }
    Ok(out)
}

/// The component table for this config: the file when set, else built-in.
pub fn specs_for(cfg: &Config) -> Result<Vec<Spec>, String> {
    match &cfg.components_file {
        Some(p) => {
            parse_specs(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?)
        }
        None => {
            // A nerve never probes its own port as a component: on the shadow
            // (8672) the shadow is "galion-nerve" (self), not a second entry.
            let own = cfg.addr.port();
            Ok(default_specs(cfg.voice_unit.clone(), cfg.voice_port)
                .into_iter()
                .filter(|s| s.kind == Kind::SelfNerve || s.port != Some(own))
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(kv: &[(&str, &str)]) -> Result<Config, String> {
        let m: HashMap<String, String> = kv
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_env(&|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_are_loopback_and_dry() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.addr.to_string(), "127.0.0.1:8670");
        assert!(c.dry_run_env);
        assert_eq!(c.fail_cycles, 3);
        assert!(cfg(&[("NERVE_ADDR", "0.0.0.0:8670")]).is_err());
        assert!(!cfg(&[("NERVE_DRY_RUN", "0")]).unwrap().dry_run_env);
        assert_eq!(cfg(&[("NERVE_INTERVAL", "1")]).unwrap().interval_s, 5);
    }

    #[test]
    fn specs_map_to_gaia_functions() {
        let s = default_specs(None, None);
        let mut names = std::collections::HashSet::new();
        for x in &s {
            assert!(GAIA_FUNCTIONS.contains(&x.gaia), "{} -> {}", x.name, x.gaia);
            assert!(names.insert(x.name), "dup {}", x.name);
        }
        let nerve = s.iter().find(|x| x.name == "galion-nerve").unwrap();
        assert_eq!(nerve.gaia, "AETHER");
        let cf = s.iter().find(|x| x.name == "cloudflared").unwrap();
        assert_eq!(cf.restarts_per_hour, 1);
        assert_eq!(
            s.iter()
                .find(|x| x.name == "tuner-quick")
                .unwrap()
                .restarts_per_hour,
            0
        );
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn components_file_and_access_logs() {
        let s = parse_specs(r#"[{"name":"tuner","port":19101,"gaia":"DEMETER"},{"name":"room","port":19102,"critical":false,"group":"rooms"}]"#).unwrap();
        assert_eq!(s.len(), 4, "host + self appended");
        assert_eq!(s[0].port, Some(19101));
        assert_eq!(s[0].path, "/health");
        assert_eq!(s[0].restarts_per_hour, 0);
        assert_eq!(s[1].group, "rooms");
        assert!(!s[1].critical);
        assert!(parse_specs(r#"[{"name":"a"},{"name":"a"}]"#).is_err());
        assert!(parse_specs(r#"[{"name":"a","gaia":"ZEUS"}]"#).is_err());
        assert!(parse_specs("{}").is_err());
        let a = parse_access_logs("tuner=/tmp/t.log@tuner, edge=/var/log/x.log,bad");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].component.as_deref(), Some("tuner"));
        assert_eq!(a[1].component, None);
        let c = Config::from_env(&|_| None).unwrap();
        assert_eq!(
            c.known_ports,
            vec![25, 53, 8670, 8671, 8672, 18800, 20241, 20242]
        );
        assert!(c.reflex && c.journal);
    }
}

#[cfg(test)]
mod t23_tests {
    use super::*;

    /// Live VPS `ss -ltn` ports, 2026-10-21 17:55 +02:00 (read-only).
    const LIVE_PORTS: [u16; 18] = [
        22, 25, 53, 8002, 8080, 8091, 8420, 8642, 8646, 8650, 8660, 8670, 8671, 8672, 18800, 19120,
        20241, 20242,
    ];

    #[test]
    fn t23_specs_are_observe_only() {
        let s = default_specs(None, None);
        let find = |n: &str| s.iter().find(|x| x.name == n).unwrap();
        for (n, unit, port, path, gaia) in [
            (
                "openmind-queue",
                "openmind-queue.service",
                Some(19120),
                "/healthz",
                "POSEIDON",
            ),
            (
                "tanga-proto",
                "tanga-proto.service",
                Some(8671),
                "/tanga/",
                "ELEUTHIA",
            ),
            (
                "galion-nerve-shadow",
                "galion-nerve-shadow.service",
                Some(8672),
                "/healthz",
                "AETHER",
            ),
            ("galion-backup", "galion-backup.timer", None, "", "DEMETER"),
            ("hades-gate", "hades-gate.service", None, "", "HADES"),
        ] {
            let x = find(n);
            assert_eq!(x.unit, Some(unit), "{n}");
            assert_eq!(x.port, port, "{n}");
            assert_eq!(x.path, path, "{n}");
            assert_eq!(x.gaia, gaia, "{n}");
            assert_eq!(x.restarts_per_hour, 0, "{n}: observe-only");
            assert!(!x.critical, "{n}: never hard-down on a probe");
            assert!(
                !crate::reflex::ALLOW.contains(&unit),
                "{n}: {unit} must not be in the reflex allowlist"
            );
        }
        assert_eq!(find("galion-backup").kind, Kind::Timer);
        assert_eq!(find("galion-backup").max_age_s, 13 * 3600);
        assert!(!find("hades-gate").expected);
        assert_eq!(crate::reflex::ALLOW.len(), 10, "allowlist unchanged");
    }

    #[test]
    fn known_ports_cover_the_live_vps() {
        let c = Config::from_env(&|_| None).unwrap();
        let specs = specs_for(&c).unwrap();
        let om = crate::owners::OwnerMap::for_config(&specs, &c);
        for p in LIVE_PORTS {
            assert!(om.is_owned(p), "live port {p} has no owner");
        }
        let no = std::collections::BTreeMap::new();
        assert_eq!(om.lookup(19120, &no).owner, "openmind-queue");
        assert_eq!(om.lookup(8671, &no).owner, "tanga-proto");
        assert_eq!(om.lookup(8672, &no).owner, "galion-nerve-shadow");
        assert!(om.lookup(18800, &no).owner.starts_with("forge-link"));
        // The old documented-unowned ports still report as unknown owner.
        assert!(!om.is_owned(8652));
    }

    #[test]
    fn shadow_does_not_list_itself() {
        let c =
            Config::from_env(&|k| (k == "NERVE_ADDR").then(|| "127.0.0.1:8672".into())).unwrap();
        let s = specs_for(&c).unwrap();
        assert!(!s.iter().any(|x| x.name == "galion-nerve-shadow"));
        assert!(s.iter().any(|x| x.name == "galion-nerve"));
        let main = specs_for(&Config::from_env(&|_| None).unwrap()).unwrap();
        assert_eq!(main.len(), s.len() + 1);
    }

    #[test]
    fn timer_kind_from_a_components_file() {
        let s = parse_specs(r#"[{"name":"bk","unit":"bk.timer","kind":"timer","max_age_s":3600}]"#)
            .unwrap();
        assert_eq!(s[0].kind, Kind::Timer);
        assert_eq!(s[0].max_age_s, 3600);
        assert_eq!(s[0].restarts_per_hour, 0);
        assert!(parse_specs(r#"[{"name":"bk","kind":"cron"}]"#).is_err());
    }
}
