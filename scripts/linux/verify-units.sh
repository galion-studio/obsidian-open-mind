#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Check deploy units and drop-ins against the loopback bind defaults:
# openmind-desktop listens on 127.0.0.1:18181, the console probe listens on
# 127.0.0.1:18080, and nothing binds port 18081. Then run systemd-analyze
# verify on each unit when that command is installed.
#
# Missing ExecStart binaries are expected on a checkout that has not
# installed /opt/galion. StartLimitIntervalSec under [Service] is a known
# warning on the 2026-10-19 rendered units (systemd 255 says "Unknown key
# name", systemd 259 says "Unknown key"). systemd-analyze also prints
# warnings for units under /usr/lib/systemd that this checkout does not
# own (CPUAccounting= on the GitHub Ubuntu 26.04 image). Those lines are
# recorded and do not fail the run. A diagnostic about one of our units
# does.
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root"

fail=0
bad() {
  printf 'linux-units: FAIL %s\n' "$*" >&2
  fail=1
}

strip_comments() {
  awk '
    /^[[:space:]]*#/ { next }
    {
      sub(/[[:space:]]+#.*$/, "")
      if ($0 ~ /^[[:space:]]*$/) next
      print
    }
  '
}

lib=crates/openmind-desktop/src/lib.rs
probe_src=crates/openmind-desktop/src/probe.rs
grep -q 'pub const DEFAULT_PORT: u16 = 18181;' "$lib" || bad "DEFAULT_PORT is not 18181"
grep -q 'pub const DEFAULT_BIND: &str = "127.0.0.1:18181";' "$lib" || bad "DEFAULT_BIND is not 127.0.0.1:18181"
grep -q 'Service::Console => 18080,' "$probe_src" || bad "console probe port is not 18080"

desktop_units=0
unit_count=0
while IFS= read -r file; do
  unit_count=$((unit_count + 1))
  body=$(strip_comments < "$file")
  if printf '%s\n' "$body" | grep -q '18081'; then
    bad "$file mentions port 18081"
  fi
  if printf '%s\n' "$body" | grep -q '\[::\]'; then
    bad "$file binds the unspecified IPv6 address"
  fi
  other=$(printf '%s\n' "$body" | grep -oE '[0-9]{1,3}(\.[0-9]{1,3}){3}' | grep -v '^127\.0\.0\.1$' || true)
  if [ -n "$other" ]; then
    bad "$file has a non-loopback address: $(printf '%s' "$other" | tr '\n' ' ')"
  fi
  if printf '%s\n' "$body" | grep -q 'openmind-desktop'; then
    desktop_units=$((desktop_units + 1))
    if ! printf '%s\n' "$body" | grep -q '127\.0\.0\.1:18181'; then
      bad "$file names openmind-desktop without bind 127.0.0.1:18181"
    fi
  fi
done < <(find deploy -type f \( -name '*.service' -o -name '*.conf' \) | sort)

probe_unit=deploy/console-rust-serve/live/console-rust-primary.service
probe_drop=deploy/console-rust-serve/console-rust-primary.service.d/60-admin-store.conf
probe_body=$(strip_comments < "$probe_unit")
printf '%s\n' "$probe_body" | grep -q -- '--bind 127.0.0.1:18080' \
  || bad "probe unit does not bind 127.0.0.1:18080"
drop_body=$(strip_comments < "$probe_drop")
printf '%s\n' "$drop_body" | grep -q -- '--bind 127.0.0.1:18080' \
  || bad "probe drop-in does not bind 127.0.0.1:18080"

printf 'linux-units: files=%s openmind-desktop-units=%s\n' "$unit_count" "$desktop_units"
if [ "$desktop_units" -eq 0 ]; then
  printf 'linux-units: no openmind-desktop unit; listener default is 127.0.0.1:18181 in %s\n' "$lib"
fi

if ! command -v systemd-analyze >/dev/null 2>&1; then
  printf 'linux-units: systemd-analyze not available; bind scan only\n'
else
  printf 'linux-units: %s\n' "$(systemd-analyze --version | head -n 1)"
  while IFS= read -r file; do
    set +e
    out=$(systemd-analyze verify "$file" 2>&1)
    status=$?
    set -e
    filtered=$(printf '%s\n' "$out" | grep -v 'is not executable: No such file or directory' \
      | grep -v 'StartLimitIntervalSec' \
      | grep -v 'CPUAccounting=' \
      | grep -v '/usr/lib/systemd/' \
      | grep -v '/lib/systemd/' \
      | sed '/^$/d' || true)
    if [ -n "$filtered" ]; then
      printf '%s\n' "$filtered" >&2
      bad "$file systemd-analyze reported an unexpected diagnostic"
    fi
    if [ "$status" -eq 0 ] && [ -z "$out" ]; then
      printf 'linux-units: verify %s clean\n' "$file"
    else
      printf '%s\n' "$out" | sed "s|^|linux-units: verify $file: |"
    fi
  done < <(find deploy -type f -name '*.service' | sort)
fi

if [ "$fail" -ne 0 ]; then
  printf 'linux-units: FAIL\n' >&2
  exit 1
fi
printf 'linux-units: ok\n'
