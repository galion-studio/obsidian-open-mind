<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes agent tools (terminal, files, browser, vision)

- Written: 2026-10-29 (Europe/Zagreb, CET +01:00). Owner ask t413u, steering t414u/t416u.
- Code: `crates/hermes-tools/src/agent/` (mod, files, shell, browser, html, vision).
- Wired in `crates/hermes-gateway/src/main.rs` after the coding tools. Off unless `HERMES_RS_TOOLS_WORKSPACE` is set.
- Tests: `crates/hermes-tools/tests/agent_tools.rs` (16 tests; the real-browser test is opt-in with `HERMES_RS_TEST_BROWSER=1`).

## What the previous (Python) stack had

hermes-agent 0.21.4, toolset `hermes-cli`:

| Prior tool | Prior limits | Rust tool now |
| --- | --- | --- |
| `terminal` (local backend) | cwd /workspace, timeout 1800 s, approvals off, no sandbox | `terminal`: deny list + openmind-sandbox, no network, timeout 60 s (max 300, hard max 1800), output cap 64 KiB |
| `file` toolset: read / write / search / patch | whole filesystem as the service user | `read_file`, `write_file`, `edit_file`, `list_files`, `search_files`: workspace-confined writes, read roots opt-in |
| `browser_navigate`, `browser_snapshot` (local Chromium) | snapshot cap 15000 chars; clicks/forms | `browser`: headless Chromium shell, one page per call, text (15000 chars + 40 links) or screenshot; no clicks/forms |
| `vision_analyze` (`auxiliary.vision`) | nvidia/nemotron-3.5-super-vl:free, fallback google/gemma-4-31b-it:free | `vision_analyze`: x-ai/grok-4.7 first (the Hermes default, accepts images), then nvidia/nemotron-3.5-super-vl:free; the answer names the model |
| `code_execution.max_tool_calls` | 200 | per-session call cap 200 |

## Safety, in the order a call meets it

1. Deny list (`hermes_rs::denylist::check_tool_call`, rules S1-S9 incl. S6-secret and S9) through the registry wrapper. The workspace is the only write root. No off switch.
2. Spend caps (vision) and the per-session call cap.
3. Path confinement (files.rs): writes only inside the workspace; no `..`; no symlink on the way; new files with `O_CREAT|O_EXCL`; names never writable: SOUL.md, config.yaml, hermes-self-modify.json and the gate/secret paths; reads only in the workspace and `HERMES_RS_TOOLS_READ_ROOTS`; secret values redacted.
4. Processes only through `openmind_sandbox::run`:
   - cleared env;
   - read-only root, only the workspace writable;
   - secret dirs, /etc/galion and the gate covered by an empty tmpfs;
   - private PID/mount namespaces, landlock;
   - rlimits, wall timeout, output cap;
   - optional cgroup (`HERMES_RS_TOOLS_CGROUP`).
   - `terminal`: empty network namespace, fails closed.
   - `browser`: network kept; a PAC script sends loopback, private, link-local, CGNAT and IPv6-literal hosts to a dead proxy; a fresh profile per call, deleted after.

## Env knobs

| Var | Default |
| --- | --- |
| `HERMES_RS_TOOLS_WORKSPACE` | unset = tools off; absolute dir, created 0750 |
| `HERMES_RS_TOOLS_READ_ROOTS` | none; `:`-separated absolute dirs, not / or top-level |
| `HERMES_RS_TOOLS_SHELL_TIMEOUT_S` / `_MAX_S` | 60 / 300 (clamped to 1800) |
| `HERMES_RS_TOOLS_OUTPUT_CAP` | 65536 B |
| `HERMES_RS_TOOLS_MAX_READ_BYTES` / `_MAX_WRITE_BYTES` | 256 KiB / 1 MiB |
| `HERMES_RS_TOOLS_WORKSPACE_MB` | 1024 |
| `HERMES_RS_TOOLS_MAX_CALLS` | 200 |
| `HERMES_RS_TOOLS_CGROUP` | unset; a dir under /sys/fs/cgroup |
| `HERMES_RS_TOOLS_BROWSER=0`, `HERMES_RS_BROWSER_BIN` | auto: newest `~/.cache/ms-playwright/chromium_headless_shell-*` |
| `HERMES_RS_BROWSER_TIMEOUT_S`, `HERMES_RS_BROWSER_TEXT_CAP` | 45 s, 15000 |
| `HERMES_RS_TOOLS_VISION=0`, `HERMES_RS_VISION_MODEL`, `HERMES_RS_VISION_FALLBACK` | Hermes default model, then nvidia/nemotron-3.5-super-vl:free |
| `HERMES_RS_VISION_MAX_BYTES` | 5 MiB |

## Known limits

- `terminal` has no network, and the deny list refuses `$( )` and `cd` out of the workspace.
- `browser` reads pages only: no click, fill or login. The PAC check is by hostname/IP literal; a public name that resolves to a private address after the check (DNS rebinding) is a residual risk.
- The Chromium headless shell aborts under RLIMIT_AS, so the browser has no address-space limit. Set `HERMES_RS_TOOLS_CGROUP` to cap its memory (1.5 GiB).
- Skills that call Python scripts still do not run (Rust-only stack). Use the staged `agent-tools` skill.

## Staged skills

`stack/research-agent/hermes/skills-staged-20261029/` (see its README): the 3 empty addy skills filled, hearth / grok-session / local-primary updated, and a new `tools/agent-tools` skill.

## run_as (2026-10-30, CET +01:00)

- `HERMES_RS_TOOLS_RUN_AS=uid:gid` sets the user for the terminal and browser children. When the gateway runs as root, the default is `65534:65534`. Use `gateway` to keep the gateway's own user. Root ids are refused, and a non-root gateway that names other ids is refused.
- The child drops its ids (setgroups, setresgid, setresuid, then a check) before any namespace is made. It then sets itself dumpable again so the uid_map write works. If the drop fails, the run fails closed.
- The browser gets the host /proc only when it was dropped to a different non-root user. Chromium needs /proc/self, and on LXC with lxcfs no private /proc can be mounted. In every other case it gets the masked /proc.
- Files written by the file tools, and the browser's directories, are lchowned to the run_as user.
- Residual risk: with the host /proc, the browser can see other processes' cmdlines (not their environ or memory).
