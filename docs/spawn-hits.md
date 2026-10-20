<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Spawn hits

- DESK-ID: openmind.SPAWN-HITS
- Written: 2026-10-19T21:06:14+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: regular `*.rs` files under crates/. Other suffixes were not searched.
- Scanned files: 119
- Search: case-sensitive fixed string `Command::new`. A hit is a call expression. A quoted spelling is not a hit.
- Note: A hit is a call site in source, not proof a process was spawned by this ticket.

The stamp offset is numeric. It is not a trailing UTC designator.

## Count

| item | value |
| --- | --- |
| scanned files | 119 |
| files | 19 |
| hits | 35 |
| spellings that are not hits | 3 |

A raw search of the same spelling finds 38 lines. Three of those lines are string literals, not calls. They are listed under Not a call site. They are not hits.

## Paths

A hit is a call site in source, not proof a process was spawned by this ticket. No crate was edited. No binary was run.

| path | count |
| --- | --- |
| crates/gaia-core/tests/cli.rs | 1 |
| crates/galion-desk/src/main.rs | 1 |
| crates/galion-one/src/main.rs | 5 |
| crates/galion-one/src/models_refresh.rs | 1 |
| crates/galleon-arena/src/cli.rs | 1 |
| crates/galleon-arena/tests/list.rs | 1 |
| crates/galleon-voice/tests/parse.rs | 1 |
| crates/guardian-tick/tests/cli.rs | 9 |
| crates/link-watch/tests/config_fixture.rs | 2 |
| crates/link-watch/tests/golden_keys.rs | 1 |
| crates/one-defence/tests/decision.rs | 1 |
| crates/one-defence/tests/golden.rs | 1 |
| crates/one-set-model/tests/temp_file.rs | 3 |
| crates/openmind-sandbox/src/lib.rs | 1 |
| crates/openmind-sandbox/tests/sandbox.rs | 2 |
| crates/openmind-survival/src/probes.rs | 1 |
| crates/openmind-units/tests/render.rs | 1 |
| crates/room/tests/no_bind.rs | 1 |
| crates/tuner/tests/serve.rs | 1 |

Notes, same order. A hit is a call site in source, not proof a process was spawned by this ticket.

- `crates/gaia-core/tests/cli.rs`: one test builds `Command::new(BIN)`.
- `crates/galion-desk/src/main.rs`: `mindpack` builds `Command::new(bin)` from the hermes binary path.
- `crates/galion-one/src/main.rs`: five calls. Programs in source are curl, pdftotext, ps, nvidia-smi, and sha256sum.
- `crates/galion-one/src/models_refresh.rs`: `curl_fetch` builds `Command::new("curl")`.
- `crates/galleon-arena/src/cli.rs`: `read_zagreb_now` builds `Command::new("date")`.
- `crates/galleon-arena/tests/list.rs`: one test builds `Command::new(bin)`.
- `crates/galleon-voice/tests/parse.rs`: one test builds `std::process::Command::new("strace")`.
- `crates/guardian-tick/tests/cli.rs`: eight calls use `Command::new(BIN)` and one uses `Command::new("/bin/sleep")`.
- `crates/link-watch/tests/config_fixture.rs`: two tests build `Command::new` on the link-watch test binary.
- `crates/link-watch/tests/golden_keys.rs`: one test builds `Command::new` on the link-watch test binary.
- `crates/one-defence/tests/decision.rs`: one test builds `Command::new` on the one-defence test binary.
- `crates/one-defence/tests/golden.rs`: one test builds `Command::new` on the one-defence test binary.
- `crates/one-set-model/tests/temp_file.rs`: three tests build `Command::new` on the one-set-model test binary.
- `crates/openmind-sandbox/src/lib.rs`: `run` builds `Command::new(prog)`.
- `crates/openmind-sandbox/tests/sandbox.rs`: two tests build `std::process::Command::new("/bin/sleep")`.
- `crates/openmind-survival/src/probes.rs`: `SystemRunner::run` builds `Command::new(program)`.
- `crates/openmind-units/tests/render.rs`: one test builds `Command::new(bin)`.
- `crates/room/tests/no_bind.rs`: one test builds `Command::new("unshare")`.
- `crates/tuner/tests/serve.rs`: one test builds `Command::new` on the tuner test binary.

## Not a call site

These three lines contain the spelling inside quotes. They are not call sites and are not hits.

| path | spellings |
| --- | --- |
| crates/link-watch/tests/decision.rs | 1 |
| crates/openmind-llama/tests/parse.rs | 1 |
| crates/openmind-mcp-stack/tests/stdio.rs | 1 |

- `crates/link-watch/tests/decision.rs`: a banned-spelling list.
- `crates/openmind-llama/tests/parse.rs`: a banned-spelling list.
- `crates/openmind-mcp-stack/tests/stdio.rs`: an assert that source text does not contain the spelling.

## Not claimed

A hit is a call site in source, not proof a process was spawned by this ticket. This scan did not run any binary. No process was started by this ticket. No crate file was edited. Nothing was committed. Nothing was pushed. cargo was not run. systemctl was not used.
