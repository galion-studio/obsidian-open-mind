#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Real-life test for galion-common: build the migrated tuner and galion-origin
# (release), start them on the slot-16 ports, hammer them over real HTTP
# (latency, malformed, oversized, slowloris), sample memory/threads, tear down.
# With OLD=1 the pre-migration binaries from 13a296ce are run on 19162/19163
# for comparison. Box only; never touches a live service.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/common}
SCRATCH=$(mktemp -d /tmp/galion-common-real.XXXXXX)
PIDS=()
cleanup() {
    for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
    wait 2>/dev/null || true
    rm -rf "$SCRATCH"
}
trap cleanup EXIT

cd "$ROOT"
nice -n 10 cargo build -q -j 4 --release -p tuner -p galion-origin
BIN=$CARGO_TARGET_DIR/release

# origin fixture root
mkdir -p "$SCRATCH/root/docs/build-session" "$SCRATCH/root/state" "$SCRATCH/grinder"
echo "# note" > "$SCRATCH/root/docs/build-session/a.md"
echo "<h1>grinder</h1>" > "$SCRATCH/grinder/index.html"

start() { # name port cmd...
    local name=$1 port=$2; shift 2
    "$@" > "$SCRATCH/$name.log" 2>&1 &
    PIDS+=($!)
    echo "$name pid $! port $port" >&2
    for _ in $(seq 50); do
        (exec 3<>/dev/tcp/127.0.0.1/"$port") 2>/dev/null && return 0
        sleep 0.1
    done
    echo "$name did not start" >&2; cat "$SCRATCH/$name.log" >&2; exit 1
}

TUNER_ADDR=127.0.0.1:19160 TUNER_ROOT=crates/tuner/www start tuner 19160 "$BIN/tuner"
TUNER_PID=${PIDS[-1]}
ORIGIN_ADDR=127.0.0.1:19161 ORIGIN_ROOT=$SCRATCH/root ORIGIN_GRINDER_ROOT=$SCRATCH/grinder \
    start origin 19161 "$BIN/galion-origin"
ORIGIN_PID=${PIDS[-1]}
TARGETS="tuner=127.0.0.1:19160,origin=127.0.0.1:19161"
WATCH="$TUNER_PID:tuner $ORIGIN_PID:origin"

if [ "${OLD:-0}" = 1 ]; then
    OLDTREE=$SCRATCH/old
    git worktree add -q --detach "$OLDTREE" 13a296ce15b5bbd66adc26f6bfae787b2c1b5190
    (cd "$OLDTREE" && CARGO_TARGET_DIR=$CARGO_TARGET_DIR/old nice -n 10 cargo build -q -j 4 --release -p tuner -p galion-origin)
    git worktree remove --force "$OLDTREE"
    TUNER_ADDR=127.0.0.1:19163 TUNER_ROOT=crates/tuner/www start old-tuner 19163 "$CARGO_TARGET_DIR/old/release/tuner"
    WATCH="$WATCH ${PIDS[-1]}:old-tuner"
    ORIGIN_ADDR=127.0.0.1:19162 ORIGIN_ROOT=$SCRATCH/root ORIGIN_GRINDER_ROOT=$SCRATCH/grinder \
        start old-origin 19162 "$CARGO_TARGET_DIR/old/release/galion-origin"
    WATCH="$WATCH ${PIDS[-1]}:old-origin"
    TARGETS="$TARGETS,old-tuner=127.0.0.1:19163,old-origin=127.0.0.1:19162"
fi

mem() { # label
    for w in $WATCH; do
        pid=${w%%:*}; name=${w#*:}
        if [ -r /proc/$pid/status ]; then
            awk -v n="$name" -v l="$1" '/^VmRSS|^VmHWM|^Threads/{a=a" "$1$2} END{print l" "n":"a}' /proc/$pid/status
        else
            echo "$1 $name: DEAD"
        fi
    done
}

echo "== $(date '+%Y-%m-%dT%H:%M:%S%:z') start"
mem before
# 1 s sampler (no busy loop): peak threads per process
( while sleep 1; do for w in $WATCH; do pid=${w%%:*}; t=$(awk '/^Threads/{print $2}' /proc/$pid/status 2>/dev/null || echo 0); echo "${w#*:} $t"; done; done ) > "$SCRATCH/threads.log" &
SAMPLER=$!
PIDS+=($SAMPLER)
GALION_REAL=1 GALION_REAL_TARGETS=$TARGETS nice -n 10 cargo test -q -j 4 --release \
    -p galion-common --test real_hammer -- --nocapture 2>&1 | grep -v '^running\|^test result\|^$'
kill $SAMPLER 2>/dev/null || true
sleep 1
echo "== $(date '+%Y-%m-%dT%H:%M:%S%:z') done"
mem after
echo "peak threads seen by the 1 s sampler:"
sort -k1,1 -k2,2n "$SCRATCH/threads.log" | awk '{m[$1]=$2>m[$1]?$2:m[$1]} END{for(k in m) print "  "k, m[k]}'
for w in $WATCH; do
    pid=${w%%:*}
    kill -0 "$pid" 2>/dev/null && echo "${w#*:}: alive" || echo "${w#*:}: DIED"
done
