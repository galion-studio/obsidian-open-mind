#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# Cross-build the Open Mind desktop binaries for aarch64-unknown-linux-gnu
# (Raspberry Pi OS Bookworm 64-bit) and, separately, record startup time and
# peak memory of openmind-desktop --probe on this machine.
#
# Binary names are read from scripts/linux/pi-bookworm-install.sh. The C
# compiler is scripts/install-zig.sh, the same installer the musl seed uses.
# This script does not build that seed, does not install onto a machine, and
# does not publish a release.
#
# measure writes the line "CI runner, not a Pi 5" to the job summary when
# GITHUB_STEP_SUMMARY is set, and always prints it. The numbers are whatever
# /usr/bin/time reported for this process. They are not a Raspberry Pi 5 result.
set -euo pipefail

TARGET=aarch64-unknown-linux-gnu
# Zig's versioned glibc target. 2.36 is the suffix this script passes.
# Bookworm's glibc matching that suffix is TODO(verify) on a Pi OS image;
# this repository does not contain that image.
ZIG_GLIBC=aarch64-linux-gnu.2.36
LABEL='CI runner, not a Pi 5'

usage() {
  cat << 'EOF'
usage: scripts/linux/pi-bookworm-gnu.sh list
       scripts/linux/pi-bookworm-gnu.sh build
       scripts/linux/pi-bookworm-gnu.sh measure

list     print the desktop binary names from pi-bookworm-install.sh
build    cargo --release --locked for aarch64-unknown-linux-gnu, then
         check each binary is a dynamic AArch64 ELF
measure  build the host openmind-desktop release and run --probe under
         /usr/bin/time. Wall seconds and maximum resident set size are
         printed and, when GITHUB_STEP_SUMMARY is set, appended there.
         The heading is: CI runner, not a Pi 5

exit 0  the subcommand finished
exit 2  bad arguments
exit 1  build, ELF check, or probe measurement failed
EOF
}

die() {
  printf 'pi-bookworm-gnu: %s\n' "$1" >&2
  exit 1
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
cd "$root"

install_sh=$root/scripts/linux/pi-bookworm-install.sh

bins() {
  [ -f "$install_sh" ] || die "missing $install_sh"
  awk '
    /^bins=\(/ { grab = 1; next }
    grab && /^\)/ { exit }
    grab {
      sub(/#.*/, "")
      gsub(/^[ \t]+|[ \t]+$/, "")
      if ($0 != "") print
    }
  ' "$install_sh"
}

# Package name for cargo -p. galion-console-serve is a bin of console-static
# (crates/console-static/Cargo.toml). The other names match the package.
pkg_for() {
  case "$1" in
    galion-console-serve) printf '%s\n' console-static ;;
    *) printf '%s\n' "$1" ;;
  esac
}

summary() {
  printf '%s\n' "$1"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '%s\n' "$1" >> "$GITHUB_STEP_SUMMARY"
  fi
}

setup_zig() {
  local zig_bin wrap
  zig_bin=$("$root/scripts/install-zig.sh" "$root/target/pi-bookworm-gnu-zig")
  wrap=$root/target/pi-bookworm-gnu-wrap
  mkdir -p "$wrap"
  # rustc passes -Wl,--fix-cortex-a53-843419. zig 0.15.2 rejects that
  # linker arg. Drop it. cc-rs also passes --target=<rust triple>, which zig
  # does not understand; the musl seed wrapper drops that the same way.
  cat > "$wrap/cc" << WRAP
#!/bin/sh
for a do
  shift
  case "\$a" in
    --target=*) continue ;;
    -Wl,--fix-cortex-a53-843419|--fix-cortex-a53-843419) continue ;;
  esac
  set -- "\$@" "\$a"
done
exec "$zig_bin" cc -target $ZIG_GLIBC "\$@"
WRAP
  chmod +x "$wrap/cc"
  printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$zig_bin" > "$wrap/ar"
  chmod +x "$wrap/ar"
  export CC_aarch64_unknown_linux_gnu=$wrap/cc
  export AR_aarch64_unknown_linux_gnu=$wrap/ar
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$wrap/cc
  export CFLAGS_aarch64_unknown_linux_gnu="-fno-asynchronous-unwind-tables -fno-unwind-tables"
  printf 'pi-bookworm-gnu: zig %s cc %s\n' "$("$zig_bin" version)" "$wrap/cc"
}

