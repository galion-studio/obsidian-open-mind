<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: xt-linker-pins-arm (slot 409, 2026-10-28 CET +01:00)

Follow-up to slot 386 (nerve-xt-rustlld-pin 9ebbed3a, which pinned rust-lld
for x86_64 musl). Question: do aarch64 musl, armv7 musl and android build from
the committed `.cargo/config.toml` with no linker env at all, and does
`real_cross_target` prove it?

Box: rustc 1.99.1 (58cf8f9c3 2026-10-13), qemu-aarch64/qemu-arm 10.0.13,
no Android NDK. Ports 22790-22794 (slot 409 block 22790-22799).

## What was wrong

`run_cargo_env` itself exported
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` (default) into every
cargo run, so the aarch64 build never depended on the committed config line,
and an outer `CARGO_TARGET_*_LINKER` / `RUSTFLAGS` for the other targets still
leaked through. armv7 musl was only `cargo check`ed (no link), and
`armv7-linux-androideabi` was not checked at all.

## Change

- `real_cross_target.rs`: every cargo run removes `CARGO_TARGET_<T>_LINKER`
  and `CARGO_TARGET_<T>_RUSTFLAGS` for all five triples plus `RUSTFLAGS`,
  `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS`, `CARGO_BUILD_TARGET`
  (`ambient_env_vars`); the live-pin `env` mode adds its one var back on
  purpose. The test asserts the COMMITTED (HEAD) config pins rust-lld for
  x86_64/aarch64/armv7 musl before building.
- armv7 musl: release build, ELF32 reader (e_machine 0x28, no PT_INTERP),
  2 MiB budget row `armv7`, `--version` and full smoke under qemu-arm
  (`NERVE_XT_QEMU_ARM`), parity with x86_64 (health/state keys, system
  verdict, /health wire order).
- android: `cargo check` for aarch64-linux-android AND armv7-linux-androideabi;
  report says link not attempted (D-409-1).
- New always-on unit tests: `ambient_env_covers_config_targets` (every
  `[target.*]` table in the config is stripped from env; musl pinned, android
  not), `elf_reader_32_and_64`, `live2_linker_split` (F-409-1 numbers).
- `.cargo/config.toml`: comment only (why android is not pinned). aarch64 and
  armv7 musl lines from slot 145 were already correct; no new pin needed.

## Manual probes (no linker env, committed config)

| time | target | result |
|---|---|---|
| 18:35-18:37 | aarch64-unknown-linux-musl | build ok 27 s, 1 591 776 B, qemu `--version` = `openmind-nerve 0.1.0` |
| 18:35-18:37 | armv7-unknown-linux-musleabihf | build ok 23 s, 1 649 220 B, qemu-arm `--version` = `openmind-nerve 0.1.0` |
| 18:37 | aarch64-linux-android | link FAILS with host `cc`: `Relocations in generic ELF (EM: 183)` |
| 18:37 | armv7-linux-androideabi | link FAILS the same way |
| 18:38 | aarch64-linux-android, linker=rust-lld | link FAILS: `unable to find library -lc / -ldl / -llog / -lm / -lunwind` (bionic, NDK only) |
| 18:46-18:48 | aarch64 musl, `--config linker="cc"` (negative) | FAILS: `unrecognized option '--fix-cortex-a53-843419'` -> the committed line is load-bearing |
| 18:46-18:48 | armv7 musl, `--config linker="cc"` (negative) | FAILS: `crt1.o: relocations in generic ELF (EM: 40)` -> load-bearing |

## real_cross_target runs (`GALION_REAL=1 NERVE_XT_PORT_BASE=22790`)

| start-end (+01:00) | mode | outer env | result |
|---|---|---|---|
| 18:41:31-18:43:32 | budget | clean | **pass** 1/1 (121.7 s) |
| 18:43:49-18:45:05 | live (9d1a4f3, committed config) | HOSTILE: all 3 musl `_LINKER=cc`, both android `_LINKER=/nonexistent`, `RUSTFLAGS=--cfg xt_hostile` | **pass** 1/1; every tree build was a cache hit (0.0-0.1 s) with the same SHAs as the clean run, i.e. none of the hostile vars reached cargo; live-pin d0c70b87... 1 458 760 B == VPS rollback binary |
| 18:45:05-18:45:26 | live, `NERVE_XT_LIVE_REV=9baa450c` (current VPS), committed config | clean | **fails as reported**: 0b8ed7c4... 1 473 704 B != VPS 8c9d7a5f... 1 479 456 B (F-409-1) |
| 18:45:45-18:46:16 | live, 9baa450c, `NERVE_XT_LIVE_CONFIG=archive` | clean | **pass**: rebuild == VPS 8c9d7a5f... 1 479 456 B |

Binaries (all runs identical):

| label | size B | sha256 | static | note |
|---|---|---|---|---|
| x86_64 | 1 923 432 | a58ecbb8... | yes (static-pie) | WARN > 90 % budget (pre-existing since slot 376, tree size) |
| x86_64-live-recipe | 1 923 432 | a58ecbb8... | yes | == solo |
| aarch64 | 1 591 776 | 6a245cb8... | yes | |
| armv7 | 1 649 220 | 9b2d87f6... | yes (ELF32) | new |

Smoke (each on its own port, killed and port checked closed after):
x86_64 :22790 0.10 s, aarch64 :22791 0.21 s, x86_64-live-recipe :22792
0.10 s, armv7 :22791 0.10-0.21 s; all `/health` 200 status ok, POST /signal
202, bad signal 400, xt-live healthy, xt-dead failing, signal seen, 24 state
keys, same wire order
`status, service, version, appended, ts, system, score, cycle` on all
three architectures. android checks: aarch64 7.2 s ok, armv7 5.3 s ok.

## VPS (read-only, 2026-10-28 18:39:22 +01:00)

`galion-nerve` active, NRestarts 0, started 17:40:13 +01:00, ExecStart
`/opt/galion/rust/bin/openmind-nerve-9baa450`, sha256
8c9d7a5fa4f84d6d56d6185dfd5b71171c8ba5b17bf49c0bde7fb5503f6593da,
1 479 456 B, x86_64; nerve `/health` 200 (status ok, system degraded,
score 68), `/state` 200; no VPS listener on 22790-22799. Nothing changed on
the VPS.

## Findings

- **F-409-1** The live nerve 9baa450c (deployed 17:40 CET) was linked by the
  host `cc` (GNU ld): its tree has no x86_64 linker line, and only the
  archive-config rebuild reproduces it. Rebuilt with the committed rust-lld
  line it is a different, 5 752 B smaller binary. Not a fault (the VPS binary
  is reproducible from git), but the D-376-2 goal "every x86_64 musl release
  linked by rust-lld" is not yet true for the live nerve.
- **F-409-2** Before this slot the test injected the aarch64 linker itself,
  so the aarch64 config line was never really tested; now it is (negative
  control above).
- **F-409-3** Android cannot be linked on the box: bionic needs the NDK.
  `cargo check` (the RULES.md requirement) passes for both android triples.
- **F-409-4** (pre-existing) tree x86_64 nerve 1 923 432 B is above the
  90 % warn line (headroom 173 720 B to 2 MiB).

## Owner decisions

- **D-409-1** Android linking: (A, recommended) keep android at `cargo
  check` and no pin until there is a phone build to ship; (B) install a pinned
  NDK on the box (licence = Android SDK License Agreement, needs Slavik's OK)
  and pin `linker = <ndk>/aarch64-linux-android<api>-clang`.
- **D-409-2** Next nerve deploy: (A, recommended) build it from a tree that
  contains the slot-386 x86_64 rust-lld line (land nerve-xt-rustlld-pin /
  this branch into the nerve deploy line), so live == committed-config
  rebuild; (B) keep the GNU ld recipe and pin with
  `NERVE_XT_LIVE_CONFIG=archive`.
- **D-409-3** Bump the default live pin from 9d1a4f3 (now the rollback
  binary) to 9baa450c? Only meaningful with `archive` config until D-409-2 A
  is done; left at 9d1a4f3 here, 9baa450c values are in `LIVE2_*`.

NO deploy, no VPS change.
