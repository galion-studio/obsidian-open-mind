<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: realscript-port-1 (wave 4, slot 112)

Task: port the first third (alphabetical) of the remaining `scripts/real/*.sh`
on swarm/integration-2 (df42e0f) to Rust `GALION_REAL=1` tests and delete the
ported shell scripts (Rust-only rule, 2026-10-20 20:41 CEST +02:00).

## Split (shared with slots 113 and 114)
26 scripts, `LC_ALL=C` sort, split 9 / 9 / 8. `common.sh` is the galion-common
real test (not a shared library), so it counts. `tuner-requests.txt` is data.

| Slot | Scripts |
|---|---|
| 112 (this) | common, desk-linkwatch, docs-sync, edge-boot, gaia-hades-tokens, gaia-live, gaia-poseidon, hades-approval, hermes-tools |
| 113 | integration-1, lan-discovery, lan-routing, launch-gates, memory-real, nerve-capture-vps, nerve, one-probe, poseidon |
| 114 | real-harness, room, sandbox, survival-heal, tuner-capture, tuner-cutover, ucpu, voice-mouth |

## What replaced what
All ports live in `crates/galion-realtest/tests/real_sh_<name>.rs` and share
`crates/galion-realtest/src/script.rs` (build, spawn with kill-on-drop, port
waits, PASS/FAIL recorder, Zagreb stamps). Each test builds the release
binaries it needs, starts them on loopback, probes them, and tears down (also
on panic). No new dependencies.

| Shell script (deleted) | Rust test | Checks (sh -> Rust) |
|---|---|---|
| common.sh | real_sh_common.rs | runs `galion-common/tests/real_hammer.rs` against release tuner + origin, RSS/threads sampler; `OLD=1` -> `GALION_REAL_OLD=1` |
| desk-linkwatch.sh | real_sh_desk_linkwatch.rs | route matrix, link.json, desk snapshot and acts, `real_wired` + `live_ports`, link-watch 60 s rate limit (+ ports closed, key not in logs) |
| docs-sync.sh | real_sh_docs_sync.rs | repo-check 0 warnings, spdx-scan, self-check, no-python, status-percent == STATUS.md, 21 branches listed |
| edge-boot.sh | real_sh_edge_boot.rs | 84 -> 92 (the 84 plus host-pid found and 7 port-closed checks) |
| gaia-hades-tokens.sh | real_sh_gaia_hades_tokens.rs | the 16 tick rows now asserted (the script only printed them), ledger, nerve, leak scan: 32 |
| gaia-live.sh (wrapper of the above) | real_sh_gaia_hades_tokens.rs | same |
| gaia-poseidon.sh | real_sh_gaia_poseidon.rs | 60 -> 60 |
| hades-approval.sh | real_sh_hades_approval.rs | 20 -> 20 |
| hermes-tools.sh | real_sh_hermes_tools.rs | 12 -> 14 (+ key-not-in-logs, listeners left; plus a unit test for the grep-equivalent parser) |

Ports: every test keeps its historic default block (19160, 19180, 19200, 19230,
19240, 19020, 19170) and takes `GALION_REAL_PORT_BASE` to move it; all offsets
are < 10. This run used the slot-112 block 19820-19829.
`edge/openmind-node/tests/real_edge_boot.rs` now also reads
`GALION_REAL_PORT_BASE` (+5/+6; default 19205/19206 as before).

Small behaviour notes (deliberate):
* edge-boot "wrong token" flips the last hex digit (the script appended `0`,
  which equals the real token 1 time in 16).
* hermes-tools reads `HOST_FILE` = the repo's `Cargo.toml` (outside the ticket
  worktree) instead of `/workspace/swarm/RULES.md`, so it runs on any box.
* Children that must not see provider keys get `env_clear()` (was `env -i`).
* `curl` and `jq` are no longer needed.

## Evidence run (box, 2026-10-21 02:26:42 to 02:27:45 CEST +02:00)
```
GALION_REAL=1 GALION_REAL_PORT_BASE=19820 cargo-slot test --no-fail-fast -j 2 -p galion-realtest \
  --test real_sh_common --test real_sh_desk_linkwatch --test real_sh_docs_sync --test real_sh_edge_boot \
  --test real_sh_gaia_hades_tokens --test real_sh_gaia_poseidon --test real_sh_hades_approval \
  --test real_sh_hermes_tools -- --nocapture --test-threads 1
```
| Port | Result |
|---|---|
| common | 3 passed, 0 failed (hammer: tuner + origin 2000/2000 sequential, 3200/3200 concurrent, malformed/oversized refused, both alive after slowloris; peak threads tuner 65, origin 33) |
| desk-linkwatch | 8 passed, 0 failed (`real_wired` 3/3, `live_ports` 1/1; example.com and one.galion.app fetched once, not re-fetched within 9 s) |
| docs-sync | 7 passed, **1 failed**: repo-check reports 29 warnings (see below) |
| edge-boot | 92 passed, 0 failed (x86_64 seccomp enforced, PID-1 namespace run, aarch64 under qemu refuses `--require-seccomp`, `real_edge_boot.rs` 1/1) |
| gaia-hades-tokens | 32 passed, 0 failed (all 16 tick rows identical to the 2026-10-20 shell evidence; 3 consumed nonces; no token or seed in any output) |
| gaia-poseidon | 60 passed, 0 failed |
| hades-approval | 20 passed, 0 failed |
| hermes-tools | 14 passed, 0 failed (7/7 sandbox escapes BLOCKED; spend caps stop at 4 mock calls) |

Total 236 PASS, 1 FAIL. No listener left on 19820-19829 afterwards. Raw
output: `docs/real-tests/captures/realscript-port-1-20261021.log`.

### The one failure is pre-existing
`repo-check` on unmodified integration-2 df42e0f prints
`0 error(s), 29 warning(s) -> PASS`; docs-sync requires 0 warnings, so
`scripts/real/docs-sync.sh` fails there too. The 29 are `utc` warnings
(`SystemTime::now()` outside zagreb-time, RFC 3339 `Z` literals) in
openmind-nerve, openmind-queue and room, brought in by the integration
merges. This branch adds no new warning (its two `SystemTime::now()` uses
carry the `repo-check: allow` marker). Fixing the 29 belongs to those crates'
owners, not to this port.

## Not run
* `GALION_REAL_OLD=1` for common (builds 13a296ce tuner + origin in a temp
  worktree for comparison); optional in the script too.
* `GALION_REAL_LONG=1` for desk-linkwatch (waits out the 60 s interval).
* No VPS probe: none of these nine scripts probed the VPS.
