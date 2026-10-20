// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Fixture /proc and /sys text. Nothing here reads the host GPU.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use openmind_boot::{
    assemble, parse_cpuinfo, parse_meminfo, probe, scan_gpu, to_json, to_nerve,
    write_hardware_json, CpuInfo, Gpu, GpuStatus, Hardware, RamInfo, CAP_MAX, CARD_SHORT_LINE,
    CPU_ONLY_LINE, WEIGHTS_LINE, WEIGHTS_MIN_GPU_FREE_MIB, WEIGHTS_MIN_RAM_MIB, WORKER_MIB,
};

const CPUINFO: &str = "\
processor : 0
vendor_id : GenuineIntel
model name : Test CPU
processor : 1
model name : Test CPU
processor : 2
model name : Test CPU
processor : 3
model name : Test CPU
";

const MEMINFO: &str = "\
MemTotal:        8388608 kB
MemFree:         1048576 kB
MemAvailable:    4194304 kB
Buffers:           65536 kB
";

const MEM_WEIGHTS: &str = "\
MemTotal:       16777216 kB
MemFree:         1048576 kB
MemAvailable:    8388608 kB
";

const TS: &str = "2026-10-19T16:45:00+02:00";

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("openmind-boot-b54-{}-{n}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn file(&self, rel: &str, body: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn cpu_ram(&self) {
        self.file("proc/cpuinfo", CPUINFO);
        self.file("proc/meminfo", MEMINFO);
    }

    fn cpu_ram_weights(&self) {
        self.file("proc/cpuinfo", CPUINFO);
        self.file("proc/meminfo", MEM_WEIGHTS);
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn absent_hw() -> Hardware {
    let cpu = parse_cpuinfo(CPUINFO).unwrap();
    let ram = parse_meminfo(MEMINFO).unwrap();
    assemble(cpu, ram, Gpu::absent(), TS).unwrap()
}

fn untouched(path: &Path) -> impl Fn() {
    let before = path.exists().then(|| fs::read(path).expect("read before"));
    let path = path.to_path_buf();
    move || {
        let after = path.exists().then(|| fs::read(&path).expect("read after"));
        assert_eq!(before, after, "live file changed: {}", path.display());
    }
}

/// Names in `path`, sorted. Used to prove `to_nerve` creates nothing.
fn dir_names(path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path).expect("read dir") {
        let entry = entry.expect("dir entry");
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

/// Keys of the outermost pretty-printed JSON object, in source order.
///
/// `serde_json` pretty output puts each top-level key on its own line
/// indented by two spaces. Nested keys are indented further.
fn top_level_object_keys(json: &str) -> Vec<&str> {
    let mut keys = Vec::new();
    for line in json.lines() {
        let Some(rest) = line.strip_prefix("  \"") else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        if rest[end + 1..].trim_start().starts_with(':') {
            keys.push(&rest[..end]);
        }
    }
    keys
}

/// Restores the process working directory when dropped.
struct CwdGuard {
    previous: PathBuf,
}

impl CwdGuard {
    fn enter(dir: &Path) -> Self {
        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir).expect("enter probe dir");
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

#[test]
fn parse_cpuinfo_counts_processors_and_model() {
    let cpu = parse_cpuinfo(CPUINFO).unwrap();
    assert_eq!(
        cpu,
        CpuInfo {
            processors: 4,
            model: "Test CPU".to_string(),
        }
    );
}

#[test]
fn parse_cpuinfo_without_processor_is_error() {
    let err = parse_cpuinfo("model name : nothing\n").unwrap_err();
    assert!(err.to_string().contains("processor"));
}

#[test]
fn parse_meminfo_prefers_available_over_free() {
    let ram = parse_meminfo(MEMINFO).unwrap();
    assert_eq!(
        ram,
        RamInfo {
            total_mib: 8192,
            free_mib: 4096,
        }
    );
}

#[test]
fn parse_meminfo_falls_back_to_memfree() {
    let text = "MemTotal: 8388608 kB\nMemFree: 2097152 kB\n";
    let ram = parse_meminfo(text).unwrap();
    assert_eq!(ram.free_mib, 2048);
    assert_eq!(ram.total_mib, 8192);
}

#[test]
fn parse_meminfo_missing_available_reports_zero_free() {
    let text = "MemTotal: 8388608 kB\n";
    let ram = parse_meminfo(text).unwrap();
    assert_eq!(ram.total_mib, 8192);
    assert_eq!(ram.free_mib, 0);
}

#[test]
fn parse_meminfo_missing_total_is_error() {
    let err = parse_meminfo("MemFree: 1024 kB\n").unwrap_err();
    assert!(err.to_string().contains("MemTotal"));
}

#[test]
fn missing_gpu_tree_is_absent() {
    let tree = Tree::new();
    tree.cpu_ram();
    let hw = probe(&tree.root).unwrap();
    assert!(hw.gpu.is_absent());
    assert_eq!(hw.gpu.status, GpuStatus::Absent);
    assert_eq!(hw.gpu.count, 0);
    assert!(!hw.weights);
    assert_eq!(hw.line, CPU_ONLY_LINE);
    assert_eq!(hw.processors, 4);
    assert_eq!(hw.ram_total_mib, 8192);
    assert_eq!(hw.ram_free_mib, 4096);
    assert_eq!(hw.worker_mib, WORKER_MIB);
    assert_eq!(hw.cap, 8);
    let json = to_json(&hw).unwrap();
    assert!(json.contains("\"status\": \"absent\""));
    assert!(!json.contains('Z'));
    assert!(!json.contains("+00:00"));
    assert!(json.contains("+0"));
}

#[test]
fn empty_drm_and_aspeed_vga_are_absent() {
    let tree = Tree::new();
    tree.cpu_ram();
    tree.file("sys/class/drm/version", "drm 1.1.0\n");
    tree.file("sys/class/drm/card0/device/vendor", "0x1a03\n");
    tree.file("sys/class/drm/card0/device/device", "0x2000\n");
    tree.file("sys/class/drm/card0/device/class", "0x030000\n");
    tree.file(
        "sys/class/drm/card0/device/uevent",
        "PCI_ID=1A03:2000\nPCI_CLASS=030000\nPCI_SLOT_NAME=0000:22:00.0\n",
    );
    tree.file("sys/bus/pci/devices/0000:22:00.0/vendor", "0x1a03\n");
    tree.file("sys/bus/pci/devices/0000:22:00.0/device", "0x2000\n");
    tree.file("sys/bus/pci/devices/0000:22:00.0/class", "0x030000\n");
    let gpu = scan_gpu(&tree.root);
    assert!(gpu.is_absent(), "BMC VGA must not count as a GPU: {gpu:?}");
    let hw = probe(&tree.root).unwrap();
    assert_eq!(hw.line, CPU_ONLY_LINE);
    assert!(hw.gpu.is_absent());
}

#[test]
fn intel_nic_is_not_a_gpu() {
    let tree = Tree::new();
    tree.file("sys/bus/pci/devices/0000:00:1f.6/vendor", "0x8086\n");
    tree.file("sys/bus/pci/devices/0000:00:1f.6/device", "0x15b8\n");
    tree.file("sys/bus/pci/devices/0000:00:1f.6/class", "0x020000\n");
    assert!(scan_gpu(&tree.root).is_absent());
}

#[test]
fn nvidia_proc_text_is_present_without_vram() {
    let tree = Tree::new();
    tree.cpu_ram();
    tree.file(
        "proc/driver/nvidia/gpus/0000:01:00.0/information",
        "Model:          NVIDIA GeForce RTX 3090\nBus Location:   0000:01:00.0\n",
    );
    let hw = probe(&tree.root).unwrap();
    assert!(hw.gpu.is_present());
    assert_eq!(hw.gpu.count, 1);
    assert_eq!(hw.gpu.vendor.as_deref(), Some("nvidia"));
    assert_eq!(hw.gpu.name.as_deref(), Some("NVIDIA GeForce RTX 3090"));
    assert_eq!(hw.gpu.source.as_deref(), Some("proc"));
    assert_eq!(hw.gpu.free_mib, None);
    assert!(!hw.weights);
    assert_eq!(hw.line, CARD_SHORT_LINE);
    let json = to_json(&hw).unwrap();
    assert!(json.contains("\"status\": \"present\""));
    assert!(!json.contains("\"status\": \"absent\""));
}

#[test]
fn nvidia_version_file_alone_is_present() {
    let tree = Tree::new();
    tree.file(
        "proc/driver/nvidia/version",
        "NVRM version: NVIDIA UNIX Kernel Module 550.90.07\n",
    );
    let gpu = scan_gpu(&tree.root);
    assert!(gpu.is_present());
    assert_eq!(gpu.vendor.as_deref(), Some("nvidia"));
    assert_eq!(gpu.count, 1);
    assert_eq!(gpu.free_mib, None);
}

#[test]
fn nvidia_proc_and_sysfs_same_slot_count_once() {
    let tree = Tree::new();
    tree.file(
        "proc/driver/nvidia/gpus/0000:01:00.0/information",
        "Model: NVIDIA GeForce RTX 3090\nBus Location: 0000:01:00.0\n",
    );
    tree.file("sys/bus/pci/devices/0000:01:00.0/vendor", "0x10de\n");
    tree.file("sys/bus/pci/devices/0000:01:00.0/device", "0x2204\n");
    tree.file("sys/bus/pci/devices/0000:01:00.0/class", "0x030000\n");
    tree.file("sys/class/drm/card0/device/vendor", "0x10de\n");
    tree.file("sys/class/drm/card0/device/device", "0x2204\n");
    tree.file("sys/class/drm/card0/device/class", "0x030000\n");
    tree.file(
        "sys/class/drm/card0/device/uevent",
        "PCI_ID=10DE:2204\nPCI_CLASS=030000\nPCI_SLOT_NAME=0000:01:00.0\n",
    );
    tree.file(
        "sys/class/drm/card0/device/mem_info_vram_total",
        "25769803776\n",
    );
    tree.file(
        "sys/class/drm/card0/device/mem_info_vram_used",
        "1073741824\n",
    );
    let gpu = scan_gpu(&tree.root);
    assert_eq!(gpu.count, 1, "{gpu:?}");
    assert_eq!(gpu.name.as_deref(), Some("NVIDIA GeForce RTX 3090"));
    assert_eq!(gpu.vendor.as_deref(), Some("nvidia"));
    assert_eq!(gpu.source.as_deref(), Some("proc"));
    assert_eq!(gpu.total_mib, Some(24576));
    assert_eq!(gpu.used_mib, Some(1024));
    assert_eq!(gpu.free_mib, Some(23552));
}

#[test]
fn two_nvidia_slots_stay_two_and_primary_has_the_vram() {
    let tree = Tree::new();
    tree.file(
        "proc/driver/nvidia/gpus/0000:01:00.0/information",
        "Model: GPU-A\nBus Location: 0000:01:00.0\n",
    );
    tree.file(
        "proc/driver/nvidia/gpus/0000:02:00.0/information",
        "Model: GPU-B\nBus Location: 0000:02:00.0\n",
    );
    tree.file("sys/class/drm/card1/device/vendor", "0x10de\n");
    tree.file("sys/class/drm/card1/device/class", "0x030000\n");
    tree.file(
        "sys/class/drm/card1/device/uevent",
        "PCI_SLOT_NAME=0000:02:00.0\nPCI_ID=10DE:2204\n",
    );
    tree.file(
        "sys/class/drm/card1/device/mem_info_vram_total",
        "17179869184\n",
    );
    tree.file("sys/class/drm/card1/device/mem_info_vram_used", "0\n");
    let gpu = scan_gpu(&tree.root);
    assert_eq!(gpu.count, 2, "{gpu:?}");
    assert_eq!(gpu.name.as_deref(), Some("GPU-B"));
    assert_eq!(gpu.free_mib, Some(16384));
}

#[test]
fn amd_sysfs_vram_enables_weights() {
    let tree = Tree::new();
    tree.cpu_ram_weights();
    tree.file("sys/bus/pci/devices/0000:03:00.0/vendor", "0x1002\n");
    tree.file("sys/bus/pci/devices/0000:03:00.0/device", "0x73bf\n");
    tree.file("sys/bus/pci/devices/0000:03:00.0/class", "0x030000\n");
    tree.file(
        "sys/bus/pci/devices/0000:03:00.0/mem_info_vram_total",
        "17179869184\n",
    );
    tree.file(
        "sys/bus/pci/devices/0000:03:00.0/mem_info_vram_used",
        "1073741824\n",
    );
    let hw = probe(&tree.root).unwrap();
    assert!(hw.gpu.is_present());
    assert_eq!(hw.gpu.vendor.as_deref(), Some("amd"));
    assert_eq!(hw.gpu.source.as_deref(), Some("sys"));
    assert_eq!(hw.gpu.free_mib, Some(15360));
    assert!(hw.weights);
    assert_eq!(hw.line, WEIGHTS_LINE);
    assert_eq!(hw.ram_total_mib, 16384);
    assert_eq!(hw.ram_free_mib, 8192);
    assert_eq!(hw.cap, 16);
}

#[test]
fn amd_small_vram_stays_present_but_weights_off() {
    let tree = Tree::new();
    tree.cpu_ram_weights();
    tree.file("sys/bus/pci/devices/0000:03:00.0/vendor", "0x1002\n");
    tree.file("sys/bus/pci/devices/0000:03:00.0/device", "0x73bf\n");
    tree.file("sys/bus/pci/devices/0000:03:00.0/class", "0x030000\n");
    tree.file(
        "sys/bus/pci/devices/0000:03:00.0/mem_info_vram_total",
        "4294967296\n",
    );
    tree.file("sys/bus/pci/devices/0000:03:00.0/mem_info_vram_used", "0\n");
    let hw = probe(&tree.root).unwrap();
    assert!(hw.gpu.is_present());
    assert_eq!(hw.gpu.free_mib, Some(4096));
    assert!(!hw.weights);
    assert_eq!(hw.line, CARD_SHORT_LINE);
}

#[test]
fn three_d_class_unknown_vendor_is_present() {
    let tree = Tree::new();
    tree.file("sys/bus/pci/devices/0000:04:00.0/vendor", "0x1ed5\n");
    tree.file("sys/bus/pci/devices/0000:04:00.0/device", "0x0100\n");
    tree.file("sys/bus/pci/devices/0000:04:00.0/class", "0x030200\n");
    let gpu = scan_gpu(&tree.root);
    assert!(gpu.is_present(), "{gpu:?}");
    assert_eq!(gpu.vendor.as_deref(), Some("pci:1ed5"));
    assert_eq!(gpu.count, 1);
}

#[test]
fn drm_known_vendor_without_class_is_present() {
    let tree = Tree::new();
    tree.file("sys/class/drm/card0/device/vendor", "0x8086\n");
    tree.file("sys/class/drm/card0/device/device", "0x9bc8\n");
    let gpu = scan_gpu(&tree.root);
    assert!(gpu.is_present(), "{gpu:?}");
    assert_eq!(gpu.vendor.as_deref(), Some("intel"));
}

#[test]
fn weights_floor_is_inclusive() {
    let cpu = CpuInfo {
        processors: 2,
        model: "Boundary".to_string(),
    };
    let ram = RamInfo {
        total_mib: WEIGHTS_MIN_RAM_MIB,
        free_mib: WORKER_MIB,
    };
    let gpu = Gpu {
        status: GpuStatus::Present,
        count: 1,
        name: Some("fixture".to_string()),
        vendor: Some("amd".to_string()),
        source: Some("sys".to_string()),
        total_mib: Some(WEIGHTS_MIN_GPU_FREE_MIB),
        used_mib: Some(0),
        free_mib: Some(WEIGHTS_MIN_GPU_FREE_MIB),
    };
    let hw = assemble(cpu, ram, gpu, "2026-12-01T13:00:00+01:00").unwrap();
    assert!(hw.weights);
    assert_eq!(hw.line, WEIGHTS_LINE);
    assert_eq!(hw.cap, 1.min(CAP_MAX));
    assert!(hw.probed_at.ends_with("+01:00"));
}

#[test]
fn json_golden_for_absent_cpu() {
    let hw = absent_hw();
    let json = to_json(&hw).unwrap();
    let expected = "\
{
  \"probed_at\": \"2026-10-19T16:45:00+02:00\",
  \"processors\": 4,
  \"cpu_model\": \"Test CPU\",
  \"ram_total_mib\": 8192,
  \"ram_free_mib\": 4096,
  \"gpu\": {
    \"status\": \"absent\",
    \"count\": 0
  },
  \"worker_mib\": 512,
  \"cap\": 8,
  \"weights\": false,
  \"line\": \"CPU only. Hermes stays on the laptop model. Weights stay off.\"
}
";
    assert_eq!(json, expected);
}

#[test]
fn nerve_golden_for_absent_gpu_is_info() {
    let hw = absent_hw();
    let json = to_nerve(&hw).unwrap();
    let expected = "\
{
  \"ts\": \"2026-10-19T16:45:00+02:00\",
  \"source\": \"boot\",
  \"kind\": \"hardware\",
  \"severity\": \"info\",
  \"data\": {
    \"cpu\": {
      \"processors\": 4,
      \"model\": \"Test CPU\"
    },
    \"ram\": {
      \"total_mib\": 8192,
      \"free_mib\": 4096
    },
    \"gpu\": {
      \"status\": \"absent\",
      \"count\": 0
    }
  }
}
";
    assert_eq!(json, expected);
    assert!(!json.contains('Z'));
    assert!(!json.contains("+00:00"));
}

