<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# nerve-recoveries-push (slot 257) — remote verify + consumer_compat re-run

Time zone: Europe/Zagreb (CET +01:00). **No deploy.** Ports assigned 21260-21269
(the `real_consumer_compat` binary still binds its historical 20500-20502 on the box).

## 1. Remote verify: `swarm/nerve-publish-recoveries` @ ae3eee0

Already on GitHub (parent-push / prior debt clear). Confirmed 2026-10-27 17:34 CET +01:00
via GitHub API `GET /repos/galion-studio/obsidian-open-mind/branches/swarm/nerve-publish-recoveries`:

| Field | Value |
|-------|-------|
| Branch | `swarm/nerve-publish-recoveries` |
| Head SHA | `ae3eee0b0de085102f9dcb84634a999acf654fb2` |
| Parent | `bb287cc6e7ea1bd0e5d78aa319a01151d98bd2e2` (nerve-event-driven / live nerve line) |
| Commit subject | nerve-publish-recoveries: unit fault incidents with published recoveries … 2026-10-22T02:11:55+02:00 Europe/Zagreb |
| HTML | https://github.com/galion-studio/obsidian-open-mind/tree/swarm/nerve-publish-recoveries |

Local wt `/workspace/swarm/wt/nerve-publish-recoveries` and this branch tip match that SHA.
**No push was required for ae3eee0.** This slot only adds the verify note + re-run doc.

## 2. GALION_REAL `real_consumer_compat` re-run (on ae3eee0 tree)

```
CARGO_TARGET_DIR=/workspace/swarm/target/nerve-recoveries-push
GALION_REAL=1 nice -n 10 cargo test --release -p openmind-nerve \
  --test real_consumer_compat -- --nocapture --test-threads 1
```

### box_binary — PASS (2026-10-27 17:37:34–17:37:39 CET +01:00)

- Binary: `/workspace/swarm/target/nerve-recoveries-push/release/openmind-nerve`
- Result: **ok** (suite 2 passed / 0 failed; `vps_live` skipped without `GALION_REAL_VPS`)
- Wall ~5.17 s after ~23 s release compile
- Highlights: realtest healthz/dashboard 200; ingest 202/visible/400; nerve-client 100/100;
  burst rejected 10 (hermes-rs rate limit); gaia-live healthy/post/tail ok; gaia /state
  contract Ok((3, 10)); SIGKILL + torn line + restart → 2 readable records; B2 without
  event log → POST /event 404 as expected.

### consumer_compat unit (11) — PASS (same target, release)

`cargo test --release -p openmind-nerve --test consumer_compat` → **11/11 ok** in 0.01 s.

### vps_live (read-only) — FAIL cycle-advance (2026-10-27 17:37:43–17:38:21 CET +01:00)

```
GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p openmind-nerve \
  --test real_consumer_compat vps_live -- --nocapture --test-threads 1
```

- Round 0 and round 1 both saw live `/state` **cycle 497**, mode `live`, system **degraded**;
  healthz/dashboard/health/events 200; `systemctl is-active galion-nerve` = active; ss :8670 = 1;
  gaia contract Ok((14, 10)).
- Panic: `live nerve loop advances` (observed cycles `[497, 497]` over ~38 s).
- **Not a code defect in ae3eee0** — live nerve (still on bb287cc per migration resume) did not
  tick between probes. Recoveries branch is **not deployed** (this slot: NO deploy). Re-check
  after a live nerve cycle advances, or after recoveries cutover (separate owner job).

## 3. Conclusion

| Item | Status |
|------|--------|
| ae3eee0 on GitHub | **YES** (verified) |
| box `real_consumer_compat` | **PASS** |
| unit `consumer_compat` 11/11 | **PASS** |
| vps_live cycle advance | **FAIL** (live cycle stuck at 497; read-only; no write) |
| Deploy | **not done** (forbidden by slot) |

Logs: `/workspace/swarm/tmp/slot257-consumer_compat.log`,
`slot257-consumer_compat-vps.log`, `slot257-consumer_compat-unit.log`.
