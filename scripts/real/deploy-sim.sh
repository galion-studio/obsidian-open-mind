#!/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio
#
# scripts/real/deploy-sim.sh [tuner|nerve|room|desk|all]
# Cutover rehearsals on the BOX (no systemd: PID 1 is tini). simsup plays systemd,
# galion-deploy runs with GALION_DEPLOY_SM=direct, a probe measures downtime.
# Ports 19330-19339 only (slot 33). Nothing touches the VPS. Every process it
# starts is stopped at the end. Run scripts/real/deploy-sim-build.sh first.
set -uo pipefail
export PATH=/workspace/swarm/bin:$PATH
ulimit -c 0
HERE=$(cd "$(dirname "$0")/../.." && pwd)
SIM=${SIM:-/workspace/swarm/sim/deploy-sim}
BIN=$SIM/bin
GD=$HERE/deploy/foundation/galion-deploy
RUNS=$SIM/runs; mkdir -p "$RUNS"
FAILS=0

now() { TZ=Europe/Zagreb date +%Y-%m-%dT%H:%M:%S.%3N%:z; }
say() { echo; echo "[$(now)] == $*"; }
x() { echo "\$ $*"; "$@"; local rc=$?; echo "  -> exit=$rc"; return $rc; }
check() { # check NAME CONDITION-RESULT(0/1) DETAIL
  if [ "$2" = 0 ]; then echo "CHECK PASS  $1 ($3)"; else echo "CHECK FAIL  $1 ($3)"; FAILS=$((FAILS+1)); fi
  echo "$1|$([ "$2" = 0 ] && echo PASS || echo FAIL)|$3" >> "$R/checks"
}
ms_since() { echo $(( ($(date +%s%N) - $1) / 1000000 )); }
code() { curl -s -o /dev/null -m 3 -w '%{http_code}' "$1"; }
ctl() { "$BIN/simsup" --root "$R" ctl "$@"; }
sha() { sha256sum "$1" | cut -c1-16; }
listeners() { ss -ltnpH "sport = :$1" 2>/dev/null | sed 's/  */ /g'; }
# PIDs of "<file> --version" processes (fixed-string match: backup names contain "+")
vpids() { local e; e=$(printf "%s" "$1" | sed "s/[][\\.*^$+?(){}|]/\\\\&/g"); pgrep -f "^$e --version" | tr "\n" " " | sed "s/ $//"; }

# DENV: extra environment for the deploy, so that a "--version" which ignores its
# argument binds the SAME address it would on the VPS (there the default port is the
# live port; in the sim the live port is 1933x, so the default must point there too).
DENV=()
deploy() {
  env "${DENV[@]}" GALION_DEPLOY_SM=direct GALION_DEPLOY_CTL="$BIN/simsup --root $R ctl" \
  GALION_DEPLOY_BACKUP_DIR="$R/var/backups/galion/deploy" \
  GALION_DEPLOY_DATA_BACKUP="$R/opt/galion/rust/bin/galion-backup.sh" \
  "$GD" "$@"
}
# tdeploy LIMIT ARGS: run deploy, print elapsed, set DRC to its exit code
tdeploy() {
  local lim=$1; shift; local t=$(date +%s%N)
  echo "\$ galion-deploy $*   (direct backend${GALION_DEPLOY_VERSION_TIMEOUT:+, GALION_DEPLOY_VERSION_TIMEOUT=$GALION_DEPLOY_VERSION_TIMEOUT}, outer timeout ${lim}s)"
  timeout "$lim" bash -c "$(declare -f deploy); $(declare -p DENV); R='$R' BIN='$BIN' GD='$GD' deploy \"\$@\"" _ "$@"
  DRC=$?; echo "  -> exit=$DRC elapsed_ms=$(ms_since "$t")"
}
bg_deploy() { ( tdeploy "$@" ) > "$R/bg.out" 2>&1 & BGP=$!; }
bg_wait() { wait "$BGP"; cat "$R/bg.out"; DRC=$(grep -o "exit=[0-9]*" "$R/bg.out" | tail -1 | cut -d= -f2); }
probe_start() { rm -f "$R/probe.stop"; "$BIN/simsup" probe "$1" "$2" "${3:-2}" "$R/probe.stop" 300 > "$R/probe.out" & PROBE=$!; sleep 0.3; }
probe_stop() {
  touch "$R/probe.stop"; wait "$PROBE"
  sed 's/^/  probe| /' "$R/probe.out"
  # outage of the last window that ended with an OK answer
  DOWN=$(grep -o 'until_ok_ms=[0-9]*' "$R/probe.out" | cut -d= -f2 | sort -n | tail -1)
  NEVER=$(grep -c 'next_ok=never' "$R/probe.out")
  DOWN=${DOWN:-0}
  # a window that never ended in an OK answer counts with its whole fail span
  local nspan; nspan=$(grep "next_ok=never" "$R/probe.out" | grep -o "fail_span_ms=[0-9]*" | cut -d= -f2 | sort -n | tail -1)
  if [ -n "$nspan" ]; then DOWN=$(( nspan > DOWN ? nspan : DOWN )); echo "  probe| NOTE: service was still down when the probe stopped"; fi
}
wait_code() { # wait_code URL CODE SECONDS
  local i; for i in $(seq 1 $(( $3 * 10 ))); do [ "$(code "$1")" = "$2" ] && return 0; sleep 0.1; done; return 1
}

