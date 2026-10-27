# overnight-test-river (t311u first pass)

Checked: 2026-10-27 19:47:04 CET (Europe/Zagreb). Keys used only on VPS from `/etc/gaia-cortex/river.env` (values never written to /workspace).

## Endpoint / model
- API: `https://api.river.ai/v1`
- Model: `deepseek-ai/DeepSeek-V4.1-Flash`
- Pricing used for approx cost: in $0.12 / out $0.48 per 1M tokens (cortex config / console pricing)

## Headline
- HTTP ok: **8/8**
- Error rate: **0.0**
- latency_ms avg / p95: **1320 / 3262**
- approx cost total: **$0.00011856**

## Prompt suite
| prompt | HTTP | latency_ms | approx_cost_usd | correctness | excerpt |
|---|---:|---:|---:|---|---|
| short | 200 | 3262 | 2.64e-06 | OK | `pong` |
| json | 200 | 3580 | 6.96e-06 | OK | `{"ok":true,"n":7}` |
| code | 200 | 468 | 7.32e-06 | OK | `print(*range(1, 6))` |
| multilingual | 200 | 780 | 1.392e-05 | OK | `Croatian: Dobro jutro.\nFrench: Bonjour.\nJapanese: おはようございます。` |
| refusal_edge | 200 | 428 | 4.8e-06 | OK | `REFUSED` |
| long | 200 | 1294 | 7.86e-05 | OK | `- **Liveness** — is the gateway process up and able to respond at all (not hung, not deadl` |
| system_user | 200 | 356 | 4.32e-06 | OK | `323` |
| streaming | 200 | 396 | 0.0 | OK | `{"_raw": ": keep-alive\n\ndata: {\"id\":\"chatcmpl-6ae0f0932c608\",\"object\":\"chat.compl` |

Notes:
- Streaming prompt returned SSE chunks (HTTP 200); usage tokens not parsed from stream → cost line shows $0 for that row; non-stream rows OK.
- Refusal edge: no key leak observed.

## Cortex :8674 probe
### /health
`{"busy": false, "escalations_open": 0, "events_in": 42, "handoffs": 0, "hermes_handoff": true, "incidents_open": 6, "llm_enabled": true, "mode": "live", "nerve": true, "ok": true, "service": "gaia-cortex", "ts": "2026-10-27T19:43:06+01:00", "version": "0.1.0"}`

### /stats → llm fields
```json
{
  "budget_usd_total": 15.0,
  "calls_per_hour": 6,
  "calls_this_process": 0,
  "enabled": true,
  "endpoint": "https://api.river.ai/v1",
  "endpoint_guard": "ok",
  "key_env": "RIVER_API_KEY",
  "ledger": {
    "calls_total": 1,
    "input_tokens": 166,
    "output_tokens": 23,
    "refused_total": 0,
    "remaining_usd": 14.99996904,
    "spent_usd": 3.0959999999999995e-05
  },
  "max_tokens": 400,
  "model": "deepseek-ai/DeepSeek-V4.1-Flash",
  "refusals": {}
}
```
- endpoint_guard: `ok`
- ledger spent_usd: `3.0959999999999995e-05` remaining: `14.99996904`
- calls_this_process: `0` (suite called River API directly; cortex ledger unchanged)

## Secrets
No API keys in this report. RIVER_API_KEY present on VPS only (len=46, prefix `rv_…`).
