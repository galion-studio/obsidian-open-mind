// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! What a task does to the world, and therefore whether HADES must approve it.
//!
//! Classification is by words in the task title and its session title. It is
//! deliberately conservative: a false "gated" only holds a task for Slavik;
//! a false "code" would let a live change through, so the rules lean towards
//! gating. Box rehearsals (systemd shim, "on the box") are not live changes.

use serde::{Deserialize, Serialize};

/// Ordered from harmless to dangerous; `max` picks the more dangerous.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ActionClass {
    /// Reading, docs, memos, reviews.
    ReadOnly,
    /// Code on a swarm/* branch, box-only tests. No approval needed.
    Code,
    /// Merge into main, opening a PR, tags, releases. Owner only.
    Merge,
    /// Anything that changes a live service: cutover, unit install, restart.
    Deploy,
    /// Kill, purge, delete, rotate, wipe.
    Destructive,
}

impl ActionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionClass::ReadOnly => "read-only",
            ActionClass::Code => "code",
            ActionClass::Merge => "merge",
            ActionClass::Deploy => "deploy",
            ActionClass::Destructive => "destructive",
        }
    }

    pub fn parse(s: &str) -> Option<ActionClass> {
        Some(match s {
            "read-only" => ActionClass::ReadOnly,
            "code" => ActionClass::Code,
            "merge" => ActionClass::Merge,
            "deploy" => ActionClass::Deploy,
            "destructive" => ActionClass::Destructive,
            _ => return None,
        })
    }

    /// Needs a signed HADES approval before it may be dispatched.
    pub fn gated(self) -> bool {
        matches!(
            self,
            ActionClass::Merge | ActionClass::Deploy | ActionClass::Destructive
        )
    }
}

const DESTRUCTIVE: &[&str] = &[
    "kill ",
    "kill-",
    "purge",
    "delete",
    "wipe",
    "drop table",
    "rotation",
    "rotate ",
    "git clean",
    "rm -rf",
    "remove the",
    "retire the",
];

const DEPLOY: &[&str] = &[
    "cutover",
    "binary swap",
    "new unit",
    "drop-in",
    "restart",
    "systemctl enable",
    "install the unit",
    "deploy ",
    "go live",
    "live cutover",
    "the swap",
    "owner runs it",
    "apply the decision on the vps",
];

const MERGE_VERBS: &[&str] = &[
    " merge ",
    "merge into",
    "merge of",
    "merges ",
    "land ",
    "fold ",
];
const MERGE_INTO: &[&str] = &["on main", "into main", "to main", "owner merge"];
const RELEASE: &[&str] = &[
    "tag v0",
    "tag v1",
    "annotated `v",
    "release v0.",
    "release v1.",
    "publish the",
    "open the pr",
];

const REHEARSAL: &[&str] = &[
    "rehears",
    "on the box",
    "systemctl` shim",
    "systemd shim",
    "box test",
    "box real test",
    "dry-run",
    "dry run",
];

const READ_ONLY: &[&str] = &["review", "inventory", "(draft only)", "doc:"];

/// Writing about an action is not the action: these make the whole task
/// read-only unless its session is owner-executed.
const STRONG_READ_ONLY: &[&str] = &[
    "read-only",
    "memo",
    "dossier",
    "proposal",
    "design doc",
    "runbook",
    "text only",
    "docs:",
    "verifier",
    "index docs",
];

