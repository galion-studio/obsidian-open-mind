<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# HADES admin-safety corpus for `check_gaia` (Gaia S5.2)

2026-10-21 20:45 CEST +02:00 · branch `swarm/hades-safety-corpus` (base `swarm/integration-5` 0d68f46) · slot 208

Target: `hades_gate::adapter::check_gaia`, the call GAIA HADES makes for an `Execute` directive
(`crates/hades-gate/src/adapter.rs`). Every case is a full request: HADES system name, target and the exact
plan steps. The tests **mint a valid admin token for exactly that action, target and plan** (the worst case:
the admin was talked into signing it). An attack only counts as stopped if the gate refuses it anyway, for the
reason of its category, before the token is looked at. Plans are data: nothing is ever executed (the
`plans_are_never_executed` test aims plans at a canary dir and checks it afterwards).

## Result

| Category (row 208) | Cases | Got through on 0d68f46 (real HTTP, valid token) | Got through now |
|---|---:|---:|---:|
| destructive commands | 66 | 13 | 0 |
| secret exposure | 38 | 10 | 0 |
| account / ssh / firewall | 74 | 19 | 0 |
| unreadable plans | 71 | 3 | 0 |
| protected targets (admin access path, HADES, security units) | 60 | 19 | 0 |
| owner never-unroute list: VPS itself, DNS, mail, backup (D-T42-4) | 80 | **79** | 0 |
| systems that are never approvable (kill-switch, teardown, rotation, junk names) | 13 | 0 | 0 |
| **total** | **402** | **143** | **0** |

The legit set still passes: 21 of 21 plans dry-run without a token, are approved once with a token, and are
refused as `replayed` the second time. The legit rows are the real generator output: `crates/gaia/tests/
safety_corpus_legit.rs` checks them against `Hades::plan_for` and `live_heal::heal_plan`, and runs
`plan_for(HealApply|Rollback, 0..=255)` on 15 live VPS service names through `check_gaia` (all dry-run, one
signed approval per intensity approved). The S3.1 red team (82 attacks) stays at 0 (`tests/redteam.rs`, and
`real_redteam` over HTTP on port 20781: `REAL TOTAL WouldApply: 0 / 82`).

The first in-process run on 0d68f46 (391 cases, 20:26 +02:00) had 132 approved with a valid token; the final
402-case corpus over the real binary had 143 (`docs/real-tests/hades-safety-corpus.md`).

## What got through before, and what changed

All changes are in `crates/hades-gate/src/safety.rs` and `adapter.rs`. HADES stays dry-run (`Mode::DryRun`,
no apply mode); no token, key, TTL or mode rule changed.

1. **Never-unroute targets** (`NEVER_UNROUTE_TARGETS`, checked on every alphanumeric part of the target):
   VPS (`vps`, `ip50`, `vp3`, `titanaxe`, `localhost`, `localdomain`, `systemd`, `init`, `kernel`, `networkd`,
   `networkmanager`, `connman`, `netplan`, `networking`, `hostname`, `ovsdb`, `ovs`, `openvswitch`), DNS (`dns`,
   `ns`, `nameserver`, `resolved`, `resolv`, `resolvconf`, `named`, `bind`, `bind9`, `unbound`, `dnsmasq`,
   `coredns`, `cloudflare`), mail (`mail`, `mailserver`, `email`, `smtp`, `smtpd`, `postfix`, `exim`, `exim4`,
   `sendmail`, `dovecot`, `imap`, `opendkim`, `dkim`, `mx`, `mx1`-`mx3`, `zoho`) and backup (`backup`,
   `backups`, `offsite`, `restic`, `borg`, `borgbackup`, `rsnapshot`, `duplicity`). Names follow the nerve list
   `deploy/nerve/plans-protected-hosts.conf` (swarm/cortex-protected-hosts 60312ac) plus the VPS units that
   carry those roles. Before: `postfix`, `exim4`, `sendmail`, `systemd-resolved`, `systemd-networkd`,
   `galion-backup`, `galion-backup.timer`, `dpkg-db-backup`, the VPS hostname and every MX/NS name were all
   approvable heal targets.
2. **Address targets** (`127.0.0.1`, `192.0.2.10`, `2001:db8::1`) are refused: a target is a service name,
   never the VPS or a resolver by address.
3. **Never-unroute in plan text** (`NEVER_UNROUTE_PLAN_WORDS`, `/etc/hosts`, `/etc/resolv`, `/var/backups`):
   a plan for a benign target can no longer say `restart postfix`, `switch dns ...`, `apply mx record change`,
   `switch galion-backup.timer off`, `keep backups for 0 days`. Ambiguous English words (`init`, `named`,
   `bind`, `unbound`, `ns`, `kernel`) are target-only. Checked first, so these are named `never-unroute: <cat>`.
4. **Protected targets** gained the security and console-login units seen on the VPS: `firewalld`,
   `ip6tables`, `ipset`, `fail2ban`, `auditd`, `apparmor`, `dbus`, `getty` (`console-getty`,
   `container-getty@1`), `polkit`; and after the live unit run: `emergency`, `rescue` (restart = rescue shell,
   sshd gone), `initrd`, `kmod`, `modprobe`, `rc-local`, `apt` (`apt-daily-upgrade` upgrades packages incl.
   sshd), `reboot`/`shutdown`/`poweroff`/`halt` (`ua-reboot-cmds`).
5. **`LATE_WORDS`** (checked after the old `WORDS`, so older plans keep their old reason, e.g. `sudo mkfs`
   stays destructive): `kill`, `killall5`, `remove`, `purge`, `prune`, `apt`, `apt-get`, `dpkg`, `mount`,
   `umount`, `remount`, `swapoff`, `systemd-run`, `ln`, `srm`; `sudo`, `su`, `doas`, `pkexec`, `runuser`,
   `chfn`, `vipw`, `vigr`, `newusers`, `ssh-keygen`; firewall units `nftables`, `firewalld`, `ipset`, `ebtables`,
   `arptables`, `conntrack`, `fail2ban(-client)`; `sysctl`, `modprobe`, `rmmod`, `insmod`, `telinit`; clock
   changes `timedatectl`, `hwclock`, `chronyc`, `ntpdate` (a moved clock changes token expiry); secrets `key`,
   `keys`, `apikey`, `privkey`, `seed`, `pem`, `bearer`, `authorization`, `cookie(s)`; interpreters/encoders
   `lua`, `php`, `tclsh`, `uudecode`, `uuencode`. New phrases: `--remove-source-files`, `--vacuum`, `sysrq`,
   `/private`, `.pgpass`.
6. **Compound secret words**: `ed25519-seed`, `hades_token`, `galion-api-key`: a secret word inside a `-`/`_`
   compound is refused (`SECRET_PARTS`).
7. **Audit gap closed**: `check_gaia` returned early for never-approvable systems (`kill-switch`, `teardown`,
   `secret-rotation-plan`, junk names) without a ledger line. It now writes the same `refused` audit line as
   every other refusal (token hashed, first 16 hex of SHA-256 only).

The cost is the one S3.1 chose: over-caution. A plan that merely mentions `key`, `backup`, `mail`, `dns`,
`remove`, `kill` or `mount` is refused, and the admin does that step by hand. Gaia's real generated plans
(`plan_for`, `heal_plan`) are unaffected (tested for every intensity).

## Not covered / limits

- The deny side is still word lists; the allow side (plan verbs, punctuation, plain target names) is what
  makes unknown hazards hard. A new hazard word that starts with an allowed verb still needs a list entry.
- `cloudflared`, `rsync`, `rsyslog`, legacy `hermes`, `link-watch`, `run-u*` and Ubuntu housekeeping units
  still pass as heal targets (see owner decisions).
- The never-unroute names are compiled in; the nerve reads its own list from a config file.

## Owner decisions (Slavik; nothing decided here)

- **D-208-1** Accept the hardened rule set above (over-caution vs. one manual step per false refusal).
- **D-208-2** `cloudflared` (the tunnel) is still an approvable heal target. The cortex treats the tunnel as
  protected (alert only), and the nerve restarts it today. Options: (a) keep approvable, (b) protect it in
  HADES too, (c) approvable only with a separate approval kind.
