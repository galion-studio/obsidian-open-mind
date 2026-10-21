<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Cortex sensor contract (T1.2)

- Written: 2026-10-20 23:05 CEST (+02:00), branch `swarm/cortex-sensor`, base `swarm/integration-1` 8fd26f6.
- Plan: /workspace/swarm/plans/cortex.md T1.2 (blocks S2). Code: `crates/openmind-nerve/src/sensors/mod.rs`, `src/cortex.rs`.

## API (small on purpose; everything else is internal)

```rust
pub trait Sensor: Send {
    fn name(&self) -> &'static str;                         // registry key, default B33 source, NERVE_SENSORS_OFF key
    fn sense(&mut self, input: &Input<'_>, out: &mut Sink<'_>); // once per cycle; &mut self = state across cycles
}

// Sink: where a sensor reports
out.finding(Finding)            // routed by kind: vulnerabilities / bleeding / ddos.findings; source stamped if empty
out.signal(Signal)              // incident signal (cause key -> one incident), scored by the correlator
out.exposure(Value)             // `exposure` section (owned by the built-in `listeners` sensor)
out.ddos_stream(Value, firing)  // one rate stream in `ddos.streams` (owned by the built-in `ddos` sensor)

// Finding (cortex.rs): build, never struct-literal
Finding::new(FindingKind::Bleeding, "id", Sev::Warn, "component", "title")
    .with_evidence(vec![..]).with_fix("..").with_cause("edge").with_source("auth.log")
f.to_nerve_event(now)           // one B33 NerveEvent (serde-identical to openmind_schema::NerveEvent)
cortex.nerve_events(now)        // B41 rule: one event per (source, kind), highest severity wins

// Input (cortex.rs): read-only view of one cycle; build with Input::new(now, comps, logs) in tests
now, comps, logs, listeners, ddos, known_ports, public_ok_ports, listen_scope, own_port, loading_grace_s,
journal   // NEW: journal lines new this cycle (all collected units)
live      // NEW: false in replay/tests -> a host-reading sensor must do nothing

// Registry (sensors/mod.rs)
Sensors::builtin()              // the 5 pre-trait detectors, fixed order
Sensors::from_env(env)          // builtin + registered S2 sensors - NERVE_SENSORS_OFF
s.add(S) / s.add_opt(Option<S>) / s.names() / s.run(&input) -> Cortex / s.last (per-sensor counts + us)
```

B41 alignment: `FindingKind` = `vulnerability | bleeding | ddos`; `Sev::b41()` maps warn -> `warn`, error -> `high`,
critical -> `crit` (info stays `info`, below B41; `nerve_events` skips info-only pairs). `Sev::parse` and
`FindingKind::parse` read both spellings back (for T2.10 pushed events). Event `data` carries id, component, cause
and `evidence_n` only (`nerve_events` `top` the same); titles, fixes and evidence lines (bind addresses, ports, paths,
hostnames) stay in the loopback finding shape `/api/v1/cortex` / `/api/v1/state` `cortex` and never ride in an event
(slot 105, golden `tests/fixtures/cortex-replay.expected.events.json`).

Visible output: every finding in `/api/v1/cortex` gains `kind` and `source`; `ddos.findings` appears only when a
sensor emits ddos-kind findings; `/api/v1/state` gains `cortex_sensors` `[{name, findings, signals, us}]` and
`cortex_events` (B33 events). Nothing else changed (replay goldens below).

## Built-in sensors (moved, no behaviour change)

| name | file | finding ids | signals | reads |
|---|---|---|---|---|
| `listeners` | sensors/listeners.rs | `public-listener-<port>`, `unmanaged-listeners` | - | listeners, ports, scope |
| `scanners` | sensors/scanners.rs | `scanner-probes-<comp>`, `dev-server-<comp>` | `scan:<comp>` | logs.scans, logs.dev_servers |
| `edge` | sensors/edge.rs | `edge-dead-origins` | `edge` | logs.dead_origins, listeners, tunnel counters |
| `components` | sensors/components.rs | `stuck-loading-*`, `restarts-*`, `memory-trend-*`, `error-rate-*`, `host-pressure-*`, `memory-heavy-*` | - (component verdict signals stay in `cortex::component_signals`) | comps |
| `ddos` | sensors/ddos.rs | - (streams) | `ddos:<stream>` | ddos verdicts |


