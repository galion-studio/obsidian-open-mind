<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: survival-heal (guardian-tick `heal` / `classify`)

Run on 2026-10-20 between 19:11 and 19:12 CEST (+02:00) on the BOX by swarm slot 04,
branch `swarm/survival-heal` (base 13a296c). Release build. Ports used: 19040 (healthy
dummy), 19041 (dummy that dies after 3 s); the gated cargo test uses 19045/19046.
No Python was run. Nothing was written, restarted or built on the VPS.

## What was tested

1. Two dummy services (`crates/guardian-tick/examples/dummy_service.rs`, a Rust loopback
   HTTP 200 server; `--die-after-s 3` makes it exit) started on the box.
2. `guardian-tick heal` ticks against them: tick 0 (both up), tick 1 dry-run after the
   dummy died (detection, plan, `would restart`), three ticks with `--allow dummy-dies`
   (real restart through the `command` backend, re-probe verify = 200), a fourth
   `--allow` tick that hits the reflex cap (3 restarts per unit per hour) and does not restart.
3. The append-only incident store and nerve JSONL, all Zagreb `+02:00` stamps.
4. Live VPS, read-only: `ssh root@<vps> 'curl -sS -m 5 http://127.0.0.1:8660/status.json'`
   (one HTTP GET each at 18:51:20 and 19:11:35 CEST; no other command was run there).
   The captured JSON is in `captures/`; `guardian-tick classify` ran on the BOX over it.

Command: `RUN=/workspace/swarm/target/survival-heal/real-run CAPTURE=docs/real-tests/captures/vps-8660-status-20261020.json scripts/real/survival-heal.sh`

## Box run output (verbatim)

