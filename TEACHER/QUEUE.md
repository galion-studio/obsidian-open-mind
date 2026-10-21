# QUEUE: openmind boss work queue (B01..B50)

Written 2026-10-18T20:00:40+02:00 (CEST) by Grok Bot (teacher). Reordered the same minute for Slavik's priority update.
Boss: Hermes (`openmind-boss-20261018`), see `TEACHER/BOSS.md`. Checks: `TEACHER/CHECKS.md`. Repo: `/opt/galion/openmind`.

Sources (to-do lists and open items only): `docs/main.md` (sections 3, 5, 10-15), `docs/goals.md` (Goals, To-do, project to-dos),
`docs/session-2026-10-18.md` and `docs/session-2026-10-18-evening.md` (open items tables), `TEACHER/STAGED.md` (open decisions),
`release/TEACHER/AWAITING-APPROVAL.md`, `TEACHER/A-foundation.md` (section 4 gaps, section 5 compute).
Excluded on purpose: anything needing money or GPU spend, GitHub push/repo creation, publishing, production switches, DNS, secrets, Revolut.

## Rules every ticket inherits
- HARD RULES = `BOSS.md` section 3. No Python. Zagreb timestamps. No CJK. No secrets. MIT + SPDX header on new files.
- Production-facing results are **staged only**: unit files without `[Install]` under `deploy/`, plus a section in `TEACHER/STAGED.md` marked "awaiting Slavik's approval". Nothing is installed, enabled, restarted or switched.
- Every ticket that builds a binary records **binary size** (`ls -l target/release/<bin>`, `file` says stripped) and **peak RSS** of one test run
  (`/usr/bin/time -v <cmd>` "Maximum resident set size" if GNU time exists, else `ps -o rss= -p <pid>` while it runs) in its report and in BOSS-PROGRESS.md.
- Target profile for Group 1-4 binaries: low-end, CPU-only commercial devices (no GPU): stripped binary < 2 MB where practical, RSS < 10 MB idle, std or small crates only (no tokio/axum/hyper unless the ticket says so), no hard-coded hosts or IPs.
- Builds: `nice -n 19 cargo ... -j 2`. Tests never touch live ports or live state files; write test output under `target/test-out/`.
- Size: S = one short subagent run; M = one longer subagent run. Nothing bigger.

## Wave order (max 3 per wave, no two tickets editing the same file in one wave)
W1: B01, B10, B17 | W2: B02, B18, B26 | W3: B11, B19, B27 | W4: B03, B12, B20 | W5: B04, B13, B28 | W6: B05, B14, B21
W7: B06, B15, B29 | W8: B07, B16, B22 | W9: B08, B23, B30 | then remaining tickets in id order, P1 before P2 before P3.

---

## Group 1: Rust foundation for low-end CPU-only commercial devices (priority 1 of Slavik's list)

### B01 | G1 | P1 | S | link-watch: no hard-coded public host
- Goal: remove `DEFAULT_FRANCE` (`http://<France server IP, redacted>`) from `crates/link-watch/src/main.rs`. The probe URL comes only from config/env (`LINK_WATCH_FRANCE_URL`); when unset the probe reports `"unconfigured"` instead of calling a fixed IP. Ollama/Hermes loopback defaults stay but move to one `Defaults` struct.
- Files: edit `crates/link-watch/src/main.rs` (+ tests in the crate). Add a STAGED.md note (behaviour change for the staged unit: env line needed).
- Accept: `rg -n '54\.37\.161\.82' crates/` empty; existing JSON keys unchanged (golden test vs a fixture from the current output schema); C1 C2 C3 C6 C7 C8; size + RSS recorded.

### B02 | G1 | P1 | M | `openmind-config` crate
- Goal: one small config loader for all binaries: TOML file at `$OPENMIND_CONFIG` (default `/etc/openmind/openmind.toml`; missing file = built-in defaults), then env overrides; typed sections `hosts`, `ports`, `paths`, `limits`. Deps: `serde` + `toml` only.
- Files: new `crates/openmind-config/` (Cargo.toml, src/lib.rs, LICENSE, example `openmind.example.toml`).
- Accept: tests for default, file, env override, bad file -> clear error; no IPs other than 127.0.0.1 in defaults; C1 C2 C3 C7 C8; lib adds < 300 KB to a release binary (measure with link-watch before/after if B03 lands, else note).