sim_up() { # sim_up NAME -> fresh root $R with units already written by the caller hook
  R=$SIM/root-$1; rm -rf "$R"; mkdir -p "$R/units" "$R/opt/galion/rust/bin" "$R/var/backups/galion"
  : > "$R/checks"
  # per-rehearsal copies, so parallel rehearsals never match each other's processes
  mkdir -p "$R/bad"; install -m 0755 "$BIN/sim-broken" "$BIN/sim-hang" "$R/bad/"
  BROKEN=$R/bad/sim-broken; HANG=$R/bad/sim-hang
  cat > "$R/opt/galion/rust/bin/galion-backup.sh" <<'B'
#!/bin/sh
# sim stand-in for galion-backup.sh: the real data snapshot takes about 3 s.
echo "galion-backup.sh (sim stub): sleeping 3 s like the real snapshot"; sleep 3
B
  chmod 0755 "$R/opt/galion/rust/bin/galion-backup.sh"
}
sup_start() {
  "$BIN/simsup" --root "$R" daemon 2>>"$R/simsup.log" & SUP=$!
  local i; for i in $(seq 1 50); do [ -S "$R/ctl.sock" ] && break; sleep 0.1; done
  echo "simsup pid $SUP, root $R"
}
sup_stop() {
  ctl shutdown >/dev/null 2>&1; wait "$SUP" 2>/dev/null
  echo "simsup stopped; leftover listeners on 19330-19339: $(ss -ltnH '( sport >= :19330 and sport <= :19339 )' | wc -l)"
}
summary() {
  echo; echo "== summary $1: $(grep -c '|PASS|' "$R/checks") pass, $(grep -c '|FAIL|' "$R/checks") fail"
  sed 's/^/  /' "$R/checks"
}

