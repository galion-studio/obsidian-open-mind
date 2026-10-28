// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Compute providers and an approval registry.
//!
//! A [`ComputeProvider`] reports its id, kind, endpoint, a health snapshot,
//! and capabilities. [`Registry`] stores those rows as JSON at a path the
//! caller supplies.
//!
//! A newly registered row is always [`Approval::Pending`]. Nothing in this
//! crate opens a socket: [`ComputeProvider::health`] is a snapshot the caller
//! already holds, and [`Registry::set_health`] only records that snapshot.
//! The file stores endpoints and approval, never key material.
//!
//! On-disk shape (version 1), unknown fields rejected:
//!
//! - `version`: `1`
//! - `providers[]`: `id`, `kind`, `endpoint`, `capabilities`, `approval`, `health`,
//!   and optionally `vram_gb` (1..=4096) and `pending_endpoint` (a known device
//!   seen at a new endpoint; applied only by the next approval)
//!
//! Routing sees approved rows only: [`Registry::approved_rows`] feeds
//! [`fallback::select_provider`] / [`route::chain`] (GPU, then LAN, then CPU).
//!
//! `kind` is `cpu_local`, `lan`, `gpu`, or `remote`.
//! `approval` is `pending`, `approved`, or `rejected`.
//! `health` is `unknown`, `up`, or `down`.
//! `capabilities` is a list of `llm`, `stt`, `tts`, `vision`.

#![forbid(unsafe_code)]

/// Provider fallback policy (was the openmind-fallback crate until 2026-10-20).
pub mod fallback;
/// Route one need to a provider row (was the openmind-route crate until 2026-10-20).
pub mod route;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// JSON document version written by [`Registry::save`].
pub const REGISTRY_VERSION: u32 = 1;

/// Where the compute sits. This is not the schema's Remote/Local row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// A process on this CPU-light box.
    CpuLocal,
    /// A machine on the local network. Stays pending until approved.
    Lan,
    /// An external GPU host. This box only relays.
    Gpu,
    /// A remote API site, not a LAN device.
    Remote,
}

/// Approval of one provider row. New rows start at [`Approval::Pending`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// Registered, not yet accepted. Not eligible for use.
    Pending,
    /// Explicitly accepted.
    Approved,
    /// Explicitly refused.
    Rejected,
}

/// Last health snapshot. Recording it does not contact the endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// No snapshot has been recorded.
    Unknown,
    /// Last snapshot said the endpoint was up.
    Up,
    /// Last snapshot said the endpoint was down.
    Down,
}

/// One thing a provider can do. Stored in the order given, without duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Chat or completion model.
    Llm,
    /// Speech to text.
    Stt,
    /// Text to speech.
    Tts,
    /// Image or video input.
    Vision,
}

impl Capability {
    /// Stable JSON name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::Llm => "llm",
            Capability::Stt => "stt",
            Capability::Tts => "tts",
            Capability::Vision => "vision",
        }
    }
}

/// A compute adapter. Methods are pure: `health` must not open a socket.
pub trait ComputeProvider {
    /// Stable id, unique inside one registry.
    fn id(&self) -> &str;
    /// Where the compute sits.
    fn kind(&self) -> ProviderKind;
    /// Endpoint string. Stored, never dialed by this crate.
    fn endpoint(&self) -> &str;
    /// Health snapshot the caller already has.
    fn health(&self) -> Health;
    /// Capabilities, in display order.
    fn capabilities(&self) -> &[Capability];
    /// VRAM in GB the device advertises, if any. Used by the POSEIDON pool.
    fn vram_gb(&self) -> Option<u32> {
        None
    }
}

/// In-memory provider used by tests and by callers that already know the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticProvider {
    id: String,
    kind: ProviderKind,
    endpoint: String,
    health: Health,
    capabilities: Vec<Capability>,
    vram_gb: Option<u32>,
}

impl StaticProvider {
    /// Build a provider with [`Health::Unknown`]. Rejects empty or illegal fields.
    pub fn try_new(
        id: impl Into<String>,
        kind: ProviderKind,
        endpoint: impl Into<String>,
        capabilities: impl Into<Vec<Capability>>,
    ) -> Result<Self, ProviderError> {
        let id = id.into();
        let endpoint = endpoint.into();
        let capabilities = capabilities.into();
        check_id(&id)?;
        check_endpoint(&endpoint)?;
        check_capabilities(&capabilities)?;
        Ok(Self {
            id,
            kind,
            endpoint,
            health: Health::Unknown,
            capabilities,
            vram_gb: None,
        })
    }

    /// Replace the health snapshot. Does not contact the endpoint.
    #[must_use]
    pub fn with_health(mut self, health: Health) -> Self {
        self.health = health;
        self
    }

