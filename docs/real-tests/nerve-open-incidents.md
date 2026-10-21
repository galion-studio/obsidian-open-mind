<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Live nerve + cortex open incidents, tracked to closure (Nerve T6.1, wave-4 slot 233)

Run: 2026-10-22 00:07 to 00:28 CEST (+02:00), Europe/Zagreb. READ-ONLY: only HTTP GETs on
127.0.0.1 over ssh, `systemctl status`, `journalctl` and `ss -ltnp` reads on the VPS. No POSTs,
no restarts, no writes on the VPS. Branch `swarm/nerve-t61-incidents` on aa77864 (docs only).
Slot ports 21030-21039 were not needed (no box binaries started).

## What is live (checked 00:07:35 +02:00)

| service | port | build | state |
|---|---|---|---|
| galion-nerve (event mode) | 8670 | openmind-nerve bb287cc (`mode: event`, `heartbeat_s 900`, `dedupe_s 600`) | started 2026-10-21 22:56:55 +02:00, score 61/D degraded |
| gaia-cortex (live actions, LLM advisor on, Hermes handoff on) | 8674 | gaia-cortex-bd6fbe6 (cortex-live-fixes) | started 2026-10-21 23:36:53 +02:00 |
| galion-nerve-shadow (old polling nerve, 30 s) | 8672 | shadow build | cycle 2342, score 60 degraded |

The event-mode nerve runs an engine cycle only on events and on the 900 s heartbeat
(`engine_cycles_total` 10 at 00:07, 11 at 00:11:56, 12 at 00:26:56; `wakes_by` heartbeat 4 -> 5).
This one fact explains most of the wrong numbers below.

## Four captures (all times +02:00)

| capture | nerve health | nerve open incidents | cortex open incidents |
|---|---|---|---|
| C1 00:07:35 | 61/D degraded, cycle 10 | edge (error), galion-one-mouth (error), ddos:edge-cloudflared (warn) | 1: `nerve-incident:ddos:edge-cloudflared` (warn) |
| C2 00:12:38 (after 00:11:56 heartbeat) | **41 failing**, cycle 11 | ddos **upgraded to critical** and absorbed the edge signals; edge (stale, last_seen 23:56:56); mouth | 1 (still the warn mirror, no update seen) |
| C3 00:25:50 | 41 failing | same as C2 | 1 |
| C4 00:27:53 (after 00:26:56 heartbeat) | 45 failing, cycle 12 | ddos critical (rps 11.4), mouth (error); edge **resolved** 00:26:56 | **0** (ddos mirror resolved 00:26:58) |

Recently resolved in the nerve (C1): `inc-fce1759f` ddos warn 23:11:56 -> 23:36:58, `inc-0b674249`
hermes-rs slow request 23:09:16 -> 23:26:56, `inc-5d566881` gaia-cortex pushed errors
23:06:30 -> 23:26:56. Nerve `/escalations`: 2, both closed/delegated (23:06:25 and 23:08:13,
gaia-hermes-handoff.path). Cortex: 0 escalations, 0 handoffs this process, 0 LLM calls,
0 actions requested (ledger total 1 call, $0.000031 spent of $15).

## Classification of every incident and finding

