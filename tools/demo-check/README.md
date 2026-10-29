<!-- SPDX-License-Identifier: MIT -->
# Demo check (Obsidian OpenMind stack)

Run from the box (needs `ssh slavik-vps`; `--mobile` also needs `google-chrome` and Node 22+).

| Script | What it does |
|---|---|
| `demo-check.sh [--quick] [--exact] [--mobile] [--port 8080]` | PASS/FAIL for every service, both console ports, the public pairing gate, dictation (galion-stt), model lists (River + OpenRouter), the phone's default model, model routing (cortex/guard on River, handoff model), MCP `tools/list` (8 tools, no write tool), error handling (400/409/404 paths), then the 4 demo chats through the real console API on River with a fresh codeword per run, streaming, and a vault-hash check around the write refusal. Retries a chat once (River 429 under load). Exit code = failures. |
| `mobile-check.mjs <url> <png> [--send] [--fail]` | Headless Chrome at 390x844 with an Android UA: no horizontal scroll, composer, model sheet (River and OpenRouter tabs, search, stars), mic button, JS errors, HTTP errors; `--send` does one River chat through the UI; `--fail` checks that a provider that is down cannot hang the page. |
| `demo-reset.sh [--dry-run]` | Non-destructive: backs up the console picker prefs, sets the phone default to River DeepSeek-V4.1-Flash (starred), checks no run is in flight, and reports which old codewords a memory search returns. Deletes nothing. |

The model is always named explicitly (`deepseek-ai/DeepSeek-V4.1-Flash`, the model behind the gateway alias `river-flash`): the gateway default is the paid OpenRouter Opus model.

## Presenter steps (phone, console.galion.app/hermes)
1. Run `demo-reset.sh` and `demo-check.sh --mobile` about 15 minutes before.
2. Check that the Model pill says **River AI · deepseek-ai/DeepSeek-V4.1-Flash**. Tap **New thread**.
3. "Remember this for later: my demo codeword is <fresh tag>. Save it to memory." Use a fresh tag, not blue-harbor-42: old test codewords stay in memory.
4. New thread: "What is my demo codeword? Check your memory."
5. "List the notes in the vault and read 02-goals.md. Give me its title and the first goal." (Expected: "Goals"; the first goal is "Light. Not the full Galion backend.")
6. "Write a new note into the vault." Hermes refuses, because the vault is read-only.
