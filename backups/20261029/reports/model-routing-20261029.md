# Model routing check — production VPS (t360u, 2026-10-29)

Checked: 2026-10-29, about 08:37–08:45 CET (UTC+1), over the existing `slavik-vps` SSH alias. Every check was read-only.
Result: **nothing changed**. I made no file edits, no backups, no restarts and no paid calls.
No secret values were printed. This report names env vars only and contains no IP addresses.

## Summary
- **Cortex and nerve already follow the rule.** Neither uses OpenRouter.
- **There is no separate "guard" LLM route in hermes-rs.** The deployed build b7fbbc78 and branch `swarm/hermes-ui-rebuild` have no guard route or guard function that calls an LLM. The only "guard" is `command-guard`, a regex plugin with no LLM, plus the console's `guard.json` toggle. Because hermes-rs has one global default provider, a River-pinned guard would need a code change (see below).
- **Everything else that calls an LLM already defaults to OpenRouter.** One exception, hermes cron, is switched off. Since nothing needed a change, I ran no smoke calls.
- **One conflict with the rule needs an owner decision:** the cortex → Hermes "boss" handoff runs on OpenRouter Opus (see item 1 under "Needs owner decision").

## Inventory and results

| Service / unit | Before (provider · base URL · model) | Key env var NAMES | Where configured | After | Changed |
|---|---|---|---|---|---|
| **gaia-cortex** (brain), LLM advisor | River AI · https://api.river.ai/v1 · deepseek-ai/DeepSeek-V4.1-Flash ($15 cap, 6 calls/h). The ledger shows 1 call, about $0.00003 in total. | `RIVER_API_KEY` (config `key_env`). The live process env has only `CORTEX_LLM_ENABLED` and `RIVER_API_KEY`. | `/etc/gaia-cortex/config.json` (`llm.*`). `EnvironmentFile=/etc/gaia-cortex/river.env` (holds only `RIVER_API_KEY`). Drop-in `50-safety.conf` unsets every OPENROUTER_/OPENAI_/ANTHROPIC_ var. Binary `gaia-cortex-4b036bc-llm` (via `92-escpersist-4b036bc.conf`). | Same (River AI) | no |
| **galion-nerve** (runs `openmind-nerve-f66974c`) | No LLM client. Escalation is `delegate` (publish only). A built-in egress guard refuses openrouter URLs and refuses escalation via Hermes. | None. The live env has no KEY/OPENROUTER/RIVER vars. | `75-no-openrouter.conf` (UnsetEnvironment + loopback IP intent), `80-event-mode.conf` (`NERVE_EVT_ESCALATE=delegate`) | Same (no LLM; never OpenRouter) | no |
| **hermes-rs guard** | There is no guard LLM route in hermes-gateway b7fbbc78. `command-guard` (`/opt/galion/hermes/plugins/command-guard`) is regex only. `guard.json` / `control.json` `guard:true` are console toggles. | — | — | Same | no (needs a code change if you want a River-pinned guard; see below) |
| **hermes-rs research / default** (:8642) | OpenRouter · https://openrouter.ai/api/v1 · anthropic/claude-opus-5.5. Per-request model, provider and aliases already work (aliases `river-flash`, `river-glm`; providers `laptop`, `river`). | `OPENROUTER_API_KEY`, `RIVER_API_KEY`, `API_SERVER_KEY` | `/opt/galion/hermes/config.yaml`: `model.provider`/`model.default`/`model.base_url`, `model_aliases`, `providers`. `EnvironmentFile=/opt/galion/hermes/.env`. Caps in `70-research-budget.conf`. | Same (already OpenRouter) | no |
| hermes-rs delegation (subagents) | OpenRouter · anthropic/claude-sonnet-5.5 | `OPENROUTER_API_KEY` | config.yaml `delegation.model`/`provider` | Same | no |
| hermes-rs auxiliary tasks | OpenRouter · free models (google/gemma-4-31b-it:free, …), `free_only: true` | `OPENROUTER_API_KEY` | config.yaml `auxiliary.*` | Same | no |
| hermes-rs api_server route `galion` | OpenRouter · anthropic/claude-haiku-5.5 | `OPENROUTER_API_KEY` | config.yaml `platforms.api_server.extra.model_routes.galion` | Same | no |
| hermes-rs cron | River · deepseek-ai/DeepSeek-V4.1-Flash. **Inert:** cron is OFF (`HERMES_RS_CRON` is unset in the live process). | `RIVER_API_KEY` | config.yaml `cron.model` / `cron.model_provider` (read once at startup) | Same | no. Left alone because it is inert, and changing it would need a hermes-rs restart plus an owner choice of OpenRouter model. |
| **Galion One** (`galion-one`; keep-warm) | Sends model `galion` to Hermes, which resolves via route `galion` to OpenRouter claude-haiku-5.5 (the model name is hard-coded in the binary). Models-list refresh calls OpenRouter `/models`. Keep-warm is OFF (`ONE_NO_WARM=1`). | `OPENROUTER_KEY_FILE` (points to the `/workspace/hermes/.env` line `OPENROUTER_API_KEY`). Hermes key file via `ONE_HERMES_ENV`. | Unit `galion-one.service`, drop-in `60-no-warm.conf`, plus the Hermes route above | Same (already OpenRouter) | no |
| gaia-hermes-handoff (.path + oneshot; cortex → Hermes "boss") | Hermes alias `boss`, which resolves to **OpenRouter · anthropic/claude-opus-5.5**. The ledger has 29 runs, all on Opus. Last run 2026-10-28 06:28 CET. | Hermes `API_SERVER_KEY` (named by `HANDOFF_KEY_ENV`). The unit unsets `OPENROUTER_API_KEY` and `RIVER_API_KEY`. | `gaia-hermes-handoff.service`: `HANDOFF_MODEL=boss`, `HANDOFF_BUDGET_USD=20` | Same | **no: needs an owner decision** (see below) |
| galion-desk | Only a 1-token liveness probe of the laptop's local Ollama (`galion:latest`). No remote provider and no model setting. | none | Built into the binary | Same | no |
| desk-hermes, link-watch-rs, galion-health, guardian-tick, openmind-memory, openmind-queue, tuner, tanga-proto, cloudflared | No LLM calls (stats readers, port probes, static servers) | — | — | — | no |
| console, console-rust-primary | Not inspected or touched, as instructed. The console's Hermes picker state is in `/opt/galion/hermes/modes.json` (`provider: river`, plan/action GLM-5.3-Flash) and `control.json` (`provider: laptop`). That belongs to the UI branch `swarm/hermes-ui-rebuild`. | — | — | — | no |
| Swarm dispatcher | Not running on the VPS; there is no unit, timer or cron for it (root has no crontab). | — | — | — | — |