| # | incident / finding | class | evidence | fix link / follow-up |
|---|---|---|---|---|
| I1 | `inc-a763c076` edge: tunnel routes 6-8 public hostnames to closed origin ports (:3010 :3011 :3020 :4001 :4020 :4060, set drifts per window) | **real fault** (config) | journal: 908 cloudflared ERR lines in 29 min (31/min; nerve says 24.6-33.9/min, correct order); cloudflared counters 7668 errors of 12565 requests (61 %); `ss` has no listener on those ports | owner decision (cloudflared ingress + DNS): slots 118 vps-tuner-dns, 103 tunnel-audit-2, t26-ingresslint. **Closure at 00:26:56 is false**: the fault is still there, its signals moved into the ddos incident (I3). |
| I2 | `inc-d7320b38` galion-one-mouth 503 `loading` | **real fault** (known) + small sensor issue | `:8002/health` 503 `{"loaded":false,"model":"chatterbox",...}`; slot 181 found it is a hard-coded stub with no voice backend; shadow nerve says "stuck 19.6 h" (`inc-495b0a0d`, open since 2026-10-21 04:37:18) while the live nerve says 1.0-1.5 h | fix = slot 181 options (owner: option 1 merge 40454f9 + nerve change recommended). Sensor: "stuck for" age restarts with the nerve (since 22:56:56), see P-233-5. |
| I3 | `inc-eb95d192` "request flood on cloudflared", warn at 23:56:56, **critical** at 00:11:56 and 00:26:56; also `inc-fce1759f` (23:11-23:36) | **sensor bug** (event mode) | ddos stream `edge-cloudflared` is counter-only: `engine.rs` adds the `total_requests` delta since the previous cycle into ONE 10 s bucket (`access.rs record_count`). In event mode the previous cycle is ~900 s ago, so the "last 60 s" holds 15 min of requests: 418/60 = 6.97 (C1 "7.0 req/s", elevated), 612/60 = 10.2 (C2, flood), ~680/60 = 11.4 (C4). Real rate from the counters: 11947 (23:56) -> 12565 (00:12:59) -> 13235 (00:25:50) = 0.6-0.8 req/s. Baseline 0.00 because 167 of 168 buckets are empty, so threshold = `min_rps` 10. The 30 s shadow nerve has **no** ddos incident for the same traffic. | **P-233-1** (new row, highest priority): live score 61 -> 41 "failing" on a phantom critical. |
| I3b | "failing-soon: 596 / 328 / 234 errors in 5 min" (cloudflared component) | **sensor bug** (same cause) | journal at 00:25:50: 234 lines in the last 15 min, 74 in the last 5 min; the nerve reported exactly 234 "in 5 min" at 00:26:56. Journal lines since the last cycle are labelled as a 5 min window. | P-233-1 |
| I4 | `inc-0b674249` hermes-rs "1 slow request: POST /v1/chat/completions 200 in 12035 ms (threshold 1000 ms)" | **threshold** | `nerve-client` `DEFAULT_SLOW_MS = 1000` for every route; an LLM chat completion of 12 s is normal | **P-233-4**: per-route slow threshold (or `NERVE_SLOW_MS` in the hermes-rs unit: owner decision, it is a live unit change). |
| I5 | `inc-5d566881` gaia-cortex "pushed 2 errors; esc-3 R4-unknown-unit unit:gaia-hermes-handoff.path.service" | **real fault, fixed** + **sensor bug** | journal: `gaia-hermes-handoff.path` "Trigger limit hit" -> failed 23:06:25 and 23:08:13 (handoff self-test, cortex e57b946 `claim-failed`); started again 23:08:38, `active (waiting)` since, self-test proposal written 23:08:50 (`notify: no-channel-configured`). The cortex subject `...path.service` is wrong: `nerve-cortex-contract Event::unit()` appends `.service` because `.path` (and `.device`, `.swap`, `.automount`) is not in its suffix list, so a known unit becomes R4 "unknown unit" and is escalated. | fault: closed. Bug: **P-233-3**. Notify channel: owner decision D-233-4. |
| I6 | cortex mirrors only 1 of the 3 nerve incidents (C1) | **cortex gap** (known) | cortex started 23:36:53, after edge and mouth incidents opened (22:56:56) | row 237 cortex-incident-backfill (already planned). |
| I7 | cortex never saw the ddos warn -> critical upgrade (C2, C3) | **contract gap** | the nerve emits `incident.opened` only for new ids (`evt/runtime.rs run_cycle`); a severity change of an open incident publishes nothing, so a real critical would never reach the cortex escalation rule (R11 escalates only on critical) | **P-233-2** |
| I8 | cortex closed the wrong incident at 00:26:58 (C4) | **contract bug** | the nerve resolved `inc-a763c076` (edge); its `incident.resolved` event carries only `{"id"}`, so the cortex subject falls back to `engine:incident.opened` (dec-1 23:37:00 and dec-7 00:26:58), never `nerve-incident:<cause>`; the cortex then resolved its ddos mirror `inc-f8c71ab9` although the nerve ddos incident is open and critical. After C4 the nerve has 2 open (1 critical), the cortex 0. | **P-233-2** (send cause/components in `incident.resolved`, key by nerve id) together with row 237 F3. |
| F1 | vulnerability `unmanaged-listeners`: :8672 :8674 :18800 :19120 "unknown owner" | **known strays / sensor gap** | `ss -ltnp` (read as root): 8672 galion-nerve-shadow, 8674 gaia-cortex-bd6fbe6 (the live cortex), 19120 openmind-queue, 18800 **sshd** (an ssh port forward of an open session, not a Galion service). The nerve cannot read /proc owners for them (unit sandboxing). | row 238 nerve-own-services (8672/8674/19120). 18800: list it as known ssh forward or leave it as a warning, do not add as a component (D-233-3). |
| F2 | info `memory-heavy-console` 1.2 GiB | **known** | largest resident service | row 243 console-v2-audit |
| F3 | component `voice-worker` absent | **known** | no voice service on the VPS (slot 181) | slot 181 options |

