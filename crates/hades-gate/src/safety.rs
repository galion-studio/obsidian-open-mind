// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The hard admin-safety rule.
//!
//! HADES must never lock out, harm or expose the admin. [`review`] runs on the
//! action, the target and the exact plan text **before** any token is looked
//! at, and a refusal here cannot be overridden by any token, valid or not.
//!
//! It refuses:
//! * destructive commands (`rm -rf`, `mkfs`, `dd`, `shred`, `DROP TABLE`, force pushes, ...);
//! * anything that could expose a secret (secret paths, private keys, `printenv`, ...);
//! * admin account, login and access changes (`passwd`, `usermod`, `sudoers`,
//!   `authorized_keys`, `sshd`, firewall rules, permission changes, reboots);
//! * targets that are the admin's access path or HADES itself, so an approval
//!   can never be used to switch off the admin's ability to stop HADES;
//! * plans the reviewer cannot read: non-ASCII text, shell substitution, `eval`,
//!   encoded payloads.
//!
//! Since S3.1 (docs/security/hades-redteam.md) the deny list is backed by
//! allow lists: plan punctuation is limited to `/ . - _ : , ( ) ;` (no shell
//! metacharacters), every statement must start with a [`PLAN_VERBS`] verb, and
//! the target must be a plain service name. On top of that, only `heal-apply`,
//! `switch` and `rollback` can be approved at all. It is deliberately over-cautious:
//! a false refusal costs the admin one manual step, a false approval could
//! cost them their box.

/// Longest plan the gate will review.
pub const MAX_PLAN_BYTES: usize = 64 * 1024;

