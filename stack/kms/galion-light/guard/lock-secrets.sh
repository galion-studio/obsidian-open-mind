#!/bin/bash
# Live secrets: /root/galion-secrets mode 400.
# Volume copies are ciphertext. A SEALED stub is not a secret and must never be copied over the live file.
set -u
LIVE=/root/galion-secrets
mkdir -p "$LIVE"
chmod 700 "$LIVE" || true
KEY=$LIVE/vault.key
if [ ! -s "$KEY" ]; then
  umask 077
  openssl rand -base64 48 > "$KEY"
  chmod 400 "$KEY" || true
fi

is_sealed() {
  [ -f "$1" ] && grep -q '^SEALED$' "$1"
}

lock_live() {
  if [ -f "$1" ]; then chmod 400 "$1" || true; fi
}

seal_vol() {
  vol=$1
  if [ -L "$vol" ]; then return 0; fi
  if [ ! -s "$vol" ] || is_sealed "$vol"; then return 0; fi
  openssl enc -aes-256-cbc -pbkdf2 -salt -pass "file:$KEY" -in "$vol" -out "$vol.sealed.tmp"
  if openssl enc -d -aes-256-cbc -pbkdf2 -salt -pass "file:$KEY" -in "$vol.sealed.tmp" | cmp -s - "$vol"; then
    mv "$vol.sealed.tmp" "$vol.sealed"
    printf 'SEALED\n' > "$vol"
  else
    rm -f "$vol.sealed.tmp"
    echo "seal failed" >&2
    return 1
  fi
}

promote() {
  name=$1
  vol=$2
  live=$LIVE/$name
  if [ -s "$live" ] && ! is_sealed "$live"; then
    lock_live "$live"
    seal_vol "$vol"
    return 0
  fi
  if [ -f "$vol.sealed" ]; then
    openssl enc -d -aes-256-cbc -pbkdf2 -salt -pass "file:$KEY" -in "$vol.sealed" -out "$live"
    lock_live "$live"
    return 0
  fi
  if [ -s "$vol" ] && ! is_sealed "$vol"; then
    cp "$vol" "$live"
    lock_live "$live"
    seal_vol "$vol"
  fi
}

promote github.token /workspace/galion-light/secrets/github.token
promote galion-app.token /workspace/galion-light/secrets/galion-app.token
promote hermes.key /workspace/galion-light/secrets/hermes.key
promote hermes.env /workspace/hermes/.env
promote github_ed25519 /workspace/galion-light/secrets/github_ed25519
lock_live "$KEY"
lock_live "$LIVE/admin-device.json"
lock_live "$LIVE/defender.pin"
