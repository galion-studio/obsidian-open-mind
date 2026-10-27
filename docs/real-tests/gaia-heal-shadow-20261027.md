<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: gaia-heal-live-shadow (wave-4 slot 262)

Run: 2026-10-27 17:35 to 17:36 CET (+01:00), Europe/Zagreb. Box ports 21300-21309.
Branch `swarm/gaia-heal-live-shadow`, based on `swarm/gaia-live-classify` 055fe91
(slot 224 classify + heal-tick plan-only tooling).

## Purpose
Read-only **heal-tick plan-only shadow** of live VPS units against the live nerve
(`:8670`) and cortex (`:8674`) **after the console cutover** (console.service now
Node `console-dist` / srvx on `:8080`, ActiveEnter 2026-10-27 17:28:29 CET /
16:28:29 UTC). No apply, no HADES token, no POST, no live cortex deploy, no
`console.service` change from this slot.

## Method
1. Release `gaia` binary from this branch
   (`CARGO_TARGET_DIR=/workspace/swarm/target/gaia-heal-live-shadow`).
2. One `ssh -N -L 127.0.0.1:2130x:127.0.0.1:<port>` tunnel for
   8420 8642 8650 8091 8080 8670 8674 8646 8002 8660.
3. One `ssh … sh -s` evidence batch per run (`systemctl is-active` ×13 +
   `stat` age on `/opt/galion/galion-light/state/link.json`). Stdout only.
4. `gaia classify --port-map … --evidence …` (GET-only probes + nerve `/state` +
   cortex `/health` + `/incidents`; heal-tick **plan-only** per unhealthy unit).
5. Independent `GET http://127.0.0.1:8670/state` (over the same tunnel) for a
   12/12 nerve-component cross-check.
6. Tear the tunnel down; confirm ports 21300-21309 closed.

`heal_tick()` apply path is never called. Every heal-tick block records
`mode:plan-only`, `applied:false`, `token_used:false`, `nerve_event_posted:false`.
Live ports are refused by the real scope guard (`gaia_live::heal::check_target`,
allowed range 19000-19999 only).

## Console cutover context (read-only facts)
| Fact | Value |
|---|---|
| Unit | `console.service` **active/running**, NRestarts=0 (counter reset on unit file change) |
| ActiveEnter / ExecMainStart | 2026-10-27 17:28:29 CET |
| ExecStart | `/opt/galion/tooling/node/bin/node …/console-dist/…/srvx.mjs --host=127.0.0.1 --port=8080 --static=…/static …/server/index.mjs` |
| Unit file mtime | 2026-10-27 17:28:29 CET |
| Description | "Galion console (production build from /opt/galion/console-dist; switched 2026-10-27 from vite)" |
| HTTP via tunnel | `GET /` → **302**, `GET /api/v1/health` → **200** |
| Cutover stops (journal) | 17:25:04, 17:25:06, 17:28:29 CET — stop with main exit 143, then start of new unit |
| Nerve view | component `console` **degrading**: "2 restart(s) in the last hour"; open incident `inc-dc52e389` (warn) |
| Cortex view | warn on console row but **no** open cortex incident for console (`cortex-missed`) |

This slot did **not** change `console.service` or any live binary.

## Live stack (unchanged by this slot)
| Service | Binary / note |
|---|---|
| galion-nerve | `openmind-nerve-bb287cc` serve --mode event (started 2026-10-22 23:46:47 +02:00; cycle 497 at probe) |
| gaia-cortex | `gaia-cortex-bd6fbe6` mode **live**, LLM on, hermes_handoff true |
| Nerve score | 35 / degraded; 5 open incidents |
| Cortex | incidents_open 2; escalations_open 18; handoffs 17 |

## Runs (3/3 green, identical table)
Times CET (+01:00): r1 17:36:17–17:36:21, r2 17:36:25–17:36:28, r3 17:36:33–17:36:36.
Each classify ~3–4 s. Exit 0. Plan-only asserts held on every heal-tick block.
Independent nerve cross-check: **12/12** component statuses matched Gaia's
recorded nerve verdicts.

```
service           gaia  nerve               cortex  agree  heal-tick (plan-only)
openmind-memory   ok    ok(healthy)         ok      true   -
hermes-rs         ok    warn(degrading)     warn    false  port 8642 outside 19000-19999
link-watch        ok    ok(healthy)         ok      true   -
tuner             ok    ok(healthy)         ok      true   -
galion-one        ok    ok(healthy)         ok      true   -
console           ok    warn(degrading)     warn    false  port 8080 outside 19000-19999
galion-nerve      ok    ok(healthy)         ok      true   -
gaia-cortex       ok    unknown(-)          ok      false  -
galion-desk       ok    ok(healthy)         ok      true   -
galion-one-mouth  bad   warn(degrading)     ok      false  port 8002 outside 19000-19999
galion-health     ok    ok(healthy)         ok      true   -
cloudflared       ok    warn(failing-soon)  ok      false  no HTTP port (heal-tick HTTP-only)
tuner-quick       bad   bad(failing)        ok      false  no HTTP port; unit inactive
openmind-queue    ok    unknown(-)          ok      false  -
```