/// Why the safety rule refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hazard {
    /// Deletes, wipes or overwrites data or history.
    Destructive(&'static str),
    /// Could print, copy or move a secret.
    SecretExposure(&'static str),
    /// Changes the admin's account, login, keys, sudo, firewall or permissions.
    AdminAccount(&'static str),
    /// Targets the admin access path or HADES itself.
    ProtectedTarget(&'static str),
    /// The plan cannot be reviewed as plain text.
    Opaque(&'static str),
}

impl Hazard {
    /// Human-readable refusal reason.
    #[must_use]
    pub fn reason(self) -> String {
        match self {
            Self::Destructive(m) => format!("admin-safety: destructive command refused ({m})"),
            Self::SecretExposure(m) => format!("admin-safety: secret exposure refused ({m})"),
            Self::AdminAccount(m) => {
                format!("admin-safety: admin account/access change refused ({m})")
            }
            Self::ProtectedTarget(m) => format!("admin-safety: protected target refused ({m})"),
            Self::Opaque(m) => format!("admin-safety: plan cannot be reviewed ({m})"),
        }
    }
}

/// Multi-word or punctuation patterns, matched on the lower-cased plan with
/// runs of whitespace folded to one space.
const PHRASES: &[(&str, Hazard)] = &[
    ("rm -r", Hazard::Destructive("rm -r")),
    ("rm -f", Hazard::Destructive("rm -f")),
    ("rm --recursive", Hazard::Destructive("rm --recursive")),
    (
        "--no-preserve-root",
        Hazard::Destructive("--no-preserve-root"),
    ),
    ("dd if=", Hazard::Destructive("dd")),
    ("dd of=", Hazard::Destructive("dd")),
    ("> /dev/sd", Hazard::Destructive("write to a disk device")),
    (">/dev/sd", Hazard::Destructive("write to a disk device")),
    ("> /dev/nvme", Hazard::Destructive("write to a disk device")),
    (">/dev/nvme", Hazard::Destructive("write to a disk device")),
    (
        "> /dev/mmcblk",
        Hazard::Destructive("write to a disk device"),
    ),
    (
        ">/dev/mmcblk",
        Hazard::Destructive("write to a disk device"),
    ),
    ("> /dev/vd", Hazard::Destructive("write to a disk device")),
    (">/dev/vd", Hazard::Destructive("write to a disk device")),
    ("drop database", Hazard::Destructive("drop database")),
    ("drop table", Hazard::Destructive("drop table")),
    ("delete from", Hazard::Destructive("delete from")),
    ("truncate table", Hazard::Destructive("truncate table")),
    ("push --force", Hazard::Destructive("force push")),
    ("push -f", Hazard::Destructive("force push")),
    ("push --mirror", Hazard::Destructive("mirror push")),
    ("reset --hard", Hazard::Destructive("git reset --hard")),
    ("clean -fd", Hazard::Destructive("git clean")),
    ("branch -d", Hazard::Destructive("branch delete")),
    ("zpool destroy", Hazard::Destructive("zpool destroy")),
    ("kill -9 -1", Hazard::AdminAccount("kill every process")),
    ("pkill -u", Hazard::AdminAccount("kill a user's sessions")),
    (
        "loginctl terminate",
        Hazard::AdminAccount("terminate sessions"),
    ),
    ("/etc/shadow", Hazard::AdminAccount("/etc/shadow")),
    ("/etc/passwd", Hazard::AdminAccount("/etc/passwd")),
    ("/etc/group", Hazard::AdminAccount("/etc/group")),
    ("/etc/pam", Hazard::AdminAccount("pam")),
    ("/etc/ssh", Hazard::AdminAccount("ssh config")),
    ("authorized_keys", Hazard::AdminAccount("authorized_keys")),
    ("sshd_config", Hazard::AdminAccount("sshd_config")),
    ("sudoers", Hazard::AdminAccount("sudoers")),
    (
        "galion-control",
        Hazard::SecretExposure("galion-control key"),
    ),
    ("galion-secrets", Hazard::SecretExposure("secrets dir")),
    ("/secrets", Hazard::SecretExposure("secrets dir")),
    ("secrets/", Hazard::SecretExposure("secrets dir")),
    ("/persist", Hazard::SecretExposure("persist dir")),
    ("/environ", Hazard::SecretExposure("process environment")),
    ("private key", Hazard::SecretExposure("private key")),
    ("begin openssh", Hazard::SecretExposure("private key")),
    ("begin rsa", Hazard::SecretExposure("private key")),
    ("begin ec", Hazard::SecretExposure("private key")),
    ("id_rsa", Hazard::SecretExposure("ssh key")),
    ("id_ed25519", Hazard::SecretExposure("ssh key")),
    ("id_ecdsa", Hazard::SecretExposure("ssh key")),
    (".ssh/", Hazard::SecretExposure("ssh dir")),
    (".env", Hazard::SecretExposure(".env file")),
    (".netrc", Hazard::SecretExposure(".netrc")),
    (
        ".git-credentials",
        Hazard::SecretExposure("git credentials"),
    ),
    ("api_key", Hazard::SecretExposure("api key")),
    ("api-key", Hazard::SecretExposure("api key")),
    ("$(", Hazard::Opaque("command substitution")),
    ("`", Hazard::Opaque("command substitution")),
    ("<(", Hazard::Opaque("process substitution")),
    ("\\x", Hazard::Opaque("escaped bytes")),
    (":(){", Hazard::Destructive("fork bomb")),
    // S5.2 corpus additions (slot 208).
    (
        "--remove-source-files",
        Hazard::Destructive("move and delete"),
    ),
    ("--vacuum", Hazard::Destructive("journal vacuum")),
    ("sysrq", Hazard::Destructive("sysrq trigger")),
    ("/private", Hazard::SecretExposure("private dir")),
    (".pgpass", Hazard::SecretExposure(".pgpass")),
];

/// Whole words (split on anything that is not `a-z 0-9 _ -`).
const WORDS: &[(&str, Hazard)] = &[
    ("mkfs", Hazard::Destructive("mkfs")),
    ("wipefs", Hazard::Destructive("wipefs")),
    ("shred", Hazard::Destructive("shred")),
    ("fdisk", Hazard::Destructive("fdisk")),
    ("sfdisk", Hazard::Destructive("sfdisk")),
    ("parted", Hazard::Destructive("parted")),
    ("blkdiscard", Hazard::Destructive("blkdiscard")),
    ("lvremove", Hazard::Destructive("lvremove")),
    ("vgremove", Hazard::Destructive("vgremove")),
    ("rmdir", Hazard::Destructive("rmdir")),
    ("unlink", Hazard::Destructive("unlink")),
    ("truncate", Hazard::Destructive("truncate")),
    ("dropdb", Hazard::Destructive("dropdb")),
    ("filter-branch", Hazard::Destructive("history rewrite")),
    ("filter-repo", Hazard::Destructive("history rewrite")),
    ("useradd", Hazard::AdminAccount("useradd")),
    ("userdel", Hazard::AdminAccount("userdel")),
    ("usermod", Hazard::AdminAccount("usermod")),
    ("adduser", Hazard::AdminAccount("adduser")),
    ("deluser", Hazard::AdminAccount("deluser")),
    ("groupmod", Hazard::AdminAccount("groupmod")),
    ("gpasswd", Hazard::AdminAccount("gpasswd")),
    ("passwd", Hazard::AdminAccount("passwd")),
    ("chpasswd", Hazard::AdminAccount("chpasswd")),
    ("chage", Hazard::AdminAccount("chage")),
    ("chsh", Hazard::AdminAccount("chsh")),
    ("visudo", Hazard::AdminAccount("visudo")),
    ("sshd", Hazard::AdminAccount("sshd")),
    ("ssh", Hazard::AdminAccount("ssh")),
    ("iptables", Hazard::AdminAccount("firewall")),
    ("ip6tables", Hazard::AdminAccount("firewall")),
    ("nft", Hazard::AdminAccount("firewall")),
    ("ufw", Hazard::AdminAccount("firewall")),
    ("firewall-cmd", Hazard::AdminAccount("firewall")),
    ("chmod", Hazard::AdminAccount("permission change")),
    ("chown", Hazard::AdminAccount("ownership change")),
    ("chgrp", Hazard::AdminAccount("ownership change")),
    ("chattr", Hazard::AdminAccount("attribute change")),
    ("setfacl", Hazard::AdminAccount("acl change")),
    ("shutdown", Hazard::AdminAccount("shutdown")),
    ("reboot", Hazard::AdminAccount("reboot")),
    ("poweroff", Hazard::AdminAccount("poweroff")),
    ("halt", Hazard::AdminAccount("halt")),
    ("killall", Hazard::AdminAccount("killall")),
    ("crontab", Hazard::AdminAccount("crontab")),
    ("printenv", Hazard::SecretExposure("printenv")),
    ("env", Hazard::SecretExposure("environment dump")),
    ("export", Hazard::SecretExposure("environment")),
    ("secret", Hazard::SecretExposure("secret")),
    ("secrets", Hazard::SecretExposure("secret")),
    ("password", Hazard::SecretExposure("password")),
    ("passphrase", Hazard::SecretExposure("passphrase")),
    ("token", Hazard::SecretExposure("token")),
    ("credentials", Hazard::SecretExposure("credentials")),
    ("eval", Hazard::Opaque("eval")),
    ("base64", Hazard::Opaque("encoded payload")),
    ("xxd", Hazard::Opaque("encoded payload")),
    ("openssl", Hazard::Opaque("openssl")),
    // S3.1 red-team additions (docs/security/hades-redteam.md).
    ("rm", Hazard::Destructive("rm")),
    ("find", Hazard::Destructive("find")),
    ("delete", Hazard::Destructive("delete")),
    ("dd", Hazard::Destructive("dd")),
    ("mv", Hazard::Destructive("mv")),
    ("cp", Hazard::Destructive("cp")),
    ("tee", Hazard::Destructive("tee")),
    ("install", Hazard::Destructive("install")),
    ("sed", Hazard::Destructive("sed")),
    ("systemctl", Hazard::AdminAccount("service manager")),
    ("mask", Hazard::AdminAccount("unit mask")),
    ("setcap", Hazard::AdminAccount("capability change")),
    ("ip", Hazard::AdminAccount("network change")),
    ("route", Hazard::AdminAccount("network change")),
    ("keygen", Hazard::AdminAccount("key generation")),
    ("curl", Hazard::Opaque("remote payload")),
    ("wget", Hazard::Opaque("remote payload")),
    ("nc", Hazard::Opaque("remote payload")),
    ("ncat", Hazard::Opaque("remote payload")),
    ("socat", Hazard::Opaque("remote payload")),
    ("bash", Hazard::Opaque("shell")),
    ("zsh", Hazard::Opaque("shell")),
    ("dash", Hazard::Opaque("shell")),
    ("ksh", Hazard::Opaque("shell")),
    ("csh", Hazard::Opaque("shell")),
    ("busybox", Hazard::Opaque("shell")),
    ("exec", Hazard::Opaque("exec")),
    ("source", Hazard::Opaque("source")),
    ("alias", Hazard::Opaque("alias")),
    ("xargs", Hazard::Opaque("xargs")),
    ("nohup", Hazard::Opaque("background job")),
    ("setsid", Hazard::Opaque("background job")),
    ("printf", Hazard::Opaque("encoded payload")),
    ("base32", Hazard::Opaque("encoded payload")),
    ("basenc", Hazard::Opaque("encoded payload")),
    ("perl", Hazard::Opaque("interpreter")),
    ("ruby", Hazard::Opaque("interpreter")),
    ("python", Hazard::Opaque("interpreter")),
    ("python3", Hazard::Opaque("interpreter")),
    ("node", Hazard::Opaque("interpreter")),
    ("awk", Hazard::Opaque("interpreter")),
    ("gawk", Hazard::Opaque("interpreter")),
];

/// Secret words that are refused even inside a `-` or `_` compound word (S5.2).
const SECRET_PARTS: &[&str] = &[
    "secret",
    "secrets",
    "password",
    "passphrase",
    "token",
    "credentials",
    "key",
    "keys",
    "apikey",
    "privkey",
    "seed",
];

/// Whole words added by the S5.2 corpus (slot 208,
/// docs/security/hades-safety-corpus.md). Checked after [`WORDS`], so a plan
/// that already hit an older rule keeps its old reason (`sudo mkfs` stays
/// destructive).
const LATE_WORDS: &[(&str, Hazard)] = &[
    ("killall5", Hazard::Destructive("kill every process")),
    ("kill", Hazard::Destructive("kill")),
    ("remove", Hazard::Destructive("remove")),
    ("purge", Hazard::Destructive("purge")),
    ("prune", Hazard::Destructive("prune")),
    ("apt", Hazard::Destructive("package change")),
    ("apt-get", Hazard::Destructive("package change")),
    ("dpkg", Hazard::Destructive("package change")),
    ("umount", Hazard::Destructive("unmount")),
    ("mount", Hazard::Destructive("mount")),
    ("remount", Hazard::Destructive("mount")),
    ("swapoff", Hazard::Destructive("swapoff")),
    ("systemd-run", Hazard::Destructive("transient unit")),
    ("ln", Hazard::Destructive("link over a file")),
    ("srm", Hazard::Destructive("srm")),
    ("chfn", Hazard::AdminAccount("chfn")),
    ("vipw", Hazard::AdminAccount("vipw")),
    ("vigr", Hazard::AdminAccount("vigr")),
    ("newusers", Hazard::AdminAccount("newusers")),
    ("sudo", Hazard::AdminAccount("privilege change")),
    ("su", Hazard::AdminAccount("privilege change")),
    ("doas", Hazard::AdminAccount("privilege change")),
    ("pkexec", Hazard::AdminAccount("privilege change")),
    ("runuser", Hazard::AdminAccount("privilege change")),
    ("ssh-keygen", Hazard::AdminAccount("key generation")),
    ("nftables", Hazard::AdminAccount("firewall")),
    ("firewalld", Hazard::AdminAccount("firewall")),
    ("ipset", Hazard::AdminAccount("firewall")),
    ("ebtables", Hazard::AdminAccount("firewall")),
    ("arptables", Hazard::AdminAccount("firewall")),
    ("conntrack", Hazard::AdminAccount("firewall")),
    ("fail2ban", Hazard::AdminAccount("intrusion ban")),
    ("fail2ban-client", Hazard::AdminAccount("intrusion ban")),
    ("sysctl", Hazard::AdminAccount("kernel setting")),
    ("modprobe", Hazard::AdminAccount("kernel module")),
    ("rmmod", Hazard::AdminAccount("kernel module")),
    ("insmod", Hazard::AdminAccount("kernel module")),
    ("telinit", Hazard::AdminAccount("runlevel change")),
    ("timedatectl", Hazard::AdminAccount("clock change")),
    ("hwclock", Hazard::AdminAccount("clock change")),
    ("chronyc", Hazard::AdminAccount("clock change")),
    ("ntpdate", Hazard::AdminAccount("clock change")),
    ("key", Hazard::SecretExposure("key")),
    ("keys", Hazard::SecretExposure("key")),
    ("apikey", Hazard::SecretExposure("api key")),
    ("privkey", Hazard::SecretExposure("private key")),
    ("seed", Hazard::SecretExposure("key seed")),
    ("pem", Hazard::SecretExposure("pem file")),
    ("bearer", Hazard::SecretExposure("auth header")),
    ("authorization", Hazard::SecretExposure("auth header")),
    ("cookie", Hazard::SecretExposure("cookie")),
    ("cookies", Hazard::SecretExposure("cookie")),
    ("lua", Hazard::Opaque("interpreter")),
    ("php", Hazard::Opaque("interpreter")),
    ("tclsh", Hazard::Opaque("interpreter")),
    ("uudecode", Hazard::Opaque("encoded payload")),
    ("uuencode", Hazard::Opaque("encoded payload")),
];

/// The allow-list (S3.1): every statement of a plan (lines, and `;`-separated
/// parts of a line) must start with one of these verbs. They are exactly the
/// leading words of the plans HADES can approve: gaia-hades `plan_for` for
/// `heal-apply` and `rollback`, and the `switch`/`restart`/`verify` plans in
/// the tests and docs. Add a verb only together with a test and a red-team
/// row; a deny-list miss then still needs an allowed verb to get through.
pub const PLAN_VERBS: &[&str] = &[
    "apply", "append", "keep", "operator", "read", "record", "restart", "switch", "take", "verify",
];

/// Bytes a plan may contain besides ASCII letters, digits, space, tab and
/// newline. No shell metacharacters: no `$ \` ' " \ | & > < = { } * ? ! ~ #`.
const PLAN_PUNCT: &[u8] = b"/.-_:,();";

/// Longest target name.
pub const MAX_TARGET_BYTES: usize = 64;

/// Target names (or parts of them) that are never approvable.
const PROTECTED_TARGETS: &[&str] = &[
    "hades",
    "admin",
    "root",
    "ssh",
    "sshd",
    "sudo",
    "login",
    "pam",
    "user",
    "account",
    "firewall",
    "iptables",
    "nftables",
    "ufw",
    "secret",
    "secrets",
    "persist",
    "key",
    "keys",
    "token",
    "credential",
    "credentials",
    "galion-control",
    "systemd-logind",
    // S5.2 (slot 208): security and console-login units seen on the VPS.
    "firewalld",
    "ip6tables",
    "ipset",
    "fail2ban",
    "auditd",
    "apparmor",
    "dbus",
    "getty",
    "polkit",
    "polkitd",
];

/// Owner never-unroute list (D-T42-4, Mate 2026-10-21 17:04 +02:00): the VPS
/// itself, DNS, the mail server and the backup server are never approvable
/// targets. Matched against every alphanumeric part of the target name
/// (`galion-backup.timer` -> `galion`, `backup`, `timer`). The names follow the
/// nerve list (deploy/nerve/plans-protected-hosts.conf on
/// swarm/cortex-protected-hosts) plus the VPS units that carry those roles
/// (read-only `systemctl list-units` on the VPS, 2026-10-21 20:20 +02:00).
pub const NEVER_UNROUTE_TARGETS: &[(&str, &str)] = &[
    ("vps", "never-unroute: vps"),
    ("ip50", "never-unroute: vps"),
    ("vp3", "never-unroute: vps"),
    ("titanaxe", "never-unroute: vps"),
    ("localhost", "never-unroute: vps"),
    ("localdomain", "never-unroute: vps"),
    ("systemd", "never-unroute: vps"),
    ("init", "never-unroute: vps"),
    ("kernel", "never-unroute: vps"),
    ("networkd", "never-unroute: vps"),
    ("networkmanager", "never-unroute: vps"),
    ("connman", "never-unroute: vps"),
    ("netplan", "never-unroute: vps"),
    ("networking", "never-unroute: vps"),
    ("hostname", "never-unroute: vps"),
    ("dns", "never-unroute: dns"),
    ("ns", "never-unroute: dns"),
    ("nameserver", "never-unroute: dns"),
    ("resolved", "never-unroute: dns"),
    ("resolv", "never-unroute: dns"),
    ("resolvconf", "never-unroute: dns"),
    ("named", "never-unroute: dns"),
    ("bind", "never-unroute: dns"),
    ("bind9", "never-unroute: dns"),
    ("unbound", "never-unroute: dns"),
    ("dnsmasq", "never-unroute: dns"),
    ("coredns", "never-unroute: dns"),
    ("cloudflare", "never-unroute: dns"),
    ("mail", "never-unroute: mail"),
    ("mailserver", "never-unroute: mail"),
    ("email", "never-unroute: mail"),
    ("smtp", "never-unroute: mail"),
    ("smtpd", "never-unroute: mail"),
    ("postfix", "never-unroute: mail"),
    ("exim", "never-unroute: mail"),
    ("exim4", "never-unroute: mail"),
    ("sendmail", "never-unroute: mail"),
    ("dovecot", "never-unroute: mail"),
    ("imap", "never-unroute: mail"),
    ("opendkim", "never-unroute: mail"),
    ("dkim", "never-unroute: mail"),
    ("mx", "never-unroute: mail"),
    ("mx1", "never-unroute: mail"),
    ("mx2", "never-unroute: mail"),
    ("mx3", "never-unroute: mail"),
    ("zoho", "never-unroute: mail"),
    ("backup", "never-unroute: backup"),
    ("backups", "never-unroute: backup"),
    ("offsite", "never-unroute: backup"),
    ("restic", "never-unroute: backup"),
    ("borg", "never-unroute: backup"),
    ("borgbackup", "never-unroute: backup"),
    ("rsnapshot", "never-unroute: backup"),
    ("duplicity", "never-unroute: backup"),
];

/// The never-unroute names a plan step may not mention (any alphanumeric part
/// of the plan text), so a plan approved for a benign target cannot change the
/// VPS identity, DNS, mail or backup either. Ambiguous English words
/// (`init`, `named`, `bind`, `unbound`, `ns`, `kernel`) are target-only.
pub const NEVER_UNROUTE_PLAN_WORDS: &[&str] = &[
    "vps",
    "ip50",
    "titanaxe",
    "localhost",
    "localdomain",
    "networkd",
    "networkmanager",
    "connman",
    "netplan",
    "hostname",
    "hostnamectl",
    "dns",
    "nameserver",
    "nameservers",
    "resolved",
    "resolv",
    "resolvconf",
    "resolvectl",
    "bind9",
    "dnsmasq",
    "coredns",
    "cloudflare",
    "mail",
    "mailserver",
    "email",
    "smtp",
    "smtpd",
    "postfix",
    "exim",
    "exim4",
    "sendmail",
    "dovecot",
    "imap",
    "opendkim",
    "dkim",
    "mx",
    "mx1",
    "mx2",
    "mx3",
    "zoho",
    "backup",
    "backups",
    "offsite",
    "restic",
    "borg",
    "borgbackup",
    "rsnapshot",
    "duplicity",
];

/// Plan phrases that touch the VPS identity or name resolution.
const NEVER_UNROUTE_PLAN_PHRASES: &[(&str, &str)] = &[
    ("/etc/hosts", "never-unroute: vps"),
    ("/etc/resolv", "never-unroute: dns"),
    ("/var/backups", "never-unroute: backup"),
];

fn never_unroute_target_hit(lower: &str) -> Option<&'static str> {
    for part in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some((_, why)) = NEVER_UNROUTE_TARGETS.iter().find(|(p, _)| *p == part) {
            return Some(why);
        }
    }
    // A target that is an address is the VPS (or a resolver), never a service.
    if lower.parse::<std::net::IpAddr>().is_ok()
        || lower.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Some("never-unroute: vps (address target)");
    }
    None
}

fn never_unroute_plan_hit(folded: &str) -> Option<&'static str> {
    for (pat, why) in NEVER_UNROUTE_PLAN_PHRASES {
        if folded.contains(pat) {
            return Some(why);
        }
    }
    for part in folded.split(|c: char| !c.is_ascii_alphanumeric()) {
        if NEVER_UNROUTE_PLAN_WORDS.contains(&part) {
            let why = NEVER_UNROUTE_TARGETS
                .iter()
                .find(|(p, _)| *p == part)
                .map_or("never-unroute", |(_, w)| *w);
            return Some(why);
        }
    }
    None
}

