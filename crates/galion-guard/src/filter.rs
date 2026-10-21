// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! The sshd filter, `mode = normal`, rule for rule the same as the stock
//! fail2ban 1.0.2 `filter.d/sshd.conf` that the VPS runs (see
//! docs/guard-rust.md for the list and the translation notes).
//!
//! The patterns were re-expressed for the Rust `regex` crate, which has no
//! look-around and no conditional groups:
//! * the IPv6 tail `(?<=:):` becomes `:` (every preceding repetition item
//!   already ends in `:`, so the look-behind is always true there);
//! * `(?:(?! from ).)*?` and `(?(cond_user)...)` (rules 2 and 3) become a
//!   lazy capture plus a post-check in [`Check::FailedFor`];
//! * digits are written `[0-9]` so Unicode digits never reach the IP parser.
//!
//! Every captured address is then parsed with `std::net`; a token that is not
//! a valid IP is reported as [`Host::Dns`] and never banned (no DNS lookups).

use std::net::IpAddr;

use regex::Regex;

use crate::ipnet::canonical;

/// Host part of a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    Ip(IpAddr),
    /// Not an IP literal. fail2ban (`usedns = warn`) would resolve it; we
    /// do not (sshd runs with `UseDNS no`, and a PTR answer is attacker data).
    Dns(String),
}

/// One matched sshd line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Index of the rule (0..=21, the same numbering as `fail2ban-client get
    /// sshd failregex`).
    pub rule: usize,
    /// Multi-line id: `host sshd[pid]: ` (one SSH connection).
    pub mlfid: String,
    pub host: Option<Host>,
    pub user: Option<String>,
    /// Line carries an address but is not itself a failure.
    pub nofail: bool,
    /// Connection ended: forget the multi-line state.
    pub forget: bool,
    /// Successful authentication (`Accepted ...`).
    pub gained: bool,
}

#[derive(Debug, Clone, Copy)]
enum Check {
    None,
    /// Rules 2/3: the `(?(cond_user): |(?:(?! from ).)*$)` tail.
    FailedFor {
        always_invalid: bool,
    },
}

struct Rule {
    re: Regex,
    check: Check,
}

/// Address part shared by every rule (fail2ban `<HOST>` for `usedns`).
const HOST: &str = r"(?:\[?(?:(?:::f{4,6}:)?(?P<ip4>(?:[0-9]{1,3}\.){3}[0-9]{1,3})|(?P<ip6>(?:[0-9a-fA-F]{1,4}::?|::){1,7}(?:[0-9a-fA-F]{1,4}|:)))\]?|(?P<dns>[\w\-.^_]*\w))";
/// `__on_port_opt`
const ON_PORT: &str = r"(?: (?:port [0-9]+|on \S+)){0,2}";
/// `__suff`
const SUFF: &str = r"(?: (?:port [0-9]+|on \S+|\[preauth\])){0,3}\s*";
/// `__authng_user`
const AUTHNG_USER: &str = r"(?: (?:invalid|authenticating) user (?P<user>\S+|.*?))?";

