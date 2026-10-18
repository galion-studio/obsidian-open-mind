# Workflow: first-connect-and-map
**Trigger:** Mate provides VPS host/IP + username (or the setup changed significantly).
**Skills:** vps-ssh-access → stack-mapping → service-health-check
**Mutating:** no (local config edits only)

## Steps
1. **Configure** — `~/workflows/vps/bin/set-target.sh <host> <user> [port]`
2. **Key installed?** — `~/workflows/vps/bin/vps-ssh.sh --print-config`; if Slavik hasn't installed the public key, give Mate the `.pub` line to pass on. Gate: wait for confirmation.
3. **Connect** — `~/workflows/vps/bin/vps-ssh.sh --check` → note hostname, OS, sudo capability; get host fingerprint `ssh-keygen -lF <host>` and, ideally, confirm it with Slavik.
4. **Discover** — `~/workflows/vps/bin/map-stack.sh` → `data/maps/map-<ts>.txt`.
5. **Drill down** per component (Galleon, Galleon1, Obsidian, OpenMind) using the read-only commands in `~/skills/stack-mapping/SKILL.md` step 2.
6. **Record** — fill `~/workflows/vps/stack-map.md` (facts only, TBD/UNVERIFIED otherwise) and add checks to `~/workflows/vps/services.list`.
7. **Baseline health** — `~/workflows/vps/bin/health-check.sh` → keep as baseline.
8. **Open questions** — list in stack map (Galleon1 role, deploy procedure, restart policy, alert channel).

## Outputs
Configured connection, `data/maps/map-*.txt`, filled `stack-map.md`, `services.list`, baseline `data/health/health-*.txt`.

## Report
Connection result; per-component summary (how run / where / port / URL); anomalies; open questions for Slavik; file paths.
