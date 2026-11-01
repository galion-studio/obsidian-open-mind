// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-vault`: the local admin's secrets vault, command line.
//!
//! Secrets and passphrases are NEVER taken from argv or the environment
//! (both are visible in /proc). They are read from stdin, one per line:
//!
//! ```text
//! galion-vault init              [--vault PATH]   stdin: passphrase      -> prints the recovery key ONCE
//! galion-vault list              [--vault PATH]   stdin: passphrase
//! galion-vault set NAME          [--vault PATH]   stdin: passphrase, then the value (rest of stdin)
//! galion-vault get NAME [--copy] [--vault PATH]   stdin: passphrase      -> value on stdout
//! galion-vault rotate-key        [--vault PATH]   stdin: passphrase      -> prints the NEW recovery key once
//! galion-vault change-passphrase [--vault PATH]   stdin: old, new
//! galion-vault export-encrypted --out PATH [--vault PATH]  stdin: passphrase, export passphrase
//! galion-vault audit             [--vault PATH]   stdin: passphrase
//! ```
//!
//! `--recovery`: the first stdin line is the recovery key instead of the
//! passphrase. `recover-passphrase` (stdin: recovery key, new passphrase)
//! sets a new passphrase after a lost one.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;

use galion_vault::{self as gv, Factor, Kdf};
use zeroize::Zeroizing;

const USAGE: &str = "usage: galion-vault <init|list|set NAME|get NAME [--copy]|rotate-key|change-passphrase|recover-passphrase|export-encrypted --out PATH|audit> [--vault PATH] [--recovery]\n\
  Passphrases, recovery keys and values come from stdin, one per line (never argv/env).\n\
  recover-passphrase: stdin = recovery key, new passphrase (sets a new passphrase after a lost one).";

fn die(msg: &str) -> ! {
    eprintln!("galion-vault: {msg}");
    std::process::exit(2)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct Args(Vec<String>);

impl Args {
    fn flag(&self, n: &str) -> bool {
        self.0.iter().any(|a| a == n)
    }
    fn opt(&self, n: &str) -> Option<String> {
        let mut it = self.0.iter();
        while let Some(a) = it.next() {
            if a == n {
                return it.next().cloned();
            }
        }
        None
    }
    /// First argument that is not a flag or a flag's value.
    fn positional(&self) -> Option<String> {
        let mut skip = false;
        for a in &self.0 {
            if skip {
                skip = false;
                continue;
            }
            if a == "--vault" || a == "--out" {
                skip = true;
                continue;
            }
            if !a.starts_with("--") {
                return Some(a.clone());
            }
        }
        None
    }
}

fn line(stdin: &mut impl BufRead) -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::new());
    stdin
        .read_line(&mut s)
        .unwrap_or_else(|e| die(&format!("stdin: {e}")));
    let t = Zeroizing::new(s.trim_end_matches(['\r', '\n']).to_string());
    if t.is_empty() {
        die("expected a line on stdin");
    }
    t
}

fn kdf() -> Kdf {
    // Hidden switch for the test harness only; never set it in production.
    if std::env::var("GALION_VAULT_TEST_KDF").as_deref() == Ok("1") {
        Kdf::TEST
    } else {
        Kdf::PRODUCTION
    }
}

fn open(a: &Args, path: &std::path::Path, stdin: &mut impl BufRead) -> gv::Vault {
    let first = line(stdin);
    let res = if a.flag("--recovery") {
        let k = gv::recovery_from_text(&first).unwrap_or_else(|| die("not a recovery key"));
        gv::unlock(path, Factor::Recovery(&k), now())
    } else {
        gv::unlock(path, Factor::Passphrase(first.as_bytes()), now())
    };
    res.unwrap_or_else(|e| die(&e.to_string()))
}

