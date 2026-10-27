# overnight-test-laptop (t311u first pass)

Checked: 2026-10-27 19:47:53 CET (Europe/Zagreb).

## Verdict: **NOT_FOUND** (uncensored / ollama bridge offline)

## Live inventory card (not an LLM endpoint)
`/opt/galion/galion-light/state/laptop.json` present (mtime ~2026-10-27 16:42 UTC = 17:42 CET):
- name: GALION-STUDIO
- cpu: 13th Gen Intel i7-13620H
- gpu: NVIDIA GeForce RTX 4060 Laptop GPU 8188 MiB
- ram_gb: 63.7 / free_gb: 56.2
- user: Gigabyte

Card age ≈ hours before this probe; **does not** imply ollama/LLM listening.

## Probes (VPS loopback)
| URL | result |
|---|---|
| http://127.0.0.1:11434/api/tags | connect fail |
| http://127.0.0.1:11434/v1/models | connect fail |
| http://127.0.0.1:11435/v1/models | connect fail |
| http://127.0.0.1:8080/v1/models | 404 (galion-console) |
| http://127.0.0.1:18080/v1/models | 404 (galion-console) |
| ss listeners 11434/11435 | none |

## Docs / paths searched
- galion docs: laptop/ollama shape `http://127.0.0.1:11434/v1` via tunnel
- `/opt/galion/offsite/laptop/` install scripts only
- CALL-RECORD prior: laptop :11434 down

Re-probe later in overnight loop if bridge comes up.
