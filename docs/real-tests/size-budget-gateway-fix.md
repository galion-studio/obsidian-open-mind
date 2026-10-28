<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: size-budget-gateway-fix (wave-4 slot 353) — 2026-10-28 (Europe/Zagreb CET +01:00)

Box only (x86_64 + aarch64 build, x86_64 run, one aarch64 qemu-user probe), `-j 2` via cargo-slot, own
target dir, ports 22230-22239. Logs: `/workspace/swarm/artifacts/release-20261028-r353/logs/`. Summary and
owner decisions: `docs/release/size-budget-gateway-fix-20261028.md`. No deploy; VPS read-only.

## Commands

```sh
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/size-budget-gateway-fix
# x86_64 musl matrix, tree 942c3d95 (04:08-04:09)
GALION_BUILD_COMMIT=942c3d95... GALION_BUILD_TIME=2026-10-28T04:10:00+01:00 SOURCE_DATE_EPOCH=1793157000 \
  cargo-slot build --release --locked -j 2 --target x86_64-unknown-linux-musl -p openmind-nerve -p hermes-gateway \
  -p openmind-memory -p openmind-queue -p galion-one -p galion-health -p galion-desk -p link-watch -p tuner \
  -p console-static -p galion-backup -p desk-hermes
# 14-bin set = those 12 (tag 942c3d9) + gaia-cortex-77df80f-llm + galion-console-serve-2d846fb (slot 339 bytes)
scripts/size-budget.sh --check-dir /workspace/swarm/artifacts/release-20261028-r353/x86_64-unknown-linux-musl
scripts/size-budget.sh --check-dir /workspace/swarm/artifacts/release-20261028-r339/x86_64-unknown-linux-musl  # negative
GALION_REAL=1 SIZE_BUDGET_REAL_DIR=.../release-20261028-r353/x86_64-unknown-linux-musl \
  SIZE_BUDGET_NEG_DIR=.../release-20261028-r339/x86_64-unknown-linux-musl SIZE_BUDGET_NEG_BIN=hermes-gateway-124a212 \
  cargo-slot test -j 2 -p galion-realtest --test size_budget_gate -- --nocapture
GALION_REAL=1 HG2_BIN=.../x86_64-unknown-linux-musl/hermes-gateway-942c3d9 HG2_PORT_BASE=22233 \
  cargo-slot test -j 2 -p hermes-gateway --test real_size_2mib -- --nocapture
GALION_REAL=1 GALION_RELEASE_DIR=.../release-20261028-r353 GALION_RELEASE_TAG=942c3d9 GALION_RELEASE_PORT_BASE=22230 \
  GALION_RELEASE_CORTEX=gaia-cortex-77df80f-llm GALION_RELEASE_CONSOLE_SERVE=galion-console-serve-2d846fb \
  cargo-slot test -j 2 -p galion-realtest --test real_release_artifacts -- --nocapture
cargo-slot test -j 2 -p galion-feature-pin --test release_feature_unify
GALION_REAL=1 GALION_REPRO_DIR=/workspace/swarm/target/size-budget-gateway-fix-repro \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_repro -- --nocapture
# aarch64 musl: 8 non-C bins, then the 4 SQLite bins (zig cc wrappers from slot 338 on PATH)
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld cargo-slot build --release --locked -j 2 \
  --target aarch64-unknown-linux-musl -p ...
scripts/size-budget.sh --check-dir /workspace/swarm/artifacts/release-20261028-r353/aarch64-unknown-linux-musl
```

## size-budget.sh on the 14-bin x86_64 set (04:09:15 CET) -> ok (14 binaries)

| binary | bytes | headroom to 2,097,152 | slot 339 bytes |
|---|---:|---:|---:|
| hermes-gateway-942c3d9 | 1,944,432 | 152,720 | 2,198,288 (FAIL) |
| openmind-nerve-942c3d9 | 1,923,464 | 173,688 | 1,930,048 |
| openmind-memory-942c3d9 | 1,555,280 | 541,872 | 1,727,216 |
| galion-backup-942c3d9 | 1,245,384 | 851,768 | 1,415,680 |
| desk-hermes-942c3d9 | 1,160,456 | 936,696 | 1,333,792 |
| gaia-cortex-77df80f-llm | 1,152,840 | 944,312 | same bytes |
| galion-one-942c3d9 | 960,680 | 1,136,472 | 963,904 |
| galion-console-serve-2d846fb | 949,064 | 1,148,088 | same bytes |
| galion-console-static-942c3d9 | 936,520 | 1,160,632 | 938,784 |
| openmind-queue-942c3d9 | 894,472 | 1,202,680 | 897,856 |
| link-watch-942c3d9 | 785,336 | 1,311,816 | 791,248 |
| galion-desk-942c3d9 | 757,736 | 1,339,416 | 766,752 |
| galion-health-942c3d9 | 557,256 | 1,539,896 | 557,856 |
| tuner-942c3d9 | 477,064 | 1,620,088 | 480,032 |

