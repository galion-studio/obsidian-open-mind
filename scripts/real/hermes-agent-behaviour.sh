#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
#
# Zero-spend behaviour test of Hermes's agent tools, live layout, on the BOX:
#   - the real hermes-gateway (release) with the same tool switches as the live
#     drop-ins 99j/99k (guard workspace = files root = tools workspace, files
#     write on, 4096 MB workspace), but every path under a temp folder,
#   - the scripted mock model (hermes-mock-model) instead of OpenRouter:
#     HERMES_RS_PROVIDER_LOOPBACK_ONLY=1, no provider key, cost 0,
#   - session "tools": terminal, read/write/edit/list/search, files_list/read,
#     then attacks (rm -rf /, curl, /etc/shadow, ../ write), a timeout, and a
#     read outside the workspace,
#   - session "script": write a shell script and run it; a script with curl.
#     EXPECT_SCRIPT_MODE=1 expects the good script to run (needs a build with
#     script mode and HERMES_RS_GUARD_SCRIPT_MODE=1); 0 expects both refused.
# Usage: scripts/real/hermes-agent-behaviour.sh            (build this tree)
#        GW=/path/hermes-gateway MOCK=/path/hermes-mock-model \
#        EXPECT_SCRIPT_MODE=1 scripts/real/hermes-agent-behaviour.sh
# Ports 19870 (gateway) / 19871 (mock), 127.0.0.1 only. Kills what it starts.
set -euo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/hermes-tools}
RUN=${RUN:-$CARGO_TARGET_DIR/agent-behaviour-run}
GW_PORT=${GW_PORT:-19870}
MOCK_PORT=${MOCK_PORT:-19871}
EXPECT_SCRIPT_MODE=${EXPECT_SCRIPT_MODE:-0}
FIX=$REPO/crates/hermes-tools/tests/fixtures/agent-mock-scripts
ts() { TZ=Europe/Zagreb date '+%H:%M:%S CET'; }
say() { echo "[$(ts)] $*"; }

for p in $GW_PORT $MOCK_PORT; do
  if ss -ltn 2>/dev/null | grep -q ":$p "; then say "port $p is busy; set GW_PORT/MOCK_PORT"; exit 2; fi
done
if [ -z "${GW:-}" ]; then
  say "build (release, nice, -j 4)"
  nice -n 10 cargo build -q --release -j 4 -p hermes-tools --bin hermes-mock-model
  nice -n 10 cargo build -q --release -j 4 -p hermes-gateway
  GW=$CARGO_TARGET_DIR/release/hermes-gateway
fi
MOCK=${MOCK:-$CARGO_TARGET_DIR/release/hermes-mock-model}
say "gateway $GW ($(stat -c %s "$GW") bytes), expect script mode: $EXPECT_SCRIPT_MODE"

rm -rf "$RUN"; mkdir -p "$RUN"/{home,canary,galion-files}
echo keep > "$RUN/canary/keep.txt"
FILES=$(cd "$RUN/galion-files" && pwd -P)
KEY=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')   # test-only gateway key, never printed

pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; }
trap cleanup EXIT

"$MOCK" --port "$MOCK_PORT" --scripts "$FIX" --log "$RUN/mock.jsonl" --var "CANARY=$RUN/canary" \
  >"$RUN/mock.out" 2>"$RUN/mock.err" &
pids+=($!)
SM_ENV=()
[ "$EXPECT_SCRIPT_MODE" = 1 ] && SM_ENV=(HERMES_RS_GUARD_SCRIPT_MODE=1)
env -i PATH=/usr/bin:/bin LANG=C.UTF-8 TZ=Europe/Zagreb HOME="$RUN/home" \
  HERMES_HOME="$RUN/home" HERMES_RS_HOST=127.0.0.1 HERMES_RS_PORT="$GW_PORT" \
  HERMES_RS_MCP=0 HERMES_RS_CRON=0 API_SERVER_KEY="$KEY" \
  OPENROUTER_BASE_URL="http://127.0.0.1:$MOCK_PORT/v1" HERMES_RS_PROVIDER_LOOPBACK_ONLY=1 \
  HERMES_RS_SESSION_USD_CAP=1.00 HERMES_RS_DAILY_USD_CAP=2.00 \
  HERMES_RS_GUARD_WORKSPACE="$FILES" HERMES_RS_FILES_ROOT="$FILES" HERMES_RS_FILES_WRITE=1 \
  HERMES_RS_TOOLS_WORKSPACE="$FILES" HERMES_RS_TOOLS_WORKSPACE_MB=4096 \
  HERMES_RS_TOOLS_BROWSER=0 HERMES_RS_TOOLS_VISION=0 "${SM_ENV[@]}" \
  "$GW" >"$RUN/gateway.out" 2>"$RUN/gateway.err" &
