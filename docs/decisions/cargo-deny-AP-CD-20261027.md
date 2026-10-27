# cargo-deny AP/CD — MIT-strict gate (2026-10-27)

**Status:** APPROVED as configured on `swarm/cargo-deny-gate` 400e93a5  
**Date:** 2026-10-27 Europe/Zagreb (+01:00)  
**Authority:** Mate t273u / caller pt273u

## Ruling
Approve the MIT-strict `deny.toml` gate as configured on branch `swarm/cargo-deny-gate` (400e93a5): MIT enforced; plain Apache-2.0 removed from the general allow list; owner-pending exception slot for `safetensors` Apache-2.0 (AP-1); `Apache-2.0 WITH LLVM-exception` noted as pending (AP-4/L7).

## Still needs Mate (do not invent)
- Explicit Apache-2.0 package allowlist (AP-1..AP-6 package names and rationale)
- CD-1 `publish=false` → `wildcards=deny` promotion
- CD-2 promote exception after allowlist

## Reference
- `docs/security/cargo-deny-gate-20261027.md` on cargo-deny-gate branch
- `docs/decisions/apache-2-allow-list.md`
