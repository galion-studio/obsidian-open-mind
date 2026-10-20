<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# SEC-1 (part 1): key rotation runbook, for the owner, Oct 21 to Oct 31

Written 2026-10-20T20:44:00+02:00 (Europe/Zagreb) on `swarm/sec-runbooks`
(slot 26). **Prepared, not executed.** The swarm rotated nothing, read no
secret value, and changed nothing on the VPS. CEST ends 2026-10-25 03:00;
after that, timestamps carry +01:00.

Sources: the redacted inventory in `stack/` (`*.redacted` files carry the
variable **names** only, values are `<REDACTED:NAME>` markers), `stack/INDEX.md`
(paths), and read-only VPS probes on 2026-10-20 20:28 to 20:42 +02:00
(`ls -l`, `stat`, `systemctl show/status`; no file contents). Out of scope by
rule: the **galion-control key** (not touched, not listed here) and the
**permissions of `secrets/persist`** (owner said on 2026-10-18 22:35 not to
change them yet).

## Ground rules for every rotation

1. **New first, old last.** Create the new credential, deploy it to every
   consumer, prove the service works with it, and only then revoke the old
   one. The one exception is a credential that is *known leaked*
   (rows marked **L**): revoke right after the new one is live, same session.
2. One credential per step; check the consumer's health after each step
   (the probes in the last column). Never two restarts at once.
3. Never paste a value into a chat, a ticket, a commit or a shell history:
   use `read -rs V` + `printf '%s' "$V" > file` or an editor, `umask 077`.
4. Every copy counts. A secret on this VPS lives in up to four places:
   `/root/galion-secrets/<name>` (live), `/opt/galion/galion-light/secrets/persist/`
   (plaintext duplicates; perms unchanged by owner decision), the sealed
   `*.sealed` / `persist-backup-*.tar.age` bundles, and `/var/backups/galion`
   snapshots. After rotating, re-create the sealed bundle and delete the old
   bundle and old snapshots that hold the old value (`galion-boot secrets-restore`
   can read the new bundle; see `docs/ws-survival.md`).
5. Restarting a unit is the owner's action. Commands below are written out
   so they can be copied, but nothing here has been run.

## Inventory: what to rotate, where it is used

Priority: **P0** = leaked or exposed now, rotate first (Oct 21 to 22);
**P1** = live production secrets (Oct 22 to 26); **P2** = dormant (from the
old grok-build backend; rotate or retire) (by Oct 29); **P3** = public by
design, no rotation needed (confirm only).

