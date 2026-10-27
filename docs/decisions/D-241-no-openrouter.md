# D-241 — nerve/cortex never OpenRouter

**Status:** ACCEPTED (standing rule)  
**Date:** 2026-10-27 Europe/Zagreb (+01:00)  
**Authority:** Mate t273u / caller pt273u

## Ruling
Nerve and cortex must never call OpenRouter, directly or indirectly (including through Hermes as a hidden path from those services). Hermes on the VPS, using the OpenRouter key already there, remains the only research path to OpenRouter.

## Implementation retained
- `galion-nerve.service.d/75-no-openrouter.conf` (UnsetEnvironment of OPENROUTER_* / LLM keys; IPAllow localhost intent)
- `gaia-cortex.service.d/50-safety.conf` UnsetEnvironment of OPENROUTER_* / OPENAI_* / ANTHROPIC_*
- Code endpoint guards on cortex River-only advisor

## Rollback
Do not remove these drop-ins without a new Mate ruling that supersedes D-241.
