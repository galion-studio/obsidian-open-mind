#!/usr/bin/env bash
# READ-ONLY log collection from the VPS into data/logs/<ts>/ for triage.
# Collects: journal errors, per-unit journals, docker logs, nginx error logs, dmesg, OOM events.
# Usage: collect-logs.sh [--since "2 hours ago"] [--unit NAME]... [--container NAME]... [--lines 300] [--grep REGEX]
#   With no --unit/--container it takes units/containers matching galleon|obsidian|openmind plus services.list entries.
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
SINCE="2 hours ago"; LINES=300; GREP=""; UNITS=(); CTRS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --since) SINCE="$2"; shift 2 ;;
    --unit) UNITS+=("$2"); shift 2 ;;
    --container) CTRS+=("$2"); shift 2 ;;
    --lines) LINES="$2"; shift 2 ;;
    --grep) GREP="$2"; shift 2 ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
require_target; ensure_data_dirs
if [ -f "$VPS_SERVICES_LIST" ]; then
  while read -r t n _; do
    case "$t" in systemd) UNITS+=("$n");; docker) CTRS+=("$n");; esac
  done < <(grep -vE '^\s*(#|$)' "$VPS_SERVICES_LIST")
fi
dir="$VPS_DATA_DIR/logs/$(ts)"; mkdir -p "$dir"
info "collecting logs since '$SINCE' -> $dir"

vps_run_remote - "SINCE=$SINCE" "LINES=$LINES" "GREP=$GREP" "UNITS=${UNITS[*]}" "CTRS=${CTRS[*]}" <<'REMOTE' > "$dir/all.txt"
S=""; sudo -n true 2>/dev/null && S="sudo -n"
KW='galleon|obsidian|openmind'
sec() { printf '\n########## %s ##########\n' "$*"; }
filt() { if [ -n "$GREP" ]; then grep -Ei -- "$GREP"; else cat; fi; }
[ -z "$UNITS" ] && UNITS=$($S systemctl list-units --all --no-legend --plain 2>/dev/null | awk '{print $1}' | grep -Ei "$KW" | tr '\n' ' ')
if command -v docker >/dev/null 2>&1; then
  [ -z "$CTRS" ] && CTRS=$($S docker ps -a --format '{{.Names}}' 2>/dev/null | grep -Ei "$KW" | tr '\n' ' ')
fi
sec "journal priority err+ since $SINCE"
$S journalctl -p err --since "$SINCE" --no-pager -q 2>&1 | filt | tail -n "$LINES"
sec "failed units"; $S systemctl --failed --no-pager --plain 2>&1
for u in $UNITS; do
  sec "unit $u status"; $S systemctl status "$u" --no-pager -l 2>&1 | head -n 40
  sec "unit $u journal since $SINCE"; $S journalctl -u "$u" --since "$SINCE" --no-pager -q 2>&1 | filt | tail -n "$LINES"
done
for c in $CTRS; do
  sec "container $c state"; $S docker inspect -f 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} restarts={{.RestartCount}} started={{.State.StartedAt}} error={{.State.Error}}' "$c" 2>&1
  sec "container $c logs since $SINCE"; $S docker logs --since "$(date -d "$SINCE" +%s 2>/dev/null || echo 2h)" --tail "$LINES" "$c" 2>&1 | filt
done
for f in /var/log/nginx/error.log /var/log/caddy/*.log /var/log/apache2/error.log; do
  [ -r "$f" ] || $S test -r "$f" 2>/dev/null || continue
  sec "$f (tail)"; $S tail -n "$LINES" "$f" 2>&1 | filt
done
sec "kernel / OOM"; $S journalctl -k --since "$SINCE" --no-pager -q 2>&1 | grep -Ei 'oom|killed process|segfault|i/o error|ext4|xfs' | tail -n 100
sec "disk & memory snapshot"; df -h -x tmpfs -x devtmpfs; free -h
REMOTE
rc=$?
# split into per-section files for easier reading
awk -v d="$dir" '/^########## /{f=$0; gsub(/^########## | ##########$/,"",f); gsub(/[^A-Za-z0-9._-]+/,"_",f); out=d"/"f".txt"} out{print > out}' "$dir/all.txt"
info "saved $(find "$dir" -type f | wc -l) files in $dir (ssh exit $rc)"
grep -Eic 'error|exception|traceback|fatal|panic|refused|timeout|oom' "$dir/all.txt" | xargs -I{} echo "error-like lines: {}" >&2
exit "$rc"
