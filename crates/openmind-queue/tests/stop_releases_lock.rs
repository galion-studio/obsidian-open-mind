// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `Handle::stop` must release the store's directory lock before it
//! returns, so an in-process restart on the same data directory never sees
//! "is in use by another process". Connection threads answer the client
//! before they drop their share of the state; before the fix `stop` did not
//! wait for them and `refusals_persist` failed about 1 run in 80 on a busy
//! box (PR #12 head 38a07700, wave-4 slot 360). Loopback port 22300 (slot
//! 360 range 22300-22309).

use openmind_queue::store::Store;
use openmind_queue::{http, server};
use std::time::Duration;

#[test]
fn stop_releases_the_store_lock_at_once() {
    let dir = std::env::temp_dir().join(format!("poseidon-stop-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let opts = server::Options {
        bind: "127.0.0.1:22300".into(),
        data: dir.clone(),
        ..server::Options::default()
    };
    for round in 0..500 {
        let h = server::start(opts.clone()).unwrap_or_else(|e| panic!("round {round}: {e}"));
        let a = h.addr.to_string();
        for _ in 0..3 {
            let (c, _) = http::request(&a, "GET", "/stats", None, Duration::from_secs(10)).unwrap();
            assert_eq!(c, 200);
        }
        h.stop();
        // The lock must be free right now, not "soon".
        match Store::open(&dir) {
            Ok(s) => drop(s),
            Err(e) => panic!("round {round}: store still locked after stop: {e}"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
