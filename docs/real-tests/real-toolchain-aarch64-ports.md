<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: real-toolchain-aarch64-ports (slot 354), 2026-10-28 CET (+01:00)

Base: swarm/cross-cc-restore 0dfc3712 (slot 338). Box x86_64, slot ports 22240-22249.
Tooling: slot 338 wrappers in `/workspace/swarm/bin` (zig 0.15.2 `aarch64-linux-musl-gcc`,
`arm-linux-musleabihf-gcc`, `qemu-aarch64-static`, `qemu-arm-static`) on PATH. No `CC_*`,
`CARGO_TARGET_*_LINKER` or `RUSTFLAGS` in the environment (`env | grep` count 0).
All cargo runs via `cargo-slot ... -j 2`, own `CARGO_TARGET_DIR`.

## Why
Slot 338 could not re-run `real_toolchain_aarch64`: ports 20150-20153 were hard-coded and lie
outside every Wave-4 slot range. `real_sh_ucpu` (aarch64 + armv7 SQLite under qemu) had the
same problem (19845-19849). Its SQLite part also only ran with `CC_aarch64_unknown_linux_musl`
set, so with the new PATH wrappers it silently skipped SQLite.

## Change
* `crates/galion-realtest/src/ports.rs`: `parse_base` / `base_from_env` (unset = old default;
  bad value = panic naming the var). 3 unit tests.
* `real_toolchain_aarch64.rs`: `REAL_TOOLCHAIN_AARCH64_PORT_BASE` (default 20150, 4 ports);
  C cross cc detected from env OR `aarch64-linux-musl-gcc` on PATH;
  `REAL_TOOLCHAIN_AARCH64_REQUIRE_C=1`; pre-run "port already in use" guard; post-stop
  "all ports closed" check.
* `real_sh_ucpu.rs`: `REAL_UCPU_PORT_BASE` (default 19845, 5 ports); it already checked
  ports free before and closed after.

## Runs
| time (+01:00) | command / case | result |
|---|---|---|
| 04:06 | `cargo test -p galion-realtest --lib ports` | 3 passed, 0 failed |
| 04:06:24-04:06:52 | `GALION_REAL=1 REAL_TOOLCHAIN_AARCH64_PORT_BASE=22240 REAL_TOOLCHAIN_AARCH64_REQUIRE_C=1 cargo test --test real_toolchain_aarch64` | **2 passed**. C cc = `/workspace/swarm/bin/aarch64-linux-musl-gcc`; build tuner + galion-one + openmind-memory aarch64 21.9 s; all e_machine 183, no PT_INTERP/PT_DYNAMIC; under qemu: `GET :22240/health` 200 `{"ok":true}`, `GET :22240/` 200 index, `GET :22241/health` 200 `ok`, `GET :22243/health` 200 `{"status":"ok",...}`, `POST :22243/capture` 200, `POST :22243/search/conversations` 200 hit=true (**aarch64 SQLite write + read-back, previously skipped**); all 4 ports closed after stop |
| 04:07 | negative: PATH without `/workspace/swarm/bin`, `REQUIRE_C=1` | FAILED as intended: `REAL_TOOLCHAIN_AARCH64_REQUIRE_C=1 but no aarch64 C cross compiler` |
| 04:07 | negative: same PATH, no REQUIRE_C | passes with `C cross cc: none (SQLite part skipped)`; no qemu on that PATH -> ELF checks only (the old silent-skip behaviour, now printed) |
| 04:07 | negative: `REAL_TOOLCHAIN_AARCH64_PORT_BASE=70000` | FAILED as intended: `port base "70000" is not a number 1024-65535` |
| 04:07 | negative: foreign listener (aarch64 tuner under qemu) on 22242 before the run | FAILED as intended: `127.0.0.1:22242 already in use: pick a free range with REAL_TOOLCHAIN_AARCH64_PORT_BASE`; listener then killed, 0 listeners on 22240-22249 |
| 04:07:27-04:08:40 | `GALION_REAL=1 REAL_UCPU_PORT_BASE=22244 cargo test --test real_sh_ucpu` | **38 passed, 0 failed** (1 test ok, 73 s). aarch64 + armv7: galion-one, tuner, openmind-memory, hermes-gateway all static (armv7 sizes 824,980 / 397,972 / 1,654,412 / 2,042,660 B); per arch: galion-one `/health` + `/me`, tuner `/health` + `/`, memory `/health`, `/capture` (SQLite write) 200 `{"l0_recorded":2,...}`, `/search/conversations` (SQLite read) 200 "Found 1 matching message(s)", hermes-gateway `--mock` `/health`, `/v1/models`, `/v1/chat/completions` (SQLite session) 200; RSS 11-21 MB under qemu; all ports closed after stop |

Before this change the only way to run either test was on ports outside any slot range.

## VPS (read-only, 04:08:52 +01:00)
`ssh slavik-vps`: `uname -m` = x86_64, no qemu-aarch64 (aarch64 binaries cannot run there;
nothing copied, built or run). Nerve `GET 127.0.0.1:8670/state` 200, 37,097 B, cycle 48,
mode live, score 64 / grade D; top alert still inc-865945dd (galion-one-mouth 503, stuck
loading ~9.2 h). Cortex observation via the nerve. This change touches tests/docs only; no
VPS service affected, nothing deployed.

## Findings
* F-354-1: openmind-memory search output stamps messages in UTC with `Z`
  (`[2026-10-28T03:08:06.277Z]`), same under x86_64 presumably; Rule 7 asks Zagreb offsets in
  docs. App-level, not changed here.
* F-354-2: other real tests still hard-code ports outside Wave-4 ranges, e.g.
  openmind-nerve `real_cross_target` (20873/20874, aarch64 nerve under qemu, no SQLite),
  gaia `real_gaia_loop` 20300, galion-health `real_fixes` 20191-20194, hermes-bench 20010/
  20015/20035, galion-wallstreet 20021. They can adopt `galion_realtest::ports` (or a copy)
  when a slot needs to re-run them.
* No other aarch64 SQLite test was skipping for a missing cross cc: `real_cross_cc` (slot 338)
  already ran; `real_omnode_enroll` / `real_enroll` aarch64 legs need no C code.
