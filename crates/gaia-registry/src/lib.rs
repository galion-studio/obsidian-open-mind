// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Ownership registry for the workspace crates.
//!
//! Each crate name in [`TABLE`] maps to exactly one of the nine functions.
//! [`owner`] returns [`None`] for every name that is not in the table.
//! The nine spellings are `GAIA`, `HADES`, `HEPHAESTUS`, `DEMETER`, `ARTEMIS`,
//! `APOLLO`, `MINERVA`, `ELEUTHIA`, and `ARES`.

#![forbid(unsafe_code)]

/// One of the nine functions that can own a crate.
///
/// The wire spelling is [`Function::name`], not the Rust variant name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Function {
    /// Orchestration and the registry itself.
    Gaia,
    /// Destructive plans only. No current crate is in this domain.
    Hades,
    /// Fabrication: builds, adapters, the desk, and the sandbox runner.
    Hephaestus,
    /// Environment health: survival, config, and the routine tick.
    Demeter,
    /// Agent population.
    Artemis,
    /// Knowledge: contract, intake, and the event archive.
    Apollo,
    /// Security audit and policy. No current crate is in this domain.
    Minerva,
    /// Personas and the agents grown from them.
    Eleuthia,
    /// Contest. Paper scores only.
    Ares,
}

impl Function {
    /// The nine functions, in the assigned order.
    pub const ALL: [Function; 9] = [
        Function::Gaia,
        Function::Hades,
        Function::Hephaestus,
        Function::Demeter,
        Function::Artemis,
        Function::Apollo,
        Function::Minerva,
        Function::Eleuthia,
        Function::Ares,
    ];

    /// Spelling used in the ownership table.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Function::Gaia => "GAIA",
            Function::Hades => "HADES",
            Function::Hephaestus => "HEPHAESTUS",
            Function::Demeter => "DEMETER",
            Function::Artemis => "ARTEMIS",
            Function::Apollo => "APOLLO",
            Function::Minerva => "MINERVA",
            Function::Eleuthia => "ELEUTHIA",
            Function::Ares => "ARES",
        }
    }

    /// Parse a function spelling. Any other string, including `none`, is [`None`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "GAIA" => Some(Function::Gaia),
            "HADES" => Some(Function::Hades),
            "HEPHAESTUS" => Some(Function::Hephaestus),
            "DEMETER" => Some(Function::Demeter),
            "ARTEMIS" => Some(Function::Artemis),
            "APOLLO" => Some(Function::Apollo),
            "MINERVA" => Some(Function::Minerva),
            "ELEUTHIA" => Some(Function::Eleuthia),
            "ARES" => Some(Function::Ares),
            _ => None,
        }
    }
}

/// One row of the static ownership table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// Crate directory name, the same string `ls crates` prints.
    pub crate_name: &'static str,
    /// The single function that owns this crate.
    pub function: Function,
}

/// Every current workspace crate, once, sorted by name.
///
/// Snapshot of `ls crates` plus this crate. See `docs/gaia-ownership.md`.
pub const TABLE: &[Owner] = &[
    Owner {
        crate_name: "cgroup-detect",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "desk-hermes",
        function: Function::Artemis,
    },
    Owner {
        crate_name: "gaia-core",
        function: Function::Gaia,
    },
    Owner {
        crate_name: "gaia-registry",
        function: Function::Gaia,
    },
    Owner {
        crate_name: "galion-desk",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "galion-one",
        function: Function::Eleuthia,
    },
    Owner {
        crate_name: "galleon-arena",
        function: Function::Ares,
    },
    Owner {
        crate_name: "galleon-voice",
        function: Function::Eleuthia,
    },
    Owner {
        crate_name: "guardian-tick",
        function: Function::Demeter,
    },
    Owner {
        crate_name: "link-watch",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "one-defence",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "one-set-model",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "one-talk",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-config",
        function: Function::Demeter,
    },
    Owner {
        crate_name: "openmind-discovery",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-exo",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-fallback",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-llama",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-mcp-stack",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-nerve",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-provider",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-sandbox",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-schema",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-survival",
        function: Function::Demeter,
    },
    Owner {
        crate_name: "persona-plex-relay",
        function: Function::Eleuthia,
    },
    Owner {
        crate_name: "room",
        function: Function::Artemis,
    },
    Owner {
        crate_name: "zagreb-time",
        function: Function::Gaia,
    },
];

/// Function that owns `crate_name`, or [`None`] when the name is not in [`TABLE`].
///
/// Match is exact. Case, underscores, and surrounding space are not folded.
#[must_use]
pub fn owner(crate_name: &str) -> Option<Function> {
    TABLE
        .binary_search_by(|row| row.crate_name.cmp(crate_name))
        .ok()
        .map(|index| TABLE[index].function)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galion_one_maps_to_one_function() {
        let found = owner("galion-one");
        assert_eq!(found, Some(Function::Eleuthia));
        assert_eq!(found.expect("galion-one is owned").name(), "ELEUTHIA");
    }

    #[test]
    fn unknown_crate_maps_to_none() {
        assert_eq!(owner("not-a-crate"), None);
        assert_eq!(owner("galion_one"), None);
        assert_eq!(owner("GALION-ONE"), None);
        assert_eq!(owner(""), None);
        assert_eq!(Function::from_name("none"), None);
    }

    #[test]
    fn nine_names_are_spelled_as_assigned() {
        let names: Vec<&str> = Function::ALL.iter().copied().map(Function::name).collect();
        assert_eq!(
            names,
            [
                "GAIA",
                "HADES",
                "HEPHAESTUS",
                "DEMETER",
                "ARTEMIS",
                "APOLLO",
                "MINERVA",
                "ELEUTHIA",
                "ARES",
            ]
        );
        for function in Function::ALL {
            assert_eq!(Function::from_name(function.name()), Some(function));
        }
        assert_eq!(Function::from_name("AETHER"), None);
        assert_eq!(Function::from_name("POSEIDON"), None);
    }

    #[test]
    fn table_is_sorted_and_each_crate_once() {
        let mut previous = "";
        for row in TABLE {
            assert!(
                row.crate_name > previous,
                "{} is not strictly after {previous}",
                row.crate_name
            );
            previous = row.crate_name;
            assert_eq!(owner(row.crate_name), Some(row.function));
            assert!(Function::ALL.contains(&row.function));
        }
        assert_eq!(TABLE.len(), 27);
    }
}
