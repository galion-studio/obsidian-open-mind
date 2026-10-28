<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex-score-v2 (slot 108, ports 19780-19789), 2026-10-21 +02:00

Box only. VPS not touched (score is pure computation; nothing to probe read-only that would differ).

## 1. `tests/real_score_v2.rs` (`GALION_REAL=1`, release binary), 01:36 +02:00

`GALION_REAL=1 cargo-slot test -j 2 --release -p openmind-nerve --test real_score_v2 -- --nocapture`

```
replay fixture: score 21 grade "F" pass false; 7 deductions, findings covered 5 / scored 2; ddos firing true
replay1 live capture: score 61 grade D (unchanged); findings covered 4 / scored 1
serve :19780 cycle 1 (strays up): score 71 grade "C" pass false; deductions [25, 4]; incidents ["\"dead-svc\" \"critical\""]; findings covered 0 / scored 1
serve :19780 cycle 2 (strays gone): score 75 grade "C" pass false; findings covered 0 / scored 0
test real_score_v2_on_the_box ... ok
```

Every report is checked against the invariant: covered findings have no own deduction, every other non-info
finding is in exactly one deduction, deductions sum to `100 - score`. The stream flood `ddos:tuner` costs 25
exactly once. Two stray listeners = one finding = one 4-point deduction, gone when the strays close.

## 2. Cross-check on branches that produce ddos-kind findings (scratch clones, not pushed), 01:37-01:39 +02:00

`src/score.rs` from this branch copied unchanged onto each branch; both build and keep their full
`openmind-nerve` test suites green (cortex-t20-merge ee75f75: 119 lib + 29 integration tests; cortex-event-ingest
7a55e6c: 80 lib + 18 integration tests).

### a. sshd credential flood WITH its signal (cortex-t20-merge ee75f75, `replay fixtures/cortex-sshd`, `NERVE_SSHD=1`)

| | score | deductions |
|---|---|---|
| v1 | 67 D | 25 critical incident `ssh-flood`, 4 vuln, 4 vuln |
| v2 | 67 D | identical (ddos finding `sshd-credential-flood` covered by `ssh-flood`; verdict adds `1 ddos finding(s)`) |

No double counting: the flood finding and its incident cost 25 once.

### b. Pushed B33 ddos events (cortex-event-ingest 7a55e6c, `serve` + `NERVE_EVENT_FEED_FILE`)

Feed: `gaia ddos/warn "request burst on tuner"` (no cause, no signal) and `hermes ddos/crit "login flood"`
(cause `gaia:hermes` + signal). v1 on :19785, v2 on :19786, `/api/v1/self-eval` at cycle 2:

| | score | deductions |
|---|---|---|
| v1 | 71 C | 25 incident `gaia:hermes`, 4 stray listener (the other nerve) |
| v2 | 67 D | 25 incident `gaia:hermes`, 4 stray listener, **4 `ddos (warn): gaia: request burst on tuner`** |

The warn ddos finding now scores on its own (v1 ignored it); the crit one is still scored once, as its incident.
Both nerves were stopped after the read (0 processes left).
