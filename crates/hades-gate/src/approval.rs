// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The real approval path: config, admin public key, replay ledger and the
//! admin-safety rule put together.
//!
//! Order of checks for [`Approver::approve`] (the first failure wins):
//!
//! 1. disable file present → [`Verdict::Refused`] (the admin switched HADES off);
//! 2. `kill` / `rotate` or an unknown action → refused, no token can change it;
//! 3. admin-safety [`crate::safety::review`] on target and plan → refused;
//! 4. no token → [`Verdict::DryRun`] (the default);
//! 5. signature, lifetime, expiry and exact scope ([`crate::token::verify`]);
//! 6. single use: the nonce is consumed in the fsynced ledger.
//!
//! A success is [`Verdict::Approved`], which maps to
//! [`Decision::WouldApply`](crate::Decision::WouldApply): the gate is still in
//! dry-run and executes nothing. It hands an approved, single-use, scoped
//! decision to the caller (operator script or GAIA HADES).

use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;

use crate::config::Config;
use crate::ledger::{Ledger, LedgerError};
use crate::token::{self, Claims, Scope, TokenError};
use crate::{Action, Decision, Mode};

/// Outcome of [`Approver::approve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No token was given: plan only.
    DryRun,
    /// Valid, in scope, unexpired, first use. Still not executed.
    Approved(Claims),
    /// Refused, with a reason that never contains key material.
    Refused(Refusal),
}

impl Verdict {
    /// The matching legacy [`Decision`].
    #[must_use]
    pub fn decision(&self) -> Decision {
        match self {
            Self::DryRun => Decision::DryRun,
            Self::Approved(_) => Decision::WouldApply,
            Self::Refused(_) => Decision::Refused,
        }
    }
}

/// Why an approval was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The admin disabled HADES (disable file present).
    Disabled,
    /// `kill`, `rotate` or unknown action kind.
    ActionNotApprovable(String),
    /// The admin-safety rule.
    Safety(crate::safety::Hazard),
    /// Token problem: forged, tampered, expired, wrong scope, ...
    Token(TokenError),
    /// Replay or ledger failure.
    Ledger(LedgerError),
    /// Typed plan (Option B) is not a canonical typed plan.
    TypedPlan(crate::typed::PlanError),
}

impl Refusal {
    /// Human-readable reason.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Disabled => "HADES is disabled by the admin (disable file present)".into(),
            Self::ActionNotApprovable(a) => format!("action {a:?} can never be approved"),
            Self::Safety(h) => h.reason(),
            Self::Token(t) => t.reason(),
            Self::Ledger(l) => l.reason(),
            Self::TypedPlan(p) => p.reason(),
        }
    }

    /// Short stable code for JSON output and tests.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::ActionNotApprovable(_) => "action",
            Self::Safety(_) => "admin-safety",
            Self::Token(TokenError::Malformed(_)) => "malformed",
            Self::Token(TokenError::BadSignature) => "forged",
            Self::Token(TokenError::Expired) => "expired",
            Self::Token(TokenError::NotYetValid) => "not-yet-valid",
            Self::Token(TokenError::BadLifetime) => "lifetime",
            Self::Token(TokenError::WrongScope(_)) => "wrong-scope",
            Self::Ledger(LedgerError::Replayed) => "replayed",
            Self::Ledger(_) => "ledger",
            Self::TypedPlan(_) => "typed-plan",
        }
    }
}

/// The approval gate with a real admin key.
#[derive(Debug, Clone)]
pub struct Approver {
    admin: VerifyingKey,
    ledger: Ledger,
    disable_file: PathBuf,
    max_ttl_secs: u64,
}

/// One approval request.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// Action kind wire name (`heal-apply`, `switch`, `rollback`).
    pub action: &'a str,
    /// Exact target service.
    pub target: &'a str,
    /// Exact plan bytes.
    pub plan: &'a [u8],
    /// Token string; empty means "no token" (dry-run).
    pub token: &'a str,
}

impl Approver {
    /// Build from parts.
    ///
    /// # Errors
    /// When the ledger cannot be opened.
    pub fn new(
        admin: VerifyingKey,
        ledger: impl Into<PathBuf>,
        disable_file: impl Into<PathBuf>,
        max_ttl_secs: u64,
    ) -> Result<Self, String> {
        Ok(Self {
            admin,
            ledger: Ledger::open(ledger).map_err(|e| e.reason())?,
            disable_file: disable_file.into(),
            max_ttl_secs: max_ttl_secs.min(token::HARD_MAX_TTL_SECS),
        })
    }

    /// Build from a [`Config`].
    ///
    /// # Errors
    /// Key or ledger problems.
    pub fn from_config(cfg: &Config) -> Result<Self, String> {
        let admin = crate::keys::load_public(&cfg.admin_pubkey)?;
        Self::new(admin, &cfg.ledger, &cfg.disable_file, cfg.max_ttl_secs)
    }

