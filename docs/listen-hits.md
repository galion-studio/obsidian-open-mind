<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Listen hits

- DESK-ID: openmind.LISTEN-HITS
- Written: 2026-10-19T21:06:07+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: regular `*.rs` files under crates/. Other suffixes were not searched.
- Scanned files: 119
- Search: case-sensitive whole words `TcpListener` and `UdpSocket`. No crate was edited. No port was bound.
- Note: A hit is a word in source, not proof this ticket bound a port.

The stamp above is the time of this write. The offset is numeric (+02:00 or +01:00). It is not a trailing UTC designator.

## Count

| item | value |
| --- | --- |
| files | 25 |
| matching lines | 54 |
| hits | 54 |
| TcpListener | 37 |
| UdpSocket | 17 |

Each hit is one whole-word occurrence. Each matching line holds one hit. A hit is a word in source, not proof this ticket bound a port.

## Paths

A hit is a word in source, not proof this ticket bound a port. No crate was edited. No port was bound.

| path | TcpListener | UdpSocket | count |
| --- | --- | --- | --- |
| crates/desk-hermes/src/jsonout.rs | 1 | 1 | 2 |
| crates/galion-desk/src/main.rs | 3 | 0 | 3 |
| crates/galion-one/src/main.rs | 3 | 0 | 3 |
| crates/galleon-voice/tests/parse.rs | 0 | 1 | 1 |
| crates/guardian-tick/tests/cli.rs | 2 | 0 | 2 |
| crates/link-watch/tests/config_fixture.rs | 2 | 0 | 2 |
| crates/link-watch/tests/decision.rs | 1 | 1 | 2 |
| crates/one-defence/tests/decision.rs | 1 | 1 | 2 |
| crates/one-talk/tests/shapes.rs | 3 | 0 | 3 |
| crates/openmind-discovery/src/lib.rs | 0 | 1 | 1 |
| crates/openmind-discovery/tests/discover.rs | 1 | 1 | 2 |
| crates/openmind-exo/tests/parse.rs | 0 | 1 | 1 |
| crates/openmind-fallback/tests/select.rs | 1 | 1 | 2 |
| crates/openmind-llama/tests/parse.rs | 0 | 1 | 1 |
| crates/openmind-memory/src/http.rs | 3 | 0 | 3 |
| crates/openmind-origin/tests/redirect.rs | 1 | 1 | 2 |
| crates/openmind-provider/tests/registry.rs | 2 | 2 | 4 |
| crates/openmind-route/tests/route.rs | 2 | 2 | 4 |
| crates/openmind-speech/tests/parse.rs | 1 | 2 | 3 |
| crates/openmind-survival/tests/survival.rs | 2 | 0 | 2 |
| crates/openmind-tulu/tests/parse.rs | 1 | 1 | 2 |
| crates/room/src/main.rs | 2 | 0 | 2 |
| crates/tuner/src/config.rs | 1 | 0 | 1 |
| crates/tuner/src/main.rs | 2 | 0 | 2 |
| crates/tuner/tests/serve.rs | 2 | 0 | 2 |

Notes, same order. A hit is a word in source, not proof this ticket bound a port.

- `crates/desk-hermes/src/jsonout.rs`: two quoted spellings, one TcpListener and one UdpSocket.
- `crates/galion-desk/src/main.rs`: a use import, one TcpListener::bind call, and one quoted TcpListener in a banned-spelling list. This scan did not call bind.
- `crates/galion-one/src/main.rs`: a use import and two TcpListener::bind calls. This scan did not call bind.
- `crates/galleon-voice/tests/parse.rs`: one quoted UdpSocket.
- `crates/guardian-tick/tests/cli.rs`: a use import and TcpListener::bind in a test. This scan did not call bind.
- `crates/link-watch/tests/config_fixture.rs`: a use import and TcpListener::bind in a test. This scan did not call bind.
- `crates/link-watch/tests/decision.rs`: quoted TcpListener and UdpSocket.
- `crates/one-defence/tests/decision.rs`: quoted TcpListener and UdpSocket.
- `crates/one-talk/tests/shapes.rs`: a use import, a TcpListener parameter, and TcpListener::bind in a test. This scan did not call bind.
- `crates/openmind-discovery/src/lib.rs`: a test asserts source text does not contain the quoted spelling UdpSocket. The spelling is still a word in this file.
- `crates/openmind-discovery/tests/discover.rs`: quoted UdpSocket and TcpListener.
- `crates/openmind-exo/tests/parse.rs`: one quoted UdpSocket.
- `crates/openmind-fallback/tests/select.rs`: quoted UdpSocket and TcpListener.
- `crates/openmind-llama/tests/parse.rs`: one quoted UdpSocket.
- `crates/openmind-memory/src/http.rs`: a use import, a TcpListener parameter, and TcpListener::bind. This scan did not call bind.
- `crates/openmind-origin/tests/redirect.rs`: quoted UdpSocket and TcpListener.
- `crates/openmind-provider/tests/registry.rs`: two quoted pairs of UdpSocket and TcpListener.
- `crates/openmind-route/tests/route.rs`: two quoted pairs of UdpSocket and TcpListener.
- `crates/openmind-speech/tests/parse.rs`: three quoted spellings, one TcpListener and two UdpSocket.
- `crates/openmind-survival/tests/survival.rs`: a use import and TcpListener::bind in a test. This scan did not call bind.
- `crates/openmind-tulu/tests/parse.rs`: quoted TcpListener and UdpSocket.
- `crates/room/src/main.rs`: a use import and TcpListener::bind. This scan did not call bind.
- `crates/tuner/src/config.rs`: a doc comment names TcpListener::bind. This file has no bind call.
- `crates/tuner/src/main.rs`: a use import and TcpListener::bind. This scan did not call bind.
- `crates/tuner/tests/serve.rs`: a use import and TcpListener::bind in a test. This scan did not call bind.

## Not claimed

These hits are words in use lines, type names, doc comments, test helpers, and quoted spellings. A hit is a word in source, not proof this ticket bound a port. No port was bound. No listener was opened. No crate file was edited. Nothing was committed. Nothing was pushed. cargo was not run. systemctl was not used.
