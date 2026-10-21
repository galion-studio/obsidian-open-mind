// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Unit-level checks of the parser, origin resolver, ss reader and stager,
//! plus a replay of the 2026-10-20 VPS capture (secrets-free fixture).

use tunnel_lint::*;

const VPS_CFG: &str = include_str!("fixtures/vps-config-20261020.yml");
const VPS_SS: &str = include_str!("fixtures/vps-ss-20261020.txt");

#[test]
fn parses_vps_fixture() {
    let rules = parse(VPS_CFG).unwrap();
    assert_eq!(rules.len(), 39);
    assert_eq!(rules[0].hostname.as_deref(), Some("one.galion.app"));
    assert_eq!(rules[0].path.as_deref(), Some("^/tanga(/.*)?$"));
    assert_eq!(rules[0].service, "http://127.0.0.1:8671");
    let api = rules
        .iter()
        .find(|r| r.hostname.as_deref() == Some("api.galion.app"))
        .unwrap();
    assert_eq!(api.service, "http://127.0.0.1:8000");
    // The nested originRequest block belongs to the rule's line range.
    assert!(api.lines.1 - api.lines.0 >= 6);
    let last = rules.last().unwrap();
    assert_eq!(last.hostname, None);
    assert_eq!(last.service, "http_status:404");
}

