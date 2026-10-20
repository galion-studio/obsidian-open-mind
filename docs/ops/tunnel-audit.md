<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Tunnel audit: cloudflared ingress vs. what actually listens

Slot 35 (`swarm/tunnel-audit`), wave 2. Read-only audit of the VPS, captured
**2026-10-20 20:35 +02:00**. Nothing on the VPS was changed, restarted or written.
The credentials file (`/root/.cloudflared/<id>.json`) and any `cert.pem` were **not** read.

Sources (all read-only): the ingress config named by `cloudflared.service`
(`/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`, 3319 B,
mtime 2026-10-18 22:36 +02:00), `ss -ltnp`, `systemctl cat/status`,
`/proc/<pid>/{cmdline,exe,cwd,cgroup}`, environment variable **names** only
(values only for `*_PORT` / `*_DATA_DIR` / `*_STATE_DB`), `journalctl -u cloudflared`
(last 24 h), cloudflared's local `/metrics` (127.0.0.1:20241 and :20242).
Raw captures: `/workspace/swarm/evidence/tunnel-audit/` on the box; a
secrets-free copy of the config and the `ss` capture are committed as
`crates/tunnel-lint/tests/fixtures/`.

## Headline

- The named tunnel has **39 ingress rules**. Only **5 rules (4 ports) reach a live
  process**. **33 rules (33 hostnames) point at 21 local ports where nothing
  listens**: 3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4000, 4001, 4003,
  4004, 4020, 4042, 4050, 4060, 4444, 5530, 7681, 8000, 8889. swarm/nerve's 10
  ports / 13 hostnames are the subset that had traffic in its window; this is the full list.
