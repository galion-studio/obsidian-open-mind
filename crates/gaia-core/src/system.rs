// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The nine subordinate functions and every Obsidian Open Mind system each one
//! owns. Ownership is a total function: every [`System`] belongs to exactly one
//! [`FunctionId`]. GAIA core owns no system; it only orchestrates and holds
//! the master override.

use std::fmt;

/// GAIA's nine subordinate functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FunctionId {
    Hades,
    Minerva,
    Apollo,
    Hephaestus,
    Artemis,
    Poseidon,
    Aether,
    Demeter,
    Eleuthia,
}

impl FunctionId {
    /// **The single config point** for who owns compute (`vram-pool`, `quotas`,
    /// `queues`, `budget-caps`; implemented by `openmind-queue`).
    ///
    /// Two registries disagree and this crate does not decide between them:
    /// the ws/gaia design (`gaia/docs/gaia-design.md` 4.2) gives compute to
    /// POSEIDON, the VPS ownership registry (`docs/gaia-ownership.md`) puts
    /// compute adapters under HEPHAESTUS. The ownership table below reads
    /// this constant for the four compute rows, so changing the owner is a
    /// one-line edit here. See `docs/gaia-poseidon.md` for what else a flip
    /// needs (the new owner must declare and delegate the systems; GAIA
    /// refuses to register a function whose declaration does not match).
    pub const COMPUTE: FunctionId = FunctionId::Poseidon;

    pub const ALL: [FunctionId; 9] = [
        FunctionId::Hades,
        FunctionId::Minerva,
        FunctionId::Apollo,
        FunctionId::Hephaestus,
        FunctionId::Artemis,
        FunctionId::Poseidon,
        FunctionId::Aether,
        FunctionId::Demeter,
        FunctionId::Eleuthia,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FunctionId::Hades => "HADES",
            FunctionId::Minerva => "MINERVA",
            FunctionId::Apollo => "APOLLO",
            FunctionId::Hephaestus => "HEPHAESTUS",
            FunctionId::Artemis => "ARTEMIS",
            FunctionId::Poseidon => "POSEIDON",
            FunctionId::Aether => "AETHER",
            FunctionId::Demeter => "DEMETER",
            FunctionId::Eleuthia => "ELEUTHIA",
        }
    }

    /// The role the function plays in the game's story (our paraphrase).
    pub fn hzd_role(self) -> &'static str {
        match self {
            FunctionId::Hades => "reverse terraforming; extinction failsafe",
            FunctionId::Minerva => "cracking and broadcasting the swarm deactivation codes",
            FunctionId::Apollo => "archive of human knowledge for the cradle-born",
            FunctionId::Hephaestus => "machine design and fabrication in the Cauldrons",
            FunctionId::Artemis => "reintroduction of animal life",
            FunctionId::Poseidon => "cleansing the hydrosphere",
            FunctionId::Aether => "cleansing and stabilising the atmosphere",
            FunctionId::Demeter => "plant life and environmental stabilisation",
            FunctionId::Eleuthia => "gestating and raising humans in the Cradles",
        }
    }

    /// Our domain for the function.
    pub fn domain(self) -> &'static str {
        match self {
            FunctionId::Hades => {
                "guarded teardown, rollback and kill switch (plans only, approval-gated)"
            }
            FunctionId::Minerva => {
                "security audit, watchdog, alerting, perception and policy review"
            }
            FunctionId::Apollo => "knowledge archive: memory, intake, docs, history",
            FunctionId::Hephaestus => {
                "fabrication: builds, artifacts, sandboxed command plans, tools"
            }
            FunctionId::Artemis => "agent population: spawning, retirement, census",
            FunctionId::Poseidon => "compute resources, quotas, queues, budget caps",
            FunctionId::Aether => "communications: messaging, nerve, voice relay, network identity",
            FunctionId::Demeter => "environment health: survival, healing, config, boot",
            FunctionId::Eleuthia => "birth of personas and agents from seeds",
        }
    }

    /// Parse a function name, case-insensitive.
    pub fn parse(s: &str) -> Option<FunctionId> {
        FunctionId::ALL
            .into_iter()
            .find(|f| f.name().eq_ignore_ascii_case(s))
    }

    /// The systems this function owns.
    pub fn systems(self) -> Vec<System> {
        System::ALL
            .iter()
            .copied()
            .filter(|s| s.function() == self)
            .collect()
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

macro_rules! systems {
    ($( $var:ident => ($name:literal, $func:ident, $component:literal, $destructive:literal) ),+ $(,)?) => {
        /// Every real Obsidian Open Mind / Galion system Gaia knows about.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum System { $( $var ),+ }

        impl System {
            pub const ALL: &'static [System] = &[ $( System::$var ),+ ];

            /// Stable kebab-case name.
            pub fn name(self) -> &'static str {
                match self { $( System::$var => $name ),+ }
            }

            /// The one subordinate function that owns this system.
            pub fn function(self) -> FunctionId {
                match self { $( System::$var => FunctionId::$func ),+ }
            }

            /// The existing crate or component that implements it today.
            pub fn component(self) -> &'static str {
                match self { $( System::$var => $component ),+ }
            }

            /// Whether acting on it can destroy state. Destructive systems are
            /// dry-run by default and need an explicit approval token.
            pub fn destructive(self) -> bool {
                match self { $( System::$var => $destructive ),+ }
            }
        }
    };
}

