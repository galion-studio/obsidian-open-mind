// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option B prototype: typed, signed plan steps.
//!
//! The admin-safety red team (docs/security/hades-redteam.md) left HADES with
//! one honest residual risk: a plan is **prose**. The allow-list keeps a plan
//! from *being* a shell command, but the words after an approved verb are still
//! free text, so HADES must never feed plan text to a shell. Option B removes
//! the parser problem at the root: a plan is a closed list of **typed steps**,
//! each carrying validated, typed fields. There is nothing to bypass because
//! there is no free text and no shell — the capability set is the `enum`.
//!
//! ## Shape
//!
//! * [`Plan`] is an ordered, non-empty, bounded list of [`Step`]s.
//! * [`Step`] is a closed enum. No variant can delete, overwrite, exfiltrate,
//!   or run arbitrary text. An operator runbook is a closed [`Runbook`] enum,
//!   not a path or a command. A service is a validated [`Service`] that reuses
//!   the admin-safety target rule (so `sshd`, `root`, `galion-control`, … are
//!   still refused). A probe path forbids `..`.
//! * [`Plan::canonical`] is a deterministic text encoding. **These canonical
//!   bytes are what the approval token hashes and signs** (`token::plan_hash`),
//!   exactly as the prose bytes were in Option A — the wire and ledger are
//!   unchanged, only the plan's grammar is now typed.
//! * [`Plan::parse`] is strict: it only accepts input that re-encodes to the
//!   same bytes (same discipline as `token::Claims::parse_payload`). Anything
//!   that is not already a canonical typed plan is rejected before any token is
//!   looked at. This is what replaces `safety::review` on the typed path.
//!
//! This module is a prototype: it adds a parallel typed path
//! ([`crate::approval::Approver::approve_typed`]) and leaves the prose path and
//! its red-team corpus untouched, so the 82 attacks stay at 0 on both paths.

use crate::safety::{self, Hazard};

/// Longest canonical plan the typed path will parse, in bytes.
pub const MAX_PLAN_BYTES: usize = 8 * 1024;
/// Most steps one plan may carry.
pub const MAX_STEPS: usize = 64;
/// Magic first line; every canonical plan starts with it.
pub const MAGIC: &str = "hades-plan/v1";
/// Longest release identifier.
pub const MAX_RELEASE_BYTES: usize = 64;
/// Longest probe path.
pub const MAX_PATH_BYTES: usize = 128;
/// Longest probe endpoint (`host:port`).
pub const MAX_ENDPOINT_BYTES: usize = 261;
/// Ceiling for `apply-fixes max=`.
pub const MAX_FIXES: u16 = 100;