#[test]
fn to_nerve_object_keys_are_ts_source_kind_severity_data_and_writes_no_file() {
    let tree = Tree::new();
    assert!(
        dir_names(&tree.root).is_empty(),
        "probe temp dir starts empty"
    );

    let watched = [
        PathBuf::from("/opt/galion/openmind/crates/openmind-boot/hardware.json"),
        PathBuf::from("/opt/galion/openmind/crates/openmind-boot/nerve.json"),
        PathBuf::from("/opt/galion/galion-light/state/hardware.json"),
        PathBuf::from("/opt/galion/galion-light/state/nerve.json"),
        std::env::temp_dir().join("hardware.json"),
        std::env::temp_dir().join("nerve.json"),
    ];
    let checks: Vec<_> = watched.iter().map(|path| untouched(path)).collect();

    let cwd = CwdGuard::enter(&tree.root);
    let json = to_nerve(&absent_hw()).expect("nerve json");
    assert!(dir_names(&tree.root).is_empty(), "to_nerve wrote a file");
    drop(cwd);

    for check in checks {
        check();
    }
    assert_eq!(
        top_level_object_keys(&json),
        ["ts", "source", "kind", "severity", "data"]
    );
}

#[test]
fn fixture_without_gpu_nerve_is_info_and_writes_only_temp() {
    let tree = Tree::new();
    tree.cpu_ram();
    let live = Path::new("/opt/galion/openmind/crates/openmind-boot/hardware.json");
    let state = Path::new("/opt/galion/galion-light/state/hardware.json");
    let check_live = untouched(live);
    let check_state = untouched(state);

    let hw = probe(&tree.root).unwrap();
    assert!(hw.gpu.is_absent());
    let json = to_nerve(&hw).unwrap();
    assert!(json.contains("\"severity\": \"info\""));
    assert!(json.contains("\"source\": \"boot\""));
    assert!(json.contains("\"kind\": \"hardware\""));
    assert!(json.contains("\"status\": \"absent\""));
    assert!(!json.contains("\"status\": \"present\""));
    assert!(json.contains(&format!("\"ts\": \"{}\"", hw.probed_at)));
    assert!(hw.probed_at.ends_with("+01:00") || hw.probed_at.ends_with("+02:00"));
    assert!(!hw.probed_at.contains('Z'));
    assert!(!tree.root.join("hardware.json").exists());

    let dest = tree.root.join("nerve.json");
    fs::write(&dest, &json).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), json);
    assert!(dest.starts_with(std::env::temp_dir()));
    assert_eq!(dest.file_name().unwrap(), "nerve.json");
    assert!(!tree.root.join("hardware.json").exists());
    check_live();
    check_state();
}

