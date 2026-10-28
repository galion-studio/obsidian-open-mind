// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Process memory samples for real-life RSS budgets (slot 401, F-383-9).
//!
//! A VmRSS budget only means something for the profile it was set for. The
//! 8 MiB nerve budget is a RELEASE budget: on the same commit a debug
//! `openmind-nerve` (~52 MB file) reads 11.5 MiB VmRSS, of which ~7.3 MiB is
//! clean file-backed text pages, while release reads 4.6 MiB with ~1.1 MiB
//! anonymous. [`check_budget`] therefore holds VmRSS to the budget in release
//! builds and only the anonymous (heap + stack) part in debug builds, which
//! still catches real allocation regressions. std only, Linux `/proc`.

/// One `/proc/<pid>` memory sample, all values in KiB (0 = field missing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sample {
    pub rss: u64,
    pub hwm: u64,
    /// `Anonymous:` from smaps_rollup (heap, stacks, private dirty anon).
    pub anon: u64,
    /// `Private_Clean:` + `Shared_Clean:` (mapped binary/libc text, read-only data).
    pub file_clean: u64,
}

impl Sample {
    /// Read `/proc/<pid>/status` and `/proc/<pid>/smaps_rollup`.
    pub fn of(pid: u32) -> Sample {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        let rollup =
            std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).unwrap_or_default();
        Sample::parse(&status, &rollup)
    }

    /// Parse the two proc texts (pure, for tests).
    pub fn parse(status: &str, rollup: &str) -> Sample {
        Sample {
            rss: field(status, "VmRSS:"),
            hwm: field(status, "VmHWM:"),
            anon: field(rollup, "Anonymous:"),
            file_clean: field(rollup, "Private_Clean:") + field(rollup, "Shared_Clean:"),
        }
    }
}

impl std::fmt::Display for Sample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rss {} KiB (hwm {}, anon {}, file-backed clean {})",
            self.rss, self.hwm, self.anon, self.file_clean
        )
    }
}

fn field(text: &str, key: &str) -> u64 {
    text.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(0)
}

/// `true` when this crate (and so the test and its `CARGO_BIN_EXE_*`, which
/// share the profile) was built without debug assertions.
pub const RELEASE: bool = !cfg!(debug_assertions);

/// Hold `s` to `budget_kib`: VmRSS in release, anonymous memory in debug.
/// `Err` carries a message naming which figure broke the budget.
pub fn check_budget(s: Sample, budget_kib: u64, release: bool) -> Result<(), String> {
    if s.rss == 0 {
        return Err(format!("no VmRSS sample: {s}"));
    }
    if release {
        if s.rss < budget_kib {
            Ok(())
        } else {
            Err(format!("RSS budget {budget_kib} KiB (release): {s}"))
        }
    } else if s.anon > 0 && s.anon < budget_kib {
        Ok(())
    } else {
        Err(format!(
            "anon budget {budget_kib} KiB (debug build; VmRSS budget is release-only): {s}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str =
        "Name:\topenmind-nerve\nVmHWM:\t   11512 kB\nVmRSS:\t   11492 kB\nThreads:\t2\n";
    const ROLLUP: &str = "Rss:   11492 kB\nShared_Clean:   1848 kB\nPrivate_Clean:   7284 kB\nPrivate_Dirty:   2380 kB\nAnonymous:   2380 kB\n";

    #[test]
    fn parses_measured_debug_sample() {
        let s = Sample::parse(STATUS, ROLLUP);
        assert_eq!(
            s,
            Sample {
                rss: 11492,
                hwm: 11512,
                anon: 2380,
                file_clean: 9132
            }
        );
    }

    #[test]
    fn debug_build_holds_anon_only() {
        let s = Sample::parse(STATUS, ROLLUP);
        assert!(check_budget(s, 8 * 1024, false).is_ok());
        let e = check_budget(s, 8 * 1024, true).unwrap_err();
        assert!(e.contains("release"), "{e}");
    }

    #[test]
    fn anon_regression_fails_in_debug_too() {
        let s = Sample {
            rss: 20_000,
            hwm: 20_000,
            anon: 9_000,
            file_clean: 7_000,
        };
        assert!(check_budget(s, 8 * 1024, false)
            .unwrap_err()
            .contains("anon"));
    }

    #[test]
    fn missing_fields_never_pass() {
        let s = Sample::parse("", "");
        assert!(check_budget(s, 8 * 1024, true).is_err());
        assert!(check_budget(s, 8 * 1024, false).is_err());
        let no_rollup = Sample::parse(STATUS, "");
        assert!(check_budget(no_rollup, 8 * 1024, false).is_err());
    }

    #[test]
    fn samples_this_process() {
        let s = Sample::of(std::process::id());
        assert!(s.rss > 0 && s.hwm >= s.rss / 2, "{s}");
    }
}
