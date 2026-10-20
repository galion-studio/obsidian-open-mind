// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Hardware probe for OpenMind boot.
//!
//! Reads CPU and RAM from `proc/cpuinfo` and `proc/meminfo`, and GPU presence
//! from `proc/driver/nvidia` plus `sys/class/drm` and `sys/bus/pci`. No other
//! paths are read. This crate does not call `nvidia-smi`, does not install a
//! unit, and does not create a cgroup.
//!
//! A missing GPU is [`GpuStatus::Absent`], not an error. A server BMC VGA
//! (ASPEED and the like) is not a GPU. Presence means an NVIDIA, AMD, or Intel
//! display device, any PCI 3D-class controller, or a loaded NVIDIA driver.
//!
//! [`write_hardware_json`] writes a file named `hardware.json` only under a
//! temp directory. `/etc` and live state paths are refused.
//!
//! [`to_nerve`] turns one probe into a single nerve JSON object (`ts`,
//! `source`, `kind`, `severity`, `data`) and does not write a file.
//! `source` is `boot`, `kind` is `hardware`, and `severity` is `info`.
//! A missing GPU stays `absent`.

#![forbid(unsafe_code)]

use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Worker size used by the old hardware report. Cap is free RAM divided by this.
pub const WORKER_MIB: u64 = 512;

/// Upper bound on `cap`, matching the old hardware report.
pub const CAP_MAX: u64 = 100;

/// Local weights need at least this much free GPU memory.
pub const WEIGHTS_MIN_GPU_FREE_MIB: u64 = 8192;

/// Local weights also need at least this much RAM.
pub const WEIGHTS_MIN_RAM_MIB: u64 = 8192;

/// Human line when no GPU was found.
pub const CPU_ONLY_LINE: &str = "CPU only. Hermes stays on the laptop model. Weights stay off.";

/// Human line when a GPU is present and free memory is large enough.
pub const WEIGHTS_LINE: &str = "Obsidian has room for local weights.";

/// Human line when a GPU is present but free memory is missing or too small.
pub const CARD_SHORT_LINE: &str =
    "A card is here, but not enough free memory. Hermes stays on the laptop model.";

const LIVE_PREFIXES: &[&str] = &[
    "/opt/galion/galion-light",
    "/opt/galion/openmind",
    "/opt/galion/bin",
    "/workspace",
];

/// Failure while reading CPU or RAM, or while refusing a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeError {
    message: String,
}

impl ProbeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(path: &Path, err: io::Error) -> Self {
        Self::new(format!("cannot read {}: {err}", path.display()))
    }

    /// The error text. No secrets.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProbeError {}

/// Logical processors and the first model name from `/proc/cpuinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    /// Number of `processor` lines.
    pub processors: u32,
    /// First `model name`, or empty when the file has none.
    pub model: String,
}

/// RAM totals in whole mebibytes (`kB / 1024`, integer division).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RamInfo {
    /// `MemTotal`.
    pub total_mib: u64,
    /// `MemAvailable`, else `MemFree`, else `0`.
    pub free_mib: u64,
}

/// Whether a compute GPU was visible. Absence is a status, not a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuStatus {
    /// No NVIDIA, AMD, or Intel display device and no 3D controller.
    Absent,
    /// At least one such device was readable.
    Present,
}

/// GPU section of `hardware.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gpu {
    /// `absent` or `present`.
    pub status: GpuStatus,
    /// How many distinct GPUs were merged. Zero when absent.
    pub count: u32,
    /// Marketing name when `/proc` had one, otherwise a short vendor label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `nvidia`, `amd`, `intel`, or `pci:<id>` for a 3D-class device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// `proc` or `sys`, whichever supplied the chosen name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// VRAM total from `mem_info_vram_total` (bytes on disk, MiB here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_mib: Option<u64>,
    /// VRAM used from `mem_info_vram_used`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_mib: Option<u64>,
    /// `total_mib - used_mib` when both files were readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_mib: Option<u64>,
}

impl Gpu {
    /// No GPU. This is a successful report.
    pub fn absent() -> Self {
        Self {
            status: GpuStatus::Absent,
            count: 0,
            name: None,
            vendor: None,
            source: None,
            total_mib: None,
            used_mib: None,
            free_mib: None,
        }
    }

    /// True when the probe found nothing it counts as a GPU.
    pub fn is_absent(&self) -> bool {
        self.status == GpuStatus::Absent
    }

