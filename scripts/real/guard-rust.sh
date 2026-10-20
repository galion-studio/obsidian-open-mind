#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# Real-life test for galion-guard (swarm slot 38, ports 19380-19389).
# Builds the release binary, then runs everything inside an unprivileged
# user + network namespace (`unshare -rn`): a real kernel nf_tables ruleset,
# a stand-in "sshd" on 10.99.0.1:19381, attackers and an allowlisted admin
# as extra addresses on the namespace's lo, synthesized auth.log lines in
# the VPS shapes. Nothing touches the host firewall or the VPS.
#
# nft: uses $NFT_ROOT/usr/sbin/nft (Debian nftables .debs unpacked with
# dpkg -x, no root needed); falls back to the dry-run backend without it.
set -euo pipefail
export PATH=/workspace/swarm/bin:$PATH

if [ "${1:-}" != "--inside" ]; then
  ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
  export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/swarm/target/guard-rust}
  export CARGO_INCREMENTAL=0
  (cd "$ROOT" && nice -n 10 cargo build --release -q -p galion-guard --bin galion-guard --example real_peer -j 3)
  export BIN=$CARGO_TARGET_DIR/release/galion-guard
  export PEER=$CARGO_TARGET_DIR/release/examples/real_peer
  export NFT_ROOT=${NFT_ROOT:-$CARGO_TARGET_DIR/nftdeb/root}
  if [ ! -x "$NFT_ROOT/usr/sbin/nft" ] && [ "${GUARD_FETCH_NFT:-1}" = 1 ]; then
    d=$(dirname "$NFT_ROOT"); mkdir -p "$d"
    (cd "$d" && timeout 180 apt-get download nftables libnftables1 libnftnl11 libjansson4 libedit2 libbsd0 libmd0 >/dev/null 2>&1 \
      && for f in *.deb; do dpkg -x "$f" root; done) || echo "note: could not fetch nftables debs; dry-run backend"
  fi
  export WORK=$(mktemp -d /tmp/guard-rust-real.XXXXXX)
  rc=0
  unshare -rn bash "$0" --inside || rc=$?
  rm -rf "$WORK"
  exit $rc
fi

# ---------------------------------------------------------------- inside
PASS=0; FAILN=0
say() { echo "[$(date +%H:%M:%S%:z)] $*"; }
ok() { PASS=$((PASS+1)); say "PASS $*"; }
bad() { FAILN=$((FAILN+1)); say "FAIL $*"; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }

ip link set lo up
for a in 10.99.0.1 10.99.0.7 10.99.0.8 10.99.0.66 10.99.0.67 10.99.0.68 10.99.0.69 10.99.0.70; do ip addr add $a/32 dev lo; done

BACKEND=dry-run
if [ -x "$NFT_ROOT/usr/sbin/nft" ]; then
  printf '#!/bin/sh\nLD_LIBRARY_PATH=%s exec %s "$@"\n' "$NFT_ROOT/usr/lib/x86_64-linux-gnu" "$NFT_ROOT/usr/sbin/nft" > "$WORK/nft"
  chmod +x "$WORK/nft"
  BACKEND=nft
fi
NFT="$WORK/nft"
say "backend=$BACKEND work=$WORK"

LOG=$WORK/auth.log; : > "$LOG"
CFG=$WORK/guard.toml
write_cfg() { # $1 source  $2 ignoreself
  cat > "$CFG" <<CFGEOF
[guard]
backend = "$BACKEND"
nft = "$NFT"
table = "galion-guard"
dry_run_log = "$WORK/dry-run.nft"
state = "$WORK/state/state.json"
socket = "$WORK/run/control.sock"
ignoreself = $2
save_every = 2
[sshd]
source = "$1"
logpath = "$LOG"
journalctl = "$WORK/fake-journalctl"
journal_match = ["_COMM=sshd"]
poll_ms = 200
maxretry = 5
findtime = "60s"
bantime = "20s"
bantime_increment = true
purge_age = "1d"
protect_accepted = "24h"
ignoreip = ["127.0.0.1/8", "::1", "10.99.0.7"]
ports = [19381]
CFGEOF
}

"$PEER" 10.99.0.1:19381 2>/dev/null & PEERPID=$!
DPID=
start() { "$BIN" run --config "$CFG" >>"$WORK/daemon.log" 2>&1 & DPID=$!; for _ in $(seq 50); do [ -S "$WORK/run/control.sock" ] && "$BIN" status --config "$CFG" >/dev/null 2>&1 && return 0; sleep 0.1; done; bad "daemon start"; }
stop() { "$BIN" stop --config "$CFG" >/dev/null 2>&1 || true; wait "$DPID" 2>/dev/null || true; DPID=; }
cleanup() {
  if [ -n "$DPID" ]; then kill -9 "$DPID" 2>/dev/null || true; fi
  kill "$PEERPID" 2>/dev/null || true
  if [ "$BACKEND" = nft ]; then "$NFT" delete table inet galion-guard 2>/dev/null || true; fi
}
trap cleanup EXIT

