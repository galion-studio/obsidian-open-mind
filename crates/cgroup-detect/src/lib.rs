// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Read-only cgroup v2 delegation probe.
//!
//! [`detect`] only reads `/proc` and `/sys`. It never creates a cgroup and never
//! writes a cgroup file. A parent under `system.slice` is never a per-run candidate.

#![deny(unsafe_code)]

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Controllers a per-run limit cgroup needs from the parent's `cgroup.subtree_control`.
pub const PER_RUN_CONTROLLERS: &[&str] = &["cpu", "memory", "pids"];

/// Result of a read-only delegation check for the calling process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Unified hierarchy: a single `0::` line in `/proc/self/cgroup`, a cgroup2
    /// mount, and no cgroup v1 mount.
    pub unified_v2: bool,
    /// cgroup2 mount point, normally `/sys/fs/cgroup`.
    pub mount_point: Option<PathBuf>,
    /// Mount options include `rw`.
    pub mount_writable: bool,
    /// Cgroup path of this process, relative to the cgroup namespace root.
    pub self_path: String,
    /// Controllers enabled on the cgroup root (`cgroup.controllers`).
    pub available_controllers: Vec<String>,
    /// Controllers this process's own cgroup can use (`cgroup.controllers`).
    pub self_controllers: Vec<String>,
    /// Controllers delegated to children of this process's cgroup.
    pub self_subtree_control: Vec<String>,
    /// Nearest ancestor where a child cgroup would already receive the per-run
    /// controllers. Never under `system.slice`. `None` when that is not possible.
    pub candidate_parent: Option<String>,
    /// `cgroup.subtree_control` of [`Self::candidate_parent`], when there is one.
    pub delegated_controllers: Vec<String>,
    /// `true` only when [`Self::candidate_parent`] is set.
    pub per_run_possible: bool,
    /// One English sentence. No cgroup was created to produce it.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cred {
    fsuid: u32,
    fsgid: u32,
    groups: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    path: String,
    controllers: Vec<String>,
    subtree: Vec<String>,
    kind: String,
    frozen: bool,
    max_depth: String,
    max_descendants: String,
    nr_descendants: u64,
    dir_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snap {
    unified: bool,
    mount: Option<PathBuf>,
    mount_rw: bool,
    self_path: String,
    root_controllers: Vec<String>,
    self_controllers: Vec<String>,
    self_subtree: Vec<String>,
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MountScan {
    cgroup2: Option<(PathBuf, bool)>,
    has_v1: bool,
}

/// Read `/proc` and `/sys` and report whether a per-run cgroup is possible.
///
/// Does not create a directory and does not write under `/sys` or `/proc`.
pub fn detect() -> io::Result<Report> {
    let status = fs::read_to_string("/proc/self/status")?;
    let cred = cred_from_status(&status)?;
    let cg_txt = fs::read_to_string("/proc/self/cgroup")?;
    let mi_txt = fs::read_to_string("/proc/self/mountinfo")?;
    Ok(decide(&snap_from_text(&cg_txt, &mi_txt, &cred)?))
}

fn snap_from_text(cg_txt: &str, mi_txt: &str, cred: &Cred) -> io::Result<Snap> {
    let parsed = parse_self_cgroup(cg_txt).filter(|p| cg_path_ok(p));
    let mount = parse_mountinfo(mi_txt);
    let unified = parsed.is_some() && mount.cgroup2.is_some() && !mount.has_v1;
    let self_path = parsed.unwrap_or_default();
    let (mount_point, mount_rw) = match mount.cgroup2 {
        Some((path, rw)) => (Some(path), rw),
        None => (None, false),
    };
    let mut nodes = Vec::new();
    if unified {
        if let Some(mount_path) = &mount_point {
            for path in ancestor_paths(&self_path) {
                if let Some(node) = load_node(mount_path, &path, cred, mount_rw) {
                    nodes.push(node);
                }
            }
        }
    }
    let self_controllers = nodes
        .first()
        .map(|n| n.controllers.clone())
        .unwrap_or_default();
    let self_subtree = nodes.first().map(|n| n.subtree.clone()).unwrap_or_default();
    let root_controllers = nodes
        .iter()
        .find(|n| n.path == "/")
        .map(|n| n.controllers.clone())
        .unwrap_or_default();
    Ok(Snap {
        unified,
        mount: mount_point,
        mount_rw,
        self_path,
        root_controllers,
        self_controllers,
        self_subtree,
        nodes,
    })
}

fn load_node(mount: &Path, path: &str, cred: &Cred, mount_rw: bool) -> Option<Node> {
    let dir = cg_dir(mount, path)?;
    let meta = fs::metadata(&dir).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let events = read_trim(&dir.join("cgroup.events"));
    let stat = read_trim(&dir.join("cgroup.stat"));
    Some(Node {
        path: path.to_string(),
        controllers: read_words(&dir.join("cgroup.controllers")),
        subtree: read_words(&dir.join("cgroup.subtree_control")),
        kind: read_trim(&dir.join("cgroup.type")),
        frozen: event_flag(&events, "frozen"),
        max_depth: read_trim(&dir.join("cgroup.max.depth")),
        max_descendants: read_trim(&dir.join("cgroup.max.descendants")),
        nr_descendants: parse_nr_descendants(&stat),
        dir_writable: mount_rw && mode_writable(&meta, cred),
    })
}

fn decide(snap: &Snap) -> Report {
    if !snap.unified {
        return refuse(
            snap,
            "per-run cgroup not possible: not a unified cgroup v2 hierarchy",
        );
    }
    if snap.mount.is_none() {
        return refuse(snap, "per-run cgroup not possible: cgroup2 mount not found");
    }
    if !snap.mount_rw {
        return refuse(
            snap,
            "per-run cgroup not possible: cgroup2 mount is read-only",
        );
    }
    for node in &snap.nodes {
        if !parent_ok(node) {
            continue;
        }
        return Report {
            unified_v2: true,
            mount_point: snap.mount.clone(),
            mount_writable: true,
            self_path: snap.self_path.clone(),
            available_controllers: snap.root_controllers.clone(),
            self_controllers: snap.self_controllers.clone(),
            self_subtree_control: snap.self_subtree.clone(),
            candidate_parent: Some(node.path.clone()),
            delegated_controllers: node.subtree.clone(),
            per_run_possible: true,
            reason: format!(
                "per-run cgroup possible under {}: subtree already delegates cpu, memory, and pids",
                node.path
            ),
        };
    }
    refuse(
        snap,
        "per-run cgroup not possible: no writable domain ancestor outside system.slice delegates cpu, memory, and pids",
    )
}

fn parent_ok(node: &Node) -> bool {
    if under_system_slice(&node.path) {
        return false;
    }
    if node.kind != "domain" || node.frozen || !node.dir_writable {
        return false;
    }
    if !has_controllers(&node.subtree) {
        return false;
    }
    allows_child(&node.max_depth, &node.max_descendants, node.nr_descendants)
}

fn refuse(snap: &Snap, reason: &str) -> Report {
    Report {
        unified_v2: snap.unified,
        mount_point: snap.mount.clone(),
        mount_writable: snap.mount_rw,
        self_path: snap.self_path.clone(),
        available_controllers: snap.root_controllers.clone(),
        self_controllers: snap.self_controllers.clone(),
        self_subtree_control: snap.self_subtree.clone(),
        candidate_parent: None,
        delegated_controllers: Vec::new(),
        per_run_possible: false,
        reason: reason.to_string(),
    }
}

fn has_controllers(subtree: &[String]) -> bool {
    PER_RUN_CONTROLLERS
        .iter()
        .all(|need| subtree.iter().any(|have| have == need))
}

fn under_system_slice(path: &str) -> bool {
    path == "/system.slice" || path.starts_with("/system.slice/")
}

fn allows_child(max_depth: &str, max_desc: &str, nr: u64) -> bool {
    let depth_ok = match max_depth {
        "max" => true,
        other => other.parse::<u64>().map(|n| n > 0).unwrap_or(false),
    };
    let desc_ok = match max_desc {
        "max" => true,
        other => other.parse::<u64>().map(|n| n > nr).unwrap_or(false),
    };
    depth_ok && desc_ok
}

fn parse_self_cgroup(text: &str) -> Option<String> {
    let mut path = None;
    let mut lines = 0usize;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        lines += 1;
        let mut parts = line.splitn(3, ':');
        let id = parts.next()?;
        let _controllers = parts.next()?;
        let cg = parts.next()?;
        if id != "0" {
            return None;
        }
        path = Some(normalize_cg(cg));
    }
    if lines == 1 {
        path
    } else {
        None
    }
}

