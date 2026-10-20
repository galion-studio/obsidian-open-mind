// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use openmind_sandbox::{Preset, SandboxPolicy};

const BANNED: &[&str] = &[
    "unshare",
    "fork",
    "Command",
    "std::process",
    "std::fs",
    "libc",
    ".spawn",
    "std::thread",
];

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("om-sand2-{}-{nanos}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for ent in fs::read_dir(dir).unwrap() {
        found.push(ent.unwrap().file_name().to_string_lossy().into_owned());
    }
    found.sort();
    found
}

fn child_pids() -> Vec<u32> {
    let me = std::process::id().to_string();
    let mut pids = Vec::new();
    for ent in fs::read_dir("/proc").unwrap().flatten() {
        let name = ent.file_name();
        let Some(text) = name.to_str() else { continue };
        if text.parse::<u32>().is_err() {
            continue;
        }
        let Ok(stat) = fs::read_to_string(ent.path().join("stat")) else {
            continue;
        };
        let Some(end) = stat.rfind(')') else { continue };
        let mut parts = stat[end + 1..].split_whitespace();
        let _state = parts.next();
        if parts.next() == Some(me.as_str()) {
            pids.push(text.parse().unwrap());
        }
    }
    pids.sort_unstable();
    pids
}

fn namespaces() -> Vec<String> {
    let mut out = Vec::new();
    for name in ["cgroup", "ipc", "mnt", "net", "pid", "user", "uts"] {
        let path = format!("/proc/self/ns/{name}");
        let link = fs::read_link(&path).unwrap_or_else(|err| panic!("{path}: {err}"));
        out.push(format!("{name}:{}", link.display()));
    }
    out
}

/// Child CPU and fault counters from /proc/self/stat. A reaped child moves these.
fn child_account() -> String {
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let end = stat.rfind(')').unwrap();
    let fields: Vec<&str> = stat[end + 1..].split_whitespace().collect();
    assert!(fields.len() > 14, "{stat}");
    format!("{}:{}:{}:{}", fields[8], fields[10], fields[13], fields[14])
}

fn toml_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/sandbox/presets");
    let mut paths = vec![
        root.join("build-tool.toml"),
        root.join("net-tool.toml"),
        root.join("read-only-tool.toml"),
    ];
    paths.sort();
    paths
}

fn read_files(paths: &[PathBuf]) -> Vec<Vec<u8>> {
    paths.iter().map(|p| fs::read(p).unwrap()).collect()
}

fn assert_contract(preset: Preset, dir: &Path) {
    assert_eq!(preset.schema(), 1);
    let policy = preset.policy(dir);
    assert_eq!(policy.workdir, dir);
    assert!(policy.fs_write.is_empty());
    assert!(policy.landlock);
    assert!(policy.namespaces);
    assert!(policy.ro_root);
    assert_eq!(
        policy
            .hide_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
        vec![
            "/root".to_string(),
            "/home".to_string(),
            "/run/secrets".to_string()
        ]
    );
    let dir_text = dir.to_str().unwrap();
    for (key, value) in &policy.env_set {
        assert!(
            value == dir_text || value.starts_with(&format!("{dir_text}/")),
            "{key}={value} leaves the temp dir"
        );
    }
    match preset.name() {
        "read-only-tool" => assert_read_only(&policy),
        "build-tool" => assert_build(&policy, dir_text),
        "net-tool" => assert_net(&policy),
        other => panic!("unknown preset {other}"),
    }
}

fn strs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

fn assert_read_only(policy: &SandboxPolicy) {
    assert_eq!(policy.wall_timeout.as_secs(), 15);
    assert_eq!(policy.cpu_secs, 5);
    assert_eq!(policy.mem_bytes, 268_435_456);
    assert_eq!(policy.nofile, 64);
    assert_eq!(policy.fsize_bytes, 1_048_576);
    assert_eq!(policy.output_cap, 65_536);
    assert!(!policy.net);
    assert!(policy.require_net_isolation);
    assert_eq!(
        strs(&policy.env_allowlist),
        ["PATH", "LANG", "LC_ALL", "TZ"]
    );
}