# ---------------------------------------------------------------- tuner
tuner() {
  sim_up tuner
  local P=19330 H=http://127.0.0.1:19330/health T
  T=$R/opt/galion/tuner/bin/tuner
  DENV=(TUNER_ADDR=127.0.0.1:$P TUNER_ROOT=$R/opt/galion/tuner/www)
  install -d "$R/opt/galion/tuner/bin" "$R/opt/galion/tuner/www"
  install -m 0755 "$BIN/tuner-old" "$T"
  cp "$HERE"/crates/tuner/www/{index.html,app.js,pitch.js,style.css} "$R/opt/galion/tuner/www/"
  # The live unit text (docs/cutover/tuner.md section 1) with sim paths and port.
  cat > "$R/units/tuner.service" <<U
[Unit]
Description=tuner (sim copy of /etc/systemd/system/tuner.service)
[Service]
ExecStart=$T
Environment=TUNER_ADDR=127.0.0.1:$P
Environment=TUNER_ROOT=$R/opt/galion/tuner/www
Restart=always
RestartSec=2
Nice=10
NoNewPrivileges=true
[Install]
WantedBy=multi-user.target
U
  sup_start
  wait_code "$H" 200 5; check "old tuner serving before cutover" $? "$(code "$H") on $H"
  local OLD NEW ART=/workspace/swarm/artifacts/tuner STAGE=$R/var/tmp/galion-cutover
  OLD=$(sha "$BIN/tuner-old"); NEW=$(sha "$ART/tuner-x86_64-unknown-linux-musl")

  say "3.1 copy (sim: into $STAGE, the stand-in for /var/tmp/galion-cutover)"
  install -d -m 0700 "$STAGE"
  cp "$ART/tuner-x86_64-unknown-linux-musl" "$ART/SHA256SUMS" "$STAGE/"
  (cd "$STAGE" && x sha256sum -c --ignore-missing SHA256SUMS && chmod 0755 tuner-x86_64-unknown-linux-musl)
  check "3.1 artefact sha256 verified" $? "sha $NEW"
  local N=$STAGE/tuner-x86_64-unknown-linux-musl

  say "3.2 preflight (read-only, no bind)"
  echo "installed sha: $(sha "$T") (sim 'live' = main 13a296c build, stands in for ecad72a4)"
  x ctl is-active tuner; echo "health: $(curl -s -m 3 "$H")"
  local v; v=$("$N" --version); echo "--version: $v"
  check "3.2 --version answers without binding" "$([ "$v" = "tuner 0.1.0" ]; echo $?)" "$v"
  TUNER_ROOT=$R/opt/galion/tuner/www x "$N" --check; check "3.2 --check" $? "4 files readable"

  say "3.3 swap: galion-deploy tuner <new> <health>  (probe every 2 ms)"
  probe_start 127.0.0.1:$P /health 2
  tdeploy 60 tuner "$N" "$H"
  probe_stop
  check "3.3 deploy exit 0" "$DRC" "exit $DRC"
  check "3.3 downtime < 1 s" "$([ "$DOWN" -lt 1000 ]; echo $?)" "longest outage ${DOWN} ms"
  echo "DOWNTIME tuner swap ${DOWN} ms" >> "$R/downtime"
  local BAK; BAK=$(ls -1t "$R"/var/backups/galion/deploy/tuner-*.bin | head -1)
  echo "backup of the old binary: $BAK"

  say "3.4 health after the switch"
  x ctl is-active tuner; ctl show tuner | grep -E 'MainPID|NRestarts|ActiveState'
  local pid; pid=$(ctl show tuner | sed -n 's/^MainPID=//p')
  echo "exe: $(readlink "/proc/$pid/exe")"
  check "3.4 running binary is the new sha" "$([ "$(sha "/proc/$pid/exe")" = "$NEW" ]; echo $?)" "$(sha "/proc/$pid/exe")"
  curl -s -m 3 -i "$H" | tr -d '\r' | sed 's/^/  | /'
  local root404 rootline
  rootline=$(curl -s -m 3 -o /dev/null -w '%{http_code} %{size_download}' "http://127.0.0.1:$P/")
  root404=$(code "http://127.0.0.1:$P/nope")
  echo "/ -> $rootline ; /nope -> $root404"
  check "3.4 / is 200 2356 and /nope is 404" "$([ "$rootline" = "200 2356" ] && [ "$root404" = 404 ]; echo $?)" "$rootline / $root404"
  ctl journal tuner 3 | sed 's/^/  journal| /'
  # 21-route byte diff against the live fixtures (old binary bytes, captured from VPS 8650)
  grep -v '^#' "$HERE/scripts/real/tuner-requests.txt" | "$HERE/scripts/real/tuner-capture.sh" "$P" > "$R/routes.b64" 2>"$R/capture.err"
  echo "route capture: exit ${PIPESTATUS[1]}, $(wc -l < "$R/routes.b64") responses"
  local same=0 tot=0 nm b64
  while read -r nm b64; do
    tot=$((tot+1))
    if [ -f "$HERE/crates/tuner/tests/fixtures/live/$nm.http" ] && cmp -s <(echo "$b64" | base64 -d) "$HERE/crates/tuner/tests/fixtures/live/$nm.http"; then same=$((same+1)); else echo "  route DIFF: $nm"; fi
  done < "$R/routes.b64"
  [ -s "$R/capture.err" ] && sed 's/^/  capture-stderr| /' "$R/capture.err"
  check "3.4 all routes byte-identical to the live fixtures" "$([ "$same" = "$tot" ] && [ "$tot" -gt 0 ]; echo $?)" "$same/$tot"

  say "rollback level 2 (manual): galion-deploy tuner <backup> <health>"
  probe_start 127.0.0.1:$P /health 2
  tdeploy 60 tuner "$BAK" "$H"
  probe_stop
  check "L2 manual rollback exit 0 and old sha back" "$([ "$DRC" = 0 ] && [ "$(sha "$T")" = "$OLD" ]; echo $?)" "exit $DRC, sha $(sha "$T")"
  echo "DOWNTIME tuner rollback-L2 ${DOWN} ms" >> "$R/downtime"
  ctl journal tuner 30 | grep -m2 -i 'panicked\|bind' | sed 's/^/  journal| /'

  say "forward again, then rollback level 3 (last resort, no galion-deploy)"
  tdeploy 60 tuner "$N" "$H"; check "re-deploy new exit 0" "$DRC" "exit $DRC"
  probe_start 127.0.0.1:$P /health 2
  local t3=$(date +%s%N)
  x install -m 0755 "$BAK" "$T.new" && x mv -f "$T.new" "$T" && x ctl restart tuner
  wait_code "$H" 200 5; echo "health after manual restart: $(curl -s -m 3 "$H") ($(ms_since "$t3") ms for the whole level-3 sequence)"
  probe_stop
  check "L3 last resort: old sha back and healthy" "$([ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "sha $(sha "$T")"
  echo "DOWNTIME tuner rollback-L3 ${DOWN} ms" >> "$R/downtime"

  # l1 LABEL FILE: automatic rollback. systemd's default start limit (5 starts per 10 s,
  # fixed window) and RestartSec=2 race with galion-deploy's 20 s health loop: if the
  # rollback restart lands in a full window it is refused, galion-deploy dies under
  # set -e (exit 1, the same code as "rolled back ok") and the unit stays failed.
  l1() {
    local label=$1 file=$2 before; before=$(grep -c start-limit-hit "$R/journal/tuner.log")
    ctl reset-failed tuner >/dev/null
    probe_start 127.0.0.1:$P /health 5
    tdeploy 90 tuner "$file" "$H"
    probe_stop
    ctl journal tuner 60 | grep -E 'SEGV|203/EXEC|exited, code' | tail -2 | sed 's/^/  journal| /'
    if [ "$(grep -c start-limit-hit "$R/journal/tuner.log")" -gt "$before" ] && [ "$(ctl is-active tuner)" = failed ]; then
      echo "TRAP: rollback restart refused by the start limit; unit $(ctl is-active tuner), sha on disk $(sha "$T"), /health $(code "$H")"
      echo "TRAP $label" >> "$R/traps"
      local t=$(date +%s%N); x ctl reset-failed tuner; x ctl restart tuner; wait_code "$H" 200 5
      check "$label: start-limit trap fired; recovered by reset-failed + restart" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, down until manual recovery, recovery $(ms_since "$t") ms"
    else
      check "$label: rolled back ok (exit 1), old sha, healthy" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
    fi
    echo "DOWNTIME tuner $label ${DOWN} ms" >> "$R/downtime"
  }
  : > "$R/traps"
  head -c 65536 "$N" > "$STAGE/tuner-truncated"; chmod 0755 "$STAGE/tuner-truncated"
  say "rollback level 1 (automatic): broken binary (valid ELF, exits 1 at start)"
  l1 "L1 broken binary" "$BROKEN"
  say "rollback level 1 (automatic): truncated ELF (first 64 KiB of the new tuner; crashes at start)"
  l1 "L1 truncated ELF" "$STAGE/tuner-truncated"
  say "rollback level 1 with GALION_DEPLOY_RESET_FAILED=1 (opt-in): the rollback restart cannot be refused"
  for i in 1 2 3; do
    GALION_DEPLOY_RESET_FAILED=1 tdeploy 90 tuner "$STAGE/tuner-truncated" "$H"
    check "L1 truncated ELF + RESET_FAILED run $i: rolled back ok" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC"
  done
  echo "start-limit traps without the opt-in in this run: $(wc -l < "$R/traps")"

  say "hang A: binary that hangs on --version, galion-deploy as shipped (no timeout)"
  ctl reset-failed tuner >/dev/null
  local nb; nb=$(ls "$R"/var/backups/galion/deploy | wc -l)
  bg_deploy 25 tuner "$HANG" "$H"
  sleep 5
  echo "5 s in: deploy still running; '--version' processes: $(vpids "$HANG"); health $(code "$H"); backups $(ls "$R"/var/backups/galion/deploy | wc -l)"
  bg_wait
  local orphan; orphan=$(vpids "$HANG")
  echo "after the outer timeout (SIGTERM to the process group): orphaned '--version' processes: ${orphan:-none}"
  check "hang A: deploy never returns on its own (killed by outer timeout 124), service untouched" \
    "$([ "$DRC" = 124 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ] && [ "$(ls "$R"/var/backups/galion/deploy | wc -l)" = "$nb" ]; echo $?)" "exit $DRC, sha $(sha "$T"), orphan ${orphan:-none}"
  [ -n "$orphan" ] && kill $orphan

  say "hang B: same binary with GALION_DEPLOY_VERSION_TIMEOUT=5 (opt-in)"
  probe_start 127.0.0.1:$P /health 5
  GALION_DEPLOY_VERSION_TIMEOUT=5 tdeploy 90 tuner "$HANG" "$H"
  probe_stop
  check "hang B: version step times out, install, health fails, auto rollback" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
  echo "DOWNTIME tuner auto-rollback-hang ${DOWN} ms" >> "$R/downtime"

  say "hang C: the real trap. New tuner installed but DOWN, manual rollback (level 2) to the OLD binary, which ignores --version and serves"
  tdeploy 60 tuner "$N" "$H"
  BAK=$(ls -1 "$R"/var/backups/galion/deploy/tuner-*.bin | xargs sha256sum | awk -v o="$OLD" 'substr($1,1,16)==o {print $2}' | tail -1)
  x ctl stop tuner; echo "port $P: $(listeners $P | wc -l) listener(s)"
  bg_deploy 20 tuner "$BAK" "$H"
  sleep 5
  local during dcode; during=$(vpids "$BAK"); dcode=$(code "$H")
  echo "5 s in: unit state $(ctl is-active tuner); listeners on $P: $(listeners $P)"
  echo "GET $H -> $dcode, answered by '$BAK --version' (pid ${during:-none}), a process outside the unit"
  bg_wait
  orphan=$(vpids "$BAK")
  echo "after the outer timeout: orphan ${orphan:-none}; unit $(ctl is-active tuner)"
  check "hang C: deploy hangs while the old binary's --version serves the port outside the unit" "$([ "$DRC" = 124 ] && [ -n "$during" ] && [ "$dcode" = 200 ]; echo $?)" "exit $DRC, --version pid ${during:-none} answered $dcode"
  [ -n "$orphan" ] && kill $orphan; sleep 0.3
  x ctl start tuner; wait_code "$H" 200 5
  echo "after killing the orphan and starting the unit: $(code "$H"), running sha $(sha "$T")"

  say "hang C with GALION_DEPLOY_VERSION_TIMEOUT=5: same situation completes"
  tdeploy 60 tuner "$N" "$H"; x ctl stop tuner
  GALION_DEPLOY_VERSION_TIMEOUT=5 tdeploy 60 tuner "$BAK" "$H"
  check "hang C + timeout: manual rollback completes, old sha, healthy, no orphan" "$([ "$DRC" = 0 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ] && [ -z "$(vpids "$BAK")" ]; echo $?)" "exit $DRC"

  say "end state"
  ctl status
  echo "backups kept: $(ls "$R"/var/backups/galion/deploy | wc -l) (galion-deploy keeps 5 per service)"
  sup_stop
  cat "$R/downtime"
  summary tuner
}

