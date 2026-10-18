#!/usr/bin/env bash
# READ-ONLY health check of the VPS: generic host checks + checks listed in services.list.
# Prints lines "OK|WARN|FAIL <check> <detail>" and saves them to data/health/health-<ts>.txt.
# Exit code: 0 = all OK, 1 = warnings, 2 = failures, 255 = SSH/connection error.
# Usage: health-check.sh [--quiet] [--disk-warn 85] [--disk-fail 95]
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
QUIET=0; DISK_WARN=85; DISK_FAIL=95
while [ $# -gt 0 ]; do
  case "$1" in
    --quiet) QUIET=1; shift ;;
    --disk-warn) DISK_WARN="$2"; shift 2 ;;
    --disk-fail) DISK_FAIL="$2"; shift 2 ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
require_target; ensure_data_dirs
CHECKS=""
[ -f "$VPS_SERVICES_LIST" ] && CHECKS="$(grep -vE '^\s*(#|$)' "$VPS_SERVICES_LIST" || true)"
[ -z "$CHECKS" ] && warn "services.list has no entries yet; running generic host checks only"
out="$VPS_DATA_DIR/health/health-$(ts).txt"

result="$(vps_run_remote - "CHECKS=$CHECKS" "DW=$DISK_WARN" "DF=$DISK_FAIL" <<'REMOTE'
S=""; sudo -n true 2>/dev/null && S="sudo -n"
r() { echo "$1 $2 $3"; }
# uptime / reboot
r OK uptime "$(uptime -p 2>/dev/null || uptime)"
[ -f /var/run/reboot-required ] && r WARN reboot "reboot-required flag present"
# load
n=$(nproc); l=$(cut -d' ' -f1 /proc/loadavg)
if awk -v l="$l" -v n="$n" 'BEGIN{exit !(l>n*1.5)}'; then r WARN load "load1=$l cpus=$n"; else r OK load "load1=$l cpus=$n"; fi
# memory
read -r tot av < <(awk '/MemTotal/{t=$2}/MemAvailable/{a=$2}END{print t,a}' /proc/meminfo)
pct=$(( av*100/tot ))
if [ "$pct" -lt 5 ]; then r FAIL memory "available=${pct}%"; elif [ "$pct" -lt 10 ]; then r WARN memory "available=${pct}%"; else r OK memory "available=${pct}%"; fi
sw=$(awk '/SwapTotal/{t=$2}/SwapFree/{f=$2}END{if(t>0)print int((t-f)*100/t); else print 0}' /proc/meminfo)
[ "$sw" -gt 50 ] && r WARN swap "used=${sw}%"
# disk
df -P -x tmpfs -x devtmpfs -x overlay -x squashfs 2>/dev/null | awk 'NR>1{gsub("%","",$5); print $5, $6}' | while read -r u m; do
  if [ "$u" -ge "$DF" ]; then r FAIL disk "$m ${u}%"; elif [ "$u" -ge "$DW" ]; then r WARN disk "$m ${u}%"; else r OK disk "$m ${u}%"; fi
done
# inodes
df -Pi -x tmpfs -x devtmpfs -x overlay -x squashfs 2>/dev/null | awk 'NR>1 && $5!="-"{gsub("%","",$5); if($5>=90) print $5, $6}' | while read -r u m; do r WARN inodes "$m ${u}%"; done
# systemd failed units
f=$($S systemctl --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | tr '\n' ' ')
if [ -n "$f" ]; then r FAIL systemd-failed "$f"; else r OK systemd-failed "none"; fi
# docker generic
if command -v docker >/dev/null 2>&1 && $S docker info >/dev/null 2>&1; then
  $S docker ps -a --format '{{.Names}}|{{.Status}}' | while IFS='|' read -r nm st; do
    case "$st" in
      *unhealthy*|Restarting*) r FAIL "container:$nm" "$st" ;;
      Exited*|Dead*|Created*)   r WARN "container:$nm" "$st" ;;
      *)                        r OK   "container:$nm" "$st" ;;
    esac
  done
fi
# journal error volume last hour
e=$($S journalctl -p err --since '1 hour ago' --no-pager -q 2>/dev/null | wc -l)
if [ "$e" -gt 50 ]; then r WARN journal-errors "$e err+ lines in last hour"; else r OK journal-errors "$e err+ lines in last hour"; fi
# TLS certificates (letsencrypt) expiring < 14 days
for c in $($S sh -c 'ls /etc/letsencrypt/live/*/cert.pem' 2>/dev/null); do
  if $S openssl x509 -checkend $((14*86400)) -noout -in "$c" >/dev/null 2>&1; then r OK "cert:$(basename "$(dirname "$c")")" "valid >14d"; else r WARN "cert:$(basename "$(dirname "$c")")" "expires <14d"; fi
done
# explicit checks from services.list
printf '%s\n' "$CHECKS" | while read -r typ tgt label; do
  [ -z "$typ" ] && continue
  name="${label:-$tgt}"
  case "$typ" in
    systemd) st=$($S systemctl is-active "$tgt" 2>/dev/null); [ "$st" = active ] && r OK "svc:$name" "$tgt active" || r FAIL "svc:$name" "$tgt ${st:-unknown}" ;;
    docker)  st=$($S docker inspect -f '{{.State.Status}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}n/a{{end}} restarts={{.RestartCount}}' "$tgt" 2>/dev/null)
             case "$st" in running*unhealthy*|"") r FAIL "ctr:$name" "$tgt ${st:-not found}";; running*) r OK "ctr:$name" "$tgt $st";; *) r FAIL "ctr:$name" "$tgt $st";; esac ;;
    port)    if $S ss -tln 2>/dev/null | awk '{print $4}' | grep -qE "[:.]$tgt\$"; then r OK "port:$name" "$tgt listening"; else r FAIL "port:$name" "$tgt not listening"; fi ;;
    http)    code=$(curl -sk -o /dev/null -m 10 -w '%{http_code}' "$tgt" 2>/dev/null); case "$code" in 2*|3*) r OK "http:$name" "$tgt $code";; *) r FAIL "http:$name" "$tgt ${code:-no-response}";; esac ;;
    path)    [ -e "$tgt" ] && r OK "path:$name" "$tgt exists" || r FAIL "path:$name" "$tgt missing" ;;
    *)       r WARN "config" "unknown check type '$typ' in services.list" ;;
  esac
done
REMOTE
)"
rc=$?
if [ $rc -ne 0 ] && [ -z "$result" ]; then echo "FAIL ssh connection error (rc=$rc)" | tee "$out"; exit 255; fi
printf '%s\n' "$result" > "$out"
if [ "$QUIET" = 1 ]; then grep -E '^(WARN|FAIL)' "$out" || true; else cat "$out"; fi
nf=$(grep -c '^FAIL' "$out" || true); nw=$(grep -c '^WARN' "$out" || true)
info "summary: FAIL=$nf WARN=$nw -> $out"
[ "$nf" -gt 0 ] && exit 2
[ "$nw" -gt 0 ] && exit 1
exit 0
