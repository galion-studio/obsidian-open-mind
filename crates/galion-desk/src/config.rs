// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! Settings for one galion-desk process.
//!
//! Bind, the Ollama address, state paths and the TCP probe targets come from
//! `openmind-config` when the config file exists (`$OPENMIND_CONFIG`, default
//! `/etc/openmind/openmind.toml`). `OPENMIND_*` overrides win over keys in
//! that file. `DESK_*` wins over both. A missing config file keeps today's
//! loopback addresses and absolute paths, and `OPENMIND_*` is not consulted.
//!
//!   DESK_BIND        listen address (default `127.0.0.1:8646`; from a present
//!                    file: `127.0.0.1:{ports.desk}`)
//!   DESK_OLLAMA      Ollama `host:port`, or an `http(s)` URL (default
//!                    `127.0.0.1:11434`; from a present file:
//!                    `{hosts.ollama}:{ports.ollama}`)
//!   DESK_STATE_DIR   desk state directory (default
//!                    `/opt/galion/galion-light/state`; from a present file:
//!                    `paths.base`)
//!   DESK_ONE_DIR     voice and session directory (default
//!                    `/workspace/galion-light/one`; from a present file:
//!                    `paths.base/one`)
//!   DESK_HERMES_DIR  Hermes state directory (default `/opt/galion/hermes`;
//!                    from a present file: `paths.base/hermes`)
//!   DESK_HERMES_BIN  mindpack binary (default
//!                    `/opt/galion/rust/bin/desk-hermes`)
//!
//! TCP probe targets for the `services` list, the `hermes`/`one`/`tunnel`
//! flags and the `stamp` note. Each is a `host:port`. The defaults are the
//! loopback ports the live desk has always probed, so an unset variable keeps
//! today's behaviour. They exist so a box instance can be wired to box test
//! instances instead of production ports.
//!
//!   DESK_PROBE_HERMES   default `127.0.0.1:8642` (file: `{hosts.hermes}:{ports.hermes}`)
//!   DESK_PROBE_ONE      default `127.0.0.1:8002` (galion-one mouth)
//!   DESK_PROBE_TUNNEL   default `127.0.0.1:20241` (cloudflared metrics)
//!   DESK_PROBE_CONSOLE  default `127.0.0.1:8080` (file: `127.0.0.1:{ports.console}`)
//!   DESK_PROBE_ORIGIN   default `127.0.0.1:80`
//!   DESK_PROBE_ROOM     default `127.0.0.1:4020`
//!   DESK_PROBE_TULU     default `127.0.0.1:8093`

use openmind_config::ConfigError;
use std::path::{Path, PathBuf};

/// Today's addresses and paths. Used when the config file is missing.
pub struct Defaults {
    pub bind: &'static str,
    pub ollama: &'static str,
    pub state_dir: &'static str,
    pub one_dir: &'static str,
    pub hermes_dir: &'static str,
    pub hermes_bin: &'static str,
}

pub const DEFAULTS: Defaults = Defaults {
    bind: "127.0.0.1:8646",
    ollama: "127.0.0.1:11434",
    state_dir: "/opt/galion/galion-light/state",
    one_dir: "/workspace/galion-light/one",
    hermes_dir: "/opt/galion/hermes",
    hermes_bin: "/opt/galion/rust/bin/desk-hermes",
};

/// Probe targets (`host:port`). Defaults are the live loopback ports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probes {
    pub hermes: String,
    pub one: String,
    pub tunnel: String,
    pub console: String,
    pub origin: String,
    pub room: String,
    pub tulu: String,
}

impl Default for Probes {
    fn default() -> Self {
        Probes {
            hermes: "127.0.0.1:8642".into(),
            one: "127.0.0.1:8002".into(),
            tunnel: "127.0.0.1:20241".into(),
            console: "127.0.0.1:8080".into(),
            origin: "127.0.0.1:80".into(),
            room: "127.0.0.1:4020".into(),
            tulu: "127.0.0.1:8093".into(),
        }
    }
}

/// Resolved listen address, Ollama address, state roots and probes.
#[derive(Clone, Debug)]
pub struct Settings {
    pub bind: String,
    pub ollama: String,
    pub state_dir: PathBuf,
    pub one_dir: PathBuf,
    pub hermes_dir: PathBuf,
    pub hermes_bin: PathBuf,
    pub probes: Probes,
}

