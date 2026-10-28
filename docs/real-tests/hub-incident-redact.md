<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# hub-incident-redact (slot 435), 2026-10-28 CET (+01:00)

Branch `swarm/hub-incident-redact` off `nerve-merge-8-sensors` 674482f1.
Owner items D-422-2 (redact) and D-422-1 (fold, staged default-OFF) from
slot 422 (realtest-goldens-refresh a7962d7b).

## What changed

- New `crates/openmind-nerve/src/redact.rs`: `redact()` masks IPv4, IPv6
  (incl. `%zone`), `ip:port`, `[v6]:port` -> `<addr>`; `host:port`,
  `localhost`, dotted host names -> `<host>`. File/unit suffixes
  (`server.rs`, `galion-nerve.service`, ...) stay; times, `gaia:t158`,
  `e.g.` stay. Errs on masking (`word:42` -> `<host>`). 5 unit tests.
- `hub.rs`: `Hub::ingest` redacts `msg` and `name` (covers `POST /event`
  via `server.rs event_signal` and `POST /signal`); `incident_signals`
  redacts the `last:` text again. So hub `recent` in `/state`, incident
  messages and self-eval deductions carry no raw address.
- `event_feed.rs`: titles (`clean_title`) and details (`short`) also go
  through `redact` (the old heuristic missed IPv6, `host:port`, hosts).
- D-422-1 staged: `NERVE_HUB_EVENT_FOLD` (config `hub_event_fold`, default
  off; start log line only when on). With it on, a `POST /event` hub signal
  opens its incident under the event-feed cause `gaia:<src>` (exact same
  cleaning as event-feed, unit-tested for parity) and the event-feed
  component (`data.top.component` / `data.component` / source), so one
  pushed error = one incident, one component, 15 points. `/signal` pushes
  from the same source keep their own group.
- `tests/real_cortex_merge_all.rs`: refreshed from a7962d7b (one gaia +
  at most the hub incident, unrelated host deductions ignored, 180 s
  score-back), adapted to this lineage (`base_from_env`, no `Block`); plus
  no `10.0.0.5` anywhere in `/state` at step 3, and `NERVE_HUB_EVENT_FOLD=1`
  in the test env tightens step 3 to exactly one 15-point incident.
- New `tests/real_hub_incident_redact.rs` (ports 23050-23059,
  `HUB_REDACT_REAL_PORT_BASE`, `HUB_REDACT_NERVE_BIN` for negative control).

## Real tests on the box (release build)

| When (CET) | Test | Result |
|---|---|---|
| 23:17:31-23:18:23 | real_hub_incident_redact (23050 off, 23051 on) | PASS. A fold off: 0 leaks in 8 views (/state 11,630 B, 56 `<addr>` 24 `<host>`), 2 incidents 15+15 (gaia:t435 + t435) score 70 C. B fold on: 0 leaks, exactly 1 incident `gaia:t435` 15 pts, score 85 B; score = 100 - sum |
| 23:18:28-23:21:59 | real_cortex_merge_all fold off, base 23050 | PASS 135 s: warn ddos -4 (title `edge flood on <addr>`), error: gaia:t158 + hub t158 (15+15) -> 70, no 10.0.0.5 in /state, resolved 120.3 s. Unrelated box `lxc-host` disk failing-soon appeared meanwhile (ignored by design) |
| 23:21:59-23:24:15 | real_cortex_merge_all fold on (`NERVE_HUB_EVENT_FOLD=1`) | PASS 135 s: step 3 exactly one `gaia:t158` incident 15 pts -> 85, resolved 120.3 s, back to 100 A |
| 23:27:04 | NEGATIVE: real_hub_incident_redact with base 674482f1 binary | FAIL as expected: `fold off leaks` 52 (raw 10.0.0.5, 198.51.100.4, 2001:db8::7, fe80::1, edge.example.org, nerve.lan, :8443, :8670 in /state, /incidents, /signals, ...) |

Views checked: `/api/v1/state`, `/incidents`, `/signals`, `/cortex`,
`/cortex/events`, `/self-eval.json`, `/alerts`, `/`. `GET /api/v1/events`
is the raw event log (evidence store, loopback) and keeps the data as
pushed, by design; not changed (F-435-2).

## Gates

- `cargo fmt --all --check` ok; clippy `-D warnings` workspace (excl
  openmind-sandbox, cgroup-detect) all targets ok; nerve
  `--no-default-features` clippy ok.
- `env -u GALION_REAL cargo test --workspace --no-fail-fast` (excl
  sandbox, cgroup-detect, hermes-tools): 400 suites 2105/0/18. nerve lean
  (`--no-default-features`) 279/0/0. nerve lib 268 (+9 new) all pass.
- Flaky, pre-existing, untouched crate: `room`
  `default_and_fixture_do_not_open_a_socket` failed 1 of 4 runs (socket fd
  count race with a parallel test), passed in the final ws run (F-435-3).
- no-python ok; spdx-scan 782 scanned, 0 missing; repo-check 0 errors,
  88 warnings (none new; event_feed test lines only moved).
- x86_64 musl release 2,028,352 B vs base 674482f1 2,016,064 B (+12,288 B,
  68,800 B under 2 MiB); aarch64 musl 1,663,936 B vs 1,657,816 B
  (+6,120 B); armv7 musleabihf and aarch64-android `cargo check` ok.

## VPS (read-only, 23:27:36)

galion-nerve and gaia-cortex active; `GET 127.0.0.1:8670/api/v1/state` 200
31,949 B, mode live, cycle 25, 68 D, open incident causes `edge`,
`galion-one`. IPv4-looking tokens in /state (127.0.0.1 excluded): 0; in
deductions 0; hub recent: 0 (counts only, nothing printed). The live nerve
has no event-feed sensor, so D-422-1 double count is not live today.
Nothing changed on the VPS. NO deploy.

## Merge order

430 nerve-merge-9 (local HEAD d22b1864 at 23:3x) touches none of this
branch's 9 files (file-set compare; a `git merge-tree` rehearsal was
blocked by the safety check and not retried). Land 430 first, then 435;
expected conflict-free.

## Findings / owner decisions

- D-435-1 (= D-422-2): land the redaction (default on, no switch).
  Recommend yes.
- D-435-2 (= D-422-1): turn on `NERVE_HUB_EVENT_FOLD=1` when event-feed
  goes live (one cause scored once, 15 not 30). Recommend yes, together
  with the event-feed deploy; code is default-off so landing is safe.
- F-435-1: with the fold on, the incident title is the hub text (`tunnel
  pushed 1 error(s) ... last: {json}`), not the event-feed title (`explain`
  takes the last worst signal). Cosmetic; can prefer the event-feed signal
  in a follow-up.
- F-435-2: `GET /api/v1/events` returns raw pushed data (by design).
  Decide whether to redact it too (would lose evidence) - recommend leave.
- F-435-3: flaky `room` fd test (pre-existing).

Third-party: none.