#[test]
fn nerve_present_gpu_stays_info() {
    let tree = Tree::new();
    tree.cpu_ram();
    tree.file(
        "proc/driver/nvidia/gpus/0000:01:00.0/information",
        "Model:          NVIDIA GeForce RTX 3090\nBus Location:   0000:01:00.0\n",
    );
    let hw = probe(&tree.root).unwrap();
    let json = to_nerve(&hw).unwrap();
    assert!(json.contains("\"severity\": \"info\""));
    assert!(json.contains("\"source\": \"boot\""));
    assert!(json.contains("\"kind\": \"hardware\""));
    assert!(json.contains("\"status\": \"present\""));
    assert!(hw.gpu.is_present());
    assert!(!tree.root.join("hardware.json").exists());
}

#[test]
fn nerve_rejects_utc_timestamp() {
    let mut hw = absent_hw();
    hw.probed_at = "2026-10-19T14:45:00Z".to_string();
    let err = to_nerve(&hw).unwrap_err();
    assert!(err.to_string().contains("never Z"));
}

#[test]
fn probe_timestamp_is_zagreb_offset() {
    let tree = Tree::new();
    tree.cpu_ram();
    let hw = probe(&tree.root).unwrap();
    assert!(hw.probed_at.ends_with("+01:00") || hw.probed_at.ends_with("+02:00"));
    assert!(!hw.probed_at.contains('Z'));
    assert!(!hw.probed_at.contains("+00:00"));
    assert!(to_json(&hw).unwrap().ends_with('\n'));
}

