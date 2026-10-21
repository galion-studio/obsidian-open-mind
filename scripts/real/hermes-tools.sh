#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for hermes-tools (slot 17, ports 19170-19179):
#   - builds the release hermes-gateway (with the coding tools) and the
#     scripted mock model server,
#   - starts both on 127.0.0.1 (gateway :19170, mock :19171),
#   - drives three sessions through the real HTTP API:
#       ticket : git status, read, cargo test (red), edit, cargo test (green),
#                diff, commit on the work branch
#       escape : traversal, absolute, symlink, .git hooks, .cargo config,
#                "shell rm -rf /", cargo arg injection, git push, then a test
#                file that tries network / writes / rm -rf / read / git push
#                from inside cargo test in the sandbox
#       spend  : mock reports $0.60 per call; the $1.00 session cap and the
#                $2.00 daily cap stop it
#   - checks the outcome, prints evidence, and kills everything it started.
# No real model call: HERMES_RS_PROVIDER_LOOPBACK_ONLY=1, no provider key in
# the environment, provider base URL = the local mock.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH

REPO=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/hermes-tools}
RUN=${RUN:-$CARGO_TARGET_DIR/real-run}
GW_PORT=19170
MOCK_PORT=19171
FIX=$REPO/crates/hermes-tools/tests/fixtures
ts() { TZ=Europe/Zagreb date -Iseconds; }
say() { echo "[$(ts)] $*"; }

say "build (release, nice, -j 4)"
nice -n 10 cargo build -q --release -j 4 -p hermes-tools --bin hermes-mock-model
nice -n 10 cargo build -q --release -j 4 -p hermes-gateway --no-default-features --features coding
GW=$CARGO_TARGET_DIR/release/hermes-gateway
MOCK=$CARGO_TARGET_DIR/release/hermes-mock-model
ls -l "$GW" "$MOCK" | awk '{print "  " $5 " bytes  " $9}'

rm -rf "$RUN"; mkdir -p "$RUN"/{home,canary}
echo keep > "$RUN/canary/keep.txt"
WT=$RUN/ticket
mkdir -p "$WT/src"
cp "$FIX/ticket/Cargo.toml" "$WT/Cargo.toml"
cp "$FIX/ticket/lib.rs.buggy" "$WT/src/lib.rs"
printf 'target/\n' > "$WT/.gitignore"
echo "# adder (fake ticket FAKE-1)" > "$WT/README.md"
ln -s /etc/hostname "$WT/link-out"
G() { git -c user.name=setup -c user.email=setup@local -c init.defaultBranch=main "$@"; }
G init -q --bare "$RUN/remote.git"
G -C "$WT" init -q
G -C "$WT" add -A && G -C "$WT" commit -qm "FAKE-1: buggy adder"
G -C "$WT" remote add origin "$RUN/remote.git"
G -C "$WT" push -q origin main
G -C "$WT" checkout -qb swarm/fake-ticket
say "fake ticket repo $WT on $(G -C "$WT" branch --show-current), origin = $RUN/remote.git"

TOOLCHAIN=$(rustc --print sysroot)
KEY=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')   # test-only gateway key, never printed

pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
trap cleanup EXIT

"$MOCK" --port $MOCK_PORT --scripts "$FIX/mock-scripts" --log "$RUN/mock.jsonl" \
  --var "CANARY=$RUN/canary" --var "MOCK_PORT=$MOCK_PORT" --var "HOST_FILE=/workspace/swarm/RULES.md" \
  >"$RUN/mock.out" 2>"$RUN/mock.err" &
pids+=($!)

env -i PATH=/usr/bin:/bin LANG=C.UTF-8 TZ=Europe/Zagreb \
  HERMES_HOME="$RUN/home" HERMES_RS_HOST=127.0.0.1 HERMES_RS_PORT=$GW_PORT \
  HERMES_RS_MCP=0 HERMES_RS_CRON=0 API_SERVER_KEY="$KEY" \
  OPENROUTER_BASE_URL="http://127.0.0.1:$MOCK_PORT/v1" HERMES_RS_PROVIDER_LOOPBACK_ONLY=1 \
  HERMES_RS_SESSION_USD_CAP=1.00 HERMES_RS_DAILY_USD_CAP=2.00 \
  HERMES_RS_CODING_WORKTREE="$WT" HERMES_RS_CODING_BRANCH=swarm/fake-ticket \
  HERMES_RS_CODING_STATE="$RUN/state" HERMES_RS_CODING_TOOLCHAIN="$TOOLCHAIN" \
  "$GW" >"$RUN/gateway.out" 2>"$RUN/gateway.err" &
pids+=($!)

for _ in $(seq 50); do
  curl -sf "http://127.0.0.1:$GW_PORT/health" >/dev/null 2>&1 && curl -sf "http://127.0.0.1:$MOCK_PORT/health" >/dev/null 2>&1 && break
  sleep 0.2
