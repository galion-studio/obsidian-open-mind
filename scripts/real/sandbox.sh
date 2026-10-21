#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Real-life adversarial test for openmind-sandbox (+ cgroup-detect).
#
# Builds the escape payload and the real_escape suite in release, then runs a
# real sandboxed process that tries to break out (network, ptrace, mount,
# setuid, fork bomb, memory bomb, setsid orphan) and proves each is contained.
#
# It runs the suite as an UNPRIVILEGED user so the setuid-root check is
# meaningful, and (if it can use sudo) sets up a delegated cgroup v2 subtree so
# the pids.max / memory.max checks run too. Everything it creates is torn down.
#
# Usage:   scripts/real/sandbox.sh
# Env:     SANDBOX_USER   unprivileged user to drop to (default: the invoking
#                         non-root user, else "box")
#          CGROUP_ROOT    cgroup v2 mount (default /sys/fs/cgroup)
set -euo pipefail

cd "$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/workspace/swarm/target/sandbox}"
CGROUP_ROOT="${CGROUP_ROOT:-/sys/fs/cgroup}"
RUN_USER="${SANDBOX_USER:-}"
ORIG_CG=""
if [ -z "$RUN_USER" ]; then
  if [ "$(id -u)" -ne 0 ]; then RUN_USER="$(id -un)"; else RUN_USER="box"; fi
fi
RUN_UID="$(id -u "$RUN_USER")"
RUN_GID="$(id -g "$RUN_USER")"

echo "== build (release) =="
nice -n 10 cargo build -j 4 --release -p openmind-sandbox --example escape
nice -n 10 cargo test  -j 4 --release -p openmind-sandbox --test real_escape --no-run
TEST_BIN="$(ls -t "$CARGO_TARGET_DIR"/release/deps/real_escape-* | grep -v '\.d$' | head -1)"

SUDO=""
if [ "$(id -u)" -eq 0 ]; then SUDO=""; elif sudo -n true 2>/dev/null; then SUDO="sudo -n"; fi

# --- optional: setuid-root helper + delegated cgroup (need root) ---
SUID_HELPER=""
CG=""
CLEAN_SUID=""
if [ -n "$SUDO" ] || [ "$(id -u)" -eq 0 ]; then
  SUID_HELPER="$(mktemp -d)/suid-id"
  $SUDO cp /bin/id "$SUID_HELPER"
  $SUDO chown root:root "$SUID_HELPER"
  $SUDO chmod 4755 "$SUID_HELPER"
  CLEAN_SUID="$(dirname "$SUID_HELPER")"

  CG="$CGROUP_ROOT/om-sandbox-real-$$"
  $SUDO mkdir -p "$CG/runner"
  echo "+cpu +memory +pids" | $SUDO tee "$CG/cgroup.subtree_control" >/dev/null
  for f in "" cgroup.procs cgroup.subtree_control cgroup.threads \
           runner runner/cgroup.procs runner/cgroup.subtree_control runner/cgroup.threads; do
    $SUDO chown "$RUN_UID:$RUN_GID" "$CG/$f"
  done
fi

cleanup() {
  set +e
  if [ -n "$CG" ]; then
    # If we moved this shell into the delegated subtree, move it back out so
    # the subtree is empty and can be removed.
    if [ -n "$ORIG_CG" ]; then
      $SUDO sh -c "echo $$ > '$CGROUP_ROOT$ORIG_CG/cgroup.procs'" 2>/dev/null
    fi
    [ -e "$CG/cgroup.kill" ] && echo 1 | $SUDO tee "$CG/runner/cgroup.kill" >/dev/null 2>&1
    $SUDO find "$CG" -depth -type d -exec rmdir {} + 2>/dev/null
  fi
  [ -n "$CLEAN_SUID" ] && $SUDO rm -rf "$CLEAN_SUID"
  $SUDO rm -rf "$CARGO_TARGET_DIR/test-out" 2>/dev/null
}
trap cleanup EXIT

ENVV=(GALION_REAL=1 "PATH=$PATH" "HOME=/home/$RUN_USER")
[ -n "$SUID_HELPER" ] && ENVV+=("GALION_SETUID_HELPER=$SUID_HELPER")
[ -n "$CG" ] && ENVV+=("GALION_REAL_CGROUP_PARENT=$CG")

echo "== run as $RUN_USER (uid $RUN_UID)${CG:+ inside $CG/runner} =="
# Do NOT exec: the EXIT trap must still run to tear the cgroup/helper down.
rc=0
if [ "$(id -u)" -eq 0 ]; then
  # A root wrapper that places the process in the delegated subtree, then
  # drops privileges for the actual run.
  sudo_wrap=$(cat <<WRAP
set -e
${CG:+echo \$\$ > "$CG/runner/cgroup.procs"}
exec setpriv --reuid "$RUN_UID" --regid "$RUN_GID" --clear-groups \
     env ${ENVV[*]} "$TEST_BIN" --ignored --nocapture
WRAP
)
  bash -c "$sudo_wrap" || rc=$?
else
  # Already unprivileged. If a cgroup was delegated, root must move this shell
  # into it so the sandbox can create sibling child cgroups (cleanup moves it
  # back out). Record where we started so cleanup can restore it.
  ORIG_CG=""
  if [ -n "$CG" ] && [ -n "$SUDO" ]; then
    ORIG_CG=$(awk -F: '/^0::/{print $3}' /proc/self/cgroup)
    $SUDO sh -c "echo $$ > $CG/runner/cgroup.procs"
  fi
  env "${ENVV[@]}" "$TEST_BIN" --ignored --nocapture || rc=$?
fi
exit "$rc"
