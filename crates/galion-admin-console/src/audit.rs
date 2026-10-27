// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Hash-chained JSONL audit log (same design as galion-os `galion-admin`
//! audit, slot 165): `hash` = SHA-256 of the line up to `,"hash":`, `prev` =
//! previous line's hash (genesis 64 zeros). Tamper-evident, not tamper-proof.
//! Fields are restricted to a safe charset, so no token, code or free text
//! from a request can ever be logged.

use galion_common::{json, sha256, time};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Keep only `[A-Za-z0-9._:-]`, at most 64 chars; anything else becomes `?`.
pub fn safe(s: &str) -> String {
    s.chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-') {
                c
            } else {
                '?'
            }
        })
        .collect()
}

fn line_for(seq: u64, ts: &str, action: &str, target: &str, result: &str, prev: &str) -> String {
    let body = format!(
        "{{\"v\":1,\"seq\":{seq},\"ts\":{},\"action\":{},\"target\":{},\"result\":{},\"prev\":{}",
        json::string(ts),
        json::string(&safe(action)),
        json::string(&safe(target)),
        json::string(&safe(result)),
        json::string(prev)
    );
    let hash = sha256::hex(body.as_bytes());
    format!("{body},\"hash\":\"{hash}\"}}")
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let i = line.find(&pat)? + pat.len();
    let rest = &line[i..];
    if let Some(r) = rest.strip_prefix('"') {
        r.split('"').next()
    } else {
        rest.split([',', '}']).next()
    }
}

/// Append one record under an exclusive lock, fsync'd. Returns the new hash.
pub fn append(path: &Path, action: &str, target: &str, result: &str) -> io::Result<String> {
    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    restrict(path)?;
    f.lock()?;
    let (seq, prev) = last(&mut f)?;
    let line = line_for(seq + 1, &time::now_rfc3339(), action, target, result, &prev);
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_data()?;
    f.unlock()?;
    Ok(field(&line, "hash").unwrap_or_default().to_string())
}

#[cfg(unix)]
fn restrict(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn restrict(_: &Path) -> io::Result<()> {
    Ok(())
}

fn last(f: &mut File) -> io::Result<(u64, String)> {
    f.seek(SeekFrom::Start(0))?;
    let mut out = (0, GENESIS.to_string());
    for line in BufReader::new(&mut *f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let seq = field(&line, "seq")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let hash = field(&line, "hash").unwrap_or(GENESIS).to_string();
        out = (seq, hash);
    }
    Ok(out)
}

/// Outcome of [`verify`].
#[derive(Debug, PartialEq, Eq)]
pub enum Verify {
    /// Chain intact: number of records and the head hash.
    Ok { records: u64, head: String },
    /// First bad line (1-based) and why.
    Broken { line: u64, why: &'static str },
}

/// Check the whole chain.
pub fn verify(path: &Path) -> io::Result<Verify> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Verify::Ok {
                records: 0,
                head: GENESIS.into(),
            })
        }
        Err(e) => return Err(e),
    };
    let mut prev = GENESIS.to_string();
    let mut n = 0u64;
    for line in BufReader::new(f).lines() {
        let line = line?;
        n += 1;
        let Some(cut) = line.find(",\"hash\":\"") else {
            return Ok(Verify::Broken {
                line: n,
                why: "no hash",
            });
        };
        let body = &line[..cut];
        let hash = field(&line, "hash").unwrap_or("");
        if sha256::hex(body.as_bytes()) != hash {
            return Ok(Verify::Broken {
                line: n,
                why: "hash mismatch",
            });
        }
        if field(&line, "prev") != Some(prev.as_str()) {
            return Ok(Verify::Broken {
                line: n,
                why: "prev mismatch",
            });
        }
        if field(&line, "seq").and_then(|s| s.parse::<u64>().ok()) != Some(n) {
            return Ok(Verify::Broken {
                line: n,
                why: "seq gap",
            });
        }
        prev = hash.to_string();
    }
    Ok(Verify::Ok {
        records: n,
        head: prev,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("gac-audit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("audit.jsonl")
    }

    #[test]
    fn chain_and_tamper() {
        let p = tmp("chain");
        append(&p, "pair.code", "-", "issued").unwrap();
        append(&p, "pair.ok", "node-0a1b2c3d", "paired").unwrap();
        append(&p, "pair.fail", "-", "bad code ' \" \n injected").unwrap();
        match verify(&p).unwrap() {
            Verify::Ok { records, .. } => assert_eq!(records, 3),
            v => panic!("{v:?}"),
        }
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("injected\n\""));
        assert!(text.contains("+01:00") || text.contains("+02:00"));
        assert!(!text.contains("Z\""));
        let bad = text.replacen("paired", "PAIRED", 1);
        std::fs::write(&p, bad).unwrap();
        assert_eq!(
            verify(&p).unwrap(),
            Verify::Broken {
                line: 2,
                why: "hash mismatch"
            }
        );
        let lines: Vec<&str> = text.lines().collect();
        std::fs::write(&p, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
        assert!(matches!(
            verify(&p).unwrap(),
            Verify::Broken { line: 2, .. }
        ));
    }

    #[test]
    fn safe_charset() {
        assert_eq!(safe("node-ab12"), "node-ab12");
        assert_eq!(safe("a b\"c"), "a?b?c");
        assert_eq!(safe(&"x".repeat(100)).len(), 64);
    }

    #[test]
    fn parallel_writers_keep_chain() {
        let p = tmp("par");
        let hs: Vec<_> = (0..4)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        append(&p, "t", &format!("w{i}"), "ok").unwrap();
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert!(matches!(
            verify(&p).unwrap(),
            Verify::Ok { records: 80, .. }
        ));
    }
}
