// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! GAIA core, dry-run only.
//!
//! [`status_line`] reports idle with a Europe/Zagreb timestamp. [`tick`] is one
//! pure step and does not loop. [`simulate_faro`] returns text and does not
//! change power, touch GPIO, or use the network. [`OverrideRefusal`] is the
//! master-override gate: it is closed unless `GAIA_OVERRIDE=1`, and even then
//! the only result is "refused: not approved". It never restarts a service.

#![deny(unsafe_code)]

/// Idle core. One [`tick`] moves `step` by exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Core {
    pub step: u64,
}

impl Core {
    /// The starting core. Nothing is running.
    pub const IDLE: Self = Self { step: 0 };
}

impl Default for Core {
    fn default() -> Self {
        Self::IDLE
    }
}

/// One pure step. No loop, no I/O, no power change, no GPIO, no network.
#[must_use]
pub fn tick(core: Core) -> Core {
    Core {
        step: core.step.saturating_add(1),
    }
}

/// Europe/Zagreb RFC 3339, explicit offset, never `Z`.
#[must_use]
pub fn now_zagreb() -> String {
    zagreb_time::now_rfc3339()
}

/// A Zagreb timestamp and the word `idle`.
#[must_use]
pub fn status_line(now_zagreb: &str) -> String {
    format!("{now_zagreb} idle")
}

/// Dry-run text for the Faro scenario.
///
/// `full_power` is only a note. It is never applied. This function does not
/// change power, touch GPIO, open a socket, or restart a service.
#[must_use]
pub fn simulate_faro(full_power: bool) -> String {
    let request = if full_power {
        "full-power: ignored, no power change"
    } else {
        "full-power: not requested"
    };
    format!("simulate-faro: dry-run\npower: unchanged\ngpio: untouched\nnetwork: none\n{request}\n")
}

/// Why a master override was refused.
///
/// There is no success variant. The override never restarts a service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverrideRefusal {
    /// `GAIA_OVERRIDE` was not exactly `1`. The flag is refused.
    EnvClosed,
    /// The env gate was open. The override is still not approved.
    NotApproved,
}

impl OverrideRefusal {
    /// Process exit code for every refusal.
    pub const EXIT_CODE: u8 = 2;

    /// Exact line printed when the env gate is open.
    pub const NOT_APPROVED_LINE: &'static str = "refused: not approved";

    /// Gate on the `GAIA_OVERRIDE` value. Only the exact string `1` opens it.
    #[must_use]
    pub fn from_env(value: Option<&str>) -> Self {
        if value == Some("1") {
            Self::NotApproved
        } else {
            Self::EnvClosed
        }
    }

    /// Text the process prints. No service is named and none is restarted.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::EnvClosed => "refused: GAIA_OVERRIDE is not 1",
            Self::NotApproved => Self::NOT_APPROVED_LINE,
        }
    }
}

/// One invocation of the `gaia` binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn ok(stdout: String) -> Self {
        Self {
            code: 0,
            stdout,
            stderr: String::new(),
        }
    }

    fn err(stderr: String) -> Self {
        Self {
            code: 2,
            stdout: String::new(),
            stderr,
        }
    }

    fn refused(refusal: OverrideRefusal) -> Self {
        Self {
            code: OverrideRefusal::EXIT_CODE,
            stdout: format!("{}\n", refusal.message()),
            stderr: String::new(),
        }
    }
}

const USAGE: &str = "\
usage: gaia <command>
  status                 print a Europe/Zagreb timestamp and idle
  tick                   one pure step, no loop
  simulate-faro          dry-run text only; --full-power is not applied
  --master-override      refused unless GAIA_OVERRIDE=1; even then prints
                         \"refused: not approved\" and exits 2
";

fn is_master_override(arg: &str) -> bool {
    arg == "--master-override" || arg == "master-override"
}

