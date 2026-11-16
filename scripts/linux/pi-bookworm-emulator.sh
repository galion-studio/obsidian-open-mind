#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: 3000 Galion / galion-studio
#
# CI only. Build a Debian Bookworm arm64 root with debootstrap, run
# pi-bookworm-install.sh twice inside it under qemu-user, and check the
# desktop probe JSON.
#
# This is not Raspberry Pi OS. It is not a boot on Raspberry Pi 5 hardware.
# The install script prints that on every run because PI_BOOKWORM_EMULATOR=1.
set -euo pipefail

EMULATOR_LINE='pi-bookworm: emulator result, not a Raspberry Pi 5 result and not a Raspberry Pi OS boot'
NO_SYSTEMD_LINE='pi-bookworm: systemd is not running; starting the same commands in the background'

printf '%s\n' "$EMULATOR_LINE"
printf '%s\n' 'pi-bookworm: Debian Bookworm arm64 root under qemu-user. Not Raspberry Pi OS and not a Raspberry Pi 5.'

if [ "$#" -ne 1 ]; then
  printf 'usage: scripts/linux/pi-bookworm-emulator.sh SEED_DIR\n' >&2
  exit 2
fi

seed=$1
if [ ! -f "$seed/openmind-desktop" ]; then
  nested=$(find "$seed" -type f -name openmind-desktop -print -quit 2>/dev/null || true)
  if [ -n "$nested" ]; then
    seed=$(dirname -- "$nested")
  fi
fi
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH= cd -- "$script_dir/../.." && pwd)
install=$repo/scripts/linux/pi-bookworm-install.sh

[ -d "$seed" ] || { printf 'pi-bookworm: seed directory is missing\n' >&2; exit 1; }
[ -f "$seed/openmind-desktop" ] || { printf 'pi-bookworm: seed is missing openmind-desktop\n' >&2; exit 1; }
[ -f "$install" ] || { printf 'pi-bookworm: install script is missing\n' >&2; exit 1; }

sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q \
  debootstrap debian-archive-keyring qemu-user binfmt-support
if apt-cache show qemu-user-static 2>/dev/null | grep -q '^Package: qemu-user-static$'; then
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q qemu-user-static
fi
if ! command -v qemu-aarch64-static >/dev/null 2>&1 && command -v qemu-aarch64 >/dev/null 2>&1; then
  sudo ln -sfn "$(command -v qemu-aarch64)" /usr/local/bin/qemu-aarch64-static
fi
if ! command -v qemu-aarch64-static >/dev/null 2>&1 && ! command -v qemu-aarch64 >/dev/null 2>&1; then
  printf 'pi-bookworm: qemu-aarch64 is missing\n' >&2
  exit 1
fi

root=
work=$(mktemp -d)
cleanup() {
  if [ -n "$root" ] && [ -d "$root" ]; then
    sudo umount "$root/dev/pts" 2>/dev/null || true
    sudo umount "$root/dev" 2>/dev/null || true
    sudo umount "$root/proc" 2>/dev/null || true
    sudo umount "$root/sys" 2>/dev/null || true
  fi
}
trap cleanup EXIT

root=$(mktemp -d)
# mktemp is mode 700 and owned by the runner. Root can still enter the
# chroot. The obsidian user cannot search that directory, so execve returns
# EACCES for every binary, including /usr/bin/env. 755 lets that user in.
chmod 755 "$root"
sudo debootstrap --variant=minbase --arch=arm64 bookworm "$root" http://deb.debian.org/debian
sudo chmod 755 "$root"

sudo mkdir -p "$root/proc" "$root/sys" "$root/dev" "$root/dev/pts"
sudo mount --bind /proc "$root/proc"
sudo mount --bind /sys "$root/sys"
sudo mount --bind /dev "$root/dev"
sudo mount --bind /dev/pts "$root/dev/pts"

