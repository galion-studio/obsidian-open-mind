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
    /// Destructive plans only. The gate classifies them and runs nothing.
    Hades,
    /// Fabrication: builds, adapters, the desk, the redirect, the tuner, and the sandbox runner.
    Hephaestus,
    /// Environment health: survival, config, hardware, staged units, and the routine tick.
    Demeter,
    /// Agent population.
    Artemis,
    /// Knowledge: contract, intake, memory, the Tulu row, and the event archive.
    Apollo,
    /// Security audit and policy. The read-only stack status stub.
    Minerva,
    /// Personas, speech fixtures, and the agents grown from them.
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
/// Snapshot of `ls crates`. See `docs/gaia-ownership.md`.
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
        crate_name: "hades-gate",
        function: Function::Hades,
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
        crate_name: "openmind-boot",
        function: Function::Demeter,
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
        function: Function::Minerva,
    },
    Owner {
        crate_name: "openmind-memory",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-nerve",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-origin",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-provider",
        function: Function::Hephaestus,
    },
    Owner {
        crate_name: "openmind-route",
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
        crate_name: "openmind-speech",
        function: Function::Eleuthia,
    },
    Owner {
        crate_name: "openmind-survival",
        function: Function::Demeter,
    },
    Owner {
        crate_name: "openmind-tulu",
        function: Function::Apollo,
    },
    Owner {
        crate_name: "openmind-units",
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
        crate_name: "tuner",
        function: Function::Hephaestus,
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
        assert_eq!(Function::from_name("unknown"), None);
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
    fn suggested_map_holds() {
        assert_eq!(owner("hades-gate"), Some(Function::Hades));
        assert_eq!(owner("tuner"), Some(Function::Hephaestus));
        assert_eq!(owner("openmind-boot"), Some(Function::Demeter));
        assert_eq!(owner("openmind-units"), Some(Function::Demeter));
        assert_eq!(owner("openmind-origin"), Some(Function::Hephaestus));
        assert_eq!(owner("openmind-tulu"), Some(Function::Apollo));
        assert_eq!(owner("openmind-speech"), Some(Function::Eleuthia));
        assert_eq!(owner("openmind-memory"), Some(Function::Apollo));
        assert_eq!(owner("openmind-mcp-stack"), Some(Function::Minerva));
        assert_eq!(owner("persona-plex-relay"), Some(Function::Eleuthia));
        assert_eq!(owner("openmind-route"), Some(Function::Hephaestus));
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
        assert_eq!(TABLE.len(), 36);
    }

    #[test]
    fn table_matches_crate_directory() {
        let crates_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&crates_dir).expect("read crates") {
            let entry = entry.expect("crate entry");
            let raw = entry.file_name();
            let name = raw.to_str().expect("crate name is utf-8");
            if !name.starts_with('.') {
                names.push(name.to_string());
            }
        }
        names.sort();
        let table: Vec<String> = TABLE.iter().map(|row| row.crate_name.to_string()).collect();
        assert_eq!(table, names);
    }
}
