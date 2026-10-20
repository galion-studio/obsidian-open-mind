// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Always-on check: every axis at 100 against an in-process mock on an
//! ephemeral loopback port (the GALION_REAL tests repeat this against the
//! release binary on 19510-19519).

use bench_banking::{axes, mock::Mock};
use std::net::TcpListener;

#[test]
fn all_axes_full_marks_in_process() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    Mock::new().spawn(l);
    let all = axes::run_all(&base);
    for a in &all {
        let f: Vec<_> = a
            .failing()
            .iter()
            .map(|c| format!("{} :: {}", c.name, c.detail))
            .collect();
        assert!(f.is_empty(), "{}: {f:#?}", a.id);
    }
    assert_eq!(all.len(), 10);
    assert_eq!(axes::pct(&all), 100.0);
}
