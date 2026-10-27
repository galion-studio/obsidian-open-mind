<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: release-artifacts-refresh (slot 312) — 2026-10-27 (Europe/Zagreb CET +01:00)

Artifacts: `/workspace/swarm/artifacts/release-20261027-r312/` (logs under `logs/`).
Ports: 21820 (service), 21821 (second service: tuner behind galion-health, galion-one mouth), 21829 (dead upstream). All free after each run (`ss -ltn`), no `/tmp/r312-*` left.

## 1. Box: release bytes, x86_64 musl (21:59:53 and 22:05 CET)

```
GALION_REAL=1 GALION_RELEASE_DIR=/workspace/swarm/artifacts/release-20261027-r312 \
GALION_RELEASE_TAG=38a0770 GALION_RELEASE_CORTEX=gaia-cortex-77df80f-llm \
GALION_RELEASE_CONSOLE_SERVE=galion-console-serve-2d846fb \
cargo test -p galion-realtest --test real_release_artifacts -- --nocapture
```

The test starts the artifact files themselves (not a fresh cargo build), one at a time, and also re-checks each ELF (no PT_INTERP, no `.symtab`, right e_machine).

```
openmind-nerve :21820 GET /health -> 200 {"status":"ok","service":"openmind-nerve","version":"0.1.0",...}
tuner :21820 GET /health -> 200 {"ok":true}
galion-health :21820 GET / -> 200 <!doctype html>...<title>galion-health</title>  (checking a real tuner on :21821)
openmind-queue :21820 GET /healthz -> 200 {"ok":true,"function":"POSEIDON","service":"openmind-queue",...}
openmind-memory :21820 GET /health -> 200 {"status":"ok","version":"0.1.0",...,"vectorStore":true,...}
galion-backup snapshot -> exit 0 snapshot ok .../vectors.db -> .../snap.db   (the SQLite file openmind-memory just wrote)
galion-backup verify -> exit 0 verify .../snap.db integrity=ok tables=15
hermes-gateway :21820 GET /health -> 200 {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs",...}  (--mock, throwaway key)
galion-one :21820 GET /health -> 200 ok
galion-desk :21820 GET /health -> 200 {"ok":true}
galion-console-static :21820 GET / -> 200 <!doctype html><title>r312</title>
gaia-cortex-77df80f-llm :21820 GET /health -> 200 {"ok":true,"service":"gaia-cortex",...,"mode":"dry-run","llm_enabled":false,...}
galion-console-serve-2d846fb :21820 GET / -> 302   (live :8080 and :18080 also answer 302 -> /console)
galion-console-serve-2d846fb :21820 GET /api/v1/health -> 200 {"status":"healthy","service":"galion-console-static","implementation":"rust","mode":"local-api",...}
--version: nerve, memory, queue, one, health, desk, link-watch, tuner, desk-hermes all exit 0; the
build-info ones print "commit 38a07700018906fc21de7ebce1500bdcc4762671, built 2026-10-27T22:00:00+01:00".
release_artifacts_health: 21 checks passed -> test result: ok. 1 passed; 0 failed
```

Without `GALION_REAL=1` the test skips (1 passed, 0.01 s).

Failures found and fixed in the test during the run (not product bugs): galion-console-static needs `_shell.html` under `--root`; `--dist` answers `/` with 302 (same as live).
Product findings: galion-console-static and galion-backup have no `--version` (D-312-3).

aarch64 artifacts were **not executed**: no qemu-user on the box (apt failed with SSL errors to deb.debian.org), VPS is x86_64.

## 2. Reproducibility (box, 22:06-22:08 CET)

| Build | Result |
|---|---|
| Same 12-package `cargo build -p …` set, fresh `CARGO_TARGET_DIR` | **12/12 byte-identical** to the matrix |
| `-p openmind-nerve -p hermes-gateway` only | both differ (feature unification across the package set) |
| `-p hermes-gateway` alone | 2,313,184 B (matrix 2,313,344 B), different sha |
| `-p openmind-nerve` alone | same size 1,923,432 B, different sha |
| openmind-queue / galion-health vs slot 275 (other tip, other day) | bit-identical sha256 |

So the artifact bytes depend on which packages are built together (D-312-4): a release recipe must name the exact `-p` set (or build each binary alone) to be reproducible.

## 3. VPS read-only (2026-10-27 22:00:32 CET, `logs/vps-ro-probe.log`)

```
GET :8670/health 200 149B      (nerve)
GET :8670/state 200 36112B     (nerve state; cortex live too)
GET :8642/health 200 122B      (hermes-rs)
GET :8080/api/v1/health 200 192B, :18080/api/v1/health 200 192B (Rust console)
systemctl is-active: galion-nerve gaia-cortex hermes-rs openmind-memory openmind-queue galion-one
  galion-health galion-desk link-watch-rs tuner console-rust-primary -> all active
live sizes: hermes-gateway-b7fbbc78 2,732,248; galion-backup-98466ab 1,959,376;
  openmind-memory-aa77864 1,702,640; openmind-nerve-9d1a4f3 1,458,760; gaia-cortex-73583ab-llm 1,136,200;
  galion-console-serve-2d846fb8 949,568 (table in docs/release/artifacts-refresh-20261027.md)
VPS loadavg 27.97 (known host noise)
```

Only GETs, `stat`, `sha256sum` and `systemctl show/is-active`. Nothing on the VPS was changed or started.

## 4. Gates on this branch (22:00-22:04 CET)

- `cargo fmt --all --check` ok (after formatting the new test).
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` rc=0; re-run for galion-realtest after fmt ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`: **378 suites, 2005 passed, 0 failed, 18 ignored** (hermes-tools excluded because its `sandboxed.rs` runs the sandbox, sandbox_vps_only).
- `scripts/no-python.sh` rc=0; `scripts/spdx-scan.sh` missing 0; `repo-check` 0 errors / 83 warnings PASS.
- `scripts/size-budget.sh --check-dir` x86_64 + aarch64: ok (14 + 14).