- cloudflared metrics since its last start (2026-10-18 22:36 +02:00):
  `total_requests 84841`, `request_errors 50061` → **59.0 % of tunnel requests fail**
  (visitors get Cloudflare's 502 page). In the last 24 h the journal has **30 108**
  "Request failed … connection refused" lines; top: merlinelement.com 6849,
  galion.studio 5784, workspace3d.app 3312, maivirtue.com 1720, api.galion.app 1627.
- No systemd unit, pm2, or docker owns any of the 21 dead ports (none installed;
  `grep` over `/etc/systemd/system/*.service` finds no reference). Source trees
  with matching names exist under `/opt/galion/galion-src/`, none with build
  output (`.next`, `dist`, `build`, `out`).
- Public check from the box at 20:42 +02:00: `https://galion.studio/`,
  `https://merlinelement.com/`, `https://api.galion.app/` → `502`.

## Hostname → port → owner → status

Status from `tunnel-lint --ss` on the capture (identical to a live probe at the
same time). 24 h failures = journal "Request failed" lines summed over
http/https and :443/:8443 variants. Owner = the process from `ss -ltnp`, or for
dead ports the best candidate source tree (by name; nothing runs it).

| # | Hostname | Port | Owner | Status | 24 h failures |
|---|---|---|---|---|---|
| 1 | one.galion.app `^/tanga(/.*)?$` | 8671 | `tanga-serve` (tanga-proto.service) | OPEN | 0 |
| 2 | one.galion.app | 8091 | `galion-one` (galion-one.service) | OPEN | 0 |
| 3 | console.galion.app | 8080 | `node … vite dev --port 8080` (console.service, `/opt/galion/console-ide`) | OPEN | 0 |
| 4 | obsidian.galion.app | 8080 | same vite dev server as console (looks like a mis-route) | OPEN | 0 |
| 5 | code.galion.app | 7681 | none (7681 = ttyd default web-terminal port; ttyd not installed) | CLOSED | 118 |
| 6 | jupyter.adriaticpadel.com | 8889 | none (Jupyter; nothing installed as a unit) | CLOSED | 0 |
| 7–8 | adriaticpadel.com, www. | 3010 | none (src: `galion-src/adriatic-padel`) | CLOSED | 1138 + 56 |
| 9–10 | galion.app, www. | 4000 | none (src: `galion-src/galion-app`) | CLOSED | 1430 + 559 |
| 11 | studio.galion.app | 4001 | none (src: `galion-src/galion-studio-app`) | CLOSED | 1351 |
| 12 | developer.galion.app | 3333 | none (src: `galion-src/galion-developer-portal`) | CLOSED | 126 |
| 13 | api.galion.app | 8000 | none (src: `galion-src/galion-backend`) | CLOSED | 1627 |
| 14 | docs.galion.app | 4042 | none (Docusaurus, src: `galion-src/galion-docs`) | CLOSED | 331 |
| 15 | reference.galion.app | 3080 | none (Next.js, src: `galion-src/galion-api-docs`) | CLOSED | 297 |
| 16 | voice.galion.app | 4003 | none (src: `galion-src/galion-voice-test` / `voice-backend`) | CLOSED | 629 |
| 17 | downloader.galion.app | 4444 | none (no matching source tree) | CLOSED | 774 |
| 18 | knowledge.galion.app | 3015 | none (src: `galion-src/galion-knowledge`) | CLOSED | 6 |
| 19–20 | galion.studio, www. | 4001 | none (src: `galion-src/galion-studio-app`) | CLOSED | 5784 + 564 |
| 21–24 | ws3d.app, www.ws3d.app, workspace3d.app, www.workspace3d.app | 4020 | none (src: `galion-src/galion-workspace3d`) | CLOSED | 1115 + 92 + 3312 + 117 |
| 25–26 | slaviknomad.com, www. | 4060 | none (src: `galion-src/slaviknomad`) | CLOSED | 521 + 8 |
| 27–28 | villapenava.com, www. | 4050 | none (src: `galion-src/villapenava`) | CLOSED | 0 |
| 29 | machine.galion.app | 3004 | none (no matching source tree) | CLOSED | 628 |
| 30 | torchbearer.galion.app | 3005 | none (no matching source tree) | CLOSED | 698 |
| 31–32 | tactical-belts.galion.app, www. | 4004 | none (src: `galion-src/tactical-belts`, `tactical-belts-pro`) | CLOSED | 117 + 0 |
| 33–34 | maivirtue.com, www. | 3020 | none (src: `galion-src/maivirtue`) | CLOSED | 1720 + 13 |
| 35–36 | merlinelement.com, www. | 3011 | none (src: `galion-src/merlinelement`, `merlin-element`) | CLOSED | 6849 + 127 |
| 37 | tulu.galion.app | 5530 | none (src: `galion-src/galion-tulu`; Rust port is `crates/openmind-tulu`) | CLOSED | 1 |
| 38 | tuner.galion.app | 8650 | `tuner` (tuner.service) | OPEN, **but no DNS record** (VPS: "Could not resolve host") | 0 hits |
| 39 | `*` | — | `http_status:404` catch-all | built-in | — |

## Other findings

1. **The config file is world-writable** (`-rw-rw-rw- root root`). Any local user or
   compromised service can reroute any hostname. Fix: `chmod 644` (owner action).
2. **Dead routes are a latent exposure.** Any process that later binds one of the
   21 ports on 127.0.0.1 (dev servers love 3000-4000 and 8000) is published to the
   internet with no review. 7681 (`code.galion.app`) is the web-terminal port: if a
   ttyd ever starts there, a root shell goes public.
3. `localhost` origins dial `[::1]` first (journal: `dial tcp [::1]:4001`). Every
   live service here binds 127.0.0.1 only; use `127.0.0.1` in routes so a dead v6
   attempt is not part of each request.
4. `console.galion.app` and `obsidian.galion.app` publish a **vite dev server**
   (`vite dev`, 1.1 GB RSS) rather than a built bundle.
5. The `# Catch-all (required)` comment sits above `tuner.galion.app`, not above the
   real catch-all (the tuner rule was inserted after the comment). Cosmetic, but
   misleading for the next editor.
6. Global `originRequest.noTLSVerify: true` is harmless today (all origins are
   plain http) but would silently accept any cert if an https origin is added.

## The quick tunnel (`tuner-quick.service`)

- Transient unit (`/run/systemd/transient/tuner-quick.service`, "Temporary
  trycloudflare tunnel for tuner"), running since 2026-10-18 19:19:30 +02:00:
  `cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650`, pid 1198838,
  metrics on 127.0.0.1:20242. It publishes the tuner at a random
  `*.trycloudflare.com` hostname (live: `GET` → 200, 2356 B, at 20:42 +02:00).
  21 requests total since start.
- Why it exists: the named tunnel already has a `tuner.galion.app` rule, but
  **`tuner.galion.app` has no DNS record**, so that route is unreachable.
- Risk: an anonymous Cloudflare quick tunnel (no account, no access policy, no
  SLA, URL changes on restart); it is transient, so it vanishes on reboot anyway.
- Recommendation (owner, needs approval because it is a DNS change): add the
  `tuner.galion.app` CNAME to the named tunnel (`cloudflared tunnel route dns …`
  or the dashboard), verify `https://tuner.galion.app/` → 200, then
  `systemctl stop tuner-quick` (no unit file to delete; it is transient).

## Unowned listeners (8431-8433, 8652, 8653)

All five are leftovers of the 2026-10-18 "growth" benchmark session in
`/root/growth-20261018` (bench scripts `bench-mem.sh`, `bench-hermes.sh` and their
result files are next to them). They were started by hand from SSH sessions
(cgroup `user.slice/user-0.slice/session-NNNNN.scope`, re-parented to pid 1), so no
unit supervises, restarts or logs them. None is referenced by any ingress rule.

| Port | PID | Binary | Started (+02:00) | RSS | What it is |
|---|---|---|---|---|---|
| 8431 | 1375169 | `/opt/galion/rust/bin/openmind-memory` (the live binary) | 2026-10-18 22:14:52 | 4.0 MB | "shadow-old" memory instance, data dir `/root/growth-20261018/shadow-old` |
| 8432 | 1412682 | `/root/growth-20261018/bin/openmind-memory.new` **(deleted)** | 2026-10-18 22:23:51 | 6.6 MB | "shadow-new" memory instance, data dir `…/shadow-new` |
| 8433 | 1428048 | same deleted `openmind-memory.new` | 2026-10-18 22:24:14 | 3.2 MB | read-only + recall-cache variant over the same `…/shadow-new` dir (`OPENMIND_MEMORY_READ_ONLY`, `…_RECALL_CACHE` set) |
| 8652 | 1395619 | `/opt/galion/rust/bin/hermes-gateway --mock` | 2026-10-18 22:15:53 | 52 MB | "hermes-old" bench gateway, state `…/hermes-old/s.db` |
| 8653 | 1443105 | `/root/growth-20261018/bin/hermes-gateway.new --mock` | 2026-10-18 22:32:28 | 29 MB | "hermes-new" bench gateway, state `…/hermes-new/s.db` |

Notes:
- Both hermes strays run `--mock` and their environment has **no**
  `OPENROUTER_API_KEY` / `RIVER_API_KEY` (names checked), so no paid-call risk;
  they do hold `API_SERVER_KEY` and the memory strays an `OPENMIND_MEMORY_API_KEY`
  in their environment (values not read).
- **8652 is the port `deploy/staged/hermes-rs.service` (the Rust Hermes shadow)
  is staged to use.** Installing that unit while the stray lives fails with
  "address in use".
- 8432/8433 run a binary whose file was replaced/deleted; they cannot be
  restarted as-is and do not match any shipped build.

Recommendation (owner action; I did not touch them): copy anything wanted from
`/root/growth-20261018/*.txt` (bench results), then `kill 1375169 1412682 1428048
1395619 1443105` (plain SIGTERM), confirm with `ss -ltn` that 8431-8433/8652/8653 are
gone, and archive `/root/growth-20261018`. If a shadow is still wanted, run it
from a unit (`deploy/staged/hermes-rs.service` for 8652), not from an SSH shell.

## Staged config.yml change (NOT applied)

Generated by `tunnel-lint --stage` from the secrets-free copy:
`docs/ops/tunnel-audit.config.staged.yml`. Every rule whose port is closed is
commented out (with a `# tunnel-lint: closed port N` marker), so those
hostnames fall through to the existing `http_status:404` catch-all instead of
a 502 plus an ERR journal line per request. Live routes, the catch-all,
`tunnel:` and `credentials-file:` are untouched. `tunnel-lint` on the staged
file: `6 rules, 0 closed routes, 0 findings`.

To restore a site later: start its service on its port, uncomment its rule,
re-run `tunnel-lint`, reload. Hostnames whose DNS should go away entirely
(owner decision) also need their CNAME removed in Cloudflare.

Owner apply sequence (needs approval; not done):
```
cp -a config.yml config.yml.bak-$(date +%Y%m%d-%H%M%S)
patch config.yml < tunnel-audit.ingress.diff     # the hunk below
tunnel-lint config.yml                            # expect exit 0
cloudflared tunnel --config config.yml ingress validate
chmod 644 config.yml
systemctl restart cloudflared                     # brief reconnect of 4 HA conns
```

Ingress hunk, line numbers as in the live file:

```diff
--- a/config.yml
+++ b/config.yml
@@ -24,106 +24,139 @@
 - hostname: obsidian.galion.app
   service: http://127.0.0.1:8080
 
-- hostname: code.galion.app
-  service: http://127.0.0.1:7681
-
-- hostname: jupyter.adriaticpadel.com
-  service: http://127.0.0.1:8889
-
-- hostname: adriaticpadel.com
-  service: http://127.0.0.1:3010
-- hostname: www.adriaticpadel.com
-  service: http://127.0.0.1:3010
-
-- hostname: galion.app
-  service: http://localhost:4000
-- hostname: www.galion.app
-  service: http://localhost:4000
-
-- hostname: studio.galion.app
-  service: http://localhost:4001
-
-- hostname: developer.galion.app
-  service: http://localhost:3333
+# tunnel-lint: closed port 7681
+# - hostname: code.galion.app
+#   service: http://127.0.0.1:7681
+
+# tunnel-lint: closed port 8889
+# - hostname: jupyter.adriaticpadel.com
+#   service: http://127.0.0.1:8889
+
+# tunnel-lint: closed port 3010
+# - hostname: adriaticpadel.com
+#   service: http://127.0.0.1:3010
+# tunnel-lint: closed port 3010
+# - hostname: www.adriaticpadel.com
+#   service: http://127.0.0.1:3010
+
+# tunnel-lint: closed port 4000
+# - hostname: galion.app
+#   service: http://localhost:4000
+# tunnel-lint: closed port 4000
+# - hostname: www.galion.app
+#   service: http://localhost:4000
+
+# tunnel-lint: closed port 4001
+# - hostname: studio.galion.app
+#   service: http://localhost:4001
+
+# tunnel-lint: closed port 3333
+# - hostname: developer.galion.app
+#   service: http://localhost:3333
 
 # API with WebSocket optimization for voice streaming
-- hostname: api.galion.app
-  service: http://127.0.0.1:8000
-  originRequest:
-    noTLSVerify: true
-    connectTimeout: 60s
-    tcpKeepAlive: 30s
-    keepAliveTimeout: 120s
-    httpHostHeader: api.galion.app
+# tunnel-lint: closed port 8000
+# - hostname: api.galion.app
+#   service: http://127.0.0.1:8000
+#   originRequest:
+#     noTLSVerify: true
+#     connectTimeout: 60s
+#     tcpKeepAlive: 30s
+#     keepAliveTimeout: 120s
+#     httpHostHeader: api.galion.app
 
 # Docusaurus Documentation - Disable caching for dev mode
-- hostname: docs.galion.app
-  service: http://localhost:4042
-  originRequest:
-    noTLSVerify: true
-    httpHostHeader: docs.galion.app
-    disableChunkedEncoding: false
+# tunnel-lint: closed port 4042
+# - hostname: docs.galion.app
+#   service: http://localhost:4042
+#   originRequest:
+#     noTLSVerify: true
+#     httpHostHeader: docs.galion.app
+#     disableChunkedEncoding: false
 
 # API Reference (Next.js)
-- hostname: reference.galion.app
-  service: http://localhost:3080
-
-- hostname: voice.galion.app
-  service: http://localhost:4003
-
-- hostname: downloader.galion.app
-  service: http://localhost:4444
-
-- hostname: knowledge.galion.app
-  service: http://localhost:3015
-
-- hostname: galion.studio
-  service: http://localhost:4001
-- hostname: www.galion.studio
-  service: http://localhost:4001
-
-- hostname: ws3d.app
-  service: http://localhost:4020
-- hostname: www.ws3d.app
-  service: http://localhost:4020
-- hostname: workspace3d.app
-  service: http://localhost:4020
-- hostname: www.workspace3d.app
-  service: http://localhost:4020
-
-- hostname: slaviknomad.com
-  service: http://localhost:4060
-- hostname: www.slaviknomad.com
-  service: http://localhost:4060
-
-- hostname: villapenava.com
-  service: http://localhost:4050
-- hostname: www.villapenava.com
-  service: http://localhost:4050
-
-- hostname: machine.galion.app
-  service: http://localhost:3004
-
-- hostname: torchbearer.galion.app
-  service: http://localhost:3005
-
-- hostname: tactical-belts.galion.app
-  service: http://localhost:4004
-- hostname: www.tactical-belts.galion.app
-  service: http://localhost:4004
-
-- hostname: maivirtue.com
-  service: http://localhost:3020
-- hostname: www.maivirtue.com
-  service: http://localhost:3020
-
-- hostname: merlinelement.com
-  service: http://localhost:3011
-- hostname: www.merlinelement.com
-  service: http://localhost:3011
-
-- hostname: tulu.galion.app
-  service: http://localhost:5530
+# tunnel-lint: closed port 3080
+# - hostname: reference.galion.app
+#   service: http://localhost:3080
+
+# tunnel-lint: closed port 4003
+# - hostname: voice.galion.app
+#   service: http://localhost:4003
+
+# tunnel-lint: closed port 4444
+# - hostname: downloader.galion.app
+#   service: http://localhost:4444
+
+# tunnel-lint: closed port 3015
+# - hostname: knowledge.galion.app
+#   service: http://localhost:3015
+
+# tunnel-lint: closed port 4001
+# - hostname: galion.studio
+#   service: http://localhost:4001
+# tunnel-lint: closed port 4001
+# - hostname: www.galion.studio
+#   service: http://localhost:4001
+
+# tunnel-lint: closed port 4020
+# - hostname: ws3d.app
+#   service: http://localhost:4020
+# tunnel-lint: closed port 4020
+# - hostname: www.ws3d.app
+#   service: http://localhost:4020
+# tunnel-lint: closed port 4020
+# - hostname: workspace3d.app
+#   service: http://localhost:4020
+# tunnel-lint: closed port 4020
+# - hostname: www.workspace3d.app
+#   service: http://localhost:4020
+
+# tunnel-lint: closed port 4060
+# - hostname: slaviknomad.com
+#   service: http://localhost:4060
+# tunnel-lint: closed port 4060
+# - hostname: www.slaviknomad.com
+#   service: http://localhost:4060
+
+# tunnel-lint: closed port 4050
+# - hostname: villapenava.com
+#   service: http://localhost:4050
+# tunnel-lint: closed port 4050
+# - hostname: www.villapenava.com
+#   service: http://localhost:4050
+
+# tunnel-lint: closed port 3004
+# - hostname: machine.galion.app
+#   service: http://localhost:3004
+
+# tunnel-lint: closed port 3005
+# - hostname: torchbearer.galion.app
+#   service: http://localhost:3005
+
+# tunnel-lint: closed port 4004
+# - hostname: tactical-belts.galion.app
+#   service: http://localhost:4004
+# tunnel-lint: closed port 4004
+# - hostname: www.tactical-belts.galion.app
+#   service: http://localhost:4004
+
+# tunnel-lint: closed port 3020
+# - hostname: maivirtue.com
+#   service: http://localhost:3020
+# tunnel-lint: closed port 3020
+# - hostname: www.maivirtue.com
+#   service: http://localhost:3020
+
+# tunnel-lint: closed port 3011
+# - hostname: merlinelement.com
+#   service: http://localhost:3011
+# tunnel-lint: closed port 3011
+# - hostname: www.merlinelement.com
+#   service: http://localhost:3011
+
+# tunnel-lint: closed port 5530
+# - hostname: tulu.galion.app
+#   service: http://localhost:5530
 
 # Catch-all (required)
 - hostname: tuner.galion.app
```

## tunnel-lint

`crates/tunnel-lint` (no dependencies). `tunnel-lint [--ss FILE] [--timeout-ms N]
[--stage OUT] [--tsv] [--no-fail] CONFIG.yml`

- Parses the cloudflared ingress subset (list items with `hostname`, `path`,
  `service`; nested `originRequest` blocks skipped; quotes and `#` comments handled;
  tabs and flow mappings rejected).
- Resolves `service:` to an origin: local (`localhost`, `127.*`, `::1`, `0.0.0.0`),
  remote (not probed) or built-in (`http_status:`, `hello_world`, `unix:` …).
- Checks listeners live (TCP connect, `localhost` → 127.0.0.1 then ::1, like
  cloudflared) or from a captured `ss -ltn[p]` text (`--ss`), which also names the
  owning process.
- Errors: route to a closed port, invalid service, last rule not a catch-all.
  Warnings: rule after the catch-all, duplicate hostname/path, world-writable file.
- `--stage OUT` writes a copy with closed-port rules commented out (never the catch-all).
- Refuses `*.json` / `*.pem` paths so it can't be pointed at tunnel credentials;
  never opens the `credentials-file`.
- Exit 0 clean, 1 errors, 2 usage/read/parse error. Release binary: 447 KB
  (x86_64 musl), 392 KB (aarch64 musl).

Real-life test: `scripts/real/tunnel-audit.sh`, results in `docs/real-tests/tunnel-audit.md`.
