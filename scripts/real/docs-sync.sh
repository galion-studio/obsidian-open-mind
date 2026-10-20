#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Real-life check for the swarm/docs-sync branch. Bash only, no Python.
# Builds the real repo-check binary (release) and runs it, then the repo's own
# scripts: spdx-scan, self-check (light; SELF_CHECK_FULL=1 for full),
# no-python and status-percent. Fails unless every one is green, repo-check
# reports 0 warnings, and status-percent prints the numbers in docs/STATUS.md.
# Read-only on the tree. Starts no server, so nothing to tear down.
set -u
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/docs-sync}
fail=0
say() { printf "real/docs-sync: %s\n" "$*"; }
say "time $(TZ=Europe/Zagreb date -Iseconds)"

nice -n 10 cargo build --release -j 4 -p repo-check >/dev/null 2>&1 || { say "build fail"; exit 1; }
rc_out=$("$CARGO_TARGET_DIR/release/repo-check"); rc=$?
summary=$(printf "%s\n" "$rc_out" | tail -n 1)
say "repo-check exit $rc: $summary"
case "$summary" in
  *" 0 error(s), 0 warning(s) -> PASS") ;;
  *) say "repo-check not clean"; printf "%s\n" "$rc_out"; fail=1 ;;
esac
[ "$rc" -eq 0 ] || fail=1

sp=$(scripts/spdx-scan.sh); missing=$(printf "%s\n" "$sp" | sed -n 's/^spdx-scan: missing //p')
say "spdx-scan: $(printf "%s\n" "$sp" | sed -n 's/^spdx-scan: scanned //p') scanned, $missing missing"
[ "$missing" = 0 ] || fail=1

if scripts/self-check.sh >/tmp/docs-sync-selfcheck.log 2>&1; then
  say "self-check: $(tail -n 1 /tmp/docs-sync-selfcheck.log)"
else
  say "self-check fail"; cat /tmp/docs-sync-selfcheck.log; fail=1
fi

scripts/no-python.sh >/dev/null 2>&1 && say "no-python: ok" || { say "no-python fail"; fail=1; }

got=$(scripts/status-percent.sh) || fail=1
want=$(awk '/^status-percent: items/{f=1} f&&/^```/{exit} f' docs/STATUS.md)
printf "%s\n" "$got"
if [ "$got" = "$want" ]; then say "status-percent matches docs/STATUS.md"; else say "status-percent differs from docs/STATUS.md"; fail=1; fi

for b in gaia-land hades-approval gaia-live survival-heal one-probe voice-mouth tuner-cutover room \
  memory-real nerve lan-discovery poseidon ucpu real-harness launch-gates common hermes-tools \
  desk-linkwatch sandbox edge-boot docs-sync; do
  grep -q "\`swarm/$b\`" docs/STATUS.md || { say "STATUS.md lacks swarm/$b"; fail=1; }
done
say "STATUS.md swarm table: $(grep -c '^| [0-9]* | `swarm/' docs/STATUS.md) branches"

if [ "$fail" -ne 0 ]; then say "FAIL"; exit 1; fi
say "PASS"