| # | Credential (name in inventory) | Live location (path only) | Consumers (running units on 2026-10-20) | Prio | How to rotate without an outage | Check after |
|---|---|---|---|---|---|---|
| 1 | GitHub token `github.token` (also the copy in `galion-light/secrets/persist`, which INFRASTRUCTURE.md calls **leaked**) **L** | `/root/galion-secrets/github.token` | swarm push helper (`/workspace/swarm/remote-push.sh` runs on the VPS with it), any `git push` from the VPS | P0 | GitHub, Settings, Developer settings: create a **fine-grained** PAT, only `galion-studio/*` repos needed, Contents read/write, 30-day expiry. Write it to the live file, run `git ls-remote https://github.com/galion-studio/obsidian-open-mind` through the helper, then revoke the old token. Do it after the swarm's last push of the day (pushes fail between steps otherwise). | helper `ls-remote` ok; old token shows *revoked* |
| 2 | GitHub SSH key `github_ed25519` + the copy `from-grok-build/workspace/.ssh/id_ed25519` (SEC-4) | `/root/galion-secrets/github_ed25519` | git over ssh from the VPS (if used; `persist-boot.sh` used to copy it to `/root/.ssh`) | P0 | `ssh-keygen -t ed25519 -f /root/galion-secrets/github_ed25519.new -C vps-$(date +%Y%m)` ; add the `.pub` on GitHub (deploy key or account key); test `ssh -i ... -T git@github.com`; swap files; delete the old key on GitHub; then remove the snapshot copy (SEC-4). | `ssh -T` greets the account |
| 3 | Cloudflare named-tunnel credential (`TunnelSecret`, tunnel `b2c049f7-...`) and the tunnel token that leaked into `galion-light/logs/gateway-deaths.log` via `ps` **L** | `/root/.cloudflared/b2c049f7-7d08-4ec9-a7bc-eecfa76484a6.json` (0600) | `cloudflared.service` (`--config /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`, locally managed) serves every public hostname | P0 | Locally-managed tunnel credentials do not expire and have no in-place refresh. Cloudflare's documented options: (a) set a new `tunnel_secret` with `PATCH /accounts/{acct}/cfd_tunnel/{id}` (API token with *Cloudflare Tunnel Write*), write the new credentials JSON, restart cloudflared, then `DELETE .../connections` to drop old connectors; or (b) `cloudflared tunnel create galion-2`, add the same ingress to a copy of config.yml, start a second unit with the new tunnel, move each DNS CNAME to `<new-id>.cfargotunnel.com` one by one, then stop and `cloudflared tunnel delete` the old one. (b) has zero downtime, (a) has a restart of a few seconds. Then rotate the tunnel token copy in the boot env file (`galion-boot tunnel-env`). | every public hostname answers 200; `cloudflared tunnel info` shows only new connectors |
| 4 | Cloudflare API token `CLOUDFLARE_API_TOKEN`, `R2_ACCESS_KEY_ID` / `R2_SECRET_ACCESS_KEY` (`cloudflare.read.env`) | `/root/galion-secrets/cloudflare.read.env` | console (`/opt/galion/console-ide`, `src/lib/cloudflare.ts`), backups to R2 if configured | P1 | Dashboard: My Profile, API Tokens: *Roll* the token (Cloudflare shows the new value once; the old one stops at once, so update the file within the same minute) or create a new token with the same permissions, swap, delete old. R2: create a new API token pair, swap, delete old. | console Cloudflare panel loads; one R2 list call |
| 5 | OpenRouter key `OPENROUTER_API_KEY` (hermes.env, hermes `.env`; also in the dormant backend env) | `/root/galion-secrets/hermes.env`, `/opt/galion/hermes/.env` (EnvironmentFile of `hermes-rs`), `hermes.env.sealed` | `hermes-rs.service`, arena homes (credential pool fingerprints in `auth.json`) | P1 | openrouter.ai, Keys: create a new key **with the same credit limit**, put it in both env files, re-seal `hermes.env.sealed` with the current `vault.key` (row 9), restart `hermes-rs` (one unit), one cheap chat call, then delete the old key. Spend cap stays as in TEACHER/BOSS.md. | hermes-rs `/health` 200 and one completion |
| 6 | `API_SERVER_KEY` (Hermes gateway client key) | same two env files | server: `hermes-rs`; clients: `console` (`src/lib/hermes.ts`), `galion-one` (reads Hermes), desk | P1 | Client and server must change together (one key, no overlap support known). Edit server env and client configs first, then restart in this order inside one minute: `hermes-rs`, `galion-one`, `console`. Generate with `head -c 32 /dev/urandom | base64` (no openssl needed). | console Hermes panel answers; galion-one `/health` 200 |
| 7 | `RIVER_API_KEY` | same env files | hermes-rs (River provider) | P1 | Provider dashboard: new key, swap, restart hermes-rs with row 5, revoke old. | provider call ok |
| 8 | Memory gateway keys `TDAI_GATEWAY_API_KEY`, `TDAI_LLM_API_KEY`, `MEMORY_TENCENTDB_LLM_API_KEY` | `/etc/memory-gateway.env`; `/etc/openmind-memory.env` (EnvironmentFile of `openmind-memory`, contents not read) | `openmind-memory.service` (running); `memory-gateway` (stopped) | P1 | Gateway key: random new value in both the server env and its clients, restart `openmind-memory`. LLM key: provider dashboard, swap, revoke. If the LLM key is an OpenRouter key, do it together with row 5. | `openmind-memory` health 200 |
| 9 | Sealing keys `vault.key`, `hermes.key`, the age identity `persist-backup.agekey` | `/root/galion-secrets/` | keep.sh / galion-boot secrets-restore, `.env.sealed` | P1 | Last in the P1 block: decrypt each sealed file with the old key into a 0700 tmpfs dir, encrypt with a new key, test a dry-run restore (`galion-boot secrets-restore --dry-run`), then shred the old key **and every bundle sealed with it** (old bundles still contain the old, now-revoked values, but also the plaintext of anything not rotated). | dry-run restore lists all names |
| 10 | Console password `console.password`, `defender.pin`, admin device pairing (`admin-device.json`: hashes only) | `/root/galion-secrets/` | `console.service`, defender | P1 | New password and PIN (password manager), restart console once; re-pair the admin device (clears `tokenHashes`). | login works with the new password only |
| 11 | `galion-app.token`, `replicate.key` | `/root/galion-secrets/` | Galion app, Replicate calls (no paid calls by the swarm) | P1 | Provider dashboards: create new, swap, revoke old. | one free/auth-only call |
| 12 | Browser session cookies `playwright.cookie`, `pw-cookie` | `/root/galion-secrets/` | Playwright automations | P1 | Cookies cannot be rotated, only invalidated: log out of every session on the site ("sign out everywhere"), log in again, save the new cookie. | automation runs logged in |
| 13 | VPS root password (seen in a screenshot on 2026-10-18, `docs/session-2026-10-18.md`) | `/etc/shadow` | ssh (key auth is used day to day) | P0 | `passwd root` from an existing ssh session; keep that session open while testing a second login. Optionally `PasswordAuthentication no` after checking that the owner's key works. | second ssh session logs in |
| 14 | Hermes `config.yaml` fields `api_key`, `client_secret`, `sudo_password` (+ `*_token_env` names) | `/opt/galion/hermes/config.yaml` (mode **666**, SEC-3) | hermes-rs / Hermes tools | P1 | Fix SEC-3 perms **first** (the file sits in a 0700 directory, so today only root can reach it, but backups and copies may keep the 666 mode), then rotate each filled field at its provider; `sudo_password` = change that account's password. | hermes-rs health |
| 15 | Dormant grok-build backend env (`galion-backend.env`, `.env.backup`): `JWT_SECRET(_KEY)`, `SECRET_KEY`, `ADMIN_API_KEY`, `DATABASE_URL` password, `DATA_ENCRYPTION_KEY`/`ENCRYPTION_KEY`, `POSTAL_API_KEY`, `CLOUDFLARE_EMAIL_TOKEN`, `ZOHO_IMAP/SMTP_PASSWORD`, `OPENROUTER_API_KEY` | `/root/galion-secrets/from-grok-build/`; worktree copies `/opt/galion/galion-src/galion-backend/.env` and `.env.backup` (0700) | **none running** (no galion-backend unit on 2026-10-20) | P2 | Because nothing runs it, revoke rather than rotate: delete the Postal key, the Cloudflare email token, change the Zoho mailbox passwords (app passwords), drop or re-password the DB user. Do **not** rotate `DATA_ENCRYPTION_KEY`/`ENCRYPTION_KEY` blindly: if any database still holds data encrypted with it, decrypt/re-encrypt first or the data is lost. JWT/SECRET_KEY: new random values only matter if the backend is ever started again. | provider dashboards show old keys revoked |
| 16 | `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET` (tactical-belts) | `/root/galion-secrets/from-grok-build/tactical-belts.env`; worktree `/opt/galion/galion-src/tactical-belts-pro/.env.local` (0600) | the tactical-belts site wherever it is deployed (not on this VPS) | P1 (real money surface) | Stripe Dashboard, Developers, API keys: **Roll key** with an expiry overlap (e.g. 24 h), deploy the new key to the site host, then let the old one expire. Webhook: roll the endpoint signing secret (Stripe allows the old secret to stay valid for up to 24 h). The swarm makes no Stripe calls (no-real-money rule). | Stripe shows requests with the new key only |
| 17 | `VITE_SUPABASE_PUBLISHABLE_KEY`, `VITE_SHOPIFY_STOREFRONT_TOKEN`, `VITE_CLOUDFLARE_TURNSTILE_SITE_KEY` (magic-merlin, merlin-element) | `from-grok-build/*.env`; worktree `.env` in both repos (0600) | public front-ends | P3 | These ship to browsers by design. Confirm the Supabase key is the *publishable/anon* key (row-level security on) and the Shopify token is a *Storefront* token; rotate only if a service-role key is found. | n/a |
| 18 | Box SSH key `~/.ssh/id_ed25519_vps` (Grok Bot box to VPS root) | box only; public half in `/root/.ssh/authorized_keys` | swarm push helper, read-only probes | keep until Oct 31 | Leave it until the swarm is done (it is how pushes reach GitHub), then remove the line from `authorized_keys` or replace it with a restricted key (`command=`/`from=` options). | n/a |

