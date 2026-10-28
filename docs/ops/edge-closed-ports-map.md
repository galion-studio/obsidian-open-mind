<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Edge closed-ports map: hostname -> ingress rule -> origin port -> owner

Slot 380 (`swarm/edge-closed-ports-map`, off publish-blockers-refresh b892d4d7).
Read-only VPS capture **2026-10-28 10:06-10:15 CET (+01:00)**. Nothing on the VPS
was changed: no DNS change, no cloudflared config change, no restart. The tunnel
credentials file was never read; the committed config copy has the tunnel id
zeroed and the credentials path replaced.

Sources: the config named by `cloudflared.service` ExecStart
(`/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`, 3420 B,
mode 644 root, mtime 2026-10-21 16:08:54 CET), `ss -ltnp`, `systemctl show`,
grep of every unit file for each port, `/opt/galion/galion-src/*` (manifest
names, last commit date, build dirs), `journalctl -u cloudflared` (24 h and
exact nerve window), cloudflared `/metrics` (127.0.0.1:20241), nerve
`/api/v1/state`, cortex `/health`, public DNS over DoH (cloudflare-dns.com) and
public HTTPS GETs from the box. Raw captures: `/workspace/swarm/evidence/edge-closed-ports-map/`.

## Headline

- **39 ingress rules: 7 open, 31 closed, 1 catch-all.** The 31 closed rules
  point at **20 local ports where nothing listens**: 3004 3005 3010 3011 3015
  3020 3080 3333 4001 4003 4004 4020 4042 4050 4060 4444 5530 8000 8889 18800.
- The nerve incident `inc-06ffb32f` ("15 hostnames -> 6 closed ports" at 06:05,
  "19 -> 9" at 10:00) only names ports that got refused traffic in its rolling
  30-min window. The static map above is the full superset; every port the
  nerve has named today is in it (pinned by test).
- **30 of 31 closed rules are retired** (no unit, no pm2/docker, no build
  output; source trees untouched since 2026-01-20, Node/Next or Python era).
  **1 is broken**: `forge.galion.studio -> :18800`, whose owner (the box-side
  `forge-link` reverse SSH tunnel) has been down since 2026-10-22 10:57 CEST.
- Traffic is real but small: **21,983 failed requests in 24 h** (journal), about
  **11 failed requests/min** now (328 in the last 30 min; metrics +10 errors of
  +11 requests in a 60 s sample). 84 % comes from four ports: 4001
  (galion.studio + studio), 3011 (merlinelement.com), 3010 (adriaticpadel.com),
  4020 (ws3d/workspace3d). Much of it is bots (`/.env`, `/.git/config`,
  `wp-admin/install.php`, `sitemap.xml`).
- cloudflared since its last start (2026-10-22 22:46:47 CET, NRestarts 0):
  241,793 requests, 122,227 errors (50.5 %), 117,981 404s (48.8 %, mostly
  historical; +1 in the 60 s sample). Only ~0.6 % of all tunnel requests were 200.
- The launch path is not affected: galion.app/www, one.galion.app,
  console.galion.app are open (200/200/302 publicly) and their pages link only
  to console.galion.app; none of the 31 dead hostnames is in docs/launch.

## Map

Class: **L** legacy site (source tree only, last commit 2026-01-20, no build
output, nothing runs it), **O** orphan (no source tree at all), **N** no public
DNS record (rule unreachable from the internet), **B** broken (real owner,
owner down). 24 h = journal "Request failed ... connection refused" for that
hostname (one line per failed request). Public = HTTPS GET from the box at
10:08 CET. "Owning unit" = none for every closed port: no systemd unit,
drop-in, pm2 or docker references it.

