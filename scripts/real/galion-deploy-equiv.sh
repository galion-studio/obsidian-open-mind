#!/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Proves the pluggable galion-deploy is unchanged for systemd: runs the original
# 2,830 B script (ws/foundation e43d7ab) and deploy/foundation/galion-deploy side by
# side against shimmed systemctl/journalctl/curl/sleep, inside an unprivileged user
# + mount namespace (/var/backups is a bind-mounted scratch dir; nothing real is
# touched). For every scenario it diffs: stdout+stderr, exit code, the ordered list
# of systemctl/journalctl/curl/sleep calls, the installed binary and the backups.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
HERE=$(cd "$(dirname "$0")/../.." && pwd)
W=$(mktemp -d /tmp/gd-equiv.XXXXXX); trap 'rm -rf "$W"' EXIT
git -C "$HERE" show e43d7ab:deploy/foundation/galion-deploy > "$W/orig" 2>/dev/null \
  || git --git-dir=/workspace/swarm/base.git show e43d7ab:deploy/foundation/galion-deploy > "$W/orig"
cp "$HERE/deploy/foundation/galion-deploy" "$W/new"
chmod +x "$W/orig" "$W/new"
[ "$(wc -c < "$W/orig")" = 2830 ] || { echo "original is not 2,830 B"; exit 1; }

mkdir -p "$W/shim"
cat > "$W/shim/systemctl" <<'S'
#!/bin/bash
echo "systemctl $*" >> "$CALLS"
case "$1" in
  cat) [ "$2" = "$KNOWN.service" ] && { echo "# unit"; exit 0; }; exit 1 ;;
  show) echo "{ path=$TARGET ; argv[]=$TARGET ; ignore_errors=no ; start_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }" ;;
  restart) exit "${FAKE_RESTART_RC:-0}" ;;
  is-active) exit 0 ;;
esac
S
cat > "$W/shim/journalctl" <<'S'
#!/bin/bash
echo "journalctl $*" >> "$CALLS"; echo "tuner: bind 127.0.0.1:8650: boom"
S
cat > "$W/shim/curl" <<'S'
#!/bin/bash
echo "curl $*" >> "$CALLS"
if [ "${FAKE_ALL_BAD:-0}" = 1 ] || cmp -s "$TARGET" "$BAD"; then printf 500; else printf 200; fi
S
cat > "$W/shim/sleep" <<'S'
#!/bin/bash
echo "sleep $*" >> "$CALLS"
S
chmod +x "$W/shim/"*
printf '\177ELFgood-old' > "$W/old.bin"; printf '\177ELFgood-new' > "$W/good.bin"
printf '\177ELFbad-new' > "$W/bad.bin"; printf '#!/bin/sh\n' > "$W/script.bin"

run() { # run WHICH SCENARIO ARGS...
  local which=$1 sc=$2; shift 2
  local d="$W/run-$sc-$which"; rm -rf "$d"; mkdir -p "$d/backups" "$d/opt"
  cp -p "$W/old.bin" "$d/opt/tuner"
  ( export CALLS="$d/calls" TARGET="$d/opt/tuner" BAD="$W/bad.bin" KNOWN=tuner
    export PATH="$W/shim:$PATH"
    unshare -rm bash -c 'mount --bind "$0" /var/backups && exec "$@"' "$d/backups" "$W/$which" "$@" \
      > "$d/out" 2>&1; echo "exit=$?" >> "$d/out" ) || true
  touch "$d/calls"
  sed -E -i "s/ts=[^ ]+/ts=TS/g; s/[0-9]{8}-[0-9]{6}[+-][0-9]{4}/STAMP/g; s|$d|D|g" "$d/out" "$d/calls"
  (cd "$d" && sha256sum opt/tuner | cut -c1-16; ls backups/galion/deploy 2>/dev/null | sed -E 's/[0-9]{8}-[0-9]{6}[+-][0-9]{4}/STAMP/') > "$d/state"
}

pass=0; fail=0
scenario() { # scenario NAME [ENV=..] -- ARGS
  local sc=$1; shift; local envs=()
  while [ "$1" != -- ]; do envs+=("$1"); shift; done; shift
  for which in orig new; do env "${envs[@]}" bash -c "$(declare -f run); W='$W'; run $which $sc \"\$@\"" _ "$@"; done
  if diff -u "$W/run-$sc-orig/out" "$W/run-$sc-new/out" && diff -u "$W/run-$sc-orig/calls" "$W/run-$sc-new/calls" \
     && diff -u "$W/run-$sc-orig/state" "$W/run-$sc-new/state"; then
    echo "SAME  $sc: $(tail -1 "$W/run-$sc-new/out"), $(wc -l < "$W/run-$sc-new/calls") shim calls"
    pass=$((pass+1))
  else
    echo "DIFF  $sc"; fail=$((fail+1))
  fi
}
scenario healthy SKIP_DATA_BACKUP=1 -- tuner "$W/good.bin"
scenario healthy-unit-suffix SKIP_DATA_BACKUP=1 -- tuner.service "$W/good.bin" http://127.0.0.1:8650/health
scenario broken-rolls-back SKIP_DATA_BACKUP=1 -- tuner "$W/bad.bin"
scenario rollback-also-bad SKIP_DATA_BACKUP=1 FAKE_ALL_BAD=1 -- tuner "$W/good.bin"
scenario restart-refused SKIP_DATA_BACKUP=1 FAKE_RESTART_RC=1 -- tuner "$W/good.bin"
scenario unit-only-health SKIP_DATA_BACKUP=1 -- tuner "$W/good.bin" none
scenario unknown-unit SKIP_DATA_BACKUP=1 -- nosuch "$W/good.bin"
scenario not-elf SKIP_DATA_BACKUP=1 -- tuner "$W/script.bin"
scenario missing-new SKIP_DATA_BACKUP=1 -- tuner "$W/nope.bin"
scenario usage -- tuner
echo "galion-deploy systemd equivalence: $pass same, $fail different"
[ "$fail" = 0 ]
