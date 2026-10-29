#!/bin/bash
# galion-gate part of the demo: extract the patch from the reply, propose, gate. No human step.
set -uo pipefail
H=/var/lib/galion-gate; R=$H/runs/demo-1; G=/opt/galion/gate/bin/hermes-gate; cd $H
python3 - $R/reply.json $R/patch.diff <<'PY'
import json,re,sys
r=json.load(open(sys.argv[1])); t=r["choices"][0]["message"]["content"] or ""
m=re.search(r"```(?:diff|patch)?\n(.*?)```", t, re.S)
p=(m.group(1) if m else t).rstrip("\n")+"\n"
if "diff --git" not in p:
    p="diff --git a/crates/hermes-rs/src/tools.rs b/crates/hermes-rs/src/tools.rs\n"+p
open(sys.argv[2],"w").write(p); print("extracted", len(p.splitlines()), "lines; usage", json.dumps(r.get("usage",{})), "model", r.get("model"))
PY
B=$(git -C $H/repo rev-parse HEAD)
$G propose --base-commit $B --patch $R/patch.diff --id clock-desc-hermes-demo --summary "Hermes demo: clock tool description" --tier low --out $R/proposal.json
$G gate --repo $H/repo --scratch $H/scratch --proposal $R/proposal.json --plan $H/plans/plan.json --key $H/keys/sandbox.seed --out $R/record.json
echo "gate exit $?"
