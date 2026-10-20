<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Python hits

- DESK-ID: openmind.PY-2
- Written: 2026-10-19T18:45:16+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: Rust and toml files under crates/ (`*.rs`, `*.toml` only)
- Search: case-insensitive substring `python`. No interpreter was started.
- Note: A hit is a word, not proof an interpreter runs.

## Count

| item | value |
| --- | --- |
| files | 8 |
| matching lines | 34 |
| substring occurrences | 35 |

One line holds two occurrences: `crates/desk-hermes/src/jsonout.rs` names `compact_cpython_separators_do_not_spawn_python_or_open_a_socket` (`cpython` and `python`).

## Paths

A hit is a word, not proof an interpreter runs. No crate was edited.

| path | lines | occurrences | note |
| --- | --- | --- | --- |
| crates/desk-hermes/src/jsonout.rs | 11 | 12 | Comments and a test about CPython json.dumps, plus the strings python3 and /usr/bin/python. A hit is a word, not proof an interpreter runs. |
| crates/desk-hermes/src/main.rs | 7 | 7 | Comments comparing stdout, int, len, and rglob to a Python original. A hit is a word, not proof an interpreter runs. |
| crates/galion-desk/src/main.rs | 1 | 1 | Comment that a Rust binary replaces desk-hermes.py. A hit is a word, not proof an interpreter runs. |
| crates/link-watch/src/jsonout.rs | 6 | 6 | Comments about CPython json.dumps layout. A hit is a word, not proof an interpreter runs. |
| crates/link-watch/src/lib.rs | 1 | 1 | Comment about CPython str of a JSON value. A hit is a word, not proof an interpreter runs. |
| crates/link-watch/src/main.rs | 6 | 6 | Comments about CPython time format and the Python original. A hit is a word, not proof an interpreter runs. |
| crates/one-defence/src/lib.rs | 1 | 1 | Comment about Python int() in defence.py. A hit is a word, not proof an interpreter runs. |
| crates/persona-plex-relay/persona-plex-relay.example.toml | 1 | 1 | Comment text python -m moshi.server describing a remote host. A hit is a word, not proof an interpreter runs. |

## Not claimed

These hits are words in comments, doc strings, or test string literals. A hit is a word, not proof an interpreter runs. No process was spawned. No crate file was edited. Nothing was committed.