ts() { date -u +%Y-%m-%dT%H:%M:%S.%6N+00:00; }
N=1000
fail_line() { N=$((N+1)); echo "$(ts) box sshd[$N]: Invalid user u$N from $1 port $((40000+N))"; echo "$(ts) box sshd[$N]: Connection closed by invalid user u$N $1 port $((40000+N)) [preauth]"; }
fails() { local ip=$1 n=$2 f=${3:-$LOG}; for _ in $(seq "$n"); do fail_line "$ip" >> "$f"; done; sleep 0.8; }
reach() { local out; out=$(curl -s --max-time 2 --interface "$1" telnet://10.99.0.1:19381 </dev/null 2>/dev/null || true); case $out in *SSH-2.0*) echo OPEN;; *) echo BLOCKED;; esac; }
sj() { "$BIN" status --json --config "$CFG" | jq -r "$1"; }
inset() { if [ "$BACKEND" = nft ]; then "$NFT" list set inet galion-guard addr4 | grep -q "$1"; else sj ".bans[].ip" | grep -qx "$1"; fi; }
eq() { [ "$1" = "$2" ] || { say "   expected [$2] got [$1]"; return 1; }; }

write_cfg file false
start
say "== P1 setup"
if [ "$BACKEND" = nft ]; then
  "$NFT" list table inet galion-guard | sed 's/^/   | /'
  check "nft table/chain/sets created with timeout flag" bash -c "'$NFT' list table inet galion-guard | grep -q 'flags timeout'"
fi
check "attacker reaches the stand-in sshd before any failure" eq "$(reach 10.99.0.66)" OPEN

say "== P2 ban after maxretry=5 within findtime"
fails 10.99.0.66 4
check "4 failures: not banned" eq "$(sj .currently_banned)" 0
check "4 failures counted" eq "$(sj .total_failed)" 4
T_BAN=$(date +%s)
fails 10.99.0.66 1
check "5th failure: banned" eq "$(sj .currently_banned)" 1
check "ban is in the kernel set" inset 10.99.0.66
[ "$BACKEND" = nft ] && "$NFT" list set inet galion-guard addr4 | grep elements | sed 's/^/   | /'
check "attacker now rejected" eq "$(reach 10.99.0.66)" BLOCKED
check "allowlisted admin still connects" eq "$(reach 10.99.0.7)" OPEN
check "unrelated address still connects" eq "$(reach 10.99.0.8)" OPEN
UNTIL=$(sj '.bans[0].until'); say "   ban until $UNTIL, ban_number $(sj '.bans[0].ban_number')"

say "== P3 never ban loopback / allowlist"
fails 10.99.0.7 10; fails 127.0.0.1 10; fails ::1 10
check "still exactly one ban" eq "$(sj .currently_banned)" 1
check "30 protected hits" eq "$(sj .protected_hits)" 30
check "manual ban of loopback refused" bash -c "! '$BIN' ban 127.0.0.1 --config '$CFG' >/dev/null 2>&1"
check "manual ban of allowlisted admin refused" bash -c "! '$BIN' ban 10.99.0.7 --config '$CFG' >/dev/null 2>&1"
check "daemon log never names a protected address" bash -c "! grep -E '10\.99\.0\.7|127\.0\.0\.1' '$WORK/daemon.log'"

say "== P4 restart persistence (clean stop, then kill -9)"
stop
check "ban survives in the kernel while the daemon is down" inset 10.99.0.66
check "attacker still rejected with the daemon down" eq "$(reach 10.99.0.66)" BLOCKED
start
check "ban restored from state after clean stop" eq "$(sj '.bans[0].ip')" 10.99.0.66
kill -9 "$DPID"; wait "$DPID" 2>/dev/null || true; DPID=
start
check "ban restored after kill -9" eq "$(sj '.bans[0].ip')" 10.99.0.66
check "until unchanged across restarts" eq "$(sj '.bans[0].until')" "$UNTIL"

say "== P5 expiry on time"
while [ "$(sj .currently_banned)" != 0 ]; do sleep 0.25; [ $(( $(date +%s) - T_BAN )) -gt 40 ] && break; done
T_UNBAN=$(date +%s); LEN=$((T_UNBAN - T_BAN)); say "   unbanned after ${LEN}s (bantime 20s)"
check "unban within 20..22 s" bash -c "[ $LEN -ge 19 ] && [ $LEN -le 22 ]"
check "kernel set no longer has it" bash -c "! { $( declare -f inset sj ); BACKEND=$BACKEND NFT='$NFT' BIN='$BIN' CFG='$CFG'; inset 10.99.0.66; }"
check "attacker reaches sshd again" eq "$(reach 10.99.0.66)" OPEN

say "== P6 increment (2nd ban = 2 x bantime) and CLI unban"
fails 10.99.0.66 5
check "second ban is ban #2" eq "$(sj '.bans[0].ban_number')" 2
REM=$(sj '.bans[0].remaining_s'); say "   remaining ${REM}s"
check "second ban lasts 40 s" bash -c "[ $REM -ge 38 ] && [ $REM -le 40 ]"
"$BIN" unban 10.99.0.66 --config "$CFG" | sed 's/^/   | /'
check "CLI unban lifts it at once" eq "$(reach 10.99.0.66)" OPEN
check "status shows no bans" eq "$(sj .currently_banned)" 0

say "== P7 log rotation (rename; late lines in the old file)"
mv "$LOG" "$LOG.1"; : > "$LOG"
fails 10.99.0.67 2 "$LOG.1"
fails 10.99.0.67 3
check "failures across rotation add up to a ban" eq "$(sj '.bans[] | select(.ip=="10.99.0.67") | .ip')" 10.99.0.67
: > "$WORK/trunc"; cp "$LOG" "$WORK/trunc"; : > "$LOG"   # copytruncate
fails 10.99.0.8 5
check "copy-truncate: new lines still read" eq "$(sj '.bans[] | select(.ip=="10.99.0.8") | .ip')" 10.99.0.8
"$BIN" unban 10.99.0.8 --config "$CFG" >/dev/null

say "== P8 successful login protects a roaming admin"
echo "$(ts) box sshd[7777]: Accepted publickey for root from 10.99.0.68 port 50000 ssh2: ED25519 SHA256:AAAAexample" >> "$LOG"
fails 10.99.0.68 12
check "no ban after a recent successful login" bash -c "[ -z \"\$('$BIN' status --json --config '$CFG' | jq -r '.bans[] | select(.ip==\"10.99.0.68\") | .ip')\" ]"

say "== P9 kernel timeout lifts bans even if the daemon is dead"
kill -9 "$DPID"; wait "$DPID" 2>/dev/null || true; DPID=
if [ "$BACKEND" = nft ]; then
  check "10.99.0.67 still blocked right after the crash" eq "$(reach 10.99.0.67)" BLOCKED
  sleep 21
  check "10.99.0.67 open again with no daemon running (kernel timeout)" eq "$(reach 10.99.0.67)" OPEN
fi
start
check "restart after expiry: stale ban dropped" eq "$(sj '[.bans[] | select(.ip=="10.99.0.67")] | length')" 0
stop

say "== P10 ignoreself: this host's own addresses are never banned"
write_cfg file true; start
fails 10.99.0.69 8
check "local address not banned" eq "$(sj '[.bans[] | select(.ip=="10.99.0.69")] | length')" 0
check "self addresses detected" bash -c "[ \$('$BIN' status --json --config '$CFG' | jq .self_addresses) -ge 8 ]"
stop

say "== P11 journald source (real child process; fake journalctl replays JSON)"
J=$WORK/journal.jsonl; : > "$J"
cat > "$WORK/fake-journalctl" <<FJ
#!/bin/sh
echo "\$*" >> "$WORK/jargs"
exec tail -n +1 -f "$J"
FJ
chmod +x "$WORK/fake-journalctl"
write_cfg journal false; start
for i in 1 2 3 4 5; do
  us=$(( $(date +%s) * 1000000 + i ))
  printf '{"__CURSOR":"s=guard;i=%d","__REALTIME_TIMESTAMP":"%s","_HOSTNAME":"box","SYSLOG_IDENTIFIER":"sshd","_PID":"%d","MESSAGE":"Invalid user j%d from 10.99.0.70 port %d"}\n' $i $us $((900+i)) $i $((41000+i)) >> "$J"
done
sleep 1.5
check "journal entries ban 10.99.0.70" eq "$(sj '[.bans[] | select(.ip=="10.99.0.70")] | length')" 1
stop; start
check "journalctl restarted from the saved cursor" grep -q -- "--after-cursor=s=guard;i=5" "$WORK/jargs"
check "first start used --since (findtime backlog)" grep -q -- "--since=@" "$WORK/jargs"
sed 's/^/   | args: /' "$WORK/jargs"
stop

say "== P12 teardown"
"$BIN" teardown --config "$CFG"
if [ "$BACKEND" = nft ]; then check "table removed" bash -c "! '$NFT' list table inet galion-guard >/dev/null 2>&1"; fi

say "daemon log (last 12 lines):"; tail -12 "$WORK/daemon.log" | sed 's/^/   | /'
say "RESULT: $PASS passed, $FAILN failed (backend=$BACKEND)"
trap - EXIT; cleanup
[ "$FAILN" = 0 ]