fn assert_build(policy: &SandboxPolicy, dir_text: &str) {
    assert_eq!(policy.wall_timeout.as_secs(), 600);
    assert_eq!(policy.cpu_secs, 300);
    assert_eq!(policy.mem_bytes, 1_073_741_824);
    assert_eq!(policy.nofile, 1024);
    assert_eq!(policy.fsize_bytes, 536_870_912);
    assert_eq!(policy.output_cap, 262_144);
    assert!(!policy.net);
    assert!(policy.require_net_isolation);
    assert_eq!(
        strs(&policy.env_allowlist),
        ["PATH", "LANG", "LC_ALL", "TZ", "TERM"]
    );
    assert!(policy
        .env_set
        .iter()
        .any(|(k, v)| k == "CARGO_HOME" && v == &format!("{dir_text}/.cargo")));
    assert!(policy
        .env_set
        .iter()
        .any(|(k, v)| k == "CARGO_TARGET_DIR" && v == &format!("{dir_text}/target")));
}

fn assert_net(policy: &SandboxPolicy) {
    assert_eq!(policy.wall_timeout.as_secs(), 30);
    assert_eq!(policy.cpu_secs, 10);
    assert_eq!(policy.mem_bytes, 268_435_456);
    assert_eq!(policy.nofile, 64);
    assert_eq!(policy.fsize_bytes, 33_554_432);
    assert_eq!(policy.output_cap, 262_144);
    assert!(policy.net);
    assert!(!policy.require_net_isolation);
    assert_eq!(
        strs(&policy.env_allowlist),
        [
            "PATH",
            "LANG",
            "LC_ALL",
            "TZ",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR"
        ]
    );
}

/// A preset builds a policy and stops. It must not spawn a process,
/// call unshare, or write outside the temp dir this test creates and deletes.
#[test]
fn preset_does_not_spawn_unshare_or_write_outside_temp_dir() {
    let src = include_str!("../src/preset.rs");
    for banned in BANNED {
        assert!(!src.contains(banned), "preset source contains {banned}");
    }

    let dir = TempDir::new();
    let marker = dir.path().join("keep");
    fs::write(&marker, b"test\n").unwrap();
    let leak = std::env::temp_dir().join(format!(
        "om-sand2-leak-{}-{}",
        std::process::id(),
        dir.path().file_name().unwrap().to_string_lossy()
    ));
    assert!(
        !leak.exists(),
        "leak path already present: {}",
        leak.display()
    );

    let files = toml_files();
    let toml_before = read_files(&files);
    for body in &toml_before {
        let text = String::from_utf8(body.clone()).unwrap();
        assert!(text.contains("schema: 1"));
        assert!(!text.contains("argv"));
        assert!(text.contains("fs_write = []"));
    }
    let listing = names(dir.path());
    let marker_bytes = fs::read(&marker).unwrap();
    let mtime = fs::metadata(dir.path()).unwrap().modified().unwrap();
    let kids = child_pids();
    let ns = namespaces();
    let account = child_account();

    assert!(Preset::get("missing").is_none());
    for preset in Preset::all() {
        assert_eq!(Preset::get(preset.name()), Some(preset));
        assert_contract(preset, dir.path());
    }
    let absent = dir.path().join("not-created");
    let unused = Preset::read_only_tool().policy(&absent);
    assert_eq!(unused.workdir, absent);
    assert!(!absent.exists());

    assert_eq!(child_pids(), kids, "a preset spawned a process");
    assert_eq!(namespaces(), ns, "a preset called unshare");
    assert_eq!(child_account(), account, "a preset reaped a child");
    assert_eq!(names(dir.path()), listing);
    assert_eq!(fs::read(&marker).unwrap(), marker_bytes);
    assert_eq!(fs::metadata(dir.path()).unwrap().modified().unwrap(), mtime);
    assert!(!leak.exists(), "preset wrote {}", leak.display());
    assert_eq!(
        read_files(&files),
        toml_before,
        "preset wrote a preset file"
    );
    for path in &files {
        assert!(path.is_file());
        assert!(!path.starts_with(dir.path()));
    }

    let path = dir.path().to_path_buf();
    drop(dir);
    assert!(!path.exists(), "temp dir was not deleted");
    assert!(!leak.exists());
}
