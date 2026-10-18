// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Operator subcommands that replace the last Galion One Python helpers:
//   galion-one set-model <plan|action> <provider/model>   (one/set-model.py)
//   galion-one trace <src.pgm|src.ppm> <dest.svg> [--no-caption]  (one/trace.py)
// With no arguments main() starts the server as before.

use crate::json::{self, dump_str, Json};
use std::path::Path;
use std::time::Duration;

const USAGE: &str = "usage: galion-one [set-model <plan|action> <provider/model> | trace <src.pgm|.ppm> <dest.svg> [--no-caption] | version]";

pub fn run(args: &[String]) -> i32 {
    match args[0].as_str() {
        "set-model" if args.len() == 3 => {
            match set_model(Path::new(&super::modes_path()), &args[1], &args[2]) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("set-model: {e}");
                    1
                }
            }
        }
        "trace" if args.len() >= 3 => {
            let caption = !args[3..].iter().any(|a| a == "--no-caption");
            match trace(Path::new(&args[1]), Path::new(&args[2]), caption) {
                Ok(line) => {
                    println!("{line}");
                    0
                }
                Err(e) => {
                    eprintln!("trace: {e}");
                    1
                }
            }
        }
        "version" | "--version" => {
            println!("galion-one {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

/// set-model.py: lane must be plan or action, the model must contain a slash.
/// `modes` is rebuilt as {"plan": .., "action": ..} (other keys under it are
/// dropped, as before); every other top-level key is kept in place. Written
/// with indent=2 plus a newline, atomically.
pub fn set_model(path: &Path, lane: &str, model: &str) -> Result<(), String> {
    if !matches!(lane, "plan" | "action") || !model.contains('/') {
        return Err("lane must be plan or action and the model must contain '/'".into());
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let Json::Obj(mut top) = json::parse(&text)? else {
        return Err("modes.json is not an object".into());
    };
    let old = top
        .iter()
        .find(|(k, _)| k == "modes")
        .map(|(_, v)| v.clone())
        .unwrap_or(Json::Obj(Vec::new()));
    let keep = |k: &str| {
        json::get(&old, k)
            .cloned()
            .unwrap_or(Json::Str(String::new()))
    };
    let mut modes = vec![
        ("plan".to_string(), keep("plan")),
        ("action".to_string(), keep("action")),
    ];
    for (k, v) in modes.iter_mut() {
        if k == lane {
            *v = Json::Str(model.to_string());
        }
    }
    let modes = Json::Obj(modes);
    match top.iter_mut().find(|(k, _)| k == "modes") {
        Some((_, v)) => *v = modes,
        None => top.push(("modes".to_string(), modes)),
    }
    let out = json::dump_pretty(&Json::Obj(top)) + "\n";
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, out).map_err(|e| format!("write: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

fn trace(src: &Path, dest: &Path, with_caption: bool) -> Result<String, String> {
    let bytes = std::fs::read(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    let grey = crate::trace::decode_pnm(&bytes)?;
    let caption = if with_caption {
        see(src, &bytes)?
    } else {
        String::new()
    };
    let points = crate::trace::corners(&grey)?;
    std::fs::write(dest, crate::trace::svg(&points, &caption))
        .map_err(|e| format!("write: {e}"))?;
    Ok(format!(
        "{{\"caption\": {}, \"corners\": {}, \"path\": {}}}",
        dump_str(&caption),
        points.len(),
        dump_str(&dest.display().to_string())
    ))
}

/// Caption from the eye (GALION_EYE, default 127.0.0.1:8645) /see, 90 s.
fn see(src: &Path, image: &[u8]) -> Result<String, String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (kind, body) = crate::trace::see_body(&name, image);
    let (code, reply) = super::http_to(
        super::eye_addr(),
        "POST",
        "/see",
        &[("Content-Type", kind.as_str())],
        &body,
        Duration::from_secs(90),
    )
    .map_err(|e| format!("eye: {e}"))?;
    if code != 200 {
        return Err(format!("eye answered {code}"));
    }
    Ok(super::json_string(&reply, "caption").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, body: &str) -> std::path::PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn set_model_like_python() {
        let p = tmp(
            "modes.json",
            r#"{"saved": [{"name": "fast", "model": "a/b"}], "modes": {"plan": "x/y", "extra": 1}, "mode": "plan"}"#,
        );
        set_model(&p, "action", "openrouter/some-model").unwrap();
        let want = "{\n  \"saved\": [\n    {\n      \"name\": \"fast\",\n      \"model\": \"a/b\"\n    }\n  ],\n  \"modes\": {\n    \"plan\": \"x/y\",\n    \"action\": \"openrouter/some-model\"\n  },\n  \"mode\": \"plan\"\n}\n";
        assert_eq!(std::fs::read_to_string(&p).unwrap(), want);
    }

    #[test]
    fn set_model_adds_modes_and_rejects_bad_input() {
        let p = tmp("modes2.json", "{}");
        set_model(&p, "plan", "a/b").unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "{\n  \"modes\": {\n    \"plan\": \"a/b\",\n    \"action\": \"\"\n  }\n}\n"
        );
        assert!(set_model(&p, "other", "a/b").is_err());
        assert!(set_model(&p, "plan", "noslash").is_err());
        assert_eq!(run(&["bogus".to_string()]), 2);
    }
}
