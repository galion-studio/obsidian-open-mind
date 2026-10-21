<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve T2.3 observe-only specs (slot 200) — real-test evidence

Branch `swarm/nerve-t23-specs`, base aa77864 (live nerve build) + slot 186 c1a9be8
(`replay --reflex`). Box only, nothing deployed, VPS read-only (GET, `ss -ltn`,
`systemctl show`, `journalctl` reads). Ports 20700-20709. Times Europe/Zagreb (+02:00).

## What changed

- Five new nerve specs, all observe-only (`restarts_per_hour 0`, `critical false`,
  none in the reflex allowlist): `openmind-queue` :19120 `/healthz` (POSEIDON),
  `tanga-proto` :8671 `/tanga/` (ELEUTHIA), `galion-nerve-shadow` :8672 `/healthz`
  (AETHER, never probed by a nerve on its own port), `galion-backup` (new
  `Kind::Timer`: `galion-backup.timer` + last run of `galion-backup.service`,
  stale after 13 h, never hard-down), `hades-gate` (HADES; absent on the VPS today,
  reported as info, not a coverage error).
- Known-port list = live VPS `ss -ltn` (20 listeners, all with an owner);
  overridable with `NERVE_KNOWN_PORTS`.
- `replay` reads capture files `systemctl-show.txt`, `systemctl-runs.txt`,
  `http.txt` (captured HTTP `000` = probe down).
- `reflex::ALLOW` unchanged (10 units; diff against aa77864 is empty).
- Goldens (`cortex-replay.expected.{json,md,preserve-order.md}`) and the DST
  fixture updated for the five new components (all `absent: not in this capture`).

## Real test: `crates/openmind-nerve/tests/real_t23_specs.rs`

`GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p openmind-nerve --test real_t23_specs -- --nocapture --test-threads 1`
— 2026-10-21 18:59:40 .. 19:01:21 +02:00: **3 passed, 0 failed** (107.5 s).

1. **Box serve** (:20700 nerve, :20701 real `openmind-queue serve`, :20702 second nerve as
   shadow), mode dry-run: queue healthy 200, shadow healthy 200; queue killed ->
   cycle 4 `degrading` (probe down), reflex recent `[]`, audit after 3 more cycles `[]`;
   queue restarted by the test -> cycle 8 healthy.
2. **Corpus, 0 new reflex actions** (slot 186 corpus: fixture + replay1, 158, 179, 186,
   192, 193, 200; forced dry-run, plain and `--worst-case --stretch 130`; pre-T2.3 table
   vs T2.3 table file vs built-in): restarts / cloudflared / rollbacks / holds / limited
   = **0 / 0 / 0 / 0 / 0 in every row** (48 rows), exit 0. Score moves only where the new
   components appear (e.g. replay179 66/D -> 70/C, the new healthy components count).
3. **Live VPS capture** `nerve-capture/replay200` (19:00:15 .. 19:01:21 +02:00): GET
   19120 200, 8671 200, 8672 200; replay score 70/C. openmind-queue, tanga-proto,
   galion-nerve-shadow healthy (active/running); galion-backup healthy (timer
   active/waiting, last run success 2026-10-21 14:17:52 +02:00, exit 0); hades-gate
   absent (not installed). Exposure: 20 listeners judged, unmanaged `[]`, unknown-owner
   findings `[]`. Dry reflex replay: 0 actions in all 6 rows.
4. **Negative control** (same capture, every new unit failed, health ports 000/503,
   backup last run failed 7 d ago, worst+130): the four present components go
   `degrading` with correct reasons; reflex **0** restarts / rollbacks / holds; 60/D.

Live ports seen (owner): 22 sshd, 25 postfix, 53 systemd-resolved, 8002
galion-one-mouth, 8080 console, 8091 galion-one, 8420 openmind-memory, 8642 hermes-rs,
8646 galion-desk, 8650 tuner, 8660 galion-health, 8670 galion-nerve, 8671 tanga-proto,
8672 galion-nerve-shadow, 18800 forge-link, 19120 openmind-queue, 20241 cloudflared,
20242 tuner-quick (+ loopback duplicates).

Cortex is not live yet; real-time observation was through the live nerve
(127.0.0.1:8670 `/state`, 3 GETs 31 s apart).

## Gates

See the DONE line for slot 200 in `/workspace/swarm/CALL-RECORD.md` (fmt, clippy -D
warnings, crate + workspace tests, musl/android/armv7 targets).

No new dependencies (no `docs/third-party/` entry needed).

## Owner decisions

- D-200-1: hades-gate spec stays `absent` (info) until HADES is installed on the VPS;
  should it become a coverage error after install?
- D-200-2: galion-backup staleness limit 13 h (timer every 6 h, 2 missed runs + slack).
- D-200-3: deploying this build changes the live score (new healthy components count,
  e.g. 66/D -> 70/C); goldens already reflect it.