Not rotated here: the galion-control key (rule 2 of the swarm rules); keys
that live only on the owner's devices.

## Order that never breaks live services

The running units on 2026-10-20 are `cloudflared`, `console`, `galion-desk`,
`galion-health`, `galion-nerve`, `galion-one`, `hermes-rs`, `link-watch-rs`,
`openmind-memory`, `tanga-proto`, `tuner`, `tuner-quick` (all `User=` root).

| Day (Zagreb) | Steps | Why this order |
|---|---|---|
| Oct 21 (Wed) | 13 root password; 1 GitHub token (after the last swarm push); 2 GitHub SSH key | no service restarts; removes the two known-leaked account credentials first |
| Oct 22 (Thu) | 3 tunnel (option b: new tunnel in parallel, move CNAMEs one by one, delete old; or option a in a quiet hour) | the only public-traffic step; done alone so any outage is attributable |
| Oct 22 to 23 | SEC-3 script (perms) then 14 config.yaml fields; 5 OpenRouter + 7 River (one hermes-rs restart); 6 API_SERVER_KEY (hermes-rs, galion-one, console in one minute) | each step restarts at most the Hermes chain once |
| Oct 23 to 24 | 8 memory keys (openmind-memory restart); 4 Cloudflare API/R2; 10 console password/PIN/device; 11; 12 | independent consumers, one restart each |
| Oct 24 to 26 | 9 sealing keys: re-seal everything with new keys, dry-run restore, destroy old keys and the bundles/snapshots sealed with them | must come after all values above are new, otherwise the new bundle would preserve old values |
| Oct 26 to 29 | 15 revoke dormant backend creds; 16 Stripe roll with overlap; 17 confirm public keys | not on this VPS's live path |
| Oct 27 to 30 | SEC-1 part 2: history purge (`docs/launch/SEC-1-HISTORY-PURGE.md`), *after* rotation, so anything still in an old clone is already dead | rotation makes the purge a hygiene step instead of the only protection |
| Oct 31 | 18 box key after the swarm stops; LAUNCH-1/2 | |