/// Check the target and plan against the admin-safety rule.
///
/// # Errors
/// The first [`Hazard`] found.
pub fn review(target: &str, plan: &[u8]) -> Result<(), Hazard> {
    review_target(target)?;
    review_plan(plan)
}

/// Only the target name.
///
/// A target is a plain service name: 1 to [`MAX_TARGET_BYTES`] ASCII bytes
/// from `A-Z a-z 0-9 . _ - @ :` (as [`crate::token::valid_target`]), starting
/// with a letter or digit, no `..`; and no
/// part of it may be a protected name.
///
/// # Errors
/// [`Hazard::ProtectedTarget`].
pub fn review_target(target: &str) -> Result<(), Hazard> {
    let b = target.as_bytes();
    if b.is_empty()
        || b.len() > MAX_TARGET_BYTES
        || !b[0].is_ascii_alphanumeric()
        || !b
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'@' | b':'))
        || target.contains("..")
    {
        return Err(Hazard::ProtectedTarget(
            "target is not a plain service name",
        ));
    }
    let lower = target.to_ascii_lowercase();
    for part in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(hit) = PROTECTED_TARGETS.iter().find(|p| **p == part) {
            return Err(Hazard::ProtectedTarget(hit));
        }
    }
    for multi in PROTECTED_TARGETS.iter().filter(|p| p.contains('-')) {
        if lower.contains(multi) {
            return Err(Hazard::ProtectedTarget(multi));
        }
    }
    if let Some(why) = never_unroute_target_hit(&lower) {
        return Err(Hazard::ProtectedTarget(why));
    }
    Ok(())
}