fn normalize_cg(path: &str) -> String {
    if path.len() > 1 && path.ends_with('/') {
        path.trim_end_matches('/').to_string()
    } else {
        path.to_string()
    }
}

fn cg_path_ok(path: &str) -> bool {
    path.starts_with('/')
        && !path
            .split('/')
            .any(|part| part == ".." || part.contains('\0'))
}

fn ancestor_paths(start: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = start.to_string();
    loop {
        out.push(cur.clone());
        if cur == "/" {
            break;
        }
        cur = match cur.rfind('/') {
            Some(0) | None => "/".to_string(),
            Some(i) => cur[..i].to_string(),
        };
    }
    out
}

fn parse_mountinfo(text: &str) -> MountScan {
    let mut found = Vec::new();
    let mut has_v1 = false;
    for line in text.lines() {
        let Some((pre, post)) = line.split_once(" - ") else {
            continue;
        };
        let fields: Vec<&str> = pre.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        let fstype = post.split_whitespace().next().unwrap_or("");
        if fstype == "cgroup" {
            has_v1 = true;
        }
        if fstype == "cgroup2" {
            let rw = fields[5].split(',').any(|opt| opt == "rw");
            found.push((PathBuf::from(unescape_mount(fields[4])), rw));
        }
    }
    let cgroup2 = found
        .iter()
        .find(|(path, _)| path == Path::new("/sys/fs/cgroup"))
        .cloned()
        .or_else(|| found.into_iter().next());
    MountScan { cgroup2, has_v1 }
}