```
== 2026-10-20T19:11:03+02:00 start dummies (run dir /workspace/swarm/target/survival-heal/real-run)
== 2026-10-20T19:11:04+02:00 tick 0: both up (dry-run)
guardian-tick heal 2026-10-20T19:11:04+02:00 mode=dry-run cap=3/h
  classify dummy-healthy          healthy  status 200
  classify dummy-dies             healthy  status 200
  plan     dummy-healthy          observe          unit=dummy-healthy reason=healthy
  plan     dummy-dies             observe          unit=dummy-dies reason=healthy
== 2026-10-20T19:11:08+02:00 tick 1: dummy-dies has died -> detection + plan, DRY-RUN (no --allow)
guardian-tick heal 2026-10-20T19:11:08+02:00 mode=dry-run cap=3/h
  classify dummy-healthy          healthy  status 200
  classify dummy-dies             down     connect: Connection refused (os error 111)
  plan     dummy-healthy          observe          unit=dummy-healthy reason=healthy
  plan     dummy-dies             restart          unit=dummy-dies reason=restart 1 of 3 this hour
  action   dummy-dies             mode=dry-run unit=dummy-dies result=would restart via command (not in --allow)
== 2026-10-20T19:11:08+02:00 tick apply-1: --allow dummy-dies
guardian-tick heal 2026-10-20T19:11:08+02:00 mode=apply-allowlisted cap=3/h
  classify dummy-healthy          healthy  status 200
  classify dummy-dies             down     connect: Connection refused (os error 111)
  plan     dummy-healthy          observe          unit=dummy-healthy reason=healthy
  plan     dummy-dies             restart          unit=dummy-dies reason=restart 1 of 3 this hour
  action   dummy-dies             mode=apply unit=dummy-dies result=restarted via command verify_ok=true (status 200)
== 2026-10-20T19:11:13+02:00 tick apply-2: --allow dummy-dies
guardian-tick heal 2026-10-20T19:11:13+02:00 mode=apply-allowlisted cap=3/h
  classify dummy-healthy          healthy  status 200
  classify dummy-dies             down     connect: Connection refused (os error 111)
  plan     dummy-healthy          observe          unit=dummy-healthy reason=healthy
  plan     dummy-dies             restart          unit=dummy-dies reason=restart 2 of 3 this hour
  action   dummy-dies             mode=apply unit=dummy-dies result=restarted via command verify_ok=true (status 200)
== 2026-10-20T19:11:18+02:00 tick apply-3: --allow dummy-dies
guardian-tick heal 2026-10-20T19:11:18+02:00 mode=apply-allowlisted cap=3/h
  classify dummy-healthy          healthy  status 200
  classify dummy-dies             down     connect: Connection refused (os error 111)
  plan     dummy-healthy          observe          unit=dummy-healthy reason=healthy
  plan     dummy-dies             restart          unit=dummy-dies reason=restart 3 of 3 this hour
  action   dummy-dies             mode=apply unit=dummy-dies result=restarted via command verify_ok=true (status 200)
== 2026-10-20T19:11:22+02:00 tick apply-4: --allow dummy-dies (expect cap-reached, no restart)
guardian-tick heal 2026-10-20T19:11:22+02:00 mode=apply-allowlisted cap=3/h
  classify dummy-healthy          healthy  status 200
  classify dummy-dies             down     connect: Connection refused (os error 111)
  plan     dummy-healthy          observe          unit=dummy-healthy reason=healthy
  plan     dummy-dies             cap-reached      unit=dummy-dies reason=reflex cap 3/3 restarts in the last hour
  action   dummy-dies             mode=held unit=dummy-dies result=reflex cap 3/3 restarts in the last hour
== dummy-dies process starts logged: 4 deaths
== incident store /workspace/swarm/target/survival-heal/real-run/state/incidents.jsonl
{"class":"down","detail":"connect: Connection refused (os error 111)","kind":"opened","service":"dummy-dies","severity":"high","ts":"2026-10-20T19:11:08+02:00","unix":1792516268}
{"cap":3,"kind":"restart","mode":"dry-run","n":1,"result":"would restart via command (not in --allow)","service":"dummy-dies","ts":"2026-10-20T19:11:08+02:00","unit":"dummy-dies","unix":1792516268}
{"cap":3,"kind":"restart","mode":"apply","n":1,"result":"restarted via command","service":"dummy-dies","ts":"2026-10-20T19:11:08+02:00","unit":"dummy-dies","unix":1792516268}
{"detail":"status 200","kind":"verify","ok":true,"service":"dummy-dies","ts":"2026-10-20T19:11:09+02:00","unit":"dummy-dies","unix":1792516269}
{"cap":3,"kind":"restart","mode":"apply","n":2,"result":"restarted via command","service":"dummy-dies","ts":"2026-10-20T19:11:13+02:00","unit":"dummy-dies","unix":1792516273}
{"detail":"status 200","kind":"verify","ok":true,"service":"dummy-dies","ts":"2026-10-20T19:11:14+02:00","unit":"dummy-dies","unix":1792516274}
{"cap":3,"kind":"restart","mode":"apply","n":3,"result":"restarted via command","service":"dummy-dies","ts":"2026-10-20T19:11:18+02:00","unit":"dummy-dies","unix":1792516278}
{"detail":"status 200","kind":"verify","ok":true,"service":"dummy-dies","ts":"2026-10-20T19:11:18+02:00","unit":"dummy-dies","unix":1792516278}
{"cap":3,"kind":"cap-hit","service":"dummy-dies","ts":"2026-10-20T19:11:23+02:00","unit":"dummy-dies","unix":1792516283,"used":3}
== nerve /workspace/swarm/target/survival-heal/real-run/state/nerve.jsonl
{"ts":"2026-10-20T19:11:08+02:00","source":"guardian-tick","kind":"incident-opened","severity":"crit","data":{"class":"down","detail":"connect: Connection refused (os error 111)","service":"dummy-dies"}}
{"ts":"2026-10-20T19:11:08+02:00","source":"guardian-tick","kind":"heal-restart","severity":"warn","data":{"cap":3,"mode":"dry-run","n":1,"result":"would restart via command (not in --allow)","service":"dummy-dies","unit":"dummy-dies"}}
{"ts":"2026-10-20T19:11:08+02:00","source":"guardian-tick","kind":"heal-restart","severity":"warn","data":{"cap":3,"mode":"apply","n":1,"result":"restarted via command","service":"dummy-dies","unit":"dummy-dies"}}
{"ts":"2026-10-20T19:11:13+02:00","source":"guardian-tick","kind":"heal-restart","severity":"warn","data":{"cap":3,"mode":"apply","n":2,"result":"restarted via command","service":"dummy-dies","unit":"dummy-dies"}}
{"ts":"2026-10-20T19:11:18+02:00","source":"guardian-tick","kind":"heal-restart","severity":"warn","data":{"cap":3,"mode":"apply","n":3,"result":"restarted via command","service":"dummy-dies","unit":"dummy-dies"}}
{"ts":"2026-10-20T19:11:23+02:00","source":"guardian-tick","kind":"heal-cap-hit","severity":"crit","data":{"cap":3,"service":"dummy-dies","text":"operator needed","unit":"dummy-dies","used":3}}
== 2026-10-20T19:11:23+02:00 classify over captured live status.json: /workspace/swarm/wt/survival-heal/docs/real-tests/captures/vps-8660-status-20261020.json
guardian-tick classify 2026-10-20T19:11:23+02:00 source_checked_at=2026-10-20T18:51:23+02:00 source_ok=true mode=classify-only (nothing applied)
  classify openmind-memory        healthy  unit=openmind-memory.service state=active port=8420 http=200
  classify hermes-rs              healthy  unit=hermes-rs.service state=active port=8642 http=200
  classify link-watch-rs          healthy  unit=link-watch-rs.service state=active port=0 http=-
  classify galion-desk            healthy  unit=galion-desk.service state=active port=8646 http=200
  classify tuner                  healthy  unit=tuner.service state=active port=8650 http=200
  classify tuner-quick            healthy  unit=tuner-quick.service state=active port=0 http=-
  classify one                    healthy  unit=- state=n/a port=8091 http=200
  classify console                healthy  unit=console.service state=active port=8080 http=302
  classify cloudflared            healthy  unit=cloudflared.service state=active port=20241 http=200
  classify journald               healthy  unit=systemd-journald.service state=active port=0 http=-
  classify ssh                    healthy  unit=ssh.service state=active port=22 http=tcp-open
  classify galion-backup.timer    healthy  unit=galion-backup.timer state=active port=0 http=-
  plan     openmind-memory        observe          unit=openmind-memory.service reason=healthy
  plan     hermes-rs              observe          unit=hermes-rs.service reason=healthy
  plan     link-watch-rs          observe          unit=link-watch-rs.service reason=healthy
  plan     galion-desk            observe          unit=galion-desk.service reason=healthy
  plan     tuner                  observe          unit=tuner.service reason=healthy
  plan     tuner-quick            observe          unit=tuner-quick.service reason=healthy
  plan     one                    observe          unit=- reason=healthy
  plan     console                observe          unit=console.service reason=healthy
  plan     cloudflared            observe          unit=cloudflared.service reason=healthy
  plan     journald               observe          unit=systemd-journald.service reason=healthy
  plan     ssh                    observe          unit=ssh.service reason=healthy
  plan     galion-backup.timer    observe          unit=galion-backup.timer reason=healthy
== 2026-10-20T19:11:23+02:00 teardown
```