fn rules() -> Vec<(String, Check)> {
    let h = HOST;
    let op = ON_PORT;
    let su = SUFF;
    let none = Check::None;
    vec![
        // 0
        (
            format!(
                r"^[aA]uthentication (?:failure|error|failed) for (?P<user>.*) from {h}(?: via \S+)?{su}$"
            ),
            none,
        ),
        // 1
        (
            format!(
                r"^User not known to the underlying authentication module for (?P<user>.*) from {h}{su}$"
            ),
            none,
        ),
        // 2
        (
            format!(
                r"^Failed publickey for invalid user (?P<user>.*?) from {h}{op}(?: ssh[0-9]*)?(?P<rest>.*)$"
            ),
            Check::FailedFor {
                always_invalid: true,
            },
        ),
        // 3
        (
            format!(
                r"^Failed (?:(?P<nofail>publickey)|\S+) for (?P<inv>invalid user )?(?P<user>.*?) from {h}{op}(?: ssh[0-9]*)?(?P<rest>.*)$"
            ),
            Check::FailedFor {
                always_invalid: false,
            },
        ),
        // 4
        (format!(r"^(?P<user>ROOT) LOGIN REFUSED FROM {h}"), none),
        // 5
        (
            format!(r"^[iI](?:llegal|nvalid) user (?P<user>.*?) from {h}{su}$"),
            none,
        ),
        // 6
        (
            format!(
                r"^User (?P<user>\S+|.*?) from {h} not allowed because not listed in AllowUsers{su}$"
            ),
            none,
        ),
        // 7
        (
            format!(
                r"^User (?P<user>\S+|.*?) from {h} not allowed because listed in DenyUsers{su}$"
            ),
            none,
        ),
        // 8
        (
            format!(r"^User (?P<user>\S+|.*?) from {h} not allowed because not in any group{su}$"),
            none,
        ),
        // 9
        (format!(r"^refused connect from \S+ \({h}\)"), none),
        // 10
        (
            format!(r"^Received (?P<forget>disconnect) from {h}{op}:\s*3: .*: Auth fail{su}$"),
            none,
        ),
        // 11
        (
            format!(
                r"^User (?P<user>\S+|.*?) from {h} not allowed because a group is listed in DenyGroups{su}$"
            ),
            none,
        ),
        // 12
        (
            format!(
                r"^User (?P<user>\S+|.*?) from {h} not allowed because none of user's groups are listed in AllowGroups{su}$"
            ),
            none,
        ),
        // 13
        (
            format!(
                r"^(?P<nofail>pam_[a-z]+\(sshd:auth\):\s+authentication failure;)(?:\s+(?:(?:logname|e?uid|tty)=\S*)){{0,4}}\s+ruser=(?P<alt_user>\S*)\s+rhost={h}(?:\s+user=(?P<user>\S*))?{su}$"
            ),
            none,
        ),
        // 14
        (
            format!(
                r"^maximum authentication attempts exceeded for (?P<user>.*) from {h}{op}(?: ssh[0-9]*)?{su}$"
            ),
            none,
        ),
        // 15 (no address: pending until the connection names one)
        (
            format!(r"^User (?P<user>\S+|.*?) not allowed because account is locked{su}"),
            none,
        ),
        // 16
        (
            format!(
                r"^(?P<forget>Disconnecting)(?: from)?(?: (?:invalid|authenticating)) user (?P<user>\S+) {h}{op}:\s*Change of username or service not allowed:\s*.*\[preauth\]\s*$"
            ),
            none,
        ),
        // 17 (no address)
        (
            format!(
                r"^Disconnecting: Too many authentication failures(?: for (?P<user>\S+|.*?))?{su}$"
            ),
            none,
        ),
        // 18
        (
            format!(r"^(?P<nofail>Received (?P<forget>disconnect)) from {h}{op}:\s*11:"),
            none,
        ),
        // 19
        (
            format!(
                r"^(?P<nofail>(?P<forget>(?:Connection (?:closed|reset)|Disconnected))) (?:by|from){AUTHNG_USER} {h}(?:{su}|\s*)$"
            ),
            none,
        ),
        // 20
        (
            format!(r"^(?P<forget>(?P<gained>Accepted \w+)) for (?P<user>\S+) from {h}(?:\s|$)"),
            none,
        ),
        // 21
        (format!(r"^(?P<nofail>Connection from) {h}"), none),
    ]
}

/// fail2ban `prefregex` for sshd, plus `sshd-session` (OpenSSH >= 9.8 logs
/// per-connection messages under that name).
const PREF: &str = r"^(?P<mlfid>\s*(?:\S+\s+)?(?:sshd(?:-session)?(?:\[[0-9]+\])?:?\s+)?(?:kernel:\s?\[ *[0-9]+\.[0-9]+\]:?\s+)?)(?:(?:error|fatal): (?:PAM: )?)?(?P<content>.+)$";

/// Lines longer than this are ignored (sshd lines are < 1 KiB; this bounds
/// regex work on hostile input).
pub const MAX_LINE: usize = 8 * 1024;

pub struct SshdFilter {
    pref: Regex,
    rules: Vec<Rule>,
}