## Keys present (names only)
- `/opt/galion/hermes/.env` (also `/root/.hermes/.env` and `/workspace/hermes/.env`): `API_SERVER_KEY`, `RIVER_API_KEY`, `OPENROUTER_API_KEY`, `TDAI_GATEWAY_API_KEY`, `MEMORY_TENCENTDB_GATEWAY_HOST`, `MEMORY_TENCENTDB_GATEWAY_PORT`.
- `/etc/gaia-cortex/river.env`: `RIVER_API_KEY` only.
- **No key is missing.** The OpenRouter and River AI keys are both configured.

## Rollback
No changes were made, so there is nothing to roll back. No `*.bak-t360u` files were created.

## Spend
- Hermes ledger (`/var/lib/hermes-rs/rs-spend.json`): about $3.38 in total across sessions. Last day recorded was 2026-10-28 at $0.42. That is well under the $16 warning and $20 cap.
- This task spent $0: no paid calls.

## Needs owner decision or a code change
1. **The cortex → Hermes boss handoff uses OpenRouter Opus.** It goes through `gaia-hermes-handoff`, which runs the cortex binary `gaia-cortex-c5cae88 hermes-dispatch`. The cortex itself is on River, but its escalations are answered by Opus on OpenRouter. That conflicts with "cortex must never use OpenRouter". The nerve's code already refuses escalation via Hermes for this reason. I did not change it because it is cortex-adjacent and ambiguous.
   - **Proposed minimal fix (not applied):** add the drop-in `/etc/systemd/system/gaia-hermes-handoff.service.d/50-river.conf` containing `[Service]` and `Environment=HANDOFF_MODEL=river-flash`, then run `systemctl daemon-reload`. No restart is needed (it is a oneshot fired by the .path unit). Hermes would then send handoffs to River DeepSeek-V4.1-Flash through the existing `river-flash` alias (`key_env RIVER_API_KEY`).
   - **Rollback:** delete that drop-in, then `daemon-reload`.
   - **Caveat:** the handoff's budget check reads the Hermes spend file, and River pricing goes through the same ledger. Check proposal quality with Flash before switching.
   - Alternatively, disable the handoff with `systemctl disable --now gaia-hermes-handoff.path`.
2. **A River-pinned guard in hermes-rs needs a code change.** hermes-rs has one global `model.provider`/`model.default`, and `resolve_target` in `crates/hermes-rs/src/provider.rs` falls back to it. Recommended change: add a guard route (for example an `api_server.extra.model_routes.guard` entry or a dedicated `guard:` config section) that is pinned to the `river` provider and refuses to fall back to openrouter (fail closed, like the cortex endpoint guard). Callers would then send `model: "guard"`. A config-only alternative works today: add `model_routes.guard: {model: deepseek-ai/DeepSeek-V4.1-Flash, provider: custom, base_url: https://api.river.ai/v1, key_env: RIVER_API_KEY}` to config.yaml. However, that needs a hermes-rs restart and does not stop a caller from omitting the model, which would fall to OpenRouter. Not applied.
3. **Hermes cron default is River, but cron is OFF.** If cron is enabled later, set config.yaml `cron.model_provider: openrouter` and `cron.model: <owner-chosen OpenRouter model>` before turning `HERMES_RS_CRON` on.
4. **The console's Hermes picker default is River** (`modes.json` `provider: river`, saved default "River DeepSeek V4.1 Flash"). It belongs to the `swarm/hermes-ui-rebuild` worker and console code, so I left it alone. Flag it to that worker if research in the UI should default to OpenRouter.
