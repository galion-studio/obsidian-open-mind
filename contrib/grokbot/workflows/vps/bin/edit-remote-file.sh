#!/usr/bin/env bash
# MUTATING (guarded): replace one file on the VPS with a locally edited version.
# Step 1 (read-only):  edit-remote-file.sh --path /etc/x.conf --fetch      -> downloads to data/changes/<ts>-x.conf(.orig)
# Step 2 (edit the local copy yourself)
# Step 3 (mutating):   edit-remote-file.sh --path /etc/x.conf --from LOCALFILE [--validate "nginx -t"] --apply
# Apply: shows diff, asks 'yes', makes remote backup <path>.bak-<ts>, uploads, runs --validate;
# if validation fails it restores the backup automatically. Rollback command is recorded locally.
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
RPATH=""; FROM=""; FETCH=0; APPLY=0; VALIDATE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --path) RPATH="$2"; shift 2 ;;
    --from) FROM="$2"; shift 2 ;;
    --fetch) FETCH=1; shift ;;
    --validate) VALIDATE="$2"; shift 2 ;;
    --apply) APPLY=1; shift ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
[ -n "$RPATH" ] || die "need --path"
[[ "$RPATH" =~ ^[A-Za-z0-9/._~-]+$ ]] || die "unsupported characters in path"
require_target; ensure_data_dirs
stamp=$(ts); base="$VPS_DATA_DIR/changes/$stamp-$(basename "$RPATH")"
SUDO_CAT="cat $RPATH 2>/dev/null || sudo -n cat $RPATH"
if [ "$FETCH" = 1 ]; then
  vps_ssh "$SUDO_CAT" > "$base.orig" || die "could not read $RPATH"
  cp "$base.orig" "$base"
  echo "fetched: $base.orig (pristine) and $base (edit this), then run with --from $base --apply"
  exit 0
fi
[ -f "$FROM" ] || die "need --from LOCALFILE (or use --fetch first)"
vps_ssh "$SUDO_CAT" > "$base.current" || die "could not read current $RPATH"
diff -u "$base.current" "$FROM" && { echo "no differences; nothing to do"; exit 0; }
echo "PLAN: backup $RPATH -> $RPATH.bak-$stamp, upload $FROM, validate: ${VALIDATE:-<none>}"
echo "ROLLBACK: ssh slavik-vps 'sudo cp -a $RPATH.bak-$stamp $RPATH' (local copy of original: $base.current)"
[ "$APPLY" = 1 ] || { echo "DRY-RUN only. Re-run with --apply."; exit 0; }
confirm "About to overwrite $RPATH on $VPS_HOST."
rec="$base.md"
record_change "$rec" "# $stamp edit $RPATH on $VPS_USER@$VPS_HOST" "- original (local): $base.current" "- new: $FROM" "- remote backup: $RPATH.bak-$stamp" "- rollback: sudo cp -a $RPATH.bak-$stamp $RPATH" "- validate: ${VALIDATE:-none}"
vps_run_remote - "RP=$RPATH" "BK=$RPATH.bak-$stamp" "VAL=$VALIDATE" "B64=$(base64 -w0 "$FROM")" <<'REMOTE'
set -e
S=""; [ -w "$RP" ] || S="sudo -n"
$S cp -a "$RP" "$BK"
echo "$B64" | base64 -d | $S tee "$RP.new-tmp" >/dev/null
$S chown --reference="$RP" "$RP.new-tmp"; $S chmod --reference="$RP" "$RP.new-tmp"
$S mv "$RP.new-tmp" "$RP"
echo "written $RP (backup $BK)"
if [ -n "$VAL" ]; then
  if ! sh -c "$VAL"; then echo "VALIDATION FAILED -> restoring backup"; $S cp -a "$BK" "$RP"; exit 3; fi
  echo "validation OK"
fi
REMOTE
rc=$?
record_change "$rec" "- result exit: $rc"
info "change record: $rec (note: reload/restart the affected service separately via service-action.sh)"
exit "$rc"
