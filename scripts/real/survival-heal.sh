#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for `guardian-tick heal` on the BOX (slot 04, ports 19040-19049).
# Builds release, starts two dummy services (19040 healthy, 19041 dies after
# DIE_S seconds), then runs real ticks: detection, plan, dry-run, apply under
# --allow (restarts the dummy), and the reflex cap after 3 restarts.
# Optional: CAPTURE=<galion-health status.json> runs `classify` over it.
# Never touches the VPS. Kills everything it started.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/survival-heal}
DIE_S=${DIE_S:-3}
RUN=${RUN:-$(mktemp -d /tmp/survival-heal-real.XXXXXX)}
mkdir -p "$RUN"
ts() { TZ=Europe/Zagreb date -Iseconds; }

cd "$ROOT"
nice -n 10 cargo build -q -j 4 --release -p guardian-tick --bin guardian-tick --example dummy_service
GT="$CARGO_TARGET_DIR/release/guardian-tick"
DUMMY="$CARGO_TARGET_DIR/release/examples/dummy_service"

cleanup() {
  for f in "$RUN"/healthy.pid "$RUN"/dies.pid; do
    [ -f "$f" ] && kill "$(cat "$f")" 2>/dev/null || true
  done
}
trap cleanup EXIT

# restart helper used by the [[repair]] command backend (argv, no shell in guardian-tick)
cat > "$RUN/restart-dies.sh" <<SH
#!/bin/sh
[ -f "$RUN/dies.pid" ] && kill "\$(cat "$RUN/dies.pid")" 2>/dev/null
setsid "$DUMMY" --port 19041 --die-after-s $DIE_S --pidfile "$RUN/dies.pid" >>"$RUN/dies.log" 2>&1 </dev/null &
exit 0
SH
chmod 755 "$RUN/restart-dies.sh"

cat > "$RUN/probes.toml" <<TOML
[[probe]]
kind = "http"
name = "dummy-healthy"
port = 19040
path = "/health"

[[probe]]
kind = "http"
name = "dummy-dies"
port = 19041
path = "/health"

[[repair]]
probe = "dummy-healthy"
unit = "dummy-healthy"
after_fails = 1
backend = "none"

[[repair]]
probe = "dummy-dies"
unit = "dummy-dies"
after_fails = 1
backend = "command"
command = ["$RUN/restart-dies.sh"]
verify_after_ms = 700
TOML

echo "== $(ts) start dummies (run dir $RUN)"
setsid "$DUMMY" --port 19040 --pidfile "$RUN/healthy.pid" >>"$RUN/healthy.log" 2>&1 </dev/null &
setsid "$DUMMY" --port 19041 --die-after-s "$DIE_S" --pidfile "$RUN/dies.pid" >>"$RUN/dies.log" 2>&1 </dev/null &
sleep 0.5
H=(heal --probes "$RUN/probes.toml" --state "$RUN/state")

echo "== $(ts) tick 0: both up (dry-run)"
"$GT" "${H[@]}"
sleep $((DIE_S + 1))
echo "== $(ts) tick 1: dummy-dies has died -> detection + plan, DRY-RUN (no --allow)"
"$GT" "${H[@]}"
for n in 1 2 3; do
  echo "== $(ts) tick apply-$n: --allow dummy-dies"
  "$GT" "${H[@]}" --allow dummy-dies
  sleep $((DIE_S + 1))
done
echo "== $(ts) tick apply-4: --allow dummy-dies (expect cap-reached, no restart)"
"$GT" "${H[@]}" --allow dummy-dies
echo "== dummy-dies process starts logged: $(grep -c dying "$RUN/dies.log" || true) deaths"
echo "== incident store $RUN/state/incidents.jsonl"
cat "$RUN/state/incidents.jsonl"
echo "== nerve $RUN/state/nerve.jsonl"
cat "$RUN/state/nerve.jsonl"

if [ -n "${CAPTURE:-}" ]; then
  echo "== $(ts) classify over captured live status.json: $CAPTURE"
  "$GT" classify --status-json "$CAPTURE"
fi
echo "== $(ts) teardown"