    /// Set the advertised VRAM (GB). `None` clears it. Checked by [`Registry::register`].
    #[must_use]
    pub fn with_vram_gb(mut self, vram_gb: Option<u32>) -> Self {
        self.vram_gb = vram_gb;
        self
    }
}

impl ComputeProvider for StaticProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn health(&self) -> Health {
        self.health
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    fn vram_gb(&self) -> Option<u32> {
        self.vram_gb
    }
}

/// One persisted row. Fields stay private so approval only changes via [`Registry`].
///
/// `vram_gb` and `pending_endpoint` are optional (absent in older files and
/// omitted when unset, so a file without them is written unchanged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRecord {
    id: String,
    kind: ProviderKind,
    endpoint: String,
    capabilities: Vec<Capability>,
    approval: Approval,
    health: Health,
    /// Advertised VRAM in GB (discovery TXT `vram`), for the POSEIDON pool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vram_gb: Option<u32>,
    /// A known device was seen at this endpoint instead of `endpoint`. It is
    /// not used until the row is approved again, which moves it into `endpoint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_endpoint: Option<String>,
}

impl ProviderRecord {
    /// Approval state stored for this row.
    pub fn approval(&self) -> Approval {
        self.approval
    }

    /// Endpoint a known device moved to, waiting for re-approval.
    pub fn pending_endpoint(&self) -> Option<&str> {
        self.pending_endpoint.as_deref()
    }
}

impl ComputeProvider for ProviderRecord {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn health(&self) -> Health {
        self.health
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    fn vram_gb(&self) -> Option<u32> {
        self.vram_gb
    }
}

/// Registry load, validate, or save failure.
#[derive(Debug)]
pub enum ProviderError {
    /// Id is empty or outside the allowed ASCII set.
    Id { message: String },
    /// Endpoint is empty or not a short ASCII token string.
    Endpoint { message: String },
    /// Capability list repeats a value.
    Capability { message: String },
    /// `vram_gb` is outside 1..=4096.
    Vram { message: String },
    /// `register` saw an id that is already present.
    Duplicate { id: String },
    /// No row has this id.
    NotFound { id: String },
    /// The path could not be read or written.
    Io { path: PathBuf, message: String },
    /// The file is not a version-1 registry document.
    Parse { path: PathBuf, message: String },
    /// `version` is present but not [`REGISTRY_VERSION`].
    Version { path: PathBuf, found: u32 },
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Id { message }
            | ProviderError::Endpoint { message }
            | ProviderError::Capability { message }
            | ProviderError::Vram { message } => f.write_str(message),
            ProviderError::Duplicate { id } => write!(f, "duplicate provider id {id}"),
            ProviderError::NotFound { id } => write!(f, "unknown provider id {id}"),
            ProviderError::Io { path, message } | ProviderError::Parse { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            ProviderError::Version { path, found } => write!(
                f,
                "{}: version {found} is not supported (want {REGISTRY_VERSION})",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    providers: Vec<ProviderRecord>,
}

/// JSON registry. The path is whatever the caller passed to [`Registry::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    path: PathBuf,
    providers: Vec<ProviderRecord>,
}

impl Registry {
    /// Load `path`. A missing file is an empty registry and is not created.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProviderError> {
        let path = path.as_ref().to_path_buf();
        match fs::read(&path) {
            Ok(bytes) => Self::from_bytes(&path, &bytes),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self {
                path,
                providers: Vec::new(),
            }),
            Err(err) => Err(io_at(&path, "read", err)),
        }
    }

    /// Path supplied by the caller. Not canonicalized.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Rows in registration order.
    pub fn providers(&self) -> &[ProviderRecord] {
        &self.providers
    }

    /// Number of rows, whatever their approval state.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// True when the registry has no rows.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// One row by id.
    pub fn get(&self, id: &str) -> Option<&ProviderRecord> {
        self.providers.iter().find(|row| row.id == id)
    }