#[test]
fn rejects_utc_timestamp() {
    let cpu = parse_cpuinfo(CPUINFO).unwrap();
    let ram = parse_meminfo(MEMINFO).unwrap();
    let err = assemble(cpu, ram, Gpu::absent(), "2026-10-19T14:45:00Z").unwrap_err();
    assert!(err.to_string().contains("never Z"));
}

#[test]
fn missing_cpuinfo_is_error_not_a_gpu_error() {
    let tree = Tree::new();
    tree.file("proc/meminfo", MEMINFO);
    let err = probe(&tree.root).unwrap_err();
    assert!(err.to_string().contains("cpuinfo"));
}

#[test]
fn write_hardware_json_only_under_temp() {
    let tree = Tree::new();
    let dest = tree.root.join("nested").join("hardware.json");
    let hw = absent_hw();
    write_hardware_json(&hw, &dest).unwrap();
    let body = fs::read_to_string(&dest).unwrap();
    assert_eq!(body, to_json(&hw).unwrap());
    assert!(dest.starts_with(std::env::temp_dir()));
    assert_eq!(dest.file_name().unwrap(), "hardware.json");
}

#[test]
fn refuses_etc_and_does_not_create_it() {
    let dest = Path::new("/etc/openmind-boot-b54/hardware.json");
    assert!(!dest.exists());
    assert!(!Path::new("/etc/openmind-boot-b54").exists());
    let err = write_hardware_json(&absent_hw(), dest).unwrap_err();
    assert!(err.to_string().contains("/etc"), "{err}");
    assert!(!dest.exists());
    assert!(!Path::new("/etc/openmind-boot-b54").exists());
}

