<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Gaia ownership registry

- DESK-ID: openmind.GAIA-3
- Written: 2026-10-19T17:45:04+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: crates/gaia-registry
- Lookup: `gaia_registry::owner` returns one of the nine functions, or none.

Unknown crate names return none. The match is exact: different case, an underscore, or extra space is none. Each crate below is listed once.

## The nine functions

These spellings are the only ones `Function::from_name` accepts. `none` is not a function.

| Name | Domain in this registry |
| --- | --- |
| GAIA | Orchestration. The core, this table, and the Zagreb clock lineage stamps use. |
| HADES | Destructive plans only. Assigned the dry-run approval gate. It classifies a plan and never runs a command. |
| HEPHAESTUS | Fabrication: the desk, the One build helpers, compute adapters, the sandbox runner, per-run cgroup limits, the redirect builder, and the tuner page. |
| DEMETER | Environment health: survival probes, config, the routine guardian tick, the hardware probe, and staged unit text. |
| ARTEMIS | Agent population: the Hermes mind snap and the room. |
| APOLLO | Knowledge: the stack contract, link intake, LAN seeds, the nerve archive, memory fixtures, and the Tulu row. |
| MINERVA | Security audit and policy. Assigned the read-only stack status stub. It opens no socket and reads no foreign tree. |
| ELEUTHIA | Personas and the channel that carries them, including Galion One, the speech fixtures, and the relay scaffold. |
| ARES | Contest. The paper arena only. No value moves. |

The backlog seed also used two domain labels that are not in this nine. Those labels are not accepted here. Crates that sat under them are placed on the nearest of the nine, and the reason is in the table.

## Crate table

`ls crates` at 2026-10-19T17:45:04+02:00 listed these thirty-six directories. Alphabetical, one row each. `crates/gaia-core` is named here and was not edited.

| Crate | Function | Why |
| --- | --- | --- |
| `cgroup-detect` | HEPHAESTUS | Read-only probe of whether a per-run cgroup is possible. Limits for the forge. Creates nothing. |
| `desk-hermes` | ARTEMIS | Hermes mind snap. Agent population. Opens its state file read-only. |
| `gaia-core` | GAIA | Dry-run orchestrator: status, one tick, simulate-faro text. This ticket does not edit it. |
| `gaia-registry` | GAIA | This ownership table. |
| `galion-desk` | HEPHAESTUS | Operator desk endpoint. Reads the pod, does not write it. |
| `galion-one` | ELEUTHIA | The persona the stack embodies: ear, mind, and mouth. |
| `galleon-arena` | ARES | Paper contest for seed homes g2-0 through g2-3. Integers only. No network. |
| `galleon-voice` | ELEUTHIA | Persona document loader: text, voice name, session policy. No audio. |
| `guardian-tick` | DEMETER | One report-only health pass. Suggestions, not restarts. |
| `hades-gate` | HADES | Pure approval gate for destructive plans. Dry-run only. WouldApply never runs a command. kill and rotate are always refused. |
| `link-watch` | APOLLO | Link intake. The address comes from config. |
| `one-defence` | HEPHAESTUS | Read-only health-check rows that ship with the One build. Writes no database. |
| `one-set-model` | HEPHAESTUS | Checks a plan or action lane for the One build. Writes nothing. |
| `one-talk` | HEPHAESTUS | Hear and reply shapes for the One build. No pairing store. |
| `openmind-boot` | DEMETER | Read-only hardware probe: CPU, RAM, and GPU presence. hardware.json is written only under a temp dir. |
| `openmind-config` | DEMETER | Config health: TOML, then env, loopback defaults. |
| `openmind-discovery` | APOLLO | Static seed list of LAN peers. The scan path sends no packets. |
| `openmind-exo` | HEPHAESTUS | Fixture parser for an exo compute adapter. No socket and no GPU. |
| `openmind-fallback` | HEPHAESTUS | Pick GPU, else an approved LAN row, else CPU local. No network. |
| `openmind-llama` | HEPHAESTUS | Fixture parser for a llama.cpp adapter. No socket and no GPU. |
| `openmind-mcp-stack` | MINERVA | Read-only stdio MCP stub for stack_status. Fixture JSON only. No sockets. |
| `openmind-memory` | APOLLO | Fixture-backed memory store and search. The live gateway is never dialed. |
| `openmind-nerve` | APOLLO | Append-only archive of nerve events. Opens no socket. |
| `openmind-origin` | HEPHAESTUS | Builds one HTTP 302 redirect. Location comes from the caller; empty uses /. The body is empty. No listener. |
| `openmind-provider` | HEPHAESTUS | Compute provider trait and approval state. Stores no key material. |
| `openmind-route` | HEPHAESTUS | Chooses one provider row: GPU, else an approved LAN row, else CPU local. Pending candidates are not chosen. No network. |
| `openmind-sandbox` | HEPHAESTUS | Runs one command under rlimits, a cleared env, a workdir, and a timeout. |
| `openmind-schema` | APOLLO | The stack contract. |
| `openmind-speech` | ELEUTHIA | Parses whisper.cpp and piper fixture outputs on CPU. No process, no download, no GPU. |
| `openmind-survival` | DEMETER | Read-only probes and suggestion-only decisions. |
| `openmind-tulu` | APOLLO | Parses a DR Tulu llama-server fixture TOML. No process, no socket, no GPU. |
| `openmind-units` | DEMETER | Renders staged unit text from an openmind.toml fixture. Prints text only. Does not install a unit or start a process. |
| `persona-plex-relay` | ELEUTHIA | Persona channel scaffold. No inference and no socket in this scaffold. |
| `room` | ARTEMIS | Read-only page of the operator files, the place agents share. |
| `tuner` | HEPHAESTUS | Static file server for the ukulele tuner page. Bind and root are config-driven. Does not start the live tuner. |
| `zagreb-time` | GAIA | Europe/Zagreb RFC 3339 for lineage. Explicit offset, never a trailing Z. |

## Lookup

- `owner("galion-one")` is `Some(Function::Eleuthia)`, spelling `ELEUTHIA`.
- `owner("not-a-crate")` is none.
- `owner("hades-gate")` is `Some(Function::Hades)`, spelling `HADES`.
- `owner("openmind-mcp-stack")` is `Some(Function::Minerva)`, spelling `MINERVA`.
- An unknown string, including `none`, is not a function and returns none.