# ---------------------------------------------------------------- nerve
nerve() {
  sim_up nerve
  local P=19331 T DATA H STAGE
  H=http://127.0.0.1:$P/healthz
  T=$R/opt/galion/rust/bin/galion-nerve; DATA=$R/var/lib/galion-nerve; STAGE=$R/var/tmp/galion-cutover
  install -m 0755 "$BIN/nerve-old" "$T"; install -d -m 0700 "$DATA"
  DENV=(NERVE_ADDR=127.0.0.1:$P NERVE_DATA_DIR=$DATA)
  # The unit of ws/nerve a63b730 (identical to swarm/nerve 5802d0e's) with sim paths and
  # port, reflexes in dry-run, plus sim-only lines: no systemd/journald on the box, no
  # reflex runner, no galion-health, 5 s cycles, listener scan limited to our range.
  git -C "$HERE" show a63b730:deploy/nerve/galion-nerve.service \
    | sed -e "s|^ExecStart=/opt/galion/rust/bin/galion-nerve serve|ExecStart=$T serve|" \
          -e "s|^Environment=NERVE_ADDR=127.0.0.1:8670|Environment=NERVE_ADDR=127.0.0.1:$P|" \
          -e "s|^Environment=NERVE_DATA_DIR=/var/lib/galion-nerve|Environment=NERVE_DATA_DIR=$DATA|" \
          -e "s|^Environment=NERVE_DRY_RUN=0|Environment=NERVE_DRY_RUN=1|" \
          -e "s|^Environment=NERVE_INTERVAL=30|Environment=NERVE_INTERVAL=5|" \
    | awk -v extra="Environment=NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_REFLEX=0 NERVE_ROLLBACK=0 NERVE_GALION_HEALTH_PORT=0 NERVE_DEPLOY_BIN=/nonexistent/galion-deploy NERVE_LISTEN_SCOPE=19330-19339" \
          '{print} /^Environment=NERVE_REPORT_EVERY/ {print extra}' > "$R/units/galion-nerve.service"
  grep -E '^(ExecStart|Environment|Restart|StartLimit)' "$R/units/galion-nerve.service" | sed 's/^/  unit| /'
  sup_start
  wait_code "$H" 200 10; check "old galion-nerve serving /healthz" $? "$(code "$H")"
  echo "old nerve: / -> $(code "http://127.0.0.1:$P/") ; /health -> $(code "http://127.0.0.1:$P/health") (live VPS 8670: / 200, /health 404)"
  check "old nerve /health is 404 like live" "$([ "$(code "http://127.0.0.1:$P/health")" = 404 ]; echo $?)" "$(code "http://127.0.0.1:$P/health")"
  echo "letting the old nerve run 2 cycles so its data dir is real..."; sleep 11
  echo "data dir after old nerve: $(cd "$DATA" && ls | tr '\n' ' ')"
  # the live unit started days ago: its start-limit window is empty
  ctl reset-failed galion-nerve >/dev/null
  local OLD NEW N; OLD=$(sha "$BIN/nerve-old"); NEW=$(sha "$BIN/nerve-new")

  say "copy + verify (stand-in for scp to /var/tmp/galion-cutover)"
  install -d -m 0700 "$STAGE"; cp "$BIN/nerve-new" "$STAGE/openmind-nerve-x86_64-unknown-linux-musl"
  (cd "$BIN" && sha256sum nerve-new | sed 's/nerve-new/openmind-nerve-x86_64-unknown-linux-musl/') > "$STAGE/SHA256SUMS"
  (cd "$STAGE" && x sha256sum -c SHA256SUMS); check "artefact sha256 verified" $? "sha $NEW"
  N=$STAGE/openmind-nerve-x86_64-unknown-linux-musl

  say "preflight"
  echo "installed sha $(sha "$T") (old = ws/nerve a63b730 galion-nerve)"; x ctl is-active galion-nerve
  local v; v=$(timeout 5 "$N" --version); echo "--version: $v"
  check "new --version answers without binding" "$([ "$v" = "openmind-nerve 0.1.0" ]; echo $?)" "$v"
  echo "health table: galion-deploy has NO entry for galion-nerve -> without a URL it would only check 'is-active' for 5 s"

  say "swap: galion-deploy galion-nerve <new> http://127.0.0.1:$P/healthz (probe every 2 ms on /healthz)"
  probe_start 127.0.0.1:$P /healthz 2
  tdeploy 60 galion-nerve "$N" "$H"
  probe_stop
  check "swap exit 0" "$DRC" "exit $DRC"
  check "swap downtime < 1 s" "$([ "$DOWN" -lt 1000 ]; echo $?)" "longest outage ${DOWN} ms"
  echo "DOWNTIME nerve swap ${DOWN} ms" >> "$R/downtime"
  local BAK; BAK=$(ls -1t "$R"/var/backups/galion/deploy/galion-nerve-*.bin | head -1)

  say "health after the switch"
  local pid; pid=$(ctl show galion-nerve | sed -n 's/^MainPID=//p')
  check "running binary is the new sha" "$([ "$(sha "/proc/$pid/exe")" = "$NEW" ]; echo $?)" "$(sha "/proc/$pid/exe")"
  echo "/healthz -> $(curl -s -m 3 "$H" | tr -d '\n')"
  echo "/health  -> $(curl -s -m 3 "http://127.0.0.1:$P/health")"
  echo "/state   -> $(code "http://127.0.0.1:$P/state"), /self-eval.md -> $(code "http://127.0.0.1:$P/self-eval.md"), / -> $(code "http://127.0.0.1:$P/")"
  sleep 6
  ctl journal galion-nerve 6 | cut -c1-200 | sed 's/^/  journal| /'
  check "new nerve reads the old nerve's data dir and keeps serving" "$([ "$(code "$H")" = 200 ] && ctl is-active galion-nerve >/dev/null; echo $?)" "data: $(cd "$DATA" && ls | tr '\n' ' ')"

  say "rollback level 2 (manual): galion-deploy galion-nerve <backup> /healthz"
  probe_start 127.0.0.1:$P /healthz 2
  tdeploy 60 galion-nerve "$BAK" "$H"
  probe_stop
  sleep 6
  check "L2 manual rollback exit 0, old sha, old nerve healthy on the new nerve's data dir" "$([ "$DRC" = 0 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, sha $(sha "$T")"
  echo "DOWNTIME nerve rollback-L2 ${DOWN} ms" >> "$R/downtime"
  ctl journal galion-nerve 4 | cut -c1-200 | sed 's/^/  journal| /'

  say "forward again, then rollback level 3 (last resort)"
  tdeploy 60 galion-nerve "$N" "$H"; check "re-deploy new exit 0" "$DRC" "exit $DRC"
  probe_start 127.0.0.1:$P /healthz 2
  x install -m 0755 "$BAK" "$T.new" && x mv -f "$T.new" "$T" && x ctl restart galion-nerve
  wait_code "$H" 200 10
  probe_stop
  check "L3 last resort: old sha, healthy" "$([ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "sha $(sha "$T")"
  echo "DOWNTIME nerve rollback-L3 ${DOWN} ms" >> "$R/downtime"
  ctl show galion-nerve | grep -E 'StartLimit|ActiveState'

  say "rollback level 1 (automatic) right after: broken binary. The unit allows 5 starts per 600 s (StartLimitBurst=5)"
  echo "starts so far in this window: swap, L2, re-deploy, L3 = 4"
  probe_start 127.0.0.1:$P /healthz 5
  tdeploy 90 galion-nerve "$BROKEN" "$H"
  probe_stop
  echo "unit: $(ctl is-active galion-nerve); installed sha $(sha "$T"); /healthz $(code "$H")"
  ctl journal galion-nerve 12 | grep -E 'start-limit|too quickly|Scheduled|exited' | sed 's/^/  journal| /'
  check "FINDING: 6th start in 600 s is refused; galion-deploy dies at its rollback restart (set -e), old binary back on disk but unit FAILED" \
    "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(ctl is-active galion-nerve)" = failed ]; echo $?)" "exit $DRC, unit $(ctl is-active galion-nerve)"
  local t=$(date +%s%N)
  x ctl reset-failed galion-nerve; x ctl restart galion-nerve; wait_code "$H" 200 10
  check "recovery: reset-failed + restart brings the old nerve back" "$([ "$(code "$H")" = 200 ]; echo $?)" "$(ms_since "$t") ms"

  say "the same 6th-start situation with GALION_DEPLOY_RESET_FAILED=1 (opt-in)"
  ctl reset-failed galion-nerve >/dev/null
  for i in 1 2 3; do ctl restart galion-nerve >/dev/null; done
  wait_code "$H" 200 10
  echo "4 starts used in the 600 s window (recovery + 3 restarts standing in for swap/L2/forward); now a broken deploy:"
  GALION_DEPLOY_RESET_FAILED=1 tdeploy 90 galion-nerve "$BROKEN" "$H"
  check "RESET_FAILED=1: rolled back ok even with a full start-limit window" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ] && [ "$(ctl is-active galion-nerve)" = active ]; echo $?)" "exit $DRC, unit $(ctl is-active galion-nerve)"

  say "rollback level 1 (automatic) with an empty start-limit window"
  ctl reset-failed galion-nerve >/dev/null
  probe_start 127.0.0.1:$P /healthz 5
  tdeploy 90 galion-nerve "$BROKEN" "$H"
  probe_stop
  check "L1 broken binary: rolled back ok, old sha, healthy" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
  echo "DOWNTIME nerve auto-rollback-broken ${DOWN} ms" >> "$R/downtime"

  say "health-URL trap: the same broken deploy judged on /health (new nerve has it, the old one answers 404)"
  ctl reset-failed galion-nerve >/dev/null
  tdeploy 90 galion-nerve "$BROKEN" "http://127.0.0.1:$P/health"
  check "FINDING: with /health the rollback to the old nerve is reported 'ROLLBACK ALSO UNHEALTHY' although it serves" \
    "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, /healthz $(code "$H"), /health $(code "http://127.0.0.1:$P/health")"

  say "hang: sim-hang, no version timeout"
  ctl reset-failed galion-nerve >/dev/null
  tdeploy 15 galion-nerve "$HANG" "$H"
  check "hang: deploy never returns on its own; service untouched" "$([ "$DRC" = 124 ] && [ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC"

  say "final forward swap (leave the sim on the new nerve) and end state"
  ctl reset-failed galion-nerve >/dev/null
  probe_start 127.0.0.1:$P /healthz 2
  tdeploy 60 galion-nerve "$N" "$H"
  probe_stop
  check "final swap exit 0" "$DRC" "exit $DRC, outage ${DOWN} ms"
  ctl status
  sup_stop; cat "$R/downtime"; summary nerve
}

