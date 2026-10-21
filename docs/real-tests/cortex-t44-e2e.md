<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: cortex T4.4 heal path end to end (slot 222)

Branch `swarm/cortex-t44-e2e`, base `swarm/cortex-t42-hades-plans` 1ce9737.
Box ports 20920-20929 only. Design and owner decisions: `docs/cortex/heal-e2e.md`.
Nothing was written, started or changed on the VPS (read-only GETs and
`systemctl show` only; see section 3).

## 1. Command

```
cargo build --release -p hades-gate            # the gate binary next to the nerve binary
NERVE_T44_LIVE_PLANS=<box copy of live plans>  # optional
GALION_REAL=1 cargo test --release -p openmind-nerve --test real_cortex_t44 -- --nocapture --test-threads 1
```

Processes: `openmind-nerve serve` :20920, `hades-gate serve` :20921 (throwaway
key made with `hades-gate keygen` in a temp dir), dummy `t44-dummy.service`
(the test binary re-executed) :20922, stuck `t44-ssh-relay` :20923, stray
listener :20924; :20929 stays closed (unreachable gate). All killed at the end.

## 2. Box runs (2026-10-21 23:01 / 23:02 / 23:03 / 23:09 / 23:10 +02:00, all pass)

Run of 23:03 (with the live plans), ~15.7 s:

```
chain: finding ["bleeding:stuck-loading-t44-dummy"] -> incident inc-c0aa94f4 (cause "t44-dummy", state "open") -> "plan-unit-restart-t44-dummy.service" after 5.7 s (880 B, sha256 4f40ac9d8874466f…)
protected: "bleeding:stuck-loading-t44-ssh-relay" skipped: "protected or unreadable unit name"
refusal paths:
  no token                               -> DryRun
  wrong family (stray-stop, valid token) -> Refused("executor:family")
  token of the stray-stop plan           -> Refused("gate:wrong-scope")
  token for action rollback              -> Refused("gate:wrong-scope")
  family relabeled on a valid plan       -> Refused("executor:family")
  protected target t44-ssh-relay.service -> Refused("executor:protected-target")
    gate on its own: 403 "admin-safety" ("admin-safety: protected target refused (ssh)")
  protected target hades-gate.service    -> Refused("executor:protected-target")
    gate on its own: 403 "admin-safety" ("admin-safety: protected target refused (hades)")
  forged signature                       -> Refused("gate:forged")
  token signed by another key            -> Refused("gate:forged")
  garbage token                          -> Refused("gate:malformed")
  plan text changed after minting        -> Refused("gate:wrong-scope")
  gate disabled, valid token             -> Refused("gate:disabled")
  gate unreachable, valid token          -> Refused("executor:gate-unreachable")
live VPS plans from /workspace/swarm/nerve-capture/t44/live-plans.json: 5
  live plan-ingress-rule-removal-cloudflared -> Refused("executor:family")
  live plan-stray-stop-stray-listener-18800 -> Refused("executor:family")
  live plan-stray-stop-stray-listener-19120 -> Refused("executor:family")
  live plan-stray-stop-stray-listener-8672 -> Refused("executor:family")
  live plan-stray-stop-stray-listener-8674 -> Refused("executor:family")
approved path:
  valid token                            -> Applied { old_pid: 494980, new_pid: 495181 } in 0.36 s
  replay of the used token               -> Refused("gate:replayed")
  fresh token, service already healthy   -> Refused("executor:precondition")
nerve record: 21 event(s) from hades-heal: 1 applied, 1 dry-run, 19 refused; event log 7393 B
nerve: incident inc-c0aa94f4 "resolved" (resolved "2026-10-21T23:03:44+02:00") and plan gone 8.5 s after the restart; plans left ["plan-stray-stop-stray-listener-20924"]
ledger: 13 line(s), 10 refused, 1 dry-run; 21 outcomes total; whole test 15.6 s
```

Also checked in every run: on each refusal the dummy keeps its pid and
keeps answering 503 (nothing ran); exactly one restart in total; the stray
listener still answers; the nerve's `reflex.recent` is empty (the nerve ran
nothing); the gate ledger and the nerve event log contain no token text;
the plan hash is the same in every run (4f40ac9d8874466f…).

Mutation check (23:02): making the executor treat a `DryRun` reply as
approval makes the test fail (`no token` case), so the dry-run path is
really asserted.

## 3. VPS (read-only, 2026-10-21 23:02-23:04 +02:00)

- live nerve `galion-nerve.service` (MainPID 938448, NRestarts 0, active
  since 22:56:55 +02:00): `/healthz` 200, `/health` status ok, system
  degraded, score 76, cycle 2 (event-driven); `/api/v1/events?n=1` 200;
  `/api/v1/cortex/plans` 404 "not found" (T4.2 not deployed).
- live gaia-cortex :8674 `/health`: ok, mode live, llm_enabled true,
  nerve true, incidents_open 0, escalations_open 0, events_in 7.
- live state (29 768 B, box copy only, `nerve-capture/t44/`, not committed)
  through the branch's `openmind-nerve plans`: 5 plans (1
  ingress-rule-removal cloudflared, 4 stray-stop :18800 :19120 :8672
  :8674), 0 unit-restart; skipped `bleeding:error-rate-cloudflared` and
  `incident:galion-one` (manual). Open live incidents: `edge`,
  `galion-one`. All 5 pass the real gate safety review and are refused by
  the executor (family) in section 2.
- Disclosure: the first probe wrote the state into `/tmp/.t44s` on the VPS
  and removed it in the same command (one temp file, nothing else written).

## 4. Gates

`cargo fmt --all --check` ok; `cargo clippy --workspace --exclude
openmind-sandbox --exclude cgroup-detect --all-targets -- -D warnings` 0;
`cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect
--exclude hermes-tools` 1030 passed / 0 failed / 3 ignored (210 suites);
`hades-gate` real_cli 1/1 (GALION_REAL); `scripts/no-python.sh` ok;
`scripts/spdx-scan.sh` 0 missing; repo-check 4 errors / 34 warnings, the
same 4 errors on the base (hard-coded test IPs in `plans.rs` and
`sensors/surface.rs`, not touched here); x86_64 + aarch64 musl release build
of openmind-nerve and hades-gate (aarch64 with `rust-lld`); android and
armv7 `cargo check` ok. Cargo.lock unchanged.
