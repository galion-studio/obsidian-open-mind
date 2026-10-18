#!/bin/sh
# build-install.sh: regenerate install-galion-offsite.ps1 = install.head + galion-offsite-pull.ps1 + install.tail
# (the installer embeds the pull script in a single-quoted here-string so the admin pastes one file).
set -eu
cd "$(dirname "$0")"
grep -q "^'@" galion-offsite-pull.ps1 && { echo "pull script must not contain a line starting with '@" >&2; exit 1; }
LC_ALL=C grep -n '[^ -~]' install.head galion-offsite-pull.ps1 install.tail && { echo "non-ASCII or control chars found" >&2; exit 1; }
cat install.head galion-offsite-pull.ps1 install.tail > install-galion-offsite.ps1
echo "built install-galion-offsite.ps1 ($(wc -l < install-galion-offsite.ps1) lines)"
