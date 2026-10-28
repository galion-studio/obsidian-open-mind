<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# console-gate-wording (wave-4 slot 410, F-396-3 / D-396-3)

Branch `swarm/console-gate-wording`, base `swarm/console-deploy-candidate`
4f763ade (code 5aa31927 = the live console since 2026-10-28 17:35 CET).
Console V2 prep only. **No deploy, no live change.**

## Change

`console-ui` page gate (`Ui::gate`, `GET /api/v1/admin` upstream) used to map
every non-200 to 503 "Console API is not answering". It now splits:

| Upstream answer to `/api/v1/admin` | Page (still 503, fail-closed) |
|---|---|
| 404, 405, TanStack no-route 500 (`Only HTML requests are supported here`) | **Admin sign-in is not set up on this console** |
| `503 {"error":"admin store not configured"}` (5aa31927+ `--local-api` without `--admin-store`) | **Admin sign-in is not set up on this console** |
| connect error / timeout, other 5xx (incl. `admin store unreadable`, `store not writable`), 401/403/429/other | Console API is not answering (unchanged) |
| 200 | unchanged (room or pairing page) |

Same split on the pairing POST `/_ui/pair`: an upstream 404/405/not-configured
now answers 503 "not set up" instead of 401 "That code was refused" (which
would send the owner retrying codes against a console with no admin route).

New finding while testing (**F-410-1**): on 5aa31927 code, `--local-api`
without `--admin-store` no longer answers 404 (that was 2d846fb8); it answers
`503 {"error":"admin store not configured"}`. The row's "404/405 only" split
would have kept the misleading text for exactly the outage shape the current
code would produce, so the JSON `error` value is matched exactly (plain-text
503 bodies containing the phrase do not count).

## Tests (box, 2026-10-28 CET +01:00)

* `console-ui` lib: 110/0 (new `admin_missing_splits_no_route_from_down`,
  `gate_texts_differ_and_fail_closed_copy`; new page in
  `every_emitted_class_is_styled`).
* `console-static` new `tests/ui_gate_wording.rs` (in-process server, one fake
  upstream per status): 3/3 - 404/405/TanStack 500/503-not-configured ->
  "not set up"; 500/502/503/503-unreadable/401/refused -> "not answering";
  on /console, /agents, /services; pair POST 404/405 -> 503, real 401 still
  "refused", dead upstream 503; 200 still pairing page.
* **Real test** `GALION_REAL=1 cargo test --release -p console-static --test real_gate_wording -- --nocapture`
  (ports 22800-22809), 18:39 CET, 1/1 in 550 ms:

```
A1 argv: --bind 127.0.0.1:22800 --dist /tmp/real-410-…/opt/console-dist --ui rust --local-api --pulse 127.0.0.1:22808 --desk 127.0.0.1:22809 --quiet
A1: 5 rooms 503 "Admin sign-in is not set up on this console"
A1: /api/v1/admin 503 not-configured, health 200, loopback /console 200, POST /_ui/pair 503 not-set-up
A2 (upstream 2d846fb8, 404): 5 rooms 503 "Admin sign-in is not set up on this console"
A2: old 2d846fb8 /api/v1/admin 404 + own /console 503 "Console API is not answering" (old text); new gate in front: 503 "Admin sign-in is not set up on this console", POST /_ui/pair 503
B dead upstream: 5 rooms 503 "Console API is not answering"
C upstream 502: 5 rooms 503 "Console API is not answering"
C upstream 405: 5 rooms 503 "Admin sign-in is not set up on this console"
D: candidate argv /api/v1/admin 200, /console 200 pairing page
real_gate_wording A-D pass in 550 ms
```

  A2 uses the byte-identical live binary `artifacts/t291u/galion-console-serve-2d846fb8`
  (sha256 516556f946977d2d…) with the pre-17:35 live argv: the real 404 case,
  and its own 503 page is the negative control (the F-396-3 bug).
* **Negative control**: with the console-ui change stashed,
  `ui_gate_wording` 1 pass / 2 FAIL and `real_gate_wording` FAIL at
  `A1 /console: want "Admin sign-in is not set up on this console"`.
* Gates: `cargo fmt --check` ok; `cargo clippy --workspace --all-targets
  (excl. openmind-sandbox, cgroup-detect) -D warnings` ok; workspace test
  (excl. openmind-sandbox, cgroup-detect, hermes-tools) 149 suites
  723/0/7 ignored; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` missing 0;
  x86_64 musl `galion-console-serve` 1 037 184 B (sha256 422fdffca956b954…),
  aarch64 musl 876 312 B (rust-lld); aarch64-linux-android, armv7-linux-androideabi,
  armv7-unknown-linux-musleabihf `cargo check` ok. No new dependencies.
  Sandbox not involved (sandbox_vps_only n/a).

## Live VPS (READ-ONLY, 2026-10-28 18:40:31 CET)

| Port | Host console.galion.app | code / bytes |
|---|---|---|
| 8080 | /api/v1/admin | 200 / 37 |
| 8080 | /console | 200 / 1145 (pairing page) |
| 18080 | /api/v1/admin | 200 / 37 |
| 18080 | /console | 200 / 1145 |

`console.service` runs `galion-console-serve-5aa31927`, active since
2026-10-28 17:34:00 CET; `console-rust-primary.service` active since 17:30:00 CET.
Public `https://console.galion.app/console` 200. Nerve `:8670/state` 200:
console healthy since 2026-10-28 17:40:13 CET, restarts_1h 0, rss 868 kB.
Cortex not consulted (nerve used). So the new text is not visible live today;
it only shows if the admin store flag is lost again (e.g. rollback to the
2d846fb8 argv), where it now names the real cause.

## Owner decisions

* **D-410-1**: ship this with the next console deploy (no urgent deploy:
  live gate answers 200). Recommend yes, bundled with D-413 (console-deploy-candidate-2).
* **D-410-2**: `admin store unreadable` / `store not writable` 503 still say
  "not answering". Option: a third text "Admin sign-in store cannot be read".
  Recommend later, low value (the store is 0600 and RW-checked by F-313-1).
* **D-410-3**: pairing POST 404/405/not-configured now 503 instead of 401.
  Recommend yes (included).
