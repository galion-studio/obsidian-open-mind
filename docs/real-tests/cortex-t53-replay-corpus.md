<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: cortex-t53-replay-corpus (slot 193), 2026-10-21 +02:00

Box only. The VPS got read-only reads (GETs, `ss -ltn`, `journalctl`); nothing was written, started or changed there.
The cortex is not deployed live (the live nerve `openmind-nerve` aa77864 serves `/state` and `/self-eval`; that is
what was observed). Ports 20630-20639 were reserved; the replay binary opens no socket, so none were used.

## 1. New live capture `replay193` (15:22:15-15:23:32 +02:00)

`ssh slavik-vps` read-only: 3x `curl -s 127.0.0.1:8670/state` 30 s apart (snapshot `ts` 15:21:54, 15:22:24,
15:22:54 +02:00, 15 components each), `curl -s 127.0.0.1:8660/status.json`, `ss -ltn`, `journalctl -o json
--output-fields=UNIT,_SYSTEMD_UNIT,MESSAGE,PRIORITY,__REALTIME_TIMESTAMP --since -30min` (1259 lines). Stored at
`/workspace/swarm/nerve-capture/replay193` on the box (not committed).

## 2. Corpus test, debug build (`cargo xtask replay-corpus --require-live`, 15:28 +02:00)

```
ok fixture-cortex-replay: final 16 "F" held 41 "F" (3 capture + 20 hold cycles, 5 incident(s), 8 finding(s))
ok fixture-cortex-sshd: final 67 "D" held 100 "A" (1 capture + 20 hold cycles, 1 incident(s), 2 finding(s))
ok live-replay1: final 56 "D" held 62 "D" (3 capture + 20 hold cycles, 3 incident(s), 6 finding(s))
ok live-replay158: final 66 "D" held 66 "D" (2 capture + 20 hold cycles, 2 incident(s), 5 finding(s))
ok live-replay179 / 186 / 192 / 193: final 66 "D" held 66 "D" (3 capture + 20 hold cycles, 2 incident(s), 5 finding(s))
replay corpus: 8 replayed, 0 live skipped
corpus unit tests 4/4, replay_corpus 4/4 (incl. drift_is_detected, a_flapping_capture_is_caught)
```

CI simulation (`NERVE_CAPTURE_ROOT=/nonexistent cargo xtask replay-corpus`): 2 replayed, 6 live skipped, exit 0.
Same with `--require-live`: fails, `live-replay1: capture dir /nonexistent/replay1 missing` (exit 1).

Mutation check: `score.rs` `(Vulnerability, Warn) => 4` changed to 5: the corpus fails on all 8 captures, e.g.
`fixture-cortex-replay: verdict drift (12 change(s))`, `$.result.final.score: 16 -> 15`,
`deductions: lost "4 vulnerability (warn): # loopback listener(s) no monitored unit owns: :# :#"`. Reverted.

First bless run caught two real issues, both fixed on this branch:

- the sshd fixture replayed to 100/A with no incident: `Engine::new` read `NERVE_SSHD` from the process environment,
  not from the replay config, so the corpus entry's `NERVE_SSHD=1` had no effect (now `Engine::new_with_env`; the
  fixture gives 67/D with the critical ssh flood, as `cortex_s2a_replay.rs` expects);
- the first flap rule (more than one presence change) called the fixture's `ddos:tuner` a flap (`0111100...`: onset
  at cycle 1, cleared when the hold sends no hits). The rule now counts episodes: clear-and-come-back is a flap,
  onset-then-clear is not.

## 3. GALION_REAL=1, release binary (`real_replay_corpus`, 15:29:23-15:31:13 +02:00): 3/3 pass

```
real fixture-cortex-replay: 24 ms, final 16 "F" held 41 "F", timeline[0..3] ["46 F","16 F","16 F"], flaps 0
real fixture-cortex-sshd: 10 ms, final 67 "D" held 100 "A"
real live-replay1: 29 ms, final 56 "D" held 62 "D"
real live-replay158: 38 ms / 179: 30 ms / 186: 34 ms / 192: 45 ms / 193: 32 ms, final 66 "D" held 66 "D"
real binary: 8 of 8 corpus captures replayed, all match (release output == expected files blessed with debug)
real flap: exit 3, flaps ["incident:hermes-rs=critical 11111000111111111111111111111111", "grade:D ...", ...]
real once: exit 0, flaps []
real VPS capture (35 ms): replay final 66 "D" held 66 "D" incidents {"edge", "galion-one"} flaps [];
  live nerve cycle 695 61 "D" incidents {"edge", "galion-one", "spend-cap:hermes-rs"}
live-only incidents (not reproducible from a /state capture): ["spend-cap:hermes-rs"]
```

The VPS part (`GALION_REAL_VPS=1`) took a fresh read-only capture inside the test (same commands as section 1, into a
temp dir, deleted afterwards), replayed it with the binary, then compared with the live nerve's own `GET /self-eval`:
every incident the replay opens is open live; the live-only `spend-cap:hermes-rs` is a pushed signal, which a
`/state` capture does not hold (5 points: 61 live vs 66 replay). Owner decision D-T53-2 in `docs/cortex/replay-corpus.md`.

## 4. Gates

See the DONE line in CALL-RECORD.md for the final numbers (fmt, clippy -D warnings workspace, workspace tests without
openmind-sandbox / cgroup-detect, no-python, musl builds). Sandbox code untouched; no sandbox test run here
(sandbox_vps_only).
