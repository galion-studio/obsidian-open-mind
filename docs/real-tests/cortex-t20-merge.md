<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: cortex T2.0 merge (slot 104)

- Recorded: 2026-10-21 01:06 CEST (+02:00), branch `swarm/cortex-t20-merge`.
- Base `swarm/cortex-sensor` e536446, merged (no-ff, in this order): cortex-s2-a 9557eb3, cortex-s2-b 51100e1,
  cortex-s2-c dbec5c9, cortex-s2-d f3468ce. Conflicts: only `crates/openmind-nerve/src/sensors/mod.rs` (module list
  and `Sensors::from_env` lines), resolved as the union: modules alphabetical, `from_env` lines in merge order
  (sshd, rustsec, drift, conn-rate, proc-owner, surface, hades). Cargo.lock / Cargo.toml / lib.rs merged cleanly.
- T2.0 change: `collect.rs` reads the journal of the component units plus `JOURNAL_EXTRA_UNITS` = `ssh.service`
  (journal-only: no sample, no status, no journal counts), unit test `journal_units_add_ssh_once`.

## Gates (box, `cargo-slot ... -j 2`, own target dir)

| check | result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo test --workspace` | 215 suites, 1025 passed, 0 failed, 4 ignored |
| `cargo test -p openmind-nerve` | 141 passed, 0 failed |
| `scripts/no-python.sh` | ok |
| release musl builds of openmind-nerve | x86_64 1 434 400 B, aarch64 (rust-lld) 1 208 456 B |

## GALION_REAL=1 (release, `--test 'real_*' --test-threads=1`), all 7 ok

| test | result (excerpt) |
|---|---|
| real_box | ok (12.2 s) |
| real_cortex_sensor | built-ins unchanged; replay golden 21/F; live capture replay1 still 61/D |
| real_cortex_s2a | sshd flood + rustsec on :19550/:19552, cycle_ms 29, rss 4128 KiB |
| real_cortex_s2b | drift detect/clear + conn-rate 96.6/s crit on :19560, cycle_ms 25, rss 4052 KiB |
| real_cortex_s2_c | stray-owner, deleted-binary, surface findings on :19570, cycle_ms 28, rss 4136 KiB |
| real_cortex_s2_d | hades-gate ledger -> hades sensor -> `/api/v1/cortex/events`, cleared after window, rss 4096 kB |
| **real_cortex_t20** (new, :19740-19742) | all 12 sensors in one nerve, see below |

New `tests/real_cortex_t20.rs` output:

```
serve :19740 cycle 2: cycle_ms 25, rss 4160 KiB, sensors us [listeners=5 scanners=1 edge=0 components=0 ddos=20 sshd=1 rustsec=14 drift=32 conn-rate=1251 proc-owner=1361 surface=5 hades=2]
finding ids ["rustsec-smallvec-1.6.0", "drift-galion-desk", "rustsec-time-0.1.45", "drift-missing-room", "stray-owner", "unmanaged-listeners", "surface-http-status"]
feed: 5 B33 line(s), sources ["drift", "listeners", "proc-owner", "rustsec", "surface"]
```

All seven S2 sensors on together: cycle 25 ms (< 50 ms budget), RSS 4.1 MiB (< 8 MiB).

## Not covered on the box

- Live `ssh.service` lines: the box has no journal files (`journalctl -u ssh.service` -> "No journal files were
  found"), so the ssh collection path is covered by the unit test and the sshd replay fixture only. A read-only VPS
  line count (`journalctl -u ssh.service --since -5min | wc -l`) was blocked by the automatic safety review and not
  retried; worth doing once to confirm the VPS unit name is `ssh.service` (Debian/Ubuntu) and not `sshd.service`.
