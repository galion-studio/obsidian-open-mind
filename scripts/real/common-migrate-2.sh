#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for the galion-common HTTP migration (slot 28, ports
# 19280-19289). Box only; never touches a live service.
#
# 1. Builds the migrated release binaries and the pre-migration ones (from
#    swarm/common 26d98b0, in a throw-away worktree).
# 2. real_migrate (GALION_REAL=1): old and new side by side, the same valid
#    traffic to both, reply bytes compared; client binaries against a
#    recording fixture, request bytes compared.
# 3. Abuse: the galion-common hammer (malformed, oversized, slowloris,
#    concurrency) against new and old hermes-gateway, openmind-memory,
#    galion-desk and room, plus chunked-upload abuse for hermes; every
#    process must still be alive at the end.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/common-migrate-2}
export CARGO_INCREMENTAL=0
OLD_REV=${OLD_REV:-26d98b055bd980045e718e3ab63a0d26888f0256}
BASE=19280
SCRATCH=$(mktemp -d /tmp/common-migrate-2-real.XXXXXX)
PIDS=()
cleanup() {
    for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
    wait 2>/dev/null || true
    rm -rf "$SCRATCH"
}
trap cleanup EXIT
BINS=(hermes-gateway openmind-memory galion-desk room link-watch guardian-tick)
PKGS=(); for b in "${BINS[@]}"; do PKGS+=(-p "$b"); done

cd "$ROOT"
nice -n 10 cargo build -q -j 3 --release "${PKGS[@]}"
NEW=$CARGO_TARGET_DIR/release
OLDTREE=$SCRATCH/old-tree
git worktree add -q --detach "$OLDTREE" "$OLD_REV"
(cd "$OLDTREE" && CARGO_TARGET_DIR=$CARGO_TARGET_DIR/old nice -n 10 cargo build -q -j 3 --release "${PKGS[@]}")
git worktree remove --force "$OLDTREE"
OLD=$SCRATCH/oldbin
mkdir -p "$OLD"
for b in "${BINS[@]}"; do cp "$CARGO_TARGET_DIR/old/release/$b" "$OLD/"; done

echo "== $(date '+%Y-%m-%dT%H:%M:%S%:z') valid traffic, old vs new"
mkdir -p "$SCRATCH/diff"
GALION_REAL=1 GALION_REAL_OLD_BIN=$OLD GALION_REAL_NEW_BIN=$NEW GALION_REAL_SCRATCH=$SCRATCH/diff \
    GALION_REAL_PORT_BASE=$BASE nice -n 10 cargo test -q -j 3 --release -p galion-common \
    --test real_migrate real_migrate -- --exact --nocapture 2>&1 | grep -v '^running\|^test result\|^$'

