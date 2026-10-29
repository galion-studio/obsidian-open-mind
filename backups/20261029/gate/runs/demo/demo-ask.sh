#!/bin/bash
# Root part of the STEP 3 demo: ask live Hermes (cheap alias) for a patch text.
# The API key is read into a variable from the hermes env file and never printed or written.
set -euo pipefail
H=/var/lib/galion-gate; R=$H/runs/demo-1; MODEL=${1:-worker}
install -d -o galion-gate -g galion-gate -m 750 $R
KEY=$(grep -m1 '^API_SERVER_KEY=' /opt/galion/hermes/.env | cut -d= -f2- | tr -d '"'"'"' \r')
[ -n "$KEY" ] || { echo "no API key found"; exit 1; }
python3 - "$R/request.json" "$MODEL" <<'PY'
import json,sys
prompt = """You are proposing a code change that will go through an automated security gate. Do not use tools.
File: crates/hermes-rs/src/tools.rs. Current lines 123-125:
    fn description(&self) -> String {
        "Current time in Europe/Zagreb (RFC 3339 with offset).".into()
    }
Task: propose ONE harmless one-line improvement to the clock tool's description string (wording only; keep it a string literal ending in .into()).
Reply with ONLY a unified diff in a ```diff fenced block, exactly in this shape (fill in the + line):
```diff
--- a/crates/hermes-rs/src/tools.rs
+++ b/crates/hermes-rs/src/tools.rs
@@ -123,3 +123,3 @@ impl Tool for ClockTool {
     fn description(&self) -> String {
-        "Current time in Europe/Zagreb (RFC 3339 with offset).".into()
+        <new line here, same 8-space indent>
     }
```"""
json.dump({"model": sys.argv[2], "max_tokens": 300, "temperature": 0,
           "messages": [{"role": "user", "content": prompt}]}, open(sys.argv[1], "w"))
PY
code=$(curl -s -o $R/reply.json -w '%{http_code}' -m 120 -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -H 'X-Hermes-Session-Id: gate-demo-20261029' --data @$R/request.json http://127.0.0.1:8642/v1/chat/completions)
unset KEY
chown galion-gate:galion-gate $R/request.json $R/reply.json
echo "hermes HTTP $code"
