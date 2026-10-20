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
5. A ddos-kind finding is not scored by itself until T3.3: emit a `Signal` with it if it must count.

## S2 tasks this unblocks

| task | input it needs | status after T1.2 |
|---|---|---|
| T2.1 sshd auth | `input.journal` (ssh unit) or auth.log when `live` | ready |
| T2.2 host posture (sshd -T, ufw, fail2ban) | `live` host commands | ready (command list needs Slavik) |
| T2.3 secrets mode bits | `live` stat | API ready, **Slavik approval** pending |
| T2.4 RustSec advisories | own files | ready |
| T2.5 source-vs-live drift | own files, comps units | ready |
| T2.6 tunnel ingress lint | own config read | ready after tunnel-audit lands |
| T2.7 per-port connection rate | stateful `&mut self`, /proc/net/tcp when `live` | ready |
| T2.8 stray process owner | `input.listeners` + /proc when `live` | ready |
| T2.9 public DNS/TLS/HTTP | network GETs when `live` (keep under budget: cache/rotate) | ready |
| T2.10 Gaia `/event` feed | event JSONL via its own env; `Sev::parse`, `FindingKind::parse` | ready |
| T2.11 POSEIDON budget bleed | after nerve-wiring lands | API ready, blocked on nerve-wiring |
| T2.12 per-hostname edge rate | `input.journal` (cloudflared `dest=`) | ready (feeding the access.rs detector itself needs engine.rs: T2.0) |
| T2.13 hades/sandbox misbehaviour | `input.journal` | ready |
| T2.15 push wiring | nerve-client/hub.rs (nerve-wiring) | not a sensor; unaffected |
