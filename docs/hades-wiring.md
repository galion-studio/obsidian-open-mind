<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# hades wiring (not wired)

- DESK-ID: openmind.HADES-2
- Written: 2026-10-19T17:42:35+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Gate: `crates/hades-gate` (`docs/hades-gate.md`)
- Scope: how a future caller would ask the gate before heal-apply, switch, or rollback. This file does not wire the gate.

## Nothing calls the gate yet

Nothing in the repo calls the gate yet. This document does not wire it.

`hades-gate` is a workspace member. `Cargo.lock` records that package and no other package depends on it. No binary and no script calls `hades_gate::decide` or `Gate::decide`. `deploy/switch.sh`, `deploy/rollback.sh`, `deploy/switch-console.sh`, and `deploy/rollback-console.sh` do not call it. `openmind-survival` has its own `decide` for probe text. That function does not ask this gate and does not apply a heal. The only calls of `hades_gate::decide` are the tests inside `crates/hades-gate`. Those tests classify strings. They do not run heal-apply, switch, or rollback.

This file adds no dependency, edits no script, and adds no caller.

## Default remains dry-run

`Mode` has one variant, `DryRun`, and that variant is the default. `Gate::new` and `Gate::default` store it and cannot leave it. `Gate::mode` is always `Mode::DryRun`. There is no apply mode.

`decide` does not spawn a process and does not use the network. `Decision::WouldApply` is a note that an allow-listed action had the fixed test token. The command is still not run. A future caller that sees `WouldApply` must not execute heal-apply, switch, or rollback because of this gate.

## How a future caller would ask

Ask before the step, not after. Do not pass an apply flag first. The ask is the function already in the crate:

```text
hades_gate::decide(action, token)
```

`Gate::new().decide(action, token)` is the same classification, because the stored mode is dry-run. `action` and `token` are strings. The gate does not read a file, an environment variable, or a flag.

The wire name must be exact. `Action::as_str` and `ALLOW_LIST` use these three:

| Later step the caller has not taken | action string |
| --- | --- |
| heal-apply | `heal-apply` |
| switch (`deploy/switch.sh`, and `deploy/switch-console.sh` which only forwards to it) | `switch` |
| rollback (`deploy/rollback.sh`, and `deploy/rollback-console.sh` which only forwards to it) | `rollback` |

`heal_apply`, `HealApply`, and any other spelling are not allow-listed. `decide("heal_apply", token)` is `DryRun` even when the token matches. `Action::parse` returns `Some` only for `heal-apply`, `switch`, `rollback`, `kill`, and `rotate`. A future caller can parse first and stop when the result is `None`. Passing an unknown string through `decide` is also not approval: the result is `DryRun`.

There is no heal-apply entry point in this repo. A future heal-apply caller, which this ticket does not add, would pass `heal-apply` and then still not apply.

The token is compared as an exact string to `APPROVED_TEST_TOKEN`, which is `approved-test`. The crate documents that constant as a fixed test token, not a credential. Empty, `no`, `Approved-test`, and `approved-test ` (trailing space) do not match. On an allow-listed action those tokens return `DryRun`.

What the future caller does with the result:

| Decision | When | Caller |
| --- | --- | --- |
| `DryRun` | Default. Allow-listed action without the exact test token, or any unknown action. | Stop. Do not apply. Do not treat the plan as approved. |
| `WouldApply` | Action is `heal-apply`, `switch`, or `rollback`, and the token is exactly `approved-test`. | Still stop. The variant is not an executor. This document does not add one. |
| `Refused` | Action is `kill` or `rotate`, with or without the test token. | Stop. Do not run it. Do not retry with another token. |

Inside `classify`, refused names are checked first. Only then can an allow-listed name plus the exact test token return `WouldApply`. Every other input returns `DryRun`.

## kill and rotate stay refused

`ALWAYS_REFUSED` is `kill` and `rotate`. `Action::Kill` and `Action::Rotate` are not allow-listed. The test token does not move them to `WouldApply`. A future caller must not read `Refused` as a dry-run that a second ask can approve. No other wire name turns a kill or a rotation into heal-apply, switch, or rollback.

## What this file does not do

- It does not call the gate.
- It does not add `hades-gate` to another crate.
- It does not change `deploy/switch.sh`, `deploy/rollback.sh`, or the console wrappers.
- It does not add a heal-apply command.
- It does not change the default away from dry-run.
- It does not allow `kill` or `rotate`.
