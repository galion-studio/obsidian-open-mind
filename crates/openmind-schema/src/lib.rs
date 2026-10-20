// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The contract for the Obsidian desk.
//!
//! The processor owns the loop, the skills, the memory, and the key box.
//! Video memory is only for a model. A provider is an adapter, not a secret.
//! `Stack::cpu_desk` builds provider `base_url` values from the OpenMind
//! config provider list instead of a fixed pair.
//! A [`NerveEvent`] is one nerve firing: `ts`, `source`, `kind`, `severity`,
//! and `data`. `ts` is Europe/Zagreb RFC 3339 from `zagreb-time`.

use openmind_config::{Config, ConfigError};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every timestamp in the stack, re-exported as `openmind_schema::time`.
///
/// `zagreb-time` owns the Europe/Zagreb RFC 3339 formatting (explicit offset,
/// never `Z`). The schema re-exports it so consumers of the contract get the
/// one shared time helper without a second dependency line.
pub use zagreb_time as time;

/// One nerve firing.
///
/// `ts` is Europe/Zagreb RFC 3339, seconds precision, with an explicit numeric
/// offset (`+02:00` in summer, `+01:00` in winter). Never `Z`, never `+00:00`.
/// [`NerveEvent::now`] and [`NerveEvent::at_unix`] stamp it through [`time`]
/// (`zagreb-time`). `data` is a JSON value, usually an object of counts and
/// short tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NerveEvent {
    #[serde(deserialize_with = "deserialize_zagreb_ts")]
    pub ts: String,
    pub source: String,
    pub kind: String,
    pub severity: String,
    pub data: serde_json::Value,
}

impl NerveEvent {
    /// Current instant as Europe/Zagreb RFC 3339.
    pub fn now(
        source: impl Into<String>,
        kind: impl Into<String>,
        severity: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self::stamped(time::now_rfc3339(), source, kind, severity, data)
    }

    /// Unix second as Europe/Zagreb RFC 3339, the same format as [`Self::now`].
    ///
    /// The 2026-10-25 fold is owned by `zagreb-time`: `2026-10-25T00:59:59Z`
    /// is `2026-10-25T02:59:59+02:00`, and `2026-10-25T01:00:00Z` is
    /// `2026-10-25T02:00:00+01:00`.
    pub fn at_unix(
        secs: i64,
        source: impl Into<String>,
        kind: impl Into<String>,
        severity: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self::stamped(time::from_unix(secs), source, kind, severity, data)
    }

    fn stamped(
        ts: String,
        source: impl Into<String>,
        kind: impl Into<String>,
        severity: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        debug_assert!(
            is_zagreb_rfc3339(&ts),
            "zagreb-time produced a non-Zagreb stamp: {ts}"
        );
        Self {
            ts,
            source: source.into(),
            kind: kind.into(),
            severity: severity.into(),
            data,
        }
    }
}

/// `YYYY-MM-DDTHH:MM:SS+01:00` or `+02:00`. Seconds precision, never `Z`.
fn is_zagreb_rfc3339(ts: &str) -> bool {
    let bytes = ts.as_bytes();
    if bytes.len() != 25 {
        return false;
    }
    if !(bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'+'
        && bytes[22] == b':')
    {
        return false;
    }
    let digit_at = |index: usize| bytes[index].is_ascii_digit();
    let digits_ok = (0..4).all(digit_at)
        && (5..7).all(digit_at)
        && (8..10).all(digit_at)
        && (11..13).all(digit_at)
        && (14..16).all(digit_at)
        && (17..19).all(digit_at)
        && (20..22).all(digit_at)
        && (23..25).all(digit_at);
    if !digits_ok {
        return false;
    }
    let month = two_digits(bytes, 5);
    let day = two_digits(bytes, 8);
    let hour = two_digits(bytes, 11);
    let minute = two_digits(bytes, 14);
    let second = two_digits(bytes, 17);
    (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..=23).contains(&hour)
        && (0..=59).contains(&minute)
        && (0..=59).contains(&second)
        && (ts.ends_with("+01:00") || ts.ends_with("+02:00"))
}

fn two_digits(bytes: &[u8], index: usize) -> u16 {
    u16::from(bytes[index] - b'0') * 10 + u16::from(bytes[index + 1] - b'0')
}

