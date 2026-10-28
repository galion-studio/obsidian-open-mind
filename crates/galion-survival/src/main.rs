// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Refresh survival.json: CPU/disk/GPU host facts + OpenRouter cash.
//! Never prints a key. Paths default to /opt/galion (VPS); /workspace is a
//! symlink to /opt/galion on the live host and is not preferred in docs.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CASH_FLOOR: f64 = 5.0;
const HOLD: &str = "x-ai/grok-code-fast-1";
const DEFAULT_ROOT: &str = "/opt/galion/galion-light";
const DEFAULT_HERMES: &str = "/opt/galion/hermes";
const DEFAULT_KEY_FILE: &str = "/opt/galion/hermes/.env";

struct Paths {
    root: PathBuf,
    hermes: PathBuf,
    key_file: PathBuf,
    disk_path: PathBuf,
}

fn paths_from_env() -> Paths {
    let root = env_path("GALION_LIGHT_ROOT", DEFAULT_ROOT);
    let hermes = env_path("GALION_HERMES_ROOT", DEFAULT_HERMES);
    let key_file = std::env::var("OPENROUTER_KEY_FILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_KEY_FILE));
    let disk_path = env_path("GALION_SURVIVAL_DISK", "/");
    Paths {
        root,
        hermes,
        key_file,
        disk_path,
    }
}

fn env_path(name: &str, default: &str) -> PathBuf {
    std::env::var(name)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn read_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

fn key_from_env_file(text: &str) -> String {
    let mut key = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("OPENROUTER_API_KEY=") {
            key = rest.trim().trim_matches('"').to_string();
        }
    }
    key
}

fn api_key(key_file: &Path) -> String {
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
        if !k.trim().is_empty() {
            return k;
        }
    }
    key_from_env_file(&fs::read_to_string(key_file).unwrap_or_default())
}

/// Fetch OpenRouter /api/v1/key. Key goes to curl via stdin config (never argv).
fn openrouter_credits(key: &str) -> Value {
    let key = key.trim();
    if key.is_empty() {
        return json!({
            "measured": false,
            "reason": "no OPENROUTER_API_KEY",
            "cash_usd": null,
            "key_left": null,
            "key_limit": null,
            "key_usage": null,
            "total_credits": null,
            "total_usage": null,
            "note": "cash_usd is credits minus usage. key_left is a cap, not extra cash."
        });
    }
    if key.contains('"') || key.contains('\n') || key.contains('\r') {
        return json!({
            "measured": false,
            "reason": "OPENROUTER_API_KEY has unusable characters",
            "cash_usd": null
        });
    }
    let url = "https://openrouter.ai/api/v1/key";
    let mut child = match Command::new("curl")
        .args(["-sS", "--fail", "--max-time", "30", "-K", "-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "measured": false,
                "reason": format!("curl spawn: {e}"),
                "cash_usd": null
            });
        }
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = writeln!(si, "header = \"Authorization: Bearer {key}\"");
    }
    let o = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return json!({
                "measured": false,
                "reason": format!("curl: {e}"),
                "cash_usd": null
            });
        }
    };
    if !o.status.success() {
        return json!({
            "measured": false,
            "reason": format!("openrouter HTTP failed (curl exit {:?})", o.status.code()),
            "cash_usd": null
        });
    }
    let body: Value = match serde_json::from_slice(&o.stdout) {
        Ok(v) => v,
        Err(_) => {
            return json!({
                "measured": false,
                "reason": "openrouter response not JSON",
                "cash_usd": null
            });
        }
    };
    let data = body.get("data").cloned().unwrap_or(body);
    let usage = data.get("usage").and_then(|v| v.as_f64());
    let limit = data.get("limit").cloned();
    let limit_remaining = data.get("limit_remaining").cloned();
    // OpenRouter key endpoint: usage is spend; limit may be null (unlimited).
    // Prefer data.limit_remaining when present; else credits-style fields if any.
    let total_credits = data
        .get("limit")
        .and_then(|v| v.as_f64())
        .or_else(|| data.get("total_credits").and_then(|v| v.as_f64()));
    let total_usage = usage.or_else(|| data.get("total_usage").and_then(|v| v.as_f64()));
    let cash = match (total_credits, total_usage) {
        (Some(c), Some(u)) => Some(((c - u) * 100.0).round() / 100.0),
        _ => limit_remaining.as_ref().and_then(|v| v.as_f64()),
    };
    json!({
        "measured": true,
        "cash_usd": cash,
        "key_left": limit_remaining,
        "key_limit": limit,
        "key_usage": usage,
        "total_credits": total_credits,
        "total_usage": total_usage,
        "note": "cash_usd is credits minus usage when both exist; else key_left. key_left is a cap, not extra cash."
    })
}