impl Settings {
    pub fn from_defaults() -> Self {
        Settings {
            bind: DEFAULTS.bind.to_string(),
            ollama: DEFAULTS.ollama.to_string(),
            state_dir: PathBuf::from(DEFAULTS.state_dir),
            one_dir: PathBuf::from(DEFAULTS.one_dir),
            hermes_dir: PathBuf::from(DEFAULTS.hermes_dir),
            hermes_bin: PathBuf::from(DEFAULTS.hermes_bin),
            probes: Probes::default(),
        }
    }
}

/// `$OPENMIND_CONFIG` when set and non-empty, otherwise the crate default path.
fn config_file_path(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    match env("OPENMIND_CONFIG") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(openmind_config::DEFAULT_PATH),
    }
}

/// `Ok(false)` when the file is absent (today's defaults apply). A directory
/// is treated as absent. Any other metadata error is reported.
fn config_file_present(path: &Path) -> Result<bool, ConfigError> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(ConfigError::Read {
            file: path.to_path_buf(),
            message: err.to_string(),
        }),
    }
}

/// Accept `host:port` or an `http(s)` URL and return `host:port`.
pub fn normalize_addr(value: &str) -> String {
    let trimmed = value.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let stripped = without_scheme.trim_end_matches('/');
    if stripped.is_empty() {
        trimmed.to_string()
    } else {
        stripped.to_string()
    }
}

