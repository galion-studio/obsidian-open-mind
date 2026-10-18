---
name: stack-mapping
description: Use on first connection to Slavik's VPS or whenever the setup may have changed, to discover how Galleon, Galleon1, Obsidian and OpenMind (and anything else) are run, and to record facts in the stack map.
---

# Stack mapping (Galleon / Galleon1 / Obsidian / OpenMind)

## Purpose
Discover — not assume — what runs on the VPS: services, containers, ports, paths, repos, proxy, DBs, schedules. Output: an up-to-date `~/workflows/vps/stack-map.md` and `~/workflows/vps/services.list`.

## Prerequisites
- `vps-ssh-access` passes (`vps-ssh.sh --check`).
- Nothing is known yet: no ports, unit names, or paths. Do not fill anything in from naming guesses.

## Procedure
1. Run the read-only discovery (saves to `data/maps/map-<ts>.txt`):
   `~/workflows/vps/bin/map-stack.sh`
   (Add keywords if names differ: `--keywords "galleon|obsidian|openmind|<other>"`.)
   It covers: host/OS/resources; keyword hits in units, processes, and file names; running services, failed units, timers; `ss -tlnp`/`-ulnp`; `docker ps -a`, `docker compose ls`, container labels/mounts; compose files under /opt /srv /var/www /home /root /etc; app markers (.git, package.json, Dockerfile, .env…) and git HEAD/remotes; pm2/supervisor; nginx/caddy server_name/listen/proxy_pass; certs; DB services; cron; firewall; 24h journal errors.
2. Read the map file. For each component, follow the evidence chain and drill in with targeted read-only commands, e.g.:
   - `vps-ssh.sh -- 'systemctl cat <unit>'` (ExecStart, WorkingDirectory, EnvironmentFile)
   - `vps-ssh.sh -- 'docker inspect <container> --format "{{json .Config.Labels}}"'`
   - `vps-ssh.sh -- 'cat <dir>/docker-compose.yml'` (redact secrets when recording)
   - `vps-ssh.sh -- 'sudo -n ss -tlnp | grep <port>'`, `ps -o pid,cmd -p <pid>`, `readlink /proc/<pid>/cwd`
   - `vps-ssh.sh -- 'grep -Rn "proxy_pass\|server_name" /etc/nginx/sites-enabled'`
3. Fill `~/workflows/vps/stack-map.md` (one table per component). Mark unconfirmed items `UNVERIFIED`; leave unknowns `TBD`. Specifically determine what Galleon1 is relative to Galleon (from evidence, or list as open question).
4. Add one health check per confirmed facet to `~/workflows/vps/services.list` (`systemd`, `docker`, `port`, `http`, `path` lines).
5. Validate: `~/workflows/vps/bin/health-check.sh` should show your new checks OK (or real failures).
6. Add a row to "Change history" in the stack map with the map filename.

## Safety
- Read-only only. Never cat `.env`/secret files into the stack map; record paths, and key *names* at most.
- Large `find` runs are limited by depth/xdev; don't run unbounded `find /` or heavy `du` on a busy box.
- `sudo -n` is used only if passwordless; never enter passwords.

## Report
Per component: how it runs, name, dir, port(s), URL, repo/SHA, logs location, health check added. Plus: unknowns/open questions for Slavik, anomalies (failed units, exposed ports, expiring certs), path to map file.
