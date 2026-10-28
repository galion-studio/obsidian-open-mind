<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: cortex Hermes handoff exclusion for test units (wave-4 slot 387)

Branch `swarm/cortex-handoff-exclude` off `cortex-nerve-pair-runbook` 4bd62a6b.
Staged only: no live config change, no deploy. The cortex never calls OpenRouter.

## Why

Live-watch 2026-10-28 07:08 +01:00: throwaway swarm test units from slot 370
(`noloop370-t1..t5-20261028062409.service`) crash-looped 06:24-06:32 +01:00 and
were handed to the Hermes boss 6 times (about $0.42). The breakers did their job;
the Hermes calls bought nothing.

## What

`hermes.exclude_units` (config JSON) or `CORTEX_HERMES_EXCLUDE_UNITS` (comma list):
unit globs (`*`, `?`, ASCII case-insensitive) matched on the ticket unit (or the
`unit:` subject). A match means: no inbox file, no rate slot, no dedupe entry.
Everything else is unchanged: rules run, breakers open, alerts go to the nerve,
the incident and the escalation are still logged, with `"handoff":"excluded"` and
`"handoff_excluded_by":"<pattern>"`. `/stats` counts `handoffs_excluded` and shows
`hermes_handoff.exclude_units`. Subjects without a unit (nerve-incident, threshold)
are never excluded. Default: empty (feature off, behaviour identical to today).

Guard (the cortex refuses to start, exit 2): at most 16 patterns, 96 bytes each,
chars `A-Za-z0-9@._:-*?`, at least 4 literal characters before the first wildcard,
and no pattern may match a production unit (reflex allow/deny lists, guard
protected units, the 30 live VPS services, each as `name` and `name.service`).

## Evidence

All times Europe/Zagreb (+01:00).

1. Corpus replay (`tests/handoff_exclude_corpus.rs`, CI): 37 real escalations read
   from the live VPS `escalations.jsonl` at 13:28 (fixture
   `escalations-live-20261028.jsonl`). Off: 30 written / 4 duplicate / 2
   rate-limited, the same as the live log; 9 test-unit handoffs. With
   `noloop*,sbxtest*,swarmtest-*`: 13 excluded, 22 written, 2 duplicate,
   0 rate-limited; every real handoff (hermes-rs, gaia-hermes-handoff.path,
   nerve-incident console/ddos, threshold) is still handed.
2. Real end to end (`tests/real_handoff_exclude.rs`, `GALION_REAL=1`): real
   nerve binary + release cortex on loopback 22570 (nerve) / 22571 (cortex) /
   22579 (dead probe). Input: 227 real systemd journal lines of the slot-370
   crash loop (VPS journal 06:23:30-06:32), replayed 8 times, plus a control
   unit `p387ctl-20261028.service` crashing 3 times.
   - Off: 5 test tickets in the inbox; control escalations R4 and R2 rate-limited
     (the test units used the slots); 5 test circuits open.
   - On: 0 test tickets, 10/10 test escalations `excluded`, `handoffs_excluded=10`,
     5 test circuits still open, control R4 and R2 handed, `rate_limited=0`.
   - Refused at start, exit 2 in at most 1 ms: `hermes*` (matches
     `hermes-rs.service`), `*`, `galion-*` (matches `galion-backup`), `*test*`.

   | Run | Nerve | Where | Result |
   |---|---|---|---|
   | 13:49 | live 9d1a4f3 (sha matches VPS binary) | box | pass |
   | 13:55-13:56 | live VPS binary file, as `nobody`, throwaway `/var/tmp` dir (removed) | VPS | pass, 108.4 s |
   | 13:58-13:59 | staged 9baa450 | box | pass, 108.4 s |

3. Staged drop-in `92-hermes-exclude.conf`: `systemd-analyze verify` on a
   throwaway copy of the live unit plus all live drop-ins plus the staged file,
   as `nobody` on the VPS at 13:57: exit 0, no warnings. Live cortex untouched
   (same PID, NRestarts 0, started 2026-10-27 18:17:08 +01:00).
4. Gates: fmt, clippy `-D warnings` (workspace and `gaia-cortex --features
   llm-advisor`), workspace tests 1228 passed / 0 failed / 4 ignored, musl
   x86_64 (+llm) and aarch64 static builds.

## Owner decisions

- D-387-1: install the staged drop-in. It needs a cortex built from this branch
  first. Use the env var only: the running binary rejects unknown keys in
  `config.json` `hermes` and would refuse to start. The old binary ignores the env var.
- D-387-2: exclusion is per subject. An R0 stack-wide escalation whose subject is
  a test unit is excluded; real units in the same burst get their own escalations.
- D-387-3: name future throwaway units `swarmtest-*` so one pattern covers them.
- Not built: a dispatcher-side filter (the cortex filter is enough).
