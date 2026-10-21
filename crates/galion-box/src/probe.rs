// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Read-only capability probe. Creates no namespaces, writes nothing: safe on any host,
//! including the dev box (sandbox_vps_only) and a phone.

use std::fs;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Probe {
    pub arch: String,
    pub euid: u32,
    pub android: bool,
    pub max_user_namespaces: Option<u64>,
    pub unprivileged_userns_clone: Option<u64>,
    pub apparmor_restrict_userns: Option<u64>,
    pub yama_ptrace_scope: Option<u64>,
    pub self_seccomp: Option<u64>,
    pub self_no_new_privs: Option<u64>,
    pub landlock_present: bool,
    /// Threads of this process at probe time. A native galion-box is single-threaded here;
    /// 2+ means an emulator (qemu-user adds an RCU thread), which cannot create namespaces.
    pub threads: Option<u64>,
}

/// Which launcher mode this host can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Rootless user namespaces look available: `galion-box run` should work.
    Userns,
    /// User namespaces are disabled (kernel or sysctl): needs root mode or a proot-style tracer.
    NoUserns,
    /// Already root: refused by this prototype (rootless only).
    RootRefused,
    /// Multi-threaded at start: most likely qemu-user, which rejects namespace clones.
    Emulated,
}

fn read_u64(path: &str) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Parse one numeric field ("Seccomp:\t2") out of /proc/self/status text.
pub fn status_field(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| v.trim().parse().ok()).flatten()
    })
}

impl Probe {
    pub fn collect() -> Probe {
        let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
        Probe {
            arch: std::env::consts::ARCH.to_string(),
            // SAFETY: geteuid has no preconditions.
            euid: unsafe { libc::geteuid() },
            android: std::path::Path::new("/system/build.prop").exists()
                || cfg!(target_os = "android"),
            max_user_namespaces: read_u64("/proc/sys/user/max_user_namespaces"),
            unprivileged_userns_clone: read_u64("/proc/sys/kernel/unprivileged_userns_clone"),
            apparmor_restrict_userns: read_u64(
                "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
            ),
            yama_ptrace_scope: read_u64("/proc/sys/kernel/yama/ptrace_scope"),
            self_seccomp: status_field(&status, "Seccomp"),
            self_no_new_privs: status_field(&status, "NoNewPrivs"),
            threads: status_field(&status, "Threads"),
            landlock_present: fs::read_to_string("/sys/kernel/security/lsm")
                .map(|s| s.contains("landlock"))
                .unwrap_or(false),
        }
    }

    pub fn verdict(&self) -> Verdict {
        if self.euid == 0 {
            return Verdict::RootRefused;
        }
        if self.threads.is_some_and(|t| t > 1) {
            return Verdict::Emulated;
        }
        let ns_off = self.max_user_namespaces == Some(0)
            || self.unprivileged_userns_clone == Some(0)
            || self.apparmor_restrict_userns == Some(1)
            // No /proc/sys/user at all usually means CONFIG_USER_NS=n (common on Android kernels).
            || self.max_user_namespaces.is_none();
        if ns_off {
            Verdict::NoUserns
        } else {
            Verdict::Userns
        }
    }

    pub fn render(&self) -> String {
        fn o(v: Option<u64>) -> String {
            v.map(|x| x.to_string()).unwrap_or_else(|| "absent".into())
        }
        let verdict = match self.verdict() {
            Verdict::Userns => "userns (rootless run should work)",
            Verdict::NoUserns => "no-userns (needs root mode or a proot-style tracer; see docs/sandbox/galion-box.md)",
            Verdict::RootRefused => "root (prototype refuses to run as root)",
            Verdict::Emulated => "emulated (multi-threaded at start, likely qemu-user: namespaces unavailable, run fails closed)",
        };
        format!(
            "arch={}\neuid={}\nandroid={}\nmax_user_namespaces={}\nunprivileged_userns_clone={}\napparmor_restrict_unprivileged_userns={}\nyama_ptrace_scope={}\nself_seccomp={}\nself_no_new_privs={}\nlandlock={}\nthreads={}\nverdict={}\n",
            self.arch,
            self.euid,
            self.android,
            o(self.max_user_namespaces),
            o(self.unprivileged_userns_clone),
            o(self.apparmor_restrict_userns),
            o(self.yama_ptrace_scope),
            o(self.self_seccomp),
            o(self.self_no_new_privs),
            self.landlock_present,
            o(self.threads),
            verdict
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parsing() {
        let s = "Name:\tx\nNoNewPrivs:\t1\nSeccomp:\t2\nSeccomp_filters:\t1\n";
        assert_eq!(status_field(s, "Seccomp"), Some(2));
        assert_eq!(status_field(s, "NoNewPrivs"), Some(1));
        assert_eq!(status_field(s, "Missing"), None);
    }

    #[test]
    fn verdicts() {
        let mut p = Probe {
            euid: 1000,
            max_user_namespaces: Some(63000),
            ..Default::default()
        };
        assert_eq!(p.verdict(), Verdict::Userns);
        p.apparmor_restrict_userns = Some(1);
        assert_eq!(p.verdict(), Verdict::NoUserns);
        p.apparmor_restrict_userns = Some(0);
        p.unprivileged_userns_clone = Some(0);
        assert_eq!(p.verdict(), Verdict::NoUserns);
        p.unprivileged_userns_clone = None;
        p.max_user_namespaces = None;
        assert_eq!(p.verdict(), Verdict::NoUserns);
        p.max_user_namespaces = Some(10);
        p.threads = Some(2);
        assert_eq!(p.verdict(), Verdict::Emulated);
        p.euid = 0;
        assert_eq!(p.verdict(), Verdict::RootRefused);
    }

    #[test]
    fn collect_is_read_only_and_renders() {
        let r = Probe::collect().render();
        assert!(r.contains("arch="));
        assert!(r.contains("verdict="));
    }
}
