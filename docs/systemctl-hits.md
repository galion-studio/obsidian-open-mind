<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Systemctl hits

- DESK-ID: openmind.CTL-HITS
- Written: 2026-10-19T19:59:35+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: regular `*.rs` and `*.toml` files under crates/. Other suffixes were not searched.
- Scanned files: 161
- Search: case-sensitive fixed string `systemctl`. The command was not executed.
- Note: A hit is a word, not proof a unit was restarted.

## Count

| item | value |
| --- | --- |
| files | 13 |
| matching lines | 30 |
| hits | 30 |

Each hit is one occurrence of the spelling. One of the 30 is the last component of the identifier `fixture_units_restart_always_zagreb_stamp_no_systemctl` in `crates/openmind-units/tests/render.rs`. A search that treats an underscore as part of a word reports 29. Both readings are text only. A hit is a word, not proof a unit was restarted.

The stamp above is the time of this scan. The offset is numeric (+02:00 or +01:00). It is not a trailing UTC designator.

## Paths

A hit is a word, not proof a unit was restarted. No crate was edited. The command was not executed.

| path | count |
| --- | --- |
| crates/gaia-core/src/lib.rs | 1 |
| crates/gaia-core/tests/cli.rs | 3 |
| crates/guardian-tick/tests/cli.rs | 3 |
| crates/hades-gate/tests/decide.rs | 1 |
| crates/hades-journal/tests/record.rs | 1 |
| crates/link-watch/tests/decision.rs | 3 |
| crates/openmind-survival/Cargo.toml | 1 |
| crates/openmind-survival/src/lib.rs | 1 |
| crates/openmind-survival/src/probes.rs | 2 |
| crates/openmind-survival/tests/decide.rs | 2 |
| crates/openmind-survival/tests/survival.rs | 1 |
| crates/openmind-units/src/lib.rs | 1 |
| crates/openmind-units/tests/render.rs | 10 |

Notes, same order. A hit is a word, not proof a unit was restarted.

- `crates/gaia-core/src/lib.rs`: a test asserts simulate-faro text does not contain the spelling.
- `crates/gaia-core/tests/cli.rs`: tests assert CLI text does not contain the spelling.
- `crates/guardian-tick/tests/cli.rs`: tests assert CLI text does not contain the spelling.
- `crates/hades-gate/tests/decide.rs`: a banned-spelling list. The test expects the gate source not to contain it.
- `crates/hades-journal/tests/record.rs`: a banned-spelling list. The test expects the journal source not to contain it.
- `crates/link-watch/tests/decision.rs`: a banned-action list, plus two asserts that rendered text does not contain the spelling.
- `crates/openmind-survival/Cargo.toml`: crate description text names an is-active probe.
- `crates/openmind-survival/src/lib.rs`: crate docs name an is-active command as read-only.
- `crates/openmind-survival/src/probes.rs`: a doc comment and a string argument for is-active. This scan did not run that string.
- `crates/openmind-survival/tests/decide.rs`: a banned spelling in suggestion text and in a source scan.
- `crates/openmind-survival/tests/survival.rs`: a fake runner asserts the program string is this spelling or df.
- `crates/openmind-units/src/lib.rs`: a forbidden-token constant.
- `crates/openmind-units/tests/render.rs`: asserts, a description string, a doc line, and the identifier `fixture_units_restart_always_zagreb_stamp_no_systemctl`.

## Not claimed

These hits are words in comments, strings, tests, one crate description, and one identifier. A hit is a word, not proof a unit was restarted. No unit was started, stopped, or restarted. The command was not executed. No crate file was edited. Nothing was committed. Nothing was pushed. cargo was not run.
