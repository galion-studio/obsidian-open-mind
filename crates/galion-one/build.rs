// `one` reaches SQLite through hand-written `extern "C"` declarations, so the
// system SQLite shared library has to be named at link time - the same library
// the live `one` binary links today (`ldd` shows libsqlite3.so.0). No rusqlite
// in this ticket.
//
// This box has the runtime library (libsqlite3.so.0) but not the -dev symlink
// `libsqlite3.so` that a bare `-lsqlite3` needs. So this script creates that one
// symlink inside OUT_DIR (under target/, never outside it) and adds OUT_DIR to
// the linker search path. Nothing on the box is modified and the resulting
// binary still records DT_NEEDED libsqlite3.so.0, exactly like the live one.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

fn main() {
    let candidates = [
        "/usr/lib/x86_64-linux-gnu/libsqlite3.so.0",
        "/lib/x86_64-linux-gnu/libsqlite3.so.0",
        "/usr/lib64/libsqlite3.so.0",
        "/usr/lib/libsqlite3.so.0",
        "/usr/local/lib/libsqlite3.so.0",
    ];
    match candidates.iter().map(Path::new).find(|p| p.exists()) {
        Some(found) => {
            let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
            let link_dir = out.join("sqlite-link");
            std::fs::create_dir_all(&link_dir).expect("create sqlite-link dir in OUT_DIR");
            let link = link_dir.join("libsqlite3.so");
            let _ = std::fs::remove_file(&link);
            symlink(found, &link).expect("symlink libsqlite3.so");
            println!("cargo:rustc-link-search=native={}", link_dir.display());
        }
        None => println!(
            "cargo:warning=libsqlite3.so.0 not found on this box; \
             falling back to the default linker search path"
        ),
    }
    println!("cargo:rustc-link-lib=dylib=sqlite3");
    // `galion_no_sqlite` is a build-time cfg the source switches on, so declare
    // it here to keep the `unexpected_cfgs` lint quiet.
    println!("cargo::rustc-check-cfg=cfg(galion_no_sqlite)");
    println!("cargo:rerun-if-changed=build.rs");
}