fn unescape_mount(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let oct: String = chars.clone().take(3).collect();
        if oct.len() == 3 && oct.bytes().all(|b| b.is_ascii_digit()) {
            let val = u8::from_str_radix(&oct, 8).unwrap_or(b'?');
            out.push(char::from(val));
            chars.next();
            chars.next();
            chars.next();
        } else {
            out.push('\\');
        }
    }
    out
}

fn cg_dir(mount: &Path, cg_path: &str) -> Option<PathBuf> {
    if !cg_path_ok(cg_path) {
        return None;
    }
    if cg_path == "/" {
        Some(mount.to_path_buf())
    } else {
        Some(mount.join(cg_path.trim_start_matches('/')))
    }
}

fn read_trim(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

fn read_words(path: &Path) -> Vec<String> {
    read_trim(path)
        .split_whitespace()
        .map(|word| word.to_string())
        .collect()
}

fn event_flag(text: &str, key: &str) -> bool {
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next() == Some(key) && parts.next() == Some("1")
    })
}

fn parse_nr_descendants(text: &str) -> u64 {
    text.lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            if parts.next() == Some("nr_descendants") {
                parts.next()?.parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn cred_from_status(text: &str) -> io::Result<Cred> {
    let mut uid = None;
    let mut gid = None;
    let mut groups = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            uid = four(rest);
        } else if let Some(rest) = line.strip_prefix("Gid:") {
            gid = four(rest);
        } else if let Some(rest) = line.strip_prefix("Groups:") {
            groups = rest
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
        }
    }
    let uid = uid.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Uid missing"))?;
    let gid = gid.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Gid missing"))?;
    Ok(Cred {
        fsuid: uid[3],
        fsgid: gid[3],
        groups,
    })
}

fn four(rest: &str) -> Option<[u32; 4]> {
    let nums: Vec<u32> = rest
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    if nums.len() < 4 {
        None
    } else {
        Some([nums[0], nums[1], nums[2], nums[3]])
    }
}