fn load_avg() -> Option<f64> {
    let t = fs::read_to_string("/proc/loadavg").ok()?;
    t.split_whitespace().next()?.parse().ok()
}

fn disk_used(path: &Path) -> Value {
    let path_s = path.to_string_lossy();
    let out = Command::new("df")
        .args(["-P", path_s.as_ref()])
        .output()
        .ok();
    let Some(o) = out else {
        return json!({
            "volume_used_pct": null,
            "path": path_s,
            "note": "df failed"
        });
    };
    let text = String::from_utf8_lossy(&o.stdout);
    // Filesystem 1024-blocks Used Available Capacity Mounted
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 5 {
            let cap = cols[4].trim_end_matches('%');
            if let Ok(p) = cap.parse::<f64>() {
                return json!({
                    "volume_used_pct": p,
                    "path": path_s,
                    "note": "root filesystem"
                });
            }
        }
    }
    json!({
        "volume_used_pct": null,
        "path": path_s,
        "note": "unparseable df"
    })
}

fn nvidia() -> Value {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.used,memory.free,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let line = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = line.trim().split(',').map(|s| s.trim()).collect();
            if parts.len() >= 4 {
                return json!({
                    "name": parts[0],
                    "used_mib": parts[1].parse::<f64>().ok(),
                    "free_mib": parts[2].parse::<f64>().ok(),
                    "util_pct": parts[3].parse::<f64>().ok()
                });
            }
            json!({"name": null, "used_mib": null, "free_mib": null, "util_pct": null})
        }
        _ => json!({"name": null, "used_mib": null, "free_mib": null, "util_pct": null}),
    }
}

fn receipt(earn: &Path) -> bool {
    let Ok(text) = fs::read_to_string(earn) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if row
            .get("buyer")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .is_some()
            && row.get("price").is_some()
            && row
                .get("delivery")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .is_some()
        {
            return true;
        }
    }
    false
}

/// Lane from hermes modes.json. Never invents a model change for a missing receipt.
fn lane(modes_path: &Path, out_path: &Path, paid: bool) -> Value {
    let store = read_json(modes_path);
    let modes = store.get("modes").cloned().unwrap_or(json!({}));
    let action = modes
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let prev = read_json(out_path)
        .get("lane")
        .cloned()
        .unwrap_or(json!({}));
    let held = prev
        .get("held")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Restore held model if action is the HOLD park and a prior held exists.
    let mut action_out = action.clone();
    let mut held_out = held.clone();
    if action == HOLD && !held.is_empty() && held != HOLD {
        if let Value::Object(mut store_obj) = store {
            if let Some(Value::Object(mut m)) = store_obj.remove("modes") {
                m.insert("action".into(), json!(held));
                action_out = held.clone();
                held_out.clear();
                store_obj.insert("modes".into(), Value::Object(m));
                if let Ok(text) = serde_json::to_string_pretty(&Value::Object(store_obj)) {
                    let _ = atomic_write(modes_path, &(text + "\n"));
                }
            }
        }
    }
    let _ = paid; // receipt is reported; never forces a model rewrite
    json!({
        "action": action_out,
        "held": held_out,
        "floor_usd": CASH_FLOOR,
        "receipt": paid
    })
}

fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