fn main() {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        die(USAGE);
    }
    let cmd = argv.remove(0);
    let a = Args(argv);
    let path: PathBuf = a
        .opt("--vault")
        .unwrap_or_else(|| "vault.json".into())
        .into();
    let stdin = std::io::stdin();
    let mut si = stdin.lock();
    let mut out = std::io::stdout().lock();
    let t = now();
    match cmd.as_str() {
        "init" => {
            let p = line(&mut si);
            let (_v, rec) =
                gv::init(&path, p.as_bytes(), kdf(), t).unwrap_or_else(|e| die(&e.to_string()));
            eprintln!("vault created: {} (0600). Write the recovery key down OFFLINE; it is shown only now:", path.display());
            let _ = writeln!(out, "{}", gv::recovery_to_text(&rec).as_str());
        }
        "list" => {
            let mut v = open(&a, &path, &mut si);
            for n in v.list(t).unwrap_or_else(|e| die(&e.to_string())) {
                let _ = writeln!(out, "{n}");
            }
        }
        "set" => {
            let name = a.positional().unwrap_or_else(|| die("set NAME"));
            let mut v = open(&a, &path, &mut si);
            let mut val = Zeroizing::new(Vec::new());
            si.read_to_end(&mut val)
                .unwrap_or_else(|e| die(&format!("stdin: {e}")));
            if val.last() == Some(&b'\n') {
                val.pop();
                if val.last() == Some(&b'\r') {
                    val.pop();
                }
            }
            v.set(&name, &val, t)
                .unwrap_or_else(|e| die(&e.to_string()));
            eprintln!("set {name}");
        }
        "get" => {
            let name = a.positional().unwrap_or_else(|| die("get NAME"));
            let mut v = open(&a, &path, &mut si);
            let action = if a.flag("--copy") { "copy" } else { "reveal" };
            let val = v
                .reveal(&name, action, t)
                .unwrap_or_else(|e| die(&e.to_string()));
            let _ = out.write_all(&val);
        }
        "rotate-key" => {
            let p = line(&mut si);
            let mut v = gv::unlock(&path, Factor::Passphrase(p.as_bytes()), t)
                .unwrap_or_else(|e| die(&e.to_string()));
            let rec = v
                .rotate_key(p.as_bytes(), kdf(), t)
                .unwrap_or_else(|e| die(&e.to_string()));
            eprintln!("data key rotated; the OLD recovery key no longer works. New recovery key (shown once):");
            let _ = writeln!(out, "{}", gv::recovery_to_text(&rec).as_str());
        }
        "change-passphrase" => {
            let old = line(&mut si);
            let new = line(&mut si);
            let mut v = gv::unlock(&path, Factor::Passphrase(old.as_bytes()), t)
                .unwrap_or_else(|e| die(&e.to_string()));
            v.change_passphrase(old.as_bytes(), new.as_bytes(), kdf(), t)
                .unwrap_or_else(|e| die(&e.to_string()));
            eprintln!("passphrase changed");
        }
        "recover-passphrase" => {
            let rec = line(&mut si);
            let new = line(&mut si);
            let k = gv::recovery_from_text(&rec).unwrap_or_else(|| die("not a recovery key"));
            let mut v =
                gv::unlock(&path, Factor::Recovery(&k), t).unwrap_or_else(|e| die(&e.to_string()));
            v.set_passphrase_with_recovery(&k, new.as_bytes(), kdf(), t)
                .unwrap_or_else(|e| die(&e.to_string()));
            eprintln!("new passphrase set from the recovery key; consider rotate-key next");
        }
        "export-encrypted" => {
            let dest = a
                .opt("--out")
                .unwrap_or_else(|| die("export-encrypted --out PATH"));
            let mut v = open(&a, &path, &mut si);
            let ep = line(&mut si);
            let text = v
                .export_encrypted(ep.as_bytes(), kdf(), t)
                .unwrap_or_else(|e| die(&e.to_string()));
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&dest)
                .unwrap_or_else(|e| die(&format!("--out {dest}: {e} (must not exist)")));
            f.write_all(text.as_bytes())
                .unwrap_or_else(|e| die(&e.to_string()));
            eprintln!("encrypted export written to {dest}");
        }
        "audit" => {
            let mut v = open(&a, &path, &mut si);
            for l in v.audit_view(t).unwrap_or_else(|e| die(&e.to_string())) {
                let _ = writeln!(out, "{l}");
            }
        }
        "-h" | "--help" | "help" => println!("{USAGE}"),
        _ => die(USAGE),
    }
}
