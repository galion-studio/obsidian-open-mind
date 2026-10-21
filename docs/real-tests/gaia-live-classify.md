<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: gaia-live-classify (Gaia S3.4, wave-4 slot 224)

Run: 2026-10-21 23:05 to 23:12 CEST (+02:00), Europe/Zagreb. Box ports 20940-20949.
Branch `swarm/gaia-live-classify`, based on `swarm/gaia-loop-decisions` b40208a.

## What was built
`gaia classify` is a read-only live classification. It runs Gaia's own probes, which
are the `status --live` set plus the nerve (8670), cortex (8674), galion-desk (8646),
Galion One mouth (8002) and galion-health (8660), and the unit-only cloudflared,
tuner-quick and openmind-queue. That makes 14 units. It then puts Gaia's verdict for
each unit next to the nerve's (`GET /state` components, open incidents and the
per-function `gaia` view) and the cortex's (`GET /health`, `GET /incidents`). Every
disagreement becomes a finding. The finding kinds are:

| kind | meaning |
|---|---|
| `gaia-nerve-disagree` | Gaia's ok/warn/bad differs from the nerve component status (error when ok vs bad) |
| `nerve-blind` | Gaia probes a unit the nerve has no component for |
| `gaia-blind` | the nerve (or the cortex) tracks something Gaia has no probe for |
| `cortex-missed` | the nerve has an open incident for the unit and the cortex has none open (matching `nerve-incident:<cause>` and `unit:<unit>`) |
| `cortex-disagree` | the cortex has an incident on a unit that Gaia and the nerve see as healthy, with no nerve incident |
| `nerve-incident-on-healthy` | the nerve has an open incident while its own component row and Gaia say healthy |
| `ownership-disagree` | Gaia's owning function differs from the nerve's `gaia.function` |
| `function-disagree` | Gaia's worst class per function differs from the nerve's function status |
| `cortex-nerve-incident-gap` | the nerve has open incidents and the cortex has 0 |
| `nerve-unreadable` / `cortex-unreadable` / `cortex-mode` | loud failures and the dry-run notice |

For every unit that is not ok, the output adds a **heal-tick plan-only** block. It
contains the real apply-path scope guard (`gaia_live::heal::check_target`, so live
ports are always refused), the nerve's restarts in the last hour against the
combined 3/h cap (D-L2, fails closed with no nerve), and the HADES plan steps. The
block always shows `applied:false`, `token_used:false` and `nerve_event_posted:false`.
`heal_tick()` is never called, because it POSTs nerve events.

Remote mode: `--port-map R=L,...` sends every GET through an `ssh -L` tunnel, and
`--evidence F` takes `unit NAME STATE` / `file PATH AGE_S|missing` lines captured by
`systemctl is-active` and `stat`. Malformed lines are an error, and an uncaptured
unit counts as `not-captured`, which is bad. `--strict` exits 3 on any disagreement.

## VPS scope (read-only)
- One `ssh -N -L 127.0.0.1:2094x:127.0.0.1:<port>` tunnel for 8420 8642 8650 8091 8080 8670 8674 8646 8002 8660.
- One `ssh … sh -s` batch per run with `systemctl is-active` ×13 and `stat -c %Y` on the link-watch state file. Output goes to stdout only.
- An independent `curl -sS -m 5 http://127.0.0.1:8670/state` to stdout for the cross-check.
- No POST, no token, no unit or permission change, no build, nothing under /opt/galion/openmind.
- **Disclosure:** my first manual probe at 22:55 used `curl -o /tmp/.x`, which wrote a 10-byte file in the VPS `/tmp`.
  I removed it right away (`rm -f /tmp/.x`, verified gone). Every later command used stdout only.

