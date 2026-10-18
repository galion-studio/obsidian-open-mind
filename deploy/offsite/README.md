# Galion offsite backups (ws/offsite)

Free offsite copy of the VPS backups, **pulled** by the admin laptop. Only age ciphertext leaves the VPS.

## How it works
1. `galion-backup.timer` (00/6:17 UTC, +<=5 min) runs `galion-backup.service` -> plaintext
   `/var/backups/galion/galion-<stamp>.tar.gz` (0600 root, contains secrets).
2. Drop-in `galion-backup.service.d/offsite.conf` -> `ExecStartPost=galion-offsite-encrypt.sh`:
   `age -e -R /etc/galion/offsite.recipient` (public key only) ->
   `/var/backups/galion/offsite/galion-<stamp>.tar.gz.age` (0640 root:galion-offsite), keep 12,
   `latest` symlink, `SHA256SUMS` (oldest -> newest). `ExecStopPost=... --perms-only` always re-sets
   `/var/backups/galion` to 0710 root:galion-offsite (traverse only; galion-backup.sh resets it to 0700).
3. User `galion-offsite` (system, no password, home `/var/lib/galion-offsite` root-owned, login shell =
   `/usr/local/bin/galion-offsite-serve`). One key in `authorized_keys` with
   `restrict,command="/usr/local/bin/galion-offsite-serve"`: `list | get <name> | latest`, nothing else.
4. Laptop: `galion-offsite-pull.sh` (launchd / systemd --user / cron) -> `~/GalionBackups/` (keep 8),
   sha256 checked against `list`, optional `age -d | tar tz` check (no plaintext on disk).

## Keys (never in git)
| file (VPS, root 0600) | purpose |
|---|---|
| `/root/galion-secrets/offsite.agekey` | age identity: decrypts offsite archives. VPS does not use it. Copy to laptop + password manager. |
| `/etc/galion/offsite.recipient` | public recipient (0644) |
| `/root/galion-secrets/offsite-laptop/id_ed25519` | laptop ssh key for galion-offsite (pull-only) |

## Install
VPS (idempotent): `/opt/galion/offsite/setup-vps.sh`
Laptop: `sh -c "$(ssh root@51.38.137.193 cat /opt/galion/offsite/laptop/install-laptop.sh)"`
(custom root key: prefix `GALION_ROOT_SSH_OPTS="-i ~/.ssh/KEY -o IdentitiesOnly=yes"` and add the same to the inner ssh.)

## Restore (on a trusted machine)
    age -d -i ~/.galion-offsite/offsite.agekey ~/GalionBackups/galion-<stamp>.tar.gz.age | tar -C /restore -xzf -
    cd /restore/galion-*; sha256sum -c SHA256SUMS; galion-backup verify db/*.db; tar -C restored -xf files/files.tar

## Revoke laptop
`: > /var/lib/galion-offsite/.ssh/authorized_keys` (or delete the key line), then rotate with
`rm /root/galion-secrets/offsite-laptop/id_ed25519*; /opt/galion/offsite/setup-vps.sh`.
