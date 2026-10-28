#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for hades-gate admin approvals (slot 02, ports 19020-19029).
# Builds the release binary, makes a THROWAWAY admin key in a temp dir (never
# a real secret), starts `hades-gate serve` on 127.0.0.1, approves a dry-run
# action once, and shows that replay, tampering, expiry, wrong scope, the
# admin-safety rule, the disable switch and /v1/stop all behave. Tears down.
# Token strings and key material are never printed.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
PORT=${HADES_REAL_PORT:-19020}
TARGET=${HADES_REAL_TARGET:-x86_64-unknown-linux-musl}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/hades-approval}
case "$PORT" in 1902[0-9]) ;; *) echo "port $PORT outside 19020-19029" >&2; exit 1 ;; esac

(cd "$ROOT" && nice -n 10 cargo build --release -p hades-gate -j 4 --target "$TARGET" >/dev/null 2>&1)
BIN="$CARGO_TARGET_DIR/$TARGET/release/hades-gate"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/hades-real.XXXXXX")
PID=
cleanup() {
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then kill "$PID"; wait "$PID" 2>/dev/null || true; fi
  rm -rf "$TMP"
}
trap cleanup EXIT

ts() { TZ=Europe/Zagreb date '+%Y-%m-%dT%H:%M:%S%:z'; }
say() { printf '\n[%s] %s\n' "$(ts)" "$*"; }
fail=0
expect() { # expect <label> <want-substring> <got>
  if printf '%s' "$3" | grep -q -- "$2"; then printf 'PASS %s\n' "$1"; else printf 'FAIL %s (want %s)\n' "$1" "$2"; fail=1; fi
}

say "binary: $BIN ($(stat -c %s "$BIN") bytes)"
"$BIN" keygen --seed "$TMP/admin.seed" --pub "$TMP/admin.pub"
cat > "$TMP/hades.toml" <<CFG
admin_pubkey = "$TMP/admin.pub"
ledger = "$TMP/ledger.jsonl"
disable_file = "$TMP/DISABLED"
max_ttl_secs = 900
CFG
printf 'restart galion-tuner\nverify /health on 127.0.0.1:8420' > "$TMP/plan.txt"

"$BIN" serve --config "$TMP/hades.toml" --port "$PORT" 2>"$TMP/serve.log" &
PID=$!
for _ in $(seq 1 50); do curl -fs "http://127.0.0.1:$PORT/health" >/dev/null 2>&1 && break; sleep 0.1; done

post() { # post <token-file-or-empty> <action> <target> <plan-file>
  local tok=""
  [ -n "$1" ] && tok=$(cat "$1")
  jq -n --arg a "$2" --arg t "$3" --rawfile p "$4" --arg k "$tok" \
    '{action:$a,target:$t,plan:$p,token:$k}' > "$TMP/body.json"
  curl -s -w ' http=%{http_code}' -X POST --data-binary @"$TMP/body.json" \
    -H 'Content-Type: application/json' "http://127.0.0.1:$PORT/v1/approve"
}

say "GET /health"
out=$(curl -s "http://127.0.0.1:$PORT/health"); echo "$out"
expect health '"mode":"dry-run"' "$out"

