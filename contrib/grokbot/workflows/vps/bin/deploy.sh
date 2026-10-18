#!/usr/bin/env bash
# MUTATING (guarded): deploy an update for one component in a remote directory.
# Default is DRY-RUN: shows current revision, dirty state, remote target, compose status. No changes.
# --apply (plus typing 'yes'): pre-health-check -> backup (tarball + git SHA + compose image IDs) ->
#   update (git and/or compose) -> optional unit restart -> post-health-check. Rollback steps recorded locally.
# Usage:
#   deploy.sh --component NAME --dir /remote/path --method git|compose|git+compose
#             [--ref BRANCH_OR_TAG] [--build] [--unit SYSTEMD_UNIT] [--allow-dirty] [--no-tarball] [--apply]
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
COMP=""; DIR=""; METHOD=""; REF=""; BUILD=0; UNIT=""; DIRTY=0; TARBALL=1; APPLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --component) COMP="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    --method) METHOD="$2"; shift 2 ;;
    --ref) REF="$2"; shift 2 ;;
    --build) BUILD=1; shift ;;
    --unit) UNIT="$2"; shift 2 ;;
    --allow-dirty) DIRTY=1; shift ;;
    --no-tarball) TARBALL=0; shift ;;
    --apply) APPLY=1; shift ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
[ -n "$COMP" ] && [ -n "$DIR" ] || die "need --component and --dir (take them from stack-map.md)"
case "$METHOD" in git|compose|git+compose) ;; *) die "--method must be git|compose|git+compose";; esac
for v in "$COMP" "$DIR" "$REF" "$UNIT"; do [[ -z "$v" || "$v" =~ ^[A-Za-z0-9/._@:~-]+$ ]] || die "invalid characters in: $v"; done
require_target; ensure_data_dirs
stamp=$(ts); HERE="$(dirname "$0")"

info "inspecting $COMP at $DIR (read-only)"
pre="$VPS_DATA_DIR/changes/$stamp-deploy-$COMP-pre.txt"
vps_run_remote - "D=$DIR" "M=$METHOD" "REF=$REF" <<'REMOTE' | tee "$pre"
S=""; [ -r "$D" ] && [ -x "$D" ] || S="sudo -n"
cd "$D" 2>/dev/null || $S test -d "$D" || { echo "DIR_MISSING $D"; exit 4; }
case "$M" in *git*)
  echo "branch: $(git -C "$D" rev-parse --abbrev-ref HEAD 2>&1)"
  echo "head:   $(git -C "$D" log -1 --format='%H %ci %s' 2>&1)"
  echo "dirty:  $(git -C "$D" status --porcelain 2>/dev/null | wc -l) files"
  git -C "$D" status --short 2>/dev/null | head -n 20
  r=$(git -C "$D" remote get-url origin 2>/dev/null); echo "origin: $r"
  [ -n "$r" ] && echo "remote refs: $(git -C "$D" ls-remote origin ${REF:-HEAD} 2>&1 | head -n 3 | tr '\n' ' ')"
;; esac
case "$M" in *compose*)
  D2="docker"; docker info >/dev/null 2>&1 || D2="sudo -n docker"
  ls "$D"/docker-compose*.y*ml "$D"/compose*.y*ml 2>/dev/null
  (cd "$D" && $D2 compose ps 2>&1 && $D2 compose images 2>&1)
;; esac
du -sh "$D" 2>/dev/null | awk '{print "size: "$1}'
REMOTE
prc=${PIPESTATUS[0]}
[ "$prc" -eq 0 ] || die "pre-inspection failed (exit $prc); fix before deploying"
if [ "$DIRTY" = 0 ] && grep -qE '^dirty: +[1-9]' "$pre"; then
  warn "working tree has local changes on the VPS (Slavik may have hot-fixed). Review them; pass --allow-dirty only if intended."
  [ "$APPLY" = 1 ] && die "refusing to deploy over a dirty tree"
fi
BK="$VPS_REMOTE_BACKUP_DIR/$COMP-$stamp"
echo
echo "PLAN ($COMP):"
echo "  1. health-check.sh (pre)"
echo "  2. backup to $BK on VPS: git SHA, compose image IDs$( [ "$TARBALL" = 1 ] && echo ', tarball of dir (excl .git, node_modules, venv)')"
case "$METHOD" in *git*) echo "  3. git fetch --all --tags && ${REF:+git checkout $REF && }git pull --ff-only (if on a branch)";; esac
case "$METHOD" in *compose*) echo "  4. docker compose pull && docker compose up -d$( [ "$BUILD" = 1 ] && echo ' --build') --remove-orphans=false";; esac
[ -n "$UNIT" ] && echo "  5. systemctl restart $UNIT"
echo "  6. health-check.sh (post)"
[ "$APPLY" = 1 ] || { echo "DRY-RUN only. Re-run with --apply to deploy."; exit 0; }

