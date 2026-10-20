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
//! This is a deny list on top of an allow list (only `heal-apply`, `switch`
//! and `rollback` can be approved at all). It is deliberately over-cautious:
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
];

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
];

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
/// # Errors
/// [`Hazard::ProtectedTarget`].
pub fn review_target(target: &str) -> Result<(), Hazard> {
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
    Ok(())
}

/// Only the plan text.
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
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
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
