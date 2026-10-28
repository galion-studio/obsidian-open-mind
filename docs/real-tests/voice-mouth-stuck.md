<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests — voice-mouth-stuck (slot 358, inc-865945dd)

Branch `swarm/voice-mouth-stuck`, ports 22280-22289. All times Europe/Zagreb
CET (+01:00). Diagnosis/plan: `docs/ops/voice-mouth-stuck.md`.

## 1. VPS read-only probes (no write, no restart)

05:08-05:09 and 05:27:52 CET:

* `galion-one.service` active/running, MainPID 304, NRestarts 0, started
  2026-10-22 22:46:47 CET; drop-ins `50-rust-bin.conf`, `60-no-warm.conf`;
  env has `ONE_MOUTH_BIND=127.0.0.1:8002`, **no `GALION_MOUTH`**.
* `ss`: galion-one pid 304 listens on 127.0.0.1:8091 and 127.0.0.1:8002;
  nothing on 8003/4003. No voice/chatterbox process.
  `/var/lib/galion/voice-models` absent.
* `GET :8002/health` -> 503
  `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}`
  (0.5 ms; same at 05:27:52). `GET :8002/` -> 404. `:8091/health` 200.
* `/opt/galion/rust/bin/galion-one` sha256 prefix `24cb4976b1325ae0`.
* galion-one journal since 2026-10-22: 1 line (systemd start) — no mouth
  request, job or success ever logged.
* nerve `:8670/state` 200: inc-865945dd "galion-one-mouth degrading: health
  probe HTTP 503 -- stuck loading for 10.8 h" (11.0 h at 05:27), open since
  2026-10-27 18:15 CET (nerve restart), next = "... then restart that one
  service (owner approval)".

## 2. Live binary reproduced byte-for-byte on the box

`git worktree` at `40454f9`, `cargo build --release --locked --target
x86_64-unknown-linux-musl -p galion-one --bin galion-one` -> sha256 prefix
**`24cb4976b1325ae0` = the live binary**. Source: `/health` returns the
hard-coded loading body whenever `speech::local().ready()` is false, and
`local()` is the `Unavailable` stub — permanently.

## 3. GALION_REAL real test `real_mouth_not_deployed`

`crates/openmind-nerve/tests/real_mouth_not_deployed.rs`, release build.
Real binaries only: live-identical galion-one 40454f9 (mouth :22281 / text
:22284), this branch's galion-one (:22282 / :22285, musl sha256 prefix
`9b4a55c06ef38d36`), galion-mouth-stub (:22283, `d0a51c3ce98aaac2`),
`openmind-nerve serve` :22280 (NERVE_INTERVAL 5, NERVE_LOADING_GRACE 3,
NERVE_COMPONENTS remapped to the three mouths, reflex off).

Runs: 05:13:58 (gnu binaries) PASS; 05:18:04 PASS; 05:26:39 with musl
binaries FAIL (my over-strict "exactly 3 open incidents": the box host added
a disk-pressure incident at 88 % — test now counts only the mouth incidents);
05:27:04 PASS 6.03 s; 05:27:25 PASS 6.12 s.

Output (05:27 run, abridged):

```
live 40454f9 :22281 /health 503 {"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}
live 40454f9 /synthesize 500 in 0 ms: {"error":"voice"}
live 40454f9 after SIGKILL+respawn /health 503 {...identical "loading" body...}
honest galion-one :22282 /health 503 {"ok":false,"status":"unavailable","provider":"stub","model":null,"loaded":false,"reason":"voice not deployed; set GALION_MOUTH to a galion-voice-serve address"}
galion-mouth-stub :22283 /health 503 {...same honest body...}
nerve :22280 cycle 2 after 5517 ms, bleeding ids ["stuck-loading-galion-one-mouth", "not-deployed-mouth-honest-one", "not-deployed-mouth-stub"]
stuck-loading-galion-one-mouth severity error, fix "...the loading state is hard-coded (no model worker exists): deploy the backend or a build with an honest /health; a restart will not help (owner approval)"
not-deployed-mouth-honest-one / -mouth-stub severity warn, evidence carries "voice not deployed", fix "deploy the backend it names (owner decision) or accept the gap; do not restart"
incident next (all three mouths): contains "a restart will not help"
test result: ok. 1 passed; 0 failed
```

Ports 22280-22289 free after every run (`ss` count 0).

## 4. Gates

* `cargo fmt --all --check` ok.
* `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok (only pre-existing vendored candle-core dep warnings).
* `cargo test -p openmind-nerve --no-fail-fast`: 354 passed / 0 failed / 3 ignored (lib 220 incl. 3 new: `health_body_keeps_honest_reason`, `honest_unavailable_is_not_deployed_not_stuck_loading`, `mouth_loading_vs_not_deployed_findings`; `cortex_replay` golden green with `s358_delta`).
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools --no-fail-fast`: 380 suites, 2016 passed / 0 failed / 18 ignored.
* `scripts/no-python.sh` ok; SPDX headers on new files.
* Release builds: x86_64 musl openmind-nerve 1930048 B, galion-one 963904 B, galion-mouth-stub 451360 B; aarch64 musl (rust-lld) 1593424 / 845096 / 402512 B; `cargo check` armv7-musleabihf, aarch64-android, armv7-androideabi ok.
* Not run: `real_cortex_sensor` / `real_score_v2` GALION_REAL halves (they bind other slots' ports 19430s/19780s); their golden comparisons got the same `s358_delta`.

## 5. Portability to the live nerve lineage

`git cherry-pick -n` of the code commit onto nerve-merge-6 `9baa450c` (next
nerve deploy candidate): all 5 `src/` files apply cleanly; conflicts only in
the test harness (`tests/support/mod.rs`, `cortex_replay.rs`,
`real_cortex_sensor.rs`; `cortex_s2_c.rs`/`real_score_v2.rs` deleted there).
A fold needs its golden deltas redone on that lineage (D-358-2).

No deps added. No deploy, no VPS write, no secrets read or printed.