Nothing in this list is a new real outage. The two real faults (I1 edge, I2 mouth) are old and
wait for owner decisions. The new problem is that the event-mode nerve itself has been
reporting a phantom **critical** flood since 00:11:56 and the system grade "failing", and
that the cortex closes nerve incidents by mistake.

## Proposed follow-up rows

- **P-233-1 nerve-cycle-gap-windows** (nerve, on nerve-event-driven bb287cc): counter streams
  must spread a counter delta over the time since the previous sample (one bucket per 10 s
  from the previous sample time to now), and window counts ("N errors in 5 min") must use the
  real window, not "since the last cycle". GALION_REAL=1 test: release nerve in event mode,
  fake tunnel metrics that grow 600 requests between two 900 s-apart cycles -> level normal,
  0 incidents, "errors in 5 min" equal to the journal count. Ports from the slot range.
  Redeploy needs a separate approval line (nerve is live).
- **P-233-2 nerve-incident-lifecycle-events** (nerve + nerve-cortex-contract + cortex): emit
  `incident.updated` on a severity change or a regroup (signals moved), put `cause`,
  `components` and `severity` in `incident.resolved`, and have the cortex key nerve incidents
  by nerve id (`nerve-incident:<id>` or a `nerve_id` field) so a resolve can only close its own
  mirror. Also stop resolving an incident when its signals were absorbed into another one
  (I1 false closure): mark it `merged_into`. Merge with row 237 work.
- **P-233-3 contract-unit-suffixes** (nerve-cortex-contract): add `.path`, `.device`, `.swap`,
  `.automount` (all systemd unit types) to `Event::unit()`; regression test with
  `gaia-hermes-handoff.path`. Tiny.
- **P-233-4 nerve-slow-per-route** (nerve-client): a per-route slow threshold (for example
  LLM routes `/v1/chat/completions` 30 s, the rest 1 s) or measure time to first byte.
- **P-233-5 nerve-stuck-age-persist** (nerve): carry the "stuck loading since" time across a
  nerve restart (state dir from slot 223) so the live nerve reports the same age as the shadow
  (19.6 h, not 1.0 h).

## Owner decisions (Slavik)

- D-233-1: run P-233-1 now and redeploy the nerve when it passes (the live grade is wrong
  until then), or switch the cloudflared ddos stream off in event mode as a stop-gap.
- D-233-2: I1 dead tunnel origins: start the 5-8 services or remove their ingress rules / DNS
  (cloudflared config + DNS, owner only).
- D-233-3: :18800 (sshd port forward): accept as a known stray, or find and close the session.
- D-233-4: the Hermes handoff writes `notify: no-channel-configured`, so Hermes cannot notify
  Mate before code changes as required; pick the channel.
- D-233-5: hermes-rs slow threshold (P-233-4 code change, or `NERVE_SLOW_MS` in its unit).

## Commands used (all read-only, over `ssh slavik-vps`)

```
curl -s 127.0.0.1:8670/{health,incidents,state,alerts,escalations,stats,inventory,actions}
curl -s 127.0.0.1:8674/{health,stats,incidents,decisions?n=50,alerts?n=50,escalations}
curl -s 127.0.0.1:8672/{health,incidents}          # shadow nerve, for comparison
curl -s 127.0.0.1:20241/metrics | grep cloudflared_tunnel_total_requests
journalctl -u cloudflared --since "N min ago" -o cat | wc -l     # also grep -c " ERR "
journalctl -u gaia-hermes-handoff.path -u gaia-hermes-handoff.service --since ...
systemctl status gaia-hermes-handoff.path gaia-hermes-handoff.service
ss -ltnp | grep -E ':(18800|19120|8672|8674) '                   # pids not recorded
```

Source lines read for the classification (no code changed on this branch):
`openmind-nerve/src/engine.rs` (tunnel counter delta -> `record_count(now, r - r0, ..)`),
`openmind-nerve/src/access.rs` (threshold = max(min_rps, 5 x median, median + 8 x 1.4826 x MAD),
elevated at half), `openmind-nerve/src/evt/runtime.rs` (`incident.opened` only for new ids,
`incident.resolved` data `{"id"}`), `nerve-cortex-contract/src/lib.rs` (`Event::unit` suffixes),
`gaia-cortex/src/rules.rs` (`subject_key`, R11), `nerve-client/src/lib.rs` (`DEFAULT_SLOW_MS`),
all at nerve-event-driven bb287cc and cortex-live-fixes bd6fbe6.

Raw captures stayed on the box in /tmp (not committed: they hold public hostnames); no secrets
were read or printed.
