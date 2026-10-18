# Hermes ↔ OpenRouter: root cause + proposed changes (NOT applied)

Written 2026-10-18 (Europe/Zagreb). Read-only investigation of root@__VPS_HOST__. No secret values are in this file.

## Where Hermes is
- Service: `hermes.service` → `/usr/local/bin/hermes gateway run --accept-hooks` (wrapper: HERMES_HOME=/opt/galion/hermes, sources `/root/galion-secrets/hermes.env`), EnvironmentFile=/etc/memory-gateway.env
- Code: `/opt/galion/tooling/hermes-agent` (NousResearch hermes-agent 0.21.4, commit fde4997f), venv `/opt/galion/venv`
- Home/config: `/opt/galion/hermes/config.yaml`, `.env`, `auth.json` (credential pool), `SOUL.md`, `state.db`
- API: 127.0.0.1:8642 (OpenAI-compatible `/v1/chat/completions`, Hermes-native `/v1/runs`)
- Callers: console-ide (`/opt/galion/console-ide/src/lib/hermes.ts`), Galion One (`/opt/galion/galion-light/one/src/main.rs`)

## Bug 1 (the OpenRouter bug): OpenRouter key invisible to Hermes → "No LLM provider configured"
Evidence (`/opt/galion/hermes/logs/agent.log.2`, 2026-10-17 21:27 CEST):
```
POST /v1/runs 202
agent.credential_pool: credential pool: no available entries (all exhausted or empty)
agent.auxiliary_client: resolve_provider_client: openrouter requested but OpenRouter credential pool has no usable entries
gateway.platforms.api_server: run run_0836ba80… failed
RuntimeError: No LLM provider configured. Run `hermes model` …
```
Why:
- `config.yaml` has `gateway.multiplex_profiles: true` → `gateway/run.py` calls `set_multiplex_active(True)`.
- In multiplex mode, `agent/secret_scope.get_secret()` reads a profile secret only from that profile's `<HERMES_HOME>/.env`; a miss returns empty and **does not fall back to os.environ**.
- `OPENROUTER_API_KEY` is only in `/root/galion-secrets/hermes.env` (loaded into the process env by the wrapper; the running pid has it, 73 chars, fingerprint matches auth.json). `/opt/galion/hermes/.env` contains only `API_SERVER_KEY` and `RIVER_API_KEY`.
- So the OpenRouter pool entry (`source: env:OPENROUTER_API_KEY`, `request_count: 0`) hydrates to an empty key and is skipped → every `provider: "openrouter"` run dies at agent creation. River keeps working because `RIVER_API_KEY` is in `.env` (and inline in config).

Fix (pick one; A is the clean one):
- A. Add `OPENROUTER_API_KEY=<the existing key>` to `/opt/galion/hermes/.env` (keep mode 600). The user/Slavik should paste it there themselves; do not route it through chat.
- B. Or set `gateway.multiplex_profiles: false` (single-profile box) so os.environ is honored.

## Bug 2: Galion One sends model "galion" with no provider → 404 storm
Evidence: 3,601 × `Error code: 404` and 1,169 × `HTTP 404: Model 'galion' not found` in Hermes logs, e.g. 2026-10-18 15:18:55 CEST `provider=custom base_url=https://api.river.ai/v1 model=galion`.
Why: `/opt/galion/galion-light/one/model` = `galion` (the laptop Ollama model). `main.rs` replaces `hermes-agent` with that id and sends no `provider`; `platforms.api_server.direct_model_requests: true` makes Hermes forward the bare id to the default provider (River) → 404. The laptop tunnel (127.0.0.1:11434) is down anyway (link.json `laptop_port:false`). `keep_warm()` repeats this every 240 s (session `one-warm`), plus each voice turn.

## Bug 3: auxiliary fallback would silently spend on Opus
`agent.log.2` 2026-10-17 21:27 CEST: `Auxiliary client: PAID lane engaged … OpenRouter fallback model '~anthropic/claude-opus-latest' is not a :free SKU`. When River returns 429 (`model_concurrency`, 63 occurrences), title generation/compression fall back to Opus on OpenRouter. It only failed because of Bug 1; after fixing Bug 1 it would start billing.

## Other related findings
- memory-gateway uses the **same OpenRouter key** (TDAI_LLM_API_KEY hash == OPENROUTER_API_KEY hash) and runs the *fallback* model `nvidia/nemotron-3.5-lightning:free` as primary (env comment says primary should be `google/gemma-4-31b-it:free`).
- console-ide hard-codes `model_options.reasoning.enabled: false` for both chat and research runs and defaults to River.
- Concurrency today: api_server.max_concurrent_runs 10, delegation.max_concurrent_children 8, max_spawn_depth 2 (worst case ≈ 10 + 80 + 640 concurrent model calls on one key). Console caps at 2 runs/user.
- Subagent sessions: Hermes delegation already gives each child its own session (records carry `parent_session_id`); children inherit the parent's model/provider **and credential pool** because `delegation.model/provider` are empty.

## Model ids (OpenRouter public /api/v1/models, fetched 2026-10-18)
| Want | id | context | $/M in / out | reasoning |
|---|---|---|---|---|
| Boss (latest Opus) | `anthropic/claude-opus-5.5` ("Claude Opus 5.5" — **exists**) | 1,000,000 | 4 / 20 | reasoning, reasoning_effort |
| Subagents (latest Sonnet) | `anthropic/claude-sonnet-5.5` | 1,000,000 | 2 / 10 | reasoning, reasoning_effort |
| Floating aliases (avoid pinning surprises) | `~anthropic/claude-opus-latest`, `~anthropic/claude-sonnet-latest` | | same as above today | |
| Cheap side tasks | `anthropic/claude-haiku-5.5` | 1,000,000 | 0.10 / 0.50 | |
Also listed: claude-opus-5, -4.8, -4.7, -4.6; sonnet-5, -4.6; and a pricier "claude-fable-5.1" ($10/$50) — not Opus.

