<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# CI realtest with the nerve (slot 213, Integration S4-T1)

Base: `integration-5` 0d68f46 -> branch `swarm/ci-realtest-with-nerve`.
Recorded 2026-10-21 (Europe/Zagreb, +02:00). Box runs only; the VPS was not touched.

## Change

- `.github/workflows/realtest.yml`: the release build adds `-p openmind-nerve`; the
  scenario step runs `galion-realtest stack --with-nerve --bin-dir target/release
  --out target/realtest/ci`. All `uses:` stay pinned to full 40-hex commit SHAs
  (checkout 11d5960a, rust-toolchain b725eda5, upload-artifact ea165f8d), unchanged.
- `galion-realtest`: `--bin-dir` and `--out` are made absolute (`std::path::absolute`).
  **Pre-existing bug found:** the workflow's own command (`--bin-dir target/release`,
  relative) made galion-boot refuse every service (`argv[0] must be an absolute path`):
  77 checks, 2 pass, 74 FAIL at base 0d68f46. So `realtest.yml` could not have passed
  as written, with or without the nerve. New unit test `relative_bin_dir_becomes_absolute`.
- `tests/real_stack.rs` (GALION_REAL=1): mirrors CI - passes `--with-nerve` when an
  `openmind-nerve` release binary is present and asserts the `nerve-ingest` suite ran
  (has `ingest-signal-202`, no `<skipped>`); `REALTEST_BASE_PORT` overrides the base port.

## Box proof (same command as CI)

Build: `cargo build --release --locked -p galion-realtest -p galion-boot -p galion-one
-p openmind-memory -p tuner -p galion-desk -p link-watch -p guardian-tick
-p persona-plex-relay -p openmind-nerve` - ok (`--locked`, Cargo.lock unchanged).

| Run (cwd = repo, `target/` = slot target dir) | Ports | Checks | Pass | FAIL | Skip | Exit |
|---|---|---|---|---|---|---|
| old CI command at base, before fix (relative bin-dir) | 19500-19551 | 77 | 2 | 74 | 1 | 1 |
| before: `stack` without `--with-nerve` (after fix) | 19500-19551 | 77 | 76 | 0 | 1 (nerve-ingest) | 0 |
| after: `stack --with-nerve` (new CI command) | 19500-19551 | 81 | 81 | 0 | 0 | 0 |
| `--base-port 20830`, without / with nerve | 20830-20881 | 77 / 81 | 76 / 81 | 0 / 0 | 1 / 0 | 0 / 0 |
| `GALION_REAL=1 cargo test -p galion-realtest --test real_stack` x3 (`REALTEST_BASE_PORT=20830`) | 20830-20881 | 81 each | 81 | 0 | 0 | 1/1 ok each, ~11.5 s |

Scenario count: 13 suites both ways; checks 77 -> 81 (+4 nerve-ingest checks
`healthz` 200, `ingest-signal-202` `{"ok":true}`, `signal-visible` 0 ms, `bad-signal-400`;
+1 `ready:nerve` healthy ~158 ms after supervisor start; -1 skip).

Nerve under realtest is sandboxed by env: `NERVE_GAIA_BIN`/`NERVE_DEPLOY_BIN=/bin/false`,
fixture data/switch/backup dirs, no telegram env, unit `realtest-none.service`.

## Gates

- `cargo fmt --all --check` ok; `cargo clippy -p galion-realtest -p openmind-nerve --all-targets -D warnings` ok.
- `cargo test -p galion-realtest` 13/0; `cargo test -p openmind-nerve` 88/0.
- repo-check: 10 errors / 48 warnings, identical to base 0d68f46 (all 10 = vendored
  candle-core `portable`, main-merge blocker 0 from slot 177); none in changed files.

## Owner decisions

- D-213-1: the slot port range 20830-20839 cannot hold the stack (base..base+51);
  box runs used 19500-19551 (CI default, free) and 20830-20881 (checked free first).
- D-213-2: accept the bin-dir/out absolutize fix in galion-realtest (needed for the
  workflow to pass at all), or change the workflow to pass `$GITHUB_WORKSPACE/target/release`.
- D-213-3: CI not yet observed on GitHub Actions (no PRs per swarm rules); first real
  Actions run happens when Slavik opens a PR / merges.