    /// True when at least one GPU was recorded.
    pub fn is_present(&self) -> bool {
        self.status == GpuStatus::Present && self.count > 0
    }
}

/// One hardware report. Safe to serialize. Does not write itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hardware {
    /// Europe/Zagreb RFC 3339, explicit offset, never `Z`.
    pub probed_at: String,
    /// Logical CPU count.
    pub processors: u32,
    /// First CPU model name. May be empty.
    pub cpu_model: String,
    /// `MemTotal` in MiB.
    pub ram_total_mib: u64,
    /// Available RAM in MiB.
    pub ram_free_mib: u64,
    /// GPU presence. Absent is not an error.
    pub gpu: Gpu,
    /// Always [`WORKER_MIB`].
    pub worker_mib: u64,
    /// `min(CAP_MAX, ram_free_mib / WORKER_MIB)`.
    pub cap: u64,
    /// True only when RAM and free VRAM both clear the weights floor.
    pub weights: bool,
    /// One English sentence: CPU only, room for weights, or card short on memory.
    pub line: String,
}

/// Parse `/proc/cpuinfo` text.
///
/// # Errors
///
/// The text has no `processor` line.
pub fn parse_cpuinfo(text: &str) -> Result<CpuInfo, ProbeError> {
    let mut processors = 0u32;
    let mut model = String::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.eq_ignore_ascii_case("processor") {
            processors = processors.saturating_add(1);
        } else if key.eq_ignore_ascii_case("model name") && model.is_empty() {
            model = value.trim().to_string();
        }
    }
    if processors == 0 {
        return Err(ProbeError::new("cpuinfo has no processor lines"));
    }
    Ok(CpuInfo { processors, model })
}

/// Parse `/proc/meminfo` text into whole mebibytes.
///
/// # Errors
///
/// `MemTotal` is missing or not an integer.
pub fn parse_meminfo(text: &str) -> Result<RamInfo, ProbeError> {
    let total_kib =
        kib(text, "MemTotal:").ok_or_else(|| ProbeError::new("meminfo has no MemTotal"))?;
    let free_kib = kib(text, "MemAvailable:").or_else(|| kib(text, "MemFree:"));
    Ok(RamInfo {
        total_mib: total_kib / 1024,
        free_mib: free_kib.unwrap_or(0) / 1024,
    })
}

/// Build a report from already-parsed parts.
///
/// `probed_at` must be Europe/Zagreb RFC 3339 (`+01:00` or `+02:00`), never `Z`.
///
/// # Errors
///
/// The timestamp is empty, ends in `Z`, or is not a Zagreb offset.
pub fn assemble(
    cpu: CpuInfo,
    ram: RamInfo,
    gpu: Gpu,
    probed_at: &str,
) -> Result<Hardware, ProbeError> {
    check_timestamp(probed_at)?;
    let (cap, weights, line) = decide(&ram, &gpu);
    Ok(Hardware {
        probed_at: probed_at.to_string(),
        processors: cpu.processors,
        cpu_model: cpu.model,
        ram_total_mib: ram.total_mib,
        ram_free_mib: ram.free_mib,
        gpu,
        worker_mib: WORKER_MIB,
        cap,
        weights,
        line,
    })
}

/// Read `root/proc` and `root/sys` and stamp the report with Zagreb time.
///
/// Pass `Path::new("/")` for the live machine. Tests pass a fixture root.
/// GPU files that are missing or unreadable count as absent, not as an error.
///
/// # Errors
///
/// CPU or RAM text is missing or unparseable.
pub fn probe(root: &Path) -> Result<Hardware, ProbeError> {
    let cpu_text = read_required(&root.join("proc/cpuinfo"))?;
    let mem_text = read_required(&root.join("proc/meminfo"))?;
    let cpu = parse_cpuinfo(&cpu_text)?;
    let ram = parse_meminfo(&mem_text)?;
    let gpu = scan_gpu(root);
    assemble(cpu, ram, gpu, &zagreb_time::now_rfc3339())
}

/// Probe the live machine (`/proc` and `/sys` only).
///
/// # Errors
///
/// Same as [`probe`].
pub fn probe_host() -> Result<Hardware, ProbeError> {
    probe(Path::new("/"))
}