#[test]
fn vps_replay_matches_capture() {
    let rules = parse(VPS_CFG).unwrap();
    let snap = SsSnapshot::parse(VPS_SS);
    let rep = lint(&rules, &snap);
    assert_eq!(
        rep.closed_ports(),
        vec![
            3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4000, 4001, 4003, 4004, 4020, 4042,
            4050, 4060, 4444, 5530, 7681, 8000, 8889
        ]
    );
    assert_eq!(rep.closed().count(), 33);
    let open: Vec<_> = rep
        .rules
        .iter()
        .filter_map(|r| match &r.status {
            Status::Open(o) => Some((r.rule.hostname.clone().unwrap(), o.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(open.len(), 5);
    assert!(open.contains(&("tuner.galion.app".into(), "tuner[1196273]".into())));
    assert!(open.contains(&("one.galion.app".into(), "galion-one[1373612]".into())));
    assert!(rep.has_errors());
    // Every nerve-reported dead port is in our list.
    for p in [3004, 3005, 3010, 3011, 3020, 4000, 4001, 4003, 4020, 4060] {
        assert!(rep.closed_ports().contains(&p), "{p}");
    }
}

#[test]
fn staged_vps_config_reparses_with_only_live_routes() {
    let rules = parse(VPS_CFG).unwrap();
    let rep = lint(&rules, &SsSnapshot::parse(VPS_SS));
    let staged = stage(VPS_CFG, &rep);
    let again = parse(&staged).unwrap();
    assert_eq!(again.len(), 6, "5 live routes + catch-all");
    let rep2 = lint(&again, &SsSnapshot::parse(VPS_SS));
    assert!(!rep2.has_errors(), "{:?}", rep2.findings);
    assert!(staged.contains("# tunnel-lint: closed port 4001"));
    // Credentials path and tunnel id lines are left as they were.
    assert!(staged.contains("\ntunnel: 00000000-"));
}

#[test]
fn origins() {
    assert_eq!(
        origin("http://localhost:4001"),
        Origin::Local {
            host: "localhost".into(),
            port: 4001
        }
    );
    assert_eq!(
        origin("https://127.0.0.1"),
        Origin::Local {
            host: "127.0.0.1".into(),
            port: 443
        }
    );
    assert_eq!(
        origin("http://[::1]:8080/x"),
        Origin::Local {
            host: "::1".into(),
            port: 8080
        }
    );
    assert_eq!(
        origin("ssh://localhost"),
        Origin::Local {
            host: "localhost".into(),
            port: 22
        }
    );
    assert_eq!(
        origin("http://example.com:81"),
        Origin::Remote {
            host: "example.com".into(),
            port: 81
        }
    );
    assert_eq!(
        origin("http_status:404"),
        Origin::Builtin("http_status:404".into())
    );
    assert_eq!(
        origin("unix:/run/x.sock"),
        Origin::Builtin("unix:/run/x.sock".into())
    );
    assert!(matches!(origin("tcp://localhost"), Origin::Invalid(_)));
    assert!(matches!(origin("http://localhost:0"), Origin::Invalid(_)));
    assert!(matches!(
        origin("http://localhost:99999"),
        Origin::Invalid(_)
    ));
    assert!(matches!(origin("ftp://localhost:21"), Origin::Invalid(_)));
}

struct Fixed(Vec<u16>);
impl Listeners for Fixed {
    fn check(&self, _h: &str, p: u16) -> Option<String> {
        self.0.contains(&p).then(String::new)
    }
}

#[test]
fn structural_findings() {
    let cfg = "ingress:\n  - hostname: a.example\n    service: http://localhost:1\n  - service: http_status:404\n  - hostname: b.example\n    service: http://localhost:2\n";
    let rep = lint(&parse(cfg).unwrap(), &Fixed(vec![1, 2]));
    let msgs: Vec<_> = rep.findings.iter().map(|f| f.msg.clone()).collect();
    assert!(msgs.iter().any(|m| m.contains("unreachable")), "{msgs:?}");
    assert!(
        msgs.iter().any(|m| m.contains("must be a catch-all")),
        "{msgs:?}"
    );

    let dup = "ingress:\n- hostname: a\n  service: http://localhost:1\n- hostname: a\n  service: http://localhost:1\n- service: http_status:404\n";
    let rep = lint(&parse(dup).unwrap(), &Fixed(vec![1]));
    assert!(rep.findings.iter().any(|f| f.msg.contains("duplicate")));
    assert!(!rep.has_errors());
}

#[test]
fn comments_quotes_and_inline_items() {
    let cfg = "tunnel: x # id\ningress:\n  # first\n  - hostname: \"a.example\" # quoted\n    service: 'http://127.0.0.1:5'\n\n  - {service: x}\n";
    // Flow mappings are not supported and must not be silently accepted as a rule.
    assert!(parse(cfg).is_err());
    let cfg = "ingress:\n  - hostname: \"a.example\" # quoted\n    path: \"^/x#y$\"\n    service: 'http://127.0.0.1:5'\n  - service: http_status:404\nwarp-routing:\n  enabled: true\n";
    let r = parse(cfg).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].hostname.as_deref(), Some("a.example"));
    assert_eq!(r[0].path.as_deref(), Some("^/x#y$"));
    assert_eq!(r[0].service, "http://127.0.0.1:5");
}

#[test]
fn parse_errors() {
    assert!(parse("tunnel: x\n").is_err());
    assert!(parse("ingress:\n").is_err());
    assert!(parse("ingress:\n  - hostname: a\n").is_err());
    assert!(parse("ingress:\n\t- service: x\n").is_err());
}

#[test]
fn ss_reader_handles_wildcards_and_v6() {
    let ss = "State Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n\
LISTEN 0 128 0.0.0.0:7000 0.0.0.0:* users:((\"a\",pid=1,fd=3))\n\
LISTEN 0 128 [::1]:7001 [::]:*\n\
LISTEN 0 128 *:7002 *:*\n\
LISTEN 0 128 127.0.0.53%lo:53 0.0.0.0:*\n\
LISTEN 0 128 127.0.0.1:7003 0.0.0.0:*\n";
    let s = SsSnapshot::parse(ss);
    assert_eq!(s.check("localhost", 7000).as_deref(), Some("a[1]"));
    assert!(s.check("localhost", 7001).is_some());
    assert!(s.check("127.0.0.1", 7001).is_none());
    assert!(s.check("::1", 7002).is_some());
    assert!(s.check("127.0.0.1", 7003).is_some());
    assert!(s.check("::1", 7003).is_none());
    assert!(s.check("127.0.0.1", 7004).is_none());
}

#[test]
fn live_probe_sees_a_real_listener() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let p = Probe {
        timeout: std::time::Duration::from_millis(300),
    };
    assert!(p.check("localhost", port).is_some());
    assert!(p.check("127.0.0.1", port).is_some());
    drop(l);
    assert!(p.check("127.0.0.1", port).is_none());
}

#[test]
fn catch_all_never_staged_out() {
    let cfg = "ingress:\n  - hostname: a\n    service: http://localhost:1\n  - service: http://localhost:2\n";
    let rep = lint(&parse(cfg).unwrap(), &Fixed(vec![]));
    let staged = stage(cfg, &rep);
    let again = parse(&staged).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].hostname, None);
}

#[test]
fn world_writable_mode() {
    assert!(world_writable(0o100666));
    assert!(!world_writable(0o100640));
}

#[test]
fn item_styles() {
    // Bare dash with keys on following lines, and wide dash spacing.
    let cfg = "ingress:\n  -\n    hostname: a\n    service: http://localhost:1\n  -   hostname: b\n      service: http://localhost:2\n  - service: http_status:404\n";
    let r = parse(cfg).unwrap();
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].hostname.as_deref(), Some("a"));
    assert_eq!(r[1].service, "http://localhost:2");
    // A misaligned key is not taken as the rule's service.
    assert!(parse("ingress:\n- hostname: a\n    service: http://localhost:1\n").is_err());
}
