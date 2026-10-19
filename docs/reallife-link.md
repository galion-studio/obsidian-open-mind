<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real life link

- DESK-ID: openmind.REAL-LINK
- Stamp: 2026-10-19T22:30:10+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Binary: /opt/galion/openmind/dist/bin/link-watch
- Once: LINK_WATCH_ONCE=1
- State dir: /tmp/om-real-link
- Live pid 1359406 was not signaled. Its binary was not replaced. systemctl was not used.

The stamp offset is numeric. It is not a trailing UTC designator.

## Run

The dist binary was started once. A 20 second cap was set so a missed once-flag could not keep a second watcher. The cap was not hit.

LINK_WATCH_OUT, LINK_WATCH_LAPTOP, and LINK_WATCH_LOG pointed at files under /tmp/om-real-link. A copy of the laptop card was placed in that dir before the run. The live card file was not the write target. Local probe bases stayed at the binary defaults (loopback). This note does not record a public host.

| Item | Value |
| --- | --- |
| Exit code | 0 |
| Printed a report | no |
| Stdout bytes | 0 |
| Stderr bytes | 0 |
| Report file written | yes |
| Report file | /tmp/om-real-link/link.json |
| Report file bytes | 404 |

Stdout and stderr were empty. The process did not print a report. It did write a JSON file under the state dir.

## Report fields

Keys in the written file: at, at_local, events, france, hermes, hermes_ms, laptop, laptop_port, model, model_ms.

| Field | Value |
| --- | --- |
| laptop_port | false |
| hermes | true |
| france | false |
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
| dist binary inode and size | 43:497616 394392 | 43:497616 394392 |

The live link file stayed 1063 bytes. The live laptop file stayed 208 bytes and kept its mtime. The live log file stayed 661 bytes and kept its mtime. This run did not write those three paths. The separate report under /tmp is 404 bytes.

## Not done

Nothing was committed. Nothing was pushed. systemctl was not used. /opt/galion/rust/bin/link-watch was not replaced. Python was not used. Pid 1359406 was not signaled.