/// Only the plan text.
///
/// Order: size and ASCII checks, the deny-list (so a known hazard is named),
/// then the allow-list: only [`PLAN_PUNCT`] punctuation, and every statement
/// starts with a [`PLAN_VERBS`] verb.
///
/// # Errors
/// The first [`Hazard`] found.
pub fn review_plan(plan: &[u8]) -> Result<(), Hazard> {
    if plan.is_empty() {
        return Err(Hazard::Opaque("empty plan"));
    }
    if plan.len() > MAX_PLAN_BYTES {
        return Err(Hazard::Opaque("plan too long"));
    }
    if !plan
        .iter()
        .all(|b| matches!(b, b'\n' | b'\t') || (0x20..0x7f).contains(b))
    {
        return Err(Hazard::Opaque("non-printable or non-ASCII bytes"));
    }
    let text = String::from_utf8_lossy(plan).to_ascii_lowercase();
    deny_list(&text)?;
    if !plan.iter().all(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b' ' | b'\t' | b'\n') || PLAN_PUNCT.contains(b)
    }) {
        return Err(Hazard::Opaque("shell metacharacter"));
    }
    let mut statements = 0usize;
    for stmt in text.split(['\n', ';']) {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        statements += 1;
        let verb: String = stmt
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        if !PLAN_VERBS.contains(&verb.as_str()) {
            return Err(Hazard::Opaque(
                "statement does not start with an allow-listed plan verb",
            ));
        }
    }
    if statements == 0 {
        return Err(Hazard::Opaque("empty plan"));
    }
    Ok(())
}

