<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Legacy web backend host decisions

- DESK-ID: openmind.B66
- Generated: 2026-10-19T17:12:54+02:00 (Europe/Zagreb, TZ=Europe/Zagreb date -Iseconds)
- Repo: /opt/galion/openmind
- Scope: one row per ingress hostname whose origin port has no TCP listener.
- Policy: decision table only. DNS was not changed. The cloudflared config was not edited. Nothing was stopped.
- Rule: if a revive or a static archive is not clear from ss or a directory listing, the decision is retire and staged is yes.

Hostname and service lines were read from /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml. That is the config path on the running named-tunnel process. The file was not modified. Credential files were not opened. Secret files were not opened. Listener check: ss -lnt on 2026-10-19. None of the ports in the table were listening.

Staged yes means the ingress cleanup is recorded for later approval. It was not applied.

## Decision table

| host or service name | current state | decision | staged |
|---|---|---|---|
| code.galion.app | ingress http://127.0.0.1:7681; ss -lnt has no listener. File listing: galion-src/galion-infrastructure/pm2/ecosystem.ttyd.config.js names ttyd on port 7681. Not running. | retire | yes |
| jupyter.adriaticpadel.com | ingress http://127.0.0.1:8889; ss -lnt has no listener. No jupyter directory in the /opt/galion/galion-src listing. | retire | yes |
| adriaticpadel.com | ingress http://127.0.0.1:3010; ss -lnt has no listener. Directory listing: galion-src/adriatic-padel (Next.js names: package.json, src, public). Bodies not opened. | retire | yes |
| www.adriaticpadel.com | same origin as adriaticpadel.com, port 3010; ss -lnt has no listener. | retire | yes |
| galion.app | ingress http://localhost:4000; ss -lnt has no listener. health-check.sh names port 4000 Galion App. Directory listing: galion-src/galion-app. Bodies not opened. | retire | yes |
| www.galion.app | same origin as galion.app, port 4000; ss -lnt has no listener. | retire | yes |
| studio.galion.app | ingress http://localhost:4001; ss -lnt has no listener. health-check.sh names port 4001 Galion Studio. Directory listing: galion-src/galion-studio-app. Bodies not opened. | retire | yes |
| developer.galion.app | ingress http://localhost:3333; ss -lnt has no listener on 3333. health-check.sh also names port 4002 Developer Portal; 4002 is not listening either. Directory listing: galion-src/galion-developer-portal. Which port was last bound is not clear. | retire | yes |
| api.galion.app | ingress http://127.0.0.1:8000; ss -lnt has no listener. pm2 ecosystem.config.js names galion-backend, uvicorn, port 8000. Directory listing: galion-src/galion-backend. Bodies not opened. | retire | yes |
| docs.galion.app | ingress http://localhost:4042; ss -lnt has no listener. Directory listing: galion-src/galion-docs (includes docusaurus.config.js). Port bind inside that tree was not opened. | retire | yes |
| reference.galion.app | ingress http://localhost:3080; ss -lnt has no listener. Directory listing: galion-src/galion-api-docs (Next.js names). Not confirmed as the process for port 3080. | retire | yes |
| voice.galion.app | ingress http://localhost:4003; ss -lnt has no listener. Directory listing: galion-src/voice-backend and galion-src/galion-voice-test. Not confirmed which tree bound port 4003. Flagged ecosystem files were not opened. | retire | yes |
| downloader.galion.app | ingress http://localhost:4444; ss -lnt has no listener. No downloader directory in the /opt/galion/galion-src listing. | retire | yes |
| knowledge.galion.app | ingress http://localhost:3015; ss -lnt has no listener. Directory listing: galion-src/galion-knowledge (Next.js names). Port bind not in the health-check script. | retire | yes |
| galion.studio | ingress http://localhost:4001; ss -lnt has no listener. Same port as studio.galion.app. Directory listing: galion-src/galion-studio-app. | retire | yes |
| www.galion.studio | same origin as galion.studio, port 4001; ss -lnt has no listener. | retire | yes |
| ws3d.app | ingress http://localhost:4020; ss -lnt has no listener. health-check.sh names port 4020 Workspace3D. Directory listing: galion-src/galion-workspace3d. | retire | yes |
| www.ws3d.app | same origin as ws3d.app, port 4020; ss -lnt has no listener. | retire | yes |
| workspace3d.app | ingress http://localhost:4020; ss -lnt has no listener. Directory listing: galion-src/galion-workspace3d. | retire | yes |
| www.workspace3d.app | same origin as workspace3d.app, port 4020; ss -lnt has no listener. | retire | yes |
| slaviknomad.com | ingress http://localhost:4060; ss -lnt has no listener. Directory listing: galion-src/slaviknomad (Next.js names). Port bind not confirmed beyond the name match. | retire | yes |
| www.slaviknomad.com | same origin as slaviknomad.com, port 4060; ss -lnt has no listener. | retire | yes |
| villapenava.com | ingress http://localhost:4050; ss -lnt has no listener. Directory listing: galion-src/villapenava (Next.js names). Port bind not confirmed beyond the name match. | retire | yes |
| www.villapenava.com | same origin as villapenava.com, port 4050; ss -lnt has no listener. | retire | yes |
| machine.galion.app | ingress http://localhost:3004; ss -lnt has no listener. Directory listing: galion-light/publish/the-machine (index.html only). Not confirmed as the port 3004 origin. | retire | yes |
| torchbearer.galion.app | ingress http://localhost:3005; ss -lnt has no listener. No torchbearer directory in a depth-4 name search under /opt/galion. | retire | yes |
| tactical-belts.galion.app | ingress http://localhost:4004; ss -lnt has no listener. Directory listing: galion-src/tactical-belts and galion-src/tactical-belts-pro (Next.js names). Port bind not confirmed beyond the name match. | retire | yes |
| www.tactical-belts.galion.app | same origin as tactical-belts.galion.app, port 4004; ss -lnt has no listener. | retire | yes |
| maivirtue.com | ingress http://localhost:3020; ss -lnt has no listener. Directory listing: galion-src/maivirtue (Next.js names). Port bind not confirmed beyond the name match. | retire | yes |
| www.maivirtue.com | same origin as maivirtue.com, port 3020; ss -lnt has no listener. | retire | yes |
| merlinelement.com | ingress http://localhost:3011; ss -lnt has no listener. Directory listing: galion-src/merlin-element (Vite names) and galion-src/merlinelement (.git only). Port bind not confirmed beyond the name match. | retire | yes |
| www.merlinelement.com | same origin as merlinelement.com, port 3011; ss -lnt has no listener. | retire | yes |
| tulu.galion.app | ingress http://localhost:5530; ss -lnt has no listener. Directory listing: galion-light/tulu (main.rs, binaries tulu and tulu-gpu, not listening) and galion-src/galion-tulu (Next.js names). Public host not revived. | retire | yes |

## Not in this table

These ingress names have a listener. They are not legacy dead origins, so they have no retire, static archive, or revive row. They were not stopped.

| host or service name | current state |
|---|---|
| one.galion.app | listening: galion-one on 127.0.0.1:8091. Path /tanga also listening: tanga-serve on 127.0.0.1:8671. |
| console.galion.app | listening: node on 127.0.0.1:8080. |
| obsidian.galion.app | listening: node on 127.0.0.1:8080. |
| tuner.galion.app | listening: tuner on 127.0.0.1:8650. |

A second cloudflared process was started with --url http://127.0.0.1:8650. It was not stopped. It is not a row in the decision table. The catch-all ingress rule http_status:404 is not a host.

## Staged ingress cleanup

Later, and only with approval, the hostnames in the decision table can be removed from tunnel ingress. Do not remove one.galion.app, console.galion.app, obsidian.galion.app, or tuner.galion.app in that step. Do not change DNS here. This ticket does not edit the cloudflared config.

No row is static archive or revive. A frozen page or a restart was not clear from ss or the listings. tulu.galion.app is retired as a public origin only. The Rust wrapper under galion-light/tulu stays a separate ticket and was not started.
