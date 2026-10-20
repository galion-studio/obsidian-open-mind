// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! HADES approvals for gated tasks (merge, deploy, destructive).
//!
//! Hermes holds only the admin PUBLIC key, through hades-gate's
//! [`Approver`]. A gated task is dispatched only on
//! [`Verdict::Approved`]: a token Slavik signed for exactly this action kind,
//! this target and these plan bytes, unexpired and never used before.
//! Everything else holds or refuses the task:
//!
//! * no HADES config -> refused (fail closed);
//! * no token -> held (HADES dry-run), waiting for Slavik;
//! * `merge` (and other kinds HADES has no allow-list entry for) -> refused by
//!   HADES itself, whatever the token: merges stay with the owner;
//! * HADES admin-safety hazards in the plan or target -> refused.

use crate::action::ActionClass;
use crate::plan::Task;
use hades_gate::approval::{unix_now, Approver, Request, Verdict};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "decision")]
pub enum GateDecision {
    /// Not a gated class; no approval needed.
    NotNeeded,
    /// Gated, no token: waiting for Slavik's signed approval.
    Held {
        action: String,
        target: String,
        plan_hash: String,
    },
    /// Signed, scoped, single-use approval consumed. Still executes nothing
    /// by itself; Hermes may now dispatch the task.
    Approved {
        action: String,
        target: String,
        expires_at: u64,
    },
    /// Refused; `code` is hades-gate's stable refusal code.
    Refused {
        action: String,
        code: String,
        reason: String,
    },
}

impl GateDecision {
    pub fn label(&self) -> &'static str {
        match self {
            GateDecision::NotNeeded => "not-needed",
            GateDecision::Held { .. } => "held",
            GateDecision::Approved { .. } => "approved",
            GateDecision::Refused { .. } => "refused",
        }
    }
    pub fn may_dispatch(&self) -> bool {
        matches!(
            self,
            GateDecision::NotNeeded | GateDecision::Approved { .. }
        )
    }
}

/// HADES action kind for a Hermes task. `merge` and `destroy` are not on the
/// HADES allow-list, so HADES refuses them; that is intended.
pub fn hades_action(task: &Task) -> &'static str {
    let t = task.title.to_ascii_lowercase();
    match task.class {
        ActionClass::Merge => "merge",
        ActionClass::Deploy => "switch",
        ActionClass::Destructive if t.contains("rollback") => "rollback",
        ActionClass::Destructive if t.contains("heal") => "heal-apply",
        ActionClass::Destructive if t.contains("rotat") => "rotate",
        ActionClass::Destructive if t.contains("kill switch") || t.contains("kill-switch") => {
            "kill"
        }
        ActionClass::Destructive => "destroy",
        ActionClass::ReadOnly | ActionClass::Code => "none",
    }
}

/// HADES target name: `hermes.<sector>.<id>`, within hades-gate's charset.
pub fn hades_target(task: &Task) -> String {
    let raw = format!(
        "hermes.{}.{}",
        task.sector,
        task.key.rsplit(':').next().unwrap_or("")
    );
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':') {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// The exact plan bytes Slavik signs: printable ASCII only (HADES refuses
/// anything else), one `key=value` per line, stable order.
pub fn plan_bytes(task: &Task) -> Vec<u8> {
    let ascii = |s: &str| -> String {
        s.chars()
            .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
            .collect()
    };
    format!(
        "hermes-plan v1\ntask={}\nsector={}\nsession={}\nclass={}\nhades_action={}\ntitle={}\n",
        ascii(&task.key),
        ascii(&task.sector),
        ascii(&task.session),
        task.class.as_str(),
        hades_action(task),
        ascii(&task.title),
    )
    .into_bytes()
}

pub struct Gate {
    approver: Option<Approver>,
    why_none: String,
}

impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gate")
            .field("configured", &self.approver.is_some())
            .finish()
    }
}

