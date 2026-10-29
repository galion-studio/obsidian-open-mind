---
name: cloudflare
description: Read the live galion.app tunnel and DNS. Use when a name will not open. Never print the token.
---

# Cloudflare

Read only. The token on this box can see the account. It cannot change DNS and it cannot create a hostname.

Account holds 13 zones. The live tunnel is galion-app, healthy. galion-light and voice-l4 are down.

Every public name we checked already points at galion-app, and the tunnel sends all of them to http://127.0.0.1:8080. That includes console, developer, galion.app, galion.studio, and merlinelement.com.

obsidian.galion.app is not in DNS. That is why the name does not open. Adding it needs Zone DNS Edit. This token is DNS Read.

Write attempts with this token fail, and no write path exists on the box:
- `PUT /accounts/{acct}/cfd_tunnel/{id}/configurations` -> 401, code 1001 "Not authorized"
- `POST /zones/{zone}/dns_records` -> 403, code 10000 "Authentication error"
- `GET /user/tokens/verify` -> 401 code 1000 "Invalid API Token" (account-owned token, so verify always fails; reads still work)
Any task that needs a new hostname or an ingress rule is blocked until an operator supplies a token with Zone DNS Edit + Cloudflare Tunnel Write. Do not retry the writes.

R2 is not enabled on the account, so there is no off-site bucket yet.

The token file is /root/galion-secrets/cloudflare.read.env and a copy is in /workspace/galion-light/secrets/persist/. Do not print it.
