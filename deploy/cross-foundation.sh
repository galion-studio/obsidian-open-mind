#!/bin/sh
# Static musl builds of galion-health + galion-backup via zig cc (see UNIVERSAL-CPU.md).
set -eu
ZIG=${ZIG:-/workspace/oom-tools/zig-linux-x86_64-0.13.0/zig}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/workspace/oom-ws/foundation-target}
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
mk() { printf '#!/bin/sh\nargs=""\nfor a in "$@"; do case "$a" in --target=*) ;; *) args="$args \\"$a\\"" ;; esac; done\neval exec "%s" cc -target %s $args\n' "$ZIG" "$2" > "$W/cc-$1"; chmod +x "$W/cc-$1"; }
printf '#!/bin/sh\nexec "%s" ar "$@"\n' "$ZIG" > "$W/ar"; chmod +x "$W/ar"
run() { t=$1; z=$2; cmd=$3; mk "$t" "$z"; l=$(echo "$t" | tr - _); u=$(echo "$l" | tr a-z A-Z)
  env "CC_$l=$W/cc-$t" "AR_$l=$W/ar" "CARGO_TARGET_${u}_LINKER=rust-lld" \
    nice -n 19 cargo "$cmd" -j 2 --release -p galion-health -p galion-backup --target "$t"; }
run x86_64-unknown-linux-musl x86_64-linux-musl build
[ "${ALL:-0}" = 1 ] && { run aarch64-unknown-linux-musl aarch64-linux-musl build; run armv7-unknown-linux-musleabihf arm-linux-musleabihf build; run aarch64-linux-android aarch64-linux-musl check; }
true
