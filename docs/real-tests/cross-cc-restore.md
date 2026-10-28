<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cross-cc-restore (slot 338, 2026-10-28, CET +01:00)

Base: swarm/cross-cc-restore off snapshot 3f015986. Box only; VPS read-only
probe at the end. Ports 22080-22081 used, free afterwards.

## Commands

```sh
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/cross-cc-restore
cargo-slot build --release -j 2 --locked --target aarch64-unknown-linux-musl -p hermes-gateway
cargo-slot build --release -j 2 --locked --target armv7-unknown-linux-musleabihf -p hermes-gateway
GALION_REAL=1 cargo-slot test -j 2 -p galion-realtest --test real_cross_cc -- --nocapture
# aarch64 byte proof, run in a clean clone of swarm/release-bins-add fc481390
GALION_REAL=1 GALION_REPRO_TARGET=aarch64-unknown-linux-musl \
  GALION_REPRO_DIR=/workspace/swarm/target/cross-cc-restore-repro \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_repro -- --nocapture
```

## Results

| check | time | result |
|---|---|---|
| zig tarball sha256 vs ziglang.org index.json + install-zig.sh pin | 02:26 | match `02aa270f…3239` |
| zig minisign vs ziglang.org key | 02:27 | OK; tampered copy FAILS |
| hermes-gateway aarch64 musl (sqlite+coding), no CC env | 02:28 | built 30.6 s, 2045824 B, AArch64, static (0 INTERP/DYNAMIC); libsqlite3-sys log `CC_aarch64_unknown_linux_musl = None`, `sqlite3.o` is AArch64 |
| negative control: same build with /workspace/swarm/bin off PATH | 02:29 | **fails** as expected: `failed to find tool "aarch64-linux-musl-gcc"` |
| hermes-gateway armv7 musleabihf, no CC env | 02:29 | built 27.8 s, 2043004 B, ARM, Tag_CPU_arch v7, VFP-args hard float, static |
| aarch64 gateway under qemu-aarch64 `--mock` on :22080 | 02:31 | /health 200 (same JSON shape as live VPS :8642), chat 200 `echo: aarch64 sqlite write`, /api/sessions lists s338, no-key /v1/models 401; after kill + restart on same state.db the session (message_count 2) is read back |
| armv7 gateway under qemu-arm on :22081 | 02:31 | chat 200, session read back after restart |
| `real_cross_cc` (new Rust GALION_REAL test, both targets) | 02:33-02:36 | 1 passed (cold 82 s; warm 1.1 s) |
| aarch64 solo-vs-full byte proof (real_release_repro, fc481390) | 02:31:40-02:37:15 | **19 compared, 0 differ**, 14/14 within budget + stripped (hermes-gateway 2048992 B / 2621440 B) |
| zig cc determinism, sqlite3.c aarch64 -O2, two cold zig caches | 02:39 | both `49090e31…b35a` (identical) |
| VPS read-only probe | 02:38 | x86_64 (no arm host to run on), nerve /state 200 37109 B, live hermes-rs active, :8642 /health same body as the aarch64 build; no cross tools on the VPS (none needed) |

aarch64 byte proof rows (sha256 prefix, solo == full):

```
openmind-nerve          1666696 B f207de74b18b9810 IDENTICAL
hermes-gateway          2048992 B 8b1d4205dd2357a4 IDENTICAL
openmind-memory         1695432 B a13d59bb06241cbc IDENTICAL
openmind-queue           763096 B 9b578cf840634dde IDENTICAL
galion-one               845048 B ce7e097fb29638f5 IDENTICAL
galion-health            496888 B 047114d5876d15ae IDENTICAL
galion-desk              665368 B 6f16edc9704e7490 IDENTICAL
link-watch               678824 B 5c048528b8870b21 IDENTICAL
tuner                    432544 B 29f63472fd32a7cc IDENTICAL
galion-console-static    867192 B 2b3ba326d0d5c6a3 IDENTICAL
galion-console-serve     867192 B 2b3ba326d0d5c6a3 IDENTICAL
galion-backup           1427984 B 276c6767b813a79d IDENTICAL
desk-hermes             1350576 B 761a49077e0b67e1 IDENTICAL
gaia-cortex              980240 B b9ab8fa310a31527 IDENTICAL
pairs nerve+gateway, cortex+nerve and the console --bin recipe: IDENTICAL
```

## Limits

- No real aarch64/armv7 server: the VPS is x86_64; runtime proof is qemu-user.
- `real_toolchain_aarch64.rs` (slot 145) was not re-run: its ports
  20150-20153 are outside this slot's range. It now finds qemu-aarch64-static
  on PATH and, with `CC_aarch64_unknown_linux_musl` set, the SQLite path too.
- Debian InRelease signature not verified (no gpgv); chain is HTTPS + sha256.
