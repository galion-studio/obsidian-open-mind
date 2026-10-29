#!/bin/bash
set -euo pipefail
G=/workspace/galion-light/guard/deny-dangerous.sh
fail=0
expect() {
  local json=$1 want=$2 name=$3
  set +e
  echo "$json" | bash "$G" >/dev/null 2>&1
  got=$?
  set -e
  if test "$got" -eq "$want"; then
    echo "ok $name"
  else
    echo "FAIL $name exit=$got want=$want"
    fail=$((fail + 1))
  fi
}
expect '{"tool_input":{"command":"rm -rf /"}}' 2 "rm-root"
expect '{"tool_input":{"command":"git push --force"}}' 2 "force-push"
expect '{"tool_input":{"command":"ls -la"}}' 0 "ls"
expect '{"tool_input":{"command":"git status"}}' 0 "git-status"
expect '{"tool_input":{"command":"rm -rf node_modules"}}' 0 "rm-node_modules"
expect '{"tool_input":{"command":"cat /workspace/galion-light/secrets/hermes.key"}}' 2 "secret-file"
echo "failed: $fail"
exit $fail
