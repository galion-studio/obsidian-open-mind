// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The stack list shown beside the command field.
//!
//! States use the brand words: `staged` (built, not switched on) and
//! `planned` (not started by this page). Nothing here is marked live.
//! This list is not a health check. Live checks are [`crate::probe`], and
//! they run only when the operator presses Look. That button does not
//! change these states. It writes the probe word into the row's `.seen`
//! span when the probe service matches `id`, and leaves `state` as it is.

/// One row of the stack view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    /// Stable id (`data-layer`).
    pub id: &'static str,
    /// Name shown to the operator.
    pub name: &'static str,
    /// One line on what this desktop does with it.
    pub role: &'static str,
    /// `staged` or `planned`. Never `live`.
    pub state: &'static str,
}

/// Open Mind first. The rest are the stack this console is for.
pub const LAYERS: &[Layer] = &[
    Layer {
        id: "open-mind",
        name: "Open Mind",
        role: "Main presence on this desktop",
        state: "staged",
    },
    Layer {
        id: "command",
        name: "Command",
        role: "Centre field. Text stays on this page",
        state: "staged",
    },
    Layer {
        id: "files",
        name: "Files",
        role: "Plus picks files. Read and upload stay on loopback",
        state: "staged",
    },
    Layer {
        id: "voice",
        name: "Voice",
        role: "Opt-in microphone on loopback. Audio is not saved",
        state: "staged",
    },
    Layer {
        id: "galion-one",
        name: "Galion One",
        role: "Voice surface. Not started by this page",
        state: "planned",
    },
    Layer {
        id: "hermes",
        name: "Hermes",
        role: "Engine. Not started by this page",
        state: "planned",
    },
    Layer {
        id: "memory",
        name: "Memory",
        role: "Long-term store. Not started by this page",
        state: "planned",
    },
    Layer {
        id: "nerve",
        name: "Nerve",
        role: "Health monitor. Not started by this page",
        state: "planned",
    },
    Layer {
        id: "desk",
        name: "Desk",
        role: "Read-only pod endpoint, separate program",
        state: "staged",
    },
    Layer {
        id: "console",
        name: "Console",
        role: "Operator rooms, separate program",
        state: "staged",
    },
];

#[cfg(test)]
mod tests {
    use super::LAYERS;

    #[test]
    fn open_mind_leads_and_nothing_is_live() {
        assert_eq!(LAYERS[0].id, "open-mind");
        assert_eq!(LAYERS[0].name, "Open Mind");
        for layer in LAYERS {
            assert!(layer.state == "staged" || layer.state == "planned");
            assert_ne!(layer.state, "live");
        }
    }
}