# ---------------------------------------------------------------- room
room() {
  sim_up room
  local P=19332 H T STAGE W
  H=http://127.0.0.1:$P/health; T=$R/opt/galion/rust/bin/room; STAGE=$R/var/tmp/galion-cutover; W=$R/workspace
  mkdir -p "$W/galion-light/docs" "$W/galion-src" "$W/galion-light/room"
  printf '# Operator notes (sim)\n' > "$W/galion-light/docs/README.md"
  printf 'sim source tree\n' > "$W/galion-src/NOTES.txt"
  printf '<html>room</html>\n' > "$W/galion-light/room/index.html"
  DENV=(ROOM_BIND=127.0.0.1:$P ROOM_ROOT=$W)
  sup_start
  say "today: no room unit, nothing on :$P (live: nothing on :4020)"
  echo "listeners: $(listeners $P | wc -l); $(ctl known room 2>&1)"
  install -d -m 0700 "$STAGE"; cp "$BIN/room-new" "$STAGE/room.new"
  echo "room.new sha $(sha "$STAGE/room.new"); --version: $("$STAGE/room.new" --version)"

  say "install the staged unit (deploy/staged/room.service from swarm/room 5bb27e1, sim paths) + daemon-reload"
  git -C "$HERE" show 5bb27e1:deploy/staged/room.service \
    | sed -e "s|^ExecStart=/opt/galion/rust/bin/room --listen|ExecStart=$T --listen|" \
          -e "s|^Environment=ROOM_BIND=127.0.0.1:4020|Environment=ROOM_BIND=127.0.0.1:$P|" \
          -e "s|^Environment=ROOM_ROOT=/workspace|Environment=ROOM_ROOT=$W|" > "$R/units/room.service"
  x ctl daemon-reload; echo "state after reload: $(ctl is-active room) (daemon-reload does not start it)"

  say "galion-deploy room <new> on a unit that never ran"
  tdeploy 30 room "$STAGE/room.new" "$H"
  check "FINDING: galion-deploy cannot do a first install (no installed binary to resolve/back up)" "$([ "$DRC" = 1 ] && [ ! -e "$T" ]; echo $?)" "exit $DRC"

  say "first install by hand (room.md fallback): install + start"
  probe_start 127.0.0.1:$P /health 5
  x install -m 755 "$STAGE/room.new" "$T" && x ctl start room
  wait_code "$H" 200 5
  probe_stop
  local body objs pw; body=$(curl -s -m 3 "$H"); objs=$(curl -s -m 3 "http://127.0.0.1:$P/api/objects" | head -c 12)
  pw=$(code "http://127.0.0.1:$P/api/read?path=/etc/passwd")
  echo "/health -> '$body' ; /api/objects -> $objs ; /api/read?path=/etc/passwd -> $pw"
  echo "listeners: $(listeners $P)"
  check "room health checks (ok, {\"objects\":[, 404, loopback only)" "$([ "$body" = ok ] && [ "$objs" = '{"objects":[' ] && [ "$pw" = 404 ] && listeners $P | grep -q '127.0.0.1:'$P; echo $?)" "$body / $objs / $pw"

  say "upgrades now go through galion-deploy: same binary (restart gap) and a broken one (auto rollback)"
  probe_start 127.0.0.1:$P /health 2
  tdeploy 60 room "$STAGE/room.new" "$H"
  probe_stop
  check "redeploy exit 0, downtime < 1 s" "$([ "$DRC" = 0 ] && [ "$DOWN" -lt 1000 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
  echo "DOWNTIME room redeploy ${DOWN} ms" >> "$R/downtime"
  probe_start 127.0.0.1:$P /health 5
  tdeploy 90 room "$BROKEN" "$H"
  probe_stop
  check "L1 broken: rolled back ok" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$(sha "$STAGE/room.new")" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
  echo "DOWNTIME room auto-rollback-broken ${DOWN} ms" >> "$R/downtime"
  probe_start 127.0.0.1:$P /health 5
  tdeploy 60 room "$BIN/room-old" "$H"
  probe_stop
  echo "room-old (main 13a296c) deploy: exit $DRC (old builds also need --listen); back to new:"
  ctl reset-failed room >/dev/null; tdeploy 60 room "$STAGE/room.new" "$H"

  say "manual rollback to today's state (room.md): stop, remove the unit, daemon-reload"
  x ctl stop room; rm -f "$R/units/room.service"; x ctl daemon-reload
  echo "listeners on $P: $(listeners $P | wc -l); $(ctl known room 2>&1)"
  check "rollback: nothing on :$P, unit gone" "$([ "$(listeners $P | wc -l)" = 0 ] && ! ctl known room >/dev/null 2>&1; echo $?)" "0 listeners"
  sup_stop; cat "$R/downtime"; summary room
}