    /// Always [`Mode::DryRun`]: approval never executes anything here.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        Mode::DryRun
    }

    /// The ledger.
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The disable file path.
    #[must_use]
    pub fn disable_file(&self) -> &Path {
        &self.disable_file
    }

    /// Whether the admin has disabled HADES.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        // Fail closed: if we cannot tell, treat as disabled.
        !matches!(self.disable_file.try_exists(), Ok(false))
    }

    /// Admin public key fingerprint (safe to print).
    #[must_use]
    pub fn admin_fingerprint(&self) -> String {
        crate::keys::fingerprint(&self.admin)
    }

    /// Decide one request at Unix time `now`. Refusals are written to the
    /// ledger as audit lines (best effort); approvals are written before they
    /// are returned (mandatory, fsynced).
    #[must_use]
    pub fn approve(&self, req: Request<'_>, now: u64) -> Verdict {
        let verdict = self.decide(req, now);
        match &verdict {
            Verdict::Approved(_) => {}
            Verdict::DryRun => {
                let _ = self
                    .ledger
                    .note("dry-run", req.action, req.target, "no token", "", now);
            }
            Verdict::Refused(r) => {
                let _ = self.ledger.note(
                    "refused",
                    req.action,
                    req.target,
                    &r.reason(),
                    req.token,
                    now,
                );
            }
        }
        verdict
    }

    fn decide(&self, req: Request<'_>, now: u64) -> Verdict {
        if self.is_disabled() {
            return Verdict::Refused(Refusal::Disabled);
        }
        match Action::parse(req.action) {
            Some(a) if a.is_allow_listed() => {}
            _ => return Verdict::Refused(Refusal::ActionNotApprovable(req.action.to_owned())),
        }
        if !token::valid_target(req.target) {
            return Verdict::Refused(Refusal::Token(TokenError::Malformed("target name")));
        }
        if let Err(h) = crate::safety::review(req.target, req.plan) {
            return Verdict::Refused(Refusal::Safety(h));
        }
        if req.token.is_empty() {
            return Verdict::DryRun;
        }
        let scope = Scope {
            action: req.action,
            target: req.target,
            plan: req.plan,
        };
        let claims = match token::verify(req.token, &self.admin, scope, now, self.max_ttl_secs) {
            Ok(c) => c,
            Err(e) => return Verdict::Refused(Refusal::Token(e)),
        };
        // Re-check the disable switch right before consuming, so a disable
        // that lands mid-request still wins.
        if self.is_disabled() {
            return Verdict::Refused(Refusal::Disabled);
        }
        match self.ledger.consume(&claims, now) {
            Ok(()) => Verdict::Approved(claims),
            Err(e) => Verdict::Refused(Refusal::Ledger(e)),
        }
    }

    /// Option B prototype: decide a request whose `plan` bytes are a canonical
    /// typed plan ([`crate::typed::Plan`]) instead of prose.
    ///
    /// Same order and the same disable / action / target / token / single-use
    /// checks as [`Approver::approve`]. The one difference is step 3: instead
    /// of the prose `safety::review`, the plan must parse as a canonical typed
    /// plan. A parse failure is the refusal, so there is no free text and no
    /// shell to bypass. The token still signs the exact `plan` bytes, which are
    /// the plan's canonical bytes ([`crate::typed::Plan::canonical_bytes`]).
    #[must_use]
    pub fn approve_typed(&self, req: Request<'_>, now: u64) -> Verdict {
        let verdict = self.decide_typed(req, now);
        match &verdict {
            Verdict::Approved(_) => {}
            Verdict::DryRun => {
                let _ = self.ledger.note(
                    "dry-run",
                    req.action,
                    req.target,
                    "no token (typed)",
                    "",
                    now,
                );
            }
            Verdict::Refused(r) => {
                let _ = self.ledger.note(
                    "refused",
                    req.action,
                    req.target,
                    &r.reason(),
                    req.token,
                    now,
                );
            }
        }
        verdict
    }

    fn decide_typed(&self, req: Request<'_>, now: u64) -> Verdict {
        if self.is_disabled() {
            return Verdict::Refused(Refusal::Disabled);
        }
        match Action::parse(req.action) {
            Some(a) if a.is_allow_listed() => {}
            _ => return Verdict::Refused(Refusal::ActionNotApprovable(req.action.to_owned())),
        }
        if !token::valid_target(req.target) {
            return Verdict::Refused(Refusal::Token(TokenError::Malformed("target name")));
        }
        // The top-level target is still held to the admin-safety target rule
        // (protected names like root/sshd/galion-control stay refused).
        if let Err(h) = crate::safety::review_target(req.target) {
            return Verdict::Refused(Refusal::Safety(h));
        }
        // Typed path: the plan must BE a canonical typed plan. This replaces
        // the prose safety::review of the PLAN entirely; a parse failure is
        // the refusal.
        if let Err(e) = crate::typed::Plan::parse(req.plan) {
            return Verdict::Refused(Refusal::TypedPlan(e));
        }
        if req.token.is_empty() {
            return Verdict::DryRun;
        }
        let scope = Scope {
            action: req.action,
            target: req.target,
            plan: req.plan,
        };
        let claims = match token::verify(req.token, &self.admin, scope, now, self.max_ttl_secs) {
            Ok(c) => c,
            Err(e) => return Verdict::Refused(Refusal::Token(e)),
        };
        if self.is_disabled() {
            return Verdict::Refused(Refusal::Disabled);
        }
        match self.ledger.consume(&claims, now) {
            Ok(()) => Verdict::Approved(claims),
            Err(e) => Verdict::Refused(Refusal::Ledger(e)),
        }
    }
}

/// Current Unix time in seconds.
#[must_use]
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
