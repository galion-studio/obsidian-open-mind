<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: gold-image format + `galion-admin image verify` (wave-4 slot 204)

All times Europe/Zagreb (CEST, +02:00). Box only (row 204: no device, no flashing, local repo
only). No sandbox was run (sandbox_vps_only does not apply: `image verify` creates no
namespaces). Slot ports 20740-20749 reserved but unused: nothing listens.

## Box checks (2026-10-21 18:13-18:25)

| Check | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --all-targets -- -D warnings` on x86_64-gnu, x86_64-musl, aarch64-musl, aarch64-linux-android, armv7-musleabihf | 0 warnings each |
| release static builds (rust-lld for arm) | x86_64-musl 1,063,048 B; aarch64-musl 1,070,592 B; armv7-musleabihf 1,039,724 B |
| unit tests x86_64 | 36 passed (25 from slot 165 + 10 image + 1 cli parse) |
| unit tests aarch64-musl under qemu-aarch64 | 36 passed + simd_free 1 |
| unit tests armv7-musleabihf under qemu-arm | 36 passed + simd_free 1 |
| `tests/simd_free.rs` (serial curve25519 backend, no AVX2) | 1 passed on all three |
| regression: slot-165 `GALION_REAL=1 real_cli` (fake launcher) 18:25:01 | 9 passed |
| `real_vps` on the box | 7 compiled, self-skip (VPS-only) |
| Python / shell files in repo | 0 |

## `GALION_REAL=1 tests/real_image.rs` (release)

Inputs: key pairs made by the **real `hades-gate keygen`** (openmind repo integration-5 0d68f46,
x86_64-musl release built at 18:18 into the slot target dir; seed files checked mode 0600);
the image holds real artifacts: release `galion-admin` (946,712 B), the `gold_image` builder,
the aarch64 and armv7 `galion-admin` builds and the `hades-gate` binary (`$GALION_IMAGE_EXTRA`),
plus `etc/galion/os.toml` (0644) and an empty `empty.conf` (0600): 7 files, 4,402,300 B.

### x86_64 binary, 18:21:13-18:21:42 — 2 passed

```
built: manifest 7 files, 4402300 bytes, sha256 9f139fe6a18d46d7...; signed by key cd0abef23b3d28a5; extra files 3; key by hades-gate keygen
ok run: exit 0 20 ms: image ok: galion-os-gold 0.1.0-s204 x86_64 commit bf33cba, 7 files, 4402300 bytes, key cd0abef23b3d28a5, manifest 9f139fe6a18d46d7 (14 ms)
```

| Tamper case (each undone and re-verified ok afterwards) | Exit | First problem line |
|---|---|---|
| bit flip byte 0 / 1 / 473356 / 946711 of 946712 | 4 | `sha256 bin/galion-admin (content differs from the signed SHA-256)` |
| truncate by 1 / 4096 / all | 4 | `truncated bin/galion-admin (size 946711 / 942616 / 0, manifest says 946712)` |
| append 1 byte | 4 | `grown bin/galion-admin (larger than the listed 946712 bytes)` |
| remove `etc/galion/os.toml` | 4 | `missing etc/galion/os.toml` |
| add `bin/implant` | 4 | `unlisted bin/implant` |
| add empty dir `var` | 4 | `unlisted-dir var` |
| replace os.toml by a symlink to identical bytes outside | 4 | `not-regular etc/galion/os.toml (symlink)` |
| chmod 0644 → 0666 | 4 | `mode etc/galion/os.toml (mode 0666, manifest says 0644)` |
| manifest version edited / last 4 bytes cut | 4 | `REJECTED: bad signature` |
| one signature hex char changed | 4 | `REJECTED: bad signature` |
| signature file deleted | 4 | `REJECTED: metadata: galion-image.sig: entity not found` |
| second real hades key as `--key` | 4 | `REJECTED: wrong key: image signed by key cd0abef23b3d28a5, verifying key is bae6713331b25c9d` |
| seed file passed as `--key` | 1 | stderr `not a ed25519-pub file` |

Also checked: `--json` → `{"ok":true,...,"problems":[]}`; a full byte+mode snapshot of the image
is identical before and after all runs (verify writes nothing); neither seed nor public key hex
appears in any output or in the audit log; audit log 37 `image.verify` records,
`audit ok: 37 records, head fc7555402fa0eb82...`. 17 tamper cases.

Throughput (512 MiB xorshift pseudo-random file, box load average ~21 on 8 vCPU):
`manifest build 4376 ms; verify runs [3607, 4010, 6052] ms, median 4010 ms = 128 MiB/s`;
1-bit flip 12,345 bytes before the end → exit 4 `sha256 data/rootfs.img` (6983 ms).
Reference: coreutils `sha256sum` (hardware SHA-NI) hashed 256 MiB in 0.40 s wall at 18:22:58.

### aarch64-musl binary under qemu-aarch64, 18:21:51-18:22:26 — 2 passed

Same 17 tamper cases, wrong key (`7cde08bdd3a5c9ab` vs `6545b7dc74b6d3ac`), audit ok 37 records.
OK run 564 ms wall (387 ms in-process, emulated). 128 MiB: median 2345 ms = 55 MiB/s (emulated).

### armv7-musleabihf binary under qemu-arm, 18:22:26-18:22:50 — 2 passed

Same 17 tamper cases, wrong key (`f60e5978fe592fcc` vs `d2d4e630f0111786`), audit ok 37 records.
OK run 380 ms wall (261 ms in-process). 128 MiB: median 2677 ms = 48 MiB/s (emulated).

### Key-format interop (18:22:58)

`hades-gate keygen` printed `{"admin_fingerprint":"e9a2596e1b6c0de0","ok":true}`;
`gold_image sign --seed <that seed>` printed `signed by key e9a2596e1b6c0de0`;
`galion-admin image verify --key <hades pub>` → `image ok ... key e9a2596e1b6c0de0`, exit 0.

## Live system observed through the nerve (cortex not deployed yet, so nerve only)

Row 204 is box-only, so nothing ran on the VPS. One read-only `GET http://127.0.0.1:8670/state`
over ssh at 18:23:13: HTTP 200, 32,276 B, cycle 17, mode `dry-run` (slot gaia-decisions-apply
switched the nerve to dry-run at ~17:05), counts healthy 11 / degrading 1 / failing 0 /
absent 2, score 76 / C. The response was saved to `/tmp/.s204n` on the VPS for the grep and
deleted in the same ssh command. Nothing in this slot touched a VPS service.

## How to re-run

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/galion-os-image-format
cargo-slot build -j 2 --release --examples
GALION_REAL=1 GALION_HADES_GATE=<hades-gate binary> GALION_IMAGE_EXTRA=<dir of extra files> \
  cargo-slot test -j 2 --release --test real_image -- --test-threads=1 --nocapture
# cross: add GALION_ADMIN_BIN=<aarch64 galion-admin> GALION_IMAGE_RUNNER=qemu-aarch64 GALION_IMAGE_BIG_MB=128
```