/// GPU presence under `root`, reading only sysfs and the NVIDIA proc driver.
///
/// Never returns an error. A missing tree, an empty tree, or a BMC VGA is
/// [`Gpu::absent`].
pub fn scan_gpu(root: &Path) -> Gpu {
    let mut found = Vec::new();
    found.extend(scan_nvidia(root));
    found.extend(scan_pci(root));
    found.extend(scan_drm(root));
    let merged = merge_found(found);
    gpu_from(merged)
}

/// Pretty `hardware.json` body, trailing newline, two-space indent.
///
/// # Errors
///
/// Serialization failed. That does not happen for a normal [`Hardware`] value.
pub fn to_json(hw: &Hardware) -> Result<String, ProbeError> {
    let mut body =
        serde_json::to_string_pretty(hw).map_err(|err| ProbeError::new(format!("json: {err}")))?;
    body.push('\n');
    Ok(body)
}

/// Nerve `source` for a boot hardware probe.
const NERVE_SOURCE: &str = "boot";

/// Nerve `kind` for a boot hardware probe.
const NERVE_KIND: &str = "hardware";

/// A hardware probe is informational. A missing GPU is not an error.
const NERVE_SEVERITY: &str = "info";

#[derive(Serialize)]
struct NerveBody<'a> {
    ts: &'a str,
    source: &'a str,
    kind: &'a str,
    severity: &'a str,
    data: NerveData<'a>,
}

#[derive(Serialize)]
struct NerveData<'a> {
    cpu: NerveCpu<'a>,
    ram: NerveRam,
    gpu: &'a Gpu,
}

#[derive(Serialize)]
struct NerveCpu<'a> {
    processors: u32,
    model: &'a str,
}

#[derive(Serialize)]
struct NerveRam {
    total_mib: u64,
    free_mib: u64,
}

/// One nerve JSON object for a hardware probe.
///
/// The object has `ts`, `source`, `kind`, `severity`, and `data`.
/// `source` is `boot`. `kind` is `hardware`. `severity` is `info`.
/// `ts` is the probe stamp: Europe/Zagreb RFC 3339 from `zagreb-time`
/// (explicit `+01:00` or `+02:00`, never `Z`). `data` holds cpu, ram,
/// and gpu status. Absence of a GPU is `absent`, not an error.
///
/// This function does not write a file and does not create `hardware.json`.
///
/// # Errors
///
/// The probe timestamp is empty, ends in `Z`, or is not a Zagreb offset.
pub fn to_nerve(hw: &Hardware) -> Result<String, ProbeError> {
    check_timestamp(&hw.probed_at)?;
    let event = NerveBody {
        ts: &hw.probed_at,
        source: NERVE_SOURCE,
        kind: NERVE_KIND,
        severity: NERVE_SEVERITY,
        data: NerveData {
            cpu: NerveCpu {
                processors: hw.processors,
                model: &hw.cpu_model,
            },
            ram: NerveRam {
                total_mib: hw.ram_total_mib,
                free_mib: hw.ram_free_mib,
            },
            gpu: &hw.gpu,
        },
    };
    let mut body = serde_json::to_string_pretty(&event)
        .map_err(|err| ProbeError::new(format!("json: {err}")))?;
    body.push('\n');
    Ok(body)
}

/// Write `hardware.json` under a temp directory.
///
/// The file name must be `hardware.json`. The path must be absolute, must not
/// contain `..`, must not be a symlink (nor sit under one), and must not be
/// `/etc` or a live state path. Nothing is created when the check fails.
///
/// # Errors
///
/// The path is refused, or the temp directory cannot be created or written.
pub fn write_hardware_json(hw: &Hardware, dest: &Path) -> Result<(), ProbeError> {
    let dest = normalize_abs(dest)?;
    if dest.file_name().and_then(|n| n.to_str()) != Some("hardware.json") {
        return Err(ProbeError::new(format!(
            "refusing to write {}: file must be named hardware.json",
            dest.display()
        )));
    }
    if dest.starts_with("/etc") {
        return Err(ProbeError::new(format!(
            "refusing to write {}: /etc is not a temp dir",
            dest.display()
        )));
    }
    for prefix in LIVE_PREFIXES {
        if dest.starts_with(prefix) {
            return Err(ProbeError::new(format!(
                "refusing to write {}: live state path",
                dest.display()
            )));
        }
    }
    if !is_under_temp(&dest) {
        return Err(ProbeError::new(format!(
            "refusing to write {}: outside a temp directory",
            dest.display()
        )));
    }
    refuse_symlink_below_temp(&dest)?;
    if let Ok(meta) = fs::symlink_metadata(&dest) {
        if !meta.file_type().is_file() {
            return Err(ProbeError::new(format!(
                "refusing to write {}: destination is not a regular file",
                dest.display()
            )));
        }
    }
    let body = to_json(hw)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| ProbeError::io(parent, err))?;
    }
    fs::write(&dest, body).map_err(|err| ProbeError::io(&dest, err))
}

