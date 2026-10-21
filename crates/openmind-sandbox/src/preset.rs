// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Policy presets.
//!
//! Each preset is data for a later runner. It stores limits and paths.
//! [`Preset::policy`] copies that data into a [`SandboxPolicy`].
//! It does not start a program, enter a kernel namespace, or create a file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::SandboxPolicy;

const FS_READ: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/etc",
    "/proc",
    "/dev/zero",
    "/dev/urandom",
    "/dev/random",
];

/// Directories covered by an empty read-only tmpfs in every tool preset.
///
/// `/opt/galion` holds the live deployment (checkouts, Hermes state, publish
/// dirs). On the VPS a world-readable `.env` under it was readable from the
/// sandbox (S8, swarm/sandbox-real-problems 334c633), so tools never see it.
/// A tool whose workdir, write root, read root or program lies under
/// `/opt/galion` keeps it visible, and the outcome reports that (`hide_home`).
const HIDE: &[&str] = &["/root", "/home", "/run/secrets", "/opt/galion"];

const ALLOW_RO: &[&str] = &["PATH", "LANG", "LC_ALL", "TZ"];
const ALLOW_BUILD: &[&str] = &["PATH", "LANG", "LC_ALL", "TZ", "TERM"];
const ALLOW_NET: &[&str] = &[
    "PATH",
    "LANG",
    "LC_ALL",
    "TZ",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

const ENV_HOME: &[(&str, &str)] = &[("HOME", "{workdir}"), ("TMPDIR", "{workdir}")];
const ENV_BUILD: &[(&str, &str)] = &[
    ("HOME", "{workdir}"),
    ("TMPDIR", "{workdir}"),
    ("CARGO_HOME", "{workdir}/.cargo"),
    ("CARGO_TARGET_DIR", "{workdir}/target"),
];

/// One named sandbox policy template. Schema 1. No program and no arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    name: &'static str,
    schema: u32,
    env_allowlist: &'static [&'static str],
    env_set: &'static [(&'static str, &'static str)],
    wall_timeout_secs: u64,
    cpu_secs: u64,
    mem_bytes: u64,
    nofile: u64,
    fsize_bytes: u64,
    output_cap: usize,
    net: bool,
    require_net_isolation: bool,
    landlock: bool,
    fs_read: &'static [&'static str],
    fs_write: &'static [&'static str],
    namespaces: bool,
    ro_root: bool,
    hide_paths: &'static [&'static str],
}

impl Preset {
    const fn base() -> Self {
        Self {
            name: "",
            schema: 1,
            env_allowlist: ALLOW_RO,
            env_set: ENV_HOME,
            wall_timeout_secs: 15,
            cpu_secs: 5,
            mem_bytes: 256 * 1024 * 1024,
            nofile: 64,
            fsize_bytes: 1024 * 1024,
            output_cap: 64 * 1024,
            net: false,
            require_net_isolation: true,
            landlock: true,
            fs_read: FS_READ,
            fs_write: &[],
            namespaces: true,
            ro_root: true,
            hide_paths: HIDE,
        }
    }

    /// Short inspection tool. Network must be isolated by the later runner.
    #[must_use]
    pub const fn read_only_tool() -> Self {
        Self {
            name: "read-only-tool",
            ..Self::base()
        }
    }

    /// Compiler-style tool. Outputs stay under the caller workdir.
    #[must_use]
    pub const fn build_tool() -> Self {
        Self {
            name: "build-tool",
            env_allowlist: ALLOW_BUILD,
            env_set: ENV_BUILD,
            wall_timeout_secs: 600,
            cpu_secs: 300,
            mem_bytes: 1024 * 1024 * 1024,
            nofile: 1024,
            fsize_bytes: 512 * 1024 * 1024,
            output_cap: 256 * 1024,
            ..Self::base()
        }
    }

    /// Same closed tree as `read-only-tool`, with the network left open.
    #[must_use]
    pub const fn net_tool() -> Self {
        Self {
            name: "net-tool",
            env_allowlist: ALLOW_NET,
            wall_timeout_secs: 30,
            cpu_secs: 10,
            fsize_bytes: 32 * 1024 * 1024,
            output_cap: 256 * 1024,
            net: true,
            require_net_isolation: false,
            ..Self::base()
        }
    }

    /// The three shipped presets, in file name order.
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::build_tool(), Self::net_tool(), Self::read_only_tool()]
    }

    /// Look up a shipped preset by its file name, without the suffix.
    #[must_use]
    pub fn get(name: &str) -> Option<Self> {
        match name {
            "read-only-tool" => Some(Self::read_only_tool()),
            "build-tool" => Some(Self::build_tool()),
            "net-tool" => Some(Self::net_tool()),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub const fn schema(&self) -> u32 {
        self.schema
    }

    /// Build a policy for `workdir`.
    ///
    /// The directory is not created. Nothing is executed. No file is written.
    /// `{workdir}` in environment values becomes the UTF-8 path of `workdir`.
    #[must_use]
    pub fn policy(&self, workdir: &Path) -> SandboxPolicy {
        let root = workdir.to_path_buf();
        let slot = root.to_str().unwrap_or("{workdir}").to_string();
        let subst = |value: &str| value.replace("{workdir}", slot.as_str());
        SandboxPolicy {
            workdir: root,
            env_allowlist: owned(self.env_allowlist),
            env_set: self
                .env_set
                .iter()
                .copied()
                .map(|(key, value)| (key.to_string(), subst(value)))
                .collect(),
            wall_timeout: Duration::from_secs(self.wall_timeout_secs),
            cpu_secs: self.cpu_secs,
            mem_bytes: self.mem_bytes,
            nofile: self.nofile,
            fsize_bytes: self.fsize_bytes,
            output_cap: self.output_cap,
            net: self.net,
            require_net_isolation: self.require_net_isolation,
            landlock: self.landlock,
            fs_read: paths(self.fs_read),
            fs_write: paths(self.fs_write),
            namespaces: self.namespaces,
            ro_root: self.ro_root,
            hide_paths: paths(self.hide_paths),
            // Callers opt in by setting a delegated parent (cgroup-detect).
            cgroup_parent: None,
            cgroup_pids_max: 64,
            cgroup_mem_bytes: self.mem_bytes,
            cgroup_cpu_pct: 0,
        }
    }
}

fn owned(list: &[&str]) -> Vec<String> {
    list.iter().copied().map(str::to_string).collect()
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().copied().map(PathBuf::from).collect()
}
