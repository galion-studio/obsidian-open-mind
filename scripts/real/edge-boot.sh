#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for the omnode edge boot path (swarm slug edge-boot, slot 20,
# ports 19200-19209). Builds the static release binaries, then runs the REAL
# binary (x86_64 natively, aarch64 under qemu-user when present) through:
#   first boot -> pairing refused (no/wrong code) -> paired -> code is single
#   use -> expired code refused -> 5 refusals lock -> seccomp enforcement with
#   real tools (strace, unshare) -> kill -9 + restart by a minimal supervisor
#   (identity and token survive) -> SIGTERM exit 0 -> PID-1 run in its own
#   PID namespace -> offline `pair verify` accept/refuse.
# No systemd: the box's PID 1 is tini, so the node runs directly / under the
# small bash supervisor below. Pairing codes and tokens are never printed.
#
#   19200 x86_64 main node        19201 x86_64 expiry/lockout node
#   19202 x86_64 PID-1 node       19203 aarch64 main node (qemu)
#   19204 aarch64 expiry node     19205-19206 tests/real (cargo, GALION_REAL=1)
#
# Usage: scripts/real/edge-boot.sh   (env: CARGO_TARGET_DIR, NO_BUILD=1, NO_QEMU=1)
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH

repo=$(cd "$(dirname "$0")/../.." && pwd)
crate=$repo/edge/openmind-node
target_dir=${CARGO_TARGET_DIR:-/workspace/swarm/target/edge-boot}
x86=$target_dir/x86_64-unknown-linux-musl/release/omnode
arm=$target_dir/aarch64-unknown-linux-musl/release/omnode
url=https://hermes.example.invalid/openmind/enroll

if [ "${NO_BUILD:-0}" != 1 ]; then
  (cd "$crate" && CARGO_TARGET_DIR=$target_dir nice -n 10 cargo build -q -j 4 --release --target x86_64-unknown-linux-musl)
  (cd "$crate" && CARGO_TARGET_DIR=$target_dir CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
    nice -n 10 cargo build -q -j 4 --release --target aarch64-unknown-linux-musl)
fi

for b in "$x86" "$arm"; do [ -x "$b" ] || { echo "missing release binary $b (run without NO_BUILD=1)" >&2; exit 2; }; done