/// Run one invocation.
///
/// `now_zagreb` is an already formatted Europe/Zagreb stamp used only by
/// `status`. `override_env` is the value of `GAIA_OVERRIDE`, if it is set.
/// A master-override flag is never applied: the command does not run.
#[must_use]
pub fn run(args: &[&str], override_env: Option<&str>, now_zagreb: &str) -> Outcome {
    if args.iter().any(|arg| is_master_override(arg)) {
        return Outcome::refused(OverrideRefusal::from_env(override_env));
    }
    match args {
        ["-h" | "--help"] => Outcome::ok(USAGE.to_string()),
        ["status"] => Outcome::ok(format!("{}\n", status_line(now_zagreb))),
        ["tick"] => {
            let next = tick(Core::IDLE);
            Outcome::ok(format!("tick step={}\n", next.step))
        }
        ["simulate-faro"] => Outcome::ok(simulate_faro(false)),
        ["simulate-faro", "--full-power"] => Outcome::ok(simulate_faro(true)),
        ["simulate-faro", ..] => {
            Outcome::err("gaia: simulate-faro accepts only optional --full-power\n".to_string())
        }
        [other, ..] => Outcome::err(format!("gaia: unknown command {other}\n{USAGE}")),
        [] => Outcome::err(format!("gaia: missing command\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: &str = "2026-10-19T16:39:32+02:00";

    #[test]
    fn tick_is_one_pure_step_from_idle() {
        let once = tick(Core::IDLE);
        assert_eq!(once, Core { step: 1 });
        assert_eq!(tick(once).step, 2);
        let out = run(&["tick"], None, STAMP);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, "tick step=1\n");
        assert_eq!(out.stdout.lines().count(), 1);
    }

    #[test]
    fn tick_rejects_a_repeat_count() {
        let out = run(&["tick", "--n", "10"], None, STAMP);
        assert_eq!(out.code, 2);
        assert!(!out.stdout.contains("step="));
        assert!(out.stderr.contains("unknown command"));
    }

    #[test]
    fn status_line_is_zagreb_stamp_and_idle() {
        assert_eq!(status_line(STAMP), "2026-10-19T16:39:32+02:00 idle");
        let out = run(&["status"], None, STAMP);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, "2026-10-19T16:39:32+02:00 idle\n");
        assert!(out.stdout.contains("idle"));
        assert!(!out.stdout.contains('Z'));
        assert!(!out.stdout.contains("+00:00"));
    }

    #[test]
    fn simulate_faro_is_dry_run_text_with_no_power_change() {
        let dry = simulate_faro(false);
        let full = simulate_faro(true);
        for text in [&dry, &full] {
            assert!(text.contains("simulate-faro: dry-run"));
            assert!(text.contains("power: unchanged"));
            assert!(text.contains("gpio: untouched"));
            assert!(text.contains("network: none"));
            assert!(!text.contains("systemctl"));
        }
        assert!(full.contains("no power change"));
        assert_eq!(run(&["simulate-faro"], None, STAMP).stdout, dry);
        assert_eq!(
            run(&["simulate-faro", "--full-power"], None, STAMP).stdout,
            full
        );
        let bad = run(&["simulate-faro", "--apply"], None, STAMP);
        assert_eq!(bad.code, 2);
        assert!(bad.stdout.is_empty());
    }

    #[test]
    fn master_override_is_refused_even_when_env_is_one() {
        assert_eq!(OverrideRefusal::from_env(None), OverrideRefusal::EnvClosed);
        assert_eq!(
            OverrideRefusal::from_env(Some("0")).message(),
            "refused: GAIA_OVERRIDE is not 1"
        );
        let open = OverrideRefusal::from_env(Some("1"));
        assert_eq!(open, OverrideRefusal::NotApproved);
        assert_eq!(open.message(), "refused: not approved");
        assert_eq!(OverrideRefusal::EXIT_CODE, 2);

        let closed = run(&["--master-override"], None, STAMP);
        assert_eq!(closed.code, 2);
        assert_eq!(closed.stdout, "refused: GAIA_OVERRIDE is not 1\n");
        assert!(!closed.stdout.contains("not approved"));

        let approved_env = run(&["status", "--master-override"], Some("1"), STAMP);
        assert_eq!(approved_env.code, 2);
        assert_eq!(approved_env.stdout, "refused: not approved\n");
        assert!(approved_env.stderr.is_empty());
        assert!(!approved_env.stdout.contains("idle"));

        let as_command = run(&["master-override"], Some("1"), STAMP);
        assert_eq!(as_command.code, 2);
        assert_eq!(as_command.stdout, "refused: not approved\n");
    }

    #[test]
    fn now_zagreb_has_an_explicit_offset() {
        let now = now_zagreb();
        assert_eq!(now.len(), 25, "{now}");
        assert!(!now.ends_with('Z'), "{now}");
        assert!(!now.contains("+00:00"), "{now}");
        assert!(now.ends_with("+01:00") || now.ends_with("+02:00"), "{now}");
    }
}