## S2 sensors (merged in T2.0, `swarm/cortex-t20-merge`, 2026-10-21 CEST +02:00)

Registered in `Sensors::from_env` in this order, after the built-ins (each is off until its env is set, so a nerve
with no new env behaves exactly like e536446). Run order fixes signal order and so incident ids: append, don't reorder.

| name | file | task / branch | on when | finding ids (kind) | signals | reads (host only when `live`) |
|---|---|---|---|---|---|---|
| `sshd` | sensors/sshd.rs | T2.1, cortex-s2-a 9557eb3 | `NERVE_SSHD=1` | `sshd-accepted-password`, `sshd-accepted-publickey-other`, `sshd-invalid-user`, `sshd-max-auth-attempts` (vulnerability); `sshd-credential-flood` (ddos); `sshd-auth-log-growth` (bleeding); source `auth.log` | `ssh-flood` | `input.journal` (`NERVE_SSHD_UNITS`, default `ssh.service,sshd.service`); size of `NERVE_SSHD_AUTH_LOG` |
| `rustsec` | sensors/rustsec.rs | T2.4, cortex-s2-a 9557eb3 | `NERVE_RUSTSEC_LOCK` | `rustsec-<pkg>-<ver>` (vulnerability) | - | the lock file; embedded offline advisory snapshot (CC0), optional `NERVE_RUSTSEC_DB` |
| `drift` | sensors/drift.rs | T2.5, cortex-s2-b 51100e1 | `NERVE_DRIFT_MANIFEST` | `drift-<name>`, `drift-missing-<name>`, `drift-manifest` (vulnerability) | - | SHA256SUMS manifest + listed files, hashing budgeted per cycle |
| `conn-rate` | sensors/connrate.rs | T2.7, cortex-s2-b 51100e1 | `NERVE_CONNRATE_PORTS` | `conn-rate-<port>`, `conn-rate-unreadable` (bleeding) | - | `/proc/net/tcp`, `/proc/net/tcp6` |
| `proc-owner` | sensors/proc_owner.rs | T2.8, cortex-s2-c dbec5c9 | `NERVE_PROC_OWNER_USERS` | `stray-owner`, `deleted-binary`, `proc-owner-unresolved` (vulnerability) | - | `/proc/net/tcp*`, `/proc/<pid>/{fd,status,exe}`, `/etc/passwd` |
| `surface` | sensors/surface.rs | T2.9, cortex-s2-c dbec5c9 | `NERVE_SURFACE_TARGETS` | `surface-dns`, `surface-tls-expired`, `surface-tls-expiring`, `surface-tls-fail`, `surface-http-down`, `surface-http-status`, info `surface-tls-unread`/`surface-note` (bleeding, cause `edge`) | - | DNS, TLS 1.2 cert read, HTTP GET on a background thread every 300 s |
| `hades` | sensors/hades.rs | T2.13, cortex-s2-d f3468ce | `NERVE_HADES_LEDGER`, `_DISABLE_FILE`, `_AUDIT` or `NERVE_HADES=1` | `hades-off`, `hades-ledger-failing`, `hades-refusal-burst`, `hades-safety-refusals`, `hades-token-abuse`, `hades-unapprovable-actions`, `sandbox-escape-attempts`, `sandbox-not-enforced` | `hades`, `sandbox` | ledger/audit JSONL tails, disable file; `input.journal` units `hades*` (also in replay) |

Also merged from cortex-s2-d: `src/feed.rs`, `GET /api/v1/cortex/events` (B33 JSONL, counts/ids only; see
docs/cortex/hades-sensor-and-feed.md).