fn kib(text: &str, key: &str) -> Option<u64> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some(key) {
            return parts.next()?.parse().ok();
        }
    }
    None
}

fn check_timestamp(ts: &str) -> Result<(), ProbeError> {
    let zagreb = ts.ends_with("+01:00") || ts.ends_with("+02:00");
    if ts.len() < 25 || ts.contains('Z') || ts.contains("+00:00") || !zagreb {
        return Err(ProbeError::new(
            "timestamp must be Europe/Zagreb RFC 3339 with a +01:00 or +02:00 offset, never Z",
        ));
    }
    Ok(())
}

fn decide(ram: &RamInfo, gpu: &Gpu) -> (u64, bool, String) {
    let cap = (ram.free_mib / WORKER_MIB).min(CAP_MAX);
    let free_gpu = gpu.free_mib.unwrap_or(0);
    let weights = gpu.is_present()
        && free_gpu >= WEIGHTS_MIN_GPU_FREE_MIB
        && ram.total_mib >= WEIGHTS_MIN_RAM_MIB;
    let line = if weights {
        WEIGHTS_LINE
    } else if gpu.is_present() {
        CARD_SHORT_LINE
    } else {
        CPU_ONLY_LINE
    };
    (cap, weights, line.to_string())
}

fn read_required(path: &Path) -> Result<String, ProbeError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Err(ProbeError::new(format!("missing {}", path.display())))
        }
        Err(err) => Err(ProbeError::io(path, err)),
    }
}

fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .filter(|text| !text.trim().is_empty())
}

fn list_dir(path: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(path) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd
        .filter_map(|ent| ent.ok().map(|ent| ent.path()))
        .collect();
    paths.sort();
    paths
}

#[derive(Debug, Clone)]
struct Found {
    slot: Option<String>,
    vendor_id: Option<String>,
    device_id: Option<String>,
    vendor: Option<String>,
    name: Option<String>,
    total_mib: Option<u64>,
    used_mib: Option<u64>,
    source: &'static str,
}

fn scan_nvidia(root: &Path) -> Vec<Found> {
    let base = root.join("proc/driver/nvidia");
    let mut out = Vec::new();
    for dir in list_dir(&base.join("gpus")) {
        if !dir.is_dir() {
            continue;
        }
        let info = read_text(&dir.join("information")).unwrap_or_default();
        let name = field(&info, "Model");
        let slot = field(&info, "Bus Location")
            .and_then(|raw| pci_slot(&raw))
            .or_else(|| dir.file_name().and_then(|n| pci_slot(&n.to_string_lossy())));
        if name.is_none() && slot.is_none() && info.trim().is_empty() {
            continue;
        }
        out.push(Found {
            slot,
            vendor_id: Some("10de".to_string()),
            device_id: None,
            vendor: Some("nvidia".to_string()),
            name,
            total_mib: None,
            used_mib: None,
            source: "proc",
        });
    }
    if out.is_empty() {
        if let Some(version) = read_text(&base.join("version")) {
            if !version.trim().is_empty() {
                out.push(Found {
                    slot: None,
                    vendor_id: Some("10de".to_string()),
                    device_id: None,
                    vendor: Some("nvidia".to_string()),
                    name: None,
                    total_mib: None,
                    used_mib: None,
                    source: "proc",
                });
            }
        }
    }
    out
}

fn scan_pci(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    for dir in list_dir(&root.join("sys/bus/pci/devices")) {
        if !dir.is_dir() {
            continue;
        }
        if let Some(found) = found_from_sys(&dir, false) {
            out.push(found);
        }
    }
    out
}

fn scan_drm(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    for dir in list_dir(&root.join("sys/class/drm")) {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_drm_card(name) || !dir.is_dir() {
            continue;
        }
        if let Some(found) = found_from_sys(&dir.join("device"), true) {
            out.push(found);
        }
    }
    out
}