    /// Rows with this approval, in registration order.
    pub fn with_approval(&self, approval: Approval) -> impl Iterator<Item = &ProviderRecord> + '_ {
        self.providers
            .iter()
            .filter(move |row| row.approval == approval)
    }

    /// Copy `provider` into the registry as [`Approval::Pending`] and write the file.
    ///
    /// Health is copied from `provider.health()` and is not probed.
    /// On a write failure the in-memory registry is left unchanged.
    pub fn register(&mut self, provider: &dyn ComputeProvider) -> Result<(), ProviderError> {
        let id = provider.id().to_string();
        let endpoint = provider.endpoint().to_string();
        let capabilities = provider.capabilities().to_vec();
        check_id(&id)?;
        check_endpoint(&endpoint)?;
        check_capabilities(&capabilities)?;
        let vram_gb = provider.vram_gb();
        check_vram(vram_gb)?;
        if self.providers.iter().any(|row| row.id == id) {
            return Err(ProviderError::Duplicate { id });
        }
        self.providers.push(ProviderRecord {
            id,
            kind: provider.kind(),
            endpoint,
            capabilities,
            approval: Approval::Pending,
            health: provider.health(),
            vram_gb,
            pending_endpoint: None,
        });
        if let Err(err) = self.persist() {
            self.providers.pop();
            return Err(err);
        }
        Ok(())
    }

    /// Set approval and write the file. Any transition is allowed.
    ///
    /// Approving a row that has a [`ProviderRecord::pending_endpoint`] (a
    /// known device that moved) is the re-approval of the move: the pending
    /// endpoint becomes the endpoint and health resets to
    /// [`Health::Unknown`] until the next `join`. Pending or rejected keep
    /// the old endpoint and the pending one as they are.
    ///
    /// On a write failure the previous row is restored in memory.
    pub fn set_approval(&mut self, id: &str, approval: Approval) -> Result<(), ProviderError> {
        let index = self.index_of(id)?;
        let previous = self.providers[index].clone();
        let row = &mut self.providers[index];
        row.approval = approval;
        if approval == Approval::Approved {
            if let Some(moved) = row.pending_endpoint.take() {
                row.endpoint = moved;
                row.health = Health::Unknown;
            }
        }
        if let Err(err) = self.persist() {
            self.providers[index] = previous;
            return Err(err);
        }
        Ok(())
    }

    /// Record that a known row was seen at `endpoint`.
    ///
    /// The row's routed endpoint, approval, and health do not change: the
    /// new endpoint is only kept as [`ProviderRecord::pending_endpoint`]
    /// until the row is approved again. Seeing the row at its current
    /// endpoint clears a stale pending move. Returns `true` when the row
    /// changed (and the file was written).
    pub fn note_endpoint(&mut self, id: &str, endpoint: &str) -> Result<bool, ProviderError> {
        check_endpoint(endpoint)?;
        let index = self.index_of(id)?;
        let row = &self.providers[index];
        let wanted = if row.endpoint == endpoint {
            None
        } else {
            Some(endpoint.to_string())
        };
        if row.pending_endpoint == wanted {
            return Ok(false);
        }
        let previous = row.pending_endpoint.clone();
        self.providers[index].pending_endpoint = wanted;
        if let Err(err) = self.persist() {
            self.providers[index].pending_endpoint = previous;
            return Err(err);
        }
        Ok(true)
    }

    /// The rows routing may use, as [`fallback::ProviderRow`]s.
    ///
    /// Only [`Approval::Approved`] rows are returned; pending and rejected
    /// rows are left out entirely, so a caller cannot route to them by
    /// mistake. A [`ProviderKind::Lan`] row must also have been joined: its
    /// health is passed through only when it is [`Health::Up`], otherwise
    /// (never joined, or joined and down) it is reported as
    /// [`fallback::Health::Down`] and skipped by the policy. Other kinds keep
    /// their health as is. A pending move is ignored: the row is routed at
    /// its approved endpoint.
    pub fn approved_rows(&self) -> Vec<fallback::ProviderRow> {
        self.with_approval(Approval::Approved)
            .map(|row| {
                let health = match (row.kind, row.health) {
                    (_, Health::Up) => fallback::Health::Up,
                    (ProviderKind::Lan, _) | (_, Health::Down) => fallback::Health::Down,
                    (_, Health::Unknown) => fallback::Health::Unknown,
                };
                fallback::ProviderRow {
                    id: row.id.clone(),
                    kind: match row.kind {
                        ProviderKind::CpuLocal => fallback::Kind::CpuLocal,
                        ProviderKind::Lan => fallback::Kind::Lan,
                        ProviderKind::Gpu => fallback::Kind::Gpu,
                        ProviderKind::Remote => fallback::Kind::Remote,
                    },
                    endpoint: row.endpoint.clone(),
                    approval: fallback::Approval::Approved,
                    health,
                    capabilities: row
                        .capabilities
                        .iter()
                        .map(|cap| match cap {
                            Capability::Llm => fallback::Capability::Llm,
                            Capability::Stt => fallback::Capability::Stt,
                            Capability::Tts => fallback::Capability::Tts,
                            Capability::Vision => fallback::Capability::Vision,
                        })
                        .collect(),
                }
            })
            .collect()
    }

    /// Record a health snapshot and write the file. Does not contact the endpoint.
    ///
    /// On a write failure the previous snapshot is restored in memory.
    pub fn set_health(&mut self, id: &str, health: Health) -> Result<(), ProviderError> {
        let index = self.index_of(id)?;
        let previous = self.providers[index].health;
        self.providers[index].health = health;
        if let Err(err) = self.persist() {
            self.providers[index].health = previous;
            return Err(err);
        }
        Ok(())
    }

    /// Write the current rows to the caller-supplied path.
    pub fn save(&self) -> Result<(), ProviderError> {
        self.persist()
    }

    fn index_of(&self, id: &str) -> Result<usize, ProviderError> {
        self.providers
            .iter()
            .position(|row| row.id == id)
            .ok_or_else(|| ProviderError::NotFound { id: id.to_string() })
    }

    fn persist(&self) -> Result<(), ProviderError> {
        let document = Document {
            version: REGISTRY_VERSION,
            providers: self.providers.clone(),
        };
        write_atomic(&self.path, &document)
    }

    fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self, ProviderError> {
        let document: Document =
            serde_json::from_slice(bytes).map_err(|err| ProviderError::Parse {
                path: path.to_path_buf(),
                message: err.to_string(),
            })?;
        if document.version != REGISTRY_VERSION {
            return Err(ProviderError::Version {
                path: path.to_path_buf(),
                found: document.version,
            });
        }
        let mut seen = HashSet::with_capacity(document.providers.len());
        for row in &document.providers {
            validate_loaded(path, row)?;
            if !seen.insert(row.id.clone()) {
                return Err(ProviderError::Parse {
                    path: path.to_path_buf(),
                    message: format!("duplicate provider id {}", row.id),
                });
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            providers: document.providers,
        })
    }
}

