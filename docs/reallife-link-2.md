<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real life link 2

- DESK-ID: openmind.REAL-LINK-2
- Stamp: 2026-10-20T10:00:18+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Built binary: /tmp/om-reallink2/debug/link-watch
- Once: LINK_WATCH_ONCE=1
- State dir: /tmp/om-reallink2-state
- Live pid 1359406 was not signaled. Its binary was not replaced. systemctl was not used.

The stamp offset is numeric. It is not a trailing UTC designator.

## Build

Source was not edited. `cargo build -j 1 -p link-watch --offline` ran with `PATH=/root/.cargo/bin:$PATH` and `CARGO_TARGET_DIR=/tmp/om-reallink2`, under `nice -n 19`, from `/opt/galion/openmind`.

`Cargo.lock` was copied to `/tmp/om-lock-REALLINK2-before` before cargo. After the build the repo lock and that snapshot compared equal. Cargo did not rewrite the lock. The snapshot was not copied back. `git checkout` was not used on `Cargo.lock`.

| Item | Value |
| --- | --- |
| Build exit code | 0 |
| Profile | dev |
| Cargo lock rewritten | no |
| New binary | /tmp/om-reallink2/debug/link-watch |
| New binary bytes | 24571440 |

The new binary was not copied onto `/opt/galion/rust/bin/link-watch`.

## Run

The new binary was started once. A 20 second cap was set so a missed once-flag could not keep a second watcher. The cap was not hit. The process exit code is 0.

LINK_WATCH_OUT, LINK_WATCH_LAPTOP, and LINK_WATCH_LOG pointed at files under /tmp/om-reallink2-state. A copy of the laptop card was placed in that dir before the run. The live card file was not the write target. Ollama and Hermes bases were the loopback defaults. The France base was empty, so no remote probe ran. This note does not record a public host.

| Item | Value |
| --- | --- |
| Exit code | 0 |
| Printed a report | no |
| Stdout bytes | 0 |
| Stderr bytes | 0 |
| Report file written | yes |
| Report file | /tmp/om-reallink2-state/link.json |
| Report file bytes | 413 |
| Event log file appeared | no |

Stdout and stderr were empty. The process did not print a report. It did write a JSON file under the state dir. `link-log.json` did not appear. Events did not change.

## Report fields

Keys in the written file: at, at_local, events, france, hermes, hermes_ms, laptop, laptop_port, model, model_ms.

| Field | Value |
| --- | --- |
| laptop_port | false |
| hermes | true |
| france | unconfigured |
| model length | 0 |
| model_ms | 0 |
| hermes_ms | 0 |
| events length | 0 |
| laptop | object, not copied |
| at_local ending | +02:00 |

The legacy at value is not copied. Secrets are not copied. No public host is copied.

## Live process

Checked just before the run and just after it.

| Check | Before | After |
| --- | --- | --- |
| pid 1359406 exists | yes | yes |
| exe | /opt/galion/rust/bin/link-watch | /opt/galion/rust/bin/link-watch |
| start tick | 57221314 | 57221314 |
| live binary inode and size | 43:627982 508248 | 43:627982 508248 |

The live link file stayed inode 43:245674 and 1063 bytes. The live laptop file stayed inode 43:246179, 208 bytes, and kept mtime 1791964380. The live log file stayed inode 43:247067, 661 bytes, and kept mtime 1792345010. This run did not write those three paths. The separate report under /tmp is 413 bytes. The temp laptop copy is inode 43:658919, not the live card.

## Not done

Nothing was committed. Nothing was pushed. systemctl was not used. /opt/galion/rust/bin/link-watch was not replaced. Python was not used for the build or the once-run. Pid 1359406 was not signaled.
