<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: cortex T4.2 HADES plan templates (slot 192)

Branch `swarm/cortex-t42-hades-plans`, base `swarm/cortex-t32-correlate` ae604dc. Box ports 20620-20629 only.
Nothing was written, started or changed on the VPS: read-only GETs, `ss`, `journalctl` only.

## 1. Box unit and integration tests (2026-10-21 14:50 +02:00)

`cargo test -p openmind-nerve --test cortex_t42_plans` 5/5, `--lib plans` 4/4:

| test | what it proves |
|---|---|
| `every_family_yields_a_well_formed_plan` | one synthetic state with a trigger per family gives exactly 7 plans covering all 5 families; each passes `hades_gate::safety::review`, `token::valid_target`, uses `heal-apply` (allow-listed), has precondition/verify/rollback, contains no `ssh`/`nft`/`kill`/`rm`/`chmod`/`dig` word and no timestamp; signed bytes = `gaia_plan_bytes(steps)`; the same state twice gives identical plans |
| `unknown_and_excluded_findings_yield_no_plan` | 19 other finding ids (rustsec, drift, hades-*, sandbox-*, scanner, dev-server, deleted-binary, surface-dns, restarts, error-rate, host-pressure, sshd flood + auth-log growth, conn-rate, an unknown id, ...) + a flood stream with no incident + an `ssh-flood` incident -> **0 plans**; every warn+ one listed in `skipped` with its reason; garbage JSON -> 0 plans, no panic |
| `protected_names_never_get_a_plan` | ssh./admin. hostnames, public port 22, unmanaged :22, `hades-gate` stuck loading, cloudflared and the host -> skipped; each `PROTECTED_LABELS` entry is also refused by hades-gate's `review_target` |
| `a_template_goes_through_a_real_single_use_approval` | all 7 plans through `hades_gate::Approver` with a throwaway key (fixed test seed): no token -> DryRun; signed token -> WouldApply (still not executed); same token again -> Refused (replayed); one byte added to the plan -> Refused |
| `golden_replay_yields_its_plans_through_the_engine` | the golden fixture replayed through the engine: 6 plans in `state.cortex_plans`, equal to `from_state(state)`; the self-eval report (golden) has no new key |

Workspace (`cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`, 14:50-14:53 +02:00):
**1040 passed, 0 failed, 3 ignored** (215 suites). `cargo fmt --all --check` ok,
`cargo clippy -p openmind-nerve --all-targets -- -D warnings` ok, `scripts/no-python.sh` ok.
Cross-target: x86_64 musl release 1,569,632 B, aarch64 musl release (rust-lld) 1,315,992 B,
`cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok.

## 2. `GALION_REAL=1` release run on the box (2026-10-21 14:51:14-14:52:02 +02:00)

```
GALION_REAL=1 NERVE_T42_CAPTURES=$N/replay192,$N/replay186,$N/replay179,$N/replay158,$N/replay1 \
  cargo test --release -p openmind-nerve --test real_cortex_t42 -- --nocapture     # 1 passed, 15.35 s
```

### 2a. `openmind-nerve replay DIR --plans` (real binary), each twice: byte-identical both times

| data | plans | families | skipped | ms |
|---|---|---|---|---|
| golden fixture `cortex-replay` | 6 | edge-rate-limit 1 (ddos:tuner), unit-restart 1, ingress 1, rebind 1 (:9999), stray-stop 2 (:8652 :8653) | 5 (scanner probes, cloudflared error rate, galion-desk restart loop; incidents scan:console, galion-desk stay manual) | 5 |
| VPS capture replay192 (14:47-14:48 today) | 4 | unit-restart galion-one.service, ingress (8 closed ports), stray-stop :8672 :19120 | 1 (error-rate-cloudflared) | 33 |
| VPS capture replay186 (12:3x today) | 4 | same 4 | 1 | 18 |
| VPS capture replay179 (09:15 today) | 4 | same 4 (7 closed ports) | 1 | 23 |
| VPS capture replay158 (08:23 today) | 3 | unit-restart, ingress, stray-stop :8672 | 1 | 25 |
| VPS capture replay1 (2026-10-20 19:25) | 7 | unit-restart, ingress, stray-stop :8431 :8432 :8433 :8652 :8653 | 3 | 13 |

Every plan passed hades-gate's admin-safety review. The replay1 stray-stop plans name exactly the five
stray mock gateways the owner had stopped by hand at 04:35 today (CALL-RECORD): the template would have
proposed the same action, gated behind an approval.

### 2b. Live `openmind-nerve serve` on 127.0.0.1:20620 with real sockets

Setup: `t42-voice` (unit t42-voice.service) answering 503 `{"status":"loading"}` on :20621, grace 4 s;
a loopback listener on :20622 owned by no component; 0.0.0.0:20623 (no component) and `t42-api`
(unit t42-api.service) on 0.0.0.0:20624; 800 requests (8 x 100) to the nerve's own API.

```
live: 800 flood requests; all 5 families' plans after 5.0 s:
  edge-rate-limit -> cloudflare-edge [critical] incident "inc-4e353046" findings ["ddos:nerve-api"] (6 steps, 609 B)
  unit-restart -> t42-voice.service [error] incident "inc-9146f57d" findings ["bleeding:stuck-loading-t42-voice"] (9 steps, 880 B)
  rebind-loopback -> listener-20623 [error] incident null findings ["vulnerability:public-listener-20623"] (8 steps, 659 B)
  rebind-loopback -> t42-api.service [error] incident null findings ["vulnerability:public-listener-20624"] (9 steps, 706 B)
  stray-stop -> stray-listener-20622 [warn] incident null findings ["vulnerability:unmanaged-listeners"] (9 steps, 918 B)
  unit-restart plan text:
    plan unit-restart for t42-voice.service: t42-voice answers 503 loading past the grace period
    precondition: t42-voice.service is active and t42-voice still fails its check (127.0.0.1:20621) at approval time; otherwise stop
    precondition: no deploy or other plan for t42-voice.service is running
    record systemctl status t42-voice.service and the last 200 journal lines of t42-voice.service for the owner
    run systemctl restart t42-voice.service once; no second restart without a new plan
    verify: t42-voice.service is active within 30 s and t42-voice answers 200 on 127.0.0.1:20621 within its grace period
    verify: the nerve shows the incident resolving in the next cycles
    rollback: none needed for a restart; if t42-voice.service does not come back, run galion-deploy rollback for t42-voice.service (separate approval)
    nothing is deleted; no config, access, packet filter or DNS change
  reflex.recent [] audit []
  cycle_ms 51 rss 4260 KiB
