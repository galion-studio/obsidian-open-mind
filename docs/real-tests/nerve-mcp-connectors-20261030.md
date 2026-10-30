<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: nerve watches the Hermes MCP connectors (2026-10-30)

Branch `swarm/nerve-mcp-connectors-20261030`, crate `openmind-nerve`.
All times Europe/Zagreb (CET, +01:00).

## What it is

New sensor `mcp-connectors` (`src/sensors/mcp_connectors.rs`). Off unless
`NERVE_MCP_SERVERS_FILE` names the Hermes servers file. Read-only, no model
call, no secret:

- the servers file (`mcp_servers:` block; `enabled: false` entries dropped);
- token files: `stat` only (present / missing / empty / mode), never opened;
- `systemctl show <unit> -p MainPID -p ActiveState -p ExecMainStartTimestamp`;
- `journalctl -u <unit> -o short-unix --since=@<cursor>` for the Hermes
  start-up lines `mcp: <name>: N tools (...)` and `mcp: <name>: NOT CONNECTED: ...`;
- `/proc/<hermes pid>/task/*/children` to see that a stdio server still runs.

States: `healthy`, `configured`, `waiting` (token file missing: no alert,
no score cost), `starting` (Hermes start-up grace), `failing`. Only a server
that was healthy and then fails raises a warn incident
`mcp-connector:<name>` (5 points, alert key `incident:<id>:warn`, the usual
30 min dedupe). Errors are sanitised (token-like strings and addresses
redacted, 240 chars max).

API: `GET /api/v1/connectors` (also `/connectors`), `/state` top-level
`connectors` and `system.connectors` (summary, read by openmind
`stack_health`, which returns `system` whole).

## Run

```text
GALION_REAL=1 cargo test --release -p openmind-nerve \
  --test real_mcp_connectors -- --nocapture --test-threads=1
```

Ports 19480-19481. Fake `systemctl`/`journalctl`/`/proc` written by the
test into a temp dir; the nerve is killed on drop.

## Result 2026-10-30 13:42 CET: pass

```text
== 13:42:16 D off: /api/v1/connectors 404, no state keys
== 13:42:21 A table: servers 3, healthy 1, waiting 1, failing 1, tools 12, alerting 0
   alpha healthy tools=12
   beta  waiting (token file not present yet)
   gamma failing error="handshake failed Authorization: [redacted] from [addr]"
== 13:42:21 A no mcp alert
== 13:42:26 B alert: warn mcp-connector:alpha "MCP connector 'alpha' is failing after it was healthy"
== 13:42:47 B deduction: 5 points for the alpha incident; beta costs nothing
== 13:42:47 B one alert after cycle 7 (deduped)
```

No token byte and no address appeared in `/api/v1/connectors`, `/state`,
`/health`, `/self-eval` or `/api/v1/alerts`.

Also green on this branch: `cargo fmt --check`, `clippy --all-targets -D
warnings`, all non-gated tests (280 lib incl. 7 new), and gated
`real_state_freshness`, `real_consumer_compat`, `real_no_openrouter`.
`real_score_v2`, `real_cortex_sensor`, `real_own_services`, `real_wiring`
fail identically on the base commit 18a5a72b (box drift in their goldens),
so they are not caused by this change.