| Rule | Hostname | Origin port | Owner / source tree (stack) | DNS | Public | 24 h | Class |
|---|---|---|---|---|---|---|---|
| 1 | one.galion.app `^/tanga(/.*)?$` | 8671 | tanga-serve (tanga-proto.service) | yes | 200 | 0 | open |
| 2 | one.galion.app | 8091 | galion-one | yes | 200 | 0 | open |
| 3 | console.galion.app | 8080 | galion-console (Rust, console-rust-primary) | yes | 302 | 0 | open |
| 4 | obsidian.galion.app | 8080 | galion-console (same as rule 3) | yes | 200 | 0 | open |
| 5 | jupyter.adriaticpadel.com | 8889 | none (Jupyter, never a unit) | **NXDOMAIN** | - | 0 | N |
| 6-7 | adriaticpadel.com, www. | 3010 | `adriatic-padel` (package.json) | yes | 502 | 3,188 + 52 | L |
| 8-9 | galion.app, www. | 8091 | galion-one | yes | 200 | 0 | open |
| 10 | studio.galion.app | 4001 | `galion-studio-app` (package.json) | yes | 502 | 236 | L |
| 11 | developer.galion.app | 3333 | `galion-developer-portal` (package.json) | yes | 502 | 8 | L |
| 12 | api.galion.app | 8000 | `galion-backend` (Python, requirements.txt) | yes | 502 | 452 | L |
| 13 | docs.galion.app | 4042 | `galion-docs` (Docusaurus) | yes | 502 | 424 | L |
| 14 | reference.galion.app | 3080 | `galion-api-docs` (Next.js) | yes | 502 | 3 | L |
| 15 | voice.galion.app | 4003 | `galion-voice-test` (Next.js) / `voice-backend` (Python) | yes | 502 | 126 | L |
| 16 | downloader.galion.app | 4444 | none (no source tree) | yes | 502 | 73 | O |
| 17 | knowledge.galion.app | 3015 | `galion-knowledge` (Next.js) | yes | 502 | 456 | L |
| 18-19 | galion.studio, www. | 4001 | `galion-studio-app` (package.json) | yes | 502 | 6,385 (+30 on :8443) + 22 | L |
| 20-23 | ws3d.app, www., workspace3d.app, www. | 4020 | `galion-workspace3d` (Next.js) | yes | 502 | 1,511 + 86 + 372 + 123 | L |
| 24-25 | slaviknomad.com, www. | 4060 | `slaviknomad` (package.json) | yes | 502 | 480 + 12 | L |
| 26-27 | villapenava.com, www. | 4050 | `villapenava` (Next.js) | **NXDOMAIN at .com registry** | - | 0 | N |
| 28 | machine.galion.app | 3004 | none (no source tree) | yes | 502 | 11 | O |
| 29 | torchbearer.galion.app | 3005 | none (no source tree) | yes | 502 | 16 | O |
| 30 | tactical-belts.galion.app | 4004 | `tactical-belts`, `tactical-belts-pro` (package.json) | yes | 502 | 267 | L |
| 31 | www.tactical-belts.galion.app | 4004 | same | **NXDOMAIN** | - | 0 | N |
| 32-33 | maivirtue.com, www. | 3020 | `maivirtue` (package.json) | yes | 502 | 1,038 + 12 | L |
| 34-35 | merlinelement.com, www. | 3011 | `merlin-element` (package.json; `merlinelement` dir has no manifest) | yes | 502 | 6,531 + 7 | L |
| 36 | tulu.galion.app | 5530 | `galion-tulu` (package.json) | yes | 502 | 7 | L |
| 37 | tuner.galion.app | 8650 | tuner (Rust) | **NXDOMAIN** | - | 0 | open, unreachable |
| 38 | forge.galion.studio | 18800 | box `/workspace/forge-link` reverse SSH tunnel; link log: last attempt 2026-10-22 10:57 CEST, ssh timeouts | yes (created after 2026-10-21) | 502 | 45 | **B** |
| 39 | catch-all | `http_status:404` | built-in | - | - | - | - |

Totals: 31 closed rules = N 4 + L 23 + O 3 + B 1 (pinned in
`crates/tunnel-lint/tests/closed_ports_map.rs`). Per port, 24 h failed requests:
4001 6,673 · 3011 6,539 · 3010 3,242 · 4020 2,092 · 3020 1,050 · 4060 499 ·
3015 456 · 8000 452 · 4042 424 · 4004 267 · 4003 126 · 4444 73 · 18800 45 ·
3005 16 · 3004 11 · 3333 8 · 5530 7 · 3080 3 · 8889 0 · 4050 0.

Drift since the 2026-10-20 tunnel audit (slot 35, 33 closed / 21 ports):
`galion.app`/`www` moved from :4000 to galion-one :8091 (now open),
`code.galion.app -> :7681` was removed (the hostname still resolves and now
gets the catch-all 404), and `forge.galion.studio -> :18800` was added
(2026-10-21 17:08 CEST). Config mode went from 666 to 644.

## Staged cleanup proposal (doc only, NOT applied)