fn deserialize_zagreb_ts<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let ts = String::deserialize(deserializer)?;
    if is_zagreb_rfc3339(&ts) {
        Ok(ts)
    } else {
        Err(D::Error::custom(
            "ts must be Europe/Zagreb RFC 3339 seconds with offset +01:00 or +02:00",
        ))
    }
}

/// What this machine is allowed to start. Measured, never assumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub studio: String,
    pub stack: String,
    pub stance: String,
    pub ram_mib: u64,
    pub gpu: bool,
    pub vram_mib: u64,
    pub public_ip: String,
}

impl Machine {
    /// Local weights only when a card has at least 8 GB free and the box has 8 GB of memory.
    pub fn weights(&self) -> bool {
        self.gpu && self.vram_mib >= 8_192 && self.ram_mib >= 8_192
    }

    /// Speech and the larger local pieces need 8 GB of ordinary memory. They do not need a card.
    pub fn mouth(&self) -> bool {
        self.ram_mib >= 8_192
    }
}

/// Where a model lives. The desk never embeds the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// A key to a site that already has the card. OpenRouter and the others.
    Remote,
    /// The laptop, or any Ollama the admin pointed at.
    Local,
}

/// One adapter. Adding a site is a new row, not a new program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// Name of the slot in the key box. The schema never holds the secret.
    pub key_slot: String,
    pub default_model: String,
}

/// What the phone is allowed to show. The secret stays in the key box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySlot {
    pub name: String,
    pub present: bool,
    pub hint: String,
}

/// The four things the admin must be able to open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Surface {
    pub console: bool,
    pub desk: bool,
    pub api: bool,
    pub hermes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    pub machine: Machine,
    pub providers: Vec<Provider>,
    pub keys: Vec<KeySlot>,
    pub surface: Surface,
}

/// OpenRouter base when the provider list has no `openrouter` row.
/// `openmind-config` types hosts and ports, not this remote site.
const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

/// `[providers.<id>]` rows in the same TOML file `openmind-config` loads.
/// The config crate ignores unknown tables, so the schema reads this list.
#[derive(Deserialize)]
struct ProviderListFile {
    #[serde(default)]
    providers: BTreeMap<String, ProviderBaseRow>,
}

#[derive(Deserialize)]
struct ProviderBaseRow {
    #[serde(default)]
    base_url: String,
}

impl Stack {
    /// CPU desk stack.
    ///
    /// Provider `base_url` values come from the OpenMind config provider list
    /// (`[providers.<id>].base_url`). A row in that list wins. Otherwise the
    /// laptop URL is `http://{hosts.ollama}:{ports.ollama}/v1` from
    /// `openmind-config`, and OpenRouter stays [`OPENROUTER_BASE`].
    ///
    /// The file is `$OPENMIND_CONFIG`, or `/etc/openmind/openmind.toml`.
    /// When that file is missing, `OPENMIND_*` is not consulted and the base
    /// URLs are `http://127.0.0.1:11434/v1` and `https://openrouter.ai/api/v1`.
    pub fn cpu_desk() -> Self {
        match Self::cpu_desk_from_env(&|key| std::env::var(key).ok()) {
            Ok(stack) => stack,
            Err(err) => panic!("openmind-schema cpu_desk: {err}"),
        }
    }

    fn cpu_desk_from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let path = config_path(env);
        if !config_file_present(&path)? {
            return Ok(stack_from(&Config::default(), &BTreeMap::new()));
        }
        let text = std::fs::read_to_string(&path).map_err(|err| ConfigError::Read {
            file: path.clone(),
            message: err.to_string(),
        })?;
        let mut cfg = Config::from_toml(&text, &path)?;
        cfg.apply_env(env)?;
        let urls = provider_base_urls(&text, &path)?;
        Ok(stack_from(&cfg, &urls))
    }
}

/// `$OPENMIND_CONFIG` when set and non-empty, otherwise the config crate default.
fn config_path(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    match env("OPENMIND_CONFIG") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(openmind_config::DEFAULT_PATH),
    }
}

/// `Ok(false)` when the file is absent. A directory counts as absent.
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

fn provider_base_urls(text: &str, file: &Path) -> Result<BTreeMap<String, String>, ConfigError> {
    let parsed: ProviderListFile = toml::from_str(text).map_err(|err| ConfigError::Parse {
        file: file.to_path_buf(),
        message: err.to_string(),
    })?;
    let mut urls = BTreeMap::new();
    for (id, row) in parsed.providers {
        let base_url = row.base_url.trim();
        if !base_url.is_empty() {
            urls.insert(id, base_url.to_string());
        }
    }
    Ok(urls)
}