/// Why a byte string is not a canonical typed plan. None reveal key material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// Empty, too long, or too many steps.
    Size(&'static str),
    /// Not printable ASCII, or not the canonical byte-for-byte form.
    NonCanonical(&'static str),
    /// Missing or wrong `hades-plan/v1` header.
    BadHeader,
    /// A step keyword is not one of the known steps.
    UnknownStep(String),
    /// A step's arguments are malformed.
    BadArgs(&'static str),
    /// A field failed validation (names the field).
    BadField(&'static str),
    /// A [`Service`] field hit the admin-safety target rule.
    Service(Hazard),
    /// A service is not in the operator inventory ([`Inventory`]).
    NotInInventory(String),
}

impl PlanError {
    /// Human-readable reason.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Size(m) => format!("typed plan size: {m}"),
            Self::NonCanonical(m) => format!("typed plan not canonical: {m}"),
            Self::BadHeader => format!("typed plan must start with the {MAGIC:?} line"),
            Self::UnknownStep(s) => format!("unknown plan step {s:?}"),
            Self::BadArgs(m) => format!("malformed step arguments: {m}"),
            Self::BadField(m) => format!("invalid {m}"),
            Self::Service(h) => h.reason(),
            Self::NotInInventory(s) => format!("service {s:?} is not in the inventory"),
        }
    }

    /// Short stable code for JSON output and tests.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Size(_) => "size",
            Self::NonCanonical(_) => "non-canonical",
            Self::BadHeader => "header",
            Self::UnknownStep(_) => "unknown-step",
            Self::BadArgs(_) => "args",
            Self::BadField(_) => "field",
            Self::Service(_) => "service",
            Self::NotInInventory(_) => "inventory",
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for PlanError {}

/// A validated service name. Same rule as the admin-safety target check, so a
/// protected target (`hades`, `sshd`, `root`, `galion-control`, …) can never
/// appear in a typed plan either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service(String);

impl Service {
    /// Validate and wrap.
    ///
    /// # Errors
    /// [`PlanError::Service`] when the admin-safety target rule refuses it.
    pub fn new(name: &str) -> Result<Self, PlanError> {
        safety::review_target(name).map_err(PlanError::Service)?;
        Ok(Self(name.to_owned()))
    }

    /// The service name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A release identifier: 1..=64 of `A-Z a-z 0-9 . _ : -`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release(String);

impl Release {
    /// Validate and wrap.
    ///
    /// # Errors
    /// [`PlanError::BadField`] on an empty, overlong or out-of-charset value.
    pub fn new(id: &str) -> Result<Self, PlanError> {
        let ok = !id.is_empty()
            && id.len() <= MAX_RELEASE_BYTES
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
        if ok {
            Ok(Self(id.to_owned()))
        } else {
            Err(PlanError::BadField("release"))
        }
    }

    /// The release id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A read-only health probe: an optional `host:port` endpoint and a URL path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    endpoint: Option<String>,
    path: String,
}

impl Probe {
    /// Validate and wrap. `endpoint` is `host:port`, `path` starts with `/`.
    ///
    /// # Errors
    /// [`PlanError::BadField`] when either part is out of its charset. Neither
    /// may contain a shell metacharacter, and the path may not contain `..`.
    pub fn new(endpoint: Option<&str>, path: &str) -> Result<Self, PlanError> {
        if let Some(ep) = endpoint {
            if !valid_endpoint(ep) {
                return Err(PlanError::BadField("probe endpoint"));
            }
        }
        let path_ok = !path.is_empty()
            && path.len() <= MAX_PATH_BYTES
            && path.starts_with('/')
            && !path.contains("..")
            && path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'));
        if !path_ok {
            return Err(PlanError::BadField("probe path"));
        }
        Ok(Self {
            endpoint: endpoint.map(str::to_owned),
            path: path.to_owned(),
        })
    }

    /// The `host:port` endpoint, if any.
    #[must_use]
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// The URL path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

fn valid_endpoint(ep: &str) -> bool {
    if ep.len() > MAX_ENDPOINT_BYTES {
        return false;
    }
    let Some((host, port)) = ep.rsplit_once(':') else {
        return false;
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    let port_ok = !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u16>().map(|p| p > 0).unwrap_or(false);
    host_ok && port_ok
}

/// A named, allow-listed operator runbook. A closed set, never a path or a
/// command: this is what replaces "operator runs deploy/rollback.sh" as free
/// text. Add a variant (and its name) deliberately, with a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runbook {
    /// `deploy/rollback.sh`.
    DeployRollback,
    /// `deploy/rollback-console.sh`.
    DeployRollbackConsole,
}

impl Runbook {
    /// Wire name used in the canonical form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeployRollback => "deploy-rollback",
            Self::DeployRollbackConsole => "deploy-rollback-console",
        }
    }

    /// Parse a wire name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "deploy-rollback" => Self::DeployRollback,
            "deploy-rollback-console" => Self::DeployRollbackConsole,
            _ => return None,
        })
    }
}

/// A named source of findings for a heal plan. Closed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The DEMETER heal-dry analysis.
    Demeter,
}

impl Source {
    /// Wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Demeter => "demeter",
        }
    }

    /// Parse a wire name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "demeter" => Some(Self::Demeter),
            _ => None,
        }
    }
}

/// One typed plan step. A closed capability set: there is no "run arbitrary
/// text" or "delete" variant, by construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Read the current and previous release lineage (read-only).
    ReadReleases,
    /// Restart a service.
    Restart(Service),
    /// Switch a service to a release.
    Switch { service: Service, release: Release },
    /// Roll a service back to a release.
    Rollback { service: Service, release: Release },
    /// Run a read-only health probe.
    Verify(Probe),
    /// The operator runs a named, allow-listed runbook.
    Operator(Runbook),
    /// Take heal-dry findings from a named source.
    TakeFindings(Source),
    /// Apply at most `max` allow-listed fixes (1..=100).
    ApplyFixes { max: u16 },
    /// Append every action to `actions.jsonl`.
    AppendActions,
    /// Keep the failed release for inspection.
    KeepFailed,
    /// Record the plan in lineage.
    RecordLineage,
}

