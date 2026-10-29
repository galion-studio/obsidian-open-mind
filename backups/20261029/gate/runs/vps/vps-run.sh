#!/bin/bash
# as galion-gate on the VPS: propose + gate the one-line clock change. No live path is read or written.
set -uo pipefail
H=/var/lib/galion-gate; G=/opt/galion/gate/bin/hermes-gate; R=$H/runs/$1; cd $H
B=$(git -C $H/repo rev-parse HEAD)
$G propose --base-commit $B --patch $R/clock.patch --id ${2:-clock-desc-vps} --summary "clock tool: clearer description" --tier low --out $R/proposal.json
$G gate --repo $H/repo --scratch $H/scratch --proposal $R/proposal.json --plan $H/plans/plan.json --key $H/keys/sandbox.seed --out $R/record.json
echo "gate exit $?"