### B03 | G1 | P1 | S | link-watch reads `openmind-config`
- Goal: link-watch takes probe URLs/out path from `openmind-config` (env still wins). Depends on B01, B02.
- Files: `crates/link-watch/`.
- Accept: golden JSON keys unchanged; config fixture test; size + RSS recorded; C1 C2 C3 C6.

### B04 | G1 | P1 | S | Portability inventory
- Goal: read-only table of every hard-coded host, port, absolute path and model id in `crates/` -> `docs/portability-inventory.md` (file:line, value, proposed config key, risk). Feeds B05-B07.
- Accept: every `rg -n '127\.0\.0\.1|/opt/galion|/workspace/|:[0-9]{4}' crates/ --glob '*.rs'` hit classified; Zagreb timestamp in header; C7.

### B05 | G1 | P2 | M | galion-desk config-driven
- Goal: desk bind, Ollama URL, state paths from `openmind-config` with today's values as defaults (identical default behaviour).
- Files: `crates/galion-desk/`.
- Accept: C5 behaviour diff on 18646 for GET `/health`, `/desk` vs live 8646 read-only; C1-C4; size + RSS.

### B06 | G1 | P2 | M | galion-one config-driven paths
- Goal: replace `/workspace/galion-light/...` and fixed ports in galion-one with config keys (defaults = today). No behaviour change.
- Files: `crates/galion-one/`. Not in the same wave as B11-B14.
- Accept: C5 on 18091 read-only routes; C1-C4; size + RSS.

### B07 | G1 | P2 | S | openmind-schema provider rows from config
- Goal: `Stack::cpu_desk()` hard-coded `base_url` -> built from `openmind-config` provider list; keep the existing no-`sk-` test.
- Accept: C1-C3; old test passes.

### B08 | G1 | P2 | S | Low-end release profile + size budget check
- Goal: `scripts/size-budget.sh` (bash) that builds release `-j 2` and fails if any bin > 2 MB or not stripped; doc the chosen profile in `docs/low-end-profile.md` (opt-level, lto, panic=abort; musl static as an option documented, not required).
- Accept: script exits 0 on today's tree; table of current sizes with Zagreb timestamp; C7 C8.

### B09 | G1 | P3 | S | RSS baseline for all binaries
- Goal: `docs/rss-baseline.md`: peak RSS of each workspace binary in a test run on spare ports (never live), method documented.
- Accept: one row per binary; test processes killed (C9).

## Group 2: Remove the last Python calls (priority 2)

### B10 | G2 | P1 | S | Python call-site audit
- Goal: read-only list of every place the Rust code still launches or depends on Python: the 4 `ONE_LEGACY_PY` paths in `crates/galion-one` (tts/worker.py voice worker, one/models.py refresh, etc.), the `origin.py` text check, plus the helpers `galion-light/one/defence.py`, `talk.py`, `warm.py` (and `set-model.py`, `trace.py`): who calls them, what they read/write, and the Rust replacement for each -> `docs/legacy-python.md`.
- Accept: each row has file:line, inputs, outputs, side effects (DB writes flagged), proposed Rust crate/function; C7; no Python executed.

### B11 | G2 | P1 | M | Rust replacement for one/models.py refresh
- Goal: port the models refresh (per B10) to a Rust function/bin with the same output file format; galion-one calls Rust, the `ONE_LEGACY_PY` branch for it is removed.
- Accept: golden output vs current file format (fixture, never overwrite the live file; write under target/test-out); C1-C3 C8; size + RSS.

### B12 | G2 | P1 | M | Rust replacement for defence.py
- Goal: port `one/defence.py` logic (read-only reads of the defence data) to Rust (`crates/one-defence` or a module), JSON-compatible output.
- Accept: golden test on a fixture; DB opened read-only; C1-C3 C8; size + RSS.

### B13 | G2 | P1 | M | Rust replacement for talk.py and warm.py helpers
- Goal: port the talk/warm helper behaviour that galion-one or the voice path still needs (per B10) to Rust; never write to Hermes state.db (the old pruning stays removed).
- Accept: same request/response shape against a fake local endpoint on a spare port; C1-C3 C8; size + RSS.

