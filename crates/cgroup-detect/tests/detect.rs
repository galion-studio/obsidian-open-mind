// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Live probe. This file only reads `/proc` and `/sys`.

use std::fs;
use std::path::{Path, PathBuf};

fn cg_file(mount: &Path, cg: &str, name: &str) -> PathBuf {
    if cg == "/" {
        mount.join(name)
    } else {
        mount.join(cg.trim_start_matches('/')).join(name)
    }
}

fn fstype(line: &str) -> Option<&str> {
    line.split_once(" - ")?.1.split_whitespace().next()
}

fn has_all(list: &[String]) -> bool {
    cgroup_detect::PER_RUN_CONTROLLERS
        .iter()
        .all(|need| list.iter().any(|have| have == need))
}

#[test]
fn live_cgroup_v2_delegation() {
    let report = cgroup_detect::detect().expect("read /proc and /sys");
    // GALION_REAL_CGROUP=1 means "this process was started inside a
    // delegated subtree" (scripts/real/sandbox.sh sets that up as root on
    // the box): then a missing delegation is a failure, not a skip.
    let required = std::env::var("GALION_REAL_CGROUP").as_deref() == Ok("1");
    if required {
        assert!(report.per_run_possible, "delegation expected: {report:?}");
    }
    if !(report.unified_v2 && report.mount_writable && report.per_run_possible) {
        println!(
            "SKIP live_cgroup_v2_delegation: no delegated, writable cgroup v2 subtree on this machine ({}); this check only means something on the target host",
            report.reason
        );
        return;
    }
    let cg = fs::read_to_string("/proc/self/cgroup").unwrap();
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap();
    let self_path = cg
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .expect("0:: line");
    assert_eq!(report.self_path, self_path);
    assert!(
        mountinfo
            .lines()
            .any(|line| fstype(line) == Some("cgroup2")),
        "cgroup2 mount"
    );
    assert!(
        !mountinfo.lines().any(|line| fstype(line) == Some("cgroup")),
        "no cgroup v1 mount"
    );
    assert!(report.unified_v2, "{}", report.reason);
    assert!(report.mount_writable);
    let mount = report.mount_point.clone().expect("mount point");
    assert_eq!(mount, PathBuf::from("/sys/fs/cgroup"));

    let root_ctrl = fs::read_to_string(mount.join("cgroup.controllers")).unwrap();
    let expected: Vec<String> = root_ctrl.split_whitespace().map(str::to_string).collect();
    assert_eq!(report.available_controllers, expected);

    let self_ctrl =
        fs::read_to_string(cg_file(&mount, &report.self_path, "cgroup.controllers")).unwrap();
    let self_sub =
        fs::read_to_string(cg_file(&mount, &report.self_path, "cgroup.subtree_control")).unwrap();
    assert_eq!(
        report.self_controllers,
        self_ctrl
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        report.self_subtree_control,
        self_sub
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );

    assert!(report.per_run_possible, "{}", report.reason);
    let parent = report.candidate_parent.clone().expect("candidate");
    assert_ne!(parent, "/system.slice");
    assert!(
        !parent.starts_with("/system.slice/"),
        "candidate must not be under system.slice: {parent}"
    );
    let delegated = fs::read_to_string(cg_file(&mount, &parent, "cgroup.subtree_control")).unwrap();
    let kind = fs::read_to_string(cg_file(&mount, &parent, "cgroup.type")).unwrap();
    assert_eq!(kind.trim(), "domain");
    assert_eq!(
        report.delegated_controllers,
        delegated
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
    assert!(has_all(&report.delegated_controllers));
    assert!(report.reason.contains(&parent));

    let prefix = "/user.slice/user-0.slice/";
    if let Some(rest) = report.self_path.strip_prefix(prefix) {
        if !rest.contains('/') && !has_all(&report.self_subtree_control) {
            assert_eq!(parent, "/user.slice/user-0.slice");
        }
    }
    eprintln!("{}", report.reason);
}