fn validate_loaded(path: &Path, row: &ProviderRecord) -> Result<(), ProviderError> {
    check_id(&row.id).map_err(|err| as_parse(path, err))?;
    check_endpoint(&row.endpoint).map_err(|err| as_parse(path, err))?;
    check_capabilities(&row.capabilities).map_err(|err| as_parse(path, err))?;
    check_vram(row.vram_gb).map_err(|err| as_parse(path, err))?;
    if let Some(moved) = &row.pending_endpoint {
        check_endpoint(moved).map_err(|err| as_parse(path, err))?;
    }
    Ok(())
}

fn as_parse(path: &Path, err: ProviderError) -> ProviderError {
    ProviderError::Parse {
        path: path.to_path_buf(),
        message: err.to_string(),
    }
}

fn check_id(id: &str) -> Result<(), ProviderError> {
    if id.is_empty() {
        return Err(ProviderError::Id {
            message: "provider id is empty".into(),
        });
    }
    if !valid_id(id) {
        let message = if id.len() <= 64 {
            format!("provider id {id:?} must be 1..=64 bytes from [A-Za-z0-9._-]")
        } else {
            "provider id must be 1..=64 bytes from [A-Za-z0-9._-]".into()
        };
        return Err(ProviderError::Id { message });
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 || id == "." || id == ".." {
        return false;
    }
    id.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn check_endpoint(endpoint: &str) -> Result<(), ProviderError> {
    if endpoint.is_empty() {
        return Err(ProviderError::Endpoint {
            message: "endpoint is empty".into(),
        });
    }
    if !endpoint.is_ascii()
        || endpoint.len() > 2048
        || endpoint
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(ProviderError::Endpoint {
            message:
                "endpoint must be 1..=2048 ascii bytes without whitespace or control characters"
                    .into(),
        });
    }
    Ok(())
}

fn check_capabilities(capabilities: &[Capability]) -> Result<(), ProviderError> {
    let mut seen = HashSet::with_capacity(capabilities.len());
    for capability in capabilities {
        if !seen.insert(*capability) {
            return Err(ProviderError::Capability {
                message: format!("duplicate capability {}", capability.as_str()),
            });
        }
    }
    Ok(())
}

fn check_vram(vram_gb: Option<u32>) -> Result<(), ProviderError> {
    match vram_gb {
        Some(gb) if gb == 0 || gb > 4096 => Err(ProviderError::Vram {
            message: format!("vram_gb {gb} must be 1..=4096"),
        }),
        _ => Ok(()),
    }
}

fn io_at(path: &Path, action: &str, err: io::Error) -> ProviderError {
    ProviderError::Io {
        path: path.to_path_buf(),
        message: format!("cannot {action} registry: {err}"),
    }
}

fn write_atomic(path: &Path, document: &Document) -> Result<(), ProviderError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| io_at(parent, "create", err))?;
        }
    }
    let mut bytes = serde_json::to_vec_pretty(document)
        .expect("registry document is built from known types and always serializes");
    bytes.push(b'\n');
    let tmp = temp_path(path);
    if let Err(err) = fs::write(&tmp, &bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(io_at(&tmp, "write", err));
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(io_at(path, "rename", err));
    }
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("registry.json"))
        .to_os_string();
    name.push(".tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}