#[test]
fn refuses_live_galion_light_state() {
    let live = Path::new("/opt/galion/galion-light/state/hardware.json");
    let check = untouched(live);
    let err = write_hardware_json(&absent_hw(), live).unwrap_err();
    assert!(err.to_string().contains("live"), "{err}");
    check();
}

#[test]
fn refuses_workspace_and_repo_paths() {
    for dest in [
        Path::new("/workspace/galion-light/state/hardware.json"),
        Path::new("/opt/galion/openmind/crates/openmind-boot/hardware.json"),
        Path::new("/opt/galion/bin/hardware.json"),
    ] {
        let check = untouched(dest);
        let err = write_hardware_json(&absent_hw(), dest).unwrap_err();
        assert!(err.to_string().contains("live"), "{dest:?} -> {err}");
        check();
    }
}

#[test]
fn refuses_dotdot_wrong_name_and_relative() {
    let hw = absent_hw();
    let sneaky = Path::new("/tmp/../etc/openmind-boot-b54-hardware.json");
    let leaked = Path::new("/etc/openmind-boot-b54-hardware.json");
    assert!(!leaked.exists());
    let err = write_hardware_json(&hw, sneaky).unwrap_err();
    assert!(err.to_string().contains(".."), "{err}");
    assert!(!leaked.exists());

    let tree = Tree::new();
    let wrong = tree.root.join("report.json");
    let err = write_hardware_json(&hw, &wrong).unwrap_err();
    assert!(err.to_string().contains("hardware.json"), "{err}");
    assert!(!wrong.exists());

    let err = write_hardware_json(&hw, Path::new("hardware.json")).unwrap_err();
    assert!(err.to_string().contains("absolute"), "{err}");
}

