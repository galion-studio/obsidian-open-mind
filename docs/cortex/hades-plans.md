<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex T4.2: HADES plan templates per fix family, 2026-10-21 +02:00

Code: `crates/openmind-nerve/src/plans.rs` (generator), 3 lines in `engine.rs` (`state.cortex_plans`),
1 route in `server.rs` (`GET /api/v1/cortex/plans`), `main.rs` (`replay DIR --plans`, `plans STATE.json|-`).
Base: `swarm/cortex-t32-correlate` ae604dc. No new runtime dependency.

## What it is (and is not)

The cortex says what is wrong; T4.2 writes the plan an admin could approve through HADES. **Plans only.**
The generator is a pure function of the nerve's own `/api/v1/state` JSON: it spawns nothing, opens no socket,
writes no file and does not call hades-gate. The nerve publishes the plans on its loopback API and never runs
them; the cortex never runs them. Every plan says `"executed": false` and
`"approval": "owner only: a single-use hades-gate heal-apply approval scoped to this target and the exact plan text"`.

Because the input is the state JSON, the same function runs in the live engine, on a replayed capture
(`openmind-nerve replay DIR --plans`) and on a saved state (`openmind-nerve plans state.json`) - including
the state of the live VPS nerve, which does not have this code.

## Families

| family | trigger (warn or worse) | hades-gate target | the change in the plan |
|---|---|---|---|
| `unit-restart` | `stuck-loading-<c>`, `memory-trend-<c>` | the component's unit; a unit-less component uses its group's unit (`galion-one-mouth` -> `galion-one.service`, said in `notes`) | `systemctl restart <unit>` once |
| `stray-stop` | `unmanaged-listeners` (one plan per port), `stray-owner` | `stray-listener-<port>` | SIGTERM to that one pid, only if **no unit owns it** (precondition) |
| `ingress-rule-removal` | `edge-dead-origins` | `cloudflared` | remove the ingress rules of closed origin ports from the tunnel config, validate, restart cloudflared once |
| `edge-rate-limit` | `flood`/`elevated` stream behind an open `ddos:*` incident, or `edge-host-rate` | `cloudflare-edge` | one Cloudflare rate-limiting rule (managed challenge); a block rule only for a single **public** source |
| `rebind-loopback` | `public-listener-<port>` | the owning unit, else `listener-<port>` | bind address -> 127.0.0.1, restart once |

Plan JSON: `id` (`plan-<family>-<target>`), `family`, `action` (`heal-apply`, allow-listed in hades-gate),
`target`, `incident` (open incident id, or null for finding-only problems), `cause`, `findings`
(`<kind>:<id>`), `severity`, `steps`, `plan_bytes`, `notes`, `executed: false`, `approval`.
Wrapper: `{executed: false, action, count, by_family, plans, skipped}`.

`steps` is the signed text: `steps.join("\n")` = `hades_gate::adapter::gaia_plan_bytes(steps)`. Every plan has a
first line naming the family, `precondition:` lines (abort if the problem is gone or the situation differs),
the change, `verify:` lines and a `rollback:` line. **Steps carry no timestamp**, so the same problem gives the
same plan hash cycle after cycle (asserted live across two cycles).

## Hard limits (by construction, all tested)

- No step changes ssh, the host packet filter, DNS records, accounts or permissions; nothing is deleted.
  `ingress-rule-removal` says outright that DNS records are a separate owner decision.
- Every plan passes the **real** hades-gate admin-safety review (`safety::review(target, plan)`), which refuses
  words such as `ssh`, `kill -1`, `chmod`, `nft`, `secret`, `token`. (The first draft wrote "no ssh change" in
  two plans and the gate refused them: the review is a real check, not a formality.)
- No plan for protected names: targets and hostnames with a label in `PROTECTED_LABELS` (hades, admin, root,
  ssh, sudo, login, pam, key, token, secret, ...; each one checked to be refused by hades-gate too), port 22,
  tunnels and the host (never restarted by a template).
