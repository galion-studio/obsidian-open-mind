<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: hermes-rs-crashloop-r3 (wave-4 slot 355)

All times Europe/Zagreb CET (+01:00). No secrets in this file. Ports 22250-22259.

## Box: GALION_REAL=1 release test, 2026-10-28 04:13:58-04:14:26 CET

`GALION_REAL=1 HERMES_GW_BASE_BIN=<6845c350 build> cargo-slot test -j 2 --release -p hermes-gateway --test real_crashloop_r3 -- --nocapture`
-> `test result: ok. 1 passed; 0 failed` (0.30 s). New binary x86_64 musl sha256 c3b9475ca12bcd16..., base 7a0982ea21c914e7....

| case | result |
|---|---|
| R1 start with worktree `/var/lib/dpkg` (live shape) | exit 2 in 20 ms, `coding: config error: worktree /var/lib/dpkg overlaps the denied path /var/lib (pick a dedicated path ... ReadWritePaths ...)`, port 22250 never bound |
| R1 3 more starts (what systemd did 13x) | exits [2, 2, 2] |
| R2 `--check-config`, same env, fixed binary | exit 2, `coding=error` + the same reason |
| R2-before, base 6845c350 binary | exit 0, no coding line (the blind spot) |
| R3 `--check-config`, state under a 0500 dir (the /srv retry shape, non-root analogue of EROFS) | exit 2, `... is not writable: Permission denied (os error 13) (is the path inside the unit's ReadWritePaths ...)` for stage, target, audit dir |
| R4 `--check-config`, good worktree + writable state | exit 0, `coding=on worktree=... branch=hermes/ticket state=... on_error=exit`; state dir NOT created |
| R4 `--check-config`, no coding env | exit 0, `coding=off` |
| R5 bad config + `HERMES_RS_CODING_ON_ERROR=disable`, `--mock` on 22251 | up in 150 ms, /health 200, /v1/models 200, log `... HERMES_RS_CODING_ON_ERROR=disable: coding tools OFF, gateway continues` then `listening on 127.0.0.1:22251` |

No sandbox run and no namespace probe in this test (every case stops before the coding preflight).

## VPS: throwaway as nobody (uid 65534), 2026-10-28 04:15:18-04:15:19 CET

`/var/tmp/hermes-355-20261028041446` (root 0755, run dir nobody 0700), binaries copied in,
loopback ports 22250-22252, `--mock`, throwaway API key, then removed.

| case | result |
|---|---|
| `--check-config` with `/var/lib/dpkg` worktree: new build | exit 2, coding line printed, `overlaps the denied path /var/lib` |
| same: base 6845c350 build | exit 0, no coding line |
| same: **live** `/opt/galion/rust/bin/hermes-gateway-b7fbbc78` (run in place as nobody, read-only use) | exit 0, no coding line: the live build cannot pre-check a coding drop-in |
| start x3, new build | exit 2 each, with hint |
| start, base build | exit 2 (same as the live 19:45 journal) |
| `ON_ERROR=disable`, new build, 22251 | /health 200, /v1/models 200, `coding tools OFF, gateway continues`, `listening on 127.0.0.1:22251` |
| live hermes-rs after | MainPID 255539, NRestarts 0, active, :8642 /health 200 (unchanged) |

Cleanup note: the `ON_ERROR=disable` gateway was started via `runuser`; killing the
runuser parent left the nobody child listening on 127.0.0.1:22251 for about a minute.
Found by the post-run `ss` check, killed (pid checked against the throwaway exe path) at
about 04:16:30 CET; afterwards 0 listeners on 22250-22259, 0 nobody processes from the
folder, folder gone.

## Live observation (read-only)

- Journal `journalctl -u hermes-rs` 2026-10-27 19:20-19:50 CET: 13 exit-2 starts
  19:45:27-19:46:10, EROFS exit 2 at 19:46:15, healthy from 19:46:18 (table in
  docs/ops/hermes-rs-crashloop-r3.md).
- Nerve `GET 127.0.0.1:8670/state` 2026-10-28 04:15:41 CET: hermes-rs healthy since
  2026-10-27 21:00:01 CET, http 200, restarts_1h 0, n_restarts_systemd 0, pid 255539,
  rss 2292 kB. Cortex live (73583ab-llm, per slot 341); its 19:45 decisions are in
  /workspace/swarm/LIVE-FIX-WATCH.md (dec-35 R3 restart, dec-40 R2 breaker).

## Gates (box)

- `cargo fmt --all --check` ok; `cargo clippy -p hermes-tools -p hermes-gateway --all-targets -D warnings` ok;
  also `-p hermes-gateway --no-default-features --features sqlite` ok.
- `cargo test -p hermes-tools --lib --test guard --test staged_dropin --test real_ticket`: 3 + 6 + 4 + 1 (skip-gated) passed, 0 failed.
- `cargo test -p hermes-gateway -p hermes-rs`: 83 passed, 0 failed.
- aarch64 musl release build ok (sha256 78c2b82e0f924bba...); armv7 musl check ok;
  aarch64-android check ok for hermes-tools and hermes-gateway without sqlite
  (with sqlite it fails in libsqlite3-sys C build: existing cross-CC gap, not this change).
- `scripts/no-python.sh`: ok.
- Note: one early `cargo test -p hermes-tools` (04:09 CET) also ran `tests/sandboxed.rs`,
  which on this base auto-runs when the box has Landlock (no GALION_REAL gate; slot 346
  fixes that gate on another branch). Later runs excluded it.
