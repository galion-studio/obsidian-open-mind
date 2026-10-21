<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: serde-order-pin (slot 146, 2026-10-21, CEST +02:00)

Box: shared 8-vCPU box, builds through `cargo-slot -j 2`,
`CARGO_TARGET_DIR=/workspace/swarm/target/serde-order-pin/...`. Ports 20160-20169.
Base = `swarm/integration-2` df42e0f (unpatched copy in a detached worktree).
VPS: read-only GETs and `systemctl is-active` only.

## 1. cargo's own feature resolution (`cargo tree -e features -i serde_json`)

| Tree | `-p <crate>` alone: preserve_order ON / off | `--workspace` |
|---|---|---|
| base df42e0f | 7 / **18** (normal edges only: 6 / 18) | ON |
| pinned | 26 / 0 (normal edges: 25 / 0) | ON |

Real test `real_cargo_resolves_preserve_order_for_every_member` (07:00:38):
`preserve_order with -p: on=51 off=0` (both edge sets summed), `--workspace` ON.

## 2. Same source, different result (07:00:24-07:00:34)

New test `crates/one-set-model/tests/key_order.rs` copied onto base:

```
base  cargo test -p one-set-model --test key_order               -> FAILED
      left:  {"alpha":{"b":null,"y":true},"mid":"x","zeta":1}  (pretty-printed)
      right: {"zeta":1,"alpha":{"y":true,"b":null},"mid":"x"}
base  cargo test -p one-set-model -p link-watch --test key_order -> ok
pin   cargo test -p one-set-model                                -> 5 passed, 0 failed
```

## 3. Real nerve binary, built `-p openmind-nerve` alone

`GALION_REAL=1 cargo test -p repo-check --test real_serde_order -- --test-threads 1`
(07:00:38-07:01:00, 3 passed, 21.97 s): release build of openmind-nerve alone,
`serve` on 127.0.0.1:20160 (one dead component on :20161):

```
GET /health -> 200 {"status":"ok","service":"openmind-nerve","version":"0.1.0","appended":0,"ts":"2026-10-21T07:01:00+02:00","system":"failing","score":75,"cycle":1}
GET /api/v1/state -> 200, 6992 B, keys ["tool","service","version","ts","started","cycle","interval_s","cycle_ms","mode","system","incidents","cortex","self_eval","components","gaia","signals","logs","reflex","alerts","probe_errors","probes_skipped"]
```

Side by side, base vs pinned `-p openmind-nerve` release binaries (07:02:24):

```
:20161 base   /health {"appended":0,"cycle":null,"score":null,"service":"openmind-nerve","status":"ok","system":null,"ts":"...","version":"0.1.0"}
:20161 base   /api/v1/state {"alerts":{"external":"none configured ...
:20162 pinned /health {"status":"ok","service":"openmind-nerve","version":"0.1.0","appended":0,"ts":"...","system":"failing","score":71,"cycle":1}
:20162 pinned /api/v1/state {"tool":"openmind-nerve","service":"galion-nerve","version":"0.1.0",...
```

Both processes killed; `ss -ltn` shows nothing on 20160-20169 afterwards.

## 4. Live VPS (read-only, 07:02:34)

```
galion-nerve-shadow :8672 /health      {"appended":0,"cycle":291,"score":70,"service":"openmind-nerve","status":"ok","system":"degraded",...}   sorted  (= base -p build)
galion-nerve-shadow :8672 /api/v1/state {"alerts":{...                                                                                         sorted
galion-nerve        :8670 /state       {"alerts":{...},"components":[...   200, 13091 B                                                         sorted
hermes-rs           :8642 /health      {"status":"ok","platform":"hermes-agent","version":"0.21.4",...}                                         insertion order
openmind-memory     :8420 /health      {"status":"ok","version":"0.1.0","uptime":...}                                                          insertion order
systemctl is-active galion-nerve galion-nerve-shadow -> active active
```

The live host already serves both orders; the nerves match the base `-p`
build byte-for-byte in key order. Nerve observation: the nerve :8670 /state was used as the live view (the
cortex is not live as its own service per the swarm record; shadow :8672 is
observe-only). Nothing was changed on the VPS.

## 5. Full suite