impl Step {
    /// The canonical one-line encoding of this step.
    #[must_use]
    pub fn canonical(&self) -> String {
        match self {
            Self::ReadReleases => "read-releases".into(),
            Self::Restart(s) => format!("restart {}", s.as_str()),
            Self::Switch { service, release } => {
                format!("switch {} release={}", service.as_str(), release.as_str())
            }
            Self::Rollback { service, release } => {
                format!("rollback {} release={}", service.as_str(), release.as_str())
            }
            Self::Verify(p) => match p.endpoint() {
                Some(ep) => format!("verify {ep} {}", p.path()),
                None => format!("verify {}", p.path()),
            },
            Self::Operator(r) => format!("operator {}", r.as_str()),
            Self::TakeFindings(src) => format!("take-findings {}", src.as_str()),
            Self::ApplyFixes { max } => format!("apply-fixes max={max}"),
            Self::AppendActions => "append-actions".into(),
            Self::KeepFailed => "keep-failed".into(),
            Self::RecordLineage => "record-lineage".into(),
        }
    }

    fn parse_line(line: &str) -> Result<Self, PlanError> {
        let mut it = line.split(' ');
        let kw = it.next().unwrap_or("");
        // Collect the rest once; each arm validates its own arity.
        let rest: Vec<&str> = it.collect();
        let step = match kw {
            "read-releases" => {
                need_args(&rest, 0)?;
                Self::ReadReleases
            }
            "restart" => {
                need_args(&rest, 1)?;
                Self::Restart(Service::new(rest[0])?)
            }
            "switch" => {
                need_args(&rest, 2)?;
                Self::Switch {
                    service: Service::new(rest[0])?,
                    release: Release::new(field(rest[1], "release=")?)?,
                }
            }
            "rollback" => {
                need_args(&rest, 2)?;
                Self::Rollback {
                    service: Service::new(rest[0])?,
                    release: Release::new(field(rest[1], "release=")?)?,
                }
            }
            "verify" => match rest.len() {
                1 => Self::Verify(Probe::new(None, rest[0])?),
                2 => Self::Verify(Probe::new(Some(rest[0]), rest[1])?),
                _ => return Err(PlanError::BadArgs("verify takes [endpoint] path")),
            },
            "operator" => {
                need_args(&rest, 1)?;
                Self::Operator(
                    Runbook::parse(rest[0]).ok_or(PlanError::BadField("operator runbook"))?,
                )
            }
            "take-findings" => {
                need_args(&rest, 1)?;
                Self::TakeFindings(
                    Source::parse(rest[0]).ok_or(PlanError::BadField("findings source"))?,
                )
            }
            "apply-fixes" => {
                need_args(&rest, 1)?;
                let n: u16 = field(rest[0], "max=")?
                    .parse()
                    .map_err(|_| PlanError::BadField("apply-fixes max"))?;
                if n == 0 || n > MAX_FIXES {
                    return Err(PlanError::BadField("apply-fixes max"));
                }
                Self::ApplyFixes { max: n }
            }
            "append-actions" => {
                need_args(&rest, 0)?;
                Self::AppendActions
            }
            "keep-failed" => {
                need_args(&rest, 0)?;
                Self::KeepFailed
            }
            "record-lineage" => {
                need_args(&rest, 0)?;
                Self::RecordLineage
            }
            other => return Err(PlanError::UnknownStep(other.to_owned())),
        };
        Ok(step)
    }
}

fn need_args(rest: &[&str], n: usize) -> Result<(), PlanError> {
    if rest.len() == n {
        Ok(())
    } else {
        Err(PlanError::BadArgs("wrong number of step arguments"))
    }
}

fn field<'a>(tok: &'a str, key: &'static str) -> Result<&'a str, PlanError> {
    tok.strip_prefix(key).ok_or(PlanError::BadArgs(key))
}

/// An ordered, non-empty, bounded list of typed steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    steps: Vec<Step>,
}

impl Plan {
    /// Build from steps.
    ///
    /// # Errors
    /// [`PlanError::Size`] for an empty plan or more than [`MAX_STEPS`].
    pub fn new(steps: Vec<Step>) -> Result<Self, PlanError> {
        if steps.is_empty() {
            return Err(PlanError::Size("empty plan"));
        }
        if steps.len() > MAX_STEPS {
            return Err(PlanError::Size("too many steps"));
        }
        Ok(Self { steps })
    }

