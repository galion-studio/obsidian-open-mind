# RSS baseline

DESK-ID: openmind.B09

Measured: 2026-10-19T16:04:20+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)

Peak RSS is the `Maximum resident set size (kbytes)` value from `/usr/bin/time -v`.
No workspace crate was built. Numbers come from release binaries already present under `target/release`, `/tmp/om-b03/release`, `/tmp/om-b12/release`, or `/tmp/om-b13/release`.
Servers were bound on `127.0.0.1:18771` and then killed. `galion-one` also bound `127.0.0.1:18772` inside a private network namespace so its second listener would not take port 8002. Both listeners were gone when the namespace exited. Host ports 18771 and 18772 were free afterwards.

| Binary | How peak RSS was measured | Peak RSS (kB) |
| --- | --- | --- |
| desk-hermes | `/usr/bin/time -v target/release/desk-hermes`, one-shot. `HERMES_HOME` and `HERMES_STATE_DB` pointed at an empty temp directory and a missing database. Exit 0. | 2432 |
| galion-desk | `/usr/bin/time -v target/release/galion-desk` with `DESK_BIND=127.0.0.1:18771`. Held 2.21s, then SIGTERM. | 1408 |
| galion-one | `/usr/bin/time -v target/release/galion-one` inside `unshare --net` (loopback up). `ONE_NO_WARM=1`, `ONE_BIND=127.0.0.1:18771`, `ONE_MOUTH_BIND=127.0.0.1:18772`, `GALION_HOME` a temp directory. Listeners came up; held 2.83s, then SIGTERM. | 8064 |
| guardian-tick | `/usr/bin/time -v target/release/guardian-tick --probes <temp.toml> --out <temp.json>`. Probe file had only `mem` and `load`. Exit 0. | 1792 |
| link-watch | `/usr/bin/time -v /tmp/om-b03/release/link-watch` with `LINK_WATCH_ONCE=1`. Outputs and `OPENMIND_CONFIG` under `/tmp`. Probe URLs `http://127.0.0.1:18771` (nothing listening). `LINK_WATCH_FRANCE_URL` empty. Exit 0. | 1920 |
| one-defence | `/usr/bin/time -v /tmp/om-b12/release/one-defence --json crates/one-defence/tests/fixtures/health.db` (read-only fixture, not the live database). Exit 0. | 2560 |
| one-talk | `/usr/bin/time -v /tmp/om-b13/release/one-talk` with no arguments (one-shot, dials nothing). Exit 0. | 1536 |
| persona-plex-relay | not measured: no release binary under `target/release`, `/tmp/om-b03`, `/tmp/om-b12`, or `/tmp/om-b13`. Not built. | not measured |
| room | `/usr/bin/time -v target/release/room` with `ROOM_BIND=127.0.0.1:18771`. Indexed, then held 3.21s, then SIGTERM. | 2096 |