fn found_from_sys(dir: &Path, from_drm: bool) -> Option<Found> {
    let mut vendor_id = read_text(&dir.join("vendor")).and_then(|raw| hex_id(&raw));
    let mut device_id = read_text(&dir.join("device")).and_then(|raw| hex_id(&raw));
    let mut class_text = read_text(&dir.join("class"));
    let mut slot = dir.file_name().and_then(|n| pci_slot(&n.to_string_lossy()));
    if let Some(uevent) = read_text(&dir.join("uevent")) {
        apply_uevent(
            &uevent,
            &mut vendor_id,
            &mut device_id,
            &mut slot,
            &mut class_text,
        );
    }
    if !counts_as_gpu(vendor_id.as_deref(), class_text.as_deref(), from_drm) {
        return None;
    }
    let (vendor, name) = labels(vendor_id.as_deref(), device_id.as_deref());
    let total_mib =
        read_text(&dir.join("mem_info_vram_total")).and_then(|raw| mib_from_bytes(&raw));
    let used_mib = read_text(&dir.join("mem_info_vram_used")).and_then(|raw| mib_from_bytes(&raw));
    Some(Found {
        slot,
        vendor_id,
        device_id,
        vendor,
        name,
        total_mib,
        used_mib,
        source: "sys",
    })
}

fn apply_uevent(
    text: &str,
    vendor_id: &mut Option<String>,
    device_id: &mut Option<String>,
    slot: &mut Option<String>,
    class_text: &mut Option<String>,
) {
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "PCI_ID" => {
                if let Some((ven, dev)) = value.split_once(':') {
                    if vendor_id.is_none() {
                        *vendor_id = hex_id(ven);
                    }
                    if device_id.is_none() {
                        *device_id = hex_id(dev);
                    }
                }
            }
            "PCI_SLOT_NAME" if slot.is_none() => {
                *slot = pci_slot(value);
            }
            "PCI_CLASS" if class_text.is_none() => {
                *class_text = Some(value.to_string());
            }
            _ => {}
        }
    }
}

fn counts_as_gpu(vendor_id: Option<&str>, class_text: Option<&str>, from_drm: bool) -> bool {
    if class_text.is_some_and(is_3d_class) {
        return true;
    }
    let Some(vendor_id) = vendor_id else {
        return false;
    };
    if !known_gpu_vendor(vendor_id) {
        return false;
    }
    if class_text.is_some_and(is_display_class) {
        return true;
    }
    from_drm && class_text.is_none()
}

fn known_gpu_vendor(id: &str) -> bool {
    matches!(id, "10de" | "1002" | "8086")
}

fn is_3d_class(text: &str) -> bool {
    class_prefix(text).is_some_and(|prefix| prefix.starts_with("0302"))
}

fn is_display_class(text: &str) -> bool {
    class_prefix(text)
        .is_some_and(|prefix| prefix.starts_with("0300") || prefix.starts_with("0380"))
}

fn class_prefix(text: &str) -> Option<String> {
    let hex = hex_id(text)?;
    Some(format!("{hex:0>6}"))
}

fn hex_id(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let bare = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if bare.is_empty() || !bare.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(bare.to_ascii_lowercase())
}

fn pci_slot(text: &str) -> Option<String> {
    let text = text.trim();
    if text.matches(':').count() >= 2 && text.contains('.') {
        Some(text.to_string())
    } else {
        None
    }
}

