<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: cortex-t51-chaos

No new dependency, no copied code. `Cargo.lock` unchanged.

Researched before writing (Mate rule t220, MIT / Apache-2.0 Rust first),
2026-10-21 23:05 +02:00:

| candidate | licence | why not used |
|---|---|---|
| [noxious](https://github.com/oguzbilgener/noxious) 1.0.4 (Rust port of Toxiproxy) | MIT OR Apache-2.0 | needs tokio + async-trait + a REST control server for one latency toxic; last release 2021-04. The test needs one std-only "sleep before forwarding each chunk" proxy (~40 lines in `real_t51_chaos.rs`, idea only, no code copied). |
| [turmoil](https://github.com/tokio-rs/turmoil) | MIT | deterministic simulation of tokio code; the task is chaos against the real release binaries (std threads), so it cannot drive them. |
| [fail](https://crates.io/crates/fail) (fail points) | Apache-2.0 | needs instrumentation inside the product code; the chaos here is external (SIGKILL / SIGSTOP / restart / latency / garbage). |

Reused in-repo (same MIT project): the test helpers follow
`crates/openmind-nerve/tests/evt_mode.rs` (journal FIFO via
`NERVE_EVT_JOURNALCTL=replay:`) and `crates/gaia-cortex/tests/binary_e2e.rs`.
The nerve under test is the unmodified `openmind-nerve` from
`swarm/nerve-event-driven` bb287cc (MIT, this repo).
