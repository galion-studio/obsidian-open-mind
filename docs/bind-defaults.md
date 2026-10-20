<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Bind defaults

- DESK-ID: openmind.BINDS
- Written: 2026-10-19T19:56:56+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: source text only. No binary was run. No port was bound.

The stamp offset is numeric. It is not a trailing UTC designator.

Strings below are the missing-file defaults visible in source. Test fixtures and config-file format templates are not defaults and are not listed.

| Program | File | String |
| --- | --- | --- |
| galion-one | crates/galion-one/src/main.rs | 127.0.0.1:8091 |
| galion-one | crates/galion-one/src/main.rs | 127.0.0.1:8002 |
| galion-desk | crates/galion-desk/src/main.rs | 127.0.0.1:8646 |
| link-watch | crates/link-watch/src/main.rs | http://127.0.0.1:11434 |
| link-watch | crates/link-watch/src/main.rs | http://127.0.0.1:8642 |
| tuner | crates/tuner/src/config.rs | 127.0.0.1:8650 |
| room | crates/room/src/main.rs | 127.0.0.1:{port} |

## galion-one

`DEFAULTS.bind` is `127.0.0.1:8091` (`ONE_BIND` listen address).
`DEFAULTS.mouth_bind` is `127.0.0.1:8002` (`ONE_MOUTH_BIND` listen address).
The same two strings are in the header comments of `crates/galion-one/src/main.rs`.

## galion-desk

`DEFAULTS.bind` is `127.0.0.1:8646` (`DESK_BIND` listen address).
The header comment of `crates/galion-desk/src/main.rs` repeats that string.
`DEFAULTS.ollama` is `127.0.0.1:11434`. That string is a dial address, not a listen bind.

## link-watch

No listen-bind string is in this crate. There is no `TcpListener`.
`DEFAULTS.ollama` is `http://127.0.0.1:11434`.
`DEFAULTS.hermes` is `http://127.0.0.1:8642`.
Both are probe bases. The binary dials them. It does not bind them.

## tuner

`DEFAULT_ADDR` in `crates/tuner/src/config.rs` is `127.0.0.1:8650`.
The same string is in a comment in `crates/tuner/src/lib.rs`.
It is the listen address when `TUNER_ADDR` is unset and no config file sets `bind`.

## room

The fallback inside `bind_addr` is the format string `127.0.0.1:{port}`.
The listen call is `bind_addr("ROOM_BIND", 4020)`.
When `ROOM_BIND` is unset or blank, that call builds `127.0.0.1:4020`.
The host and port `127.0.0.1:4020` is not one string literal in the file.