Journal collection (T2.0, `collect.rs`): the engine reads the journal of the component units plus
`collect::JOURNAL_EXTRA_UNITS` = `ssh.service`, so `sshd` sees live lines without a fake component. The extra unit
gets no sample, status or `journal_counts` entry; its lines only reach `input.journal` and the log scan. A host
whose daemon is `sshd.service` / `sshd@*.service` needs that unit added to the list (one line). Collection is still
skipped when no component has a unit or `NERVE_JOURNAL=0`.

New dependencies from the merged branches: `semver` 1 (cortex-s2-a, MIT OR Apache-2.0), `toml` (dev, workspace),
`galion-common` (path, cortex-s2-b). Details in docs/third-party/cortex-s2-{a,b,c,d}.md.

## Adding an S2 sensor

1. `src/sensors/<name>.rs` with SPDX header; `pub struct X { .. }`, `impl Sensor for X`, and
   `pub fn from_env(env: Env) -> Option<Self>` reading only its own `NERVE_<NAME>_*` vars (None = off).
2. In `sensors/mod.rs`: `pub mod <name>;` and one line in `Sensors::from_env`, e.g.
   `s.add_opt(sshd::SshAuth::from_env(env));`. These two lines are the only shared edit (adjacent additions from
   parallel branches are trivial conflicts for the T2.0 merger). Never edit engine.rs / cortex.rs / config.rs.
3. Host reads only when `input.live`; prefer `input.journal` over a second `journalctl` call. Budget: a few ms
   (see `cortex_sensors[].us`; cycle must stay < 50 ms, RSS < 8 MiB with all sensors on).
4. Tests: unit test with `Input::new` + `Sensors::empty().add(X)` + `run`; a fixture; a GALION_REAL=1 test in your
   slot range (pattern: `crates/openmind-nerve/tests/real_cortex_sensor.rs`).
5. Score v2 (T3.3, `src/score.rs`): a finding costs points on its own (ddos crit/error/warn 15/10/4, vuln 10/4,
   bleeding 8/3) unless an open incident with the same `cause` (or an absorbing `ddos:<stream>` / `stack-wide`
   incident listing its component) already scores it; uncovered findings sharing a `cause` are one deduction.
   Emit a `Signal` with the same cause when it should open an incident; set `with_cause` so it is not counted twice.

## S2 tasks this unblocks

| task | input it needs | status after T1.2 (T2.0: merged = in `swarm/cortex-t20-merge`) |
|---|---|---|
| T2.1 sshd auth | `input.journal` (ssh unit) or auth.log when `live` | **merged (T2.0)** |
| T2.2 host posture (sshd -T, ufw, fail2ban) | `live` host commands | ready (command list needs Slavik) |
| T2.3 secrets mode bits | `live` stat | API ready, **Slavik approval** pending |
| T2.4 RustSec advisories | own files | **merged (T2.0)** |
| T2.5 source-vs-live drift | own files, comps units | **merged (T2.0)** |
| T2.6 tunnel ingress lint | own config read | ready after tunnel-audit lands |
| T2.7 per-port connection rate | stateful `&mut self`, /proc/net/tcp when `live` | **merged (T2.0)** |
| T2.8 stray process owner | `input.listeners` + /proc when `live` | **merged (T2.0)** |
| T2.9 public DNS/TLS/HTTP | network GETs when `live` (keep under budget: cache/rotate) | **merged (T2.0)** |
| T2.10 Gaia `/event` feed | event JSONL via its own env; `Sev::parse`, `FindingKind::parse` | outbound B33 feed `/api/v1/cortex/events` merged (T2.0); inbound `/event` sensor open |
| T2.11 POSEIDON budget bleed | after nerve-wiring lands | API ready, blocked on nerve-wiring |
| T2.12 per-hostname edge rate | `input.journal` (cloudflared `dest=`) | ready (feeding the access.rs detector itself needs engine.rs: T2.0) |
| T2.13 hades/sandbox misbehaviour | `input.journal` | **merged (T2.0)** |
| T2.15 push wiring | nerve-client/hub.rs (nerve-wiring) | not a sensor; unaffected |
