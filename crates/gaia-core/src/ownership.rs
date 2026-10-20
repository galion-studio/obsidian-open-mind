// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The Gaia ownership registry: every workspace crate and every known live
//! service unit, mapped to one [`System`] and therefore one [`FunctionId`].
//!
//! This replaces the older nine-name list of `gaia-registry` (GAIA, HADES,
//! HEPHAESTUS, DEMETER, ARTEMIS, APOLLO, MINERVA, ELEUTHIA, ARES). That list
//! counted GAIA itself as a function and had an ARES function, and it had no
//! POSEIDON and no AETHER. The 52-system design in `gaia/docs/gaia-design.md`
//! wins (see `docs/gaia-ownership.md` for the decision record):
//!
//! * GAIA core owns no system. Only `gaia-core` and the `gaia` CLI are
//!   [`CrateRole::Core`].
//! * Each of the nine function crates (`gaia-hades` ... `gaia-eleuthia`) is
//!   [`CrateRole::Function`]: it implements that function's handlers.
//! * Every other crate implements part of exactly one [`System`]
//!   ([`CrateRole::System`]); its function is that system's owner.
//!
//! Pure data: no file I/O. The test `tests/workspace_ownership.rs` reads
//! `crates/*/Cargo.toml` and fails if any workspace crate is missing here.

use crate::json::Json;
use crate::system::{FunctionId, System};

/// What a crate is to Gaia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrateRole {
    /// GAIA core or its CLI: orchestration and the master override only.
    Core,
    /// The crate that implements one subordinate function.
    Function(FunctionId),
    /// The crate implements (part of) one system.
    System(System),
}

impl CrateRole {
    /// The owning function; `None` for GAIA core, which owns no system.
    pub fn function(self) -> Option<FunctionId> {
        match self {
            CrateRole::Core => None,
            CrateRole::Function(f) => Some(f),
            CrateRole::System(s) => Some(s.function()),
        }
    }

    pub fn system(self) -> Option<System> {
        match self {
            CrateRole::System(s) => Some(s),
            _ => None,
        }
    }

    pub fn kind(self) -> &'static str {
        match self {
            CrateRole::Core => "core",
            CrateRole::Function(_) => "function",
            CrateRole::System(_) => "system",
        }
    }
}

/// One row of the crate table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrateOwner {
    /// Package name, exactly as in `crates/<name>/Cargo.toml`.
    pub name: &'static str,
    pub role: CrateRole,
    /// Why it sits there (one line).
    pub why: &'static str,
}

impl CrateOwner {
    pub fn to_json(&self) -> Json {
        Json::obj()
            .with("crate", self.name)
            .with("kind", self.role.kind())
            .with("system", self.role.system().map(System::name))
            .with(
                "function",
                self.role.function().map(FunctionId::name).unwrap_or("GAIA"),
            )
            .with("why", self.why)
    }
}

use CrateRole::{Core, Function as F, System as S};

