# Hermes model picker restored, 2026-10-29

Owner orders t364u (fix the model choice) and t365u (bring back the old picker). Branch `swarm/hermes-model-picker`, commit **dff00cbe**, built on a69e0e35. It was pushed with push-branch.sh: no force-push and no merge to main. Deployed to :18080 (console-rust-primary) at about 11:01 CET, then to :8080 (console) at about 11:03 CET.

## Root cause
1. The a69e0e35 page built its model list from hermes-rs `/v1/models`. That endpoint only returns the gateway aliases (`hermes-agent` and `galion` → anthropic/claude-haiku-5.5), so there was never an OpenRouter or River AI model list to choose from.
2. The model `<select>` was hidden unless OpenRouter was picked (`syncModel`). The server's `ask()` also refused any model for other providers ("a model alias can only be picked for OpenRouter"). Choosing River therefore always meant River's default model, and nothing on the page said which one.
3. The old picker (search, stars, check mark) was never ported to the Rust page, and the status bar showed no model.

hermes-rs did not need a change. Its `resolve_target` already accepts a raw OpenRouter or River model id together with `provider`.

## The old picker: what it was and where I found it
- **Component:** `stack/research-agent/console-ide/src/components/hermes-panel.tsx`. It is in the repo history at f0c69e7c, and a read-only copy is on the VPS at `/opt/galion/console-ide/src/components/hermes-panel.tsx` (Oct 18).
  - It had Laptop / OpenRouter / River buttons. A tap picked the provider's default model. A **long-press (420 ms)** opened a "model box".
  - The model box had a "Search models" field, category chips and rows with a ☆/★ star button. The current model had a ✓.
  - Rows were ordered: current model first, then starred models, then alphabetical.
- **Model lists:** `/opt/galion/console-ide/src/routes/api/hermes/models.ts`.
  - OpenRouter: the public openrouter.ai `/api/v1/models` endpoint.
  - River: api.river.ai `/v1/models` with RIVER_API_KEY.
  - Both were cached for 10 minutes.
- **Favourites:** `/opt/galion/console-ide/src/routes/api/hermes/modes.ts` stored them server-side in `/workspace/hermes/modes.json` (`stars:{laptop,openrouter,river}`, up to 24, plus the chosen provider). The new build does not touch that file.

## Changes (dff00cbe)
- **`crates/console-static/src/hermes_models.rs` (new):**
  - OpenRouter list: no key needed. It keeps only models that output text and support `tools`, because Hermes always sends tools: 373 models today.
  - River list: uses `RIVER_API_KEY`, read by name from the existing `--hermes-key-file`. The key is passed to curl on stdin, never in argv or logs: 7 models.
  - Both lists are cached for 10 minutes. If a refresh fails, the stale copy is served.
  - Picker choices (provider, model per provider, stars) are saved to a prefs file with atomic writes.
- **`hermes.rs` (console-static), new and changed API:**
  - `GET /api/hermes/models?provider=openrouter|river|laptop|runpod`. Without `provider` it still returns the old alias list.
  - `GET` and `POST /api/hermes/prefs`.
  - `ask()` now accepts a model for OpenRouter and River, but only if it is on that provider's list (OpenRouter also accepts the gateway aliases). Anything else gets a 400. The reply now includes `model`.
- **`config.rs`:** new flags `--hermes-prefs FILE`, plus `--hermes-openrouter-models URL` and `--hermes-river-models URL` (used by the tests). All three need `--hermes-api`.
- **`entry.rs`:** the start-up log now says whether the River list key was found and where the prefs file is. It never prints the key.
- **console-ui (`pages/hermes.rs`, `hermes.js`, `hermes.css`):**
  - A "Model" button under the provider choice. It opens a model sheet that is full screen on phones and a centred card on wide screens.
  - The sheet has OpenRouter and River AI tabs, a "Search models" box, a ★/☆ toggle on each row and a ✓ on the selected model. A "Favourites" section sits at the top, then "All models".
  - Each row shows the name, id, context size and $/M tokens.
  - Tap targets are at least 44 px, nothing depends on hover, and long-press was replaced by a plain tap.
  - The sheet closes with Escape, the Close button, a tap on the backdrop, or Android back (history entry). There is no animation (§11).
  - Laptop and RunPod stay in the provider grid with their status.
  - A "Model" pill in the status bar shows the current choice. After each run the answer is tagged "Answered by <model>", taken from the session the gateway actually ran.
  - Choices are saved to `/api/hermes/prefs`, with localStorage as a fallback.
  - hermes.css is now 10.9 KB; I raised its budget comment from 10 to 12 KiB.
- **Tests:**
  - `tests/hermes_api.rs` now uses a mock model-list server and adds 3 tests: model lists and key handling, asks forwarding the chosen model, and prefs.
  - The page asserts in `tests/ui_port2.rs` and the console-ui page test were updated.
