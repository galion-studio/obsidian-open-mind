# Decision record: Revolut standing ban lifted (Hermes Rust V2)

**When:** 2026-10-20 ~22:56 CEST (+02:00)
**Who:** Slavik (typed; relayed by Mate Grajczyk in Grok Bot chat)
**Exact words:** "Revolut bank is allowed on the current Rust V2 redesign of V1 Hermes Python build."

## What changed
- Previous hard rule: "No Revolut, no real money…".
- New rule: Revolut is in scope for the Hermes Rust V2 redesign (replacing the V1 Hermes Python build) and for the admin-phone connection design. RULES.md §5 and `config/swarm/swarm-settings.toml` `[revolut]` updated.

## What did NOT change
- Agents must not place orders, send transfers, top up, withdraw, buy, sell, or otherwise move money.
- Live mutating Revolut API calls need a separate written approval from Slavik.
- Card PANs and banking secrets are never stored in the repo or printed.

## Where recorded
- `/workspace/swarm/RULES.md` hard rule 5
- `swarm/settings-snapshot` → `config/swarm/swarm-settings.toml` `[revolut]`
- this file
- CALL-RECORD.md