Summary (all 3 runs):
`{"units":14,"agree":7,"disagree":7,"findings":17,"by_severity":{"info":6,"warn":11}}`

Overall:
`gaia=degraded`, `nerve_status=degraded`, `nerve_score=35`, `nerve_open_incidents=5`,
`cortex_mode=live`, `cortex_open_incidents=2`, `cortex_escalations_open=18`,
`cortex_hermes_handoff=true`.

systemd evidence (all runs): 12 units `active`, `tuner-quick.service` `inactive`,
link.json age 2–3 s.

### Heal-tick plan-only (unhealthy / disagreed units)
| service | stage | nerve_restarts_1h | cap_left_for_gaia | reason |
|---|---|---|---|---|
| hermes-rs | scope | 0 | 3 | port 8642 outside test range |
| console | scope | **2** | **1** | port 8080 outside test range |
| galion-one-mouth | scope | 0 | 3 | port 8002 outside test range |
| cloudflared | scope | 0 | 3 | no HTTP port |
| tuner-quick | scope | 0 | 3 | no HTTP port |

HADES plan text for console (not applied): inspect journal → restart after Alpha
approval → rollback path. Cap note: D-L2 combined 3/h; nerve already counted 2
console restarts in the hour of the cutover, so Gaia would have 1 left if apply
were ever allowed (it is not on live ports).

## Findings (live, post-cutover)
1. **Console cutover shows as nerve degrading, Gaia HTTP-ok.** Gaia probes `GET :8080/`
   (Expect::Alive) and sees the unit active → `ok`. Nerve counts the two stop/start
   cycles in the last hour → `degrading` + `inc-dc52e389`. Cortex has a warn row but
   **misses** that incident (`cortex-missed`). Heal-tick correctly refuses scope on
   live :8080. Suggested: treat intentional cutover restarts as operator-ack'd so
   they do not open a warn incident; or have Gaia note recent NRestarts / nerve
   restart counts even when HTTP is alive.
2. **Cortex still misses most nerve incidents** (same class as slot 224 / fixed in
   undeployed `cortex-incident-backfill` / `cortex-live-merge-2` 1305dec). Live
   cortex is still `bd6fbe6`. Missed here: console, gaia-cortex, mouth, cloudflared,
   tuner-quick. Cortex open set was only `http:other` + `nerve-incident:hermes-rs`.
3. **Nerve still blind to gaia-cortex and openmind-queue** as components (slot 224 /
   238 own-services still not on live nerve bb287cc). Nerve nevertheless holds
   incident `inc-8cfd0e18` on cause `gaia-cortex` (cortex pushed an error about
   console.service crash during cutover — R3-known-crash alert path).
4. **Mouth still stuck loading** (~4.8 d, HTTP 503). Gaia `bad` vs nerve `degrading`.
5. **cloudflared failing-soon** (origin errors to closed ports including :18800);
   Gaia cannot see error rate from unit-active alone.
6. **tuner-quick inactive/failing** — Gaia and nerve agree `bad`/`failing`; cortex
   still `ok` (missed).

## Delta vs slot 224 (2026-10-21 23:12 CEST)
| Item | 224 | 262 (post console cutover) |
|---|---|---|
| console | ok / ok / ok, agree | ok / warn(degrading) / warn, disagree + cortex-missed |
| nerve score | 51 | 35 |
| cortex open / escalations | 2 / 1 | 2 / 18 |
| mouth | bad vs degrading | same (now ~4.8 d) |
| heal-tick apply | never | never |
| live cortex SHA | bd6fbe6 (just deployed that night) | still bd6fbe6 |

## Safety / scope
- No POST to nerve or cortex. No token mint. No `systemctl` writes. No files written
  under `/opt/galion/openmind`. No CF DNS / HADES mode change. No live cortex deploy.
- Console journal and `systemctl show` were read-only; secret-like tokens redacted
  in the capture pipeline (none appeared in the 30-line journal sample).
- Box ports 21300-21309 freed after the run.

## Gates on this branch
Docs-only follow-up on classify tip; no new crates or Cargo.lock changes. Release
`gaia` built and used for the three VPS shadows above. Third-party: none added
(see `docs/third-party/gaia-heal-live-shadow.md`).

## Owner decisions (none blocking this shadow)
- D-262-1: whether cutover-driven console restarts should suppress nerve
  `degrading` / incident open (operator ack) vs leave as-is until the hour window
  ages out.
- D-262-2: deploy undeployed cortex bundle (`cortex-live-merge-2` 1305dec ±
  sandbox-oom-info) so `cortex-missed` for console and others clears — **not this
  slot** (live cortex deploy forbidden).
- D-262-3: live nerve own-services for cortex/queue (slot 238) — still pending deploy.
