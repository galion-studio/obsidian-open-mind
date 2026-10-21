// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Stand-in for `galion-backup run` in tests/real_backup.rs: the real binary
//! (swarm/rust-ports) always reads the fixed production paths under `/`, so on
//! the box it has nothing to archive. This writes one
//! `galion-<stamp>.tar.gz` into $GALION_BACKUP_DEST and logs like the real one.

fn main() {
    if std::env::args().nth(1).as_deref() != Some("run") {
        eprintln!("usage: fake_galion_backup run");
        std::process::exit(2);
    }
    let dest = std::path::PathBuf::from(std::env::var("GALION_BACKUP_DEST").expect("dest"));
    std::fs::create_dir_all(&dest).unwrap();
    let name = "galion-20261020-231900+0200.tar.gz";
    // gzip magic + a tiny body is enough: galion-deploy only hashes it.
    std::fs::write(
        dest.join(name),
        b"\x1f\x8b\x08\x00fake-galion-backup-archive",
    )
    .unwrap();
    println!("ts=2026-10-20T23:19:00+02:00 svc=galion-backup level=info msg=\"ok\" archive={name}");
}