- No plan for: ssh credential floods and auth-log bleed (`no template touches ssh or the host packet filter`),
  restart loops (another restart does not fix them), deleted binaries, rustsec, drift, hades/sandbox findings,
  scanner probes, dev servers, error rates, host pressure, conn-rate, any unknown finding id, info findings.
  Each warn+ finding without a plan and each open incident without a plan is listed in `skipped` with the reason.
- A ban line is never written for a loopback/private/link-local source (the live test's API flood comes from
  127.0.0.1: it gets the rate limit only, and a note says why).

## Evidence (details: docs/real-tests/cortex-t42-hades-plans.md)

- Box: 5 integration tests + 5 unit tests; `GALION_REAL=1` release run: all 5 families appear live in 5.0 s
  on real sockets, plans are byte-stable across cycles, the nerve ran nothing, fixed problems lose their plans
  in 5.0 s.
- Real data: the live VPS nerve state of 14:47 +02:00 and 5 VPS captures (2026-10-20 19:25 to 2026-10-21 14:48)
  give 3-7 plans each; every one passes the real `hades-gate check` CLI (DryRun, not Refused); a throwaway-key
  approval of the live `galion-one.service` plan is `WouldApply` once and `replayed` the second time.

## Owner decisions (not decided here)

- **D-T42-1 publish plans on the loopback API.** `state.cortex_plans` + `GET /api/v1/cortex/plans`, every
  cycle (< 1 ms; the CLI on the 46 KB live state takes 0.00 s, 3.3 MB RSS). Options: (a) on (current);
  (b) behind `NERVE_PLANS=1`. Recommendation (a): text only, loopback only, and Gaia/T4.4 needs a place to read them.
- **D-T42-2 the mouth restart plan would not fix the mouth.** Live and in every capture the top plan is
  `unit-restart galion-one.service` for `galion-one-mouth` 503 loading. Slot 181 found that 503 is a hardcoded
  stub with no voice service behind it, so a restart changes nothing (its `verify:` step would fail). Options:
  (a) keep the template generic and fix the mouth (slot 181 option 1); (b) a per-component "do not plan
  restarts" list in the components table; (c) both. Recommendation (c), (a) first.
- **D-T42-3 known ports.** Live `unmanaged-listeners` = :8672 and :19120; read-only `ss -ltnp` on the VPS
  (14:53 +02:00) shows `galion-nerve-shadow.service` and `openmind-queue.service`. The stray-stop plans' unit
  precondition aborts for both (correct), but the finding is noise. Options: (a) add `8672,19120` to
  `NERVE_KNOWN_PORTS` in the live nerve unit (live config change: owner approval); (b) enable the
  `proc-owner` sensor so unit-owned ports are known; (c) leave. Recommendation (a) now, (b) with the cortex cutover.
- **D-T42-4 hostnames that must stay.** The live ingress plan lists 8 closed ports / 18 hostnames, including
  galion.studio (:4001), which Slavik wants serving (tunnel-fix-502, 08:27). The plan's precondition asks the
  owner to confirm, but a keep list would be safer. Options: (a) precondition only (current); (b)
  `NERVE_PLANS_KEEP_HOSTS` list that is never unrouted. Recommendation (b).
- **D-T42-5 edge rate-limit numbers.** Limit = max(stream threshold, 4 x baseline) x 10 s per client, at least
  20; managed challenge for 10 min; block rule only for a single public source, review after 24 h.
  Options: approve, or set other numbers. No Cloudflare change is made by any of this.
- **D-T42-6 target naming.** Restart/rebind plans target the exact unit (`galion-one.service`), not the
  component name (`galion-tuner` style used in hades-gate's own examples). Recommendation: the unit name, since
  it is what the operator runs; the gate accepts both.

## Not in this slot

- T4.1 (finding -> Gaia directive) and T4.4 (end-to-end: plan -> gaia -> hades-gate token -> heal-apply on a
  dummy -> nerve record). The plan bytes are already `gaia_plan_bytes`-compatible for T4.4.
- No live deployment: this branch is not on the VPS (`GET :8670/api/v1/cortex/plans` -> 404 at 14:53 +02:00).
