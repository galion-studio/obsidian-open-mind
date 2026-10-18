#!/bin/sh
# setup-vps.sh (ws/offsite): idempotent VPS-side install of the offsite pull path. Run as root
# from /opt/galion/offsite. Never prints private keys. Does not touch firewall/cloudflared/DNS,
# root's ssh access or sshd config (no AllowUsers/Match in effect; see README).
set -eu
umask 077
H=/opt/galion/offsite
U=galion-offsite
UHOME=/var/lib/galion-offsite
SECRETS=/root/galion-secrets
AGEKEY=$SECRETS/offsite.agekey
LKEY=$SECRETS/offsite-laptop/id_ed25519
RCPT=/etc/galion/offsite.recipient

[ "$(id -u)" = 0 ] || { echo "run as root" >&2; exit 1; }
command -v age >/dev/null && command -v age-keygen >/dev/null || { echo "need age + age-keygen" >&2; exit 1; }

# 1. serve script = forced command and login shell
install -o root -g root -m 0755 "$H/galion-offsite-serve" /usr/local/bin/galion-offsite-serve

# 2. system user, no password ('*' = no valid password; '!' would read as locked to sshd)
getent group "$U" >/dev/null || groupadd --system "$U"
if ! id "$U" >/dev/null 2>&1; then
  useradd --system --gid "$U" --home-dir "$UHOME" --no-create-home \
          --shell /usr/local/bin/galion-offsite-serve --comment "Galion offsite pull (read-only)" "$U"
fi
usermod -s /usr/local/bin/galion-offsite-serve -p '*' "$U"
install -d -o root -g root -m 0755 "$UHOME" "$UHOME/.ssh"   # root-owned: user can't edit its keys

# 3. offsite age keypair (private stays root-only, VPS only needs the recipient)
install -d -o root -g root -m 0700 "$SECRETS" "$SECRETS/offsite-laptop"
[ -s "$AGEKEY" ] || age-keygen -o "$AGEKEY" 2>/dev/null
chown root:root "$AGEKEY"; chmod 0600 "$AGEKEY"
install -d -o root -g root -m 0755 /etc/galion
age-keygen -y "$AGEKEY" > "$RCPT.tmp" && chmod 0644 "$RCPT.tmp" && mv -f "$RCPT.tmp" "$RCPT"

# 4. laptop ssh keypair (private fetched by admin over root ssh; never printed)
[ -s "$LKEY" ] || ssh-keygen -q -t ed25519 -N '' -C "galion-offsite-laptop" -f "$LKEY"
chmod 0600 "$LKEY"; chmod 0644 "$LKEY.pub"
printf 'restrict,command="/usr/local/bin/galion-offsite-serve" %s\n' "$(cat "$LKEY.pub")" > "$UHOME/.ssh/authorized_keys.tmp"
chmod 0644 "$UHOME/.ssh/authorized_keys.tmp"; mv -f "$UHOME/.ssh/authorized_keys.tmp" "$UHOME/.ssh/authorized_keys"

# 5. systemd hook after galion-backup.service
install -d -m 0755 /etc/systemd/system/galion-backup.service.d
install -m 0644 "$H/galion-backup.service.d-offsite.conf" /etc/systemd/system/galion-backup.service.d/offsite.conf
systemctl daemon-reload

# 6. perms + encrypt whatever plaintext archives exist now
"$H/galion-offsite-encrypt.sh"
echo "setup ok. recipient: $(cat "$RCPT")"