Rollback for every row: until the old credential is revoked, putting the
old file back and restarting the one unit restores the previous state. That
is why revocation is always the last action of a row.

## What the swarm checked (read-only, 2026-10-20 20:28 to 20:42 +02:00)

- `/root/.cloudflared/b2c049f7-...json` exists, mode 0600 (`ls -l`).
- `hermes-rs` EnvironmentFile `/opt/galion/hermes/.env` (0600); `openmind-memory`
  EnvironmentFile `/etc/openmind-memory.env`; `cloudflared` runs with
  `--config` (locally managed), no `--token` on its command line.
- `/opt/galion/galion-src` (28 clones, 366 MB): worktree files with
  secret-shaped names: `galion-backend/.env` + `.env.backup` (0700),
  `magic-merlin-showcase/.env` (0600), `merlin-element/.env` (0600),
  `tactical-belts-pro/.env.local` (0600); `.env.example` files in
  galion-console and galion-rs (0777) and tactical-belts-pro (0666) should get
  `chmod 644` (templates, but world-writable). Whether the real `.env` files
  are *tracked* (O1) was not checked: that needs `git` inside the clones,
  which the swarm may not run on the VPS. Owner command (read-only, prints
  paths only):

  ```
  for r in /opt/galion/galion-src/*/; do
    git -C "$r" log --all --format= --name-only --diff-filter=A 2>/dev/null \
      | grep -E '(^|/)(\.env(\..*)?|.*\.pem|.*\.key|id_(rsa|ed25519)|credentials.*\.json)$' \
      | grep -v '\.example$' | sort -u | sed "s#^#$(basename "$r"): #"
  done
  ```
