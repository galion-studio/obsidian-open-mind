//! The contract for the Obsidian desk.
//!
//! The processor owns the loop, the skills, the memory, and the key box.
//! Video memory is only for a model. A provider is an adapter, not a secret.
//! `Stack::cpu_desk` builds provider `base_url` values from the OpenMind
//! config provider list instead of a fixed pair.

use openmind_config::{Config, ConfigError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every timestamp in the stack, re-exported as `openmind_schema::time`.
///
/// `zagreb-time` owns the Europe/Zagreb RFC 3339 formatting (explicit offset,
/// never `Z`). The schema re-exports it so consumers of the contract get the
/// one shared time helper without a second dependency line.
pub use zagreb_time as time;

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
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/openmind-schema");
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
}
