# overnight-test-openrouter (t311u first pass)

Checked: 2026-10-27 19:47:04 CET (Europe/Zagreb). OPENROUTER_* discovered from `/opt/galion/hermes/.env` on VPS only (names: OPENROUTER_API_KEY present). Hermes spend caps unchanged (session $5 / daily $7 / 2000 calls). Suite used **direct OpenRouter API** for comparable latency/correctness; Hermes gateway chat covered in overnight-test-hermes.md.

## Endpoint / model
- API: `https://openrouter.ai/api/v1`
- Model used: `deepseek/deepseek-chat`
- models list probe status: `200`

## Headline
- HTTP ok: **8/8**
- Error rate: **0.0**
- latency_ms avg / p95: **2539 / 2372**
- approx cost total: **$0.00026428** (rough token×rate; not billing invoice)

## Prompt suite
| prompt | HTTP | latency_ms | approx_cost_usd | correctness | excerpt |
|---|---:|---:|---:|---|---|
| short | 200 | 973 | 5.55e-06 | OK | `pong` |
| json | 200 | 1550 | 1.79e-05 | OK | ````json\n{"ok":true,"n":7}\n```` |
| code | 200 | 1280 | 1.886e-05 | OK | ````python\nprint(*range(1, 6))\n```` |
| multilingual | 200 | 2372 | 3.634e-05 | OK | `Croatian: Dobro jutro.  \nFrench: Bonjour.  \nJapanese: おはようございます (Ohayou gozaimasu).` |
| refusal_edge | 200 | 1462 | 1.029e-05 | OK | `REFUSED` |
| long | 200 | 10540 | 0.00015122 | OK | `- **Latency**: Report the average, minimum, and maximum response times for queries process` |
| system_user | 200 | 1043 | 1.01e-05 | OK | `323` |
| streaming | 200 | 1098 | 1.402e-05 | OK | `1  \n2  \n3  \n4  \n5` |

## Hermes spend after suite (VPS ledger)
- day_usd: `0.436656` / cap 7.00
- day_calls: `13` / cap 2000
(Direct OpenRouter calls may not all appear in hermes-rs ledger; Hermes chat attempts did.)

## Secrets
No keys in this report. OPENROUTER_API_KEY never copied to /workspace.