All stripped; every row `budget 2097152`, key = tag-stripped name (`gaia-cortex-77df80f-llm` -> `gaia-cortex`).

## Negative controls

- Real: slot 339 set with the new script -> `hermes-gateway-124a212 2198288 yes yes FAIL 2097152 hermes-gateway`,
  exit 1 (the 7427f96e script passed it, exit 0).
- Old scripts on the new set: 7427f96e, 6ffb2570, a0d099c2 all exit 0; on the 339 set 0 / 1 / 1.
- size_budget_gate.rs fixtures (synthetic ELF64 EXEC headers): gateway at exactly 2,097,152 B ok (tagged);
  2,198,288 B tagged and untagged FAIL; nerve 2,097,153 B FAIL; `.symtab` FAIL (`stripped no`);
  custom table `tuner 4000` FAIL via `--budget-file` and `SIZE_BUDGET_FILE`, untabled bin -> `default`;
  bad tables (no default, duplicate, `2MiB`, 3 fields, `*` name, missing file) exit 1 with their message;
  empty dir exit 1.

## Other real tests

```
size_budget_gate (GALION_REAL=1): 4 passed; 0 failed (real 14 ok, real negative FAIL as expected)
real_size_2mib x86_64 release hermes-gateway-942c3d9, ports 22233-22235 (04:09:57 CET):
  SIZE 1944432 B (budget 2097152 B, headroom 152720 B); ELF stripped yes, .relr.dyn yes
  /health 200 after 50 ms; /v1/models 200; chat 200 (stub upstream, loopback only); turn 2 200; roles 4;
  legacy FTS5 state.db read-only 200 roles 2; restart keeps session (roles 4); no key 401;
  --mock :22234 chat 200, mock-fail 502; PASS
real_release_artifacts (tag 942c3d9, ports 22230/22231/22239, 04:10:04 CET): 21 checks passed
  (9 --version, nerve/tuner/health/queue/memory/gateway/one/desk/console-static/cortex/console-serve GETs 200/302,
  galion-backup snapshot+verify integrity=ok tables=15)
release_feature_unify: 3 passed; 0 failed
real_release_repro x86_64 (04:10-04:15 CET, 288.6 s): 14 compared, 0 differ (12 solo + nerve/gateway pair)
aarch64 musl: 12/12 built (8 non-C 41.9 s, 4 SQLite 58.7 s), AArch64, no PT_INTERP, size-budget ok (12);
  hermes-gateway 1,883,296 B, openmind-memory 1,537,840 B, openmind-nerve 1,591,824 B
aarch64 hermes-gateway under qemu-aarch64 --mock :22236 (04:13 CET): /health 200, chat 200 "echo: hi a64",
  /api/sessions lists it; rs.db written (40960 B); process killed, temp dir removed
```

## VPS read-only (04:10:24 CET, `logs/vps-ro-probe.log`)

```
GET :8670/health 200 149B ; GET :8670/state 200 37097B (nerve; cortex unit active)
GET :8642/health 200 122B ; :8080 and :18080 /api/v1/health 200 192B
hermes-rs, galion-nerve, gaia-cortex active
live ExecStart: hermes-gateway-b7fbbc78, openmind-nerve-9d1a4f3
/opt/galion/rust/bin sizes: hermes-gateway-b7fbbc78 2,732,248 B (stripped, no .relr.dyn, PIE x86_64,
  mtime 2026-10-27 19:07:22 CET); hermes-gateway-aa77864 1,841,904; openmind-nerve-9d1a4f3 1,458,760
nerve /state alerts.recent: galion-one incident inc-865945dd [error] (already known); loadavg 21.77
```

F-353-1: the live gateway is over budget (2,732,248 > 2,097,152 and > the old 2,621,440). Nothing on the
VPS was changed, started or copied; only GETs, `systemctl is-active/show`, `stat`, `readelf`.

## Gates (04:15-04:19 CET)

- `cargo fmt --all --check` rc 0.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` rc 0.
- `cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  **387 suites, 2014 passed, 0 failed, 18 ignored** (hermes-tools excluded: its sandboxed tests are VPS-only).
- `scripts/no-python.sh`: ok.