### B14 | G2 | P1 | S | Retire the legacy TTS worker spawn
- Goal: remove the `tts/worker.py` spawn path and the `ONE_LEGACY_PY`/`ONE_LEGACY_PY_BIN` gate entirely; voice-worker-unavailable returns the same JSON error as today's "off" path. Record in STAGED.md.
- Accept: `rg -n -i 'python|\.py' crates/ --glob '*.rs'` only doc comments naming replaced scripts; C1-C4 C8.

### B15 | G2 | P2 | S | set-model.py and trace.py
- Goal: if B10 shows they are called by anything we ship, port to a small Rust bin; else document as not shipped.
- Accept: per B10; C8.

### B16 | G2 | P2 | S | No-Python gate in self-check
- Goal: `scripts/no-python.sh`: fails on any `.py` file or Python spawn (`Command::new("python`, `python3`) in crates/ and deploy/.
- Accept: exits 0 after B14; exits 1 on a temp fixture (removed after test).

## Group 3: Rust guardian + healer, Survival Skills (priority 3)

### B17 | G3 | P1 | S | Guardian/healer study (read-only)
- Goal: read `galion-light/scripts/guardian-tick.py`, `galion-light/guardian/desk_heal.py`, `guardian/upstream/{guardian,auto_healer,diagnostics}.py`, the schema of `guardian/guardian.db` (`sqlite3 -readonly ... .schema` only) and the `galion/survival` skill; list every probe, every action, and why they stopped (8-12 Oct, from mtimes and logs, read-only) -> `docs/survival-rust-plan.md`. Mark which actions are excluded: secret sealing, process killing, restarts.
- Accept: probe table (name, target, interval, pass condition); guardian.db table/column list; never run the scripts; C7.

### B18 | G3 | P1 | M | `openmind-survival` probe crate
- Goal: library with read-only probes: HTTP status on loopback, `systemctl is-active <unit>` (read-only), disk free, mem/load from /proc, file freshness (mtime age). Each returns `ProbeResult { name, ok, detail, ts }` with `ts` from `zagreb-time`.
- Accept: unit tests with fixtures (no live network in tests); C1-C3 C6 C8; no tokio.

### B19 | G3 | P1 | M | `guardian-tick` Rust bin (report only)
- Goal: runs the probe list once (from a TOML probe file, B20), writes JSON report to a configurable path (default `target/test-out/guardian.json`) and optionally guardian.db-compatible rows into a COPY of the DB under target/test-out (never the live DB). `--suggest` prints healer suggestions; it never kills, restarts or seals anything.
- Accept: run once against loopback read-only; size + RSS (< 2 MB, < 10 MB); C1-C4 C6 C8 C9.

### B20 | G3 | P1 | S | Healer probe list as data
- Goal: the probes from desk_heal.py / guardian-tick.py as `deploy/survival/probes.toml`; no shell commands that mutate anything.
- Accept: parsed by B19 test; every probe read-only; C7.

### B21 | G3 | P1 | S | Staged Restart=always units
- Goal: `deploy/survival/guardian-tick.service` + `.timer` and drop-ins `deploy/survival/<svc>.d/10-restart.conf` (`Restart=always`, `RestartSec=5`, start-limit) for galion-one, galion-desk, link-watch, console, tuner. No `[Install]`; STAGED.md section "Survival units, awaiting Slavik's approval".
- Accept: `systemd-analyze verify` on the files (read-only) or documented reason if unavailable; `systemctl is-enabled guardian-tick` = not-found; C11.

### B22 | G3 | P2 | S | Service survival audit
- Goal: read-only table: each service's Restart=, is-enabled, WantedBy, boot survival; `one` under nohup gap -> `docs/survival-audit.md`.
- Accept: every service in BOSS.md rule 6 listed; C7.

### B23 | G3 | P2 | S | Healer decision engine (pure)
- Goal: pure function `decide(results) -> Vec<Suggestion>` with escalation levels (observe, warn, suggest-restart); suggestions only, no execution.
- Accept: table-driven tests; C1-C3.