fn mode_writable(meta: &fs::Metadata, cred: &Cred) -> bool {
    let mode = meta.permissions().mode();
    if cred.fsuid == 0 {
        return true;
    }
    if cred.fsuid == meta.uid() {
        return mode & 0o200 != 0;
    }
    if cred.fsgid == meta.gid() || cred.groups.contains(&meta.gid()) {
        return mode & 0o020 != 0;
    }
    mode & 0o002 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(|w| w.to_string()).collect()
    }

    fn node(path: &str, subtree: &str, writable: bool) -> Node {
        Node {
            path: path.to_string(),
            controllers: words("cpu memory pids"),
            subtree: words(subtree),
            kind: "domain".to_string(),
            frozen: false,
            max_depth: "max".to_string(),
            max_descendants: "max".to_string(),
            nr_descendants: 0,
            dir_writable: writable,
        }
    }

    fn snap(nodes: Vec<Node>) -> Snap {
        let self_path = nodes.first().map(|n| n.path.clone()).unwrap_or_default();
        let self_controllers = nodes
            .first()
            .map(|n| n.controllers.clone())
            .unwrap_or_default();
        let self_subtree = nodes.first().map(|n| n.subtree.clone()).unwrap_or_default();
        let root_controllers = nodes
            .iter()
            .find(|n| n.path == "/")
            .map(|n| n.controllers.clone())
            .unwrap_or_default();
        Snap {
            unified: true,
            mount: Some(PathBuf::from("/sys/fs/cgroup")),
            mount_rw: true,
            self_path,
            root_controllers,
            self_controllers,
            self_subtree,
            nodes,
        }
    }

    #[test]
    fn nearest_delegated_ancestor_outside_the_session() {
        let report = decide(&snap(vec![
            node("/user.slice/user-0.slice/session-1.scope", "", true),
            node("/user.slice/user-0.slice", "cpu memory pids", true),
            node("/user.slice", "cpu memory pids", true),
            node("/", "cpu memory pids", true),
        ]));
        assert!(report.per_run_possible);
        assert_eq!(
            report.candidate_parent.as_deref(),
            Some("/user.slice/user-0.slice")
        );
        assert!(report.reason.contains("/user.slice/user-0.slice"));
    }

    #[test]
    fn system_slice_is_never_the_parent() {
        let report = decide(&snap(vec![
            node("/system.slice/hermes-rs.service", "", true),
            node("/system.slice", "cpu memory pids", true),
            node("/", "cpu memory pids", true),
        ]));
        assert!(report.per_run_possible);
        assert_eq!(report.candidate_parent.as_deref(), Some("/"));
        assert!(!report.candidate_parent.unwrap().contains("system.slice"));
    }

    #[test]
    fn delegated_service_under_system_slice_is_refused() {
        let mut root = node("/", "cpu memory pids", false);
        root.dir_writable = false;
        let report = decide(&snap(vec![
            node("/system.slice/sandbox.service", "cpu memory pids", true),
            node("/system.slice", "cpu memory pids", true),
            root,
        ]));
        assert!(!report.per_run_possible);
        assert!(report.candidate_parent.is_none());
    }

    #[test]
    fn self_is_the_candidate_when_it_already_delegates() {
        let report = decide(&snap(vec![
            node("/custom.slice/runner", "cpu memory pids", true),
            node("/", "cpu memory pids", true),
        ]));
        assert_eq!(
            report.candidate_parent.as_deref(),
            Some("/custom.slice/runner")
        );
    }

    #[test]
    fn no_room_and_not_unified() {
        let mut only = node("/user.slice/user-0.slice", "cpu memory pids", true);
        only.max_descendants = "2".to_string();
        only.nr_descendants = 2;
        let full = decide(&snap(vec![only]));
        assert!(!full.per_run_possible);

        let mut hybrid = snap(vec![node("/", "cpu memory pids", true)]);
        hybrid.unified = false;
        let report = decide(&hybrid);
        assert!(!report.per_run_possible);
        assert!(!report.unified_v2);
    }

    #[test]
    fn frozen_or_threaded_parent_is_skipped() {
        let mut frozen = node("/user.slice/user-0.slice", "cpu memory pids", true);
        frozen.frozen = true;
        let mut threaded = node("/user.slice", "cpu memory pids", true);
        threaded.kind = "domain threaded".to_string();
        let report = decide(&snap(vec![
            frozen,
            threaded,
            node("/", "cpu memory pids", true),
        ]));
        assert_eq!(report.candidate_parent.as_deref(), Some("/"));
    }

    #[test]
    fn parsers_match_v2_mount_text() {
        let line = "3429 3421 0:29 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:2196 - cgroup2 none rw";
        let scan = parse_mountinfo(line);
        assert!(!scan.has_v1);
        let (path, rw) = scan.cgroup2.unwrap();
        assert_eq!(path, PathBuf::from("/sys/fs/cgroup"));
        assert!(rw);
        let mixed = format!("{line}\n1 2 0:20 / /sys/fs/cgroup/memory rw - cgroup memory rw\n");
        assert!(parse_mountinfo(&mixed).has_v1);
        assert_eq!(
            parse_self_cgroup("0::/user.slice/user-0.slice/session-1.scope\n").as_deref(),
            Some("/user.slice/user-0.slice/session-1.scope")
        );
        assert!(parse_self_cgroup("0::/a\n1:name=systemd:/b\n").is_none());
        let cred = cred_from_status("Uid:\t0\t1\t2\t3\nGid:\t4\t5\t6\t7\nGroups:\t8 9\n").unwrap();
        assert_eq!(cred.fsuid, 3);
        assert_eq!(cred.fsgid, 7);
        assert_eq!(cred.groups, vec![8, 9]);
    }
}
