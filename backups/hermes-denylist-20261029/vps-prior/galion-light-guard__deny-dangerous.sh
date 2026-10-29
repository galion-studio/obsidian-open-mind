#!/bin/bash
# Ondrej shared guard. stdin: hook JSON. exit 2 = block.
set -euo pipefail
PATTERNS="${GUARD_PATTERNS:-/workspace/galion-light/guard/dangerous-patterns.txt}"
if test -f "$HOME/.agents/hooks/dangerous-patterns.txt"; then
  PATTERNS="$HOME/.agents/hooks/dangerous-patterns.txt"
fi
payload=$(cat)
mode=${1:-}
if test "$mode" = "cursor"; then
  cmd=$(printf '%s' "$payload" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('command') or '')" 2>/dev/null || true)
else
  cmd=$(printf '%s' "$payload" | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(
  ((d.get('tool_input') or {}).get('command'))
  or ((d.get('toolInput') or {}).get('command'))
  or d.get('command')
  or ''
)
" 2>/dev/null || true)
fi
if test -z "$cmd"; then
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    ""|\#*) continue ;;
  esac
  if printf '%s' "$cmd" | grep -E -- "$line" >/dev/null 2>&1; then
    echo "Blocked by Guard: $line" >&2
    if test "$mode" = "cursor"; then
      printf '%s\n' '{"permission":"deny","reason":"dangerous-command guard"}'
    fi
    exit 2
  fi
done < "$PATTERNS"
exit 0
