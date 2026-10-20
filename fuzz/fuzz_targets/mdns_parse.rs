// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! openmind-discovery `dns::parse_message`: the mDNS / DNS-SD wire parser
//! that reads untrusted LAN multicast packets.
//!
//! Invariants: no panic; decoded names are safe ASCII with a trailing dot
//! and at most 255+63 bytes; and a decoded message re-encoded with
//! `build_response` parses back to the same questions and records.

#![no_main]

use libfuzzer_sys::fuzz_target;
use openmind_discovery::dns::{build_response, parse_message};
use openmind_discovery::RData;

fn name_ok(name: &str) -> bool {
    name.ends_with('.')
        && name.len() <= 255 + 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

fuzz_target!(|data: &[u8]| {
    let Ok(msg) = parse_message(data) else {
        return;
    };
    assert!(msg.questions.len() + msg.records.len() <= 256);
    for q in &msg.questions {
        assert!(name_ok(&q.name), "{:?}", q.name);
    }
    for rec in &msg.records {
        assert!(name_ok(&rec.name), "{:?}", rec.name);
        match &rec.rdata {
            RData::Ptr(t) | RData::Srv { target: t, .. } => assert!(name_ok(t), "{t:?}"),
            RData::Txt(pairs) => assert!(pairs.len() <= 32),
            RData::A(a) => assert!(a.parse::<std::net::Ipv4Addr>().is_ok()),
            RData::Aaaa(a) => assert!(a.parse::<std::net::Ipv6Addr>().is_ok()),
        }
    }

    let wire = build_response(msg.id, &msg.questions, &msg.records, &[], u32::MAX, false);
    let back = parse_message(&wire).expect("re-encoded message must parse");
    assert_eq!(back.id, msg.id);
    assert!(back.response);
    assert_eq!(back.questions.len(), msg.questions.len());
    for (a, b) in back.questions.iter().zip(&msg.questions) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.qtype, b.qtype);
    }
    assert_eq!(back.records, msg.records);
});
