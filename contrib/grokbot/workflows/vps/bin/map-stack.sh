#!/usr/bin/env bash
# READ-ONLY discovery of the VPS stack (Galleon, Obsidian, OpenMind, Galleon1 + anything else).
# Runs inspection commands over SSH and saves raw output locally to data/maps/map-<ts>.txt.
# Nothing is written or changed on the VPS. Uses passwordless sudo only if available (sudo -n).
# Usage: map-stack.sh [--keywords "galleon|obsidian|openmind"]
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
KEYWORDS="galleon|obsidian|openmind"
while [ $# -gt 0 ]; do
  case "$1" in
    --keywords) KEYWORDS="$2"; shift 2 ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
require_target; ensure_data_dirs
out="$VPS_DATA_DIR/maps/map-$(ts).txt"
info "mapping $VPS_USER@$VPS_HOST -> $out"

vps_run_remote - "KW=$KEYWORDS" <<'REMOTE' | tee "$out"
S=""; sudo -n true 2>/dev/null && S="sudo -n"
sec() { printf '\n########## %s ##########\n' "$*"; }
run() { printf '\n$ %s\n' "$*"; eval "$*" 2>&1 | head -n "${LIMIT:-300}"; }
sec "HOST"
run 'whoami; id; hostname; uname -a; uptime'
run 'cat /etc/os-release'
run 'echo "sudo: ${S:-none (non-root view, some output may be partial)}"'
run 'nproc; free -h; df -hT -x tmpfs -x devtmpfs -x overlay'
sec "KEYWORD HITS ($KW)"
run "$S systemctl list-units --all --no-pager --plain 2>/dev/null | grep -Ei '$KW'"
run "$S systemctl list-unit-files --no-pager 2>/dev/null | grep -Ei '$KW'"
run "ps -eo pid,user,etime,cmd --sort=pid | grep -Ei '$KW' | grep -v grep"
printf '\n$ find / -xdev -maxdepth 4 (names matching %s)\n' "$KW"
fargs=(); IFS='|' read -ra kws <<<"$KW"
for k in "${kws[@]}"; do [ ${#fargs[@]} -gt 0 ] && fargs+=(-o); fargs+=(-iname "*$k*"); done
$S find / -xdev -maxdepth 4 \( -path /proc -o -path /sys -o -path /usr/share -o -path /usr/lib -o -path /var/lib/docker \) -prune -o \( "${fargs[@]}" \) -print 2>/dev/null | head -n 200
sec "SYSTEMD"
run "$S systemctl list-units --type=service --state=running --no-pager --plain"
run "$S systemctl --failed --no-pager --plain"
run "$S systemctl list-timers --all --no-pager"
run "ls -1 /etc/systemd/system/ /etc/systemd/system/*.wants/ 2>/dev/null"
run "ls -1 ~/.config/systemd/user 2>/dev/null; systemctl --user list-units --type=service --no-pager --plain 2>/dev/null"
sec "LISTENING PORTS"
run "$S ss -tlnp"
run "$S ss -ulnp"
sec "DOCKER / PODMAN"
run "command -v docker podman docker-compose"
run "$S docker ps -a --format 'table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}'"
run "$S docker compose ls 2>/dev/null"
run "$S docker network ls; $S docker volume ls"
run "for c in \$($S docker ps -aq 2>/dev/null); do $S docker inspect --format '{{.Name}} restart={{.HostConfig.RestartPolicy.Name}} compose_dir={{index .Config.Labels \"com.docker.compose.project.working_dir\"}} mounts={{range .Mounts}}{{.Source}}:{{.Destination}} {{end}}' \$c; done"
run "podman ps -a 2>/dev/null"
sec "COMPOSE FILES"
run "$S find /opt /srv /var/www /home /root /etc -maxdepth 5 \\( -name 'docker-compose*.y*ml' -o -name 'compose*.y*ml' \\) 2>/dev/null"
sec "APP DIRECTORIES"
run "ls -la /opt /srv /var/www /home 2>/dev/null"
run "$S find /opt /srv /var/www /home -maxdepth 3 \\( -name .git -o -name package.json -o -name requirements.txt -o -name pyproject.toml -o -name Dockerfile -o -name .env -o -name 'ecosystem.config.*' -o -name go.mod -o -name Cargo.toml \\) 2>/dev/null"
run "for d in \$($S find /opt /srv /var/www /home -maxdepth 3 -name .git -type d 2>/dev/null); do r=\${d%/.git}; echo \"\$r: \$(git -C \$r rev-parse --abbrev-ref HEAD 2>/dev/null) \$(git -C \$r log -1 --format='%h %ci %s' 2>/dev/null) remote=\$(git -C \$r remote get-url origin 2>/dev/null)\"; done"
sec "PROCESS MANAGERS / RUNTIMES"
run "command -v pm2 node python3 java go supervisorctl gunicorn uvicorn"
run "pm2 ls 2>/dev/null; $S supervisorctl status 2>/dev/null"
sec "WEB SERVER / REVERSE PROXY"
run "command -v nginx apache2 caddy traefik haproxy"
run "ls -la /etc/nginx/sites-enabled /etc/nginx/conf.d 2>/dev/null"
run "$S grep -RhnE '^\\s*(server_name|listen|proxy_pass|root)\\b' /etc/nginx/sites-enabled /etc/nginx/conf.d 2>/dev/null"
run "ls /etc/caddy 2>/dev/null && $S cat /etc/caddy/Caddyfile 2>/dev/null"
run "ls /etc/letsencrypt/live 2>/dev/null; $S certbot certificates 2>/dev/null | grep -E 'Name|Domains|Expiry'"
sec "DATABASES"
run "$S systemctl list-units --type=service --no-pager --plain | grep -Ei 'postgres|mysql|maria|mongo|redis|sqlite|qdrant|chroma|elastic|opensearch|clickhouse'"
sec "SCHEDULED JOBS"
run "crontab -l 2>/dev/null; $S ls -la /etc/cron.d 2>/dev/null"
sec "FIREWALL"
run "$S ufw status verbose 2>/dev/null || $S nft list ruleset 2>/dev/null | head -n 80"
sec "RECENT ERRORS (24h, priority err+)"
LIMIT=80 run "$S journalctl -p err --since '24 hours ago' --no-pager -q | tail -n 80"
echo; echo "# end of map"
REMOTE
rc=${PIPESTATUS[0]}
info "saved $out (exit $rc). Next: transcribe facts into $VPS_STACK_MAP and $VPS_SERVICES_LIST"
exit "$rc"