## Box real test: `real_gaia_classify_box` (GALION_REAL=1)
The real release `gaia` binary ran against an in-test fake nerve (20940), a fake
cortex (20941) and a fake memory service (20942). The other 7 live ports were mapped
to closed ports 20943-20949. The test checked the verdicts, the findings, `--strict`
exit 3, and that **only GET request lines** arrived:
```
nerve saw 4 request(s): ["GET /health HTTP/1.1", "GET /state HTTP/1.1", "GET /health HTTP/1.1", "GET /state HTTP/1.1"]
cortex saw 6 request(s): ["GET /health HTTP/1.1", "GET /health HTTP/1.1", "GET /incidents HTTP/1.1", ...]
memory saw 2 request(s): ["GET /health HTTP/1.1", "GET /health HTTP/1.1"]
```
The fakes are torn down and ports 20940-20942 are checked closed.

## VPS real test: `real_gaia_classify_vps` (GALION_REAL=1 GAIA_VPS_SSH=slavik-vps)
There were 6 green runs: 23:05:51, 23:06:38, 23:07:09, 23:09:24, 23:10:05 and
23:11:28, plus the final 23:12:20 below. Each classify took 2.7 to 3.2 s. In every run
**12/12** nerve verdicts recorded by Gaia matched an independent `/state` read, and
the tunnel ports were closed after teardown. Final run (commit f76c92b), 23:12:20 CEST:
```
service           gaia  nerve               cortex  agree  heal-tick (plan-only)
openmind-memory   ok    ok(healthy)         ok      true   -
hermes-rs         ok    ok(healthy)         warn    false  -
link-watch        ok    ok(healthy)         ok      true   -
tuner             ok    ok(healthy)         ok      true   -
galion-one        ok    ok(healthy)         ok      true   -
console           ok    ok(healthy)         ok      true   -
galion-nerve      ok    ok(healthy)         ok      true   -
gaia-cortex       ok    unknown(-)          ok      false  -
galion-desk       ok    ok(healthy)         ok      true   -
galion-one-mouth  bad   warn(degrading)     ok      false  port 8002 is outside the allowed test range 19000-19999
galion-health     ok    ok(healthy)         ok      true   -
cloudflared       ok    warn(failing-soon)  warn    false  no HTTP port: heal-tick only heals loopback HTTP test services
tuner-quick       ok    ok(healthy)         ok      true   -
openmind-queue    ok    unknown(-)          ok      false  -
FINDING info  nerve-incident-on-healthy  hermes-rs         nerve open inc-0b674249 while its row and Gaia say healthy
FINDING warn  nerve-blind                gaia-cortex       Gaia sees gaia-cortex ok but the nerve has no component for it
FINDING warn  cortex-missed              gaia-cortex       nerve has open incident inc-5d566881 for it; the cortex has none open
FINDING warn  gaia-nerve-disagree        galion-one-mouth  Gaia says bad [GET :8002/health -> HTTP 503], nerve says warn (degrading)
FINDING warn  cortex-missed              galion-one-mouth  nerve has open incident inc-d7320b38 for it; the cortex has none open
FINDING warn  gaia-nerve-disagree        cloudflared       Gaia says ok, nerve says warn (failing-soon: 792 errors in 5 min)
FINDING warn  nerve-blind                openmind-queue    Gaia sees openmind-queue ok but the nerve has no component for it
FINDING info  gaia-blind                 voice-worker      nerve: absent (ws/voice KittenTTS worker; not deployed yet)
FINDING info  gaia-blind                 lxc-host          nerve: healthy; Gaia has no probe
FINDING info  function-disagree          AETHER            Gaia says bad, nerve says failing-soon
FINDING info  function-disagree          MINERVA           Gaia says ok, nerve says no-live-systems
summary {"agree":9,"disagree":5,"findings":11,"units":14,"by_severity":{"info":5,"warn":6}}
overall {"gaia":"degraded","nerve_status":"degraded","nerve_score":51,"nerve_open_incidents":5,
         "cortex_mode":"live","cortex_open_incidents":2,"cortex_escalations_open":1,"cortex_hermes_handoff":true}
```
All 13 units were `active`. The link-watch state file was 1-2 s old.