Teardown check right after: `ss -ltn | grep :1904` → nothing; `pgrep -fa dummy_service` → nothing.
`dies.log` shows 4 deaths: the first start plus the 3 restarts guardian-tick applied.

## Live VPS (read-only) at 19:11:35 CEST: what heal WOULD do

All 12 galion-health checks were OK, so the plan is `observe` for all 12 (no restart, no refusal).
```
guardian-tick classify 2026-10-20T19:12:29+02:00 source_checked_at=2026-10-20T19:11:35+02:00 source_ok=true mode=classify-only (nothing applied)
  classify openmind-memory        healthy  unit=openmind-memory.service state=active port=8420 http=200
  classify hermes-rs              healthy  unit=hermes-rs.service state=active port=8642 http=200
  classify link-watch-rs          healthy  unit=link-watch-rs.service state=active port=0 http=-
  classify galion-desk            healthy  unit=galion-desk.service state=active port=8646 http=200
  classify tuner                  healthy  unit=tuner.service state=active port=8650 http=200
  classify tuner-quick            healthy  unit=tuner-quick.service state=active port=0 http=-
  classify one                    healthy  unit=- state=n/a port=8091 http=200
  classify console                healthy  unit=console.service state=active port=8080 http=302
  classify cloudflared            healthy  unit=cloudflared.service state=active port=20241 http=200
  classify journald               healthy  unit=systemd-journald.service state=active port=0 http=-
  classify ssh                    healthy  unit=ssh.service state=active port=22 http=tcp-open
  classify galion-backup.timer    healthy  unit=galion-backup.timer state=active port=0 http=-
  plan     openmind-memory        observe          unit=openmind-memory.service reason=healthy
  plan     hermes-rs              observe          unit=hermes-rs.service reason=healthy
  plan     link-watch-rs          observe          unit=link-watch-rs.service reason=healthy
  plan     galion-desk            observe          unit=galion-desk.service reason=healthy
  plan     tuner                  observe          unit=tuner.service reason=healthy
  plan     tuner-quick            observe          unit=tuner-quick.service reason=healthy
  plan     one                    observe          unit=- reason=healthy
  plan     console                observe          unit=console.service reason=healthy
  plan     cloudflared            observe          unit=cloudflared.service reason=healthy
  plan     journald               observe          unit=systemd-journald.service reason=healthy
  plan     ssh                    observe          unit=ssh.service reason=healthy
  plan     galion-backup.timer    observe          unit=galion-backup.timer reason=healthy
```

