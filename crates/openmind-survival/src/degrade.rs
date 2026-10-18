// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! B24: graceful degradation rules as data. Given what the machine has (GPU,
//! RAM, LAN, WAN), pick the CPU-light fallback. First matching rule wins.
//! The GPU/RAM thresholds and the three `line` texts are the ones in
//! galion-light hardware.sh and stack-boot.sh, so hardware.json stays the same.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    pub gpu_present: bool,
    pub gpu_free_mib: u64,
    pub ram_total_mib: u64,
    /// A LAN model host (laptop, GPU box) answers.
    pub lan_ok: bool,
    /// The internet/provider API answers.
    pub wan_ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Llm {
    /// Local weights on this GPU.
    LocalGpu,
    /// Provider API over the internet (laptop model / OpenRouter).
    Api,
    /// A model on the LAN (laptop or GPU box).
    Lan,
    /// No model reachable: queue work, keep probes and the desk static.
    CpuLight,
}

impl Llm {
    pub fn as_str(self) -> &'static str {
        match self {
            Llm::LocalGpu => "local-gpu",
            Llm::Api => "api",
            Llm::Lan => "lan",
            Llm::CpuLight => "cpu-light",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub rule: &'static str,
    pub llm: Llm,
    pub weights: bool,
    /// Enough RAM for the voice worker (stack-boot.sh `mouth`).
    pub mouth: bool,
    pub line: &'static str,
}

pub const MIN_GPU_FREE_MIB: u64 = 8192;
pub const MIN_RAM_MIB: u64 = 8192;

pub const LINE_WEIGHTS: &str = "Obsidian has room for local weights.";
pub const LINE_CARD_LOW: &str =
    "A card is here, but not enough free memory. Hermes stays on the laptop model.";
pub const LINE_CPU: &str = "CPU only. Hermes stays on the laptop model. Weights stay off.";
pub const LINE_LAN: &str = "Internet is down. Hermes uses the LAN model.";
pub const LINE_OFFLINE: &str = "No network. CPU-light: probes and desk only, work is queued.";

type Pred = fn(&Signals) -> bool;

/// (rule name, condition, llm, weights, line). Ordered.
pub const RULES: &[(&str, Pred, Llm, bool, &str)] = &[
    (
        "gpu-room",
        |s| s.gpu_present && s.gpu_free_mib >= MIN_GPU_FREE_MIB && s.ram_total_mib >= MIN_RAM_MIB,
        Llm::LocalGpu,
        true,
        LINE_WEIGHTS,
    ),
    (
        "wan-down-lan-up",
        |s| !s.wan_ok && s.lan_ok,
        Llm::Lan,
        false,
        LINE_LAN,
    ),
    (
        "offline",
        |s| !s.wan_ok && !s.lan_ok,
        Llm::CpuLight,
        false,
        LINE_OFFLINE,
    ),
    ("gpu-low", |s| s.gpu_present, Llm::Api, false, LINE_CARD_LOW),
    ("cpu-only", |_| true, Llm::Api, false, LINE_CPU),
];

pub fn profile(s: &Signals) -> Profile {
    let (rule, _, llm, weights, line) = RULES
        .iter()
        .find(|r| (r.1)(s))
        .copied()
        .unwrap_or(RULES[RULES.len() - 1]);
    Profile {
        rule,
        llm,
        weights,
        mouth: s.ram_total_mib >= MIN_RAM_MIB,
        line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(gpu: bool, free: u64, ram: u64, lan: bool, wan: bool) -> Signals {
        Signals {
            gpu_present: gpu,
            gpu_free_mib: free,
            ram_total_mib: ram,
            lan_ok: lan,
            wan_ok: wan,
        }
    }

    #[test]
    fn table() {
        let rows = [
            (
                s(true, 9000, 16000, true, true),
                "gpu-room",
                Llm::LocalGpu,
                true,
                true,
            ),
            (
                s(true, 9000, 16000, false, false),
                "gpu-room",
                Llm::LocalGpu,
                true,
                true,
            ),
            (
                s(true, 4000, 16000, true, true),
                "gpu-low",
                Llm::Api,
                false,
                true,
            ),
            (
                s(true, 9000, 4000, true, true),
                "gpu-low",
                Llm::Api,
                false,
                false,
            ),
            (
                s(false, 0, 2048, true, true),
                "cpu-only",
                Llm::Api,
                false,
                false,
            ),
            (
                s(false, 0, 16000, true, false),
                "wan-down-lan-up",
                Llm::Lan,
                false,
                true,
            ),
            (
                s(false, 0, 1024, false, false),
                "offline",
                Llm::CpuLight,
                false,
                false,
            ),
        ];
        for (sig, rule, llm, w, mouth) in rows {
            let p = profile(&sig);
            assert_eq!(
                (p.rule, p.llm, p.weights, p.mouth),
                (rule, llm, w, mouth),
                "{sig:?}"
            );
        }
        assert_eq!(profile(&s(false, 0, 1, true, true)).line, LINE_CPU);
        assert_eq!(Llm::CpuLight.as_str(), "cpu-light");
    }
}