### B24 | G3 | P2 | S | Graceful degradation rules
- Goal: rules for loss of GPU/LAN/network -> CPU-light fallback, as data + tests in `openmind-survival`.
- Accept: tests; doc section in survival-rust-plan.md.

### B25 | G3 | P3 | S | Guardian report -> nerve event
- Goal: guardian-tick can also emit `NerveEvent` lines (B33) to a configurable JSONL path, off by default.
- Accept: test; C6.

## Group 4: Agentic sandbox, CPU-only (priority 4)

### B26 | G4 | P1 | S | Sandbox design
- Goal: `docs/agentic-sandbox.md`: CPU-only sandbox for agent tool execution. Compare: Linux namespaces (user/mount/pid/net), seccomp, landlock, cgroups v2 limits, rlimits, vs wasmtime/WASI. Check what this LXC kernel allows (read-only: `uname -r`, `/proc/sys/kernel/unprivileged_userns_clone` if present, landlock ABI via `/sys/kernel/security/lsm`, cgroup v2 mount). Pick a layered first version that builds fast on low-end CPUs (note wasmtime compile cost).
- Accept: decision + threat model + what is enabled where; C7; nothing enabled.

### B27 | G4 | P1 | M | `openmind-sandbox` crate v0
- Goal: `SandboxPolicy { workdir, env_allowlist, wall_timeout, cpu_secs, mem_bytes, nofile, net: bool, fs_read, fs_write }` and `run(cmd, policy) -> Outcome` using rlimits + cleared env + fixed workdir + kill-on-timeout of the child it spawned (its own child only), with landlock when available (skip gracefully when not). Small crates only (`rustix` or `libc`, optional `landlock`).
- Accept: tests: timeout kills its own child, memory limit enforced, env cleared, write outside workdir denied when landlock available (test reports skipped otherwise); C1-C3 C8; size + RSS of a demo bin.

### B28 | G4 | P2 | M | seccomp/namespace layer (optional feature)
- Goal: feature `strict` adding a seccomp deny-list and an unprivileged user+net namespace when the kernel allows; off by default.
- Accept: tests skip cleanly when unsupported; C1-C3.

### B29 | G4 | P2 | S | cgroup v2 limits (detect only)
- Goal: detect cgroup v2 delegation and report whether per-run cgroups are possible; never create cgroups under system slices.
- Accept: read-only detection test; doc update.

### B30 | G4 | P2 | S | Sandbox CLI demo
- Goal: `openmind-sandbox run --policy p.toml -- <cmd>` printing an Outcome JSON with Zagreb ts; size + RSS.
- Accept: demo with `/bin/true` and a timeout case; C1-C4 C6.

### B31 | G4 | P3 | S | Sandbox policy presets
- Goal: presets `read-only-tool`, `build-tool`, `net-tool` in TOML with docs.
- Accept: parsed in tests.

## Group 5: Universal compute plug-in, nerve/cortex time

### B32 | G5 | P1 | S | LAN plug-in interface design
- Goal: `docs/lan-plugin.md`: mDNS/LAN discovery, explicit approval (no device joins without approval), register as OpenMind Provider row, tunnel to VPS (design only).
- Accept: flow + data model aligned with openmind-schema `Provider`; C7.

### B33 | G5 | P1 | S | `NerveEvent` schema
- Goal: in openmind-schema: `NerveEvent { ts (Zagreb RFC 3339), source, kind, severity, data }`; serde; tests incl. DST 2026-10-25.
- Accept: C1-C3 C6.

### B34 | G5 | P1 | M | `openmind-provider` trait + registry
- Goal: trait `ComputeProvider { id, kind, endpoint, health(), capabilities() }`, registry with approval state (Pending/Approved/Rejected) persisted as JSON at a configurable path. No network in tests.
- Accept: tests; C1-C3.

### B35 | G5 | P2 | M | `openmind-discovery` stub
- Goal: candidates from a static seed list + parser for mDNS-SD style records (fixtures); active scan behind `--scan`, off by default, never run in tests.
- Accept: fixture tests; C1-C3; size + RSS.

### B36 | G5 | P2 | S | llama.cpp adapter (rpc-server / llama-server)
- Goal: `LlamaCppProvider` parsing `/health` and `/v1/models` fixtures. No live calls, no GPU.
- Accept: tests.