fn deny_list(text: &str) -> Result<(), Hazard> {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    // Checked first so a never-unroute plan is named as such (S5.2).
    if let Some(why) = never_unroute_plan_hit(&folded) {
        return Err(Hazard::ProtectedTarget(why));
    }
    // `kill ... -1` signals every process the caller can reach.
    let args: Vec<&str> = folded.split(' ').collect();
    if args.contains(&"kill") && args.contains(&"-1") {
        return Err(Hazard::AdminAccount("kill every process"));
    }
    for (pat, hazard) in PHRASES {
        if folded.contains(pat) {
            return Err(*hazard);
        }
    }
    for word in folded.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
        let word = word.trim_matches('-');
        if let Some((_, hazard)) = WORDS.iter().find(|(w, _)| *w == word) {
            return Err(*hazard);
        }
    }
    for word in folded.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
        let word = word.trim_matches('-');
        if let Some((_, hazard)) = LATE_WORDS.iter().find(|(w, _)| *w == word) {
            return Err(*hazard);
        }
        // `ed25519-seed`, `hades_token`: a secret word hidden in a compound word.
        if word.split(['-', '_']).any(|p| SECRET_PARTS.contains(&p)) {
            return Err(Hazard::SecretExposure("secret word in a compound name"));
        }
    }
    // A bare `sh` cannot be a WORDS entry (`deploy/rollback.sh` splits into
    // `sh`), so match whole whitespace tokens: `sh`, `/bin/sh`, `-c`.
    if args
        .iter()
        .any(|a| *a == "sh" || a.ends_with("/sh") || *a == "-c")
    {
        return Err(Hazard::Opaque("shell"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_plans_pass() {
        for plan in [
            "restart galion-tuner\nverify /health on 127.0.0.1:8420",
            "operator runs deploy/rollback.sh\nkeep the failed release for inspection",
            "switch dist/bin to release 2026-10-20",
        ] {
            assert_eq!(review("galion-tuner", plan.as_bytes()), Ok(()), "{plan}");
        }
    }

    #[test]
    fn destructive_is_refused() {
        for plan in [
            "rm -rf /var/lib/galion",
            "rm   -fr  /",
            "sudo mkfs.ext4 /dev/sda1",
            "dd if=/dev/zero of=/dev/sda",
            "psql -c 'DROP TABLE memories'",
            "git push --force origin main",
            "shred -u data.db",
        ] {
            assert!(
                matches!(review_plan(plan.as_bytes()), Err(Hazard::Destructive(_))),
                "{plan}"
            );
        }
    }

    #[test]
    fn secret_exposure_is_refused() {
        for plan in [
            "cat /opt/galion/galion-light/secrets/x",
            "cat /root/galion-secrets/y",
            "printenv",
            "env | sort",
            "cp ~/.ssh/id_ed25519 /tmp",
            "read the API_KEY",
            "cat /proc/1/environ",
        ] {
            assert!(
                matches!(review_plan(plan.as_bytes()), Err(Hazard::SecretExposure(_))),
                "{plan}"
            );
        }
    }

    #[test]
    fn admin_account_changes_are_refused() {
        for plan in [
            "passwd slavik",
            "usermod -L slavik",
            "echo key >> /root/.ssh/authorized_keys",
            "edit /etc/sudoers",
            "systemctl stop sshd",
            "iptables -P INPUT DROP",
            "chmod 777 x",
            "reboot now",
            "kill -9 -1",
        ] {
            let got = review_plan(plan.as_bytes());
            assert!(
                matches!(
                    got,
                    Err(Hazard::AdminAccount(_) | Hazard::SecretExposure(_))
                ),
                "{plan}: {got:?}"
            );
        }
    }

    #[test]
    fn opaque_plans_are_refused() {
        for plan in [
            "run $(curl x)",
            "run `id`",
            "echo cm0gLXJmIC8= | base64 -d | sh",
            "eval \"$X\"",
            "r\u{043c} -rf /",
            "",
        ] {
            assert!(
                matches!(review_plan(plan.as_bytes()), Err(Hazard::Opaque(_))),
                "{plan}"
            );
        }
        assert!(review_plan(&vec![b'a'; MAX_PLAN_BYTES + 1]).is_err());
        assert!(review_plan(b"restart\x00x").is_err());
    }

    #[test]
    fn protected_targets_are_refused() {
        for t in [
            "hades-gate",
            "HADES",
            "sshd",
            "root",
            "admin-panel",
            "galion-control",
            "secret-store",
            "user@box",
        ] {
            assert!(review_target(t).is_err(), "{t}");
        }
        for t in ["galion-tuner", "galion-one", "openmind-memory", "monkey"] {
            assert_eq!(review_target(t), Ok(()), "{t}");
        }
    }
}