/// Every workspace crate, alphabetical. 35 crates of main 13a296c, the
/// 11 Gaia crates from `ws/gaia`, and hermes-core (swarm/hermes-core).
pub const CRATES: &[CrateOwner] = &[
    c("cgroup-detect", S(System::Quotas), "read-only probe of per-run cgroup v2 limits; quotas are POSEIDON compute resources (was HEPHAESTUS)"),
    c("desk-hermes", S(System::HermesAgents), "Hermes mind snap for the desk: the agent population view"),
    c("gaia", Core, "the gaia CLI: wires GAIA core to the nine functions; dry-run only"),
    c("gaia-aether", F(FunctionId::Aether), "AETHER function crate"),
    c("gaia-apollo", F(FunctionId::Apollo), "APOLLO function crate"),
    c("gaia-artemis", F(FunctionId::Artemis), "ARTEMIS function crate"),
    c("gaia-core", Core, "GAIA core: orchestrator, ownership table, bus, Alpha registry, lineage"),
    c("gaia-demeter", F(FunctionId::Demeter), "DEMETER function crate"),
    c("gaia-eleuthia", F(FunctionId::Eleuthia), "ELEUTHIA function crate"),
    c("gaia-hades", F(FunctionId::Hades), "HADES function crate (plans only, approval-gated)"),
    c("gaia-hephaestus", F(FunctionId::Hephaestus), "HEPHAESTUS function crate"),
    c("gaia-minerva", F(FunctionId::Minerva), "MINERVA function crate"),
    c("gaia-poseidon", F(FunctionId::Poseidon), "POSEIDON function crate"),
    c("galion-boot", S(System::BootUnits), "boot/keep replacement and systemd unit renderer"),
    c("galion-desk", S(System::GalionDesk), "operator desk endpoint (:8646)"),
    c("galion-one", S(System::GalionOne), "Galion One, the persona the stack embodies (:8091, :8002)"),
    c("galion-origin", S(System::Origin), "origin.py replacement: front door, desk notes, 302 builder (folded openmind-origin)"),
    c("galion-voice-core", S(System::VoiceRelay), "full-duplex framing, VAD, jitter buffer, barge-in"),
    c("galleon-arena", S(System::EcosystemCensus), "paper arena that lists and scores the gladiator population; no value moves (was ARES)"),
    c("guardian-tick", S(System::GuardianTick), "one report-only health pass"),
    c("hades-gate", S(System::Teardown), "approval gate and journal for destructive plans; dry-run, never executes"),
    c("hermes-core", S(System::BossNode), "Hermes coordinator above the seven swarm sectors: registry, governor, retry loop, HADES-gated dispatch, ledger (swarm/hermes-core)"),
    c("hermes-gateway", S(System::Messaging), "Hermes API server binary: the chat/messaging channel"),
    c("hermes-rs", S(System::HermesAgents), "Rust Hermes: agent loop, delegate_task caps, sessions, spend caps"),
    c("link-watch", S(System::LinkWatch), "laptop link intake"),
    c("one-defence", S(System::GuardianWatchdog), "read-only health_checks rows and a defence note; security watch (was HEPHAESTUS)"),
    c("one-set-model", S(System::GalionOne), "Galion One plan/action model lane, validated, writes nothing (was HEPHAESTUS)"),
    c("one-talk", S(System::GalionOne), "Galion One hear/reply request shapes (was HEPHAESTUS)"),
    c("openmind-config", S(System::ConfigHealth), "config health: TOML, then env, loopback defaults"),
    c("openmind-discovery", S(System::LanDiscovery), "LAN peer seeds and scan path; network identity is AETHER (was APOLLO)"),
    c("openmind-exo", S(System::VramPool), "exo compute adapter (fixtures); external compute is POSEIDON (was HEPHAESTUS)"),
    c("openmind-llama", S(System::VramPool), "llama.cpp compute adapter (fixtures) (was HEPHAESTUS)"),
    c("openmind-mcp-stack", S(System::StackSchema), "stdio MCP stack_status: serves knowledge about the stack (was MINERVA)"),
    c("openmind-memory", S(System::MemoryGateway), "memory store and gateway (:8420)"),
    c("openmind-nerve", S(System::Nerve), "nerve event archive: the signal lines are AETHER (was APOLLO)"),
    c("openmind-persona", S(System::PersonaPlex), "persona TOML documents"),
    c("openmind-provider", S(System::VramPool), "compute provider registry, approval state, GPU/LAN/CPU fallback and route (was HEPHAESTUS)"),
    c("openmind-sandbox", S(System::Terminal), "runs one command under rlimits, cleared env, workdir, timeout"),
    c("openmind-schema", S(System::StackSchema), "the stack contract"),
    c("openmind-speech", S(System::SpeechIo), "whisper.cpp / piper output parsers on CPU (was ELEUTHIA)"),
    c("openmind-survival", S(System::Survival), "read-only probes and suggestion-only decisions"),
    c("openmind-tulu", S(System::VramPool), "DR Tulu llama-server row (fixture); a compute provider (was APOLLO)"),
    c("persona-plex-relay", S(System::VoiceRelay), "voice relay backend adapter for Persona Plex (was ELEUTHIA)"),
    c("repo-check", S(System::RepoCheck), "self-eval: SPDX, licence, secrets, no-Python, Zagreb time"),
    c("room", S(System::Room), "read-only page of the operator files agents share"),
    c("tuner", S(System::Tuner), "ukulele tuner static server (:8650)"),
    c("zagreb-time", S(System::ZagrebClock), "Europe/Zagreb RFC 3339 with explicit offset (was GAIA)"),
];

const fn c(name: &'static str, role: CrateRole, why: &'static str) -> CrateOwner {
    CrateOwner { name, role, why }
}

/// The owner row of a crate. Exact match only.
pub fn crate_owner(name: &str) -> Option<&'static CrateOwner> {
    CRATES.iter().find(|r| r.name == name)
}

/// The crates that implement a system.
pub fn crates_for(system: System) -> Vec<&'static str> {
    CRATES
        .iter()
        .filter(|r| r.role.system() == Some(system))
        .map(|r| r.name)
        .collect()
}

/// One known systemd unit (live VPS names, without the `.service` suffix).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitOwner {
    pub unit: &'static str,
    pub system: System,
    /// `true` when this is the legacy unit a Rust unit replaced.
    pub legacy: bool,
}

const fn u(unit: &'static str, system: System, legacy: bool) -> UnitOwner {
    UnitOwner {
        unit,
        system,
        legacy,
    }
}

