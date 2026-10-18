#!/usr/bin/env bash
# MUTATING (guarded): start/stop/restart/reload one systemd unit or docker container on the VPS.
# Default is DRY-RUN (shows current state + what would run). Requires --apply AND typing 'yes'.
# Before acting it saves current state + recent logs locally and writes a change record with rollback steps.
# Usage: service-action.sh (--unit NAME | --container NAME) --action restart|start|stop|reload [--apply]
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
KIND=""; NAME=""; ACTION=""; APPLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --unit) KIND=systemd; NAME="$2"; shift 2 ;;
    --container) KIND=docker; NAME="$2"; shift 2 ;;
    --action) ACTION="$2"; shift 2 ;;
    --apply) APPLY=1; shift ;;
    -h|--help) sed -n '2,5p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
[ -n "$KIND" ] && [ -n "$NAME" ] || die "need --unit or --container"
case "$ACTION" in restart|start|stop|reload) ;; *) die "--action must be restart|start|stop|reload";; esac
[ "$KIND" = docker ] && [ "$ACTION" = reload ] && die "reload not supported for containers"
[[ "$NAME" =~ ^[A-Za-z0-9@._:-]+$ ]] || die "invalid name: $NAME"
require_target; ensure_data_dirs

if [ "$KIND" = systemd ]; then
  STATE_CMD="S=''; sudo -n true 2>/dev/null && S='sudo -n'; \$S systemctl status $NAME --no-pager -l | head -n 30; echo; \$S journalctl -u $NAME -n 100 --no-pager -q"
  ACT_CMD="sudo -n systemctl $ACTION $NAME || systemctl $ACTION $NAME"
  VERIFY_CMD="sleep 5; systemctl is-active $NAME; systemctl status $NAME --no-pager -l | head -n 15"
  case "$ACTION" in stop) RB="systemctl start $NAME";; start) RB="systemctl stop $NAME";; *) RB="systemctl status $NAME; journalctl -u $NAME -n 200 (restart has no undo; if it fails to come up, check unit/config changes made just before)";; esac
else
  STATE_CMD="S=''; sudo -n true 2>/dev/null && S='sudo -n'; docker info >/dev/null 2>&1 && S=''; \$S docker inspect -f 'status={{.State.Status}} health={{if .State.Health}}{{.State.Health.Status}}{{end}} image={{.Config.Image}} imageid={{.Image}} restarts={{.RestartCount}}' $NAME; \$S docker logs --tail 100 $NAME 2>&1"
  ACT_CMD="if docker info >/dev/null 2>&1; then docker $ACTION $NAME; else sudo -n docker $ACTION $NAME; fi"
  VERIFY_CMD="sleep 5; (docker inspect -f '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}' $NAME 2>/dev/null || sudo -n docker inspect -f '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}' $NAME)"
  case "$ACTION" in stop) RB="docker start $NAME";; start) RB="docker stop $NAME";; *) RB="docker logs --tail 200 $NAME (restart has no undo; compare image id in pre-state file)";; esac
fi

stamp=$(ts); pre="$VPS_DATA_DIR/changes/$stamp-$KIND-$NAME-pre.txt"
info "current state of $KIND:$NAME"
vps_ssh "$STATE_CMD" | tee "$pre" | head -n 40
echo
echo "PLAN: on $VPS_USER@$VPS_HOST run: $ACT_CMD"
echo "ROLLBACK: $RB"
if [ "$APPLY" != 1 ]; then echo "DRY-RUN only. Re-run with --apply to execute."; exit 0; fi
confirm "About to $ACTION $KIND '$NAME' on $VPS_HOST."
rec="$VPS_DATA_DIR/changes/$stamp-$KIND-$NAME.md"
record_change "$rec" "# $stamp $ACTION $KIND:$NAME on $VPS_USER@$VPS_HOST" "- pre-state: $pre" "- command: $ACT_CMD" "- rollback: $RB"
vps_ssh "$ACT_CMD"; arc=$?
post="$(vps_ssh "$VERIFY_CMD" 2>&1)"
printf '%s\n' "$post"
record_change "$rec" "- exit: $arc" "- post-state:" '```' "$post" '```'
info "change record: $rec"
exit "$arc"