/// Classify one task from its title, with the session title as context.
pub fn classify(title: &str, session_title: &str) -> ActionClass {
    // `kill -9` in a crash test is a test technique, not an action.
    let t = format!(" {} ", title.to_ascii_lowercase()).replace("kill -9", "sig9-test");
    let has = |list: &[&str]| list.iter().any(|w| t.contains(w));
    let rehearsal = has(REHEARSAL);
    let read_only = has(READ_ONLY) || has(STRONG_READ_ONLY);
    let mut class = if read_only {
        ActionClass::ReadOnly
    } else {
        ActionClass::Code
    };
    if !read_only && (has(MERGE_VERBS) && has(MERGE_INTO) || has(RELEASE)) {
        class = class.max(ActionClass::Merge);
    }
    if has(DEPLOY) && !rehearsal {
        class = class.max(ActionClass::Deploy);
    }
    if has(DESTRUCTIVE) && !rehearsal {
        class = class.max(ActionClass::Destructive);
    }
    let s = session_title.to_ascii_lowercase();
    let owner_session = s.contains("owner-gated")
        || s.contains("one service per approval")
        || s.contains("live cutover")
        || s.contains("owner execution");
    if has(STRONG_READ_ONLY) && !owner_session {
        return ActionClass::ReadOnly;
    }
    if owner_session && class != ActionClass::ReadOnly {
        class = class.max(ActionClass::Deploy);
    }
    class
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_titles_from_the_plans() {
        use ActionClass::*;
        let cases = [
            (
                "tuner 8650 binary swap via galion-deploy (cutover/tuner.md §3)",
                "S4: Cutovers, one service per approval (Oct 22-29)",
                Deploy,
            ),
            (
                "T1 clean-clone re-verify of 8fd26f6",
                "S1. Land integration-1 on main",
                Code,
            ),
            (
                "Merge agent: integration-1, then gaia-hades-tokens, then gaia-poseidon",
                "S1: Land everything Gaia on one branch",
                Code,
            ),
            (
                "Rehearse each cutover on the box with the real galion-deploy and a systemd shim",
                "S3: Box rehearsal",
                Code,
            ),
            (
                "SEC-1 history purge with `galion-purge` on a scratch mirror",
                "S7. Owner execution of SEC-1..5",
                Destructive,
            ),
            (
                "Decision memo for Slavik (read-only; see §5)",
                "S2: Ownership decision",
                ReadOnly,
            ),
            (
                "Tag and version policy memo: annotated `v0.1.0` on the merged launch commit",
                "S2",
                ReadOnly,
            ),
            (
                "T3 owner merge pack (draft only)",
                "S1. Land integration-1 on main",
                ReadOnly,
            ),
            (
                "Land integration-1 on main after the owner merge",
                "S1",
                Merge,
            ),
            (
                "T3 post-release verification",
                "S6. Freeze and go/no-go",
                Code,
            ),
            (
                "Merge swarm/integration-1 (8fd26f6) into main",
                "H. Operator intake",
                Merge,
            ),
            (
                "Per-port dossier ... Then kill proposals, one port per approval",
                "S6: Strays",
                ReadOnly,
            ),
            (
                "Crash/soak suite: kill -9 loops during ack, torn tail",
                "S4: POSEIDON",
                Code,
            ),
            (
                "Persist refusal counters across kill -9 (poseidon.md limits)",
                "S4",
                Code,
            ),
            (
                "`scripts/release/artifacts.sh`: clean checkout of a merged main SHA",
                "S2",
                Code,
            ),
            (
                "SEC-1 rotation, order P0 (GitHub token, cloudflared tunnel credential)",
                "S7. Owner execution of SEC-1..5 with swarm verification",
                Destructive,
            ),
            (
                "Key custody and rotation design doc (keygen on the owner's device)",
                "S5: HADES gate wiring",
                ReadOnly,
            ),
        ];
        for (title, session, want) in cases {
            assert_eq!(classify(title, session), want, "{title}");
        }
    }

    #[test]
    fn gated_classes() {
        assert!(!ActionClass::Code.gated());
        assert!(!ActionClass::ReadOnly.gated());
        assert!(ActionClass::Merge.gated());
        assert!(ActionClass::Deploy.gated());
        assert!(ActionClass::Destructive.gated());
        for c in ["read-only", "code", "merge", "deploy", "destructive"] {
            assert_eq!(ActionClass::parse(c).unwrap().as_str(), c);
        }
    }
}
