<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Boot units (staged, not installed)

- DESK-ID: openmind.B55
- Written: 2026-10-19T16:50:55+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: `crates/openmind-units`
- Fixture: `crates/openmind-units/tests/fixtures/openmind.toml`
- Output: `deploy/boot/rendered/`

## Status

These files are staged and not installed.

The tuner unit is staged and not installed. `deploy/boot/rendered/tuner.service` holds bind text `127.0.0.1:8650` as renderer output only.

The `*.service` files under `deploy/boot/rendered/` are unit text from the
renderer. They are staged and not installed. Nothing in that directory is
copied into a host unit directory. Nothing there is enabled or started.
The files have no install section.

The `openmind-units` binary prints unit text. With `--out` it also writes
those text files. It does not install units and it does not start them.

## What each file contains

Every rendered file has:

- `Restart=always`
- a Zagreb timestamp comment `# Rendered: <RFC3339> Europe/Zagreb`
- an explicit offset of `+01:00` or `+02:00`, never a `Z` suffix

The stamp on the files in this commit is `2026-10-19T16:50:55+02:00`.

## Print them again

```
openmind-units --fixture crates/openmind-units/tests/fixtures/openmind.toml
openmind-units --fixture crates/openmind-units/tests/fixtures/openmind.toml --stamp 2026-10-19T16:50:55+02:00 --out deploy/boot/rendered
```

`--out` only writes text files. It does not install them.

## Files

| File | Bind / exec from the fixture |
| --- | --- |
| `galion-one.service` | `paths.bin`/galion-one, `ONE_BIND` from `hosts.ollama` and `ports.one` |
| `galion-desk.service` | `paths.bin`/galion-desk, `DESK_BIND` from `hosts.ollama` and `ports.desk` |
| `link-watch.service` | `paths.bin`/link-watch |

In this fixture `hosts.ollama` is `127.0.0.1`.

Do not edit unit files that already live directly under `deploy/`. Those are
a different staged set. `deploy/boot/rendered/` is renderer output only.