/// Stack units seen on the VPS (`systemctl list-units --type=service --all`,
/// 2026-10-20 18:52 CEST). Units not listed here are reported as unmapped.
pub const UNITS: &[UnitOwner] = &[
    u("cloudflared", System::Tunnels, false),
    u("console", System::Console, false),
    u("galion-backup", System::Survival, false),
    u("galion-desk", System::GalionDesk, false),
    u("galion-health", System::GuardianWatchdog, false),
    u("galion-nerve", System::Nerve, false),
    u("galion-one", System::GalionOne, false),
    u("hermes", System::HermesAgents, true),
    u("hermes-rs", System::HermesAgents, false),
    u("link-watch", System::LinkWatch, true),
    u("link-watch-rs", System::LinkWatch, false),
    u("memory-gateway", System::MemoryGateway, true),
    u("openmind-memory", System::MemoryGateway, false),
    u("tanga-proto", System::GalionOne, false),
    u("tuner", System::Tuner, false),
    u("tuner-quick", System::Tunnels, false),
];

/// The owner of a unit name (with or without `.service`).
pub fn unit_owner(unit: &str) -> Option<&'static UnitOwner> {
    let base = unit.strip_suffix(".service").unwrap_or(unit);
    UNITS.iter().find(|r| r.unit == base)
}

/// The whole registry as JSON (for `gaia ownership`).
pub fn to_json() -> Json {
    let mut per_function = Vec::new();
    for f in FunctionId::ALL {
        let systems: Vec<Json> = f
            .systems()
            .into_iter()
            .map(|s| {
                Json::obj().with("system", s.name()).with(
                    "crates",
                    Json::Arr(crates_for(s).into_iter().map(Json::from).collect()),
                )
            })
            .collect();
        let fcrate = CRATES
            .iter()
            .find(|r| r.role == CrateRole::Function(f))
            .map(|r| r.name);
        per_function.push(
            Json::obj()
                .with("function", f.name())
                .with("function_crate", fcrate)
                .with("systems", Json::Arr(systems)),
        );
    }
    let systems_with_crates = System::ALL
        .iter()
        .filter(|s| !crates_for(**s).is_empty())
        .count();
    Json::obj()
        .with(
            "design",
            "gaia/docs/gaia-design.md (9 functions, 52 systems)",
        )
        .with("crates_total", CRATES.len())
        .with(
            "system_crates",
            CRATES.iter().filter(|r| r.role.system().is_some()).count(),
        )
        .with("systems_total", System::ALL.len())
        .with("systems_with_a_crate", systems_with_crates)
        .with(
            "crates",
            Json::Arr(CRATES.iter().map(CrateOwner::to_json).collect()),
        )
        .with("functions", Json::Arr(per_function))
        .with(
            "units",
            Json::Arr(
                UNITS
                    .iter()
                    .map(|r| {
                        Json::obj()
                            .with("unit", r.unit)
                            .with("system", r.system.name())
                            .with("function", r.system.function().name())
                            .with("legacy", r.legacy)
                    })
                    .collect(),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn crate_names_unique_and_sorted() {
        let names: Vec<_> = CRATES.iter().map(|r| r.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted, "CRATES must be alphabetical and unique");
    }

    #[test]
    fn each_function_has_exactly_one_function_crate() {
        for f in FunctionId::ALL {
            let n = CRATES
                .iter()
                .filter(|r| r.role == CrateRole::Function(f))
                .count();
            assert_eq!(n, 1, "{f}");
            let want = format!("gaia-{}", f.name().to_ascii_lowercase());
            assert!(crate_owner(&want).is_some(), "{want}");
        }
    }

    #[test]
    fn core_owns_no_system() {
        let core: BTreeSet<_> = CRATES
            .iter()
            .filter(|r| r.role == CrateRole::Core)
            .map(|r| r.name)
            .collect();
        assert_eq!(core, BTreeSet::from(["gaia", "gaia-core"]));
        for r in CRATES.iter().filter(|r| r.role == CrateRole::Core) {
            assert_eq!(r.role.function(), None);
        }
    }

    #[test]
    fn non_gaia_crates_map_to_a_system() {
        for r in CRATES.iter().filter(|r| !r.name.starts_with("gaia")) {
            let s = r
                .role
                .system()
                .unwrap_or_else(|| panic!("{} has no system", r.name));
            assert_eq!(r.role.function(), Some(s.function()));
        }
        // 35 crates of main 13a296c, plus hermes-core (swarm/hermes-core).
        assert_eq!(
            CRATES.iter().filter(|r| r.role.system().is_some()).count(),
            36
        );
    }

    #[test]
    fn lookups() {
        assert_eq!(
            crate_owner("hades-gate").and_then(|r| r.role.function()),
            Some(FunctionId::Hades)
        );
        assert_eq!(crate_owner("Hades-Gate"), None);
        assert_eq!(
            unit_owner("openmind-memory.service").map(|r| r.system),
            Some(System::MemoryGateway)
        );
        assert_eq!(unit_owner("sshd"), None);
        assert_eq!(crates_for(System::VramPool).len(), 4);
    }

    #[test]
    fn units_unique() {
        let names: BTreeSet<_> = UNITS.iter().map(|r| r.unit).collect();
        assert_eq!(names.len(), UNITS.len());
    }
}
