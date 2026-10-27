# Mate t273u / t274u — deploy-all + Hermes notify

**Date:** 2026-10-27 Europe/Zagreb (+01:00)  
**Authority:** Mate t273u (pt273u) + t274u Hermes notify

## Authorized
- Deploy undeployed service binaries: nerve (own-services + publish-recoveries + flood), cortex (merge-2/3 + OOM), hermes tip if ready, console Rust V2 **only if ready**, backup Rust timer **only after restore-parity**.
- hoff-23 APPROVED; D-241 ACCEPTED; cargo-deny MIT-strict APPROVED as configured.
- Hermes notify APPROVED in principle — configure working channel + TEST send.

## Blocked without further Mate input
- Hermes Telegram: need bot token + chat id (not present on VPS `.env`; do not invent).
- HADES / H1–C5: need admin/custody keys (do not invent).
- Apache-2.0 allowlist package list.
- Console Rust V2 cutover if R1–R5 / Rust binary missing — leave Node console-dist.
- Backup `.sh` → ELF timer switch without restore-parity evidence.

## Safety
- Before any `console.service` stop/restart: temporarily disable cortex live actions; restore after.
- No force-push; never Auto-review approval-retry; secrets never printed; no VPS IP in committed files.