### B37 | G5 | P2 | S | Exo adapter (Apple Silicon)
- Goal: `ExoProvider` for exo's OpenAI-compatible API (fixtures); note in doc that exo v1 GPU on Linux is in development (A-foundation.md section 5).
- Accept: tests.

### B38 | G5 | P2 | S | Provider fallback policy
- Goal: `select_provider(registry, need)` GPU -> LAN -> CPU local fallback; tests.
- Accept: tests.

### B39 | G5 | P2 | S | Timestamp audit
- Goal: read-only list of every UTC/epoch output in crates/ and state files they write -> `docs/timestamps-audit.md` with fix per row.
- Accept: C6 grep evidence; C7.

### B40 | G5 | P2 | M | `openmind-nerve` JSONL writer
- Goal: append-only JSONL with Zagreb ts, size rotation, configurable path; tests.
- Accept: C1-C3 C6.

### B41 | G5 | P3 | S | Cortex design
- Goal: `docs/cortex.md`: signals (vulnerabilities, bleeding resources, DDoS), read-only sources (auth.log counts, memory-gateway /health rate), thresholds, nerve schema link.
- Accept: C7.

## Group 6: Open source + security, Galleon Voice to Voice, Galleon Arena

### B42 | G6 | P1 | S | SPDX/REUSE headers
- Goal: SPDX MIT headers on every `.rs`, `LICENSES/MIT.txt`, `REUSE.toml` for non-commentable files. Do NOT install the Python `reuse` tool; check with rg. Run alone in its wave (touches many files).
- Accept: `rg -L 'SPDX-License-Identifier: MIT' crates --glob '*.rs'` empty; C1-C3.

### B43 | G6 | P1 | S | Self-check script
- Goal: `scripts/self-check.sh` running C1-C3, C6-C8, SPDX, no-python, secret scan (prints file:line only, never the match), size budget.
- Accept: exit 0 on a clean tree; nice -j 2.

### B44 | G6 | P2 | S | SBOM config
- Goal: `cargo metadata --format-version 1` + jq -> `dist/sbom-cargo.json` + license allowlist check (MIT/Apache-2.0/BSD/ISC/Zlib/Unicode); CI step draft in `.github/` (not pushed).
- Accept: report with every dep license; no `cargo install`.

### B45 | G6 | P2 | S | SECURITY.md + OpenSSF checklist
- Goal: review existing SECURITY.md (no invented contacts; placeholder marked TODO) and `docs/openssf.md` honest status map.
- Accept: C7; no secrets.

### B46 | G6 | P2 | S | Galleon Voice to Voice integration design
- Goal: `docs/voice-to-voice.md`: Galion One front end -> VPS relay -> external GPU PersonaPlex (design only, no GPU spend), Persona Plex v1 mapping, license fit from goals.md.
- Accept: facts traced to goals.md/persona file; C7.

### B47 | G6 | P2 | S | Persona Plex v1 machine-readable
- Goal: `personas/persona-plex-v1.toml` mirroring persona-plex-v1.md (incl. reply length one or two short sentences) + parse test in `galleon-voice` crate scaffold (traits only, no audio).
- Accept: C1-C3.

### B48 | G6 | P2 | S | Galleon Arena design
- Goal: `docs/arena.md`: gladiator roles from personas/workers/teams, rules, scoring tied to the vending machine benchmark (paper only, no money).
- Accept: C7.

### B49 | G6 | P3 | M | `galleon-arena` crate scaffold
- Goal: `Gladiator`, `Match`, `Score`; load g2-0..g2-3 SOUL.md/TASK.md from the redacted copy under `stack/agents/personas/arena/homes` (read-only); deterministic scoring tests.
- Accept: C1-C3.

### B50 | G6 | P3 | S | Arena CLI
- Goal: `galleon-arena list` prints gladiators with Zagreb timestamp; size + RSS.
- Accept: C1-C4 C6.

---
Ticket count by group: G1 9 (B01-B09), G2 7 (B10-B16), G3 9 (B17-B25), G4 6 (B26-B31), G5 10 (B32-B41), G6 9 (B42-B50). Total 50.
Budget: $5 session cap (BOSS.md section 6). At current prices only the first waves fit; the queue is meant to continue across sessions.