    /// The steps.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The canonical text encoding: the magic line, then one step per line,
    /// joined with `\n`, no trailing newline.
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut out = String::from(MAGIC);
        for s in &self.steps {
            out.push('\n');
            out.push_str(&s.canonical());
        }
        out
    }

    /// The exact bytes the approval token hashes and signs.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.canonical().into_bytes()
    }

    /// Parse canonical bytes into a plan. Strict: the input must re-encode to
    /// the same bytes, so only an already-canonical typed plan is accepted.
    ///
    /// # Errors
    /// The first [`PlanError`] found. This is the typed path's replacement for
    /// `safety::review`: a parse failure is the refusal.
    pub fn parse(bytes: &[u8]) -> Result<Self, PlanError> {
        if bytes.is_empty() {
            return Err(PlanError::Size("empty plan"));
        }
        if bytes.len() > MAX_PLAN_BYTES {
            return Err(PlanError::Size("plan too long"));
        }
        // Printable ASCII plus '\n' only: no NUL/CR, no non-ASCII, no tab.
        if !bytes
            .iter()
            .all(|b| *b == b'\n' || (0x20..0x7f).contains(b))
        {
            return Err(PlanError::NonCanonical("non-printable or non-ASCII byte"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| PlanError::NonCanonical("utf-8"))?;
        let mut lines = text.split('\n');
        if lines.next() != Some(MAGIC) {
            return Err(PlanError::BadHeader);
        }
        let mut steps = Vec::new();
        for line in lines {
            steps.push(Step::parse_line(line)?);
        }
        let plan = Self::new(steps)?;
        // Canonicality: the parse must round-trip byte-for-byte. This rejects
        // trailing spaces, blank lines, odd spacing, trailing newlines, etc.
        if plan.canonical().as_bytes() != bytes {
            return Err(PlanError::NonCanonical("input is not the canonical form"));
        }
        Ok(plan)
    }

    /// A human-readable rendering (for a console or the ledger). Display only;
    /// it is never what is signed. The signed artifact is [`Plan::canonical`].
    #[must_use]
    pub fn render_prose(&self) -> String {
        self.steps
            .iter()
            .map(|s| match s {
                Step::ReadReleases => "read the current and the previous release".into(),
                Step::Restart(sv) => format!("restart {}", sv.as_str()),
                Step::Switch { service, release } => {
                    format!(
                        "switch {} to release {}",
                        service.as_str(),
                        release.as_str()
                    )
                }
                Step::Rollback { service, release } => {
                    format!(
                        "roll {} back to release {}",
                        service.as_str(),
                        release.as_str()
                    )
                }
                Step::Verify(p) => match p.endpoint() {
                    Some(ep) => format!("verify {} on {ep}", p.path()),
                    None => format!("verify {}", p.path()),
                },
                Step::Operator(r) => format!("operator runs the {} runbook", r.as_str()),
                Step::TakeFindings(src) => {
                    format!("take the heal-dry findings from {}", src.as_str())
                }
                Step::ApplyFixes { max } => format!("apply at most {max} allow-listed fixes"),
                Step::AppendActions => "append every action to actions.jsonl".into(),
                Step::KeepFailed => "keep the failed release for inspection".into(),
                Step::RecordLineage => "record the plan in lineage".into(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// An operator-supplied inventory of the services a typed plan may name.
///
/// The admin-safety target rule is a **deny-list** of protected names. Against
/// the real VPS unit list (docs/real-tests/hades-option-b.md) it still lets
/// security units such as `firewalld`, `fail2ban`, `auditd` or `apparmor`
/// through, and it cannot tell that a plan names a unit that does not exist
/// (the corpus `galion-tuner` is `tuner` on the VPS). An inventory closes both:
/// every [`Service`] in a plan must be one of the listed names, exactly.
///
/// Prototype only: [`crate::approval::Approver::approve_typed`] does not load
/// an inventory yet (that is an owner decision, see docs/security/hades-option-b.md).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    names: Vec<String>,
}

impl Inventory {
    /// Build from names. Every name must itself pass [`Service::new`], so a
    /// protected name can never be put on the list.
    ///
    /// # Errors
    /// The first name that fails [`Service::new`].
    pub fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self, PlanError> {
        let mut out: Vec<String> = Vec::new();
        for n in names {
            Service::new(n)?;
            if !out.iter().any(|x| x == n) {
                out.push(n.to_owned());
            }
        }
        out.sort();
        Ok(Self { names: out })
    }

    /// Whether `name` is listed (exact, case-sensitive).
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names
            .binary_search_by(|x| x.as_str().cmp(name))
            .is_ok()
    }

    /// The listed names, sorted.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Check every service a plan names.
    ///
    /// # Errors
    /// [`PlanError::NotInInventory`] for the first unlisted service.
    pub fn check(&self, plan: &Plan) -> Result<(), PlanError> {
        for s in plan.steps() {
            let svc = match s {
                Step::Restart(sv) => sv,
                Step::Switch { service, .. } | Step::Rollback { service, .. } => service,
                _ => continue,
            };
            if !self.contains(svc.as_str()) {
                return Err(PlanError::NotInInventory(svc.as_str().to_owned()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Plan {
        Plan::new(vec![
            Step::ReadReleases,
            Step::Restart(Service::new("galion-tuner").unwrap()),
            Step::Switch {
                service: Service::new("galion-one").unwrap(),
                release: Release::new("2026-10-20").unwrap(),
            },
            Step::Verify(Probe::new(Some("127.0.0.1:8420"), "/health").unwrap()),
            Step::Verify(Probe::new(None, "/health").unwrap()),
            Step::Operator(Runbook::DeployRollback),
            Step::TakeFindings(Source::Demeter),
            Step::ApplyFixes { max: 12 },
            Step::AppendActions,
            Step::KeepFailed,
            Step::RecordLineage,
        ])
        .unwrap()
    }

    #[test]
    fn round_trips_byte_for_byte() {
        let p = sample();
        let bytes = p.canonical_bytes();
        let back = Plan::parse(&bytes).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.canonical_bytes(), bytes);
    }

    #[test]
    fn canonical_is_what_we_expect() {
        let p = Plan::new(vec![
            Step::Restart(Service::new("galion-tuner").unwrap()),
            Step::Verify(Probe::new(Some("127.0.0.1:8420"), "/health").unwrap()),
        ])
        .unwrap();
        assert_eq!(
            p.canonical(),
            "hades-plan/v1\nrestart galion-tuner\nverify 127.0.0.1:8420 /health"
        );
    }

    #[test]
    fn protected_service_is_refused() {
        for bad in ["sshd", "root", "hades-gate", "galion-control", "admin"] {
            assert!(
                matches!(Service::new(bad), Err(PlanError::Service(_))),
                "{bad}"
            );
        }
        assert!(Service::new("galion-tuner").is_ok());
    }

    #[test]
    fn non_canonical_inputs_are_refused() {
        for bad in [
            "restart galion-tuner",                        // no header
            "hades-plan/v1",                               // no steps
            "hades-plan/v1\n",                             // trailing blank line
            "hades-plan/v1\nrestart galion-tuner\n",       // trailing newline
            "hades-plan/v1\nrestart  galion-tuner",        // double space
            "hades-plan/v1\n restart galion-tuner",        // leading space
            "hades-plan/v1\nrestart galion-tuner ",        // trailing space
            "hades-plan/v2\nrestart galion-tuner",         // wrong version
            "hades-plan/v1\nwipe galion-tuner",            // unknown step
            "hades-plan/v1\nswitch galion-one 2026-10-20", // missing release= key
        ] {
            assert!(Plan::parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn probe_path_rejects_traversal_and_metachars() {
        assert!(Probe::new(None, "/health").is_ok());
        assert!(Probe::new(None, "../../root/.ssh/").is_err());
        assert!(Probe::new(None, "/a;b").is_err());
        assert!(Probe::new(Some("127.0.0.1:8420"), "/health").is_ok());
        assert!(Probe::new(Some("127.0.0.1:0"), "/health").is_err());
        assert!(Probe::new(Some("x y:80"), "/health").is_err());
    }

    #[test]
    fn inventory_is_exact_and_refuses_protected_names() {
        assert!(Inventory::new(["tuner", "sshd"]).is_err());
        let inv = Inventory::new(["tuner", "galion-one", "tuner"]).unwrap();
        assert_eq!(inv.names(), ["galion-one", "tuner"]);
        let ok =
            Plan::parse(b"hades-plan/v1\nrestart tuner\nswitch galion-one release=r1").unwrap();
        assert!(inv.check(&ok).is_ok());
        for bad in [
            "hades-plan/v1\nrestart galion-tuner",
            "hades-plan/v1\nrestart Tuner",
            "hades-plan/v1\nrestart firewalld",
            "hades-plan/v1\nrecord-lineage\nrollback fail2ban release=r1",
        ] {
            let p = Plan::parse(bad.as_bytes()).unwrap();
            assert!(
                matches!(inv.check(&p), Err(PlanError::NotInInventory(_))),
                "{bad:?}"
            );
        }
        // Steps without a service are not affected.
        let p = Plan::parse(b"hades-plan/v1\nread-releases\nverify /health").unwrap();
        assert!(inv.check(&p).is_ok());
    }
}
