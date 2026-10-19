<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Crate index

- DESK-ID: openmind.INDEX-2
- Written: 2026-10-19T19:54:48+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- One row per directory under crates/. name and description match that crate Cargo.toml. A missing description is none. No Cargo.toml was edited.

| name | description |
| --- | --- |
| cgroup-detect | Read-only cgroup v2 delegation probe. Reports whether a per-run cgroup is possible. Never creates a cgroup. |
| desk-hermes | Rust port of the desk-hermes mind snap (was /opt/galion/bin/desk-hermes.py) |
| gaia-core | GAIA core dry-run orchestrator and the gaia binary: status, one tick, simulate-faro text. Master override never restarts a service. |
| gaia-registry | Static ownership table: each workspace crate maps to one of nine Gaia functions. Unknown names return none. |
| galion-desk | galion-desk: the operator's desk endpoint. Reads the pod, never writes it. |
| galion-one | Galion One: the operator's own ear, mind and mouth on one box. |
| galleon-arena | Read-only paper arena: list seed gladiators and score a match. Paper integers only. No network. |
| galleon-voice | Persona Plex v1 document loader. Traits only: text persona, voice name, session policy. No audio. |
| guardian-tick | Report-only guardian tick: runs a TOML probe set once via openmind-survival, writes a JSON report, optionally prints suggestions. JSONL is off unless --jsonl PATH or GUARDIAN_JSONL is set. |
| hades-gate | Pure approval gate. Dry-run by default. WouldApply is a decision only and never runs a command. kill and rotate are always refused. |
| hades-journal | One JSON record of a hades-gate decision. Stores nothing on disk. Default is DryRun. kill and rotate are Refused. |
| link-watch | Rust port of the laptop link watcher (was /opt/galion/galion-light/scripts/link-watch.py) |
| one-defence | Read-only port of galion-light/one/defence.py: newest health_checks row per service. |
| one-set-model | Validate a plan or action lane and return the modes JSON set-model.py would write. Reads a caller path. Writes nothing. |
| one-talk | Hear/reply request shapes from talk.py, and the warm.py lock path plus synthesize body. No pairing, no secret files, no Hermes state.db. |
| openmind-boot | Read-only hardware probe: CPU, RAM, and GPU presence from /proc and /sys. hardware.json is written only under a temp dir. |
| openmind-config | One small config loader for OpenMind binaries: TOML file, then env overrides, loopback defaults. |
| openmind-discovery | LAN discovery stub: static seed candidates plus an mDNS-SD fixture parser. Active scan is behind --scan and sends no packets. |
| openmind-exo | Parse exo fixture bodies for an OpenAI-compatible /v1/models and /health. No sockets and no GPU. |
| openmind-fallback | Pure provider fallback policy: GPU, else an approved LAN row, else a CPU local row. Tests only. No network. |
| openmind-llama | Parse llama.cpp llama-server fixture bodies for /health and /v1/models. No sockets and no GPU. |
| openmind-mcp-stack | Read-only stdio MCP stub. initialize and tools/list for stack_status. Fixture JSON only. No sockets. |
| openmind-memory | Fixture-backed memory store and search for the Hermes gateway subset. |
| openmind-nerve | Append-only JSONL nerve log: Europe/Zagreb timestamps, size rotation, caller-supplied path. |
| openmind-origin | Pure function that builds an HTTP 302 redirect. Location comes from the caller; the default is /. The body is empty. No listener. |
| openmind-provider | ComputeProvider trait and a JSON registry of approval state (Pending, Approved, Rejected). |
| openmind-route | Choose one provider row for a need: GPU, else an approved LAN row, else a CPU local row. Pending candidates are not chosen. No network. |
| openmind-sandbox | Run one command under rlimits, a cleared env, a fixed workdir, a wall timeout and (when the kernel has it) Landlock. |
| openmind-schema | Galion Open Mind stack contract. The processor runs this. The card only runs the model. |
| openmind-speech | Parse whisper.cpp and piper fixture outputs on CPU. No process, no download, no GPU. |
| openmind-survival | Std-only read-only survival probes (HTTP, systemctl is-active, disk, /proc, freshness), pure incident logic, and suggestion-only healer decisions with graceful degradation. |
| openmind-tulu | Parse a DR Tulu llama-server fixture TOML. No process, no socket, no GPU. |
| openmind-units | Render staged unit text from an openmind.toml fixture. Prints text only and does not install anything. |
| persona-plex-relay | Persona Plex v2: VPS relay scaffold for the voice-to-voice channel (auth, session, speaker verification policy; no inference). |
| room | room: a read-only view of the operator's files as one page. |
| tuner | Ukulele tuner static server. Bind and root are config-driven; defaults match the live tuner. |
| zagreb-time | Europe/Zagreb time helpers: every timestamp in the stack is RFC 3339 with an explicit offset, never Z. |