- **Gates:**
  - `cargo fmt --all --check`: clean.
  - `cargo clippy -p console-static -p console-ui --all-targets -- -D warnings`: clean.
  - `cargo test -p console-static -p console-ui`: all pass.

## Deploy
- **Binary:** `/opt/galion/rust/bin/galion-console-serve-dff00cbe` (musl, 1233792 B, sha256 67dd392c…bc385, `.sha256` file next to it).
- **Backups first:** both drop-in directories were copied to `/var/backups/v1029/hermes-model-picker-20261029-1001/` (the VPS clock is UTC; 10:01 UTC is 11:01 CET).
- **Prefs directory:** created `/var/lib/galion-console-hermes` (root, mode 700). Both ports share `prefs.json` there.
- **New drop-in** `64-hermes-model-picker-dff00cbe.conf` in `console-rust-primary.service.d` and `console.service.d`:
  - `ReadWritePaths=/var/lib/galion-console-hermes` (needed because the units run with ProtectSystem=strict).
  - The same argv as 63, with the new binary plus `--hermes-prefs /var/lib/galion-console-hermes/prefs.json`. `--stt` is kept.
  - A diff against 63 shows only those changes.
- **Restarts:** daemon-reload, then restart console-rust-primary, verify, then restart console.
- **Left alone:** the 63 drop-in was not changed. Neither were hermes-rs, cortex, nerve or guard routing, galion-stt, the tuner, Cloudflare or DNS.

## Verification (loopback Host on the VPS; headless Chrome at 390×844 with an Android Chrome UA through an SSH tunnel)
- **Both ports:**
  - `/hermes` returns 200 and `/api/stt/health` returns 200.
  - Providers: laptop ready (the tunnel was up during the test), runpod not set up, openrouter ready, river ready.
  - The OpenRouter list has 373 models and is not stale. The River list has 7.
  - None of the three key values appear in `/hermes`, `/api/hermes/models?provider=*` or `/api/hermes/prefs`.
- **:18080, River, `zai-org/GLM-5.3-Flash`:** the run completed with "One short sentence: 4." The run and session model was zai-org/GLM-5.3-Flash and the provider river.
- **:18080, OpenRouter, `openai/gpt-4o-mini`:** the run completed with "2+2 equals 4." The session model was openai/gpt-4o-mini; cost $0.000274.
- **:8080, River, `deepseek-ai/DeepSeek-V4.1-Flash`:** the run completed with "Four." The session model was deepseek-ai/DeepSeek-V4.1-Flash and the provider river.
- **:8080, OpenRouter, `openai/gpt-4.1-nano`:** the run completed with "2+2 equals 4." The session model was openai/gpt-4.1-nano; cost $0.000183.
- **Headless UI on both live ports:**
  - The sheet opens and lists 373 OpenRouter models. Searching "4o mini" returns the three gpt-4o-mini variants.
  - Starring a model moves it under "Favourites" at the top, and tapping the star again removes it.
  - Picking a model puts a ✓ on it and updates both the Model button and the status pill.
  - The River tab lists 7 models.
  - After a reload the choice is still there, and a fresh browser profile shows the same choice (saved on the server).
  - There is no horizontal scroll at 390 px and no JS errors or HTTP ≥ 400 responses.
- **Local run (release binary against a mock gateway and the real public OpenRouter list):**
  - Tap targets measured at least 44×44 px.
  - Escape and the back button both close the sheet.
  - The ask body carried the chosen provider and model, and the answers were tagged "Answered by <model>".
- **Screenshots:** `reports/hermes-model-picker-20261029/`.
- **Afterwards:** I reset the saved choice to the default: OpenRouter, Gateway default (the paid research model), no stars.

## Test spend
- OpenRouter: about **$0.00046**.
- River runs show $0 in the session cost.
- Guard total went from $0.066416 to $0.066872. The Opus default was never used.

## Rollback (either unit)
`rm /etc/systemd/system/<unit>.service.d/64-hermes-model-picker-dff00cbe.conf; systemctl daemon-reload; systemctl restart <unit>`. The unit then goes back to galion-console-serve-a69e0e35 through the 63 drop-in. The full pre-deploy drop-in directories are in `/var/backups/v1029/hermes-model-picker-20261029-1001/`. `/var/lib/galion-console-hermes` can stay; the old binary does not read it.

## Open items
- The OpenRouter list shows any text-output model that supports tools. That includes audio-input models such as gpt-audio-mini, which may answer research prompts poorly. Category chips like the old panel's were not ported, only search.
- River's list returns bare ids, so River rows show the id instead of a display name.
- The laptop (Ollama) and RunPod providers have no model list. Laptop uses the gateway's laptop default.
- Picker choices are per console (shared by both ports), not per device. That matches the old modes.json behaviour.
- The branch is not merged to main.