sudo mkdir -p "$root/opt/obsidian/seed" "$root/usr/local/sbin"
sudo cp -a "$seed"/. "$root/opt/obsidian/seed/"
sudo cp "$install" "$root/usr/local/sbin/pi-bookworm-install.sh"
sudo chmod 755 "$root/usr/local/sbin/pi-bookworm-install.sh"

run_in() {
  sudo chroot "$root" /usr/bin/env PI_BOOKWORM_EMULATOR=1 \
    /bin/bash /usr/local/sbin/pi-bookworm-install.sh "$@"
}

run_in --seed /opt/obsidian/seed 2>&1 | tee "$work/install-1.log"
grep -F "$EMULATOR_LINE" "$work/install-1.log" >/dev/null
grep -F "$NO_SYSTEMD_LINE" "$work/install-1.log" >/dev/null
grep -F 'pi-bookworm: installed' "$work/install-1.log" >/dev/null
if grep -F 'pi-bookworm: unchanged' "$work/install-1.log" >/dev/null; then
  printf 'pi-bookworm: first install reported unchanged\n' >&2
  exit 1
fi
started=$(grep -c 'pi-bookworm: started ' "$work/install-1.log" || true)
if [ "$started" -ne 8 ]; then
  printf 'pi-bookworm: first install started %s services, want 8\n' "$started" >&2
  exit 1
fi

pids() {
  sudo chroot "$root" /bin/bash -c 'cd /var/lib/obsidian/run && LC_ALL=C ls *.pid | while read -r f; do printf "%s " "$f"; tr -d "[:space:]" < "$f"; printf "\n"; done'
}
pids > "$work/pids-1.txt"

run_in --seed /opt/obsidian/seed 2>&1 | tee "$work/install-2.log"
grep -F "$EMULATOR_LINE" "$work/install-2.log" >/dev/null
grep -F 'pi-bookworm: unchanged' "$work/install-2.log" >/dev/null
if grep -F 'pi-bookworm: started ' "$work/install-2.log" >/dev/null; then
  printf 'pi-bookworm: second install started a new process\n' >&2
  exit 1
fi
still=$(grep -c 'already running pid=' "$work/install-2.log" || true)
if [ "$still" -ne 8 ]; then
  printf 'pi-bookworm: second install kept %s services, want 8\n' "$still" >&2
  exit 1
fi
pids > "$work/pids-2.txt"
if ! cmp -s "$work/pids-1.txt" "$work/pids-2.txt"; then
  printf 'pi-bookworm: pids changed on the second install\n' >&2
  printf 'pi-bookworm: first\n' >&2
  cat "$work/pids-1.txt" >&2
  printf 'pi-bookworm: second\n' >&2
  cat "$work/pids-2.txt" >&2
  exit 1
fi

set +e
run_in probe >"$work/probe.json" 2>"$work/probe.err"
probe_status=$?
set -e
printf '%s\n' "$EMULATOR_LINE"
printf '%s\n' 'pi-bookworm: desktop --probe JSON from the emulator follows'
cat "$work/probe.json"
printf '\n'
if [ "$probe_status" -ne 0 ] && [ "$probe_status" -ne 1 ]; then
  printf 'pi-bookworm: probe exited %s\n' "$probe_status" >&2
  cat "$work/probe.err" >&2 || true
  exit 1
fi
grep -F "$EMULATOR_LINE" "$work/probe.err" >/dev/null
if ! PI_BOOKWORM_EMULATOR=1 "$install" check-report "$work/probe.json"; then
  printf '%s\n' 'pi-bookworm: service logs from the emulator follow' >&2
  sudo chroot "$root" /bin/bash -c 'for f in /var/lib/obsidian/log/*; do printf "== %s\n" "$f"; tail -n 40 "$f"; done' >&2 || true
  exit 1
fi
printf '%s\n' "$EMULATOR_LINE"
printf '%s\n' 'pi-bookworm: emulator install finished. These probe states are not Raspberry Pi 5 results.'
