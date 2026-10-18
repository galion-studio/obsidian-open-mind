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

## Windows laptop (no root ssh needed, private key never leaves the laptop)
1. Admin runs `powershell -NoProfile -ExecutionPolicy Bypass -File .\install-galion-offsite.ps1`
   (`laptop/windows/`, PowerShell 5.1, no admin). It makes `%USERPROFILE%\.galion-offsite\id_ed25519`
   (user-only ACL), pins the VPS host key (SHA256:35BjCqYWlUUOEZIos88Y6csdExE4Riql18I2Q+pRzRI) in
   `known_hosts`, writes `galion-offsite-pull.ps1`, registers task `GalionOffsitePull`
   (02:45 08:45 14:45 20:45 local, StartWhenAvailable) and prints the PUBLIC key line.
2. VPS (root): `/opt/galion/offsite/add-laptop-key.sh 'ssh-ed25519 AAAA... comment'`
   (validates one ed25519 key, adds the restrict/command prefix, de-dups; `--remove`, `--list`).
3. Pull: `list` -> `get <newest>` via Start-Process -RedirectStandardOutput (binary-safe) -> Get-FileHash
   == list -> `%USERPROFILE%\GalionBackups` (or `GALION_BACKUP_DIR`), keep 8. Log `.galion-offsite\pull.log`.
   No decryption on Windows; the agekey stays in the password manager.
`install-galion-offsite.ps1` embeds `galion-offsite-pull.ps1`; regenerate with `laptop/windows/build-install.sh`.

## Restore (on a trusted machine)
    age -d -i ~/.galion-offsite/offsite.agekey ~/GalionBackups/galion-<stamp>.tar.gz.age | tar -C /restore -xzf -
    cd /restore/galion-*; sha256sum -c SHA256SUMS; galion-backup verify db/*.db; tar -C restored -xf files/files.tar

## Revoke laptop
`add-laptop-key.sh --remove 'ssh-ed25519 AAAA...'` (one key; `--list` shows fingerprints) or `: > /var/lib/galion-offsite/.ssh/authorized_keys` (all), then rotate with
`rm /root/galion-secrets/offsite-laptop/id_ed25519*; /opt/galion/offsite/setup-vps.sh`.