fn is_drm_card(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("card") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

fn labels(vendor_id: Option<&str>, device_id: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(vendor_id) = vendor_id else {
        return (None, None);
    };
    let vendor = match vendor_id {
        "10de" => "nvidia".to_string(),
        "1002" => "amd".to_string(),
        "8086" => "intel".to_string(),
        other => format!("pci:{other}"),
    };
    let name = device_id.map(|device| format!("{vendor} {device}"));
    (Some(vendor), name)
}

fn mib_from_bytes(text: &str) -> Option<u64> {
    let bytes: u64 = text.trim().parse().ok()?;
    Some(bytes / 1_048_576)
}

fn field(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let Some((raw_key, raw_value)) = line.split_once(':') else {
            continue;
        };
        if raw_key.trim().eq_ignore_ascii_case(key) {
            let value = raw_value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn same_device(left: &Found, right: &Found) -> bool {
    if let (Some(a), Some(b)) = (&left.slot, &right.slot) {
        return a == b;
    }
    if let (Some(av), Some(ad), Some(bv), Some(bd)) = (
        &left.vendor_id,
        &left.device_id,
        &right.vendor_id,
        &right.device_id,
    ) {
        if av == bv && ad == bd {
            return true;
        }
    }
    let left_bare =
        left.slot.is_none() && left.device_id.is_none() && left.vendor.as_deref() == Some("nvidia");
    let right_bare = right.slot.is_none()
        && right.device_id.is_none()
        && right.vendor.as_deref() == Some("nvidia");
    (left_bare && right.vendor.as_deref() == Some("nvidia"))
        || (right_bare && left.vendor.as_deref() == Some("nvidia"))
}

fn absorb(dst: &mut Found, src: Found) {
    if dst.slot.is_none() {
        dst.slot = src.slot;
    }
    if dst.vendor_id.is_none() {
        dst.vendor_id = src.vendor_id;
    }
    if dst.device_id.is_none() {
        dst.device_id = src.device_id;
    }
    if dst.vendor.is_none() {
        dst.vendor = src.vendor;
    }
    let src_is_model = src.source == "proc" && src.name.is_some();
    let dst_is_model = dst.source == "proc" && dst.name.is_some();
    if src.name.is_some() && (dst.name.is_none() || (src_is_model && !dst_is_model)) {
        dst.name = src.name;
    }
    if dst.total_mib.is_none() {
        dst.total_mib = src.total_mib;
    }
    if dst.used_mib.is_none() {
        dst.used_mib = src.used_mib;
    }
    if src_is_model {
        dst.source = "proc";
    }
}

fn merge_found(items: Vec<Found>) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for item in items {
        if let Some(existing) = out.iter_mut().find(|have| same_device(have, &item)) {
            absorb(existing, item);
        } else {
            out.push(item);
        }
    }
    out
}

fn gpu_from(items: Vec<Found>) -> Gpu {
    let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
    let Some(best) = primary(&items) else {
        return Gpu::absent();
    };
    Gpu::from_found(count, best)
}

impl Gpu {
    fn from_found(count: u32, found: &Found) -> Self {
        let free_mib = match (found.total_mib, found.used_mib) {
            (Some(total), Some(used)) => Some(total.saturating_sub(used)),
            _ => None,
        };
        Self {
            status: GpuStatus::Present,
            count,
            name: found.name.clone(),
            vendor: found.vendor.clone(),
            source: Some(found.source.to_string()),
            total_mib: found.total_mib,
            used_mib: found.used_mib,
            free_mib,
        }
    }
}

fn primary(items: &[Found]) -> Option<&Found> {
    let mut best: Option<&Found> = None;
    for item in items {
        best = Some(match best {
            None => item,
            Some(current) => {
                let item_mem = item.total_mib.unwrap_or(0);
                let current_mem = current.total_mib.unwrap_or(0);
                let richer = item_mem > current_mem;
                let named =
                    item_mem == current_mem && current.name.is_none() && item.name.is_some();
                if richer || named {
                    item
                } else {
                    current
                }
            }
        });
    }
    best
}

fn normalize_abs(path: &Path) -> Result<PathBuf, ProbeError> {
    if !path.is_absolute() {
        return Err(ProbeError::new(format!(
            "refusing to write {}: path must be absolute",
            path.display()
        )));
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ProbeError::new(format!(
                    "refusing to write {}: path must not contain ..",
                    path.display()
                )));
            }
            Component::Normal(part) => out.push(part),
            Component::Prefix(_) => {
                return Err(ProbeError::new(format!(
                    "refusing to write {}: path must be absolute",
                    path.display()
                )));
            }
        }
    }
    Ok(out)
}

fn is_under_temp(path: &Path) -> bool {
    path.starts_with("/tmp")
        || path.starts_with("/var/tmp")
        || path.starts_with(std::env::temp_dir())
}

fn refuse_symlink_below_temp(path: &Path) -> Result<(), ProbeError> {
    let mut cur = path.to_path_buf();
    loop {
        if cur == Path::new("/tmp") || cur == Path::new("/var/tmp") || cur == std::env::temp_dir() {
            break;
        }
        if let Ok(meta) = fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return Err(ProbeError::new(format!(
                    "refusing to write {}: symlink",
                    path.display()
                )));
            }
        }
        if !is_under_temp(&cur) || !cur.pop() {
            break;
        }
    }
    Ok(())
}
