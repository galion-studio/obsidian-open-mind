<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 245 cortex-live-merge-2: real-test record and deploy checklist

Branch `swarm/cortex-live-merge-2`, all times Europe/Zagreb (CEST +02:00), 2026-10-22.

## What is merged

| Step | Commit | Content |
|---|---|---|
| base | `bd6fbe6` | cortex-live-fixes = the build live on the VPS (`gaia-cortex-bd6fbe6`, live mode, LLM advisor on, Hermes handoff on) |
| 1 | `50f5d43` / `5e0adba` | cortex-api-load (slot 236): snapshot reads, ingest without engine lock, ack 2 s, `evict_idle`; `/health` keeps `hermes_handoff` + `handoffs` |
| 2 | `a06bdbd` | cortex-flap-dedupe (slot 235): repeat watch (`GET /evt` re-read of folded repeats), occurrence-weighted flap rule, caps 2/unit/h + 6/h total in every mode |
| 3 | `1754ff2` | cortex-incident-backfill (slot 237): nerve incidents keyed by cause, backfill + reconcile on every stream (re)connect, incident lifetime (cleared / expired / stale) |

Conflict resolutions (all keep bd6fbe6's llm-advisor and hermes_handoff behaviour):

- `engine.rs` struct/constructor: kept `handoff`, `snap` (api-load) and `repeats` (flap-dedupe) side by side; Stats keep `events_repeat` and the backfill counters.
- `stats_json`: one `json!` with every field hit serde_json's macro recursion limit, so it is built from two `json!` objects merged (same keys and values; serde_json's map is sorted, so output order is unchanged). Backfill counters live in the second object.
- `config.rs`: `restarts_per_unit_hour` default comes from the contract constant (2, flap-dedupe), not 3 (backfill); `incident_quiet_s` / `incident_sticky_max_s` added; both clamps kept.
- `main.rs` env doc lists every variable; `real_t51_chaos.rs` keeps the overridable `cortex_port()` with flap-dedupe's message.
- `real_t237_backfill.rs`: the backfill count now equals every incident the real nerve holds (at least 3). Why: the real nerve also watches the host it runs on, and on this box (disk 88% by df, 93% by the nerve's count) it opened its own `lxc-host` "disk 93% used" incident, which the cortex correctly backfilled as a 4th. The test had hard-coded 3, so it failed 3 times in a row; it passes with the fix.

## Gates (box, 01:57-02:07)

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -D warnings` ok; `cargo clippy -p gaia-cortex --all-targets --features llm-advisor -D warnings` ok.
- `cargo test --workspace --no-fail-fast` (excluding sandbox and cgroup-detect): 240 suites, 1182 passed / 1 failed / 4 ignored. The one failure is `openmind-nerve replay_corpus::every_saved_capture_is_in_the_corpus`. It reads the box's shared `/workspace/swarm/nerve-capture` folder, where other slots saved captures `replay200` and `replay207` that are not in the corpus. This branch doesn't touch `crates/openmind-nerve`; with `NERVE_CAPTURE_ROOT=/nonexistent` the test passes 4/4.
- `gaia-cortex` + `nerve-cortex-contract` with `--features llm-advisor`: all suites green.
- x86_64 musl release build (`--features llm-advisor`) ok, 1,127,200 B, sha256 prefix `c28c96d9f73f701e`; `cargo check` for aarch64 musl, aarch64 android and armv7 musleabihf ok; `scripts/no-python.sh` ok.

## Real tests (release, box loopback, real nerve `openmind-nerve-bb287cc` = live build)

| Test | Time | Result |
|---|---|---|
| `real_t51_chaos` (`GALION_T51_PORT_BASE=21150`, slot 245 range) | 02:00:01-02:02:03 | 2/2 ok. S1-S9 pass: S7 hung nerve gives `/health` and `/stats` 200 in 0 ms; S8 re-subscribes 913 ms after nerve SIGKILL; S9 LLM disabled so 0 calls, spend 0. Teardown: ports 21150-21153 free |
| `real_api_load` (21060-21061) | 02:02:03-02:02:32 | 1/1 ok. p99 /health / /stats: quiet 0.99 / 1.19 ms, flood 3.27 / 2.97 ms, hung decider 3.28 / 3.06 ms (busy 284/284), hung + flood 7.38 / 5.16 ms; 0 failed requests |
| `real_t237_backfill` (21070-21078) | 02:04:55-02:05:20 (after the test fix) | 2/2 ok (live shadow skipped, no live port given). P1 backfill of 3 t237 incidents + lxc-host in 508 ms (R11, 0 actions); P2 tuner incident resolved_by cleared in 505 ms; P3 nerve resolve reaches the cortex in +1032 ms; P4 nerve restart: b reconciled in 1562 ms, c remapped without a second alert; 0 executed, 0 LLM calls |
| `real_flap_dedupe` (21050-21059) | 02:02:39-02:03:38 | 1/1 ok. AFTER: R2-circuit-open after 3 folded writes, 2026 ms; caps dry-run 2/unit then R3-cap; total 6/h then R3-cap-total; live mode same caps |

Logs are in `/workspace/swarm/logs/cortex-live-merge-2/` on the box.

## Live VPS, read-only (02:05:36)

- `gaia-cortex` runs `/opt/galion/rust/bin/gaia-cortex-bd6fbe6` (active, NRestarts 0, started 2026-10-21 23:36:53 +02:00). It is in live mode with the LLM on (1 call, $0.000031), Hermes handoff on (3 handoffs, 3 escalations open), 34 events in.
- The live cortex shows **0 open incidents**, while the nerve `/incidents` holds 9 open ones (ddos:edge-cloudflared x3, edge x2, hermes-rs x2, gaia-cortex, galion-one). This is the gap slot 237 fixes: the cortex never learned about incidents the nerve opened before it subscribed.
- `/etc/gaia-cortex/config.json` pins `"restarts_per_unit_hour": 3` and `"actions": "dry-run"` (the drop-in env `CORTEX_ACTIONS=live` overrides that). All its keys exist in the merged Config (`deny_unknown_fields` is safe: the new fields are additions only).

## Deploy checklist (for the owner or parent; this slot does NOT deploy)

1. Artifact: `/workspace/swarm/artifacts/gaia-cortex-<sha>-x86_64-musl` (built with `--features llm-advisor`, the same features as bd6fbe6). Install as `/opt/galion/rust/bin/gaia-cortex-<sha>`, keeping `gaia-cortex-bd6fbe6` for rollback.
2. Drop-in: a new `ExecStart=` line for the new binary; keep `CORTEX_ACTIONS=live`, `CORTEX_LLM_ENABLED=1`, `CORTEX_HERMES_HANDOFF=1`, `CORTEX_HERMES_PER_HOUR=6`.
3. Decide **D-245-1 caps**: the live config pins 3/unit/h, which overrides the new default of 2 (matching the nerve executor cap, D-235-1). To get 2, remove the key or set it to 2. The 6/h total cap applies in any case (default).
4. Decide **D-245-2 backfill burst**: on its first connect the new cortex backfills about 9 open nerve incidents. Each one gets an R11 alert to the nerve (no actions). Critical ones escalate and can be handed to Hermes, capped at 6/h with a per-subject window. Expect a one-off batch of alerts and handoffs right after the restart.
5. Repeat watch: `repeat_poll_s` defaults to 30 s. While a failure event is being watched, the cortex makes a loopback `GET /evt` to the nerve every 30 s; when idle it makes no calls. Set `CORTEX_REPEAT_POLL_S=0` to turn it off.
6. Incident lifetime: one-shot subjects expire after `incident_quiet_s` 1800 s. Sticky subjects (units, nerve incidents, thresholds) never go stale (`incident_sticky_max_s` = 0) unless the owner sets a TTL (D-237).
7. After the restart, check: `/health` ok with `hermes_handoff` true; `/stats` `nerve_incidents_backfilled` equals the nerve's open count, `incidents_open` > 0, `caps` as decided, `repeat_watch.poll_errors` 0, `actions_executed` unchanged; nerve `/state` cortex stream connected; `journalctl -u gaia-cortex` shows the "repeat-poll 30s" start line.
8. Rollback: point `ExecStart=` back at `gaia-cortex-bd6fbe6`, then `daemon-reload` and restart.
