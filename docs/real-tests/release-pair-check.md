<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: release-pair-check (wave-4 slot 339) — 2026-10-28 (Europe/Zagreb CET +01:00)

Box only (x86_64 + aarch64 build, x86_64 run), `-j 2` via cargo-slot, own target dirs, ports
22090-22099. Logs: `/workspace/swarm/artifacts/release-20261028-r339/logs/`. Summary and owner
decisions: `docs/release/release-pair-check-20261028.md`. No deploy; VPS read-only.

## Commands

```sh
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/release-pair-check
# matrix (02:26-02:31)
GALION_BUILD_COMMIT=124a212456b00ce79ef9a0dcbd6abf69bf55a0f7 GALION_BUILD_TIME=2026-10-28T02:30:00+01:00 \
SOURCE_DATE_EPOCH=1793151000 cargo-slot build --release --locked -j 2 --target x86_64-unknown-linux-musl \
  -p openmind-nerve -p hermes-gateway -p openmind-memory -p openmind-queue -p galion-one -p galion-health \
  -p galion-desk -p link-watch -p tuner -p console-static -p galion-backup -p desk-hermes
scripts/size-budget.sh --check-dir /workspace/swarm/artifacts/release-20261028-r339/x86_64-unknown-linux-musl
# release bytes, ports 22090/22091/22099 (02:31:27-02:31:34)
GALION_REAL=1 GALION_RELEASE_DIR=/workspace/swarm/artifacts/release-20261028-r339 GALION_RELEASE_TAG=124a212 \
GALION_RELEASE_PORT_BASE=22090 GALION_RELEASE_CORTEX=gaia-cortex-77df80f-llm \
GALION_RELEASE_CONSOLE_SERVE=galion-console-serve-2d846fb \
  cargo-slot test -j 2 -p galion-realtest --test real_release_artifacts -- --nocapture
# byte proof (02:31:40-02:36:55)
GALION_REAL=1 GALION_REPRO_DIR=/workspace/swarm/target/release-pair-check-repro \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_repro -- --nocapture
cargo-slot test -j 2 -p galion-feature-pin --test release_feature_unify
```

## Outputs (trimmed)

```
openmind-nerve :22090 GET /health -> 200 {"status":"ok","service":"openmind-nerve","version":"0.1.0",...}
tuner :22090 GET /health -> 200 {"ok":true}
galion-health :22090 GET / -> 200 <!doctype html>...<title>galion-health</title>
openmind-queue :22090 GET /healthz -> 200 {"ok":true,"function":"POSEIDON",...}
openmind-memory :22090 GET /health -> 200 {"status":"ok",...,"vectorStore":true,...}
galion-backup snapshot -> exit 0; verify -> exit 0 integrity=ok tables=15
hermes-gateway :22090 GET /health -> 200 {"status":"ok","platform":"hermes-agent","implementation":"hermes-rs",...}
galion-one :22090 GET /health -> 200 ok
galion-desk :22090 GET /health -> 200 {"ok":true}
galion-console-static :22090 GET / -> 200
gaia-cortex-77df80f-llm :22090 GET /health -> 200 {"ok":true,"service":"gaia-cortex",...,"mode":"dry-run",...}
galion-console-serve-2d846fb :22090 GET / -> 302 ; GET /api/v1/health -> 200
--version: 9 bins exit 0; build-info bins print "commit 124a212456b0..., built 2026-10-28T02:30:00+01:00"
release_artifacts_health: 21 checks passed -> 1 passed; 0 failed
size-budget: ok (14 binaries)            [6ffb2570 script: FAIL on hermes-gateway-124a212]
real_release_repro x86_64-unknown-linux-musl: 14 compared, 0 differ   (full set 70.3 s, test 311.3 s)
release_feature_unify: 3 passed; 0 failed
aarch64 8-bin non-C set: built 43.5 s, size-budget ok (8), AArch64, no PT_INTERP
aarch64 solo vs 8-set: openmind-nerve, galion-one, console-static IDENTICAL (3/3)
```

aarch64 bins were not executed (no qemu-user on the box; VPS is x86_64). SQLite bins on aarch64
need a C cross compiler (no zig on the box, D-318-3).

## VPS read-only (2026-10-28 02:27:03 CET, `logs/vps-ro-probe.log`)

```
GET :8670/health 200 149B ; GET :8670/state 200 37049B (nerve; cortex active)
GET :8642/health 200 122B ; :8080 and :18080 /api/v1/health 200 192B
11 units active (galion-nerve gaia-cortex hermes-rs openmind-memory openmind-queue galion-one
  galion-health galion-desk link-watch-rs tuner console-rust-primary)
live ExecStart: openmind-nerve-9d1a4f3, hermes-gateway-b7fbbc78, openmind-memory-aa77864, galion-backup-98466ab
loadavg 18.13
```

Only GETs and `systemctl is-active/show`. Nothing on the VPS was changed or started.

## Gates (02:39-02:48 CET)

- `cargo fmt --all --check` rc 0.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` rc 0.
- `cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  **385 suites, 2009 passed, 0 failed, 18 ignored** (hermes-tools excluded: its sandboxed tests are VPS-only).
- `scripts/no-python.sh` rc 0; `scripts/spdx-scan.sh` 755 scanned, missing 0;
  `repo-check` 2132 files, 0 errors / 83 warnings PASS.
