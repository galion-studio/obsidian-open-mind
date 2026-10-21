// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// DESK-ID: openmind.B44
//
// Cargo licence SBOM, the Rust port of the former scripts/sbom.sh (bash + jq).
// `cargo metadata --format-version 1 --offline` -> dist/sbom-cargo.json, then
// fail when any package licence is outside the allowlist.
//
// An SPDX expression passes when the allowlist can satisfy it. OR passes if
// one side passes; a slash between ids is treated as OR. AND passes only if
// every side passes. WITH passes only if both ids are on the allowlist.
// A missing, empty or unparsable licence fails. Identifiers are case-sensitive.

use crate::run::{cargo_bin, cmd, output, repo_root, Res};
use serde_json::{json, Value};

pub const ALLOW: [&str; 7] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
];

const RULE: &str = "SPDX expression must be satisfiable from the allowlist. OR passes when one side passes. A slash between license ids is treated as OR. AND passes only when every side passes. WITH passes only when both the license id and the exception id are on the allowlist. Missing, empty, or unparsable licenses fail. Identifiers are case-sensitive.";

fn allowed(id: &str) -> bool {
    ALLOW.contains(&id)
}

/// Tokens: `(`, `)`, `AND`, `OR`, `WITH`, identifiers `[A-Za-z0-9][A-Za-z0-9.+-]*`.
/// `/` becomes OR; any other character separates tokens. Unlike the jq
/// scan, an id such as `ORACLE` stays one token (jq split it into OR + ACLE);
/// both versions reject it, only the reason text differs.
pub fn lex(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    for c in expr.chars() {
        let id_char =
            c.is_ascii_alphanumeric() || (!cur.is_empty() && matches!(c, '.' | '+' | '-'));
        if id_char {
            cur.push(c);
            continue;
        }
        flush(&mut cur, &mut out);
        match c {
            '(' | ')' => out.push(c.to_string()),
            '/' => out.push("OR".into()),
            _ => {}
        }
    }
    flush(&mut cur, &mut out);
    out
}

struct Step {
    ok: bool,
    i: usize,
    err: Option<&'static str>,
}

fn is_op(t: Option<&str>) -> bool {
    matches!(t, Some("AND" | "OR" | "WITH" | "(" | ")"))
}

fn primary(t: &[String], i: usize) -> Step {
    let at = |k: usize| t.get(k).map(String::as_str);
    match at(i) {
        None => Step {
            ok: false,
            i,
            err: Some("truncated"),
        },
        Some("(") => {
            let inner = or(t, i + 1);
            if at(inner.i) != Some(")") {
                Step {
                    ok: false,
                    i: inner.i,
                    err: Some("missing paren"),
                }
            } else {
                Step {
                    ok: inner.ok,
                    i: inner.i + 1,
                    err: inner.err,
                }
            }
        }
        Some("WITH" | "AND" | "OR" | ")") => Step {
            ok: false,
            i,
            err: Some("bad token"),
        },
        Some(id) => {
            if at(i + 1) == Some("WITH") {
                let ex = at(i + 2);
                if ex.is_none() || is_op(ex) {
                    Step {
                        ok: false,
                        i: i + 1,
                        err: Some("bad exception"),
                    }
                } else {
                    Step {
                        ok: allowed(id) && allowed(ex.unwrap_or_default()),
                        i: i + 3,
                        err: None,
                    }
                }
            } else {
                Step {
                    ok: allowed(id),
                    i: i + 1,
                    err: None,
                }
            }
        }
    }
}

fn and(t: &[String], i: usize) -> Step {
    let a = primary(t, i);
    if t.get(a.i).map(String::as_str) == Some("AND") {
        let b = and(t, a.i + 1);
        Step {
            ok: a.ok && b.ok,
            i: b.i,
            err: a.err.or(b.err),
        }
    } else {
        a
    }
}

