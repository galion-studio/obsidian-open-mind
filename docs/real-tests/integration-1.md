<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life test: integration-1 (slot 22)

Box run, 2026-10-20 (Europe/Zagreb, +02:00). Ports: 19220-19229 (own) and
19500-19551 (real-harness range, slot 22 only in wave 2). All started
processes were torn down; ports checked closed afterwards. The only VPS
contact was one `galion-realtest live` run (GET-only curl, `systemctl
is-active`, `ss -ltnH` over `ssh slavik-vps sh -s`; nothing written there).
Build env: `nice -n 10`, `-j 3`, `CARGO_TARGET_DIR=/workspace/swarm/target/integration-1`,
`CARGO_INCREMENTAL=0`, dev/test debug info off.

## Static gates (final head `881de9d`, 20:30-20:32)

| check | result |
|---|---|
| `cargo fmt --all --check` (workspace + edge/openmind-node) | ok |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok |
| `cargo test --workspace --locked` | 206 suites, **950 passed, 0 failed**, 4 ignored |
| edge/openmind-node clippy -D warnings + `cargo test --locked` | **72 passed, 0 failed** |
| `scripts/ucpu-gate.sh all` (zig 0.15.2) | RESULT ok: grep ok; x86_64 + aarch64 musl release builds (workspace + omnode) static, size-budget ok (27 + 1 binaries per target); armv7 + aarch64-android `cargo check` ok |
| `scripts/public-export.sh` (launch-gates tools) | 0 FAIL, 1 WARN (gate 10: 20 public IPv4 literals, 1 quick-tunnel hostname; owner decides) |
| `repo-check --copyright "3000 Galion / galion-studio"` | 1074 files, 0 errors, 28 warnings (utc) -> PASS |
| `scripts/no-python.sh` | ok |

Before the fixups (head `853cb92`, all 20 merges, no fixups): ucpu-gate FAIL
(grep: 2 comment hits; `aarch64-linux-android` check: `libc::PTRACE_SEIZE`
missing in `openmind-sandbox/examples/escape.rs`); public-export FAIL
(gate 7 REUSE: 5 problems; gate 8 cargo deny: crates not cached offline);
repo-check 9 errors. All fixed in F1/F2 (docs/integration/integration-1.md).

## `galion-realtest stack` without the galion-one known failures

`tests/real/known-failures.txt` now lists only the two live-VPS entries.

```
$ target/release/galion-realtest stack --bin-dir target/release --base-port 19500 --out .../stack
PASS        stack-boot                               galion-boot
PASS        one-pairing                              galion-one
PASS        one-fail-closed-missing-store            galion-one
PASS        one-fail-closed-empty-store              galion-one
PASS        memory-save-search                       openmind-memory
SKIP        nerve-ingest                             openmind-nerve
PASS        tuner                                    tuner
PASS        link-watch                               link-watch
PASS        desk                                     galion-desk
PASS        voice-relay-contract                     persona-plex-relay
PASS        guardian-tick-heal                       guardian-tick
PASS        heal-request                             galion-boot
PASS        teardown                                 galion-realtest
Checks: 77 total, 76 pass, 0 FAIL, 0 known-fail, 1 skipped.   (exit 0)
```

`one-fail-closed-missing-store` and `one-fail-closed-empty-store`: 7/7 checks
each, all PASS (the 10 former KNOWN-FAIL ids included). The skip is
nerve-ingest, which only runs with `--with-nerve`:

```
$ ... stack --with-nerve --out .../stack-nerve
(all 13 scenarios PASS, nerve-ingest included)
Checks: 81 total, 81 pass, 0 FAIL, 0 known-fail, 0 skipped.   (exit 0)
```

Both were run twice (20:25-20:26 at `853cb92` + fixups in the tree, and
20:29 at `177dd0c` from `scripts/real/integration-1.sh`), same results.

## Cross-branch: gaia-live heal-tick -> nerve's `openmind-nerve serve`

`scripts/real/integration-1.sh` part C, 2026-10-20T20:29:23+02:00, nerve on
127.0.0.1:19221 (data dir in the run dir, systemd/journald/8660 probes off,
dry-run), gaia-dummy on 127.0.0.1:19222, allow range 19220-19229, per-run
random test token (never printed):

```
nerve GET /health: {"status":"ok","service":"openmind-nerve","appended":0}
1-healthy    exit=0 outcome=healthy
2-no-approval exit=4 outcome=refused
3-approved   exit=0 outcome=restarted
dummy after restart: {"pid":1467171}
nerve events: 5, kinds: gaia.heal.applied,gaia.heal.refused,service.probe
nerve JSONL lines on disk: 5
ok: test token not present in tick output or nerve log
port 19221 closed
port 19222 closed
```

## `galion-realtest live` (READ-ONLY, once, 2026-10-20T20:26:51+02:00)

```
PASS        live-vps:galion-one / openmind-memory / hermes-rs / galion-desk / tuner
            / galion-health / galion-nerve / tanga-serve / console / cloudflared / ports
PASS        live-public:public
KNOWN-FAIL  live-vps:galion-one-mouth   mouth-health: HTTP 503 {"status":"loading","model":"chatterbox","loaded":false}
KNOWN-FAIL  live-vps:units              link-watch.service: is-active: inactive
Checks: 43 total, 41 pass, 0 FAIL, 2 known-fail, 0 skipped.   (exit 0)
```

The live VPS still runs the 2f5a730 builds; this run only confirms the
probes and the contract, not this branch's code.
