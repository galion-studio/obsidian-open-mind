# hoff-23 — cloudflared flood / non-zero baseline floor

**Status:** APPROVED  
**Date:** 2026-10-27 Europe/Zagreb (+01:00)  
**Authority:** Mate t273u / caller pt273u

## Ruling
Approve the nerve flood detector baseline floor (zero-baseline / first-bucket skip; ≥3× baseline for flood; cycle-gap bucket spread P-233-1).

## Implementation
- Code: `swarm/nerve-flood-baseline` 5da6eed + `swarm/nerve-flood-cycle-gap` 1d33c59, merged into deploy tip `swarm/nerve-deploy-t273u`.
- Prefer nerve code change over Cloudflare rate-limit / WAF / DNS edits.
- RULES: do **not** touch Cloudflare DNS or HADES for this ruling.

## Out of scope
- Cloudflare dashboard rate-limit rules (not authorized here)
- HADES / H1–C5 key material
