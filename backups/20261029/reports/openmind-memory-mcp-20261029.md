<!-- SPDX-License-Identifier: MIT -->
# OpenMind memory + MCP job: report (2026-10-29, Europe/Zagreb CET)

Status: **STOPPED before PRs, merges, sandbox deploy and live deploy.** A second agent ("omfix") is doing the same job
in parallel, and it has already changed live. Details below.

## 1. Clash found (14:03 to 14:10 CET)
- **PR #15** `swarm/openmind-mcp-rust` was opened at 13:59 CET. It is a second Rust MCP server with `memory_search`,
  `memory_conversations`, `memory_recall`, `memory_save`, `vault_*`, and it reads the key with `--key-file`.
- **PR #16** `swarm/memory-integration-20261029` was opened at 13:59 CET. It merges the four memory branches.
- Both PRs come from the galion-studio account. I did not open them.
- **That agent deployed LIVE at 14:07 to 14:09 CET.** The gate report did not exist then and it was before 15:00. CI had not run.
  - `openmind-memory.service.d/91-omfix-5c7bad5f.conf` → `openmind-memory-5c7bad5f` (restarted 14:07:25)
  - `hermes-rs.service.d/96-omfix-memory.conf` → `hermes-gateway-5c7bad5f` with `HERMES_RS_MEMORY=1` (**hermes-rs
    restarted 14:07:42**; the log says `memory on … key_set=true scope=session health 200`)
  - `galion-nerve.service.d/93-omfix-memfresh.conf` → `NERVE_MEMFRESH_BIN=…/openmind-memory-freshness-5c7bad5f`
    (nerve restarted 14:09:19)
  - Also staged: `/opt/galion/rust/bin/openmind-mcp-stack-498244ec`. MCP is still off (`HERMES_RS_MCP=0`;
    config.yaml is unchanged since 18 Oct and still lists the Python `galion-stack`).
  - Its backups are in `/var/backups/omfix-20261029-20261029130707/`. Each drop-in has its own rollback line
    (rm the drop-in, daemon-reload, restart).
- All three units were active at 14:10.
- I touched nothing live, so that I would not race it or double-deploy.

## 2. CI is not running at all
- Since 28 Oct 20:22 CET, every GitHub Actions job fails in about 2 s. Jobs have **no runner, no steps**,
  `runner_id 0`. This covers miri, self-eval, realtest, launch-gates, and scheduled main.
  PRs #15 and #16 are affected too, as are my 6 branch pushes.
- That pattern means jobs are never picked up, which is normally a billing problem or an exhausted
  Actions minutes or spending limit on the account. The annotations API is not reachable unauthenticated,
  so I could not read the exact message.
- Nothing can go green until Mate fixes Actions billing or limits. I did not bypass any required check.

## 3. What I built (my branch, as an alternative to #15)
`swarm/openmind-mcp-vault` @ `9d63bdb7`, pushed. It is stacked on the session-scope branch.
- `openmind-mcp-stack`: a real stdio MCP server, with no fixture data.
  - Memory tools: `memory_search`, `memory_recall` (fails closed if the server doesn't echo the session scope),
    `memory_save`, `memory_recent`.
  - Read-only vault tools: `vault_list`, `vault_read`, `vault_search`.
  - Vault path is set by `--vault` / `OPENMIND_MCP_VAULT`.
  - Refused paths: `..`, absolute paths, symlinks at any level, dot directories (`.git`, `.obsidian`), non-`.md` files, NUL.
    The inode is checked again after open.
  - Write-like tool names get "refused: read-only".
  - Memory URL must be loopback only. The key comes from env and is never printed.
- `openmind-memory`: new `POST /recent` route (newest rows first, behind auth).
- `hermes-rs`: new `mcp_servers.<name>.pass_env` list (env names copied into the cleared child env).
  The gateway now logs MCP tool names.
- Tests on the VPS:
  - openmind-mcp-stack: 4 unit, 1 main, and 7 stdio integration tests. The integration tests run the real binary against a
    real openmind-memory on 127.0.0.1:0 and a temp vault with symlinks out, `.git`, and `.obsidian`. The vault is
    snapshotted byte for byte before and after and is unchanged.
  - openmind-memory golden: 8 passed (adds 2 `/recent` tests).
  - hermes-rs config/mcp tests pass.
- Integration tree: main + session-scope + freshness-sensor + write-guard + mcp-vault.
  It is local only (branch `swarm/openmind-memory-integration-20261029`, not pushed).
  - fmt: ok.
  - clippy --workspace --all-targets -D warnings: ok.
  - repo-check: PASS (0 errors).
  - cargo test --workspace: 2340 passed, 11 failed, 28 ignored. The failures are guardian-tick cli (1),
    hermes-tools sandboxed (8, needs the userns setup that CI does), nerve reflex (1) and survival http_probe_mock (1).
    None of these crates is touched. The guardian-tick, nerve reflex and survival failures **fail the same way on plain
    main 7ba85b60** on this VPS.

## 4. Branches pushed (no force push; no PRs opened)
| branch | head |
|---|---|
| swarm/memory-db-stale-diag-onmain | 14f07ae7 |
| swarm/hermes-rs-memory-client-onmain | 704048a4 |
| swarm/hermes-memory-session-scope-onmain | 9ed0fa85 |
| swarm/memory-freshness-sensor-onmain | b3fb1078 |
| swarm/openmind-write-guard-onmain | a33abb3c (conflicts resolved: 74 crates / 62 system) |
| swarm/openmind-mcp-vault | 9d63bdb7 |

## 5. Other facts for the deploy decision
- The previous live hermes-gateway `b7fbbc78` was not on main, but it is only 2 survival-docs/unit commits past it.
- The live nerve `f66974c` is 103 commits past main. It already contains the memory-freshness sensor, so the alarm
  needs only the check binary plus a drop-in, and no nerve swap. A main-built nerve would be a regression.
- Spend in rs-spend.json went from 3.449 (71 sessions, about 13:52) to 3.5136 (73 sessions, about 14:12): +$0.0646, mostly
  claude-opus. **This job made no model calls ($0.00).** The new spend is from someone else.

## 6. Rollback
I deployed nothing, so I have nothing to roll back. For the other agent's changes, see the comment at the top of each
omfix drop-in.

## 7. Leftovers (mine, on the VPS, no live paths)
`/root/swarm-om-20261029/` (clone, worktrees, build targets) and `/var/tmp/om-sbx-20261029/` (release build only,
no data copied). I deleted nothing.