impl Default for SshdFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl SshdFilter {
    pub fn new() -> Self {
        let rules = rules()
            .into_iter()
            .map(|(p, check)| Rule {
                re: Regex::new(&p).unwrap_or_else(|e| panic!("sshd rule does not compile: {e}")),
                check,
            })
            .collect();
        SshdFilter {
            pref: Regex::new(PREF).expect("prefregex compiles"),
            rules,
        }
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Match one log line with the timestamp already removed, e.g.
    /// `host sshd[123]: Invalid user bob from 203.0.113.9 port 4242`.
    /// Lines whose prefix does not name sshd are ignored, so other programs
    /// in auth.log cannot feed the jail.
    pub fn matches(&self, line: &str) -> Option<Match> {
        if line.len() > MAX_LINE {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let pc = self.pref.captures(line)?;
        let mlfid = pc.name("mlfid").map(|m| m.as_str()).unwrap_or("");
        if !mlfid.contains("sshd") {
            return None;
        }
        let content = pc.name("content")?.as_str();
        for (idx, rule) in self.rules.iter().enumerate() {
            let Some(c) = rule.re.captures(content) else {
                continue;
            };
            let user = c.name("user").map(|m| m.as_str().to_string());
            if let Check::FailedFor { always_invalid } = rule.check {
                let rest = c.name("rest").map(|m| m.as_str()).unwrap_or("");
                let u = user.as_deref().unwrap_or("");
                let invalid = always_invalid || c.name("inv").is_some();
                let yes =
                    !u.is_empty() && !u.contains(char::is_whitespace) && rest.starts_with(": ");
                let no = !rest.contains(" from ")
                    && if invalid {
                        !u.contains(" from ")
                    } else {
                        !u.is_empty() && !u.contains(':')
                    };
                if !(yes || no) {
                    continue;
                }
            }
            // Hardening (stricter than fail2ban): an address must end at a
            // token boundary, so "2001:db8::Failed" never yields 2001:db8::fa
            // and "192.0.2.4567" never yields 192.0.2.456.
            if let Some(h) = c.name("ip4").or_else(|| c.name("ip6")) {
                let next = content[h.end()..].chars().next();
                if !matches!(next, None | Some(' ' | '\t' | ']' | ')' | ':' | ',')) {
                    continue;
                }
            }
            let host = host_of(&c);
            return Some(Match {
                rule: idx,
                mlfid: mlfid.to_string(),
                host,
                user: user.filter(|u| !u.is_empty()),
                nofail: c.name("nofail").is_some(),
                forget: c.name("forget").is_some(),
                gained: c.name("gained").is_some(),
            });
        }
        None
    }
}

fn host_of(c: &regex::Captures<'_>) -> Option<Host> {
    if let Some(m) = c.name("ip4") {
        return m
            .as_str()
            .parse::<IpAddr>()
            .ok()
            .map(|ip| Host::Ip(canonical(ip)));
    }
    if let Some(m) = c.name("ip6") {
        return match m.as_str().parse::<IpAddr>() {
            Ok(ip) => Some(Host::Ip(canonical(ip))),
            Err(_) => Some(Host::Dns(m.as_str().to_string())),
        };
    }
    c.name("dns").map(|m| {
        let s = m.as_str();
        match s.parse::<IpAddr>() {
            Ok(ip) => Host::Ip(canonical(ip)),
            Err(_) => Host::Dns(s.to_string()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(line: &str) -> Option<Match> {
        SshdFilter::new().matches(line)
    }
    fn ip(s: &str) -> Option<Host> {
        Some(Host::Ip(s.parse().unwrap()))
    }

    #[test]
    fn all_rules_compile() {
        assert_eq!(SshdFilter::new().rule_count(), 22);
    }

    #[test]
    fn invalid_user() {
        let r = m("ip50 sshd[1234]: Invalid user admin from 203.0.113.7 port 51234").unwrap();
        assert_eq!(r.rule, 5);
        assert_eq!(r.host, ip("203.0.113.7"));
        assert_eq!(r.user.as_deref(), Some("admin"));
        assert_eq!(r.mlfid, "ip50 sshd[1234]: ");
        assert!(!r.nofail);
        let r = m("ip50 sshd[1234]: Invalid user  from 203.0.113.7 port 51234").unwrap();
        assert_eq!(r.rule, 5);
        assert_eq!(r.user, None);
    }

    #[test]
    fn user_injection_takes_last_address() {
        // A username that contains a fake "from <victim>" must not ban the victim.
        let r =
            m("h sshd[1]: Invalid user x from 198.51.100.1 port 1 ssh2 from 203.0.113.8 port 22")
                .unwrap();
        assert_eq!(r.host, ip("203.0.113.8"));
        // Failed password with an injected " from " in the tail does not match rule 3.
        assert!(m("h sshd[1]: Failed password for invalid user x from 198.51.100.1 port 1 ssh2 from 203.0.113.8 port 22 ssh2").is_none());
    }

    #[test]
    fn failed_password_and_publickey() {
        let r = m("h sshd[9]: Failed password for root from 203.0.113.9 port 2222 ssh2").unwrap();
        assert_eq!((r.rule, r.nofail), (3, false));
        assert_eq!(r.host, ip("203.0.113.9"));
        let r =
            m("h sshd[9]: Failed password for invalid user bob from 203.0.113.9 port 2222 ssh2")
                .unwrap();
        assert_eq!((r.rule, r.user.as_deref()), (3, Some("bob")));
        let r = m("h sshd[9]: Failed publickey for root from 203.0.113.9 port 2222 ssh2: ED25519 SHA256:abc").unwrap();
        assert_eq!((r.rule, r.nofail), (3, true));
        let r = m("h sshd[9]: Failed publickey for invalid user bob from 203.0.113.9 port 2222 ssh2: RSA SHA256:abc").unwrap();
        assert_eq!((r.rule, r.nofail), (2, false));
    }

    #[test]
    fn error_prefix_and_max_attempts() {
        let r = m("ip50 sshd[77]: error: maximum authentication attempts exceeded for root from 203.0.113.10 port 4000 ssh2 [preauth]").unwrap();
        assert_eq!(r.rule, 14);
        assert_eq!(r.mlfid, "ip50 sshd[77]: ");
        assert_eq!(r.host, ip("203.0.113.10"));
    }

    #[test]
    fn nofail_forget_gained() {
        let r = m("h sshd[5]: Connection closed by authenticating user root 203.0.113.11 port 6000 [preauth]").unwrap();
        assert_eq!((r.rule, r.nofail, r.forget), (19, true, true));
        let r =
            m("h sshd[5]: Disconnected from invalid user test 203.0.113.11 port 6000 [preauth]")
                .unwrap();
        assert_eq!((r.rule, r.nofail, r.forget), (19, true, true));
        let r = m("h sshd[5]: Connection closed by 203.0.113.11 port 6000").unwrap();
        assert_eq!(r.rule, 19);
        let r =
            m("h sshd[5]: Received disconnect from 203.0.113.11 port 6000:11: Bye Bye [preauth]")
                .unwrap();
        assert_eq!((r.rule, r.nofail, r.forget), (18, true, true));
        let r = m("h sshd[5]: Accepted publickey for root from 203.0.113.12 port 6000 ssh2: ED25519 SHA256:x").unwrap();
        assert_eq!((r.rule, r.gained, r.forget), (20, true, true));
        let r = m("h sshd[5]: Disconnecting: Too many authentication failures [preauth]").unwrap();
        assert_eq!((r.rule, r.host.clone()), (17, None));
        let r = m("h sshd[5]: User bob not allowed because account is locked").unwrap();
        assert_eq!((r.rule, r.host.clone()), (15, None));
    }

    #[test]
    fn ipv6_and_mapped() {
        let r = m("h sshd[5]: Invalid user a from 2001:db8::1 port 22").unwrap();
        assert_eq!(r.host, ip("2001:db8::1"));
        let r = m("h sshd[5]: Invalid user a from ::ffff:203.0.113.5 port 22").unwrap();
        assert_eq!(r.host, ip("203.0.113.5"));
        let r = m("h sshd[5]: Invalid user a from fe80:: port 22").unwrap();
        assert_eq!(r.host, ip("fe80::"));
        let r = m("h sshd[5]: Invalid user a from [2001:db8::2] port 22").unwrap();
        assert_eq!(r.host, ip("2001:db8::2"));
    }

    #[test]
    fn dns_and_bad_ip_not_ip() {
        let r = m("h sshd[5]: Invalid user a from evil.example port 22").unwrap();
        assert_eq!(r.host, Some(Host::Dns("evil.example".into())));
        // 999.1.1.1 is not an IPv4 address: no ban target.
        let r = m("h sshd[5]: Invalid user a from 999.1.1.1 port 22").unwrap();
        assert!(!matches!(r.host, Some(Host::Ip(_))));
    }

    #[test]
    fn address_must_end_at_a_boundary() {
        // Found by the fuzzer: hex letters of the next word were taken as
        // part of an IPv6 address.
        let r = m(
            "h sshd[1]: Failed password for x from 2001:db8::Failed password for 66 r 66 port 1",
        );
        assert!(!matches!(r.and_then(|r| r.host), Some(Host::Ip(_))));
        assert!(m("h sshd[1]: Invalid user a from 192.0.2.4567 port 1")
            .is_none_or(|r| !matches!(r.host, Some(Host::Ip(_)))));
        let r = m("h sshd[1]: Received disconnect from 2001:db8::1: 11: Bye [preauth]");
        assert!(r.is_none() || matches!(r.unwrap().host, Some(Host::Ip(_))));
    }

    #[test]
    fn pam_rule() {
        let r = m("h sshd[5]: pam_unix(sshd:auth): authentication failure; logname= uid=0 euid=0 tty=ssh ruser= rhost=203.0.113.20  user=root").unwrap();
        assert_eq!((r.rule, r.nofail), (13, true));
        assert_eq!(r.host, ip("203.0.113.20"));
    }

    #[test]
    fn other_programs_ignored() {
        assert!(m("h sudo[5]: Invalid user a from 203.0.113.1 port 1").is_none());
        assert!(m("h CRON[5]: pam_unix(cron:session): session opened").is_none());
        assert!(m(
            "h sshd[5]: pam_unix(sshd:session): session opened for user root by root(uid=0)"
        )
        .is_none());
        assert!(
            m("h sshd[5]: fatal: Timeout before authentication for 203.0.113.1 port 22").is_none()
        );
        let long = format!(
            "h sshd[5]: Invalid user {} from 203.0.113.1 port 1",
            "a".repeat(MAX_LINE)
        );
        assert!(m(&long).is_none());
    }

    #[test]
    fn sshd_session_ident() {
        let r = m("h sshd-session[5]: Invalid user a from 203.0.113.1 port 1").unwrap();
        assert_eq!(r.mlfid, "h sshd-session[5]: ");
    }
}