confirm "About to deploy $COMP in $DIR on $VPS_HOST."
rec="$VPS_DATA_DIR/changes/$stamp-deploy-$COMP.md"
record_change "$rec" "# $stamp deploy $COMP ($METHOD) on $VPS_USER@$VPS_HOST:$DIR" "- pre-inspection: $pre" "- remote backup dir: $BK"
info "pre-deploy health check"
"$HERE/health-check.sh" --quiet > "$VPS_DATA_DIR/changes/$stamp-deploy-$COMP-health-pre.txt" 2>&1; hpre=$?
record_change "$rec" "- pre health exit: $hpre (0 ok,1 warn,2 fail)"
[ "$hpre" -ge 2 ] && warn "VPS already has FAIL checks before deploy (see health-pre file)"

vps_run_remote - "D=$DIR" "M=$METHOD" "REF=$REF" "BUILD=$BUILD" "UNIT=$UNIT" "BK=$BK" "TARBALL=$TARBALL" <<'REMOTE' 2>&1 | tee -a "$rec.log"
set -euo pipefail
S=""; [ -w "$D" ] || S="sudo -n"
DK="docker"; docker info >/dev/null 2>&1 || DK="sudo -n docker"
BK=$(eval echo "$BK"); mkdir -p "$BK"
echo "== backup -> $BK"
case "$M" in *git*) git -C "$D" rev-parse HEAD > "$BK/git-head.txt"; git -C "$D" rev-parse --abbrev-ref HEAD > "$BK/git-branch.txt"; git -C "$D" diff > "$BK/uncommitted.diff" || true; echo "git head $(cat "$BK/git-head.txt")";; esac
case "$M" in *compose*) (cd "$D" && $DK compose images --format json > "$BK/compose-images.json" 2>/dev/null || $DK compose images > "$BK/compose-images.txt"; $DK compose config > "$BK/compose-config.yml" 2>/dev/null || true);; esac
if [ "$TARBALL" = 1 ]; then $S tar czf "$BK/dir.tgz" --exclude=.git --exclude=node_modules --exclude=venv --exclude=.venv -C "$(dirname "$D")" "$(basename "$D")"; ls -lh "$BK/dir.tgz"; fi
case "$M" in *git*)
  echo "== git update"
  cd "$D"; $S git fetch --all --tags --prune
  [ -n "$REF" ] && $S git checkout "$REF"
  if git symbolic-ref -q HEAD >/dev/null; then $S git pull --ff-only; fi
  echo "now at $(git log -1 --format='%H %s')"
;; esac
case "$M" in *compose*)
  echo "== compose update"
  cd "$D"; $DK compose pull
  if [ "$BUILD" = 1 ]; then $DK compose up -d --build; else $DK compose up -d; fi
  $DK compose ps
;; esac
if [ -n "$UNIT" ]; then echo "== restart $UNIT"; sudo -n systemctl restart "$UNIT"; sleep 5; systemctl is-active "$UNIT"; fi
echo "== deploy steps done"
REMOTE
drc=${PIPESTATUS[0]}
record_change "$rec" "- deploy exit: $drc (full log: $rec.log)" \
  "## Rollback" \
  "1. ssh slavik-vps" \
  "2. git:     cd $DIR && git checkout \$(cat $BK/git-head.txt)   # then: git checkout \$(cat $BK/git-branch.txt) && git reset --hard <sha> only if you are sure" \
  "3. compose: cd $DIR && docker compose up -d   # images: see $BK/compose-images.*; pin old image tag/ID in compose if pull changed it" \
  "4. files:   tar xzf $BK/dir.tgz -C $(dirname "$DIR")   # if tarball was taken; restores working files" \
  "5. unit:    ${UNIT:+sudo systemctl restart $UNIT}" \
  "6. verify:  ~/workflows/vps/bin/health-check.sh"
info "post-deploy health check"
"$HERE/health-check.sh" --quiet > "$VPS_DATA_DIR/changes/$stamp-deploy-$COMP-health-post.txt" 2>&1; hpost=$?
record_change "$rec" "- post health exit: $hpost"
cat "$VPS_DATA_DIR/changes/$stamp-deploy-$COMP-health-post.txt"
info "change record with rollback steps: $rec"
[ "$drc" -ne 0 ] && { warn "deploy step failed (exit $drc). Consider rollback per $rec"; exit "$drc"; }
[ "$hpost" -gt "$hpre" ] && { warn "health got worse after deploy ($hpre -> $hpost). Consider rollback."; exit 3; }
exit 0