fn ollama_v1(cfg: &Config) -> String {
    let host = cfg.hosts.ollama.trim();
    let port = cfg.ports.ollama;
    format!("http://{host}:{port}/v1")
}

fn stack_from(cfg: &Config, urls: &BTreeMap<String, String>) -> Stack {
    let laptop_base = urls
        .get("laptop")
        .cloned()
        .unwrap_or_else(|| ollama_v1(cfg));
    let openrouter_base = urls
        .get("openrouter")
        .cloned()
        .unwrap_or_else(|| OPENROUTER_BASE.to_string());
    Stack {
        machine: Machine {
            studio: "Galion Studio".into(),
            stack: "Obsidian".into(),
            stance: "Open Mind".into(),
            ram_mib: 16_384,
            gpu: false,
            vram_mib: 0,
            public_ip: String::new(),
        },
        providers: vec![
            Provider {
                id: "laptop".into(),
                kind: ProviderKind::Local,
                base_url: laptop_base,
                key_slot: "laptop".into(),
                default_model: "huihui_ai/qwen3.5-abliterated:9b".into(),
            },
            Provider {
                id: "openrouter".into(),
                kind: ProviderKind::Remote,
                base_url: openrouter_base,
                key_slot: "openrouter".into(),
                default_model: String::new(),
            },
        ],
        keys: vec![
            KeySlot {
                name: "laptop".into(),
                present: false,
                hint: String::new(),
            },
            KeySlot {
                name: "openrouter".into(),
                present: false,
                hint: String::new(),
            },
        ],
        surface: Surface {
            console: true,
            desk: true,
            api: true,
            hermes: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(name: &str, body: &str) -> PathBuf {
        // Per target dir (next to this test binary: <target>/<profile>/test-out),
        // not per source tree: copies of the suite with their own
        // CARGO_TARGET_DIR may run from one checkout at once.
        let dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("test-out")
            .join("openmind-schema");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn env_for<'a>(
        path: &'a Path,
        extra: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            if key == "OPENMIND_CONFIG" {
                return Some(path.display().to_string());
            }
            extra
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        }
    }

    fn base_url(stack: &Stack, id: &str) -> String {
        stack
            .providers
            .iter()
            .find(|provider| provider.id == id)
            .unwrap()
            .base_url
            .clone()
    }

    #[test]
    fn a_cpu_box_does_not_turn_weights_on() {
        let stack = Stack::cpu_desk();
        assert!(!stack.machine.weights());
        assert!(stack.machine.mouth());
        assert!(
            stack.surface.hermes
                && stack.surface.desk
                && stack.surface.console
                && stack.surface.api
        );
        let raw = serde_json::to_string(&stack).unwrap();
        assert!(!raw.contains("sk-"));
    }

    #[test]
    fn missing_config_file_keeps_builtin_base_urls() {
        let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-out/openmind-schema/does-not-exist.toml");
        let stack = Stack::cpu_desk_from_env(&env_for(
            &missing,
            &[
                ("OPENMIND_HOSTS_OLLAMA", "ignored.example.invalid"),
                ("OPENMIND_PORTS_OLLAMA", "1"),
            ],
        ))
        .unwrap();
        assert_eq!(base_url(&stack, "laptop"), "http://127.0.0.1:11434/v1");
        assert_eq!(
            base_url(&stack, "openrouter"),
            "https://openrouter.ai/api/v1"
        );
        let raw = serde_json::to_string(&stack).unwrap();
        assert!(!raw.contains("sk-"));
    }

    #[test]
    fn example_config_uses_loopback_defaults() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openmind-config/openmind.example.toml");
        let stack = Stack::cpu_desk_from_env(&env_for(&path, &[])).unwrap();
        assert_eq!(base_url(&stack, "laptop"), "http://127.0.0.1:11434/v1");
        assert_eq!(
            base_url(&stack, "openrouter"),
            "https://openrouter.ai/api/v1"
        );
    }

    #[test]
    fn hosts_and_env_build_laptop_base_url() {
        let path = write_fixture(
            "hosts.toml",
            "[hosts]\nollama = \"file.example.invalid\"\n[ports]\nollama = 1\n",
        );
        let from_file = Stack::cpu_desk_from_env(&env_for(&path, &[])).unwrap();
        assert_eq!(
            base_url(&from_file, "laptop"),
            "http://file.example.invalid:1/v1"
        );
        assert_eq!(
            base_url(&from_file, "openrouter"),
            "https://openrouter.ai/api/v1"
        );

        let from_env = Stack::cpu_desk_from_env(&env_for(
            &path,
            &[
                ("OPENMIND_HOSTS_OLLAMA", "env.example.invalid"),
                ("OPENMIND_PORTS_OLLAMA", "9"),
            ],
        ))
        .unwrap();
        assert_eq!(
            base_url(&from_env, "laptop"),
            "http://env.example.invalid:9/v1"
        );
    }

    #[test]
    fn provider_list_base_url_wins() {
        let path = write_fixture(
            "providers.toml",
            r#"
[hosts]
ollama = "ollama.example.invalid"
[ports]
ollama = 14001

[providers.laptop]
base_url = "  http://providers.example.invalid:9/v1  "

[providers.openrouter]
base_url = "https://router.example.invalid/api/v1"

[providers.extra]
base_url = "http://extra.example.invalid/v1"
"#,
        );
        let stack = Stack::cpu_desk_from_env(&env_for(&path, &[])).unwrap();
        assert_eq!(stack.providers.len(), 2);
        assert_eq!(
            base_url(&stack, "laptop"),
            "http://providers.example.invalid:9/v1"
        );
        assert_eq!(
            base_url(&stack, "openrouter"),
            "https://router.example.invalid/api/v1"
        );
        assert!(stack.providers.iter().all(|row| row.id != "extra"));
        let raw = serde_json::to_string(&stack).unwrap();
        assert!(!raw.contains("sk-"));
    }

    #[test]
    fn bad_config_file_is_an_error() {
        let path = write_fixture("bad.toml", "[ports]\nollama = \"nope\"\n");
        let err = Stack::cpu_desk_from_env(&env_for(&path, &[])).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("bad config"), "{message}");
    }

    /// 2026-10-25T00:59:59Z, 2026-10-25T01:00:00Z, and 2026-12-01T12:00:00Z.
    const DST_BEFORE_2026_10_25: i64 = 1_792_889_999;
    const DST_AFTER_2026_10_25: i64 = 1_792_890_000;
    const WINTER_2026_12_01: i64 = 1_796_126_400;

    fn nerve(secs: i64, source: &str, kind: &str, severity: &str) -> NerveEvent {
        NerveEvent::at_unix(
            secs,
            source,
            kind,
            severity,
            serde_json::json!({"window_s": 300, "failed_password": 30}),
        )
    }

    #[test]
    fn nerve_event_dst_2026_10_25() {
        let before = nerve(DST_BEFORE_2026_10_25, "auth.log", "ddos", "warn");
        assert_eq!(before.ts, "2026-10-25T02:59:59+02:00");
        assert_eq!(before.source, "auth.log");
        assert_eq!(before.kind, "ddos");
        assert_eq!(before.severity, "warn");
        assert_eq!(before.data["failed_password"], 30);

        let after = nerve(DST_AFTER_2026_10_25, "memory-gateway", "bleeding", "crit");
        assert_eq!(after.ts, "2026-10-25T02:00:00+01:00");
        assert_eq!(after.source, "memory-gateway");
        assert_eq!(after.kind, "bleeding");
        assert_eq!(after.severity, "crit");

        let winter = nerve(WINTER_2026_12_01, "auth.log", "vulnerability", "high");
        assert_eq!(winter.ts, "2026-12-01T13:00:00+01:00");

        for event in [&before, &after, &winter] {
            assert!(is_zagreb_rfc3339(&event.ts), "{}", event.ts);
            assert!(!event.ts.ends_with('Z'), "{}", event.ts);
            assert!(!event.ts.contains("+00:00"), "{}", event.ts);
            let raw = serde_json::to_string(event).unwrap();
            let ts_at = raw.find("\"ts\"").unwrap();
            let source_at = raw.find("\"source\"").unwrap();
            let kind_at = raw.find("\"kind\"").unwrap();
            let severity_at = raw.find("\"severity\"").unwrap();
            let data_at = raw.find("\"data\"").unwrap();
            assert!(
                ts_at < source_at
                    && source_at < kind_at
                    && kind_at < severity_at
                    && severity_at < data_at,
                "{raw}"
            );
            let back: NerveEvent = serde_json::from_str(&raw).unwrap();
            assert_eq!(event, &back);
            assert!(!raw.contains("+00:00"), "{raw}");
            assert!(!raw.contains("sk-"), "{raw}");
        }
    }

    #[test]
    fn nerve_event_rejects_non_zagreb_ts() {
        let samples = [
            r#"{"ts":"2026-10-25T00:59:59Z","source":"auth.log","kind":"ddos","severity":"warn","data":{}}"#,
            r#"{"ts":"2026-10-25T01:00:00+00:00","source":"auth.log","kind":"ddos","severity":"warn","data":{}}"#,
            r#"{"ts":"2026-10-25T02:59:59.000+02:00","source":"auth.log","kind":"ddos","severity":"warn","data":{}}"#,
            r#"{"ts":"2026-10-25T03:00:00+03:00","source":"auth.log","kind":"ddos","severity":"warn","data":{}}"#,
        ];
        for raw in samples {
            assert!(
                serde_json::from_str::<NerveEvent>(raw).is_err(),
                "accepted {raw}"
            );
        }
        let folded = r#"{"ts":"2026-10-25T02:59:59+02:00","source":"auth.log","kind":"ddos","severity":"warn","data":{"window_s":300,"rule":"failed_password"}}"#;
        let event: NerveEvent = serde_json::from_str(folded).unwrap();
        assert_eq!(event.ts, "2026-10-25T02:59:59+02:00");
        assert_eq!(event.data["window_s"], 300);
        assert_eq!(event.data["rule"], "failed_password");
    }

    #[test]
    fn nerve_event_now_is_zagreb_rfc3339() {
        let event = NerveEvent::now(
            "auth.log",
            "vulnerability",
            "warn",
            serde_json::json!({"invalid_user": 0}),
        );
        assert_eq!(event.ts.len(), 25, "{}", event.ts);
        assert!(is_zagreb_rfc3339(&event.ts), "{}", event.ts);
        assert!(
            event.ts.ends_with("+02:00") || event.ts.ends_with("+01:00"),
            "{}",
            event.ts
        );
        assert!(!event.ts.ends_with('Z'), "{}", event.ts);
        assert!(!event.ts.contains("+00:00"), "{}", event.ts);
        let raw = serde_json::to_string(&event).unwrap();
        assert!(!raw.contains("sk-"), "{raw}");
        let back: NerveEvent = serde_json::from_str(&raw).unwrap();
        assert_eq!(event, back);
    }

    /// Boot hardware JSON is the existing nerve event.
    ///
    /// Object keys are `ts`, `source` `boot`, `kind` `hardware`,
    /// `severity` `info`, and `data`. Parsing uses [`NerveEvent`] as it
    /// stands. This test does not change that contract.
    #[test]
    fn nerve_event_parses_boot_hardware() {
        let raw = "{\n  \"ts\": \"2026-10-19T16:45:00+02:00\",\n  \"source\": \"boot\",\n  \"kind\": \"hardware\",\n  \"severity\": \"info\",\n  \"data\": {\n    \"cpu\": {\n      \"processors\": 4,\n      \"model\": \"Test CPU\"\n    },\n    \"ram\": {\n      \"total_mib\": 8192,\n      \"free_mib\": 4096\n    },\n    \"gpu\": {\n      \"status\": \"absent\",\n      \"count\": 0\n    }\n  }\n}\n";
        let event: NerveEvent = serde_json::from_str(raw).expect("boot hardware nerve");
        assert_eq!(event.ts, "2026-10-19T16:45:00+02:00");
        assert!(is_zagreb_rfc3339(&event.ts), "{}", event.ts);
        assert_eq!(event.source, "boot");
        assert_eq!(event.kind, "hardware");
        assert_eq!(event.severity, "info");
        assert_eq!(event.data["cpu"]["processors"], 4);
        assert_eq!(event.data["cpu"]["model"], "Test CPU");
        assert_eq!(event.data["ram"]["total_mib"], 8192);
        assert_eq!(event.data["ram"]["free_mib"], 4096);
        assert_eq!(event.data["gpu"]["status"], "absent");
        assert_eq!(event.data["gpu"]["count"], 0);
        let again = serde_json::to_string(&event).unwrap();
        assert!(!again.contains("sk-"), "{again}");
        assert!(!again.contains("+00:00"), "{again}");
        let back: NerveEvent = serde_json::from_str(&again).unwrap();
        assert_eq!(event, back);
    }
}
