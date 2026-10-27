<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 238 nerve-own-services (D-T44-2): own services are not strays

Branch `swarm/nerve-own-services`, base `nerve-event-driven` bb287cc (the
live nerve). Branch only, no deploy. Whether to deploy stays Slavik's call
(D-T44-2). All times are Europe/Zagreb (CEST +02:00).

## Problem (live, read-only, 2026-10-22 01:08:58 +02:00)

The live nerve (`openmind-nerve-bb287cc serve --mode event`, no
`NERVE_KNOWN_PORTS` / `NERVE_PORT_OWNERS` / `NERVE_UNOWNED_PORTS` override in
the unit) reports `unmanaged_ports [8672, 8674, 18800, 19120]`, i.e. the
`unmanaged-listeners` finding "4 loopback listener(s) no monitored unit owns".
The plan engine (cortex T4.2, `plans.rs` on `swarm/cortex-t44-e2e` 7893698)
turns that finding into stray-stop plans. Taking the live `/state` at
01:19:39 through `openmind-nerve plans` gives:

```
plan-unit-restart-galion-one.service
plan-ingress-rule-removal-cloudflared
plan-stray-stop-stray-listener-18800
plan-stray-stop-stray-listener-19120   <- openmind-queue (POSEIDON)
plan-stray-stop-stray-listener-8672    <- galion-nerve-shadow
plan-stray-stop-stray-listener-8674    <- gaia-cortex (the live cortex)
```

Holders, read with `ss -ltnp` and `/proc/<pid>/cgroup` on the VPS (read-only):

| port | process | unit | verdict |
|---|---|---|---|
| 8672 | galion-nerve-sh | galion-nerve-shadow.service | own service |
| 8674 | gaia-cortex-bd6 | gaia-cortex.service | own service |
| 19120 | openmind-queue | openmind-queue.service | own service |
| 18800 | sshd | session-71331.scope | **not** a Galion service: a manual ssh port forward in a login session (no unit). Stays a stray (D-233-3). |

## Change

- `owners.rs`: new `OWN_SERVICES` table (8672 galion-nerve-shadow AETHER,
  8674 gaia-cortex AETHER, 19120 openmind-queue POSEIDON), always merged
  into the owner map (also when `NERVE_KNOWN_PORTS` is set).
  `NERVE_PORT_OWNERS` still relabels and `NERVE_UNOWNED_PORTS` still wins.
  They are not components: no health probe, no reflex restart, no score
  change. Owning them only stops the stray report.
- `evt/inventory.rs`: port hint 8674 -> `cortex` / `gaia-cortex` (8672 and
  19120 had one already).
- Unit test `own_services_are_owned_unless_documented_unowned`; real test
  `tests/real_own_services.rs`; fixture
  `tests/fixtures/own-services/ss-ltnp-vps-20261022-0110.txt` (live
  `ss -ltnp` 01:09:46, fd numbers stripped, loopback only, no public IPs).

## Real tests (box, release binary, ports 21080-21089)

`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_own_services -- --nocapture --test-threads 1`
at 01:12:28:

```
238 replay: unmanaged [18800]; finding "1 loopback listener(s) no monitored unit owns: :18800"
238 replay override: unmanaged [8674, 18800]          (NERVE_UNOWNED_PORTS=8674)
238 serve mode_event=false after 0.5s: unmanaged [21081]; stray holder "real_own_servic" pid 2336163
238 serve mode_event=true  after 0.5s: unmanaged [21081]; stray holder "real_own_servic" pid 2336163
test result: ok. 2 passed; 0 failed
```

Mutation: the same test against the base `owners.rs` fails with
`left: [8672, 8674, 18800, 19120]` / `right: [18800]`, which is exactly what the
live nerve reports.

End to end through the plan engine (01:19-01:21): the patched release nerve
replayed the live `/state` snapshot (01:11:56) together with the live
`ss -ltnp` fixture, and the t44 `openmind-nerve plans` was run on its output:

```
unmanaged_ports [18800]
plan-unit-restart-galion-one.service
plan-stray-stop-stray-listener-18800
```

The three own-service stray-stops are gone and the real stray (:18800) is still
planned. (`ingress-rule-removal` is missing only because the replay had no
cloudflared journal lines. That is not related to this change.)

## Gates

`cargo fmt --all --check` ok; `cargo clippy -p openmind-nerve --all-targets
-D warnings` ok; `cargo test -p openmind-nerve` all pass (lib 99, all
integration suites ok); `scripts/no-python.sh` ok. No new dependencies, and
Cargo.lock is unchanged.

Cross builds (01:24-01:26): x86_64-unknown-linux-musl release ok; aarch64-unknown-linux-musl release ok with
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` (the default linker fails
to link on this box, same as slot 222); `cargo check` ok for aarch64-linux-android
and armv7-unknown-linux-musleabihf. The first attempt at 01:22 hit `No space left on
device` (the shared box disk was full at 124G/125G). I removed my build dir and retried
once there was free space. The build dir was removed again afterwards.

## Owner decisions

- D-T44-2: deploy this nerve change (folds into the next nerve rollout).
- D-233-3: :18800 (sshd forward from session-71331): keep it as a known stray
  warning or close the session. This change deliberately leaves it reported.
