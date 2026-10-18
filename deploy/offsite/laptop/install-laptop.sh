#!/bin/sh
# install-laptop.sh (ws/offsite): set up the Galion offsite pull on the admin's laptop (macOS/Linux).
# Uses the admin's EXISTING root ssh to fetch, into ~/.galion-offsite/ (0700, files 0600):
#   id_ed25519       laptop key for galion-offsite@VPS (pull-only, forced command)
#   offsite.agekey   age identity to decrypt/verify the backups (keep it safe, see README)
#   known_hosts      VPS host key pinned via the trusted root session
# then installs the scheduler (launchd / systemd --user / cron) and runs a first pull.
# One-liner:  sh -c "$(ssh root@51.38.137.193 cat /opt/galion/offsite/laptop/install-laptop.sh)"
# Env: GALION_VPS (51.38.137.193), GALION_ROOT_SSH_OPTS (e.g. "-i ~/.ssh/id_ed25519_vps -o IdentitiesOnly=yes"),
#      GALION_OFFSITE_SCHED (auto|launchd|systemd|cron|none), GALION_OFFSITE_NO_AGEKEY=1 to skip the agekey.
set -eu
umask 077
VPS=${GALION_VPS:-51.38.137.193}
O=${GALION_ROOT_SSH_OPTS:-}
CFG=$HOME/.galion-offsite
R=/opt/galion/offsite/laptop
SCHED=${GALION_OFFSITE_SCHED:-auto}
say() { echo "install-laptop: $*"; }
# shellcheck disable=SC2086
fetch() { scp -q $O "root@$VPS:$1" "$2" </dev/null && chmod "$3" "$2"; }

mkdir -p "$CFG"; chmod 700 "$CFG"
fetch /root/galion-secrets/offsite-laptop/id_ed25519 "$CFG/id_ed25519" 600
[ "${GALION_OFFSITE_NO_AGEKEY:-0}" = 1 ] || fetch /root/galion-secrets/offsite.agekey "$CFG/offsite.agekey" 600
fetch "$R/galion-offsite-pull.sh" "$CFG/galion-offsite-pull.sh" 700
# shellcheck disable=SC2086
ssh -n $O "root@$VPS" cat /etc/ssh/ssh_host_ed25519_key.pub | awk -v h="$VPS" 'NF>=2{print h, $1, $2}' > "$CFG/known_hosts"
[ -s "$CFG/known_hosts" ] || { say "could not pin host key"; exit 1; }
chmod 600 "$CFG/known_hosts"
say "keys in $CFG (0600)"

[ "$SCHED" = auto ] && { if [ "$(uname -s)" = Darwin ]; then SCHED=launchd
  elif command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then SCHED=systemd
  else SCHED=cron; fi; }
case "$SCHED" in
  launchd)
    P=$HOME/Library/LaunchAgents/com.galion.offsite-pull.plist
    mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
    fetch "$R/com.galion.offsite-pull.plist" "$CFG/plist.tmpl" 600
    sed "s#__HOME__#$HOME#g" "$CFG/plist.tmpl" > "$P"; rm -f "$CFG/plist.tmpl"; chmod 644 "$P"
    launchctl bootout "gui/$(id -u)/com.galion.offsite-pull" 2>/dev/null || true
    launchctl bootstrap "gui/$(id -u)" "$P"
    say "launchd agent loaded: $P (02:45 08:45 14:45 20:45 local)" ;;
  systemd)
    D=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user; mkdir -p "$D"
    fetch "$R/galion-offsite-pull.service" "$D/galion-offsite-pull.service" 644
    fetch "$R/galion-offsite-pull.timer" "$D/galion-offsite-pull.timer" 644
    systemctl --user daemon-reload
    systemctl --user enable --now galion-offsite-pull.timer
    say "systemd user timer enabled (00/6:45 UTC = 02:45 08:45 14:45 20:45 CEST). For pulls while logged out: sudo loginctl enable-linger $(id -un)" ;;
  cron)
    L="45 2,8,14,20 * * * $CFG/galion-offsite-pull.sh >> $CFG/pull.log 2>&1"
    ( crontab -l 2>/dev/null | grep -v 'galion-offsite-pull.sh'; echo "$L" ) | crontab -
    say "cron line installed: $L" ;;
  none) say "no scheduler installed" ;;
  *) say "unknown GALION_OFFSITE_SCHED=$SCHED"; exit 1 ;;
esac

say "first pull..."
"$CFG/galion-offsite-pull.sh"
say "done. Backups: ${GALION_BACKUP_DIR:-$HOME/GalionBackups}"
