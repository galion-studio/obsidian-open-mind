// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! The contract for the Obsidian desk.
//!
//! The processor owns the loop, the skills, the memory, and the key box.
//! Video memory is only for a model. A provider is an adapter, not a secret.

use serde::{Deserialize, Serialize};

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

impl Stack {
    pub fn cpu_desk() -> Self {
        Self {
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
                    base_url: "http://127.0.0.1:11434/v1".into(),
                    key_slot: "laptop".into(),
                    default_model: "huihui_ai/qwen3.5-abliterated:9b".into(),
                },
                Provider {
                    id: "openrouter".into(),
                    kind: ProviderKind::Remote,
                    base_url: "https://openrouter.ai/api/v1".into(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