# ---------------------------------------------------------------- desk
desk() {
  sim_up desk
  local P=19333 H T STAGE
  H=http://127.0.0.1:$P/health; T=$R/opt/galion/bin/galion-desk; STAGE=$R/var/tmp/galion-cutover
  install -d "$R/opt/galion/bin" "$R/state" "$R/one" "$R/hermes"
  install -m 0755 "$BIN/desk-old" "$T"
  local E="DESK_BIND=127.0.0.1:$P DESK_STATE_DIR=$R/state DESK_ONE_DIR=$R/one DESK_HERMES_DIR=$R/hermes DESK_HERMES_BIN=$R/opt/galion/rust/bin/desk-hermes DESK_OLLAMA=127.0.0.1:19339 OPENMIND_CONFIG=$R/none.toml"
  local PR="DESK_PROBE_HERMES=127.0.0.1:19339 DESK_PROBE_ONE=127.0.0.1:19339 DESK_PROBE_TUNNEL=127.0.0.1:19339 DESK_PROBE_CONSOLE=127.0.0.1:19339 DESK_PROBE_ORIGIN=127.0.0.1:19339 DESK_PROBE_ROOM=127.0.0.1:19339 DESK_PROBE_TULU=127.0.0.1:19339"
  read -r -a DENV <<<"$E"
  # The live unit has no Environment= (defaults = production paths); the sim must
  # point every path and port into the sim root and our port range.
  cat > "$R/units/galion-desk.service" <<U
[Unit]
Description=galion-desk (sim; live: /opt/galion/bin/galion-desk, no Environment=)
[Service]
ExecStart=$T
Environment=$E
Environment=$PR
Restart=always
RestartSec=2
U
  sup_start
  wait_code "$H" 200 5; check "old desk serving" $? "$(curl -s -m 3 "$H")"
  local OLD NEW; OLD=$(sha "$BIN/desk-old"); NEW=$(sha "$BIN/desk-new")
  install -d -m 0700 "$STAGE"; cp "$BIN/desk-new" "$STAGE/galion-desk.new"
  local N=$STAGE/galion-desk.new
  echo "old (main 13a296c stand-in for the Oct 10 glibc build) sha $OLD; new (swarm/desk-linkwatch a54c792) sha $NEW"
  echo "/desk top-level keys (old): $(curl -s -m 5 "http://127.0.0.1:$P/desk" | jq -r 'keys_unsorted|length')"

  say "preflight: new --help is safe; new --version is NOT handled (it resolves settings and binds)"
  x "$N" --help
  local t=$(date +%s%N)
  env "${DENV[@]}" timeout 5 "$N" --version; local vrc=$?
  echo "  --version while the desk holds :$P -> exit $vrc after $(ms_since "$t") ms (bind error, so galion-deploy would continue)"

  say "manual cutover per docs/cutover/desk.md steps 8-11 (probe every 2 ms)"
  x cp -p "$T" "$T.oct10"
  probe_start 127.0.0.1:$P /health 2
  x install -m 0755 "$N" "$T" && x ctl restart galion-desk
  wait_code "$H" 200 5
  probe_stop
  echo "DOWNTIME desk manual-swap ${DOWN} ms" >> "$R/downtime"
  local keys mp; keys=$(curl -s -m 5 "http://127.0.0.1:$P/desk" | jq -r 'keys_unsorted|join(",")')
  mp=$(curl -s -m 5 "http://127.0.0.1:$P/desk" | jq -c '.mindpack')
  echo "/health $(curl -s -m 3 "$H"); /desk keys: $keys"; echo "mindpack (no desk-hermes in the sim): $mp"
  ctl journal galion-desk 2 | sed 's/^/  journal| /'
  check "manual swap: new sha running, 22 /desk keys, downtime < 1 s" "$([ "$(sha "$T")" = "$NEW" ] && [ "$(echo "$keys" | tr ',' '\n' | wc -l)" = 22 ] && [ "$DOWN" -lt 1000 ]; echo $?)" "outage ${DOWN} ms"

  say "manual rollback per step 13"
  probe_start 127.0.0.1:$P /health 2
  x install -m 0755 "$T.oct10" "$T" && x ctl restart galion-desk
  wait_code "$H" 200 5
  probe_stop
  check "step 13 rollback: old sha, healthy, downtime < 1 s" "$([ "$(sha "$T")" = "$OLD" ] && [ "$(code "$H")" = 200 ] && [ "$DOWN" -lt 1000 ]; echo $?)" "outage ${DOWN} ms"
  echo "DOWNTIME desk manual-rollback ${DOWN} ms" >> "$R/downtime"

  say "the galion-deploy route (it has [galion-desk]=8646/health): swap, broken, manual rollback"
  probe_start 127.0.0.1:$P /health 2
  tdeploy 60 galion-desk "$N" "$H"
  probe_stop
  check "galion-deploy swap exit 0, downtime < 1 s" "$([ "$DRC" = 0 ] && [ "$DOWN" -lt 1000 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
  echo "DOWNTIME desk deploy-swap ${DOWN} ms" >> "$R/downtime"
  local BAK; BAK=$(ls -1t "$R"/var/backups/galion/deploy/galion-desk-*.bin | head -1)
  probe_start 127.0.0.1:$P /health 5
  tdeploy 90 galion-desk "$BROKEN" "$H"
  probe_stop
  check "L1 broken: rolled back ok to the new desk" "$([ "$DRC" = 1 ] && [ "$(sha "$T")" = "$NEW" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC, outage ${DOWN} ms"
  probe_start 127.0.0.1:$P /health 2
  tdeploy 60 galion-desk "$BAK" "$H"
  probe_stop
  check "L2 manual rollback via galion-deploy: old sha back" "$([ "$DRC" = 0 ] && [ "$(sha "$T")" = "$OLD" ]; echo $?)" "exit $DRC, outage ${DOWN} ms"

  say "hang trap: desk DOWN, forward deploy of the new desk (it ignores --version and serves)"
  x ctl stop galion-desk
  bg_deploy 20 galion-desk "$N" "$H"
  sleep 5
  local during dcode; during=$(vpids "$N"); dcode=$(code "$H")
  echo "5 s in: unit $(ctl is-active galion-desk); GET $H -> $dcode from '$N --version' (pid ${during:-none})"
  bg_wait
  check "FINDING: deploy hangs while 'galion-desk --version' serves the port outside the unit" "$([ "$DRC" = 124 ] && [ -n "$during" ] && [ "$dcode" = 200 ]; echo $?)" "exit $DRC"
  v=$(vpids "$N"); [ -n "$v" ] && kill $v; sleep 0.3
  GALION_DEPLOY_VERSION_TIMEOUT=5 tdeploy 60 galion-desk "$N" "$H"
  check "same with GALION_DEPLOY_VERSION_TIMEOUT=5: deploy completes" "$([ "$DRC" = 0 ] && [ "$(sha "$T")" = "$NEW" ] && [ "$(code "$H")" = 200 ]; echo $?)" "exit $DRC"
  ctl status
  sup_stop; cat "$R/downtime"; summary desk
}

case ${1:-all} in
  tuner) tuner ;;
  nerve) nerve ;;
  room) room ;;
  desk) desk ;;
  all) tuner; nerve; room; desk ;;
  *) echo "usage: $0 [tuner|nerve|room|desk|all]"; exit 2 ;;
esac
echo; echo "deploy-sim: $FAILS failed check(s)"
[ "$FAILS" = 0 ]