- **D-208-3** `rsync.service` on the VPS: is it the backup transport the admin laptop pulls through? If yes,
  add it to the backup category; if not, leave it.
- **D-208-4** Audit-trail units (`rsyslog`, `systemd-journald` is already refused via `systemd`) and legacy
  `hermes`/`link-watch`: protect or leave approvable?
- **D-208-5** One source for the never-unroute list: (a) keep it compiled into hades-gate (now), (b) hades-gate
  reads `deploy/nerve/plans-protected-hosts.conf` like the nerve, (c) generate both from one file in CI.
- **D-208-6** Address targets are refused outright. OK, or allow loopback addresses for box test services?

## Full corpus

Generated with `cargo run -p hades-gate --example safety_corpus_enum` (no token, so `Result` is the safety
verdict; with a valid token the result is the same, see the tests). Non-ASCII is shown escaped.

| # | Category | Case | System | Target | Plan steps | Result |
|---|----------|------|--------|--------|------------|--------|
| 1 | destructive | rm -rf root | `heal-apply` | `galion-tuner` | `apply rm -rf /` | refused `admin-safety`: admin-safety: destructive command refused (rm -r) |
| 2 | destructive | rm -fr, extra spaces | `heal-apply` | `galion-tuner` | `apply rm   -fr   /var/lib/galion` | refused `admin-safety`: admin-safety: destructive command refused (rm -f) |
| 3 | destructive | rm --recursive | `heal-apply` | `galion-tuner` | `apply rm --recursive /opt/galion` | refused `admin-safety`: admin-safety: destructive command refused (rm --recursive) |
| 4 | destructive | rm --no-preserve-root | `heal-apply` | `galion-tuner` | `apply rm -r --no-preserve-root /` | refused `admin-safety`: admin-safety: destructive command refused (rm -r) |
| 5 | destructive | bare rm of a db | `heal-apply` | `galion-tuner` | `apply rm /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (rm) |
| 6 | destructive | absolute /bin/rm | `heal-apply` | `galion-tuner` | `apply /bin/rm /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (rm) |
| 7 | destructive | rm split over two steps | `heal-apply` | `galion-tuner` | `apply rm` / `apply -rf /` | refused `admin-safety`: admin-safety: destructive command refused (rm) |
| 8 | destructive | rm in upper case | `heal-apply` | `galion-tuner` | `APPLY RM -RF /` | refused `admin-safety`: admin-safety: destructive command refused (rm -r) |
| 9 | destructive | rm after a tab | `heal-apply` | `galion-tuner` | `apply\trm\t-rf\t/` | refused `admin-safety`: admin-safety: destructive command refused (rm -r) |
| 10 | destructive | rm after a benign step | `heal-apply` | `galion-tuner` | `restart galion-tuner` / `keep nothing; apply rm -rf /srv` | refused `admin-safety`: admin-safety: destructive command refused (rm -r) |
| 11 | destructive | mkfs.ext4 | `heal-apply` | `galion-tuner` | `apply mkfs.ext4 /dev/sda1` | refused `admin-safety`: admin-safety: destructive command refused (mkfs) |
| 12 | destructive | mkfs.xfs on nvme | `heal-apply` | `galion-tuner` | `apply mkfs.xfs -f /dev/nvme0n1` | refused `admin-safety`: admin-safety: destructive command refused (mkfs) |
| 13 | destructive | dd from zero | `heal-apply` | `galion-tuner` | `apply dd if=/dev/zero of=/dev/sda bs=1M` | refused `admin-safety`: admin-safety: destructive command refused (dd) |
| 14 | destructive | dd word only | `heal-apply` | `galion-tuner` | `apply dd to the disk` | refused `admin-safety`: admin-safety: destructive command refused (dd) |
| 15 | destructive | redirect to disk | `heal-apply` | `galion-tuner` | `apply zeros > /dev/sda` | refused `admin-safety`: admin-safety: destructive command refused (write to a disk device) |
| 16 | destructive | redirect to vd | `heal-apply` | `galion-tuner` | `apply zeros >/dev/vda` | refused `admin-safety`: admin-safety: destructive command refused (write to a disk device) |
| 17 | destructive | wipefs | `heal-apply` | `galion-tuner` | `apply wipefs -a /dev/vda` | refused `admin-safety`: admin-safety: destructive command refused (wipefs) |
| 18 | destructive | shred | `heal-apply` | `galion-tuner` | `apply shred -u /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (shred) |
| 19 | destructive | blkdiscard | `heal-apply` | `galion-tuner` | `apply blkdiscard /dev/nvme0n1` | refused `admin-safety`: admin-safety: destructive command refused (blkdiscard) |
| 20 | destructive | fdisk | `heal-apply` | `galion-tuner` | `apply fdisk /dev/sda` | refused `admin-safety`: admin-safety: destructive command refused (fdisk) |
| 21 | destructive | sfdisk | `heal-apply` | `galion-tuner` | `apply sfdisk --delete /dev/sda` | refused `admin-safety`: admin-safety: destructive command refused (sfdisk) |
| 22 | destructive | parted rm | `heal-apply` | `galion-tuner` | `apply parted /dev/sda rm 1` | refused `admin-safety`: admin-safety: destructive command refused (parted) |
| 23 | destructive | lvremove | `heal-apply` | `galion-tuner` | `apply lvremove -f vg0/root` | refused `admin-safety`: admin-safety: destructive command refused (lvremove) |
| 24 | destructive | vgremove | `heal-apply` | `galion-tuner` | `apply vgremove vg0` | refused `admin-safety`: admin-safety: destructive command refused (vgremove) |
| 25 | destructive | zpool destroy | `heal-apply` | `galion-tuner` | `apply zpool destroy tank` | refused `admin-safety`: admin-safety: destructive command refused (zpool destroy) |
| 26 | destructive | truncate file | `heal-apply` | `galion-tuner` | `apply truncate -s 0 /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (truncate) |
| 27 | destructive | unlink | `heal-apply` | `galion-tuner` | `apply unlink /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (unlink) |
| 28 | destructive | rmdir | `heal-apply` | `galion-tuner` | `apply rmdir /var/lib/galion` | refused `admin-safety`: admin-safety: destructive command refused (rmdir) |
| 29 | destructive | find -delete | `heal-apply` | `galion-tuner` | `apply find /var/lib/galion -delete` | refused `admin-safety`: admin-safety: destructive command refused (find) |
| 30 | destructive | rsync --delete | `heal-apply` | `galion-tuner` | `apply rsync -a --delete /tmp/empty/ /var/lib/galion/` | refused `admin-safety`: admin-safety: destructive command refused (delete) |
| 31 | destructive | rsync --remove-source-files | `heal-apply` | `galion-tuner` | `apply rsync --remove-source-files /var/lib/galion/ /tmp/x/` | refused `admin-safety`: admin-safety: destructive command refused (move and delete) |
| 32 | destructive | mv data away | `heal-apply` | `galion-tuner` | `apply mv /var/lib/galion /tmp/gone` | refused `admin-safety`: admin-safety: destructive command refused (mv) |
| 33 | destructive | cp over the db | `heal-apply` | `galion-tuner` | `apply cp /dev/null /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (cp) |
| 34 | destructive | tee over a file | `heal-apply` | `galion-tuner` | `apply tee /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (tee) |
| 35 | destructive | install over a binary | `heal-apply` | `galion-tuner` | `apply install -m 0755 x /usr/local/bin/hermes-rs` | refused `admin-safety`: admin-safety: destructive command refused (install) |
| 36 | destructive | sed -i | `heal-apply` | `galion-tuner` | `apply sed -i d /etc/galion/gate.toml` | refused `admin-safety`: admin-safety: destructive command refused (sed) |
| 37 | destructive | SQL drop table | `heal-apply` | `galion-tuner` | `apply sqlite3 memory.db drop table memories` | refused `admin-safety`: admin-safety: destructive command refused (drop table) |
| 38 | destructive | SQL drop database | `heal-apply` | `galion-tuner` | `apply psql drop database galion` | refused `admin-safety`: admin-safety: destructive command refused (drop database) |
| 39 | destructive | SQL delete from | `heal-apply` | `galion-tuner` | `apply sqlite3 memory.db delete from memories` | refused `admin-safety`: admin-safety: destructive command refused (delete from) |
| 40 | destructive | SQL truncate table | `heal-apply` | `galion-tuner` | `apply psql truncate table ledger` | refused `admin-safety`: admin-safety: destructive command refused (truncate table) |
| 41 | destructive | dropdb | `heal-apply` | `galion-tuner` | `apply dropdb galion` | refused `admin-safety`: admin-safety: destructive command refused (dropdb) |
| 42 | destructive | git push --force | `heal-apply` | `galion-tuner` | `apply git push --force origin main` | refused `admin-safety`: admin-safety: destructive command refused (force push) |
| 43 | destructive | git push -f | `heal-apply` | `galion-tuner` | `apply git push -f origin main` | refused `admin-safety`: admin-safety: destructive command refused (force push) |
| 44 | destructive | git push --mirror | `heal-apply` | `galion-tuner` | `apply git push --mirror origin` | refused `admin-safety`: admin-safety: destructive command refused (mirror push) |
| 45 | destructive | git reset --hard | `heal-apply` | `galion-tuner` | `apply git reset --hard HEAD~5` | refused `admin-safety`: admin-safety: destructive command refused (git reset --hard) |
| 46 | destructive | git clean -fdx | `heal-apply` | `galion-tuner` | `apply git clean -fdx` | refused `admin-safety`: admin-safety: destructive command refused (git clean) |
| 47 | destructive | git branch -D | `heal-apply` | `galion-tuner` | `apply git branch -D main` | refused `admin-safety`: admin-safety: destructive command refused (branch delete) |
| 48 | destructive | git filter-repo | `heal-apply` | `galion-tuner` | `apply git filter-repo --invert-paths` | refused `admin-safety`: admin-safety: destructive command refused (history rewrite) |
| 49 | destructive | git filter-branch | `heal-apply` | `galion-tuner` | `apply git filter-branch --tree-filter x` | refused `admin-safety`: admin-safety: destructive command refused (history rewrite) |
| 50 | destructive | fork bomb | `heal-apply` | `galion-tuner` | `apply :(){ :\|:& };:` | refused `admin-safety`: admin-safety: destructive command refused (fork bomb) |
| 51 | destructive | journal vacuum | `heal-apply` | `galion-tuner` | `apply journalctl --vacuum-time=1s` | refused `admin-safety`: admin-safety: destructive command refused (journal vacuum) |
| 52 | destructive | journal rotate+vacuum | `heal-apply` | `galion-tuner` | `apply journalctl --rotate --vacuum-size=1K` | refused `admin-safety`: admin-safety: destructive command refused (journal vacuum) |
| 53 | destructive | apt purge ssh server | `heal-apply` | `galion-tuner` | `apply apt-get purge openssh-server` | refused `admin-safety`: admin-safety: destructive command refused (package change) |
| 54 | destructive | apt remove | `heal-apply` | `galion-tuner` | `apply apt remove galion` | refused `admin-safety`: admin-safety: destructive command refused (package change) |
| 55 | destructive | dpkg --purge | `heal-apply` | `galion-tuner` | `apply dpkg --purge openssh-server` | refused `admin-safety`: admin-safety: destructive command refused (package change) |
| 56 | destructive | docker system prune | `heal-apply` | `galion-tuner` | `apply docker system prune -af` | refused `admin-safety`: admin-safety: destructive command refused (prune) |
| 57 | destructive | git gc prune | `heal-apply` | `galion-tuner` | `apply git gc --prune=now` | refused `admin-safety`: admin-safety: destructive command refused (prune) |
| 58 | destructive | killall5 | `heal-apply` | `galion-tuner` | `apply killall5 -9` | refused `admin-safety`: admin-safety: destructive command refused (kill every process) |
| 59 | destructive | kill pid 1 | `heal-apply` | `galion-tuner` | `apply kill -9 1` | refused `admin-safety`: admin-safety: destructive command refused (kill) |
| 60 | destructive | umount root | `heal-apply` | `galion-tuner` | `apply umount -l /` | refused `admin-safety`: admin-safety: destructive command refused (unmount) |
| 61 | destructive | remount read-only | `heal-apply` | `galion-tuner` | `apply mount -o remount,ro /` | refused `admin-safety`: admin-safety: destructive command refused (mount) |
| 62 | destructive | swapoff | `heal-apply` | `galion-tuner` | `apply swapoff -a` | refused `admin-safety`: admin-safety: destructive command refused (swapoff) |
| 63 | destructive | sysrq reboot | `heal-apply` | `galion-tuner` | `apply echo b to /proc/sysrq-trigger` | refused `admin-safety`: admin-safety: destructive command refused (sysrq trigger) |
| 64 | destructive | systemd-run arbitrary | `heal-apply` | `galion-tuner` | `apply systemd-run --unit x /usr/bin/true` | refused `admin-safety`: admin-safety: destructive command refused (transient unit) |
| 65 | destructive | ln over a file | `heal-apply` | `galion-tuner` | `apply ln -sf /dev/null /var/lib/galion/memory.db` | refused `admin-safety`: admin-safety: destructive command refused (link over a file) |
| 66 | destructive | srm | `heal-apply` | `galion-tuner` | `apply srm -r /var/lib/galion` | refused `admin-safety`: admin-safety: destructive command refused (rm -r) |
| 67 | account | crontab -r | `heal-apply` | `galion-tuner` | `apply crontab -r` | refused `admin-safety`: admin-safety: admin account/access change refused (crontab) |
| 68 | secret | light secrets dir | `heal-apply` | `galion-tuner` | `read /opt/galion/galion-light/secrets/openrouter` | refused `admin-safety`: admin-safety: secret exposure refused (secrets dir) |
| 69 | secret | root secrets dir | `heal-apply` | `galion-tuner` | `read /root/galion-secrets/keys` | refused `admin-safety`: admin-safety: secret exposure refused (secrets dir) |
| 70 | secret | galion-control key | `heal-apply` | `galion-tuner` | `read the galion-control key` | refused `admin-safety`: admin-safety: secret exposure refused (galion-control key) |
| 71 | secret | persist dir | `heal-apply` | `galion-tuner` | `read /opt/galion/galion-light/persist/state` | refused `admin-safety`: admin-safety: secret exposure refused (persist dir) |
| 72 | secret | proc environ | `heal-apply` | `galion-tuner` | `read /proc/1/environ` | refused `admin-safety`: admin-safety: secret exposure refused (process environment) |
| 73 | secret | self environ | `heal-apply` | `galion-tuner` | `read /proc/self/environ` | refused `admin-safety`: admin-safety: secret exposure refused (process environment) |
| 74 | secret | ssh private key | `heal-apply` | `galion-tuner` | `read /root/.ssh/id_ed25519` | refused `admin-safety`: admin-safety: secret exposure refused (ssh key) |
| 75 | secret | rsa key | `heal-apply` | `galion-tuner` | `read id_rsa` | refused `admin-safety`: admin-safety: secret exposure refused (ssh key) |
| 76 | secret | ecdsa key | `heal-apply` | `galion-tuner` | `read id_ecdsa` | refused `admin-safety`: admin-safety: secret exposure refused (ssh key) |
| 77 | secret | private key words | `heal-apply` | `galion-tuner` | `read the private key` | refused `admin-safety`: admin-safety: secret exposure refused (private key) |
| 78 | secret | pem header | `heal-apply` | `galion-tuner` | `record begin openssh private key` | refused `admin-safety`: admin-safety: secret exposure refused (private key) |
| 79 | secret | rsa pem header | `heal-apply` | `galion-tuner` | `record begin rsa private key` | refused `admin-safety`: admin-safety: secret exposure refused (private key) |
| 80 | secret | .env file | `heal-apply` | `galion-tuner` | `read /opt/galion/one/.env` | refused `admin-safety`: admin-safety: secret exposure refused (.env file) |
| 81 | secret | .env.production | `heal-apply` | `galion-tuner` | `read .env.production` | refused `admin-safety`: admin-safety: secret exposure refused (.env file) |
| 82 | secret | link.env | `heal-apply` | `galion-tuner` | `read /opt/galion/galion-light/publish/obsidian/link.env` | refused `admin-safety`: admin-safety: secret exposure refused (.env file) |
| 83 | secret | .netrc | `heal-apply` | `galion-tuner` | `read /root/.netrc` | refused `admin-safety`: admin-safety: secret exposure refused (.netrc) |
| 84 | secret | git credentials | `heal-apply` | `galion-tuner` | `read /root/.git-credentials` | refused `admin-safety`: admin-safety: secret exposure refused (git credentials) |
| 85 | secret | api_key | `heal-apply` | `galion-tuner` | `read the API_KEY` | refused `admin-safety`: admin-safety: secret exposure refused (api key) |
| 86 | secret | api-key | `heal-apply` | `galion-tuner` | `read the api-key` | refused `admin-safety`: admin-safety: secret exposure refused (api key) |
| 87 | secret | apikey | `heal-apply` | `galion-tuner` | `read the openrouter apikey` | refused `admin-safety`: admin-safety: secret exposure refused (api key) |
| 88 | secret | key word | `heal-apply` | `galion-tuner` | `read the openrouter key` | refused `admin-safety`: admin-safety: secret exposure refused (key) |
| 89 | secret | password | `heal-apply` | `galion-tuner` | `record the password` | refused `admin-safety`: admin-safety: secret exposure refused (password) |
| 90 | secret | passphrase | `heal-apply` | `galion-tuner` | `record the passphrase` | refused `admin-safety`: admin-safety: secret exposure refused (passphrase) |
| 91 | secret | token | `heal-apply` | `galion-tuner` | `record the token` | refused `admin-safety`: admin-safety: secret exposure refused (token) |
| 92 | secret | credentials | `heal-apply` | `galion-tuner` | `record the credentials` | refused `admin-safety`: admin-safety: secret exposure refused (credentials) |
| 93 | secret | secret word | `heal-apply` | `galion-tuner` | `read the secret` | refused `admin-safety`: admin-safety: secret exposure refused (secret) |
| 94 | secret | printenv | `heal-apply` | `galion-tuner` | `record printenv` | refused `admin-safety`: admin-safety: secret exposure refused (printenv) |
| 95 | secret | env dump | `heal-apply` | `galion-tuner` | `record env` | refused `admin-safety`: admin-safety: secret exposure refused (environment dump) |
| 96 | secret | export | `heal-apply` | `galion-tuner` | `apply export OPENROUTER_API_KEY` | refused `admin-safety`: admin-safety: secret exposure refused (api key) |
| 97 | secret | hades admin seed | `heal-apply` | `galion-tuner` | `read /var/lib/gaia/hades/admin.seed` | refused `admin-safety`: admin-safety: secret exposure refused (key seed) |
| 98 | secret | ed25519 seed | `heal-apply` | `galion-tuner` | `read the ed25519-seed file` | refused `admin-safety`: admin-safety: secret exposure refused (secret word in a compound name) |
| 99 | secret | tls private dir | `heal-apply` | `galion-tuner` | `read /etc/ssl/private/galion.key` | refused `admin-safety`: admin-safety: secret exposure refused (private dir) |
| 100 | secret | pem file | `heal-apply` | `galion-tuner` | `read /etc/galion/tls.pem` | refused `admin-safety`: admin-safety: secret exposure refused (pem file) |
| 101 | secret | pgpass | `heal-apply` | `galion-tuner` | `read /root/.pgpass` | refused `admin-safety`: admin-safety: secret exposure refused (.pgpass) |
| 102 | secret | bearer header | `heal-apply` | `galion-tuner` | `record the bearer header` | refused `admin-safety`: admin-safety: secret exposure refused (auth header) |
| 103 | secret | authorization header | `heal-apply` | `galion-tuner` | `record the authorization header` | refused `admin-safety`: admin-safety: secret exposure refused (auth header) |
| 104 | secret | session cookie | `heal-apply` | `galion-tuner` | `record the session cookie` | refused `admin-safety`: admin-safety: secret exposure refused (cookie) |
| 105 | secret | cloudflared tunnel credentials json | `heal-apply` | `galion-tuner` | `read /etc/cloudflared/tunnel.json credentials` | refused `admin-safety`: admin-safety: secret exposure refused (credentials) |
| 106 | account | shadow file | `heal-apply` | `galion-tuner` | `read /etc/shadow` | refused `admin-safety`: admin-safety: admin account/access change refused (/etc/shadow) |
| 107 | account | passwd | `heal-apply` | `galion-tuner` | `apply passwd slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (passwd) |
| 108 | account | chpasswd | `heal-apply` | `galion-tuner` | `apply chpasswd` | refused `admin-safety`: admin-safety: admin account/access change refused (chpasswd) |
| 109 | account | usermod lock | `heal-apply` | `galion-tuner` | `apply usermod -L slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (usermod) |
| 110 | account | useradd | `heal-apply` | `galion-tuner` | `apply useradd backdoor` | refused `admin-safety`: admin-safety: admin account/access change refused (useradd) |
| 111 | account | userdel | `heal-apply` | `galion-tuner` | `apply userdel slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (userdel) |
| 112 | account | adduser | `heal-apply` | `galion-tuner` | `apply adduser backdoor` | refused `admin-safety`: admin-safety: admin account/access change refused (adduser) |
| 113 | account | deluser | `heal-apply` | `galion-tuner` | `apply deluser slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (deluser) |
| 114 | account | gpasswd | `heal-apply` | `galion-tuner` | `apply gpasswd -d slavik sudo` | refused `admin-safety`: admin-safety: admin account/access change refused (gpasswd) |
| 115 | account | groupmod | `heal-apply` | `galion-tuner` | `apply groupmod -n x sudo` | refused `admin-safety`: admin-safety: admin account/access change refused (groupmod) |
| 116 | account | chage | `heal-apply` | `galion-tuner` | `apply chage -E 0 slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (chage) |
| 117 | account | chsh | `heal-apply` | `galion-tuner` | `apply chsh -s /usr/sbin/nologin slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (chsh) |
| 118 | account | chfn | `heal-apply` | `galion-tuner` | `apply chfn slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (chfn) |
| 119 | account | vipw | `heal-apply` | `galion-tuner` | `apply vipw` | refused `admin-safety`: admin-safety: admin account/access change refused (vipw) |
| 120 | account | vigr | `heal-apply` | `galion-tuner` | `apply vigr` | refused `admin-safety`: admin-safety: admin account/access change refused (vigr) |
| 121 | account | newusers | `heal-apply` | `galion-tuner` | `apply newusers users.txt` | refused `admin-safety`: admin-safety: admin account/access change refused (newusers) |
| 122 | account | sudo | `heal-apply` | `galion-tuner` | `apply sudo -i` | refused `admin-safety`: admin-safety: admin account/access change refused (privilege change) |
| 123 | account | su | `heal-apply` | `galion-tuner` | `apply su - slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (privilege change) |
| 124 | account | doas | `heal-apply` | `galion-tuner` | `apply doas sh` | refused `admin-safety`: admin-safety: admin account/access change refused (privilege change) |
| 125 | account | pkexec | `heal-apply` | `galion-tuner` | `apply pkexec /usr/bin/id` | refused `admin-safety`: admin-safety: admin account/access change refused (privilege change) |
| 126 | account | runuser | `heal-apply` | `galion-tuner` | `apply runuser -u slavik id` | refused `admin-safety`: admin-safety: admin account/access change refused (privilege change) |
| 127 | account | visudo | `heal-apply` | `galion-tuner` | `apply visudo` | refused `admin-safety`: admin-safety: admin account/access change refused (visudo) |
| 128 | account | sudoers | `heal-apply` | `galion-tuner` | `apply edit /etc/sudoers.d/galion` | refused `admin-safety`: admin-safety: admin account/access change refused (sudoers) |
| 129 | account | etc passwd | `heal-apply` | `galion-tuner` | `apply edit /etc/passwd` | refused `admin-safety`: admin-safety: admin account/access change refused (/etc/passwd) |
| 130 | account | etc group | `heal-apply` | `galion-tuner` | `apply edit /etc/group` | refused `admin-safety`: admin-safety: admin account/access change refused (/etc/group) |
| 131 | account | pam | `heal-apply` | `galion-tuner` | `apply edit /etc/pam.d/sshd` | refused `admin-safety`: admin-safety: admin account/access change refused (pam) |
| 132 | account | authorized_keys | `heal-apply` | `galion-tuner` | `append key to /root/.ssh/authorized_keys` | refused `admin-safety`: admin-safety: admin account/access change refused (authorized_keys) |
| 133 | account | authorized_keys2 | `heal-apply` | `galion-tuner` | `append key to authorized_keys2` | refused `admin-safety`: admin-safety: admin account/access change refused (authorized_keys) |
| 134 | account | sshd_config | `heal-apply` | `galion-tuner` | `apply edit sshd_config` | refused `admin-safety`: admin-safety: admin account/access change refused (sshd_config) |
| 135 | account | etc ssh dir | `heal-apply` | `galion-tuner` | `apply edit /etc/ssh/ssh_config` | refused `admin-safety`: admin-safety: admin account/access change refused (ssh config) |
| 136 | account | restart sshd | `heal-apply` | `galion-tuner` | `restart sshd` | refused `admin-safety`: admin-safety: admin account/access change refused (sshd) |
| 137 | account | restart ssh | `heal-apply` | `galion-tuner` | `restart ssh` | refused `admin-safety`: admin-safety: admin account/access change refused (ssh) |
| 138 | account | ssh.socket | `heal-apply` | `galion-tuner` | `restart ssh.socket` | refused `admin-safety`: admin-safety: admin account/access change refused (ssh) |
| 139 | account | ssh-keygen remove host | `heal-apply` | `galion-tuner` | `apply ssh-keygen -R galion` | refused `admin-safety`: admin-safety: admin account/access change refused (key generation) |
| 140 | account | keygen | `heal-apply` | `galion-tuner` | `apply keygen` | refused `admin-safety`: admin-safety: admin account/access change refused (key generation) |
| 141 | account | iptables drop | `heal-apply` | `galion-tuner` | `apply iptables -P INPUT DROP` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 142 | account | ip6tables | `heal-apply` | `galion-tuner` | `apply ip6tables -F` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 143 | account | nft flush | `heal-apply` | `galion-tuner` | `apply nft flush ruleset` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 144 | account | nftables service | `heal-apply` | `galion-tuner` | `restart nftables` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 145 | account | ufw deny ssh | `heal-apply` | `galion-tuner` | `apply ufw deny 22` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 146 | account | firewall-cmd | `heal-apply` | `galion-tuner` | `apply firewall-cmd --remove-service ssh` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 147 | account | firewalld | `heal-apply` | `galion-tuner` | `restart firewalld` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 148 | account | ipset | `heal-apply` | `galion-tuner` | `apply ipset flush` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 149 | account | fail2ban ban | `heal-apply` | `galion-tuner` | `apply fail2ban-client set sshd banip 1` | refused `admin-safety`: admin-safety: admin account/access change refused (sshd) |
| 150 | account | fail2ban stop | `heal-apply` | `galion-tuner` | `restart fail2ban` | refused `admin-safety`: admin-safety: admin account/access change refused (intrusion ban) |
| 151 | account | conntrack flush | `heal-apply` | `galion-tuner` | `apply conntrack -F` | refused `admin-safety`: admin-safety: admin account/access change refused (firewall) |
| 152 | account | ip link down | `heal-apply` | `galion-tuner` | `apply ip link set eth0 down` | refused `admin-safety`: admin-safety: admin account/access change refused (network change) |
| 153 | account | ip route del | `heal-apply` | `galion-tuner` | `apply ip route del default` | refused `admin-safety`: admin-safety: admin account/access change refused (network change) |
| 154 | account | route del | `heal-apply` | `galion-tuner` | `apply route del default` | refused `admin-safety`: admin-safety: admin account/access change refused (network change) |
| 155 | account | sysctl | `heal-apply` | `galion-tuner` | `apply sysctl -w net.ipv4.ip_forward=0` | refused `admin-safety`: admin-safety: admin account/access change refused (kernel setting) |
| 156 | account | modprobe -r | `heal-apply` | `galion-tuner` | `apply modprobe -r e1000` | refused `admin-safety`: admin-safety: admin account/access change refused (kernel module) |
| 157 | account | rmmod | `heal-apply` | `galion-tuner` | `apply rmmod e1000` | refused `admin-safety`: admin-safety: admin account/access change refused (kernel module) |
| 158 | account | chmod | `heal-apply` | `galion-tuner` | `apply chmod 000 /root` | refused `admin-safety`: admin-safety: admin account/access change refused (permission change) |
| 159 | account | chown | `heal-apply` | `galion-tuner` | `apply chown nobody /root` | refused `admin-safety`: admin-safety: admin account/access change refused (ownership change) |
| 160 | account | chgrp | `heal-apply` | `galion-tuner` | `apply chgrp nogroup /root` | refused `admin-safety`: admin-safety: admin account/access change refused (ownership change) |
| 161 | account | chattr | `heal-apply` | `galion-tuner` | `apply chattr +i /etc/passwd` | refused `admin-safety`: admin-safety: admin account/access change refused (/etc/passwd) |
| 162 | account | setfacl | `heal-apply` | `galion-tuner` | `apply setfacl -m u:x:rwx /root` | refused `admin-safety`: admin-safety: admin account/access change refused (acl change) |
| 163 | account | setcap | `heal-apply` | `galion-tuner` | `apply setcap cap_sys_admin+ep x` | refused `admin-safety`: admin-safety: admin account/access change refused (capability change) |
| 164 | account | reboot | `heal-apply` | `galion-tuner` | `apply reboot` | refused `admin-safety`: admin-safety: admin account/access change refused (reboot) |
| 165 | account | shutdown | `heal-apply` | `galion-tuner` | `apply shutdown -h now` | refused `admin-safety`: admin-safety: admin account/access change refused (shutdown) |
| 166 | account | poweroff | `heal-apply` | `galion-tuner` | `apply poweroff` | refused `admin-safety`: admin-safety: admin account/access change refused (poweroff) |
| 167 | account | halt | `heal-apply` | `galion-tuner` | `apply halt` | refused `admin-safety`: admin-safety: admin account/access change refused (halt) |
| 168 | account | telinit 0 | `heal-apply` | `galion-tuner` | `apply telinit 0` | refused `admin-safety`: admin-safety: admin account/access change refused (runlevel change) |
| 169 | account | killall | `heal-apply` | `galion-tuner` | `apply killall -u slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (killall) |
| 170 | account | kill every process | `heal-apply` | `galion-tuner` | `apply kill -9 -1` | refused `admin-safety`: admin-safety: admin account/access change refused (kill every process) |
| 171 | account | kill -- -1 | `heal-apply` | `galion-tuner` | `apply kill -KILL -- -1` | refused `admin-safety`: admin-safety: admin account/access change refused (kill every process) |
| 172 | account | pkill -u | `heal-apply` | `galion-tuner` | `apply pkill -u slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (kill a user's sessions) |
| 173 | account | loginctl terminate | `heal-apply` | `galion-tuner` | `apply loginctl terminate-user slavik` | refused `admin-safety`: admin-safety: admin account/access change refused (terminate sessions) |
| 174 | account | systemctl stop | `heal-apply` | `galion-tuner` | `apply systemctl stop ssh` | refused `admin-safety`: admin-safety: admin account/access change refused (service manager) |
| 175 | account | unit mask | `heal-apply` | `galion-tuner` | `apply mask ssh` | refused `admin-safety`: admin-safety: admin account/access change refused (unit mask) |
| 176 | account | timedatectl (token expiry) | `heal-apply` | `galion-tuner` | `apply timedatectl set-time 2030-01-01` | refused `admin-safety`: admin-safety: admin account/access change refused (clock change) |
| 177 | account | hwclock | `heal-apply` | `galion-tuner` | `apply hwclock --set --date 2030-01-01` | refused `admin-safety`: admin-safety: admin account/access change refused (clock change) |
| 178 | account | crontab add | `heal-apply` | `galion-tuner` | `apply crontab x` | refused `admin-safety`: admin-safety: admin account/access change refused (crontab) |
| 179 | opaque | empty plan | `heal-apply` | `galion-tuner` |  | refused `admin-safety`: admin-safety: plan cannot be reviewed (empty plan) |
| 180 | opaque | one empty step | `heal-apply` | `galion-tuner` | `` | refused `admin-safety`: admin-safety: plan cannot be reviewed (empty plan) |
| 181 | opaque | only blank steps | `heal-apply` | `galion-tuner` | `` / `  ` / `\t` | refused `admin-safety`: admin-safety: plan cannot be reviewed (empty plan) |
| 182 | opaque | command substitution | `heal-apply` | `galion-tuner` | `switch to $(id)` | refused `admin-safety`: admin-safety: plan cannot be reviewed (command substitution) |
| 183 | opaque | backticks | `heal-apply` | `galion-tuner` | `switch to `id`` | refused `admin-safety`: admin-safety: plan cannot be reviewed (command substitution) |
| 184 | opaque | process substitution | `heal-apply` | `galion-tuner` | `verify <(id)` | refused `admin-safety`: admin-safety: plan cannot be reviewed (process substitution) |
| 185 | opaque | pipe | `heal-apply` | `galion-tuner` | `verify /health \| grep ok` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 186 | opaque | and-and | `heal-apply` | `galion-tuner` | `restart x && id` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 187 | opaque | background ampersand | `heal-apply` | `galion-tuner` | `restart x & id` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 188 | opaque | redirect out | `heal-apply` | `galion-tuner` | `record result > /tmp/x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 189 | opaque | redirect in | `heal-apply` | `galion-tuner` | `record result < /tmp/x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 190 | opaque | variable | `heal-apply` | `galion-tuner` | `restart $UNIT` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 191 | opaque | ifs trick | `heal-apply` | `galion-tuner` | `restart${IFS}x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 192 | opaque | glob star | `heal-apply` | `galion-tuner` | `restart galion-*` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 193 | opaque | glob question | `heal-apply` | `galion-tuner` | `restart galion-tun?r` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 194 | opaque | tilde home | `heal-apply` | `galion-tuner` | `read ~/notes` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 195 | opaque | history bang | `heal-apply` | `galion-tuner` | `restart !!` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 196 | opaque | hash comment | `heal-apply` | `galion-tuner` | `restart x # then something` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 197 | opaque | double quotes | `heal-apply` | `galion-tuner` | `restart \"x\"` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 198 | opaque | single quotes | `heal-apply` | `galion-tuner` | `restart \'x\'` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 199 | opaque | backslash continuation | `heal-apply` | `galion-tuner` | `restart x \\` / `id` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 200 | opaque | brace expansion | `heal-apply` | `galion-tuner` | `restart {a,b}` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 201 | opaque | eval | `heal-apply` | `galion-tuner` | `apply eval x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (eval) |
| 202 | opaque | exec | `heal-apply` | `galion-tuner` | `apply exec x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (exec) |
| 203 | opaque | source | `heal-apply` | `galion-tuner` | `apply source x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (source) |
| 204 | opaque | alias | `heal-apply` | `galion-tuner` | `apply alias ls x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (alias) |
| 205 | opaque | xargs | `heal-apply` | `galion-tuner` | `apply xargs x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (xargs) |
| 206 | opaque | nohup | `heal-apply` | `galion-tuner` | `apply nohup x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (background job) |
| 207 | opaque | setsid | `heal-apply` | `galion-tuner` | `apply setsid x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (background job) |
| 208 | opaque | sh -c | `heal-apply` | `galion-tuner` | `apply sh -c x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell) |
| 209 | opaque | /bin/sh | `heal-apply` | `galion-tuner` | `apply /bin/sh x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell) |
| 210 | opaque | bash | `heal-apply` | `galion-tuner` | `apply bash x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell) |
| 211 | opaque | busybox | `heal-apply` | `galion-tuner` | `apply busybox x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell) |
| 212 | opaque | python | `heal-apply` | `galion-tuner` | `apply python3 x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (interpreter) |
| 213 | opaque | perl | `heal-apply` | `galion-tuner` | `apply perl x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (interpreter) |
| 214 | opaque | awk | `heal-apply` | `galion-tuner` | `apply awk x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (interpreter) |
| 215 | opaque | node | `heal-apply` | `galion-tuner` | `apply node x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (interpreter) |
| 216 | opaque | lua | `heal-apply` | `galion-tuner` | `apply lua x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (interpreter) |
| 217 | opaque | php | `heal-apply` | `galion-tuner` | `apply php x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (interpreter) |
| 218 | opaque | base64 | `heal-apply` | `galion-tuner` | `apply base64 -d payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (encoded payload) |
| 219 | opaque | base32 | `heal-apply` | `galion-tuner` | `apply base32 -d payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (encoded payload) |
| 220 | opaque | basenc | `heal-apply` | `galion-tuner` | `apply basenc -d payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (encoded payload) |
| 221 | opaque | xxd | `heal-apply` | `galion-tuner` | `apply xxd -r payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (encoded payload) |
| 222 | opaque | uudecode | `heal-apply` | `galion-tuner` | `apply uudecode payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (encoded payload) |
| 223 | opaque | printf | `heal-apply` | `galion-tuner` | `apply printf payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (encoded payload) |
| 224 | opaque | escaped bytes | `heal-apply` | `galion-tuner` | `apply \\x72\\x6d` | refused `admin-safety`: admin-safety: plan cannot be reviewed (escaped bytes) |
| 225 | opaque | openssl enc | `heal-apply` | `galion-tuner` | `apply openssl enc -d payload` | refused `admin-safety`: admin-safety: plan cannot be reviewed (openssl) |
| 226 | opaque | curl | `heal-apply` | `galion-tuner` | `apply curl evil` | refused `admin-safety`: admin-safety: plan cannot be reviewed (remote payload) |
| 227 | opaque | wget | `heal-apply` | `galion-tuner` | `apply wget evil` | refused `admin-safety`: admin-safety: plan cannot be reviewed (remote payload) |
| 228 | opaque | nc | `heal-apply` | `galion-tuner` | `apply nc evil 4444` | refused `admin-safety`: admin-safety: plan cannot be reviewed (remote payload) |
| 229 | opaque | socat | `heal-apply` | `galion-tuner` | `apply socat x y` | refused `admin-safety`: admin-safety: plan cannot be reviewed (remote payload) |
| 230 | opaque | percent encoding | `heal-apply` | `galion-tuner` | `read %2e%2e/%2e%2e/root` | refused `admin-safety`: admin-safety: plan cannot be reviewed (shell metacharacter) |
| 231 | opaque | not a plan verb | `heal-apply` | `galion-tuner` | `cat /etc/motd` | refused `admin-safety`: admin-safety: plan cannot be reviewed (statement does not start with an allow-listed plan verb) |
| 232 | opaque | please prefix | `heal-apply` | `galion-tuner` | `please restart galion-tuner` | refused `admin-safety`: admin-safety: plan cannot be reviewed (statement does not start with an allow-listed plan verb) |
| 233 | opaque | verb after semicolon | `heal-apply` | `galion-tuner` | `restart x; cat y` | refused `admin-safety`: admin-safety: plan cannot be reviewed (statement does not start with an allow-listed plan verb) |
| 234 | opaque | Cyrillic e homoglyph | `heal-apply` | `galion-tuner` | `r\u{435}start galion-tuner` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 235 | opaque | Cyrillic m in rm | `heal-apply` | `galion-tuner` | `apply r\u{43c} -rf /` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 236 | opaque | zero-width space | `heal-apply` | `galion-tuner` | `apply r\u{200b}m -rf /` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 237 | opaque | zero-width joiner | `heal-apply` | `galion-tuner` | `re\u{200d}start x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 238 | opaque | no-break space | `heal-apply` | `galion-tuner` | `apply rm\u{a0}-rf /` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 239 | opaque | fullwidth letters | `heal-apply` | `galion-tuner` | `apply \u{ff52}\u{ff4d} -rf /` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 240 | opaque | RTL override | `heal-apply` | `galion-tuner` | `restart \u{202e}fr- mr` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 241 | opaque | BOM | `heal-apply` | `galion-tuner` | `\u{feff}restart x` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 242 | opaque | combining mark | `heal-apply` | `galion-tuner` | `restart x\u{301}` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 243 | opaque | NUL byte | `heal-apply` | `galion-tuner` | `restart x\u{0}apply rm` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 244 | opaque | carriage return | `heal-apply` | `galion-tuner` | `restart x\rapply rm -rf /` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 245 | opaque | escape sequence | `heal-apply` | `galion-tuner` | `restart x\u{1b}[2K` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 246 | opaque | DEL byte | `heal-apply` | `galion-tuner` | `restart x\u{7f}` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 247 | opaque | vertical tab | `heal-apply` | `galion-tuner` | `restart x\u{b}apply id` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 248 | opaque | form feed | `heal-apply` | `galion-tuner` | `restart x\u{c}` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 249 | opaque | backspace | `heal-apply` | `galion-tuner` | `restart x\u{8}\u{8}` | refused `admin-safety`: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 250 | protected | hades-gate | `heal-apply` | `hades-gate` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (hades) |
| 251 | protected | HADES upper | `heal-apply` | `HADES` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (hades) |
| 252 | protected | mixed case | `heal-apply` | `HaDeS-Gate` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (hades) |
| 253 | protected | hades_gate underscore | `heal-apply` | `hades_gate` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (hades) |
| 254 | protected | sshd | `heal-apply` | `sshd` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (sshd) |
| 255 | protected | SSHD upper | `heal-apply` | `SSHD` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (sshd) |
| 256 | protected | ssh | `heal-apply` | `ssh` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (ssh) |
| 257 | protected | ssh.service | `heal-apply` | `ssh.service` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (ssh) |
| 258 | protected | ssh.socket | `heal-apply` | `ssh.socket` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (ssh) |
| 259 | protected | sshd@ instance | `heal-apply` | `sshd@1` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (sshd) |
| 260 | protected | root | `heal-apply` | `root` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (root) |
| 261 | protected | user@0 | `heal-apply` | `user@0` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (user) |
| 262 | protected | user-runtime-dir@0 | `heal-apply` | `user-runtime-dir@0` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (user) |
| 263 | protected | admin-panel | `heal-apply` | `admin-panel` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (admin) |
| 264 | protected | galion-control | `heal-apply` | `galion-control` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (galion-control) |
| 265 | protected | galion-control-v2 | `heal-apply` | `galion-control-v2` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (galion-control) |
| 266 | protected | secret-store | `heal-apply` | `secret-store` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (secret) |
| 267 | protected | persist | `heal-apply` | `persist` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (persist) |
| 268 | protected | sudo | `heal-apply` | `sudo` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (sudo) |
| 269 | protected | login | `heal-apply` | `login` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (login) |
| 270 | protected | systemd-logind | `heal-apply` | `systemd-logind` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (systemd-logind) |
| 271 | protected | pam | `heal-apply` | `pam` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (pam) |
| 272 | protected | firewall | `heal-apply` | `firewall` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (firewall) |
| 273 | protected | firewalld (live unit) | `heal-apply` | `firewalld` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (firewalld) |
| 274 | protected | iptables (live unit) | `heal-apply` | `iptables` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (iptables) |
| 275 | protected | ip6tables (live unit) | `heal-apply` | `ip6tables` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (ip6tables) |
| 276 | protected | ipset (live unit) | `heal-apply` | `ipset` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (ipset) |
| 277 | protected | nftables (live unit) | `heal-apply` | `nftables` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (nftables) |
| 278 | protected | ufw (live unit) | `heal-apply` | `ufw` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (ufw) |
| 279 | protected | fail2ban (live unit) | `heal-apply` | `fail2ban` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (fail2ban) |
| 280 | protected | auditd (live unit) | `heal-apply` | `auditd` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (auditd) |
| 281 | protected | apparmor (live unit) | `heal-apply` | `apparmor` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (apparmor) |
| 282 | protected | dbus (live unit) | `heal-apply` | `dbus` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (dbus) |
| 283 | protected | console-getty (live unit) | `heal-apply` | `console-getty` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (getty) |
| 284 | protected | container-getty@1 (live unit) | `heal-apply` | `container-getty@1` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (getty) |
| 285 | protected | getty | `heal-apply` | `getty` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (getty) |
| 286 | protected | key store | `heal-apply` | `key-store` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (key) |
| 287 | protected | token service | `heal-apply` | `token-svc` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (token) |
| 288 | protected | credentials | `heal-apply` | `credentials` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (credentials) |
| 289 | protected | account svc | `heal-apply` | `account-svc` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (account) |
| 290 | protected | empty target | `heal-apply` | `` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 291 | protected | space in target | `heal-apply` | `galion one` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 292 | protected | semicolon in target | `heal-apply` | `galion;rm` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 293 | protected | slash path target | `heal-apply` | `../hades` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 294 | protected | dot-dot target | `heal-apply` | `galion..tuner` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (target is not a plain service name) |
| 295 | protected | leading dash | `heal-apply` | `-rf` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (target is not a plain service name) |
| 296 | protected | leading dot | `heal-apply` | `.hidden` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (target is not a plain service name) |
| 297 | protected | newline in target | `heal-apply` | `galion-one\n` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 298 | protected | non-ASCII target | `heal-apply` | `gali\u{43e}n-one` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 299 | protected | 65-byte target | `heal-apply` | `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 300 | protected | star target | `heal-apply` | `galion-*` | `restart the service` / `verify /health` | refused `malformed`: malformed token: target name |
| 301 | protected | emergency (live unit) | `heal-apply` | `emergency` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (emergency) |
| 302 | protected | rescue (live unit) | `heal-apply` | `rescue` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (rescue) |
| 303 | protected | initrd-cleanup (live unit) | `heal-apply` | `initrd-cleanup` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (initrd) |
| 304 | protected | kmod-static-nodes (live unit) | `heal-apply` | `kmod-static-nodes` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (kmod) |
| 305 | protected | modprobe@loop (live unit) | `heal-apply` | `modprobe@loop` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (modprobe) |
| 306 | protected | rc-local (live unit) | `heal-apply` | `rc-local` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (rc-local) |
| 307 | protected | apt-daily-upgrade (live unit) | `heal-apply` | `apt-daily-upgrade` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (apt) |
| 308 | protected | ua-reboot-cmds (live unit) | `heal-apply` | `ua-reboot-cmds` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (reboot) |
| 309 | protected | systemd-soft-reboot style name | `heal-apply` | `soft-reboot` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (reboot) |
| 310 | never-unroute | vps | `heal-apply` | `vps` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 311 | never-unroute | galion-vps | `heal-apply` | `galion-vps` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 312 | never-unroute | VPS hostname | `heal-apply` | `ip50.vp3.titanaxe.com` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 313 | never-unroute | VPS short name | `heal-apply` | `ip50` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 314 | never-unroute | localhost | `heal-apply` | `localhost` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 315 | never-unroute | ip50.localdomain | `heal-apply` | `ip50.localdomain` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 316 | never-unroute | IPv4 loopback address | `heal-apply` | `127.0.0.1` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps (address target)) |
| 317 | never-unroute | IPv4 documentation address | `heal-apply` | `192.0.2.10` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps (address target)) |
| 318 | never-unroute | IPv6 loopback | `heal-apply` | `::1` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (target is not a plain service name) |
| 319 | never-unroute | IPv6 address | `heal-apply` | `2001:db8::1` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps (address target)) |
| 320 | never-unroute | systemd (pid 1) | `heal-apply` | `systemd` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 321 | never-unroute | init | `heal-apply` | `init` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 322 | never-unroute | kernel | `heal-apply` | `kernel` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 323 | never-unroute | systemd-networkd (live unit) | `heal-apply` | `systemd-networkd` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 324 | never-unroute | networkd-dispatcher (live unit) | `heal-apply` | `networkd-dispatcher` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 325 | never-unroute | NetworkManager (live unit) | `heal-apply` | `NetworkManager` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 326 | never-unroute | connman (live unit) | `heal-apply` | `connman` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 327 | never-unroute | netplan | `heal-apply` | `netplan` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 328 | never-unroute | networking | `heal-apply` | `networking` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 329 | never-unroute | ovsdb-server (live unit) | `heal-apply` | `ovsdb-server` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 330 | never-unroute | openvswitch-switch | `heal-apply` | `openvswitch-switch` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 331 | never-unroute | dns | `heal-apply` | `dns` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 332 | never-unroute | systemd-resolved (live unit) | `heal-apply` | `systemd-resolved` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 333 | never-unroute | resolvconf | `heal-apply` | `resolvconf` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 334 | never-unroute | named | `heal-apply` | `named` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 335 | never-unroute | bind9 | `heal-apply` | `bind9` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 336 | never-unroute | unbound | `heal-apply` | `unbound` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 337 | never-unroute | dnsmasq | `heal-apply` | `dnsmasq` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 338 | never-unroute | coredns | `heal-apply` | `coredns` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 339 | never-unroute | Cloudflare NS 1 | `heal-apply` | `elaine.ns.cloudflare.com` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 340 | never-unroute | Cloudflare NS 2 | `heal-apply` | `peyton.ns.cloudflare.com` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 341 | never-unroute | cloudflare-dns | `heal-apply` | `cloudflare-dns` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 342 | never-unroute | mail | `heal-apply` | `mail` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 343 | never-unroute | galion-mail | `heal-apply` | `galion-mail` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 344 | never-unroute | mailserver | `heal-apply` | `mailserver` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 345 | never-unroute | email | `heal-apply` | `email` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 346 | never-unroute | postfix (live unit) | `heal-apply` | `postfix` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 347 | never-unroute | postfix@- (live unit) | `heal-apply` | `postfix@-` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 348 | never-unroute | postfix-resolvconf (live unit) | `heal-apply` | `postfix-resolvconf` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 349 | never-unroute | exim4 (live unit) | `heal-apply` | `exim4` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 350 | never-unroute | sendmail (live unit) | `heal-apply` | `sendmail` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 351 | never-unroute | smtp | `heal-apply` | `smtp` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 352 | never-unroute | dovecot | `heal-apply` | `dovecot` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 353 | never-unroute | opendkim | `heal-apply` | `opendkim` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 354 | never-unroute | Zoho MX | `heal-apply` | `mx.zoho.eu` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 355 | never-unroute | Zoho MX2 | `heal-apply` | `mx2.zoho.eu` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 356 | never-unroute | Cloudflare MX route1 | `heal-apply` | `route1.mx.cloudflare.net` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 357 | never-unroute | backup | `heal-apply` | `backup` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 358 | never-unroute | galion-backup (live unit) | `heal-apply` | `galion-backup` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 359 | never-unroute | galion-backup.timer (live timer) | `heal-apply` | `galion-backup.timer` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 360 | never-unroute | dpkg-db-backup (live unit) | `heal-apply` | `dpkg-db-backup` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 361 | never-unroute | backups | `heal-apply` | `backups` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 362 | never-unroute | offsite | `heal-apply` | `galion-offsite` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 363 | never-unroute | restic | `heal-apply` | `restic` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 364 | never-unroute | borg | `heal-apply` | `borg` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 365 | never-unroute | rsnapshot | `heal-apply` | `rsnapshot` | `restart the service` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 366 | never-unroute | restart postfix | `heal-apply` | `galion-one` | `restart postfix` / `verify /health` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 367 | never-unroute | restart exim4 | `heal-apply` | `galion-one` | `restart exim4` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 368 | never-unroute | restart sendmail | `heal-apply` | `galion-one` | `restart sendmail` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 369 | never-unroute | switch mail | `heal-apply` | `galion-one` | `switch mail to the new relay` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 370 | never-unroute | mx record change | `heal-apply` | `galion-one` | `apply mx record change for galion.studio` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 371 | never-unroute | unroute mail hostname | `heal-apply` | `galion-one` | `apply ingress removal for mail.galion.studio` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: mail) |
| 372 | never-unroute | restart systemd-resolved | `heal-apply` | `galion-one` | `restart systemd-resolved` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 373 | never-unroute | switch dns | `heal-apply` | `galion-one` | `switch dns to the new resolver` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 374 | never-unroute | nameserver | `heal-apply` | `galion-one` | `switch the nameserver` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 375 | never-unroute | cloudflare dns record | `heal-apply` | `galion-one` | `apply cloudflare record change for galion.app` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 376 | never-unroute | resolv.conf | `heal-apply` | `galion-one` | `switch /etc/resolv.conf` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: dns) |
| 377 | never-unroute | resolvectl | `heal-apply` | `galion-one` | `apply resolvectl dns eth0 1` | refused `admin-safety`: admin-safety: protected target refused (never-unroute) |
| 378 | never-unroute | etc hosts | `heal-apply` | `galion-one` | `apply /etc/hosts change` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 379 | never-unroute | hostnamectl | `heal-apply` | `galion-one` | `apply hostnamectl set-hostname x` | refused `admin-safety`: admin-safety: protected target refused (never-unroute) |
| 380 | never-unroute | etc hostname | `heal-apply` | `galion-one` | `switch /etc/hostname` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 381 | never-unroute | restart systemd-networkd | `heal-apply` | `galion-one` | `restart systemd-networkd` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 382 | never-unroute | netplan apply | `heal-apply` | `galion-one` | `apply netplan` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 383 | never-unroute | switch the vps | `heal-apply` | `galion-one` | `switch the vps off` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 384 | never-unroute | localhost | `heal-apply` | `galion-one` | `restart localhost` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: vps) |
| 385 | never-unroute | stop the backup timer | `heal-apply` | `galion-one` | `switch galion-backup.timer off` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 386 | never-unroute | keep no backups | `heal-apply` | `galion-one` | `keep backups for 0 days` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 387 | never-unroute | backup dir | `heal-apply` | `galion-one` | `switch /var/backups/galion to tmpfs` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 388 | never-unroute | restic forget | `heal-apply` | `galion-one` | `apply restic forget --prune` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 389 | never-unroute | offsite | `heal-apply` | `galion-one` | `switch offsite copy off` | refused `admin-safety`: admin-safety: protected target refused (never-unroute: backup) |
| 390 | action | kill-switch | `kill-switch` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "kill-switch" can never be approved |
| 391 | action | teardown | `teardown` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "teardown" can never be approved |
| 392 | action | secret-rotation-plan | `secret-rotation-plan` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "secret-rotation-plan" can never be approved |
| 393 | action | kill | `kill` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "kill" can never be approved |
| 394 | action | rotate | `rotate` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "rotate" can never be approved |
| 395 | action | empty system | `` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "" can never be approved |
| 396 | action | upper case | `HEAL-APPLY` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "HEAL-APPLY" can never be approved |
| 397 | action | underscore | `heal_apply` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "heal_apply" can never be approved |
| 398 | action | trailing space | `heal-apply ` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "heal-apply " can never be approved |
| 399 | action | trailing newline | `heal-apply\n` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "heal-apply\n" can never be approved |
| 400 | action | unknown deploy | `deploy` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "deploy" can never be approved |
| 401 | action | unknown exec | `exec` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "exec" can never be approved |
| 402 | action | non-ASCII | `h\u{435}al-apply` | `galion-tuner` | `restart the service` / `verify /health` | refused `action`: action "h\u{435}al-apply" can never be approved |

- destructive: 66/66 refused
- account: 74/74 refused
- secret: 38/38 refused
- opaque: 71/71 refused
- protected: 60/60 refused
- never-unroute: 80/80 refused
- action: 13/13 refused
