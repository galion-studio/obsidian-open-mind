#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# One-command install of the Open Mind desktop and the local services its
# probes ask, for Raspberry Pi OS Bookworm 64-bit.
#
# The binaries are the aarch64 half of the Pi 5 seed
# (scripts/linux/pi-seed.sh, docs/PI5-SEED.md). This script does not build
# them, does not download them, and does not publish a release.
#
# On a Raspberry Pi OS Bookworm aarch64 machine:
#   sudo scripts/linux/pi-bookworm-install.sh --seed DIR
# DIR is the seed's bin/aarch64-unknown-linux-musl directory, or
# target/pi-seed-out/aarch64-unknown-linux-musl after a local seed build.
# A second run with the same binaries and unit text exits 0 and does not
# restart processes that are still running.
#
# The CI job runs this inside a Debian Bookworm arm64 root under qemu-user.
# That is not Raspberry Pi OS and not a Raspberry Pi 5. The job sets
# PI_BOOKWORM_EMULATOR=1, which this script refuses when a Pi OS marker is
# also present. It then prints:
#   pi-bookworm: emulator result, not a Raspberry Pi 5 result and not a Raspberry Pi OS boot
set -euo pipefail

export PATH=/usr/sbin:/usr/bin:/sbin:/bin

EMULATOR_LINE='pi-bookworm: emulator result, not a Raspberry Pi 5 result and not a Raspberry Pi OS boot'
NO_SYSTEMD_LINE='pi-bookworm: systemd is not running; starting the same commands in the background'

# Names in the seed directory. These are the desktop and the eight services
# in crates/openmind-desktop/src/probe.rs SERVICES.
bins=(
  openmind-desktop
  openmind-memory
  openmind-nerve
  gaia-cortex
  galion-console-serve
  hermes-gateway
  galion-desk
  galion-one
)

usage() {
  cat << 'EOF'
usage: scripts/linux/pi-bookworm-install.sh [--seed DIR]
       scripts/linux/pi-bookworm-install.sh --files-only --root DIR --seed DIR
       scripts/linux/pi-bookworm-install.sh probe
       scripts/linux/pi-bookworm-install.sh check-report FILE

--seed DIR   aarch64 Pi 5 seed binaries. Default: PI_BOOKWORM_SEED, or
             target/pi-seed-out/aarch64-unknown-linux-musl when this script
             still lives in scripts/linux of a checkout.
install      copy the binaries to /opt/obsidian/bin, write systemd system
             units, and start them. Needs root. Raspberry Pi OS Bookworm
             aarch64, or PI_BOOKWORM_EMULATOR=1 on Debian Bookworm aarch64.
--files-only write the same tree under --root and do not start anything.
probe        run the installed desktop's --probe. Stdout is the JSON report.
check-report read a --probe JSON file. The voice probe must be down with
             HTTP 503 (no speech backend). The other seven must be up.

exit 0  the command finished
exit 2  bad arguments
exit 1  install, start, or report check failed
EOF
}

die() {
  printf 'pi-bookworm: %s\n' "$1" >&2
  if [ -n "${log_dir:-}" ] && [ -d "$log_dir" ]; then
    local f
    for f in "$log_dir"/*; do
      [ -f "$f" ] || continue
      printf 'pi-bookworm: log %s\n' "$(basename -- "$f")" >&2
      tail -n 40 -- "$f" >&2 || true
    done
  fi
  exit 1
}

banner() {
  if [ "${PI_BOOKWORM_EMULATOR:-}" = 1 ]; then
    printf '%s\n' "$EMULATOR_LINE"
  fi
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=
case "$script_dir" in
  */scripts/linux)
    repo=$(CDPATH= cd -- "$script_dir/../.." && pwd)
    ;;
esac

seed=
root_dir=
files_only=0
mode=install
report_file=

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help|help)
      usage
      exit 0
      ;;
    --seed)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      seed=$2
      shift 2
      ;;
    --root)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      root_dir=$2
      shift 2
      ;;
    --files-only)
      files_only=1
      shift
      ;;
    probe)
      mode=probe
      shift
      ;;
    check-report)
      mode=check-report
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      report_file=$2
      shift 2
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