## Findings for the owner (live, 2026-10-21 22:55-23:12 CEST)
1. **The nerve does not watch the cortex.** There is no `gaia-cortex` component. The
   nerve's own vulnerability check listed :8674, :8672 (shadow nerve), :18800 and
   :19120 (openmind-queue) as loopback listeners that "no monitored unit owns". Yet
   the nerve opened incident inc-5d566881 (cause `gaia-cortex`, "pushed 2 errors")
   at 23:06:30. Suggested fix: add `gaia-cortex` (8674 /health) and `openmind-queue`
   components to the nerve, and decide what to do with :8672 and :18800.
2. **The cortex misses incidents that were open before it started.** The cortex only
   mirrors nerve `incident.opened` events it receives itself. The nerve's edge
   (inc-a763c076, opened 22:56:56) and mouth (inc-d7320b38) incidents were never
   mirrored. After the cortex restarted around 23:07-23:09 (incident ids and
   `opened_total` reset), its 23:06 `nerve-incident:gaia-cortex` mirror was gone
   while the nerve incident stayed open. Suggested fix: on start, the cortex
   should rebuild its open incidents from the nerve's `GET /state` `incidents.open`.
   At 23:12 it did hold `nerve-incident:edge`. The mouth incident is still missed.
3. **The mouth's 503 is graded differently.** Gaia says `bad` (the health GET fails)
   and the nerve says `degrading` (it treats 503 "loading" as warming up). It has been
   stuck in "loading" for more than 3.5 h. Gaia is the stricter, honest view. The
   heal-tick plan is refused at scope (port 8002, live), and the nerve has used 0 of
   its 3/h.
4. **Gaia cannot see cloudflared's error rate.** The unit is active, so Gaia says ok.
   The nerve says failing-soon (190 to 792 errors in 5 min, dead origins). Gaia needs a
   log or tunnel sensor here, or should take the nerve's verdict.
5. **hermes-rs:** the nerve has incident inc-0b674249 open since 23:09:16 while its
   component row says healthy. The cortex mirrors it (inc-4f248f42) and has 1 open
   escalation with Hermes handoff on.
6. **Ownership:** Gaia files the cortex under MINERVA (guardian-watchdog). The nerve
   reports MINERVA as "no-live-systems". AETHER is bad for Gaia (mouth) and
   failing-soon for the nerve. These follow from items 1 and 3.
7. The cortex `/health` now reports `mode: live`, with `llm_enabled` and
   `hermes_handoff` true. The earlier probe at 22:55 still showed `dry-run`.

## Decisions for Slavik
- D-224-1: Should `gaia-cortex` be filed under MINERVA/guardian-watchdog, or get a new
  system (for example `cortex` under MINERVA)? It decides the function-disagree
  finding for MINERVA.
- D-224-2: For the mouth 503 "loading", should Gaia keep `bad` or adopt the nerve's
  warming-up `warn`? Gaia `bad` is recommended after more than 10 min.
- D-224-3: Should the cortex rebuild its incidents from nerve `/state` on start
  (finding 2)? That is a cortex change and needs a stronger model than DeepSeek
  under the rules of 22:50.
- D-224-4: Should the nerve add `gaia-cortex` and `openmind-queue` components
  (finding 1)? That is a nerve change.

## Gates (23:12-23:15 CEST)
- `cargo test -p gaia --test classify`: 14/0. `real_gaia_classify` with GALION_REAL=1 and the VPS: 2/0 in 7 runs.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 1283 passed, 0 failed, 6 ignored (274 suites).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean. `scripts/no-python.sh`: ok.
- Cross builds: `gaia` release for x86_64-unknown-linux-musl (1,164,160 B) and for
  aarch64-unknown-linux-musl (983,248 B, static, needs `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`
  because the box `cc` rejects `--fix-cortex-a53-843419`). `cargo check` of gaia and
  gaia-live for aarch64-linux-android and armv7-unknown-linux-musleabihf: ok.
