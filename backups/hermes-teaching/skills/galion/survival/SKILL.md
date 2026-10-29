---
name: survival
description: Survival scope for this host. Cash, CPU, GPU, RunPod, OpenRouter, Revolut. Use when the operator says survival, money, credits, spend, balance, or be effective.
---

# Survival

Read `/opt/galion/galion-light/state/survival.json` before you spend or start a process. If `when` is older than six hours, run `/opt/galion/rust/bin/galion-survival` first (Rust refresh; never print a key). Do not run the old Python `survival.py`.

`cash_usd` is the OpenRouter money left (credits minus usage when both exist). `key_left` is a cap, not extra cash. While `cash_usd` is under 5, prefer Flash-class models. Grok on OpenRouter stays parked until there is a real receipt. The builder machine does the heavy thinking.

This host is the server. Use free CPU for file work, builds, and loop workers. Do not start a second weight. A local answer goes to `127.0.0.1:8094` when a local model is up.

RunPod balance is unmeasured. Do not launch another pod and do not resize this one. Revolut is not wired. Do not invent either balance.

Do not restart the gateway, keep.sh, Whisper, or vision. A job is done when a file, a score, or a health code changed.

Guardian report-only tick writes `/opt/galion/galion-light/state/guardian-tick.json` every 60s (S4.7 classify/dry-run first; no allow-listed apply yet).

## Off-host continuity (2026-10-27)

When the VPS itself may go offline (gov infra gap ~1.5y / Teglenice), read `/opt/galion/hermes/docs/CALL-CONTEXT-20261027.md`. Off-VPS survival = external snapshot + heartbeat + resume (tip aa77864). Do not confuse with cash/CPU survival.json or guardian-tick probes. PR-only until an authorized deploy.