done
say "gateway log:"; sed 's/^/  /' "$RUN/gateway.err"
say "GET /health: $(curl -s "http://127.0.0.1:$GW_PORT/health" | head -c 300)"

chat() { # session script
  curl -s -m 900 "http://127.0.0.1:$GW_PORT/v1/chat/completions" \
    -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
    -H "X-Hermes-Session-Id: $1" \
    -d "{\"model\":\"default\",\"messages\":[{\"role\":\"user\",\"content\":\"Run SCRIPT:$2\"}]}"
}
answer() { sed -n 's/.*"content":"\(\([^"\\]\|\\.\)*\)".*/\1/p' | head -c 600; }

fail=0
check() { if eval "$2"; then say "PASS $1"; else say "FAIL $1"; fail=1; fi; }

say "=== session 1: fake ticket FAKE-1 ==="
chat real-ticket-1 ticket > "$RUN/ticket.resp"
say "assistant: $(answer < "$RUN/ticket.resp")"
G -C "$WT" log --format='  %h %an <%ae> %s' -3 swarm/fake-ticket
check "commit on swarm/fake-ticket by hermes-rs" '[ "$(G -C "$WT" log -1 --format=%an swarm/fake-ticket)" = hermes-rs ]'
check "main untouched" '[ "$(G -C "$WT" log -1 --format=%s main)" = "FAKE-1: buggy adder" ]'
check "origin has no new branch (nothing pushed)" '[ "$(G -C "$RUN/remote.git" for-each-ref --format="%(refname)" | tr "\n" " ")" = "refs/heads/main " ]'
check "src/lib.rs fixed" 'grep -q "a + b" "$WT/src/lib.rs"'
grep '"script":"ticket"' "$RUN/mock.jsonl" | sed 's/"tools_offered":\[[^]]*\],//; s/^/  mock: /' | cut -c1-260

say "=== session 2: escape attempts ==="
chat real-escape-1 escape > "$RUN/escape.resp"
say "assistant: $(answer < "$RUN/escape.resp")"
grep '"script":"escape"' "$RUN/mock.jsonl" | sed 's/.*"last_tool_result":"\(.\{0,200\}\).*"reply":"\([^"]*\)".*/  step result: \1  -> next: \2/'
check "canary intact" '[ "$(cat "$RUN/canary/keep.txt")" = keep ] && [ ! -e "$RUN/canary/pwned.txt" ]'
check "no .git/hooks/pre-commit written" '[ ! -e "$WT/.git/hooks/pre-commit" ]'
check "no .cargo/config.toml written" '[ ! -e "$WT/.cargo/config.toml" ]'
check "origin still only main" '[ "$(G -C "$RUN/remote.git" for-each-ref --format="%(refname)" | tr "\n" " ")" = "refs/heads/main " ]'
ESC=$(grep -o "ESCAPE [a-z_.:0-9-]*: [A-Z]*" "$RUN/mock.jsonl" | sort -u); echo "$ESC" | sed "s/^/  /"
check "escape test ran inside the sandbox: 7 attempts, all BLOCKED" '[ "$(echo "$ESC" | grep -c BLOCKED)" = 7 ] && ! echo "$ESC" | grep -q SUCCEEDED'

say "=== session 3+4+5: spend caps (\$0.60 per mock call; session cap \$1.00, daily \$2.00) ==="
chat real-spend-1 spend > "$RUN/spend1.resp"
say "spend-1: $(head -c 300 "$RUN/spend1.resp")"
chat real-spend-2 spend > "$RUN/spend2.resp"
say "spend-2: $(head -c 300 "$RUN/spend2.resp")"
chat real-spend-3 spend > "$RUN/spend3.resp"
say "spend-3: $(head -c 300 "$RUN/spend3.resp")"
n1=$(grep -c '"script":"spend"' "$RUN/mock.jsonl" || true)
check "mock saw exactly 4 spend calls (2 per session, 0 for the third)" '[ "$n1" = 4 ]'

say "=== audit log summary ($RUN/state/audit.jsonl) ==="
grep -o '"decision":"[a-z]*"' "$RUN/state/audit.jsonl" | sort | uniq -c | sed 's/^/  /'
grep '"refused"' "$RUN/state/audit.jsonl" | sed 's/.*"tool":"\([^"]*\)".*"reason":"\(\([^"\\]\|\\.\)*\)".*/  refused \1: \2/' | cut -c1-200
check "mock never saw a key in Authorization" '! grep -q "\"authorization_header_sent\":true" "$RUN/mock.jsonl"'
check "no non-loopback connection from the gateway" '! grep -q "refusing non-loopback" "$RUN/gateway.err"'

cleanup; trap - EXIT
say "processes stopped; listeners on 1917x: $(ss -ltn 2>/dev/null | grep -c ':1917[0-9] ' || true)"
[ $fail = 0 ] && say "ALL CHECKS PASSED" || { say "SOME CHECKS FAILED"; exit 1; }