fn trim_dir(value: &str) -> String {
    let trimmed = value.trim();
    let stripped = trimmed.trim_end_matches('/');
    if stripped.is_empty() {
        trimmed.to_string()
    } else {
        stripped.to_string()
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn apply_desk_env(settings: &mut Settings, env: &dyn Fn(&str) -> Option<String>) {
    if let Some(value) = env("DESK_BIND").as_deref().and_then(non_empty) {
        settings.bind = value.to_string();
    }
    if let Some(value) = env("DESK_OLLAMA").as_deref().and_then(non_empty) {
        settings.ollama = normalize_addr(value);
    }
    if let Some(value) = env("DESK_STATE_DIR").as_deref().and_then(non_empty) {
        settings.state_dir = PathBuf::from(trim_dir(value));
    }
    if let Some(value) = env("DESK_ONE_DIR").as_deref().and_then(non_empty) {
        settings.one_dir = PathBuf::from(trim_dir(value));
    }
    if let Some(value) = env("DESK_HERMES_DIR").as_deref().and_then(non_empty) {
        settings.hermes_dir = PathBuf::from(trim_dir(value));
    }
    // Unset keeps the default. A set value, including empty, is used as-is,
    // matching the previous `std::env::var("DESK_HERMES_BIN").unwrap_or_else`.
    if let Some(value) = env("DESK_HERMES_BIN") {
        settings.hermes_bin = PathBuf::from(value);
    }
    let probes = &mut settings.probes;
    for (key, slot) in [
        ("DESK_PROBE_HERMES", &mut probes.hermes),
        ("DESK_PROBE_ONE", &mut probes.one),
        ("DESK_PROBE_TUNNEL", &mut probes.tunnel),
        ("DESK_PROBE_CONSOLE", &mut probes.console),
        ("DESK_PROBE_ORIGIN", &mut probes.origin),
        ("DESK_PROBE_ROOM", &mut probes.room),
        ("DESK_PROBE_TULU", &mut probes.tulu),
    ] {
        if let Some(value) = env(key).as_deref().and_then(non_empty) {
            *slot = normalize_addr(value);
        }
    }
}

/// Precedence: `DESK_*`, then a present config file (with `OPENMIND_*`
/// already applied by `openmind-config`), then [`DEFAULTS`] when that file
/// is missing.
pub fn resolve(env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, ConfigError> {
    let mut settings = Settings::from_defaults();
    let path = config_file_path(env);
    if config_file_present(&path)? {
        let loaded = openmind_config::load_from(Some(&path), env)?;
        settings.bind = format!("127.0.0.1:{}", loaded.ports.desk);
        settings.ollama = format!("{}:{}", loaded.hosts.ollama.trim(), loaded.ports.ollama);
        settings.state_dir = loaded.paths.base.clone();
        settings.one_dir = loaded.paths.state_file("one");
        settings.hermes_dir = loaded.paths.state_file("hermes");
        settings.probes.hermes = format!("{}:{}", loaded.hosts.hermes.trim(), loaded.ports.hermes);
        settings.probes.console = format!("127.0.0.1:{}", loaded.ports.console);
    }
    apply_desk_env(&mut settings, env);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out_dir() -> PathBuf {
        let dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("test-out/galion-desk-config");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    fn write_fixture(name: &str, body: &str) -> PathBuf {
        let path = out_dir().join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn missing_config_file_keeps_defaults_and_ignores_openmind_env() {
        let missing = out_dir().join("does-not-exist.toml");
        let _ = std::fs::remove_file(&missing);
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "evil.example.invalid"),
            ("OPENMIND_PORTS_DESK", "1"),
            ("OPENMIND_PATHS_BASE", "/tmp/not-today"),
            ("DESK_BIND", "   "),
            ("DESK_PROBE_HERMES", "  "),
        ]))
        .unwrap();
        assert_eq!(settings.bind, DEFAULTS.bind);
        assert_eq!(settings.ollama, DEFAULTS.ollama);
        assert_eq!(settings.state_dir, PathBuf::from(DEFAULTS.state_dir));
        assert_eq!(settings.one_dir, PathBuf::from(DEFAULTS.one_dir));
        assert_eq!(settings.hermes_dir, PathBuf::from(DEFAULTS.hermes_dir));
        assert_eq!(settings.hermes_bin, PathBuf::from(DEFAULTS.hermes_bin));
        assert_eq!(settings.probes, Probes::default());
        // The live ports, exactly as the Oct 10 binary hard-coded them.
        assert_eq!(settings.probes.hermes, "127.0.0.1:8642");
        assert_eq!(settings.probes.one, "127.0.0.1:8002");
        assert_eq!(settings.probes.tunnel, "127.0.0.1:20241");
        assert_eq!(settings.probes.console, "127.0.0.1:8080");
        assert_eq!(settings.probes.origin, "127.0.0.1:80");
        assert_eq!(settings.probes.room, "127.0.0.1:4020");
        assert_eq!(settings.probes.tulu, "127.0.0.1:8093");
        assert_eq!(
            settings.one_dir.join("voice"),
            PathBuf::from("/workspace/galion-light/one/voice")
        );
        assert_eq!(
            settings.hermes_dir.join("gateway_state.json"),
            PathBuf::from("/opt/galion/hermes/gateway_state.json")
        );
    }

    #[test]
    fn config_fixture_supplies_bind_ollama_state_paths_and_probes() {
        let base = out_dir().join("unit-base");
        let path = write_fixture(
            "unit-fixture.toml",
            &format!(
                "\
[hosts]
ollama = \"ollama.example.invalid\"
hermes = \"hermes.example.invalid\"
[ports]
ollama = 14001
desk = 14002
hermes = 14003
console = 14004
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let settings = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap();
        assert_eq!(settings.bind, "127.0.0.1:14002");
        assert_eq!(settings.ollama, "ollama.example.invalid:14001");
        assert_eq!(settings.state_dir, base);
        assert_eq!(settings.one_dir, base.join("one"));
        assert_eq!(settings.hermes_dir, base.join("hermes"));
        assert_eq!(settings.hermes_bin, PathBuf::from(DEFAULTS.hermes_bin));
        assert_eq!(settings.probes.hermes, "hermes.example.invalid:14003");
        assert_eq!(settings.probes.console, "127.0.0.1:14004");
        assert_eq!(settings.probes.one, Probes::default().one);
    }

    #[test]
    fn openmind_env_wins_over_config_file() {
        let base = out_dir().join("unit-file-base");
        let path = write_fixture(
            "unit-env-file.toml",
            &format!(
                "\
[hosts]
ollama = \"file.example.invalid\"
[ports]
ollama = 1
desk = 2
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let overridden = out_dir().join("unit-env-base");
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "env.example.invalid"),
            ("OPENMIND_PORTS_OLLAMA", "9"),
            ("OPENMIND_PORTS_DESK", "8"),
            ("OPENMIND_PATHS_BASE", overridden.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:8");
        assert_eq!(settings.ollama, "env.example.invalid:9");
        assert_eq!(settings.state_dir, overridden);
        assert_eq!(settings.one_dir, overridden.join("one"));
        assert_eq!(settings.hermes_dir, overridden.join("hermes"));
    }

    #[test]
    fn desk_env_wins_over_file_and_openmind_env() {
        let base = out_dir().join("unit-desk-base");
        let path = write_fixture(
            "unit-desk-file.toml",
            &format!(
                "\
[hosts]
ollama = \"file.example.invalid\"
[ports]
ollama = 1
desk = 2
[paths]
base = '{base}'
",
                base = base.display()
            ),
        );
        let state_dir = out_dir().join("env-state");
        let one_dir = out_dir().join("env-one");
        let hermes_dir = out_dir().join("env-hermes");
        let settings = resolve(&env_from(&[
            ("OPENMIND_CONFIG", path.to_str().unwrap()),
            ("OPENMIND_HOSTS_OLLAMA", "openmind.example.invalid"),
            ("OPENMIND_PORTS_OLLAMA", "9"),
            ("OPENMIND_PORTS_DESK", "8"),
            ("OPENMIND_PATHS_BASE", "/tmp/openmind-base"),
            ("DESK_BIND", "127.0.0.1:18646"),
            ("DESK_OLLAMA", "http://127.0.0.1:1/"),
            ("DESK_STATE_DIR", state_dir.to_str().unwrap()),
            ("DESK_ONE_DIR", one_dir.to_str().unwrap()),
            ("DESK_HERMES_DIR", hermes_dir.to_str().unwrap()),
            ("DESK_HERMES_BIN", "/bin/false"),
            ("DESK_PROBE_HERMES", "http://127.0.0.1:19182/"),
            ("DESK_PROBE_ONE", "127.0.0.1:19183"),
            ("DESK_PROBE_TUNNEL", "127.0.0.1:19189"),
            ("DESK_PROBE_CONSOLE", "127.0.0.1:19189"),
            ("DESK_PROBE_ORIGIN", "127.0.0.1:19189"),
            ("DESK_PROBE_ROOM", "127.0.0.1:19189"),
            ("DESK_PROBE_TULU", "127.0.0.1:19189"),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:18646");
        assert_eq!(settings.ollama, "127.0.0.1:1");
        assert_eq!(settings.state_dir, state_dir);
        assert_eq!(settings.one_dir, one_dir);
        assert_eq!(settings.hermes_dir, hermes_dir);
        assert_eq!(settings.hermes_bin, PathBuf::from("/bin/false"));
        assert_eq!(settings.probes.hermes, "127.0.0.1:19182");
        assert_eq!(settings.probes.one, "127.0.0.1:19183");
        assert_eq!(settings.probes.tulu, "127.0.0.1:19189");

        let missing = out_dir().join("also-missing.toml");
        let _ = std::fs::remove_file(&missing);
        let from_env = resolve(&env_from(&[
            ("OPENMIND_CONFIG", missing.to_str().unwrap()),
            ("OPENMIND_PATHS_BASE", "/tmp/ignored"),
            ("DESK_BIND", "127.0.0.1:18646"),
            ("DESK_OLLAMA", "https://10.0.0.8:11434"),
            ("DESK_STATE_DIR", "/tmp/desk-state/"),
        ]))
        .unwrap();
        assert_eq!(from_env.bind, "127.0.0.1:18646");
        assert_eq!(from_env.ollama, "10.0.0.8:11434");
        assert_eq!(from_env.state_dir, PathBuf::from("/tmp/desk-state"));
        assert_eq!(from_env.one_dir, PathBuf::from(DEFAULTS.one_dir));
        assert_eq!(from_env.hermes_dir, PathBuf::from(DEFAULTS.hermes_dir));
    }

    #[test]
    fn bad_config_file_is_an_error() {
        let path = write_fixture("unit-bad.toml", "[ports]\nollama = \"nope\"\n");
        let err = resolve(&env_from(&[("OPENMIND_CONFIG", path.to_str().unwrap())])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unit-bad.toml"), "{msg}");
    }

    #[test]
    fn normalize_addr_strips_scheme_and_slash() {
        assert_eq!(normalize_addr("http://h:1/"), "h:1");
        assert_eq!(normalize_addr(" https://h:2 "), "h:2");
        assert_eq!(normalize_addr("h:3"), "h:3");
        assert_eq!(normalize_addr("/"), "/");
    }
}