## Synthetic what-if over the same capture (NOT live data)

`captures/SYNTHETIC-whatif-3-down.json` is the 19:11 capture with three rows edited by `sed` on
the box (hermes-rs and tuner `failed`, one HTTP 503) to show the plan on a bad day:
```
guardian-tick classify 2026-10-20T19:12:29+02:00 source_checked_at=2026-10-20T19:11:35+02:00 source_ok=false mode=classify-only (nothing applied)
  classify hermes-rs              down     unit=hermes-rs.service state=failed port=8642 http=refused
  classify tuner                  down     unit=tuner.service state=failed port=8650 http=refused
  classify one                    down     unit=- state=n/a port=8091 http=503
  plan     hermes-rs              refuse-protected unit=hermes-rs.service reason=unit is protected; never restarted
  plan     tuner                  restart          unit=tuner.service reason=restart 1 of 3 this hour
  plan     one                    notify           unit=- reason=down: no allow-list rule
  action   hermes-rs              mode=would unit=hermes-rs.service result=unit is protected; never restarted
  action   tuner                  mode=would unit=tuner.service result=WOULD restart tuner.service (dry-run; classify never applies)
  action   one                    mode=would unit= result=down: no allow-list rule
```

hermes-rs is refused (protected list), tuner would be restarted (once the owner adds a
`[[repair]]` rule and `--allow`s it), one has no unit in galion-health and gets notify only.

## Unit and gated tests

`cargo test -p openmind-survival -p guardian-tick` green; `GALION_REAL=1 cargo test -p guardian-tick --test real_heal`
runs the real binary: dry-run never runs the backend, three applies run it exactly three times, the fourth tick is `cap-reached`.

> **2026-10-21 (realscript-port-3):** the shell driver for this evidence was ported to Rust and deleted; re-run with the matching test in `crates/galion-realtest/tests/` (see `docs/real-tests/realscript-port-3.md`).
