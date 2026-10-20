<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# hades wire (not wired)

- DESK-ID: openmind.HADES-WIRE
- Written: 2026-10-19T19:56:21+02:00
- Repo: /opt/galion/openmind
- Crates: `crates/hades-gate`, `crates/hades-journal`
- Scope: say whether either binary is installed or started. This file does not start one and does not edit either crate.
- Stamp: seconds with a numeric offset from the required clock command. Not a UTC suffix.

## Neither binary is installed or started

Neither crate builds a binary. Each manifest has `[package]` and no `[[bin]]`. Each tree has `src/lib.rs` and no `src/main.rs`. Neither source has `fn main`.

Checked at the stamp above, without starting a process:

- `command -v hades-gate` printed nothing. `command -v hades-journal` printed nothing.
- No regular file named `hades-gate` or `hades-journal` was found under `/opt`, `/usr`, `/root`, `/home`, or `/etc`. No `libhades_gate*` or `libhades_journal*` file was found there.
- No process had the command name `hades-gate` or `hades-journal`.
- No unit file named for either crate sits under `/etc/systemd`, `/lib/systemd`, or `deploy/`. A name search of those trees found no mention.
- `dist/bin` and `dist/bin-ws` do not hold either name.

Neither binary is installed. Neither binary is started. `systemctl` was not used. Cargo was not run.

## kill and rotate stay refused

`kill` and `rotate` stay refused. In `crates/hades-gate`, `ALWAYS_REFUSED` is those two names. `decide` returns `Refused` for them, with an empty token, with `no`, and with the fixed test token. `WouldApply` is not the result. `hades_journal::record` calls `hades_gate::decide` and puts `Refused` in the `decision` field for the same inputs.

## A journal record writes no file

A journal record writes no file. `hades_journal::record` returns one JSON object. The keys are `action`, `decision`, and `ts`. `decision` is `DryRun`, `WouldApply`, or `Refused`. The library source in `crates/hades-journal/src/lib.rs` does not call `std::fs`, `OpenOptions`, `File::create`, or `File::open`. It does not spawn a process and it does not use the network. A test may write a temp file of its own. The record call does not.

## Nothing is wired to a live service

Nothing is wired to a live service. `crates/hades-journal` depends on `crates/hades-gate` by path. `Cargo.lock` records that link. No other package depends on `hades-gate` or `hades-journal`. The `record` call stays in the library. No live service calls it.

`crates/gaia-registry` lists both names on an ownership table under `Function::Hades`. That row does not install a binary and does not start a process.

No deploy script calls `decide` or `record`. No other crate source calls them. This file adds no caller.

## What this file does not do

- It does not edit `crates/hades-gate` or `crates/hades-journal`.
- It does not start either binary.
- It does not install a binary.
- It does not allow `kill` or `rotate`.
- It does not make a journal record write a file.
- It does not wire either crate to a live service.
- It does not commit. It does not push.
