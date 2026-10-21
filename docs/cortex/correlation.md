<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex cross-sensor correlation (plan T3.2), 2026-10-21 +02:00

Code: `crates/openmind-nerve/src/incident.rs` (owner of correlation), member floor in `src/score.rs`,
wiring in `src/engine.rs` (`Correlator::update_with`, `explain` arm for `ssh-flood`, `correlated_md`),
`Cortex::members` in `src/cortex.rs`. Base: `swarm/cortex-merge-all` 6bcb882. No new dependency.

## What was missing

- Findings carried a `cause` key, but only the score read it. The incident list (`/api/v1/state`,
  `/api/v1/incidents`, alerts, the self-eval markdown) showed the signals alone: the `edge` incident did not
  say which sensors agreed (dead origins, ingress lint closed routes, DNS/TLS/HTTP surface, cloudflared's own
  error rate), and the score's `covered` list was the only trace.
- Score v2 charged a covered finding **less** than alone when its incident was milder: the fixture's
  scanner incident is `warn` (5 points) while its member `scanner-probes-console` is an `error`
  vulnerability (10 points and blocks `pass` when uncovered). Correlation hid half the cost.
- An sshd credential flood writes one auth-log line per attempt. When that fills the disk, the host's
  `disk N% used` signal opened its own incident next to `ssh-flood`: one story, two incidents, two
  deductions.

## Rules (incident.rs)

| rule | what becomes one incident |
|---|---|
| `same-cause` | signals and findings with one cause key. `edge` = dead origins (sensor `edge`) + ingress lint closed routes (`ingress-lint`, branch cortex-t26-ingresslint, already `cause = edge`) + surface DNS/TLS/HTTP (`surface`) + tunnel components (`components`). `ssh-flood` = sshd credential flood + auth-log growth (`auth.log`). |
| `ddos-target` | unchanged: a `ddos:<stream>` flood at error or worse pulls its target component's signals. |
| `ssh-disk-bleed` | new: an `ssh-flood` signal (warn or worse) **and** an auth-log bleed finding (`bleeding`, cause `ssh-flood`, warn or worse) in the same cycle pull the host's disk-only signals (`degrading: disk 92% used`, `failing-soon: disk full in ~h`) into `ssh-flood`. RAM or any non-disk reason keeps the host signal where it was. |
| `stack-wide` | unchanged; `ssh-flood` is not a hard group (like `ddos:`). |

Members (`Cortex::members`: `<kind>:<id>`, sensor, cause, component, severity) join the incident score v2
charges for them: same cause first, else the first absorbing incident (`ddos:<stream>`, `stack-wide`) whose
signals name the component; `score::covering` checks the members first, so both always agree.

Members **never** open an incident, never add components (no extra-component points) and never change an
incident's severity (alerts, reflexes, `system.status`, verdict counts stay signal-driven).

Every incident's JSON gets an additive `correlation` section; the markdown gets one `- correlated:` line
per incident with members or folds:

```json
"correlation": {"rules": ["same-cause"], "sensors": ["components", "edge"],
  "findings": [{"finding": "bleeding:edge-dead-origins", "source": "edge", "severity": "error"},
               {"finding": "bleeding:error-rate-cloudflared", "source": "components", "severity": "warn"}],
  "folded": [], "worst_finding": {"finding": "bleeding:edge-dead-origins", "severity": "error"}}
```

`cause` stays the incident key string (unchanged for every consumer); the new section explains it.

## Score v2 member floor (score.rs)

An incident costs at least what its worst member would cost uncovered, and a member that would block `pass`
alone still blocks it (the verdict then says `degraded`). Max, not sum: never a double count. Without
members (`Correlator::update`, unit tests) score v2 is unchanged.

## Evidence (details in docs/real-tests/cortex-t32-correlate.md)

| data | before (6bcb882) | after | why |
|---|---|---|---|
| golden fixture `cortex-replay` | 21/F | **16/F** | scanner incident 5 -> 10 (member floor); every other deduction identical (tests apply `support::t32_delta` to the untouched pre-trait golden) |
| live capture replay1 (2026-10-20 19:25) | 61/D | **56/D** | same scanner floor |
| live capture replay158 (2026-10-21 08:23) | 66/D | 66/D | no member costs more than its incident |
| live capture replay179 (2026-10-21 09:15) | 66/D | 66/D | same; `edge` holds edge-dead-origins + error-rate-cloudflared |
| T2.12 edge-flood capture (real_edge_rate) | 11/F | 6/F | scanner floor |
| in-process ssh flood + 1.1 MiB auth log + 92 % disk | flood 25 + host incident | one `ssh-flood` incident, 28 (25 + 3 for `lxc-host`), host incident resolves | `ssh-disk-bleed` |

## Owner decisions (not decided here)

- **D-T32-1 finding-only incidents.** Findings never open an incident. So an `edge` problem seen only by
  surface (DNS) and ingress lint, with no refused dials this window, is one *finding* deduction ("same cause
  `edge`, scored once"), not an incident: no alert. Options: (a) keep (current, conservative); (b) open an
  incident when 2+ different sensors report findings of one cause (corroboration); (c) open at warn at most.
  Recommendation: (b) once the ingress-lint sensor (cortex-t26-ingresslint 036f891) is merged, because two
  independent sensors agreeing is the strongest evidence the nerve has, and it would alert on galion.studio
  502s before visitors' requests fail.
- **D-T32-2 member floor.** Accepting it changes the golden score 21 -> 16 and replay1 61 -> 56. Options: (a)
  floor + pass blocking (current); (b) floor on points only, no pass blocking; (c) no floor (v2 as merged).
  Recommendation: (a): an exposed dev server (error vulnerability) should not get cheaper because a scanner
  also hit it.
- **D-T32-3 ssh-disk-bleed scope.** The fold needs the auth-log bleed finding, which the sshd sensor only
  produces live (it stats the file; replays never read the host). On the VPS today the sensor is not deployed,
  password auth is off, and the 30-min capture has 307 ssh lines and no flood. Options: (a) keep evidence-based
  (current); (b) fold any disk signal during a flood (more folding, more risk of hiding a real disk problem).
  Recommendation: (a).

## Not in this slot

- The ingress-lint sensor lives on `swarm/cortex-t26-ingresslint` 036f891 (not in cortex-merge-all). Its
  findings already use cause `edge` and component `cloudflared`, so they join the `edge` incident with no
  further change once merged (covered by the unit test `dead_origin_ingress_lint_and_dns_are_one_edge_incident`).
- Cortex not deployed on the VPS (`/api/v1/state` 404): observed through the live nerve, read-only.
