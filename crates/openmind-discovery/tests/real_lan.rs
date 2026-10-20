// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real multicast round trip on this machine. Runs only with `GALION_REAL=1`.
//!
//! An in-process announcer on mDNS port 19110 (slot 11's range, not 5353,
//! so no other responder on the LAN answers) advertises `_galion._tcp` and
//! `_exo._tcp`; a browse to 224.0.0.251:19110 must find both.

use openmind_discovery::mdns::{browse, local_ipv4, Announcer, BrowseOpts, Service};
use openmind_discovery::{fold_records, DeviceKind};
use std::time::{Duration, Instant};

#[test]
fn announcer_is_found_by_browse_over_real_multicast() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let addr = local_ipv4();
    let announcer = Announcer {
        host: "real-test.local.".into(),
        addr,
        services: vec![
            Service {
                instance: "rt-galion".into(),
                service_type: "_galion._tcp.local.".into(),
                port: 19111,
                txt: vec![("id".into(), "rt-galion".into())],
            },
            Service {
                instance: "rt-exo".into(),
                service_type: "_exo._tcp.local.".into(),
                port: 19112,
                txt: vec![],
            },
        ],
        port: 19110,
        ttl: 30,
    };
    let a = announcer.clone();
    let handle = std::thread::spawn(move || a.run(Instant::now() + Duration::from_secs(4), |_| {}));
    std::thread::sleep(Duration::from_millis(300));
    let out = browse(&BrowseOpts {
        services: vec!["_galion._tcp.local.".into(), "_exo._tcp.local.".into()],
        timeout: Duration::from_millis(1500),
        port: 19110,
        iface: Some(addr),
    })
    .expect("browse");
    let stats = handle.join().unwrap().expect("announcer");
    assert!(out.packets_sent <= 2);
    let rows = fold_records(&out.records);
    let kinds: Vec<_> = rows.iter().filter_map(|c| c.kind()).collect();
    assert!(kinds.contains(&DeviceKind::Galion), "{rows:?}");
    assert!(kinds.contains(&DeviceKind::Exo), "{rows:?}");
    assert!(rows.iter().all(|c| c.approval == "pending"));
    assert_eq!(stats.announcements_sent, 2);
    assert!(stats.replies_sent >= 1);
}