| Run | Result |
|---|---|
| base, `-p` the 18 off-crates (preserve_order off) 06:53-06:57 | 89 test binaries, 443 passed, 0 failed |
| pinned, same 18 `-p` + repo-check (now ON) 06:59-06:59 | 95 binaries, 471 passed, 0 failed |
| base, `--workspace --exclude openmind-sandbox --exclude cgroup-detect` 07:04-07:05 | 204 binaries, 962 passed, 0 failed, 3 ignored |
| pinned, same 07:03 | 207 binaries, **978 passed, 0 failed, 3 ignored** (+16 = the new tests) |
| pinned, `cargo clippy --workspace --all-targets -- -D warnings` (sandbox crates excluded) | clean |
| `cargo fmt --all --check`, `scripts/no-python.sh` | ok |
| `repo-check` on pinned tree | 1120 files, 0 errors, 29 pre-existing warnings, PASS |
| `repo-check --only deps` on base tree | 6 errors (root + the 5 per-crate lines), FAIL as intended |

Binary size, host release (`opt-level=z`, LTO), base -> pinned:
openmind-nerve 1 007 816 -> 1 012 872 (+5 056), hades-gate +2 304,
guardian-tick +3 152, gaia +10 720, one-set-model +9 768, openmind-queue -1 176.

Sandbox crates were not built or run here (sandbox_vps_only); they do not
depend on serde_json.

## 6. Slot 240 serde-pin-mergefix: re-run against main-staging (2026-10-22, CEST +02:00)

Branch `swarm/serde-pin-mergefix` = serde-order-pin 1d222ad + merges of
`swarm/main-staging` 7e2a12e and 7c95348; fast-forwards main-staging 7c95348.

Why the main merge skipped 1d222ad: main-staging brought three crates that
still asked for `preserve_order` per crate (bench-banking, galion-bench,
hermes-bench). On the plain merge (01:08:45):

```
error deps crates/bench-banking/Cargo.toml:12: serde_json in a member crate must be `{ workspace = true }` ...
error deps crates/galion-bench/Cargo.toml:19: ...
error deps crates/hermes-bench/Cargo.toml:16: ...
repo-check: 3 error(s) -> FAIL
deps_pin::this_repo_has_one_pin FAILED (11 passed, 1 failed)
```

Fix: the three lines now read `serde_json = { workspace = true }`; their
feature set does not change (the root pin keeps preserve_order on), and
Cargo.lock is unchanged. main-staging 7c95348 had already fixed the nerve
`Msg` large_enum_variant and the duplicate `#[test]` in cortex_replay.rs
(the two clippy errors that the pin exposes), so openmind-nerve is
byte-identical to main-staging.

| Run (final tree 7f77092) | Result |
|---|---|
| `repo-check --only deps` | 0 errors, PASS (was 3) |
| `repo-check` full | 2089 files, 0 errors, 79 warnings, PASS |
| repo-check tests (deps_pin 12/0, checks, lib) + one-set-model key_order | all pass |
| `cargo clippy --workspace --all-targets -D warnings` (excl. sandbox, cgroup-detect) | clean |
| `cargo test --workspace` (excl. sandbox, cgroup-detect, hermes-tools; debuginfo 0 because the box disk was full) 01:25-01:27 | 370 suites, 1966 passed, 0 failed, 18 ignored |
| `GALION_REAL=1 SERDE_ORDER_PORT=21100` real_serde_order 01:27:42-01:28:11 | 3/3; `cargo tree -p` preserve_order on=67 off=0; `-p openmind-nerve` release on :21100 `/health` = `status, service, version, appended, ts, system, score, cycle` (source order) |
| fmt, no-python | ok |
| x86_64 + aarch64 musl release build; aarch64-android + armv7 check (repo-check, bench-banking, galion-bench, hermes-bench) | ok |
| Throwaway test merge (copy of main-staging 7c95348, `merge --ff-only`) | fast-forward to 7f77092; deps 0 errors; tests pass |

Live VPS, read-only GETs 01:29:30:

```
galion-nerve :8670 /health {"appended":57,"cycle":18,"score":61,"service":"openmind-nerve","status":"ok","system":"degraded",...}   sorted (live bb287cc, built -p alone)
hermes-rs    :8642 /health {"status":"ok","platform":"hermes-agent","version":"0.21.4",...}                                        insertion order
openmind-memory :8420 /health {"status":"ok","version":"0.1.0","uptime":56645,...}                                                 insertion order
systemctl is-active galion-nerve gaia-cortex -> active active
```

So the live nerve still prints sorted keys; once a build with this pin is
deployed its `/health` and `/state` switch to source order (D-SOP2: confirm
no raw-text readers before the nerve redeploy). Nothing changed on the VPS.