fn or(t: &[String], i: usize) -> Step {
    let a = and(t, i);
    if t.get(a.i).map(String::as_str) == Some("OR") {
        let b = or(t, a.i + 1);
        Step {
            ok: a.ok || b.ok,
            i: b.i,
            err: a.err.or(b.err),
        }
    } else {
        a
    }
}

/// (licence ok, reason): reason is ok, missing, disallowed, trailing, or a
/// parse error (truncated, missing paren, bad token, bad exception).
pub fn judge(expr: Option<&str>) -> (bool, &'static str) {
    let Some(e) = expr.filter(|e| !e.trim().is_empty()) else {
        return (false, "missing");
    };
    let t = lex(e);
    if t.is_empty() {
        return (false, "missing");
    }
    let r = or(&t, 0);
    if let Some(err) = r.err {
        (false, err)
    } else if r.i != t.len() {
        (false, "trailing")
    } else if r.ok {
        (true, "ok")
    } else {
        (false, "disallowed")
    }
}

/// Build the SBOM document from cargo metadata JSON.
pub fn build(meta: &Value, ts: &str, mode: &str) -> Result<Value, String> {
    let pkgs = meta
        .get("packages")
        .and_then(Value::as_array)
        .filter(|_| meta.get("version").and_then(Value::as_u64).unwrap_or(1) == 1)
        .ok_or("need cargo metadata format-version 1 with a packages array")?;
    let members: Vec<&str> = meta
        .get("workspace_members")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let s = |p: &Value, k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut comps: Vec<Value> = pkgs
        .iter()
        .map(|p| {
            let lic = p.get("license").and_then(Value::as_str);
            let (ok, reason) = judge(lic);
            let id = s(p, "id");
            json!({
                "name": p.get("name"),
                "version": p.get("version"),
                "id": p.get("id"),
                "license": p.get("license"),
                "license_file": p.get("license_file"),
                "workspace_member": members.contains(&id.as_str()),
                "license_ok": ok,
                "reason": reason,
            })
        })
        .collect();
    comps.sort_by_key(|c| (s(c, "name"), s(c, "version"), s(c, "id")));
    let failed = comps
        .iter()
        .filter(|c| c["license_ok"] != json!(true))
        .count();
    let n = comps.len();
    Ok(json!({
        "bomFormat": "openmind-cargo-sbom",
        "specVersion": "1",
        "desk_id": "openmind.B44",
        "generated": ts,
        "source": "cargo metadata --format-version 1 --offline",
        "mode": mode,
        "workspace_root": meta.get("workspace_root"),
        "allowlist": ALLOW,
        "license_rule": RULE,
        "components": comps,
        "summary": { "packages": n, "failed": failed, "ok": n > 0 && failed == 0 },
    }))
}

