// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `galion-guardian [--config F] [--once]`. Loop by default; a systemd
//! service, the galion-boot supervisor, or Termux can all run it as is.

use galion_guardian::{beat, config, open_store, probe};
use openmind_survival::SystemRunner;
use std::path::Path;
use std::time::{Duration, Instant};

fn real_main() -> Result<(), (i32, String)> {
    let mut cfg_path = std::env::var("GUARDIAN_CONFIG").ok();
    let mut once = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => cfg_path = Some(it.next().ok_or((2, "--config needs a value".into()))?),
            "--once" => once = true,
            "--version" => {
                println!("galion-guardian {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            o => return Err((2, format!("unknown argument {o}"))),
        }
    }
    let p = cfg_path.ok_or((2, "no config (--config or GUARDIAN_CONFIG)".to_string()))?;
    let text = std::fs::read_to_string(&p).map_err(|e| (2, format!("{p}: {e}")))?;
    let mut f = config::parse(&text).map_err(|e| (2, format!("{p}: {e}")))?;
    if let Ok(d) = std::env::var("GUARDIAN_STATE_DIR") {
        f.guardian.state_dir = d.into();
    }
    let mut store = open_store(&f.guardian).map_err(|e| (2, e))?;
    let env = probe::Env {
        runner: &SystemRunner,
        proc_root: Path::new("/proc"),
        has_systemd: probe::systemd_present(),
    };
    loop {
        let t0 = Instant::now();
        match beat(&f.guardian, &f.probe, &env, store.as_mut()) {
            Ok(out) => println!("{}", out.line()),
            Err(e) => eprintln!("guardian beat error: {e}"),
        }
        if once {
            return Ok(());
        }
        let period = Duration::from_secs(f.guardian.interval_s);
        std::thread::sleep(period.saturating_sub(t0.elapsed()));
    }
}

fn main() {
    if let Err((code, msg)) = real_main() {
        eprintln!("galion-guardian: {msg}");
        std::process::exit(code);
    }
}
