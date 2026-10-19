<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Session 2026-10-19, many agents

- Written: 2026-10-19T22:22:11+02:00 (Europe/Zagreb)
- Repo: /opt/galion/openmind
- Remote: none. This tree stays private. Nothing in this session was pushed.

This note is what the parallel desks finished. It is not a claim that Hermes is autonomous. The live processes were not replaced.

## Still running, unchanged

| pid | exe |
| --- | --- |
| 1373612 | /opt/galion/openmind/dist/bin-ws/galion-one |
| 231 | /opt/galion/bin/galion-desk |
| 1359406 | /opt/galion/rust/bin/link-watch |
| 1196273 | /opt/galion/tuner/bin/tuner |

Port 8080 is still vite. Port 8091 answers the word ok. Port 8420 is still the live memory gateway. Staged unit files under deploy/boot/rendered/ were not installed.

## What the desks changed

The crates fail closed. A library call does not dial, bind, spawn, install, or apply a firewall rule unless a later ticket says that call is allowed. Tests hold that line.

- Providers parse fixtures. Llama, exo, Tulu, and speech do not use a GPU and do not open a socket.
- Discovery can be asked to scan and still reports packets_sent 0.
- Route never returns a pending seed and never returns a remote row. Order stays GPU, then approved LAN, then CPU local.
- A missing openmind config returns ports 11434, 8642, 8643, 8645, 8091, 8646, and 8080, and does not read overrides. A present file still lets env win.
- galion-one missing-file defaults stay 127.0.0.1:8091 and mouth 127.0.0.1:8002. Page root /workspace/galion-light. Hermes dir /workspace/hermes.
- Desk health JSON does not spawn Python, does not bind 8646 or 8091, and does not dial 11434.
- Room with no flags prints the page and exits. --listen is required.
- A missing persona relay config prints one Zagreb stamp and opens no socket.
- Defence decide applies no firewall change. Sandbox presets build a policy and do not exec. Guardian ticks write a report. Survival suggestions and probes do not restart a unit.
- Hades kill and rotate stay refused. A journal record writes no file. Neither crate is installed.
- Boot to_nerve keys are ts, source, kind, severity, data, in that order. NerveEvent accepts them. A Z timestamp is rejected. now_rfc3339 ends in +01:00 or +02:00.
- gaia-core does not spawn systemctl. Ownership table has 37 crates. hades-journal is HADES.
- SPDX-License-Identifier is in the first five lines of all 119 crates/**/*.rs files that were scanned. REUSE.toml is not added.
- Persona toml matches the markdown. max_sentences is 2. The voice crate does not open an audio device.

## Counts from this session, text only

- The word python: 35 hits in 8 files. Not a running interpreter.
- The word systemctl: 30 hits in 13 files. Not a restart.
- Command::new: 35 call sites in 19 files.
- TcpListener 37 and UdpSocket 17, 54 hits in 25 files. Not a bind made by the scan.
- Fixture directories: 12, holding 30 files.
- Cargo.lock in the working tree lists the new crates. It was not regenerated as a clean single job. This commit records that working copy.

## Left out of git on purpose

- stage/ (console node_modules, hundreds of megabytes)
- dist/bin-ws/ (the live galion-one binary)
- every *.bak-* copy
- no push, no public remote