elf_dynamic_aarch64() {
  local path=$1 machine interp
  command -v readelf >/dev/null 2>&1 || die "readelf is required"
  machine=$(readelf -hW -- "$path" | sed -n 's/^ *Machine: *//p' | tr -d '\r')
  case "$machine" in
    *AArch64*) ;;
    *) die "$path machine '$machine' is not AArch64" ;;
  esac
  if ! readelf -lW -- "$path" | grep -q 'INTERP'; then
    die "$path has no interpreter (want a dynamic gnu binary, not static musl)"
  fi
  interp=$(readelf -lW -- "$path" | sed -n 's/.*Requesting program interpreter: //p' | tr -d '[]\r')
  case "$interp" in
    *ld-linux-aarch64.so.1*) ;;
    *) die "$path interpreter '$interp' is not ld-linux-aarch64.so.1" ;;
  esac
  if ! readelf -dW -- "$path" | grep -q 'libc\.so'; then
    die "$path does not need libc.so"
  fi
  printf 'pi-bookworm-gnu: binary=%s bytes=%s machine=%s interp=%s\n' \
    "$(basename -- "$path")" "$(stat -c '%s' -- "$path")" "$machine" "$interp"
}

cmd_list() {
  local bin n=0
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    printf '%s\n' "$bin"
    n=$((n + 1))
  done < <(bins)
  [ "$n" -gt 0 ] || die "no binaries in $install_sh"
}

cmd_build() {
  local jobs nice_level rel out bin pkg seen bytes machine interp
  local -a args=()
  if ! command -v rustc >/dev/null 2>&1; then
    die "rustc is required"
  fi
  if ! rustup target list --installed | grep -qx "$TARGET"; then
    rustup target add "$TARGET"
  fi
  setup_zig
  jobs=${PI_BOOKWORM_GNU_JOBS:-2}
  nice_level=${PI_BOOKWORM_GNU_NICE:-19}
  seen=" "
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    pkg=$(pkg_for "$bin")
    case "$seen" in
      *" $pkg "*) ;;
      *)
        args+=(-p "$pkg")
        seen="$seen$pkg "
        ;;
    esac
  done < <(bins)
  [ "${#args[@]}" -gt 0 ] || die "no binaries in $install_sh"
  printf 'pi-bookworm-gnu: cargo build --release --locked --target %s %s\n' "$TARGET" "${args[*]}"
  nice -n "$nice_level" cargo build -j "$jobs" --release --locked --target "$TARGET" "${args[@]}"
  rel=$root/target/$TARGET/release
  out=$root/target/pi-bookworm-gnu
  mkdir -p "$out"
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    [ -f "$rel/$bin" ] || die "missing $rel/$bin"
    cp -- "$rel/$bin" "$out/$bin"
    chmod 755 "$out/$bin"
    elf_dynamic_aarch64 "$out/$bin"
  done < <(bins)
  summary "# $LABEL"
  summary ""
  summary "Cross-build for \`$TARGET\` (zig \`$ZIG_GLIBC\`). Names come from \`scripts/linux/pi-bookworm-install.sh\`. These binaries were not executed. Not a Raspberry Pi 5 result."
  summary ""
  summary "| binary | bytes | machine | interpreter |"
  summary "| --- | --- | --- | --- |"
  while IFS= read -r bin; do
    [ -n "$bin" ] || continue
    bytes=$(stat -c '%s' -- "$out/$bin")
    machine=$(readelf -hW -- "$out/$bin" | sed -n 's/^ *Machine: *//p' | tr -d '\r')
    interp=$(readelf -lW -- "$out/$bin" | sed -n 's/.*Requesting program interpreter: //p' | tr -d '[]\r')
    summary "| $bin | $bytes | $machine | $interp |"
  done < <(bins)
  summary ""
  summary "$LABEL"
}

host_expect_machine() {
  local host=$1
  case "$host" in
    aarch64-*) printf '%s\n' AArch64 ;;
    x86_64-*) printf '%s\n' X86-64 ;;
    *) die "no machine check for host $host" ;;
  esac
}

