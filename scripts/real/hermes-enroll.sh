#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for Hermes device enrollment (swarm slug hermes-enroll,
# slot 32, ports 19320-19329). Builds the static release binaries, then runs
# the REAL hermes-gateway (--mock: offline provider, no network, no spend)
# and two REAL omnode edge nodes (`omnode boot`, own state dirs) and walks:
#   route off by default (404) -> unsafe enroll config refuses to start ->
#   node A enrolls (202 pending) -> claim before approval = pending, no token ->
#   wrong fingerprint refused -> approve -> A claims a scoped credential ->
#   Hermes accepts A, scope check -> refuses replayed nonce, expired payload,
#   tampered payloads, unenrolled node (forged token, rejected claim) ->
#   node B (aarch64 under qemu when available) enrolls too -> revoke A ->
#   A refused, B unaffected -> restart Hermes: revocation, B's credential and
#   the nonce store persist -> corrupt / too-open store refuses to start.
# Only loopback is used. Tokens and the throwaway owner key are never printed
# (they travel in mode-600 header files, not argv).
#
#   19320 hermes-gateway   19321 omnode A (x86_64)   19322 omnode B
#   19323 hermes fail-closed start probes (never binds when refusing)
#
# Usage: scripts/real/hermes-enroll.sh   (env: CARGO_TARGET_DIR, NO_BUILD=1, NO_QEMU=1, ZIG)
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH

repo=$(cd "$(dirname "$0")/../.." && pwd)
target_dir=${CARGO_TARGET_DIR:-/workspace/swarm/target/hermes-enroll}
zig=${ZIG:-$(command -v zig 2>/dev/null || echo /workspace/tools/zig-x86_64-linux-0.15.2/zig)}
hermes=$target_dir/x86_64-unknown-linux-musl/release/hermes-gateway
omx=$target_dir/x86_64-unknown-linux-musl/release/omnode
oma=$target_dir/aarch64-unknown-linux-musl/release/omnode
P=19320
url=http://127.0.0.1:$P/openmind/enroll

