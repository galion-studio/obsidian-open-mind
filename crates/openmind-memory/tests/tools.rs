// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Shadow-run tooling over real sockets: snapshot a DB, serve the original
//! and the read-only copy on two loopback ports, run parity. Synthetic data.

use openmind_memory::{api::App, config::Config, engine::Engine, http, store::Store, tools};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn serve(dir: &Path, read_only: bool) -> String {
    let d = dir.to_string_lossy().to_string();
    let cfg = Config::from_env(&move |k| match k {
        "OPENMIND_MEMORY_DATA_DIR" => Some(d.clone()),
        "OPENMIND_MEMORY_READ_ONLY" => read_only.then(|| "1".to_string()),
        _ => None,
    });
    let app = Arc::new(App::new(Engine::new(cfg)));
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || http::serve(l, 1 << 20, 8, Arc::new(move |r| app.handle(r))));
    format!("http://{addr}")
}

#[test]
fn snapshot_then_parity_matches() {
    let root =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (live, shadow) = (root.join("live"), root.join("shadow"));
    {
        let st = Store::open(&live.join("vectors.db"), false).unwrap();
        st.insert_l1(
            "r1",
            "synthetic fact about sailing",
            "persona",
            50,
            "",
            "p",
            "",
            "{}",
        )
        .unwrap();
    }
    let a = serve(&live, false);
    let (s, _) = tools::post_json(&a, "/capture", &serde_json::json!({"user_content": "sailing on Sunday", "assistant_content": "nice", "session_key": "p"}), None).unwrap();
    assert_eq!(s, 200);
    tools::snapshot(&live.join("vectors.db"), &shadow.join("vectors.db")).unwrap();
    assert!(
        tools::snapshot(&live.join("vectors.db"), &shadow.join("vectors.db")).is_err(),
        "never overwrites"
    );
    let b = serve(&shadow, true);
    let q = root.join("q.txt");
    std::fs::write(&q, "sailing\nSunday\nnothing-here\n").unwrap();
    assert!(tools::parity(&a, &b, &q, "p", None).unwrap());
}
