#!/bin/bash
cd /workspace/swarm/tmp/gate-1029
G=$PWD/target/release/hermes-gate
for i in 1 2 3; do exec 9>/workspace/swarm/locks/build-$i.lock; flock -n 9 && break; done
flock -n 9 || flock 9
echo "slot held $(date '+%F %T %Z')"
nice -n 10 $G gate --repo $PWD/repo --scratch $PWD/run-box/scratch --proposal run-box/proposal.json --plan run-box/plan.json --key run-box/keys/sandbox.seed --out run-box/record.json
echo "gate exit $? $(date '+%F %T %Z')"
