<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: console-port-3 rooms against the live VPS (read-only)

Slot 137 ports 20070-20079 on the box. `crates/console-static/tests/real_port3.rs` (`#[ignore]` + `GALION_REAL=1`):
opens `ssh -N` to the VPS (alias `slavik-vps`, address not in the code) with forwards 20071 -> 127.0.0.1:8080 (console
API) and 20073 -> 8670 (galion-nerve), starts the real release `galion-console-static --ui rust --upstream
127.0.0.1:20071 --bind 127.0.0.1:20070`, compares every room with the live API read through the same forward, reads the
live marker ring before and after (must be unchanged), reads the nerve `/state` before and after, then stops the binary
and the forward. **Only GETs reach the VPS.** Mint, Revoke and Ping are tested against a fake upstream only
(`tests/ui_port3.rs`). Nothing was started, written or changed on the VPS. The cortex is not deployed on the VPS, so the
infrastructure was observed through the nerve only.

```
GALION_REAL=1 cargo test --release -p console-static --test real_port3 -- --ignored --nocapture
```

## Live API, read directly on the VPS (GET, 2026-10-21 05:40 CEST +02:00)

| path | status | size | notes |
|---|---|---|---|
| `/api/v1/health` | 200 | 644 B | healthy, 3.8.3-board |
| `/api/v1/instances` | 200 | 1835 B | 3 nodes, 8 parts (front, hermes, one online; voice, whisper, vision, tulu, laptop offline), link with laptop public key + tunnel command (key not copied here) |
| `/api/v1/laptop` | 200 | 40 B | `{"connected":false,"jobs":[],"lines":[]}` |
| `/api/v1/keys` | 401 | 26 B | `{"error":"phone required"}` (no paired cookie, even from loopback) |
| `/api/v1/markers` | 200 | 31 B | `{"count":0,"cap":42,"items":[]}` |

On the VPS loopback `/api/v1/instances` answers in 24 ms and `/api/v1/laptop` in 19 ms; through the box's ssh forward
each call takes 0.38-0.58 s, which is why each Rust page takes ~0.84 s in the runs below (two parallel calls per page plus
the forward's connection setup). On the VPS itself the page time would be dominated by the ~20 ms API calls.

## Final runs, branch code as committed

Run 1, 2026-10-21 05:50:07-05:50:39 CEST (+02:00):
```
real: forward 127.0.0.1:20071->8080 127.0.0.1:20073->8670 (ssh -N, GET only)
real: nerve before: cycle 6631 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: /instances 200 17956 B in 840 ms
real: /instances ok: nodes 3 parts 8 (3 online) laptop no path jobs 0 lines 0; phone=online pod=online laptop=offline front=online hermes=online one=online voice=offline whisper=offline vision=offline tulu=offline laptop=offline
real: /keys 200 15197 B in 845 ms
real: /keys live 401 (phone required), page says: The key list did not load: HTTP 401 (phone required).
real: /markers 200 14875 B in 843 ms
real: /markers ok: cap 42, 0 stored (live count 0)
real: /instances x5 ms [841, 853, 860, 857, 870]
real: /keys x5 ms [856, 843, 840, 839, 840]
real: /markers x5 ms [615, 845, 842, 843, 843]
real: nerve after: cycle 6632 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static
real: stopped ssh forward
test result: ok. 1 passed; 0 failed
```

Run 2, 2026-10-21 05:50:39-05:51:03 CEST (+02:00): same results (instances x5 837-915 ms, keys x5 840-858 ms, markers x5
838-843 ms; nerve cycle 6632 before and after, the 4 watched components healthy; live ring count 0 before and after).
An earlier run at 05:47:02 CEST (before the clippy/fmt edits) also passed with the same values.

After the runs: no listener on 20070-20079 and no leftover `galion-console-static` / `ssh -N` process on the box.

## What the comparison checks
- `/instances`: one card per live node, laptop job and part (3 + 0 + 8 = 11); each node/part name with the badge tone
  and word the API gave (`mark` or `state`); "3 of 8 online"; laptop badge `no path` (live `connected:false`); the live
  laptop public key appears in the page (compared, not printed); no "did not load".
- `/keys`: the live 401 and its `error` are stated on the page; no "No keys."; the mint form is present; no key material
  on the unpaired page.
- `/markers`: "Ring of 42 · 0 stored" equals the live cap/items; row count equals the live items; live `count`
  unchanged before/after the run.
- No `<script` in any of the 15 timed page loads per run.
