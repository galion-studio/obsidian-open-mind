<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: tanga-proto (tanga-serve, 127.0.0.1:8671, one.galion.app/tanga/)

- Status: **NOT CUT OVER** (no replacement planned). The first install of 2026-10-18 22:35:49 CEST (+02:00) runs unchanged.
- **Approved: Yes, for the 2026-10-18 install only**: "Owner-approved scope change", recorded in the box file `/workspace/galion-one-design/DEPLOY.md` (not in a repo; not in CALL-RECORD, which starts 2026-10-20).
  No cutover is proposed or approved.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from that DEPLOY.md and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/tanga-proto.service` (547 B, sha256 `45c84e4262d7377280b2809d0aec8bc7c200e6390f5a29f640a1006e6fff0cef`, 2026-10-18 22:35), enabled, active, no drop-ins, `DynamicUser=yes`, `CPUQuota=10%`, `MemoryMax=32M`, `TasksMax=64`, `Nice=10`, `Restart=on-failure` 5 s |
| exe | `/opt/galion/tanga-proto/bin/tanga-serve`, 447,264 B, sha256 `9c540eeb054dd1205666f116167b9ad8ba7edf08f900ce4228a79d210a1de7f7`, RSS 0.2 MB |
| source | **proven by bytes**: the box build `/workspace/galion-one-design/tanga-serve/target/x86_64-unknown-linux-musl/release/tanga-serve` (2026-10-18 22:35) has the same sha256. The source is `/workspace/galion-one-design/tanga-serve/` (std-only Rust, GET/HEAD, strips `/tanga`). That folder is **not a git repo** and is in no swarm branch |
| content | static mock page `/opt/galion/tanga-proto/tanga/index.html`; response headers `X-Robots-Tag: noindex, nofollow` and a `Content-Security-Policy` (checked 17:23) |
| ingress | cloudflared rule `one.galion.app` + `path: ^/tanga(/.*)?$` → `http://127.0.0.1:8671`, placed before the `one.galion.app` rule |
| NRestarts 3 | all at first start, 2026-10-18 22:34:25–22:34:35 (exit 1, three failed starts during the install), then started 22:35:49 and stable since |

## 2. Verify (on the VPS, GET only)

```
systemctl show tanga-proto -p ActiveState -p NRestarts
sha256sum /proc/$(systemctl show tanga-proto -p MainPID --value)/exe      # 9c540eeb…a1de7f7
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8671/tanga/ # 200
```

At 17:22: `/tanga/` 200 (41,547 B). The nerve has **no** component for tanga-proto (plans/nerve.md T2.3), and galion-health does not check it either.

## 3. Rollback / removal (from DEPLOY.md, for the 2026-10-18 install; not rehearsed)

```
BK=/root/galion-switch-20261018-2140/tanga-20261018-223412
F=/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml
cat $BK/config.yml.orig > $F
cloudflared tunnel --config $F ingress validate && systemctl restart cloudflared
systemctl disable --now tanga-proto && rm /etc/systemd/system/tanga-proto.service && systemctl daemon-reload
rm -rf /opt/galion/tanga-proto      # optional
```

**Warning:** the first three lines are stale. `config.yml.orig` is the 2026-10-18 22:34 config, so writing it back would also undo the 2026-10-21 ingress changes
(C8 galion.app/www → 8091 and code.galion.app removal, and the forge.galion.studio rule of 17:08). To remove tanga today, delete only the `/tanga` rule from the current config
(backup first, then `ingress validate`, then restart cloudflared), then run the `systemctl` line. That needs an owner decision; nothing was changed here.
The backup folder held `config.yml.new`, `config.yml.orig` and `restart-at` at 17:23.

## 4. Open points

- The source is not in version control. Committing `tanga-serve/` to a swarm branch would make the binary rebuildable from git. This needs an owner decision.
- No monitoring (nerve spec missing).