`docs/ops/edge-closed-ports-map.config.staged.yml` is the exact
`tunnel-lint --stage` output for this capture: all 31 closed rules commented
out with the reason, 7 live rules + catch-all kept, and re-linted clean
(8 rules, 0 findings). Pinned byte-for-byte by test. It is built from the
secrets-free copy; when applying, stage from the real VPS file so the real
`tunnel:` / `credentials-file:` lines stay as they are.

Suggested order, smallest risk first. Each step needs owner approval (cloudflared
config is a live service change; DNS records are Cloudflare changes).

1. **Phase A, dead in both directions (5 rules, zero visitors):**
   jupyter.adriaticpadel.com, villapenava.com + www, www.tactical-belts.galion.app
   (no DNS at all), plus tuner.galion.app is open but has no DNS (keep or add
   DNS; decision D-380-4). 0 requests in 24 h, so removing them changes nothing
   for anyone. Pure config hygiene.
2. **Phase B, retired and orphan sites (26 rules):** comment out the ingress
   rules. Visitors then get Cloudflare's tunnel catch-all 404 instead of 502,
   the nerve/cortex `edge` incident and the 50 % error counter stop masking
   real faults, and the bot scans stop reaching the origin. Then, separately,
   delete or redirect their DNS records (owner, Cloudflare).
3. **Phase C, brand domains:** galion.studio (6,407/24 h), merlinelement.com
   (6,538), adriaticpadel.com (3,240), maivirtue.com, slaviknomad.com,
   ws3d/workspace3d are real domains with steady traffic. Options per domain:
   redirect at Cloudflare (e.g. galion.studio -> galion.app), a static
   "coming back" page served by an already-running Rust origin (e.g. `tuner`
   on 8650 with a host-based root), or plain removal.
4. **Phase D, forge.galion.studio (broken):** either restart the box link when
   the forge is wanted (`/workspace/forge-link/README.md`), or remove the rule
   and the DNS record. While it stays, it is a public 502 that bots probe for
   `/.env` and `/.git/config`.

Rollback for any step: restore the backed-up config
(`/var/backups/cloudflared-config-<ts>.yml`, same pattern as forge-link on
2026-10-21) and restart cloudflared; the catch-all stays in all variants.

## Findings

- **F-380-1 (nerve, rate inflated ~37x).** The edge incident title says
  "318.0 failed requests/min over 29 min", but the cloudflared journal for the
  nerve's exact window (09:30:01-09:59:47 CET) has 772 refused lines = 386
  failed requests = ~13/min (the nerve's own `edge_failures` is 387). Per port
  the nerve shows :3011 9,152 dials / 4,576 failed vs journal 250 / 125, and
  :4001 8,794 / 4,397 vs 240 / 120. Ratio ~36.6. Hypothesis (not proven on the live 9d1a4f3 build): each
  event cycle re-reads an overlapping journal span and `LogInsights::merge`
  (`crates/openmind-nerve/src/logscan.rs`) adds it on top of the window
  instead of de-duplicating. Severity stays right (still
  >= 1/min -> error), but the number is wrong. Not fixed here (read-only row);
  follow-up for the nerve owner, with a replay test that feeds the same lines
  twice.
- **F-380-2.** `villapenava.com` does not resolve at the .com registry
  (NXDOMAIN from a.gtld-servers.net): the domain looks lapsed. Owner should
  confirm whether it is still wanted before someone else registers it.
- **F-380-3.** `code.galion.app` still has a DNS record but no ingress rule
  (catch-all 404). Part of the DNS cleanup in Phase B.
- **F-380-4.** Bots probe dead hosts for `/.env`, `/.env.local`, `/.git/config`,
  `wp-admin/install.php`. Harmless while the ports are closed; this is the
  reason not to "fix" a dead route by pointing it at a generic dev server.

## Owner decisions

- **D-380-1** Approve Phase A + B (comment out 31 closed rules, keep 7 live +
  catch-all) as one cloudflared config change, or phase by phase.
- **D-380-2** Brand domains (galion.studio, merlinelement.com,
  adriaticpadel.com, maivirtue.com, slaviknomad.com, ws3d/workspace3d):
  redirect, placeholder page from a running Rust origin, or remove.
- **D-380-3** forge.galion.studio: bring the box link back or retire the rule + DNS.
- **D-380-4** tuner.galion.app: it has a live origin but no DNS. Add DNS or drop the rule.
- **D-380-5** DNS records for retired hostnames: delete vs keep (Cloudflare, owner only).