/// `cargo xtask sbom [--check FILE]`.
pub fn run(args: &[String]) -> Res {
    let (mode, raw) = match args {
        [] => {
            // Fill the cache without rewriting Cargo.lock, then read offline.
            crate::run::cargo(&["fetch", "--locked"])?;
            let raw = output(cmd(
                &cargo_bin(),
                &["metadata", "--format-version", "1", "--offline"],
            ))?;
            ("write", raw)
        }
        [flag, file] if flag == "--check" => (
            "check",
            std::fs::read(file).map_err(|e| format!("{file}: {e}"))?,
        ),
        _ => return Err("usage: cargo xtask sbom [--check FILE]".into()),
    };
    let meta: Value = serde_json::from_slice(&raw).map_err(|e| format!("bad JSON: {e}"))?;
    let ts = zagreb_time::now_rfc3339();
    let doc = build(&meta, &ts, mode)?;
    println!("sbom: DESK-ID openmind.B44");
    println!("sbom: time {ts}");
    println!("sbom: mode {mode}");
    println!("sbom: allow {}", ALLOW.join(" "));
    println!("status\tkind\tname\tversion\tlicense\treason");
    for c in doc["components"].as_array().into_iter().flatten() {
        let txt = |k: &str| match c[k].as_str() {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => "-".into(),
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            if c["license_ok"] == json!(true) {
                "ok"
            } else {
                "FAIL"
            },
            if c["workspace_member"] == json!(true) {
                "workspace"
            } else {
                "dep"
            },
            txt("name"),
            txt("version"),
            txt("license"),
            txt("reason"),
        );
    }
    if mode == "write" {
        let dist = repo_root().join("dist");
        std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
        let path = dist.join("sbom-cargo.json");
        let body = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("sbom: wrote {}", path.display());
    }
    let sum = &doc["summary"];
    if sum["ok"] == json!(true) {
        println!("sbom: ok ({} packages)", sum["packages"]);
        Ok(())
    } else {
        println!(
            "sbom: FAIL ({} packages, {} disallowed)",
            sum["packages"], sum["failed"]
        );
        Err("licence outside the allowlist (or empty graph)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spdx_judge() {
        assert_eq!(judge(Some("MIT")), (true, "ok"));
        assert_eq!(judge(Some("MIT OR Apache-2.0")), (true, "ok"));
        assert_eq!(judge(Some("MIT/Apache-2.0")), (true, "ok"));
        assert_eq!(judge(Some("Apache-2.0 / MIT")), (true, "ok"));
        assert_eq!(judge(Some("GPL-3.0 OR MIT")), (true, "ok"));
        assert_eq!(judge(Some("GPL-3.0")), (false, "disallowed"));
        assert_eq!(judge(Some("MIT AND GPL-3.0")), (false, "disallowed"));
        assert_eq!(
            judge(Some("(MIT OR Apache-2.0) AND Unicode-3.0")),
            (true, "ok")
        );
        assert_eq!(
            judge(Some("Apache-2.0 WITH LLVM-exception")),
            (false, "disallowed")
        );
        assert_eq!(judge(Some("Apache-2.0 WITH MIT")), (true, "ok"));
        assert_eq!(judge(Some("MPL-2.0")), (false, "disallowed"));
        assert_eq!(judge(Some("mit")), (false, "disallowed"));
        assert_eq!(judge(None), (false, "missing"));
        assert_eq!(judge(Some("   ")), (false, "missing"));
        assert_eq!(judge(Some("(MIT")), (false, "missing paren"));
        assert_eq!(judge(Some("MIT AND")), (false, "truncated"));
        assert_eq!(judge(Some("AND MIT")), (false, "bad token"));
        assert_eq!(judge(Some("MIT WITH")), (false, "bad exception"));
        assert_eq!(judge(Some("MIT Apache-2.0")), (false, "trailing"));
        assert_eq!(judge(Some("ORACLE")), (false, "disallowed"));
    }

    #[test]
    fn sbom_document() {
        let meta = json!({
            "version": 1,
            "workspace_root": "/r",
            "workspace_members": ["b 0.1.0"],
            "packages": [
                {"name": "b", "version": "0.1.0", "id": "b 0.1.0", "license": "MIT", "license_file": null},
                {"name": "a", "version": "1.0.0", "id": "a 1.0.0", "license": "GPL-3.0", "license_file": null}
            ]
        });
        let d = build(&meta, "2026-10-21T03:00:00+02:00", "check").unwrap();
        assert_eq!(d["components"][0]["name"], "a");
        assert_eq!(d["components"][0]["license_ok"], false);
        assert_eq!(d["components"][1]["workspace_member"], true);
        assert_eq!(
            d["summary"],
            json!({"packages": 2, "failed": 1, "ok": false})
        );
        let empty = json!({"version": 1, "packages": []});
        assert_eq!(build(&empty, "t", "check").unwrap()["summary"]["ok"], false);
        assert!(build(&json!({"version": 2, "packages": []}), "t", "check").is_err());
    }
}