fixed: stray + voice plans gone after 5.0 s; left: ["plan-edge-rate-limit-cloudflare-edge-nerve-api", "plan-rebind-loopback-listener-20623", "plan-rebind-loopback-t42-api.service"]
```

Also asserted: plan text and `plan_hash` identical across two cycles; the stray on :20622 still answered
200 after its plan appeared (the nerve ran nothing; no reflex action, empty audit); the API flood plan has
**no** ban line (its single source is 127.0.0.1) and a note saying why; `openmind-nerve plans state.json`
on the saved state equals `/api/v1/cortex/plans`; a missing file exits 2. All listeners and the nerve were
stopped and the temp dir removed at the end.

## 3. Real server: the live VPS nerve (read-only)

- 14:47:14-14:48 +02:00 capture `replay192` (kept on the box, not committed: journal lines carry addresses):
  3x `GET 127.0.0.1:8670/state`, `GET :8660/status.json`, `ss -ltn`, `journalctl -o json --since -30min`
  (1434 lines). Live nerve: cycle 609, `degraded`, 3 open incidents (galion-one error, edge error,
  spend-cap:hermes-rs warn); the live build (aa77864) already serves `cortex` in `/state` but without the T3.2
  `correlation` sections.
- `openmind-nerve plans state-3.json` (release binary, this branch) on the **live state itself**: 4 plans in
  0.00 s, 3.3 MB RSS, identical in 3 runs: `unit-restart galion-one.service` (incident inc-3405a091),
  `ingress-rule-removal cloudflared` (incident inc-96f076b4; 8 closed origin ports, 18 hostnames),
  `stray-stop :19120`, `stray-stop :8672`. Without `correlation` sections the generator links plans to
  incidents by cause; it works on the old state shape.
- 14:52:49 +02:00, real `hades-gate` release CLI on the box with a throwaway key in a temp dir (removed after):
  `check` on the live `galion-one.service` plan (908 B, sha256 47bc7c8a...) -> `DryRun` "no token: plan only";
  `mint` + `check` -> `WouldApply`, `executed: false`; same token again -> `Refused` `replayed`, exit 2.
  `check` (no token) on all 17 plans from the golden fixture, replay192 and replay1 -> all `DryRun`, none refused.
  (A first run with the draft text had 3 plans refused for the word `ssh` in "no ssh change": fixed, and the
  word list is now asserted in the tests.)
- 14:53 +02:00 read-only `ss -ltnp` for the two unmanaged ports: :8672 = `galion-nerve-shadow.service`,
  :19120 = `openmind-queue.service`. Both stray-stop plans would stop at their unit precondition: the
  templates are right not to trust "unknown to the nerve" as "stray" (owner decision D-T42-3).
- 14:53:25 +02:00 `GET :8670/api/v1/cortex/plans` -> 404 (this branch is not deployed),
  `/api/v1/state` 200, `/health` 200 (`degraded`, score 61, cycle 619).

Cortex status: the live nerve runs the cortex sensors (aa77864) but not T3.2/T4.2; observed through the
live nerve and by replaying its captures with this branch.
