<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex score v2 (plan T3.3), 2026-10-21 +02:00

Code: `crates/openmind-nerve/src/score.rs` (single owner of the score). Base: `swarm/cortex-sensor` e536446.

## Problem in v1

- Findings of kind `ddos` (`cx.ddos_findings`: sshd credential flood, T2.12 per-hostname edge rate, pushed Gaia
  B33 `ddos` events) were never scored. A ddos finding without a `Signal` cost 0 points: a pushed
  `ddos/warn` event, or any sensor that forgets the signal, was invisible in the score.
- Findings were skipped only when an incident had exactly their `cause`. When correlation rewrites a cause
  (a component's signals pulled into `ddos:<stream>`, or 3+ hard groups folded into `stack-wide`), a finding
  with the old cause was scored again on top of the incident.
- Uncovered findings that share one `cause` (sshd flood + the auth-log disk bleed it causes, both `ssh-flood`)
  were separate deductions.

## v2 rules

| what | points |
|---|---|
| open incident critical / error / warn | 25 (+3 per extra component, max +9) / 15 (+2, max +6) / 5 (unchanged) |
| **ddos** critical / error / warn, not covered | **15 / 10 / 4** (new) |
| vulnerability error / warn, not covered | 10 / 4 (unchanged) |
| bleeding error / warn, not covered | 8 / 3 (unchanged) |
| probe errors, self-check warns | unchanged |

1. **Covered** = an open incident already scores the finding: same `cause`, or an absorbing incident
   (`ddos:<stream>`, `stack-wide`) that lists the finding's component. Covered findings cost 0 and are listed
   in `Score.covered` (`{finding: "<kind>:<id>", by: "<incident cause>"}`, in `Score::to_json`).
2. Uncovered findings sharing a `cause` are **one deduction**: the worst finding's points; `why` ends with
   `same cause `<c>`, scored once: also <kind> <id> (<sev>)`. Causeless findings are scored one by one (v1).
3. Stream verdicts (`ddos.streams`) are never scored directly: the `ddos` sensor already turns `flood` /
   `elevated` into a `ddos:<stream>` signal, so they count once, as the incident.
4. **Pass rule**: no critical/error incident, no uncovered error-level vulnerability **or ddos** finding, no probe
   error, score >= 70. Bleeding still only costs points. The verdict says `degraded` (not `degrading`) when an
   uncovered blocking finding exists, and appends `, N ddos finding(s)` only when N > 0 (so existing goldens
   are byte-identical).

## Evidence

- Golden replay (`tests/fixtures/cortex-replay`) and the live capture replay1: unchanged (21/F, 61/D), same
  deductions. `cortex_replay` tests untouched and green.
- With a signal the flood costs the same as v1 (scored once); without one it now costs its own points.
  Real numbers in `docs/real-tests/cortex-score-v2.md`.

## Open owner decision (calibration, T3.1)

The new ddos weights (15/10/4) and "an uncovered error-level ddos finding blocks pass" are a design proposal.
Alternatives: (a) keep as is; (b) ddos = vulnerability weights (10/4) and never block; (c) block only at
critical. Recommendation: (a), because an untracked active flood is worse than a static exposure and every
sensor that wants incident-level weight can still emit a `Signal`.

## T3.2 update (2026-10-21 09:30 +02:00, swarm/cortex-t32-correlate)

Covered findings are now listed on their incident (`correlation.findings`, see docs/cortex/correlation.md)
and the incident costs at least what its worst member would cost uncovered (member floor; a member that
would block `pass` alone still blocks). Golden fixture 21/F -> 16/F and replay1 61/D -> 56/D, both from the
scanner incident (warn, 5) whose member `scanner-probes-console` is an error vulnerability (10). Owner
decision D-T32-2 in correlation.md.