say "1. no token -> DryRun (default)"
out=$(post "" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect dry-run '"decision":"DryRun"' "$out"

say "2. old public token approved-test -> refused"
printf 'approved-test' > "$TMP/old.tok"
out=$(post "$TMP/old.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect old-token '"code":"malformed"' "$out"

say "3. mint a token (throwaway key) and approve the dry-run action once"
"$BIN" mint --seed "$TMP/admin.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/plan.txt" --ttl 120 --out "$TMP/good.tok"
out=$(post "$TMP/good.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect approved '"decision":"WouldApply"' "$out"
expect not-executed '"executed":false' "$out"

say "4. replay the same token -> refused"
out=$(post "$TMP/good.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect replay '"code":"replayed"' "$out"

say "5. tamper: one payload hex digit changed (fresh token) -> refused"
"$BIN" mint --seed "$TMP/admin.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/plan.txt" --ttl 120 --out "$TMP/t2.tok"
tok=$(cat "$TMP/t2.tok"); p=${tok#hades1.}; p=${p%%.*}; s=${tok##*.}
# The last payload digit is the last digit of exp: '3' (0x33) <-> '2' (0x32).
last=${p: -1}; if [ "$last" = 3 ]; then new=2; else new=3; fi
printf 'hades1.%s%s.%s' "${p%?}" "$new" "$s" > "$TMP/tampered.tok"; unset tok p s
out=$(post "$TMP/tampered.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect tamper '"code":"forged"' "$out"

say "6. forged: token signed by a different throwaway key -> refused"
"$BIN" keygen --seed "$TMP/mallory.seed" --pub "$TMP/mallory.pub" >/dev/null
"$BIN" mint --seed "$TMP/mallory.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/plan.txt" --ttl 120 --out "$TMP/mallory.tok" >/dev/null
out=$(post "$TMP/mallory.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect forged '"code":"forged"' "$out"

say "7. wrong scope: valid token used for another target -> refused"
out=$(post "$TMP/t2.tok" heal-apply galion-one "$TMP/plan.txt"); echo "$out"
expect wrong-scope '"code":"wrong-scope"' "$out"

say "8. expiry: token with ttl 2 s, used after 3 s -> refused"
"$BIN" mint --seed "$TMP/admin.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/plan.txt" --ttl 2 --out "$TMP/short.tok"
sleep 3
out=$(post "$TMP/short.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect expired '"code":"expired"' "$out"

say "9. admin-safety: validly signed token for 'rm -rf' -> refused"
printf 'rm -rf /var/lib/galion' > "$TMP/evil.txt"
"$BIN" mint --seed "$TMP/admin.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/evil.txt" --ttl 120 --out "$TMP/evil.tok" >/dev/null
out=$(post "$TMP/evil.tok" heal-apply galion-tuner "$TMP/evil.txt"); echo "$out"
expect safety-destructive '"code":"admin-safety"' "$out"
printf 'usermod -L slavik' > "$TMP/lock.txt"
"$BIN" mint --seed "$TMP/admin.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/lock.txt" --ttl 120 --out "$TMP/lock.tok" >/dev/null
out=$(post "$TMP/lock.tok" heal-apply galion-tuner "$TMP/lock.txt"); echo "$out"
expect safety-admin '"code":"admin-safety"' "$out"

say "10. kill with a valid signature -> refused"
"$BIN" mint --seed "$TMP/admin.seed" --action kill --target galion-tuner \
  --plan-file "$TMP/plan.txt" --ttl 120 --out "$TMP/kill.tok" >/dev/null
out=$(post "$TMP/kill.tok" kill galion-tuner "$TMP/plan.txt"); echo "$out"
expect kill '"code":"action"' "$out"

say "11. admin disables HADES (no token needed) -> everything refused"
"$BIN" disable --config "$TMP/hades.toml"
"$BIN" mint --seed "$TMP/admin.seed" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/plan.txt" --ttl 120 --out "$TMP/t3.tok" >/dev/null
out=$(post "$TMP/t3.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect disabled '"code":"disabled"' "$out"
"$BIN" enable --config "$TMP/hades.toml"
out=$(post "$TMP/t3.tok" heal-apply galion-tuner "$TMP/plan.txt"); echo "$out"
expect re-enabled '"decision":"WouldApply"' "$out"

say "12. CLI check path: replay of t3 via the CLI -> exit 2"
set +e
out=$("$BIN" check --config "$TMP/hades.toml" --action heal-apply --target galion-tuner \
  --plan-file "$TMP/plan.txt" --token-file "$TMP/t3.tok"); rc=$?
set -e
echo "$out exit=$rc"
expect cli-replay '"code":"replayed"' "$out"
[ "$rc" = 2 ] && echo "PASS cli-exit-2" || { echo "FAIL cli-exit-2"; fail=1; }

say "13. ledger (append-only, fsynced): event counts; no token strings stored"
jq -r '.event' "$TMP/ledger.jsonl" | sort | uniq -c
if grep -qF "$(cat "$TMP/t3.tok")" "$TMP/ledger.jsonl"; then echo "FAIL token-in-ledger"; fail=1; else echo "PASS no-token-in-ledger"; fi
stat -c 'ledger mode %a' "$TMP/ledger.jsonl"; stat -c 'seed mode %a' "$TMP/admin.seed"

say "14. admin stop: POST /v1/stop"
curl -s -X POST "http://127.0.0.1:$PORT/v1/stop"; echo
sleep 0.3
if kill -0 "$PID" 2>/dev/null; then echo "FAIL still running"; fail=1; else echo "PASS stopped"; PID=; fi
if ss -ltn 2>/dev/null | grep -q ":$PORT "; then echo "FAIL port still open"; fail=1; else echo "PASS port $PORT closed"; fi

say "result: $([ $fail = 0 ] && echo ALL PASS || echo FAILURES)"
exit $fail