pids+=($!)
for _ in $(seq 100); do
  curl -sf "http://127.0.0.1:$GW_PORT/health" >/dev/null 2>&1 && curl -sf "http://127.0.0.1:$MOCK_PORT/health" >/dev/null 2>&1 && break
  sleep 0.2
done
grep -E "agent-tools|files" "$RUN/gateway.err" | sed 's/^/  gw: /' | cut -c1-240 || true

chat() { # session script
  curl -s -m 300 "http://127.0.0.1:$GW_PORT/v1/chat/completions" \
    -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
    -H "X-Hermes-Session-Id: $1" \
    -d "{\"model\":\"default\",\"messages\":[{\"role\":\"user\",\"content\":\"Run SCRIPT:$2\"}]}"
}
say "=== session tools ==="; chat behave-tools-1 tools > "$RUN/tools.resp"
say "=== session script ==="; chat behave-script-1 script > "$RUN/script.resp"
cleanup; trap - EXIT

set +e
python3 - "$RUN" "$FILES" "$EXPECT_SCRIPT_MODE" <<'PY'
import json, os, sys
run, files, sm = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
res = {}
for l in open(os.path.join(run, "mock.jsonl")):
    v = json.loads(l)
    res.setdefault(v["script"], {})[v["step"]] = v.get("last_tool_result") or ""
def r(script, step):  # result of tool step `step` (0-based) = seen at step+1
    return res.get(script, {}).get(step + 1, "")
def refused(t): return '"refused' in t or "deny list" in t or "refused" in t.lower()
fail = 0
def check(name, ok, detail=""):
    global fail
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else "  <- " + detail[:300]))
    fail |= not ok
offered = json.loads(open(os.path.join(run, "mock.jsonl")).readline())["tools_offered"]
print("tools offered:", ",".join(sorted(offered)))
for t in ["terminal", "read_file", "write_file", "edit_file", "list_files", "search_files", "files_list", "files_read"]:
    check(f"tool {t} offered", t in offered)
T = "tools"
check("terminal runs in the files root", "hello-from-terminal" in r(T,0) and files in r(T,0), r(T,0))
check("write_file", '"bytes"' in r(T,1) and not refused(r(T,1)), r(T,1))
check("read_file", "alpha beta" in r(T,2), r(T,2))
check("edit_file", open(os.path.join(files, "notes/a.txt")).read() == "gamma beta\n", r(T,3))
check("list_files", "a.txt" in r(T,4), r(T,4))
check("search_files", "a.txt" in r(T,5), r(T,5))
check("files_list (galion-files)", "a.txt" in r(T,6), r(T,6))
check("files_read (galion-files)", "gamma beta" in r(T,7), r(T,7))
check("rm -rf / refused", refused(r(T,8)), r(T,8))
check("curl refused", refused(r(T,9)), r(T,9))
check("/etc/shadow refused", refused(r(T,10)) or "denied" in r(T,10).lower(), r(T,10))
check("../ write refused", "error" in r(T,11).lower() or refused(r(T,11)), r(T,11))
check("no file escaped the root", not os.path.exists(os.path.join(os.path.dirname(files), "escape.txt")))
check("timeout_s honoured", '"timed_out":true' in r(T,12).replace(" ", ""), r(T,12))
check("outside the workspace is hidden or refused", "keep.txt" not in r(T,13), r(T,13))
check("tools session answered", "Tools script finished" in open(os.path.join(run, "tools.resp")).read())
S = "script"
good, bad = r(S,1), r(S,3)
if sm:
    check("script mode: good script runs", "wrote 3 lines" in good, good)
    check("script mode: output file written", os.path.exists(os.path.join(files, "count-3.txt")), good)
else:
    check("strict mode: script refused", refused(good) and "wrote 3 lines" not in good, good)
check("script with curl refused", refused(bad), bad)
check("script session answered", "Script script finished" in open(os.path.join(run, "script.resp")).read())
check("no key ever sent to the mock", all('"authorization_header_sent":true' not in l for l in open(os.path.join(run, "mock.jsonl"))))
check("canary intact", open(os.path.join(run, "canary/keep.txt")).read() == "keep\n")
sys.exit(1 if fail else 0)
PY
rc=$?
say "listeners left on $GW_PORT/$MOCK_PORT: $(ss -ltn 2>/dev/null | grep -cE ":($GW_PORT|$MOCK_PORT) " || true)"
[ $rc = 0 ] && say "ALL CHECKS PASSED" || say "SOME CHECKS FAILED (evidence in $RUN)"
exit $rc