systems! {
    // HADES: reverse terraforming, failsafe. Plans only.
    Teardown => ("teardown", Hades, "teardown plans over the stack units (no executor)", true),
    Rollback => ("rollback", Hades, "deploy/rollback.sh, deploy/rollback-console.sh", true),
    KillSwitch => ("kill-switch", Hades, "stop-everything plan for the stack units", true),
    HealApply => ("heal-apply", Hades, "galion-healer heal-apply (ws/survival)", true),
    SecretRotation => ("secret-rotation-plan", Hades, "rotation plans; values are never read", true),
    // MINERVA: decryption and override.
    RepoCheck => ("repo-check", Minerva, "crates/repo-check", false),
    SecretScan => ("secret-scan", Minerva, "crates/repo-check (secrets check)", false),
    GuardianWatchdog => ("guardian-watchdog", Minerva, "galion-guardian (ws/survival)", false),
    Alerting => ("alerting", Minerva, "guardian health-tick alerting (ws/survival)", false),
    Vision => ("vision", Minerva, "image/screen perception (ws/speech, queued)", false),
    SandboxPolicyReview => ("sandbox-policy-review", Minerva, "crates/openmind-sandbox policy, docs/agentic-sandbox.md", false),
    // APOLLO: knowledge archive.
    MemoryGateway => ("memory-gateway", Apollo, "openmind-memory (ws/memory)", false),
    LinkWatch => ("link-watch", Apollo, "crates/link-watch", false),
    TeacherDocs => ("teacher-docs", Apollo, "TEACHER/, release/TEACHER, docs/", false),
    SessionHistory => ("session-history", Apollo, "session handoff, desk notes", false),
    StackSchema => ("stack-schema", Apollo, "crates/openmind-schema", false),
    // HEPHAESTUS: fabrication and Cauldrons.
    Terminal => ("terminal", Hephaestus, "crates/openmind-sandbox (command runner)", false),
    CargoBuild => ("cargo-build", Hephaestus, "workspace cargo builds (UNIVERSAL-CPU.md)", false),
    ReleaseArtifacts => ("release-artifacts", Hephaestus, "release/, dist/bin", false),
    GalionDesk => ("galion-desk", Hephaestus, "crates/galion-desk", false),
    OmnodeSandbox => ("omnode-sandbox-runner", Hephaestus, "omnode sandbox runner (planned)", false),
    Crafting => ("crafting", Hephaestus, "Gaia artifact fabrication (gaia/)", false),
    Console => ("console", Hephaestus, "console-ide, deploy/console-prod.*", false),
    DeploySwitch => ("deploy-switch", Hephaestus, "deploy/switch.sh, deploy/switch-console.sh (plan only)", false),
    // ARTEMIS: agent population.
    HermesAgents => ("hermes-agents", Artemis, "crates/desk-hermes, hermes-rs (ws/hermes)", false),
    BossNode => ("boss-node", Artemis, "Hermes boss node (ws/hermes)", false),
    Room => ("room", Artemis, "crates/room", false),
    AgentSpawning => ("agent-spawning", Artemis, "agent spawning (Gaia, Hermes)", false),
    AgentRetirement => ("agent-retirement", Artemis, "agent retirement (Gaia, Hermes)", false),
    EcosystemCensus => ("ecosystem-census", Artemis, "population census (Gaia)", false),
    // Compute: owned by `FunctionId::COMPUTE` (POSEIDON today; the single config point).
    VramPool => ("vram-pool", COMPUTE, "crates/openmind-queue VRAM registry, GPU -> LAN -> CPU routing (model only, no spend)", false),
    Quotas => ("quotas", COMPUTE, "crates/openmind-queue per-agent quotas; openmind-sandbox rlimits", false),
    Queues => ("queues", COMPUTE, "crates/openmind-queue durable queues, leases, DLQ, backpressure", false),
    BudgetCaps => ("budget-caps", COMPUTE, "crates/openmind-queue spend caps (read-only via Gaia); no real-money access", false),
    // AETHER: atmosphere and communications.
    Messaging => ("messaging", Aether, "chat and messaging channels", false),
    Nerve => ("nerve", Aether, "the nerve: signal lines between components", false),
    VoiceRelay => ("voice-relay", Aether, "crates/galion-voice-core, crates/persona-plex-relay", false),
    SpeechIo => ("speech-io", Aether, "STT/TTS on CPU (ws/speech, queued)", false),
    WifiIdentity => ("wifi-identity", Aether, "omnode Wi-Fi identity", false),
    LanDiscovery => ("lan-discovery", Aether, "omnode LAN discovery", false),
    Tunnels => ("tunnels", Aether, "tuner-publish quick tunnel, network", false),
    Origin => ("origin", Aether, "galion-origin front door (B56)", false),
    ZagrebClock => ("zagreb-clock", Aether, "crates/zagreb-time", false),
    // DEMETER: flora, growth, environment health.
    Survival => ("survival", Demeter, "crates/openmind-survival (decide/degrade)", false),
    Healer => ("healer", Demeter, "galion-healer heal-dry (ws/survival)", false),
    Tuner => ("tuner", Demeter, "tuner crate (B53)", false),
    ConfigHealth => ("config-health", Demeter, "crates/openmind-config", false),
    GuardianTick => ("guardian-tick", Demeter, "crates/guardian-tick", false),
    BootUnits => ("boot-units", Demeter, "galion-boot, keep.sh/stack-boot replacements, deploy/*.service", false),
    // ELEUTHIA: birth and cloning.
    PersonaPlex => ("persona-plex", Eleuthia, "Persona Plex v2 personas (docs/persona-plex-v2-design.md)", false),
    GalionOne => ("galion-one", Eleuthia, "crates/galion-one (Samantha persona)", false),
    GaiaSeeds => ("gaia-seeds", Eleuthia, "gaia/ seed files", false),
}