fn refresh(p: &Paths) -> Result<Value, String> {
    let out = p.root.join("state/survival.json");
    let line_path = p.root.join("state/eval/survival.txt");
    let hard = read_json(&p.root.join("state/hardware.json"));
    let gpu_n = nvidia();
    let key = api_key(&p.key_file);
    let credits = openrouter_credits(&key);
    // Drop key from memory scope ASAP (best-effort; String drop).
    drop(key);
    let cash = credits.get("cash_usd").and_then(|v| v.as_f64());
    let _ = cash; // lane no longer auto-switches on cash; skill still reads cash_usd
    let paid = receipt(&p.root.join("state/earn.jsonl"));
    let row_lane = lane(&p.hermes.join("modes.json"), &out, paid);
    let when = zagreb_time::now_rfc3339();
    let cpu_load = load_avg();
    let processors = hard
        .get("processors")
        .cloned()
        .or_else(|| {
            std::thread::available_parallelism()
                .ok()
                .map(|n| json!(n.get()))
        })
        .unwrap_or(json!(1));
    let disk = disk_used(&p.disk_path);
    let row = json!({
        "when": when,
        "cpu": {"load": cpu_load, "processors": processors},
        "disk": disk,
        "gpu": {
            "name": hard.get("gpu").and_then(|g| g.get("name")).cloned().or_else(|| gpu_n.get("name").cloned()),
            "weights": hard.get("weights").cloned().unwrap_or(json!(false)),
            "used_mib": gpu_n.get("used_mib"),
            "free_mib": gpu_n.get("free_mib"),
            "util_pct": gpu_n.get("util_pct"),
            "line": hard.get("line").cloned().unwrap_or(json!("CPU only."))
        },
        "openrouter": credits,
        "runpod": {"measured": false, "reason": "no API key on this host"},
        "revolut": {"measured": false, "reason": "not wired"},
        "lane": row_lane,
        "rules": [
            "This host is the server. Use the free CPU.",
            "Do not start a second weight. Local answers go to 127.0.0.1:8094 when a local model is up.",
            "No receipt means no money was made. Do not invent one. The desk model stays the one the operator set.",
            "Do not launch another pod. Do not invent a RunPod or Revolut balance.",
            "Do not restart the gateway, keep.sh, Whisper, or vision."
        ]
    });
    let text = serde_json::to_string_pretty(&row).map_err(|e| e.to_string())? + "\n";
    atomic_write(&out, &text)?;
    let scope = format!(
        "SCOPE cash={} key_left={} gpu_free_mib={} cpu_load={} action={} receipt={} runpod=unmeasured revolut=unwired volume_used_pct={} volume={} when={}\n",
        credits.get("cash_usd").map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
        credits.get("key_left").map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
        gpu_n.get("free_mib").map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
        cpu_load.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
        row_lane.get("action").and_then(|v| v.as_str()).unwrap_or(""),
        if paid { "yes" } else { "no" },
        disk.get("volume_used_pct").map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
        p.disk_path.to_string_lossy(),
        when
    );
    if let Some(parent) = line_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&line_path, &scope);
    // Print only the SCOPE line (no secrets).
    print!("{scope}");
    Ok(row)
}

fn main() {
    let p = paths_from_env();
    match refresh(&p) {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("galion-survival: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_env_file_reads_last_openrouter_line() {
        let t = "API_SERVER_KEY=aaa\nOPENROUTER_API_KEY=sk-test-one\nOTHER=x\nOPENROUTER_API_KEY=sk-test-two\n";
        assert_eq!(key_from_env_file(t), "sk-test-two");
    }

    #[test]
    fn key_from_env_file_strips_quotes() {
        assert_eq!(
            key_from_env_file("OPENROUTER_API_KEY=\"sk-quoted\"\n"),
            "sk-quoted"
        );
    }

    #[test]
    fn openrouter_empty_key_unmeasured() {
        let v = openrouter_credits("");
        assert_eq!(v["measured"], false);
        assert!(v["cash_usd"].is_null());
    }
}