impl Gate {
    /// No HADES: every gated task is refused.
    pub fn closed(why: &str) -> Gate {
        Gate {
            approver: None,
            why_none: why.into(),
        }
    }

    /// From a hades-gate config file path; empty path = closed.
    pub fn from_config(path: &str) -> Gate {
        if path.is_empty() {
            return Gate::closed("no HADES config (hades.config is empty)");
        }
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| format!("read {path}: {e}"))
            .and_then(|t| hades_gate::config::Config::parse(&t))
            .and_then(|c| Approver::from_config(&c));
        match loaded {
            Ok(a) => Gate {
                approver: Some(a),
                why_none: String::new(),
            },
            Err(e) => Gate::closed(&format!("HADES config unusable: {e}")),
        }
    }

    pub fn configured(&self) -> bool {
        self.approver.is_some()
    }

    /// Admin key fingerprint (public), when configured.
    pub fn admin_fingerprint(&self) -> Option<String> {
        self.approver.as_ref().map(Approver::admin_fingerprint)
    }

    pub fn why_closed(&self) -> &str {
        &self.why_none
    }

    /// Decide one task. An empty token means "no approval given".
    pub fn check(&self, task: &Task, token: &str) -> GateDecision {
        if !task.class.gated() {
            return GateDecision::NotNeeded;
        }
        let action = hades_action(task).to_string();
        let target = hades_target(task);
        let plan = plan_bytes(task);
        let Some(approver) = &self.approver else {
            return GateDecision::Refused {
                action,
                code: "no-hades".into(),
                reason: self.why_none.clone(),
            };
        };
        let req = Request {
            action: &action,
            target: &target,
            plan: &plan,
            token: token.trim(),
        };
        match approver.approve(req, unix_now()) {
            Verdict::DryRun => GateDecision::Held {
                action,
                target,
                plan_hash: hades_gate::token::to_hex(&hades_gate::token::plan_hash(&plan)),
            },
            Verdict::Approved(c) => GateDecision::Approved {
                action,
                target,
                expires_at: c.expires_at,
            },
            Verdict::Refused(r) => GateDecision::Refused {
                action,
                code: r.code().into(),
                reason: r.reason(),
            },
        }
    }

    /// Check with a token read from a file (never printed).
    ///
    /// # Errors
    /// The token file cannot be read.
    pub fn check_with_file(&self, task: &Task, token_file: &Path) -> Result<GateDecision, String> {
        let token = std::fs::read_to_string(token_file)
            .map_err(|e| format!("read token file {}: {e}", token_file.display()))?;
        Ok(self.check(task, &token))
    }
}

impl Gate {
    /// What [`Gate::check`] would answer with no token, without writing to
    /// the HADES ledger. Used by dry runs, readiness and the GET API, which
    /// must not have side effects.
    pub fn preview(&self, task: &Task) -> GateDecision {
        if !task.class.gated() {
            return GateDecision::NotNeeded;
        }
        let action = hades_action(task).to_string();
        let target = hades_target(task);
        let plan = plan_bytes(task);
        let refused = |code: &str, reason: String| GateDecision::Refused {
            action: action.clone(),
            code: code.into(),
            reason,
        };
        let Some(approver) = &self.approver else {
            return refused("no-hades", self.why_none.clone());
        };
        if approver.is_disabled() {
            return refused("disabled", "HADES is disabled by the admin".into());
        }
        if !hades_gate::Action::parse(&action).is_some_and(hades_gate::Action::is_allow_listed) {
            return refused("action", format!("action {action:?} can never be approved"));
        }
        if !hades_gate::token::valid_target(&target) {
            return refused("malformed", "target name".into());
        }
        if let Err(h) = hades_gate::safety::review(&target, &plan) {
            return refused("admin-safety", h.reason());
        }
        GateDecision::Held {
            action,
            target,
            plan_hash: hades_gate::token::to_hex(&hades_gate::token::plan_hash(&plan)),
        }
    }
}