## Proposed changes (diffs; NOT applied)

### 1. `/opt/galion/hermes/.env`  (fix Bug 1)
```diff
 API_SERVER_KEY=…
 RIVER_API_KEY=…
+OPENROUTER_API_KEY=<same value as in /root/galion-secrets/hermes.env>
```

### 2. `/opt/galion/hermes/config.yaml`
```diff
 model:
-  provider: river
-  default: deepseek-ai/DeepSeek-V4.1-Flash
+  provider: openrouter
+  default: anthropic/claude-opus-5.5
   ollama_num_ctx: 65536
   context_length: 65536
 …
-  base_url: https://api.river.ai/v1
+  base_url: https://openrouter.ai/api/v1
 model_aliases:
+  boss:
+    model: anthropic/claude-opus-5.5
+    provider: openrouter
+  worker:
+    model: anthropic/claude-sonnet-5.5
+    provider: openrouter
   river-flash: …            # keep River as an explicit alternative
```
```diff
 agent:
-  reasoning_effort: "medium"
+  reasoning_effort: "high"        # boss/research; console can still send per-run model_options
```
```diff
 delegation:
   max_iterations: 250
-  max_concurrent_children: 8
-  max_spawn_depth: 2
+  max_concurrent_children: 3      # cap parallel Sonnet workers per parent
+  max_spawn_depth: 1              # workers cannot spawn workers (no fan-out explosion)
   subagent_auto_approve: false
-  model: ""
-  provider: ""
+  model: "anthropic/claude-sonnet-5.5"
+  provider: "openrouter"
+  reasoning_effort: "medium"
   fallback_providers: []
```
```diff
   api_server:
-    max_concurrent_runs: 10
+    max_concurrent_runs: 3        # at most 3 boss runs at once → ≤ 3 + 3×3 = 12 concurrent model calls
```
```diff
+auxiliary:
+  free_only: true                 # never fall back to paid ~anthropic/claude-opus-latest for titles/compression
+  title_generation:
+    max_concurrency: 1
+  compression:
+    max_concurrency: 2
```
```diff
 platforms:
   api_server:
     enabled: true
     direct_model_requests: true
+    extra:
+      model_routes:
+        galion:                   # what Galion One sends; stop the 404s
+          model: "anthropic/claude-haiku-5.5"   # DECISION: haiku (cheap, fast voice) vs sonnet-5.5 vs laptop
+          provider: "openrouter"
```

### 3. `/opt/galion/console-ide/src/lib/hermes.ts`
```diff
 const PROVIDER_MODELS = [
   { provider: "laptop", model: "huihui_ai/qwen3.5-abliterated:9b" },
-  { provider: "openrouter", model: "google/gemini-2.5-flash" },
+  { provider: "openrouter", model: "anthropic/claude-opus-5.5" },
+  { provider: "openrouter", model: "anthropic/claude-sonnet-5.5" },
   { provider: "river", model: "deepseek-ai/DeepSeek-V4.1-Flash" },
 …
-  return PROVIDER_MODELS.find((row) => row.model === model) ?? { provider: "river", model: "deepseek-ai/DeepSeek-V4.1-Flash" };
+  return PROVIDER_MODELS.find((row) => row.model === model) ?? { provider: "openrouter", model: "anthropic/claude-opus-5.5" };
```
In `startHermesRun` (the "Hermes Research" run):
```diff
-        model_options: { reasoning: { enabled: false, effort: "low" } },
+        model_options: { reasoning: { enabled: true, effort: "high" } },
```
In `hermesChat` (quick chat) — keep reasoning off, but move off River if wanted:
```diff
-        model: "deepseek-ai/DeepSeek-V4.1-Flash",
-        provider: "river",
+        model: "anthropic/claude-sonnet-5.5",
+        provider: "openrouter",
```
Also `/opt/galion/hermes/modes.json`: `"provider": "openrouter"`, `modes.plan/action` → `anthropic/claude-opus-5.5`. Then rebuild/restart console (it runs vite dev, so a restart picks it up).

### 4. Galion One (`/opt/galion/galion-light/one/`)
- With the `galion` route above no code change is needed. Optional: raise warm interval in `main.rs` `warm_due()` from `240.0` s, or skip warming when the route is a paid model (each warm is a full Hermes agent turn: ≈360/day).

### 5. memory-gateway (`/etc/memory-gateway.env`)
- Set `TDAI_LLM_MODEL` / `MEMORY_TENCENTDB_LLM_MODEL` back to the documented primary `google/gemma-4-31b-it:free`.
- Prefer a **separate OpenRouter key** (with its own credit limit) for memory-gateway so its traffic and limits are isolated from Hermes.

### 6. Rate-limit / spend safety
- Create an OpenRouter key credit limit for the Hermes key (OpenRouter → Keys → limit), so a loop cannot overspend.
- Keep `fallback_providers: []` for children (no silent cross-provider fan-out), or add one explicit fallback: `- provider: openrouter, model: anthropic/claude-sonnet-5` .

### Apply order (when approved, by Slavik/Mate)
1. Back up `config.yaml`, `.env`, `modes.json`, `hermes.ts` (`~/vps-backups`).
2. Edit .env + config.yaml → `systemctl restart hermes` → check `/health`, then one `/v1/runs` with provider openrouter; confirm `auth.json` openrouter `request_count` > 0 and no "credential pool" lines.
3. Edit console files → `systemctl restart console`.
4. Watch `logs/errors.log` for 404/429 for 30 min.