work=$(mktemp -d "$target_dir/real-edge-boot.XXXXXX")
pass=0
fail=0
bg=()
cleanup() {
  rm -f "$work"/keep-* 2>/dev/null || true
  for p in "${bg[@]}"; do pkill -TERM -P "$p" 2>/dev/null || true; kill -TERM "$p" 2>/dev/null || true; done
  for p in "${bg[@]}"; do wait "$p" 2>/dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT

ts() { date '+%Y-%m-%dT%H:%M:%S%:z'; }
ok() { pass=$((pass + 1)); echo "  PASS $*"; }
no() { fail=$((fail + 1)); echo "  FAIL $*"; }
check() { # check "label" expected actual
  if [ "$2" = "$3" ]; then ok "$1 ($3)"; else no "$1: want $2, got $3"; fi
}

# req METHOD PORT PATH [json-body] [bearer]: sets $code and $body (file)
req() {
  local args=(-s -m 5 -o "$work/body" -w '%{http_code}' -X "$1")
  [ -n "${4:-}" ] && args+=(-H 'content-type: application/json' --data-binary "$4")
  [ -n "${5:-}" ] && args+=(-H "authorization: Bearer $5")
  code=$(curl "${args[@]}" "http://127.0.0.1:$2$3" 2>/dev/null) || true
  code=${code:-000}
}
j() { jq -r "$1" "$work/body"; }

wait_health() { # port [want_boot_count]
  for _ in $(seq 1 100); do
    req GET "$1" /v1/node
    if [ "$code" = 200 ] && { [ -z "${2:-}" ] || [ "$(j .boot_count)" = "$2" ]; }; then return 0; fi
    sleep 0.2
  done
  return 1
}

# wait_exit PID: wait at most 10 s, then SIGKILL; sets $rc.
wait_exit() {
  for _ in $(seq 1 50); do kill -0 "$1" 2>/dev/null || break; sleep 0.2; done
  if kill -0 "$1" 2>/dev/null; then
    no "pid $1 still running after 10 s; killed"
    pkill -KILL -P "$1" 2>/dev/null || true
    kill -KILL "$1" 2>/dev/null || true
  fi
  set +e; wait "$1"; rc=$?; set -e
}

# Minimal supervisor: restart the node until the keep file is removed.
supervise() { # keepfile logprefix cmd...
  local keep=$1 log=$2
  shift 2
  while [ -e "$keep" ]; do
    set +e
    "$@" >>"$log.out" 2>>"$log.err"
    echo "exit=$?" >>"$log.exits"
    set -e
    [ -e "$keep" ] && sleep 0.5
  done
}

binary_facts() {
  echo "  $(readelf -h "$1" | awk -F: '/Machine/{gsub(/^ +/,"",$2); print "machine: " $2}')"
  echo "  NEEDED: $(readelf -d "$1" 2>/dev/null | grep -c NEEDED || true)  INTERP: $(readelf -l "$1" | grep -c 'program interpreter' || true)"
  echo "  size: $(stat -c %s "$1")  sha256: $(sha256sum "$1" | cut -c1-16)"
}

suite() { # label runner bin port port2 seccomp_expected
  local label=$1 runner=$2 bin=$3 port=$4 port2=$5 sec=$6
  local run=()
  [ -n "$runner" ] && run=("$runner")
  run+=("$bin")
  local xdg=$work/xdg-$label state=$work/xdg-$label/openmind cf=$work/code-$label
  mkdir -p "$xdg"
  echo "== $label: $(ts)"
  binary_facts "$bin"
  echo "  version: $(XDG_CONFIG_HOME=$xdg "${run[@]}" version)"

  # -- seccomp gate (fail-closed when not enforceable)
  if [ "$sec" = enforced ]; then
    set +e; "${run[@]}" sandbox selftest >"$work/body"; rc=$?; set -e
    check "$label sandbox selftest exit" 0 "$rc"
    check "$label selftest enforced" true "$(j .enforced)"
    check "$label fsconfig(431) denied" true "$(j .fsconfig_denied)"
    set +e
    "${run[@]}" sandbox run --wall 10 -- strace -f -o /dev/null true >"$work/s1" 2>&1; rc1=$?
    "${run[@]}" sandbox run --wall 10 --no-seccomp -- strace -f -o /dev/null true >"$work/s2" 2>&1; rc2=$?
    "${run[@]}" sandbox run --wall 10 -- unshare -U true >"$work/s3" 2>&1; rc3=$?
    "${run[@]}" sandbox run --wall 10 -- sh -c 'echo hello >out.txt && cat out.txt' >"$work/s4" 2>&1; rc4=$?
    set -e
    [ "$rc1" != 0 ] && grep -q 'Operation not permitted' "$work/s1" && ok "$label strace under seccomp refused (EPERM)" || no "$label strace under seccomp not refused (rc $rc1)"
    check "$label strace with --no-seccomp (control)" 0 "$rc2"
    [ "$rc3" != 0 ] && grep -q 'Operation not permitted' "$work/s3" && ok "$label unshare -U under seccomp refused (EPERM)" || no "$label unshare not refused (rc $rc3)"
    check "$label everyday shell work allowed" 0 "$rc4"
    grep -q '"seccomp": "deny-list"' "$work/s4" && ok "$label sandbox report seccomp=deny-list" || no "$label sandbox report lacks deny-list"
    req_sec=(--require-seccomp)
  else
    set +e; "${run[@]}" sandbox selftest >"$work/body" 2>/dev/null; rc=$?; set -e
    check "$label sandbox selftest refuses (not enforceable here)" 2 "$rc"
    check "$label selftest enforced" false "$(j .enforced)"
    set +e
    XDG_CONFIG_HOME=$xdg "${run[@]}" boot --state "$work/refuse-$label" --listen "127.0.0.1:$port" --require-seccomp \
      --code-file "$work/refuse-code-$label" >"$work/r.out" 2>"$work/r.err"; rc=$?
    set -e
    check "$label boot --require-seccomp refuses" 2 "$rc"
    grep -q 'not enforced' "$work/r.err" && ok "$label refusal reason: not enforced" || no "$label refusal reason missing"
    [ ! -e "$work/refuse-code-$label" ] && ok "$label no pairing code opened when refused" || no "$label code opened despite refusal"
    req_sec=()
  fi

  # -- first boot under the supervisor
  local keep=$work/keep-$label log=$work/log-$label
  : >"$keep"
  supervise "$keep" "$log" env XDG_CONFIG_HOME="$xdg" "${run[@]}" boot --state "$state" \
    --listen "127.0.0.1:$port" --code-file "$cf" --code-ttl 300 "${req_sec[@]}" &
  local sup=$!
  bg+=("$sup")
  wait_health "$port" 1 && ok "$label first boot listening on :$port" || { no "$label did not come up"; cat "$log.err"; return; }
  local ev
  ev=$(head -1 "$log.out")
  check "$label boot event first_boot" true "$(jq -r .first_boot <<<"$ev")"
  check "$label boot event seccomp.enforced" "$([ "$sec" = enforced ] && echo true || echo false)" "$(jq -r .seccomp.enforced <<<"$ev")"
  local dev pid1
  dev=$(j .device_id)
  pid1=$(j .pid)
  echo "  device_id $dev  fingerprint $(j .fingerprint)  first_boot $(j .first_boot)"
  check "$label key file mode" 600 "$(stat -c %a "$state/node.key")"
  check "$label pair store mode" 600 "$(stat -c %a "$state/pair-store.json")"
  check "$label code file mode" 600 "$(stat -c %a "$cf")"
  local thecode
  thecode=$(tr -d '\n' <"$cf")
  grep -q -- "$thecode" "$state/pair-store.json" && no "$label code stored in clear" || ok "$label store holds only the code hash"

  # -- pairing
  req GET "$port" /v1/whoami
  check "$label whoami unpaired" 401 "$code"
  req POST "$port" /v1/pair '{"code":"AAAA-AAAA"}'
  check "$label wrong code" "403 no" "$code $(j .error)"
  req POST "$port" /v1/pair 'not json'
  check "$label malformed body" 400 "$code"
  req POST "$port" /v1/pair "{\"code\":\"$(tr 'A-Z' 'a-z' <<<"$thecode")\"}"
  check "$label right code (lower-case)" "200 true" "$code $(j .ok)"
  local tok
  tok=$(j .token)
  echo "  token: <redacted, ${#tok} hex chars>"
  req GET "$port" /v1/whoami "" "$tok"
  check "$label whoami with token" "200 $dev" "$code $(j .device_id)"
  req GET "$port" /v1/whoami "" "${tok%?}0"
  check "$label whoami with wrong token" 401 "$code"
  req POST "$port" /v1/pair "{\"code\":\"$thecode\"}"
  check "$label code is single use" "403 no" "$code $(j .error)"
  grep -q -- "$tok" "$state/pair-store.json" "$log.out" "$log.err" && no "$label token leaked to disk/log" || ok "$label token not on disk or in logs"

  # -- restart: kill -9, supervisor brings it back
  kill -KILL "$pid1"
  wait_health "$port" 2 && ok "$label restarted by supervisor after kill -9" || no "$label no restart"
  check "$label same device after restart" "$dev" "$(j .device_id)"
  local pid2
  pid2=$(j .pid)
  check "$label new pid after restart" yes "$([ "$pid2" != "$pid1" ] && echo yes || echo no)"
  check "$label first_boot timestamp kept" "$(jq -r .at <<<"$ev" | cut -c1-16)" "$(j .first_boot | cut -c1-16)"
  check "$label boot event on restart" "false 2 null" "$(sed -n 2p "$log.out" | jq -r '"\(.first_boot) \(.boot_count) \(.pairing_open_until)"')"
  req GET "$port" /v1/whoami "" "$tok"
  check "$label token survives restart" 200 "$code"
  req POST "$port" /v1/pair "{\"code\":\"$thecode\"}"
  check "$label old code still refused after restart" "403 no" "$code $(j .error)"

  # -- offline pair verify against the same identity
  XDG_CONFIG_HOME=$xdg "${run[@]}" pair --dry-run "$url" 2>/dev/null | jq '.would_send.body' >"$work/pair-$label.json"
  set +e
  "${run[@]}" pair verify "$work/pair-$label.json" --url "$url" >"$work/v1" 2>/dev/null; v1=$?
  "${run[@]}" pair verify "$work/pair-$label.json" --url "$url" --now $(($(date +%s) + 3600)) >"$work/v2" 2>/dev/null; v2=$?
  "${run[@]}" pair verify "$work/pair-$label.json" --url https://other.example.invalid/enroll >"$work/v3" 2>/dev/null; v3=$?
  jq '.payload.capabilities += ["money"]' "$work/pair-$label.json" >"$work/pair-t.json"
  "${run[@]}" pair verify "$work/pair-t.json" --url "$url" >"$work/v4" 2>/dev/null; v4=$?
  set -e
  check "$label pair verify fresh payload" "0 true $dev" "$v1 $(jq -r '"\(.ok) \(.verified.device_id)"' "$work/v1")"
  check "$label pair verify expired payload" "2 expired" "$v2 $(jq -r .error "$work/v2" | grep -o expired)"
  check "$label pair verify other server" "2 enroll_url" "$v3 $(jq -r .error "$work/v3" | grep -o enroll_url)"
  check "$label pair verify tampered caps" "2 signature" "$v4 $(jq -r .error "$work/v4" | grep -o signature)"

  # -- SIGTERM: clean exit 0, supervisor stops
  rm -f "$keep"
  kill -TERM "$pid2"
  wait_exit "$sup"
  check "$label exits (kill -9, then SIGTERM)" "exit=137 exit=0" "$(tr '\n' ' ' <"$log.exits" | sed 's/ $//')"
  req GET "$port" /health
  check "$label port closed after stop" 000 "$code"

  # -- expired code and lockout on a second node
  local xdg2=$work/xdg2-$label cf2=$work/code2-$label
  mkdir -p "$xdg2"
  XDG_CONFIG_HOME=$xdg2 "${run[@]}" boot --state "$xdg2/openmind" --listen "127.0.0.1:$port2" --code-file "$cf2" --code-ttl 1 \
    >"$work/n2.out" 2>"$work/n2.err" &
  local n2=$!
  bg+=("$n2")
  wait_health "$port2" 1 || no "$label node 2 did not come up"
  sleep 2.2
  req POST "$port2" /v1/pair "{\"code\":\"$(tr -d '\n' <"$cf2")\"}"
  check "$label expired code refused" "403 expired" "$code $(j .error)"
  for _ in 1 2 3 4; do req POST "$port2" /v1/pair '{"code":"BBBB-BBBB"}'; done
  check "$label 5th refusal" "403 no" "$code $(j .error)"
  req POST "$port2" /v1/pair '{"code":"CCCC-CCCC"}'
  check "$label locked after 5 refusals" "429 later" "$code $(j .error)"
  check "$label fails counted in store" 5 "$(jq -r .fails "$xdg2/openmind/pair-store.json")"
  kill -TERM "$n2"
  wait_exit "$n2"
  check "$label node 2 SIGTERM exit" 0 "$rc"
}

pid1_suite() { # PID 1 in its own PID namespace (like an init-less container)
  local port=19202 xdg=$work/xdg-pid1
  mkdir -p "$xdg"
  echo "== x86_64 as PID 1 (unshare --pid --fork): $(ts)"
  if ! unshare --user --map-root-user --pid --fork --mount-proc true 2>/dev/null; then
    echo "  SKIP: unprivileged PID namespaces unavailable"
    return
  fi
  XDG_CONFIG_HOME=$xdg unshare --user --map-root-user --pid --fork --mount-proc \
    "$x86" boot --state "$xdg/openmind" --listen "127.0.0.1:$port" --code-file "$work/code-pid1" >"$work/p1.out" 2>"$work/p1.err" &
  local u=$!
  bg+=("$u")
  wait_health "$port" 1 && ok "PID-1 node up" || { no "PID-1 node not up"; cat "$work/p1.err"; return; }
  check "PID-1 node sees itself as pid" 1 "$(j .pid)"
  local hostpid
  hostpid=$(pgrep -P "$u" -f 'omnode boot' | head -1)
  kill -TERM "$hostpid"
  wait_exit "$u"
  check "PID-1 node exits 0 on SIGTERM (handler installed)" 0 "$rc"
  grep -q 'stopping (signal)' "$work/p1.err" && ok "PID-1 stop message" || no "PID-1 stop message missing"
}

echo "omnode edge-boot real-life test, $(ts), kernel $(uname -r), PID 1 on this box: $(cat /proc/1/comm)"
suite x86_64 "" "$x86" 19200 19201 enforced
pid1_suite
if [ "${NO_QEMU:-0}" != 1 ] && command -v qemu-aarch64 >/dev/null; then
  suite aarch64-qemu qemu-aarch64 "$arm" 19203 19204 unavailable
else
  echo "== aarch64: SKIP (no qemu-aarch64)"
fi
echo "== cargo real test (GALION_REAL=1, x86_64 release binary, ports 19205-19206): $(ts)"
if (cd "$crate" && GALION_REAL=1 OMNODE_BIN=$x86 CARGO_TARGET_DIR=$target_dir nice -n 10 cargo test -q -j 4 --test real_edge_boot 2>&1 | grep -E "^test result"); then ok "tests/real_edge_boot.rs"; else no "tests/real_edge_boot.rs"; fi
echo "== done $(ts): $pass passed, $fail failed"
[ "$fail" = 0 ]