abs_path() {
  local path=$1
  case "$path" in
    /*) printf '%s\n' "$path" ;;
    *) printf '%s\n' "$PWD/$path" ;;
  esac
}

codename() {
  [ -f /etc/os-release ] || return 1
  sed -n 's/^VERSION_CODENAME=//p' /etc/os-release | head -n 1 | tr -d '"'
}

arch_is_arm64() {
  local arch=
  if command -v dpkg >/dev/null 2>&1; then
    arch=$(dpkg --print-architecture)
  fi
  if [ "$arch" != arm64 ]; then
    case "$(uname -m)" in
      aarch64|arm64) arch=arm64 ;;
    esac
  fi
  [ "$arch" = arm64 ]
}

pi_os_marker() {
  if [ -f /etc/rpi-issue ]; then
    return 0
  fi
  if [ -f /proc/device-tree/model ] && grep -a -q 'Raspberry Pi' /proc/device-tree/model; then
    return 0
  fi
  return 1
}

require_os() {
  local name
  name=$(codename || true)
  if [ "$name" != bookworm ]; then
    die "need Raspberry Pi OS or Debian Bookworm (VERSION_CODENAME=$name)"
  fi
  if ! arch_is_arm64; then
    die "need aarch64 (64-bit). 32-bit Raspberry Pi OS is not this install"
  fi
  if [ "${PI_BOOKWORM_EMULATOR:-}" = 1 ]; then
    if pi_os_marker; then
      die "PI_BOOKWORM_EMULATOR=1 is set on a Raspberry Pi OS system"
    fi
    return 0
  fi
  if ! pi_os_marker; then
    die "not Raspberry Pi OS. A Debian Bookworm emulator sets PI_BOOKWORM_EMULATOR=1 and is not a Pi result"
  fi
}

is_aarch64_elf() {
  local path=$1 mag class data mach
  [ -f "$path" ] || return 1
  mag=$(od -An -t x1 -N 4 -- "$path" | tr -d ' \n')
  [ "$mag" = 7f454c46 ] || return 1
  class=$(od -An -t u1 -j 4 -N 1 -- "$path" | tr -d ' ')
  data=$(od -An -t u1 -j 5 -N 1 -- "$path" | tr -d ' ')
  [ "$class" = 2 ] && [ "$data" = 1 ] || return 1
  mach=$(od -An -t x1 -j 18 -N 2 -- "$path" | tr -d ' \n')
  [ "$mach" = b700 ]
}

changed=0
prefix=/
unit_dir=
bin_dir=
log_dir=
run_dir=

install_file() {
  local src=$1 dest=$2 mode=$3
  mkdir -p -- "$(dirname -- "$dest")"
  if [ -f "$dest" ] && cmp -s -- "$src" "$dest"; then
    chmod "$mode" -- "$dest"
    return 0
  fi
  cp -- "$src" "$dest"
  chmod "$mode" -- "$dest"
  changed=1
}

write_unit() {
  local name=$1 body=$2 tmp
  tmp=$(mktemp)
  printf '%s\n' "$body" > "$tmp"
  install_file "$tmp" "$unit_dir/$name" 644
  rm -f -- "$tmp"
}

write_units() {
  local spdx
  spdx='# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
# Copyright (c) 3000 Galion / galion-studio'
  write_unit obsidian-desktop.service "$spdx
[Unit]
Description=Open Mind desktop loopback listener
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
ExecStart=/opt/obsidian/bin/openmind-desktop --listen 127.0.0.1:18181 --stack off
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-memory.service "$spdx
[Unit]
Description=Open Mind memory loopback health
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
Environment=OPENMIND_MEMORY_HOST=127.0.0.1
Environment=OPENMIND_MEMORY_PORT=8420
Environment=OPENMIND_MEMORY_DATA_DIR=/var/lib/obsidian/memory
ExecStart=/opt/obsidian/bin/openmind-memory
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-nerve.service "$spdx
[Unit]
Description=Open Mind nerve loopback health
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
Environment=NERVE_ADDR=127.0.0.1:8670
Environment=NERVE_DATA_DIR=/var/lib/obsidian/nerve
Environment=NERVE_DRY_RUN=1
Environment=NERVE_REFLEX=0
Environment=NERVE_SYSTEMD=0
Environment=NERVE_JOURNAL=0
Environment=NERVE_ROLLBACK=0
ExecStart=/opt/obsidian/bin/openmind-nerve serve
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-cortex.service "$spdx
[Unit]
Description=Gaia cortex loopback health (dry-run)
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
Environment=CORTEX_LISTEN=127.0.0.1:8674
Environment=CORTEX_DATA_DIR=/var/lib/obsidian/cortex
Environment=CORTEX_ACTIONS=dry-run
Environment=CORTEX_LLM_ENABLED=0
Environment=CORTEX_HERMES_HANDOFF=0
ExecStart=/opt/obsidian/bin/gaia-cortex serve
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-console.service "$spdx
[Unit]
Description=Galion console loopback local API
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
ExecStart=/opt/obsidian/bin/galion-console-serve --bind 127.0.0.1:18080 --local-api --ui rust --quiet
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-hermes.service "$spdx
[Unit]
Description=Hermes loopback standby health
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
Environment=HERMES_HOME=/var/lib/obsidian/hermes
Environment=HERMES_RS_HOST=127.0.0.1
Environment=HERMES_RS_PORT=8642
Environment=HERMES_RS_MCP=0
Environment=HERMES_RS_CRON=0
ExecStart=/opt/obsidian/bin/hermes-gateway --offhost-standby
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-desk.service "$spdx
[Unit]
Description=Galion desk loopback health
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
Environment=DESK_BIND=127.0.0.1:8646
Environment=OPENMIND_CONFIG=/var/lib/obsidian/no-desk-config.toml
ExecStart=/opt/obsidian/bin/galion-desk
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"

  write_unit obsidian-one.service "$spdx
[Unit]
Description=Galion One loopback listener and mouth health
After=network.target

[Service]
Type=simple
User=obsidian
Group=obsidian
WorkingDirectory=/var/lib/obsidian
Environment=ONE_BIND=127.0.0.1:8091
Environment=ONE_MOUTH_BIND=127.0.0.1:8002
Environment=ONE_NO_WARM=1
Environment=ONE_CONSOLE=off
ExecStart=/opt/obsidian/bin/galion-one
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target"
}

services=(
  obsidian-desktop
  obsidian-memory
  obsidian-nerve
  obsidian-cortex
  obsidian-console
  obsidian-hermes
  obsidian-desk
  obsidian-one
)

ensure_user() {
  if [ "$prefix" != / ]; then
    return 0
  fi
  [ -x /usr/sbin/useradd ] || die "useradd is missing"
  if getent passwd obsidian >/dev/null; then
    return 0
  fi
  if [ -d /var/lib/obsidian ]; then
    /usr/sbin/useradd --system --home-dir /var/lib/obsidian --user-group obsidian
  else
    /usr/sbin/useradd --system --home-dir /var/lib/obsidian --create-home --user-group obsidian
  fi
}

ensure_dirs() {
  local d
  for d in \
    "$prefix/opt/obsidian/bin" \
    "$prefix/var/lib/obsidian" \
    "$prefix/var/lib/obsidian/memory" \
    "$prefix/var/lib/obsidian/nerve" \
    "$prefix/var/lib/obsidian/cortex" \
    "$prefix/var/lib/obsidian/hermes" \
    "$prefix/var/lib/obsidian/log" \
    "$prefix/var/lib/obsidian/run"
  do
    mkdir -p -- "$d"
  done
  if [ "$prefix" = / ]; then
    chown obsidian:obsidian \
      /var/lib/obsidian \
      /var/lib/obsidian/memory \
      /var/lib/obsidian/nerve \
      /var/lib/obsidian/cortex \
      /var/lib/obsidian/hermes \
      /var/lib/obsidian/log \
      /var/lib/obsidian/run
  fi
  # This file must not exist. galion-desk then keeps its built-in loopback defaults.
  rm -f -- "$prefix/var/lib/obsidian/no-desk-config.toml"
}

port_open() {
  local port=$1
  (echo >"/dev/tcp/127.0.0.1/$port") >/dev/null 2>&1
}

wait_ports() {
  local deadline=$((SECONDS + 180))
  local port pending
  local ports=(18181 8420 8670 8674 18080 8642 8646 8002 8091)
  while [ "$SECONDS" -lt "$deadline" ]; do
    pending=0
    for port in "${ports[@]}"; do
      if ! port_open "$port"; then
        pending=1
      fi
    done
    if [ "$pending" = 0 ]; then
      for port in "${ports[@]}"; do
        printf 'pi-bookworm: listening port=%s\n' "$port"
      done
      return 0
    fi
    sleep 1
  done
  for port in "${ports[@]}"; do
    if ! port_open "$port"; then
      printf 'pi-bookworm: port %s is not listening\n' "$port" >&2
    fi
  done
  die "not all services accepted a connection"
}

start_bg() {
  local name=$1
  shift
  local pidfile="$run_dir/$name.pid"
  local log="$log_dir/$name.log"
  local old n
  if [ -f "$pidfile" ]; then
    old=$(tr -d ' \n' < "$pidfile")
    if [ -n "$old" ] && kill -0 "$old" 2>/dev/null; then
      if [ "$changed" = 0 ]; then
        printf 'pi-bookworm: %s already running pid=%s\n' "$name" "$old"
        return 0
      fi
      kill "$old" 2>/dev/null || true
      n=0
      while kill -0 "$old" 2>/dev/null && [ "$n" -lt 50 ]; do
        sleep 0.1
        n=$((n + 1))
      done
    fi
  fi
  [ -x /usr/sbin/runuser ] || die "runuser is missing"
  /usr/sbin/runuser -u obsidian -- "$@" >>"$log" 2>&1 &
  printf '%s\n' "$!" > "$pidfile"
  printf 'pi-bookworm: started %s pid=%s\n' "$name" "$!"
}

start_from_unit() {
  local name=$1
  local unit="$unit_dir/$name.service"
  local -a env_args=()
  local -a cmd=()
  local line exec=
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      Environment=*) env_args+=("${line#Environment=}") ;;
      ExecStart=*) exec=${line#ExecStart=} ;;
    esac
  done < "$unit"
  [ -n "$exec" ] || die "no ExecStart in $unit"
  # The units this script writes use spaces only as argument separators.
  set -f
  # shellcheck disable=SC2086
  cmd=($exec)
  set +f
  if [ "${#env_args[@]}" -eq 0 ]; then
    start_bg "$name" "${cmd[@]}"
  else
    start_bg "$name" /usr/bin/env "${env_args[@]}" "${cmd[@]}"
  fi
}

start_services() {
  local name
  if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload
    for name in "${services[@]}"; do
      systemctl enable "$name.service"
      if systemctl is-active --quiet "$name.service"; then
        if [ "$changed" = 1 ]; then
          systemctl restart "$name.service"
          printf 'pi-bookworm: restarted %s\n' "$name"
        else
          printf 'pi-bookworm: %s already running\n' "$name"
        fi
      else
        systemctl start "$name.service"
        printf 'pi-bookworm: started %s\n' "$name"
      fi
    done
  else
    printf '%s\n' "$NO_SYSTEMD_LINE"
    for name in "${services[@]}"; do
      start_from_unit "$name"
    done
  fi
  wait_ports
}

cmd_install() {
  local bin
  banner
  if [ "$files_only" = 1 ]; then
    [ -n "$root_dir" ] || { usage >&2; exit 2; }
    [ -n "$seed" ] || { usage >&2; exit 2; }
    root_dir=$(abs_path "$root_dir")
    [ "$root_dir" != / ] || die "--files-only does not write to /"
    prefix=$root_dir
  else
    [ -z "$root_dir" ] || die "--root is only for --files-only"
    require_os
    [ "$(id -u)" -eq 0 ] || die "install needs root"
    prefix=/
  fi
  if [ -z "$seed" ]; then
    if [ -n "${PI_BOOKWORM_SEED:-}" ]; then
      seed=$PI_BOOKWORM_SEED
    elif [ -n "$repo" ]; then
      seed=$repo/target/pi-seed-out/aarch64-unknown-linux-musl
    else
      die "--seed is required"
    fi
  fi
  seed=$(abs_path "$seed")
  [ -d "$seed" ] || die "seed directory is missing: $seed"
  for bin in "${bins[@]}"; do
    [ -f "$seed/$bin" ] || die "seed missing $bin"
    if ! is_aarch64_elf "$seed/$bin"; then
      die "seed $bin is not an aarch64 ELF"
    fi
  done
  bin_dir=$prefix/opt/obsidian/bin
  unit_dir=$prefix/etc/systemd/system
  log_dir=$prefix/var/lib/obsidian/log
  run_dir=$prefix/var/lib/obsidian/run
  ensure_user
  ensure_dirs
  for bin in "${bins[@]}"; do
    install_file "$seed/$bin" "$bin_dir/$bin" 755
  done
  write_units
  if [ "$changed" = 1 ]; then
    printf 'pi-bookworm: installed\n'
  else
    printf 'pi-bookworm: unchanged\n'
  fi
  if [ "$files_only" = 1 ]; then
    return 0
  fi
  start_services
}

cmd_probe() {
  local bin=/opt/obsidian/bin/openmind-desktop
  if [ "${PI_BOOKWORM_EMULATOR:-}" = 1 ]; then
    printf '%s\n' "$EMULATOR_LINE" >&2
  fi
  [ -x "$bin" ] || die "installed desktop is missing"
  exec "$bin" --probe
}

probe_object() {
  local json=$1 name=$2
  local rest
  case "$json" in
    *"\"service\":\"$name\""*) ;;
    *) return 1 ;;
  esac
  rest=${json#*"\"service\":\"$name\""}
  printf '%s' "${rest%%\}*}"
}

cmd_check_report() {
  local json obj name
  [ -n "$report_file" ] || { usage >&2; exit 2; }
  [ -f "$report_file" ] || die "report file is missing"
  if [ "${PI_BOOKWORM_EMULATOR:-}" = 1 ]; then
    printf '%s\n' "$EMULATOR_LINE" >&2
  fi
  json=$(tr -d '\n' < "$report_file")
  case "$json" in
    '{"read_only":true,"writes":0,'*) ;;
    *) die "report is not the desktop read-only probe JSON" ;;
  esac
  for name in nerve cortex console hermes desk memory galion-one; do
    obj=$(probe_object "$json" "$name") || die "report is missing $name"
    case "$obj" in
      *'"enabled":true,"state":"up"'*) ;;
      *)
        printf 'pi-bookworm: %s object %s\n' "$name" "$obj" >&2
        die "$name is not up"
        ;;
    esac
  done
  obj=$(probe_object "$json" voice) || die "report is missing voice"
  case "$obj" in
    *'"enabled":true,"state":"down"'*) ;;
    *)
      printf 'pi-bookworm: voice object %s\n' "$obj" >&2
      die "voice is not down"
      ;;
  esac
  case "$obj" in
    *'"http_status":503'*) ;;
    *) die "voice HTTP status is not 503" ;;
  esac
  case "$obj" in
    *'"detail":"http 503"'*) ;;
    *) die "voice detail is not http 503" ;;
  esac
  printf 'pi-bookworm: probe report ok\n'
}

case "$mode" in
  install) cmd_install ;;
  probe) cmd_probe ;;
  check-report) cmd_check_report ;;
  *)
    usage >&2
    exit 2
    ;;
esac