#[test]
fn refuses_symlink_file_and_dir() {
    let hw = absent_hw();
    let hosts = Path::new("/etc/hosts");
    let check_hosts = untouched(hosts);

    let tree = Tree::new();
    let dest = tree.root.join("hardware.json");
    std::os::unix::fs::symlink(hosts, &dest).unwrap();
    let err = write_hardware_json(&hw, &dest).unwrap_err();
    assert!(err.to_string().contains("symlink"), "{err}");
    check_hosts();

    let tree = Tree::new();
    let link = tree.root.join("escape");
    std::os::unix::fs::symlink("/etc", &link).unwrap();
    let dest = link.join("hardware.json");
    let err = write_hardware_json(&hw, &dest).unwrap_err();
    assert!(err.to_string().contains("symlink"), "{err}");
    assert!(!Path::new("/etc/hardware.json").exists());
}

#[test]
#[ignore = "reads host /proc and /sys; does not assert which GPU is installed"]
fn host_read_does_not_error() {
    let hw = probe_host_for_ignore();
    let json = to_json(&hw).unwrap();
    assert!(!json.contains('Z'));
    assert!(!json.contains("+00:00"));
    assert!(hw.gpu.is_absent() || hw.gpu.is_present());
    eprintln!("{json}");
}

fn probe_host_for_ignore() -> Hardware {
    openmind_boot::probe_host().expect("host cpu and ram are readable")
}
