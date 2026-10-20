<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life flake hunt: swarm/flake-fix (2026-10-20, Europe/Zagreb +02:00)

Box: 8 cores, ~20 other swarm agents building/testing at the same time.
Base: swarm/sandbox d194c19 (on main 13a296c). Script: `scripts/real/flake-fix.sh`
(N sequential full `cargo test --workspace` runs, then R rounds of C copies at
once, each copy in its own `CARGO_TARGET_DIR`, all from one checkout).
No ports of the slot range (19270-19279) were needed: every listener in the
suite now binds `127.0.0.1:0`. No VPS probe: nothing here has a live twin.

## Before the fixes (20:16-20:22 +02:00)

* 10 sequential runs: 10/10 green, 500 passed, 1 ignored each.
* 3 rounds x 4 parallel copies: **11 of 12 runs failed** (1-4 failures each).
  A 4th parallel round after the first fixes still hit galion-one (3 of 4 runs).

| Test(s) | Symptom | Root cause | Fix |
| --- | --- | --- | --- |
| openmind-sandbox `group_kill_leaves_no_grandchild` | (reported in the task; collides whenever two runs overlap) "grandchild still alive" | scanned the whole host for `sleep 3171`, so any concurrent run's grandchild matched; plus a fixed 300 ms sleep before looking | marker `sleep 3171<test pid>` (unique among live processes, so a match can only be our own grandchild, even after it was reparented out of our tree); bounded poll for "gone" instead of a fixed delay; stat state parsed after the last `)` and Z/X both count as dead; workdir moved to `CARGO_TARGET_TMPDIR` |
| galion-boot `secrets_restore_rules`, `tunnel_env_file`, `hardware_profiles`, `supervisor_restarts_and_requests` | "age could not decrypt", ENOENT, wrong install results | every copy `rm -rf`'d and rebuilt the same `<checkout>/target/test-out/galion-boot/<name>` | `CARGO_TARGET_TMPDIR/test-out/...` (per target dir) |
| galion-desk `health_desk_and_note`, galion-origin `routes`, link-watch `binary_loads_config_fixture_for_probes_and_output_path` | ENOENT / wrong file | same shared source-tree `target/test-out` | `CARGO_TARGET_TMPDIR` |
| guardian-tick `default_run_writes_no_jsonl`, `jsonl_flag_writes_one_line_per_probe`, `tick_result_is_report_only` | ENOENT, 6 JSONL lines instead of 3 | one fixed `/tmp/guardian-tick-tests` for every checkout and agent on the box, subdirs `rm -rf`'d | `CARGO_TARGET_TMPDIR/guardian-tick-tests` |
| guardian-tick `mock_listener_ok_and_down`, openmind-survival `http_probe_mock` | (latent; 18080 is also a "spare" port other agents use for real tests) | fixed `127.0.0.1:18080`; "closed" probe assumed 18646 is free | ephemeral `127.0.0.1:0`; the closed port is one we bound and released |
| one-talk `spare_listener_shapes` | `127.0.0.1:18002 ... Address already in use` | fixed port 18002; and its drop-time `POST /stop` could stop another run's listener | ephemeral port, asserting it is not one of the refused 8002/8642/8643 |
| openmind-survival `probe_path_does_not_spawn_systemctl_or_restart_a_service` | `sentinel missing: ["1398787 "]` (empty cmdline) | found the sentinel child by `cmdline` containing "sleep"; `spawn()` (vfork/posix_spawn) returns once the child released the parent's mm in exec, before the new argv is mapped, so cmdline can read empty under load | identify the sentinel by its pid |
| galion-one `set_model_like_python`, `set_model_adds_modes_and_rejects_bad_input`, `fetch_error_keeps_old_file` | rename ENOENT, empty file | unit tests in `src/` wrote/renamed fixed files in the shared `<checkout>/target/test-out` | `<test binary>/../../test-out` (per target dir; `CARGO_TARGET_TMPDIR` is not set for unit tests) |
| link-watch (src unit tests), openmind-schema (src), openmind-config (tests) | (latent, same pattern) | shared source-tree `target/test-out` | same per-target-dir fix |

Also fixed (not a flake): `openmind-sandbox/examples/escape.rs` failed
`cargo check --target aarch64-linux-android` because the libc crate has no
`PTRACE_SEIZE` for Android (rustc suggested PTRACE_KILL). It is now
`use libc::PTRACE_SEIZE` off Android and the kernel value 0x4206 on Android,
and the call passes real null pointers for addr/data through the variadic
`ptrace`. aarch64-linux-android and armv7-unknown-linux-musleabihf
`cargo check --all-targets` pass for every touched crate.

No retries and no new sleeps were added. The only waits are bounded polls for
an end state (a SIGKILLed process disappearing), which replace fixed delays.

## After the fixes (20:35-20:37 +02:00)

```
== 10 sequential runs ==
2026-10-20T20:35:42+02:00 seq-1 rc=0 passed=500 failed=0
... (seq-2 .. seq-9 identical)
2026-10-20T20:36:18+02:00 seq-10 rc=0 passed=500 failed=0
== 6 rounds x 4 parallel copies (own target dirs) ==
2026-10-20T20:36:23+02:00 par-r1-c1 rc=0 passed=500 failed=0
... (all 24 runs rc=0 passed=500 failed=0)
2026-10-20T20:36:47+02:00 par-r6-c3 rc=0 passed=500 failed=0
== failing runs: 0 ==
```

An earlier 3x4 round after the first batch of fixes was also 12/12 green
(20:27 +02:00), before the galion-one flake showed up in the next round.

## real_escape (adversarial sandbox suite) with the new markers, 20:38 +02:00

`scripts/real/sandbox.sh` (unprivileged uid 1000, delegated cgroup v2):

```
--- ["seebystander", "sleep 301583513 "] -> Exited(0)
stdout: RESULT seebystander CONTAINED bystander not visible in /proc
--- ["setsid-orphan", "31331583513"] -> Exited(0)
stdout: RESULT setsid-orphan CONTAINED spawned detached sleeper (pid in ns 2)
RESULT forkbomb CONTAINED stopped after 22 forks: Resource temporarily unavailable (os error 11)
test result: ok. 1 passed; 0 failed; 0 ignored
```

Every RESULT line said CONTAINED (13 attacks; the membomb run was killed before it could print); the cgroup was removed afterwards and no
`sleep 3133*` / `sleep 3171*` / `sleep 30<pid>` process was left.

## Known, not changed

* `real_escape` still uses `<checkout>/target/test-out/escape` because
  `scripts/real/sandbox.sh` runs it as another uid that may not be able to
  write the cargo target dir; it is a one-at-a-time `GALION_REAL=1` test.
* galion-desk's unit test already uses `$CARGO_TARGET_DIR/test-out` at run time.
* Concurrent runs that share ONE target dir still share its test-out; the
  swarm rule (own `CARGO_TARGET_DIR` per agent) covers that.
