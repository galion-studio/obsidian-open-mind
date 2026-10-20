// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Per-run cgroup v2 child: created in the parent before the fork, joined by
//! the child (`echo 0 > cgroup.procs`) before anything else, killed with
//! `cgroup.kill` and removed after the run. Only files inside the delegated
//! parent the caller named are touched.

use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::CgroupStats;

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) struct RunCgroup {
    pub(crate) dir: PathBuf,
    /// `<dir>/cgroup.procs`, prepared for the async-signal-safe join.
    pub(crate) procs: CString,
}

fn write(dir: &Path, name: &str, val: &str) -> Result<(), String> {
    fs::write(dir.join(name), val).map_err(|e| format!("write {name}={val}: {e}"))
}

impl RunCgroup {
    pub(crate) fn create(
        parent: &Path,
        pids_max: u64,
        mem_bytes: u64,
        cpu_pct: u32,
    ) -> Result<Self, String> {
        let name = format!(
            "om-sbx-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let dir = parent.join(name);
        fs::create_dir(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        let cg = RunCgroup {
            procs: CString::new(dir.join("cgroup.procs").as_os_str().as_bytes())
                .map_err(|_| "cgroup path contains NUL".to_string())?,
            dir,
        };
        if let Err(e) = cg.configure(pids_max, mem_bytes, cpu_pct) {
            let _ = fs::remove_dir(&cg.dir);
            return Err(e);
        }
        Ok(cg)
    }

    fn configure(&self, pids_max: u64, mem_bytes: u64, cpu_pct: u32) -> Result<(), String> {
        // The limit files only exist when the parent delegates the controller.
        for need in ["pids.max", "memory.max"] {
            if !self.dir.join(need).exists() {
                return Err(format!(
                    "{need} missing: the parent's cgroup.subtree_control does not enable that controller"
                ));
            }
        }
        write(&self.dir, "pids.max", &pids_max.max(1).to_string())?;
        write(&self.dir, "memory.max", &mem_bytes.max(1 << 20).to_string())?;
        // No swap escape hatch; absent when swap accounting is off (no swap).
        if self.dir.join("memory.swap.max").exists() {
            write(&self.dir, "memory.swap.max", "0")?;
        }
        if self.dir.join("memory.oom.group").exists() {
            write(&self.dir, "memory.oom.group", "1")?;
        }
        if cpu_pct > 0 {
            if !self.dir.join("cpu.max").exists() {
                return Err(
                    "cpu.max missing: the parent's cgroup.subtree_control does not enable the cpu controller"
                        .into(),
                );
            }
            // period 100 ms; quota = pct % of one CPU per period
            let quota = u64::from(cpu_pct).max(1) * 1000;
            write(&self.dir, "cpu.max", &format!("{quota} 100000"))?;
        }
        Ok(())
    }

    /// SIGKILL everything still inside, read the counters, remove the cgroup.
    pub(crate) fn kill(&self) {
        if write(&self.dir, "cgroup.kill", "1").is_err() {
            // Linux < 5.14: kill pid by pid until empty (pids are ours to see).
            for p in fs::read_to_string(self.dir.join("cgroup.procs"))
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|p| p.parse::<i32>().ok())
                .filter(|p| *p > 1)
            {
                // SAFETY: plain kill(2) of a pid listed in our per-run cgroup.
                unsafe { libc::kill(p, libc::SIGKILL) };
            }
        }
    }

    pub(crate) fn finish(&self) -> CgroupStats {
        self.kill();
        let t = Instant::now();
        while populated(&self.dir) && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
            self.kill();
        }
        let ev = |f: &str, k: &str| {
            fs::read_to_string(self.dir.join(f))
                .unwrap_or_default()
                .lines()
                .find_map(|l| {
                    let mut it = l.split_whitespace();
                    (it.next() == Some(k)).then(|| it.next()?.parse().ok())?
                })
                .unwrap_or(0)
        };
        let num = |f: &str| {
            fs::read_to_string(self.dir.join(f))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        let mut st = CgroupStats {
            path: self.dir.clone(),
            pids_max_hits: ev("pids.events", "max"),
            oom_kills: ev("memory.events", "oom_kill"),
            mem_max_hits: ev("memory.events", "max"),
            mem_peak: num("memory.peak"),
            pids_peak: num("pids.peak"),
            removed: false,
        };
        st.removed = fs::remove_dir(&self.dir).is_ok();
        st
    }
}

fn populated(dir: &Path) -> bool {
    fs::read_to_string(dir.join("cgroup.events"))
        .map(|s| s.lines().any(|l| l.trim() == "populated 1"))
        .unwrap_or(false)
}
