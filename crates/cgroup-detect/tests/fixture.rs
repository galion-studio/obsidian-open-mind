// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Reading a fixture must not write a cgroup file and must not call a delegate.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

struct Fixture {
    root: PathBuf,
    mount: PathBuf,
}

impl Fixture {
    fn create() -> Self {
        let nanos = SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("om-cgroup2-fixture-{}-{nanos}", std::process::id()));
        let mount = root.join("sys/fs/cgroup");
        let fix = Self { root, mount };
        let _ = fs::remove_dir_all(&fix.root);
        fs::create_dir_all(fix.root.join("proc/self")).unwrap();
        write_node(&fix.mount, "cpu memory pids\n");
        write_node(&fix.mount.join("user.slice"), "cpu memory pids\n");
        write_node(
            &fix.mount.join("user.slice/user-0.slice"),
            "cpu memory pids\n",
        );
        write_node(
            &fix.mount.join("user.slice/user-0.slice/session-1.scope"),
            "\n",
        );
        fs::write(
            fix.root.join("proc/self/status"),
            "Uid:\t0\t0\t0\t0\nGid:\t0\t0\t0\t0\nGroups:\t0\n",
        )
        .unwrap();
        fs::write(
            fix.root.join("proc/self/cgroup"),
            "0::/user.slice/user-0.slice/session-1.scope\n",
        )
        .unwrap();
        fs::write(
            fix.root.join("proc/self/mountinfo"),
            format!(
                "10 1 0:29 / {} rw,nosuid,nodev,noexec,relatime - cgroup2 none rw\n",
                fix.mount.to_str().unwrap()
            ),
        )
        .unwrap();
        fix
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let name_ok = self
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("om-cgroup2-fixture-"));
        if name_ok && self.root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn write_node(dir: &Path, subtree: &str) {
    fs::create_dir_all(dir).unwrap();
    let files = [
        ("cgroup.controllers", "cpu memory pids\n"),
        ("cgroup.subtree_control", subtree),
        ("cgroup.type", "domain\n"),
        ("cgroup.events", "populated 0\nfrozen 0\n"),
        ("cgroup.stat", "nr_descendants 0\n"),
        ("cgroup.max.depth", "max\n"),
        ("cgroup.max.descendants", "max\n"),
    ];
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
}

fn stamp(root: &Path) -> Vec<(PathBuf, SystemTime, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, SystemTime, Option<Vec<u8>>)>) {
        let meta = fs::metadata(dir).unwrap();
        out.push((dir.to_path_buf(), meta.modified().unwrap(), None));
        let mut kids: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .map(|ent| ent.unwrap().path())
            .collect();
        kids.sort();
        for path in kids {
            if path.is_dir() {
                walk(&path, out);
            } else {
                let meta = fs::metadata(&path).unwrap();
                let bytes = fs::read(&path).unwrap();
                out.push((path, meta.modified().unwrap(), Some(bytes)));
            }
        }
    }
    walk(root, &mut out);
    out
}

#[test]
fn reading_a_fixture_does_not_write_a_cgroup_file_or_call_a_delegate() {
    let fix = Fixture::create();
    assert!(fix.root.starts_with(std::env::temp_dir()));
    assert!(!fix.mount.starts_with("/sys/fs/cgroup"));
    let before = stamp(&fix.root);
    let report = cgroup_detect::detect_fixture(&fix.root, || panic!("delegate called"))
        .expect("read fixture");
    let after = stamp(&fix.root);
    assert_eq!(before, after, "reading the fixture wrote a cgroup file");
    assert!(report.unified_v2, "{}", report.reason);
    assert!(report.mount_writable);
    assert_eq!(report.self_path, "/user.slice/user-0.slice/session-1.scope");
    assert_eq!(report.mount_point.as_deref(), Some(fix.mount.as_path()));
    assert!(report.per_run_possible, "{}", report.reason);
    assert_eq!(
        report.candidate_parent.as_deref(),
        Some("/user.slice/user-0.slice")
    );
    assert_eq!(
        report.delegated_controllers,
        vec!["cpu".to_string(), "memory".to_string(), "pids".to_string()]
    );
    assert!(!report.candidate_parent.unwrap().contains("system.slice"));
}