impl System {
    /// The compute systems, whose owner is [`FunctionId::COMPUTE`].
    pub const COMPUTE: [System; 4] = [
        System::VramPool,
        System::Quotas,
        System::Queues,
        System::BudgetCaps,
    ];

    /// Whether this is one of the [`System::COMPUTE`] systems.
    pub fn is_compute(self) -> bool {
        System::COMPUTE.contains(&self)
    }

    /// Parse a kebab-case system name.
    pub fn parse(s: &str) -> Option<System> {
        System::ALL.iter().copied().find(|x| x.name() == s)
    }
}

impl fmt::Display for System {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_system_maps_to_exactly_one_function() {
        let mut seen = BTreeSet::new();
        let mut total = 0;
        for f in FunctionId::ALL {
            for s in f.systems() {
                assert!(seen.insert(s), "{s} owned twice");
                assert_eq!(s.function(), f);
                total += 1;
            }
        }
        assert_eq!(total, System::ALL.len());
        assert_eq!(seen.len(), System::ALL.len());
    }

    #[test]
    fn every_function_owns_at_least_one_system() {
        for f in FunctionId::ALL {
            assert!(!f.systems().is_empty(), "{f} owns nothing");
        }
    }

    #[test]
    fn only_hades_systems_are_destructive() {
        for s in System::ALL {
            assert_eq!(s.destructive(), s.function() == FunctionId::Hades, "{s}");
        }
    }

    #[test]
    fn compute_rows_follow_the_single_config_point() {
        assert!(FunctionId::COMPUTE.systems().len() >= 4);
        for s in System::COMPUTE {
            assert!(s.is_compute());
            assert_eq!(s.function(), FunctionId::COMPUTE, "{s}");
        }
        let n = System::ALL.iter().filter(|s| s.is_compute()).count();
        assert_eq!(n, 4);
    }

    #[test]
    fn names_are_unique_and_parse_back() {
        let names: BTreeSet<_> = System::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names.len(), System::ALL.len());
        for s in System::ALL {
            assert_eq!(System::parse(s.name()), Some(*s));
        }
        assert_eq!(System::parse("no-such-system"), None);
        assert_eq!(FunctionId::parse("hades"), Some(FunctionId::Hades));
    }
}