cmd_measure() {
  local jobs nice_level host expect machine bin time_file probe_out probe_err status elapsed rss
  local gnu_elapsed start_s end_s
  if ! command -v rustc >/dev/null 2>&1; then
    die "rustc is required"
  fi
  [ -x /usr/bin/time ] || die "/usr/bin/time is required (GNU time)"
  command -v timeout >/dev/null 2>&1 || die "timeout is required"
  command -v readelf >/dev/null 2>&1 || die "readelf is required"
  command -v awk >/dev/null 2>&1 || die "awk is required"
  command -v date >/dev/null 2>&1 || die "date is required"
  jobs=${PI_BOOKWORM_GNU_JOBS:-2}
  nice_level=${PI_BOOKWORM_GNU_NICE:-19}
  host=$(rustc -vV | sed -n 's/^host: //p')
  [ -n "$host" ] || die "rustc -vV did not report a host"
  expect=$(host_expect_machine "$host")
  printf 'pi-bookworm-gnu: host cargo build --release --locked -p openmind-desktop\n'
  nice -n "$nice_level" cargo build -j "$jobs" --release --locked -p openmind-desktop
  bin=$root/target/release/openmind-desktop
  [ -x "$bin" ] || die "missing $bin"
  machine=$(readelf -hW -- "$bin" | sed -n 's/^ *Machine: *//p' | tr -d '\r')
  case "$machine" in
    *"$expect"*) ;;
    *) die "$bin machine '$machine' is not $expect (host $host)" ;;
  esac
  time_file=$(mktemp)
  probe_out=$(mktemp)
  probe_err=$(mktemp)
  # GNU time %e is centiseconds, so a fast --probe is reported as 0.00.
  # The wall clock here is date's nanosecond clock around that same run.
  start_s=$(date +%s.%N)
  set +e
  /usr/bin/time -f 'elapsed_s %e\nmax_rss_kb %M\n' -o "$time_file" \
    timeout 30 "$bin" --probe >"$probe_out" 2>"$probe_err"
  status=$?
  set -e
  end_s=$(date +%s.%N)
  printf 'pi-bookworm-gnu: probe exit %s\n' "$status"
  if [ -s "$probe_err" ]; then
    printf 'pi-bookworm-gnu: probe stderr:\n'
    cat "$probe_err"
  fi
  if [ "$status" -ne 0 ] && [ "$status" -ne 1 ]; then
    rm -f "$time_file" "$probe_out" "$probe_err"
    die "probe exit $status (want 0 or 1)"
  fi
  if ! [ -s "$probe_out" ]; then
    rm -f "$time_file" "$probe_out" "$probe_err"
    die "probe printed no report"
  fi
  gnu_elapsed=$(sed -n 's/^elapsed_s //p' "$time_file" | tr -d '\r')
  rss=$(sed -n 's/^max_rss_kb //p' "$time_file" | tr -d '\r')
  elapsed=$(awk -v s="$start_s" -v e="$end_s" 'BEGIN {
    if (s !~ /^[0-9]+([.][0-9]+)?$/ || e !~ /^[0-9]+([.][0-9]+)?$/) exit 2
    d = e - s
    if (d < 0) exit 3
    printf "%.3f\n", d
  }') || {
    rm -f "$time_file" "$probe_out" "$probe_err"
    die "could not read the wall clock around --probe"
  }
  printf 'pi-bookworm-gnu: gnu time elapsed_s %s\n' "$gnu_elapsed"
  if ! printf '%s\n' "$elapsed" | grep -Eq '^[0-9]+([.][0-9]+)?$'; then
    rm -f "$time_file" "$probe_out" "$probe_err"
    die "wall clock did not report elapsed seconds"
  fi
  if ! printf '%s\n' "$rss" | grep -Eq '^[0-9]+$'; then
    rm -f "$time_file" "$probe_out" "$probe_err"
    die "time did not report maximum resident set size"
  fi
  summary "# $LABEL"
  summary ""
  summary "Startup time and peak memory of \`openmind-desktop --probe\` (headless; no \`--listen\`) on this machine. Startup time is the wall clock around that process. Peak memory is /usr/bin/time maximum resident set size. The aarch64 binaries were not this process."
  summary ""
  summary "| field | value |"
  summary "| --- | --- |"
  summary "| label | $LABEL |"
  summary "| host | $host |"
  summary "| uname -m | $(uname -m) |"
  summary "| binary | target/release/openmind-desktop |"
  summary "| mode | --probe |"
  summary "| startup time (wall seconds) | $elapsed |"
  summary "| peak memory (maximum resident set, kbytes) | $rss |"
  summary "| probe exit | $status |"
  summary ""
  summary "$LABEL"
  summary ""
  summary "Probe report from this run:"
  summary ""
  summary '```json'
  summary "$(cat "$probe_out")"
  summary '```'
  rm -f "$time_file" "$probe_out" "$probe_err"
}

if [ "$#" -lt 1 ]; then
  usage >&2
  exit 2
fi

case "$1" in
  -h|--help|help)
    usage
    exit 0
    ;;
  list)
    [ "$#" -eq 1 ] || { usage >&2; exit 2; }
    cmd_list
    ;;
  build)
    [ "$#" -eq 1 ] || { usage >&2; exit 2; }
    cmd_build
    ;;
  measure)
    [ "$#" -eq 1 ] || { usage >&2; exit 2; }
    cmd_measure
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