setup_zig() { # same wrappers as scripts/ucpu-gate.sh (SQLite C code via zig cc)
  local w=$target_dir/ucpu-zig rt zt v pair
  mkdir -p "$w" "$target_dir/zig-cache"
  export ZIG_GLOBAL_CACHE_DIR=$target_dir/zig-cache ZIG_LOCAL_CACHE_DIR=$target_dir/zig-cache
  for pair in x86_64-unknown-linux-musl:x86_64-linux-musl aarch64-unknown-linux-musl:aarch64-linux-musl; do
    rt=${pair%%:*}; zt=${pair#*:}
    printf '#!/bin/sh\nfor a do shift; case "$a" in --target=*) continue ;; esac; set -- "$@" "$a"; done\nexec "%s" cc -target %s "$@"\n' "$zig" "$zt" > "$w/cc-$rt"
    chmod +x "$w/cc-$rt"
    v=$(printf '%s' "$rt" | tr '-' '_')
    export "CC_$v=$w/cc-$rt" "AR_$v=$w/ar"
  done
  printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$zig" > "$w/ar"; chmod +x "$w/ar"
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld
}

qemu=""
if [ "${NO_QEMU:-0}" != 1 ]; then qemu=$(command -v qemu-aarch64-static || command -v qemu-aarch64 || true); fi

if [ "${NO_BUILD:-0}" != 1 ]; then
  setup_zig
  export CARGO_INCREMENTAL=0
  (cd "$repo" && CARGO_TARGET_DIR=$target_dir nice -n 10 cargo build -q -j 3 --release --locked \
    -p hermes-gateway --target x86_64-unknown-linux-musl)
  (cd "$repo/edge/openmind-node" && CARGO_TARGET_DIR=$target_dir nice -n 10 cargo build -q -j 3 --release \
    --target x86_64-unknown-linux-musl)
  if [ -n "$qemu" ]; then
    (cd "$repo/edge/openmind-node" && CARGO_TARGET_DIR=$target_dir nice -n 10 cargo build -q -j 3 --release \
      --target aarch64-unknown-linux-musl)
  fi
fi
for b in "$hermes" "$omx"; do [ -x "$b" ] || { echo "missing release binary $b" >&2; exit 2; }; done
runB=("$omx"); labelB="x86_64"
if [ -n "$qemu" ] && [ -x "$oma" ]; then runB=("$qemu" "$oma"); labelB="aarch64 (qemu-user)"; fi

work=$(mktemp -d "$target_dir/real-hermes-enroll.XXXXXX")
chmod 700 "$work"
pass=0; fail=0; bg=()
cleanup() {
  for p in "${bg[@]}"; do kill -TERM "$p" 2>/dev/null || true; done
  for p in "${bg[@]}"; do wait "$p" 2>/dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT

ts() { date '+%Y-%m-%dT%H:%M:%S%:z'; }
ok() { pass=$((pass + 1)); echo "  PASS $*"; }
no() { fail=$((fail + 1)); echo "  FAIL $*"; }
check() { if [ "$2" = "$3" ]; then ok "$1 ($3)"; else no "$1: want $2, got $3"; fi; }

# Throwaway owner key for this run only (never printed, header file mode 600).
umask 077
key=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
printf 'Authorization: Bearer %s\n' "$key" > "$work/owner.hdr"
printf 'Authorization: Bearer omn1_%s\n' "$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')" > "$work/forged.hdr"

# req METHOD PATH [body-file] [header-file] -> $code, body in $work/body
req() {
  local args=(-s -m 10 -o "$work/body" -w '%{http_code}' -X "$1")
  [ -n "${3:-}" ] && args+=(-H 'Content-Type: application/json' --data-binary "@$3")
  [ -n "${4:-}" ] && args+=(-H "@$4")
  code=$(curl "${args[@]}" "http://127.0.0.1:${PORT:-$P}$2" 2>/dev/null) || true
  code=${code:-000}
}
j() { jq -r "$1" "$work/body"; }
hdr_from_cred() { printf 'Authorization: Bearer %s\n' "$(jq -r .token "$1")" > "$2"; }

henv() { # sets $henv: a clean environment for hermes (direct command, so $! is hermes itself)
  henv=(env -i PATH="$PATH" HOME="$work/hermes" HERMES_HOME="$work/hermes" HERMES_RS_HOST=127.0.0.1
    HERMES_RS_PORT="${PORT:-$P}" HERMES_RS_MCP=0 API_SERVER_KEY="$key")
}
start_hermes() { # log extra-env...
  local log=$1; shift
  henv
  "${henv[@]}" "$@" "$hermes" --mock >>"$work/$log" 2>&1 &
  hpid=$!; bg+=("$hpid")
  for _ in $(seq 1 100); do req GET /health; [ "$code" = 200 ] && return 0; sleep 0.1; done
  no "hermes did not come up ($log)"; return 1
}
stop_hermes() { kill -TERM "$hpid" 2>/dev/null || true; wait "$hpid" 2>/dev/null || true; }
ENR=(HERMES_RS_ENROLL=1 HERMES_RS_ENROLL_URL="$url" HERMES_RS_ENROLL_RATE_PER_MIN=200)

mkdir -p "$work/hermes" "$work/a" "$work/b"
echo "== hermes-enroll real-life test $(ts)"
echo "  hermes-gateway $(stat -c %s "$hermes") bytes, NEEDED=$(readelf -d "$hermes" 2>/dev/null | grep -c NEEDED || true)"
echo "  omnode A x86_64 $(stat -c %s "$omx") bytes; omnode B $labelB"

echo "== 1. enrollment is off by default $(ts)"
start_hermes hermes-off.log
req GET /openmind/node/whoami; check "off: GET /openmind/node/whoami" 404 "$code"
printf '{}' > "$work/empty.json"
req POST /openmind/enroll "$work/empty.json"; check "off: POST /openmind/enroll" 404 "$code"
stop_hermes

echo "== 2. unsafe enrollment config refuses to start $(ts)"
probe_start() { # label want-substring env...
  local label=$1 want=$2; shift 2
  PORT=19323 henv
  set +e; "${henv[@]}" "$@" timeout 10 "$hermes" --mock >"$work/probe.log" 2>&1; local rc=$?; set -e
  if [ "$rc" = 2 ] && grep -q "$want" "$work/probe.log"; then ok "$label: exit 2, \"$want\""; else no "$label: rc=$rc $(tail -1 "$work/probe.log")"; fi
}
probe_start "no URL" "HERMES_RS_ENROLL_URL is not set" HERMES_RS_ENROLL=1
probe_start "plain http to a public host" "plain http is only allowed for a loopback" HERMES_RS_ENROLL=1 HERMES_RS_ENROLL_URL=http://hermes.example.invalid/openmind/enroll
probe_start "wrong path" "path must be /openmind/enroll" HERMES_RS_ENROLL=1 HERMES_RS_ENROLL_URL=http://127.0.0.1:19323/enroll
probe_start "non-loopback bind" "needs HERMES_RS_ENROLL_ALLOW_PUBLIC=1" HERMES_RS_HOST=0.0.0.0 HERMES_RS_ENROLL=1 HERMES_RS_ENROLL_URL=https://hermes.example.invalid/openmind/enroll
probe_start "nonce TTL shorter than payload life" "NONCE_TTL must be" HERMES_RS_ENROLL=1 HERMES_RS_ENROLL_URL="$url" HERMES_RS_ENROLL_NONCE_TTL=60

echo "== 3. hermes (enroll on) + two omnode nodes $(ts)"
start_hermes hermes-1.log "${ENR[@]}"
grep -o 'enroll on: .*' "$work/hermes-1.log" | sed 's/^/  log: /'
store=$work/hermes/rs-enroll.json
check "store created, mode" 600 "$(stat -c %a "$store")"
XDG_CONFIG_HOME=$work/a "$omx" boot --state "$work/a/openmind" --listen 127.0.0.1:19321 --code-file "$work/a/code" >"$work/a/boot.out" 2>"$work/a/boot.err" &
bg+=($!)
XDG_CONFIG_HOME=$work/b "${runB[@]}" boot --state "$work/b/openmind" --listen 127.0.0.1:19322 --code-file "$work/b/code" >"$work/b/boot.out" 2>"$work/b/boot.err" &
bg+=($!)
for port in 19321 19322; do
  up=no
  for _ in $(seq 1 150); do PORT=$port req GET /v1/node; [ "$code" = 200 ] && { up=yes; break; }; sleep 0.2; done
  check "omnode on $port up" yes "$up"
done
PORT=19321 req GET /v1/node; devA=$(j .device_id)
PORT=19322 req GET /v1/node; devB=$(j .device_id); archB=$(j .arch // empty)
fpA=$(XDG_CONFIG_HOME=$work/a "$omx" identity show | awk '/fingerprint/{print $2}')
fpB=$(XDG_CONFIG_HOME=$work/b "${runB[@]}" identity show | awk '/fingerprint/{print $2}')
echo "  node A $devA $fpA"
echo "  node B $devB $fpB ${archB}"
omA() { XDG_CONFIG_HOME=$work/a "$omx" "$@"; }
omB() { XDG_CONFIG_HOME=$work/b "${runB[@]}" "$@"; }
rc_of() { set +e; "$@" >"$work/cmd.out" 2>"$work/cmd.err"; rc=$?; set -e; }

echo "== 4. node A enrolls, owner approves by fingerprint, A claims $(ts)"
# A advertises a payment capability: enrollment must never grant it.
printf '[node]\ncapabilities = ["cpu", "sandbox", "revolut-pay"]\n' > "$work/a/openmind/node.toml"
rc_of omA enroll request "$url" --state "$work/a/openmind"
check "A: enroll request -> pending (exit 3)" 3 "$rc"
check "A: hermes answered" "202 pending" "$(jq -r '"\(.result.http) \(.result.hermes.status)"' "$work/cmd.out")"
req GET /openmind/enroll/pending; check "pending list without owner key" 401 "$code"
req GET /openmind/enroll/pending "" "$work/owner.hdr"; check "pending list (owner)" 200 "$code"
check "pending fingerprint == omnode identity show" "$fpA" "$(j ".pending[] | select(.device_id==\"$devA\") | .fingerprint")"
rc_of omA enroll claim --state "$work/a/openmind"
check "A: claim before approval -> pending (exit 3), no credential" "3 absent" "$rc $([ -e "$work/a/openmind/hermes-credential.json" ] && echo present || echo absent)"
printf '{"device_id":"%s","fingerprint":"SHA256:0000-0000-0000-0000-0000-0000-0000-0000"}' "$devA" > "$work/appr-bad.json"
req POST /openmind/enroll/approve "$work/appr-bad.json" "$work/owner.hdr"; check "approve with wrong fingerprint" 409 "$code"
printf '{"device_id":"%s","fingerprint":"%s","approved_by":"slavik"}' "$devA" "$fpA" > "$work/appr-a.json"
req POST /openmind/enroll/approve "$work/appr-a.json"; check "approve without owner key" 401 "$code"
req POST /openmind/enroll/approve "$work/appr-a.json" "$work/owner.hdr"; check "approve (owner)" 200 "$code"
check "A: scopes granted / denied" "cpu,sandbox / revolut-pay" "$(j '.scopes|join(",")') / $(j '.denied|join(",")')"
rc_of omA enroll claim --state "$work/a/openmind"
check "A: claim -> credential (exit 0)" 0 "$rc"
credA=$work/a/openmind/hermes-credential.json
check "A: credential file mode" 600 "$(stat -c %a "$credA")"
tokA=$(jq -r .token "$credA")
if grep -qF "$tokA" "$work/cmd.out" "$work/cmd.err" "$work/hermes-1.log"; then no "A token leaked to stdout/stderr/log"; else ok "A token not printed by omnode or hermes"; fi
if grep -qF "$tokA" "$store"; then no "A token stored in clear by hermes"; else ok "hermes store holds no clear token"; fi
hdr_from_cred "$credA" "$work/a.hdr"
rc_of omA enroll status --state "$work/a/openmind"; check "A: status (whoami) accepted" 0 "$rc"
rc_of omA enroll status --state "$work/a/openmind" --scope cpu; check "A: scope cpu" 0 "$rc"
rc_of omA enroll status --state "$work/a/openmind" --scope revolut-pay; check "A: scope revolut-pay refused" "2 403" "$rc $(jq -r '.error' "$work/cmd.out" | grep -o 'HTTP [0-9]*' | cut -c6-)"
req GET /v1/models "" "$work/a.hdr"; check "node token on the owner API (/v1/models)" 401 "$code"
req GET /openmind/enroll/devices "" "$work/a.hdr"; check "node token on owner enroll routes" 401 "$code"
printf '{"model":"hermes-agent","messages":[{"role":"user","content":"ping"}]}' > "$work/chat.json"
req POST /v1/chat/completions "$work/chat.json" "$work/owner.hdr"; check "owner API unchanged: mock chat with owner key" 200 "$code"

echo "== 5. refusals: replayed nonce, expired, tampered, unenrolled $(ts)"
omB pair "$url" --state "$work/b/openmind" --dry-run 2>/dev/null | jq '.would_send.body' > "$work/docB.json"
req POST /openmind/enroll "$work/docB.json"; check "B: first POST of its payload" "202 pending" "$code $(j .status)"
req POST /openmind/enroll "$work/docB.json"; check "B: same payload replayed" "409 nonce already used" "$code $(j .error)"
omB pair "$url" --state "$work/b/openmind" --dry-run --now $(( $(date +%s) - 1000 )) 2>/dev/null | jq '.would_send.body' > "$work/docB-old.json"
req POST /openmind/enroll "$work/docB-old.json"; check "B: expired payload (created 1000 s ago)" "400 expired" "$code $(j .error | grep -o expired)"
jq '.payload.capabilities += ["money"]' "$work/docB.json" > "$work/t1.json"
req POST /openmind/enroll "$work/t1.json"; check "tampered capabilities" "400 signature does not verify" "$code $(j .error)"
jq '.payload.expires_unix += 86400' "$work/docB.json" > "$work/t2.json"
req POST /openmind/enroll "$work/t2.json"; check "tampered expires_unix" 400 "$code"
jq --arg pk "$(jq -r .payload.public_key "$work/docB.json" | sed 's/^./f/')" '.payload.public_key = $pk' "$work/docB.json" > "$work/t3.json"
req POST /openmind/enroll "$work/t3.json"; check "foreign public key" 400 "$code"
jq '.payload.enroll_url = "https://other.example.invalid/openmind/enroll"' "$work/docB.json" > "$work/t4.json"
req POST /openmind/enroll "$work/t4.json"; check "payload for another server" 400 "$code"
jq '.payload.admin = true' "$work/docB.json" > "$work/t5.json"
req POST /openmind/enroll "$work/t5.json"; check "unknown field" 400 "$code"
req GET /openmind/node/whoami "" "$work/forged.hdr"; check "unenrolled: forged token" "401 not enrolled" "$code $(j .error)"
req GET /openmind/node/whoami; check "unenrolled: no token" 401 "$code"
rc_of omB enroll claim --state "$work/b/openmind"
check "unenrolled B (no request file yet) cannot claim" 2 "$rc"
# B's request is pending (from the raw POST above); record it for omnode and claim
printf '{"url":"%s","device_id":"%s"}' "$url" "$devB" > "$work/b/openmind/enroll.json"
rc_of omB enroll claim --state "$work/b/openmind"; check "pending B: claim -> pending (exit 3)" 3 "$rc"
printf '{"device_id":"%s"}' "$devB" > "$work/rej-b.json"
req POST /openmind/enroll/reject "$work/rej-b.json" "$work/owner.hdr"; check "owner rejects B" 200 "$code"
rc_of omB enroll claim --state "$work/b/openmind"
check "rejected B: claim refused" "2 not enrolled" "$rc $(jq -r .error "$work/cmd.out" | grep -o 'not enrolled')"
check "B has no credential" absent "$([ -e "$work/b/openmind/hermes-credential.json" ] && echo present || echo absent)"

echo "== 6. node B enrolls properly ($labelB) $(ts)"
rc_of omB enroll request "$url" --state "$work/b/openmind"; check "B: enroll request -> pending" 3 "$rc"
printf '{"device_id":"%s","fingerprint":"%s","scopes":["cpu"]}' "$devB" "$fpB" > "$work/appr-b.json"
req POST /openmind/enroll/approve "$work/appr-b.json" "$work/owner.hdr"; check "approve B with scopes [cpu]" "200 cpu" "$code $(j '.scopes|join(",")')"
rc_of omB enroll claim --state "$work/b/openmind"; check "B: claim -> credential" 0 "$rc"
credB=$work/b/openmind/hermes-credential.json
hdr_from_cred "$credB" "$work/b.hdr"
rc_of omB enroll status --state "$work/b/openmind"; check "B: accepted" 0 "$rc"
rc_of omB enroll status --state "$work/b/openmind" --scope sandbox; check "B: sandbox not granted" 2 "$rc"

echo "== 7. revoke A $(ts)"
printf '{"device_id":"%s","revoked_by":"slavik"}' "$devA" > "$work/rev-a.json"
req POST /openmind/enroll/revoke "$work/rev-a.json"; check "revoke without owner key" 401 "$code"
req POST /openmind/enroll/revoke "$work/rev-a.json" "$work/owner.hdr"; check "revoke A (owner)" 200 "$code"
req GET /openmind/node/whoami "" "$work/a.hdr"; check "A's credential refused after revoke" 401 "$code"
rc_of omA enroll status --state "$work/a/openmind"; check "A: omnode status refused" 2 "$rc"
rc_of omA enroll claim --state "$work/a/openmind"
check "A: new claim refused" "2 device revoked" "$rc $(jq -r .error "$work/cmd.out" | grep -o 'device revoked')"
rc_of omA enroll request "$url" --state "$work/a/openmind"
check "A: re-enroll refused" "2 device revoked" "$rc $(jq -r .error "$work/cmd.out" | grep -o 'device revoked')"
req GET /openmind/node/whoami "" "$work/b.hdr"; check "B unaffected" "200 $devB" "$code $(j .device_id)"

echo "== 8. restart hermes: persistence $(ts)"
stop_hermes
start_hermes hermes-2.log "${ENR[@]}"
req GET /openmind/node/whoami "" "$work/a.hdr"; check "after restart: A still refused" 401 "$code"
req GET /openmind/node/whoami "" "$work/b.hdr"; check "after restart: B's credential still accepted" "200 cpu" "$code $(j '.scopes|join(",")')"
req POST /openmind/enroll "$work/docB.json"; check "after restart: replayed nonce still refused" "409 nonce already used" "$code $(j .error)"
req GET /openmind/enroll/devices "" "$work/owner.hdr"
check "devices after restart" "$(printf '%s\n' "$devA:revoked" "$devB:active" | sort | paste -sd' ')" "$(j '[.devices[] | "\(.device_id):\(.status)"] | sort | join(" ")')"
if grep -qF "$(jq -r .token "$credB")" "$work/body"; then no "devices list leaks token"; else ok "devices list shows no token"; fi
check "store mode after writes" 600 "$(stat -c %a "$store")"
echo "  store: $(jq -c '{nonces: (.nonces|length), pending: (.pending|length), devices: (.devices|length), flags: (.flags|length)}' "$store")"
stop_hermes

echo "== 9. damaged store refuses to start (never reset) $(ts)"
cp "$store" "$work/corrupt.json"; printf 'x' >> "$work/corrupt.json"; sum=$(sha256sum "$work/corrupt.json" | cut -c1-64)
probe_start "corrupt store" "is corrupt" "${ENR[@]}" HERMES_RS_ENROLL_STORE="$work/corrupt.json"
check "corrupt store left untouched" "$sum" "$(sha256sum "$work/corrupt.json" | cut -c1-64)"
cp "$store" "$work/open.json"; chmod 644 "$work/open.json"
probe_start "store mode 644" "must be 600" "${ENR[@]}" HERMES_RS_ENROLL_STORE="$work/open.json"
probe_start "store dir missing" "write" "${ENR[@]}" HERMES_RS_ENROLL_STORE="$work/nope/s.json"

echo "== hermes log lines (no secrets) =="
grep -h 'enroll' "$work/hermes-1.log" "$work/hermes-2.log" | sed 's/^/  /' | head -40
echo "RESULT $(ts) pass=$pass fail=$fail"
[ "$fail" = 0 ]
