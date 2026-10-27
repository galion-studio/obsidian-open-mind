# overnight-test-laptop (t311u first pass)

Checked: 2026-10-27 19:47:04 CET (Europe/Zagreb).

## Verdict: **NOT_FOUND**

## Probes (VPS loopback)
| URL | result |
|---|---|
| http://127.0.0.1:11434/api/tags | connect fail |
| http://127.0.0.1:11434/v1/models | connect fail |
| http://127.0.0.1:11435/v1/models | connect fail |
| http://127.0.0.1:8080/v1/models | 404 (galion-console, not LLM) |
| http://127.0.0.1:18080/v1/models | 404 (galion-console) |
| ss listeners 11434/11435 | none |

## Docs searched (box)
- `docs/lan-plugin.md` / `lan-discovery.md`: laptop/ollama shape historically `http://127.0.0.1:11434/v1` via tunnel; not live now.
- `reallife-link.md`: laptop_port false in prior runs.
- No live bridge endpoint found on VPS or box for uncensored laptop model within this pass.

Will re-probe if bridge appears later in overnight loop.

## Extra live state checked
- `/opt/galion/galion-light/state/laptop.json` exists on VPS (see push commit body / companion note); no :11434 listener.
- Historical CALL-RECORD: laptop :11434 down as of prior live-ai-plan.
