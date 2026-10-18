// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! B23: pure healer decision engine. Suggestions only, nothing executes here.

/// What a probe watches; decides how far escalation may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    /// HTTP health endpoint of a service.
    Http,
    /// systemd unit state.
    Unit,
    /// The tunnel process.
    Tunnel,
    /// Memory, disk or load. Never a restart.
    Host,
    /// File freshness. Never a restart.
    Fresh,
}

impl ProbeKind {
    /// Classify a probe result name as used by guardian-tick / galion-guardian.
    pub fn of(name: &str) -> ProbeKind {
        if name.starts_with("unit:") || name.starts_with("unit-") {
            ProbeKind::Unit
        } else if name.starts_with("disk:") || name == "mem" || name == "load" {
            ProbeKind::Host
        } else if name.starts_with("fresh:") {
            ProbeKind::Fresh
        } else if name == "cloudflared" || name == "tunnel" {
            ProbeKind::Tunnel
        } else {
            ProbeKind::Http
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Observe,
    Warn,
    SuggestRestart,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Observe => "observe",
            Level::Warn => "warn",
            Level::SuggestRestart => "suggest-restart",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub name: String,
    pub ok: bool,
    /// Failures in a row, including this one (0 when ok).
    pub fails_in_row: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub service: String,
    pub level: Level,
    pub text: String,
}

/// Escalation thresholds (beats in a row).
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub warn_after: u32,
    pub restart_after: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            warn_after: 2,
            restart_after: 4,
        }
    }
}

/// One suggestion per failing observation; healthy ones produce nothing.
pub fn decide(obs: &[Observation], policy: Policy) -> Vec<Suggestion> {
    obs.iter()
        .filter(|o| !o.ok)
        .map(|o| {
            let kind = ProbeKind::of(&o.name);
            let mut level = if o.fails_in_row >= policy.restart_after {
                Level::SuggestRestart
            } else if o.fails_in_row >= policy.warn_after {
                Level::Warn
            } else {
                Level::Observe
            };
            if matches!(kind, ProbeKind::Host | ProbeKind::Fresh) && level > Level::Warn {
                level = Level::Warn;
            }
            let n = o.name.as_str();
            let text = match (level, kind) {
                (Level::Observe, _) => format!("{n} failed once: watching"),
                (Level::Warn, ProbeKind::Host) => format!("{n} high for {} beats", o.fails_in_row),
                (Level::Warn, _) => format!("{n} down for {} beats", o.fails_in_row),
                (Level::SuggestRestart, _) => {
                    format!("{n} down for {} beats: operator may restart it", o.fails_in_row)
                }
            };
            Suggestion {
                service: o.name.clone(),
                level,
                text,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(name: &str, ok: bool, n: u32) -> Observation {
        Observation {
            name: name.into(),
            ok,
            fails_in_row: n,
        }
    }

    #[test]
    fn table() {
        let rows = [
            (o("one", true, 0), None),
            (o("one", false, 1), Some(Level::Observe)),
            (o("one", false, 2), Some(Level::Warn)),
            (o("one", false, 4), Some(Level::SuggestRestart)),
            (o("unit:hermes", false, 9), Some(Level::SuggestRestart)),
            (o("cloudflared", false, 5), Some(Level::SuggestRestart)),
            (o("mem", false, 9), Some(Level::Warn)),
            (o("disk:/", false, 9), Some(Level::Warn)),
            (o("fresh:/x", false, 9), Some(Level::Warn)),
            (o("load", false, 1), Some(Level::Observe)),
        ];
        for (ob, want) in rows {
            let got = decide(std::slice::from_ref(&ob), Policy::default());
            assert_eq!(got.first().map(|s| s.level), want, "{ob:?}");
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(ProbeKind::of("unit-console"), ProbeKind::Unit);
        assert_eq!(ProbeKind::of("tunnel"), ProbeKind::Tunnel);
        assert_eq!(ProbeKind::of("whisper"), ProbeKind::Http);
        assert_eq!(Level::SuggestRestart.as_str(), "suggest-restart");
    }
}
