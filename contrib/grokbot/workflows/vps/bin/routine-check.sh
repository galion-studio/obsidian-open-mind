#!/usr/bin/env bash
# READ-ONLY recurring check: runs health-check.sh, compares with the previous run,
# writes data/health/ALERT-<ts>.txt on new/persisting problems and optionally pipes it to $VPS_ALERT_CMD.
# Designed to be scheduled later (cron/systemd timer) — this script does NOT install any schedule.
# Usage: routine-check.sh [--collect-logs-on-fail] [--always-alert]
# Exit codes mirror health-check.sh (0 ok, 1 warn, 2 fail, 255 ssh error).
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
COLLECT=0; ALWAYS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --collect-logs-on-fail) COLLECT=1; shift ;;
    --always-alert) ALWAYS=1; shift ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
require_target; ensure_data_dirs
HERE="$(dirname "$0")"; H="$VPS_DATA_DIR/health"
prev="$(find "$H" -maxdepth 1 -name 'health-*.txt' -printf '%T@ %p\n' 2>/dev/null | sort -n | tail -n1 | cut -d' ' -f2-)"
"$HERE/health-check.sh" --quiet >/dev/null 2>&1; rc=$?
cur="$(find "$H" -maxdepth 1 -name 'health-*.txt' -printf '%T@ %p\n' | sort -n | tail -n1 | cut -d' ' -f2-)"
problems="$(grep -E '^(WARN|FAIL)' "$cur" 2>/dev/null || true)"
new=""
if [ -n "$prev" ] && [ "$prev" != "$cur" ]; then
  new="$(comm -13 <(grep -E '^(WARN|FAIL)' "$prev" | awk '{print $1,$2}' | sort -u) <(printf '%s\n' "$problems" | awk 'NF{print $1,$2}' | sort -u))"
  recovered="$(comm -23 <(grep -E '^(WARN|FAIL)' "$prev" | awk '{print $1,$2}' | sort -u) <(printf '%s\n' "$problems" | awk 'NF{print $1,$2}' | sort -u))"
else
  new="$(printf '%s\n' "$problems" | awk 'NF{print $1,$2}')"; recovered=""
fi
summary="VPS routine check $(date '+%F %T %Z') host=$VPS_HOST exit=$rc
NEW: ${new:-none}
RECOVERED: ${recovered:-none}
CURRENT PROBLEMS:
${problems:-none}
report: $cur"
echo "$summary"
if [ "$rc" -eq 255 ] || [ -n "$new" ] || { [ "$ALWAYS" = 1 ] && [ "$rc" -ne 0 ]; } || grep -q '^FAIL' <<<"$problems"; then
  af="$H/ALERT-$(ts).txt"; printf '%s\n' "$summary" > "$af"; info "alert written: $af"
  if [ "$COLLECT" = 1 ] && [ "$rc" -ge 2 ]; then "$HERE/collect-logs.sh" --since "1 hour ago" >/dev/null 2>&1 && info "logs collected under $VPS_DATA_DIR/logs/"; fi
  if [ -n "$VPS_ALERT_CMD" ]; then printf '%s\n' "$summary" | sh -c "$VPS_ALERT_CMD" || warn "alert command failed"; fi
fi
exit "$rc"