start() { # name port bin args... (env set by caller)
    local name=$1 port=$2; shift 2
    "$@" > "$SCRATCH/$name.log" 2>&1 &
    PIDS+=($!)
    for _ in $(seq 100); do
        (exec 3<>/dev/tcp/127.0.0.1/"$port") 2>/dev/null && return 0
        sleep 0.1
    done
    echo "$name did not start" >&2; cat "$SCRATCH/$name.log" >&2; exit 1
}
KEY=real-test-key-not-a-secret-0123
WATCH=""; TARGETS=""
for side in new old; do
    if [ $side = new ]; then D=$NEW; o=1; else D=$OLD; o=0; fi
    hp=$((BASE + o)); mp=$((BASE + 2 + o)); dp=$((BASE + 4 + o)); rp=$((BASE + 6 + o))
    mkdir -p "$SCRATCH/$side/hermes" "$SCRATCH/$side/memory" "$SCRATCH/$side/desk/state" "$SCRATCH/$side/desk/one"
    start $side-hermes $hp env -i PATH=/usr/bin:/bin HOME=/nonexistent HERMES_HOME="$SCRATCH/$side/hermes" \
        HERMES_RS_HOST=127.0.0.1 HERMES_RS_PORT=$hp HERMES_RS_MCP=0 API_SERVER_KEY=$KEY "$D/hermes-gateway" --mock
    WATCH="$WATCH ${PIDS[-1]}:$side-hermes"
    start $side-memory $mp env -i PATH=/usr/bin:/bin HOME=/nonexistent \
        OPENMIND_MEMORY_DATA_DIR="$SCRATCH/$side/memory" OPENMIND_MEMORY_PORT=$mp "$D/openmind-memory"
    WATCH="$WATCH ${PIDS[-1]}:$side-memory"
    start $side-desk $dp env -i PATH=/usr/bin:/bin HOME=/nonexistent OPENMIND_CONFIG=/nonexistent DESK_BIND=127.0.0.1:$dp \
        DESK_STATE_DIR="$SCRATCH/$side/desk/state" DESK_ONE_DIR="$SCRATCH/$side/desk/one" \
        DESK_HERMES_DIR="$SCRATCH/$side/desk/hermes" DESK_OLLAMA=127.0.0.1:$((BASE + 9)) DESK_HERMES_BIN=/bin/false \
        "$D/galion-desk"
    WATCH="$WATCH ${PIDS[-1]}:$side-desk"
    start $side-room $rp env -i PATH=/usr/bin:/bin HOME=/nonexistent ROOM_BIND=127.0.0.1:$rp "$D/room" --listen
    WATCH="$WATCH ${PIDS[-1]}:$side-room"
    TARGETS="$TARGETS${TARGETS:+,}$side-hermes=127.0.0.1:$hp,$side-memory=127.0.0.1:$mp,$side-desk=127.0.0.1:$dp,$side-room=127.0.0.1:$rp"
done

mem() {
    for w in $WATCH; do
        pid=${w%%:*}; name=${w#*:}
        if [ -r /proc/$pid/status ]; then
            awk -v n="$name" -v l="$1" '/^VmRSS|^VmHWM|^Threads/{a=a" "$1$2} END{print l" "n":"a}' /proc/$pid/status
        else
            echo "$1 $name: DEAD"
        fi
    done
}
echo "== $(date '+%Y-%m-%dT%H:%M:%S%:z') abuse"
mem before
( while sleep 1; do for w in $WATCH; do pid=${w%%:*}; t=$(awk '/^Threads/{print $2}' /proc/$pid/status 2>/dev/null || echo 0); echo "${w#*:} $t"; done; done ) > "$SCRATCH/threads.log" &
SAMPLER=$!
PIDS+=($SAMPLER)
GALION_REAL=1 GALION_REAL_CHUNKED_TARGETS="new-hermes=127.0.0.1:$((BASE + 1)),old-hermes=127.0.0.1:$BASE" \
    nice -n 10 cargo test -q -j 3 --release -p galion-common --test real_migrate real_chunked_abuse \
    -- --exact --nocapture 2>&1 | grep -v '^running\|^test result\|^$'
GALION_REAL=1 GALION_REAL_TARGETS=$TARGETS nice -n 10 cargo test -q -j 3 --release \
    -p galion-common --test real_hammer -- --nocapture 2>&1 | grep -v '^running\|^test result\|^$'
kill $SAMPLER 2>/dev/null || true
sleep 1
echo "== $(date '+%Y-%m-%dT%H:%M:%S%:z') done"
mem after
echo "peak threads seen by the 1 s sampler:"
sort -k1,1 -k2,2n "$SCRATCH/threads.log" | awk '{m[$1]=$2>m[$1]?$2:m[$1]} END{for(k in m) print "  "k, m[k]}' | sort
dead=0
for w in $WATCH; do
    pid=${w%%:*}
    if kill -0 "$pid" 2>/dev/null; then echo "${w#*:}: alive"; else echo "${w#*:}: DIED"; dead=1; fi
done
exit $dead
