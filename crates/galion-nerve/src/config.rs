// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

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
            ..svc("cloudflared", "cloudflared.service", None, "", "AETHER", "tunnels", 1)
        },
        Spec {
            kind: Kind::Tunnel,
            metrics_port: Some(20242),
            critical: false,
            note: "tuner quick tunnel (transient unit); never restarted: its trycloudflare URL would change",
            ..svc("tuner-quick", "tuner-quick.service", None, "", "AETHER", "tunnels", 0)
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
        })
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
