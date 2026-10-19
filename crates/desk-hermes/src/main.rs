// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Rust port of `/opt/galion/bin/desk-hermes.py`.
//!
//! Same stdout as the Python original: one compact JSON object
//! (`json.dumps(out, separators=(",", ":"))`) with the same keys in the same
//! order, produced from the same inputs.
//!
//! The Hermes state database is opened READ-ONLY (`SQLITE_OPEN_READ_ONLY`);
//! this program never writes to it.
//!
//! Without the optional env overrides below, every path is exactly the path the
//! Python original hard-coded, so behaviour on the live box is unchanged.
//!
//!   HERMES_HOME      default /opt/galion/hermes
//!   HERMES_STATE_DB  default $HERMES_HOME/state.db

mod jsonout;

use jsonout::{json_str, to_json_compat, truthy};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn jint(value: i64) -> Value {
    Value::Number(value.into())
}

/// CPython `int(float(x))` semantics, including the milliseconds heuristic.
fn secs(value: &Value) -> i64 {
    let n = match value {
        Value::Null => return 0,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => match s.trim().parse::<f64>() {
            Ok(v) => v,
            Err(_) => return 0,
        },
        _ => return 0,
    };
    let n = if n > 10_000_000_000.0 { n / 1000.0 } else { n };
    n.trunc() as i64
}

/// CPython `int(x)` for a JSON value.
fn int_val(value: &Value) -> i64 {
    match value {
        Value::Null => 0,
        Value::Bool(b) => i64::from(*b),
        Value::Number(n) => n.as_f64().unwrap_or(0.0).trunc() as i64,
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map(|f| f.trunc() as i64)
            .unwrap_or(0),
        _ => 0,
    }
}

/// `" ".join(str(text).split())[:n]`
fn clip(text: &str, n: usize) -> String {
    let mut joined = String::new();
    for (i, part) in text.split_whitespace().enumerate() {
        if i > 0 {
            joined.push(' ');
        }
        joined.push_str(part);
    }
    joined.chars().take(n).collect()
}

/// `str(x) if x else ""`
fn json_str_or_empty(value: &Value) -> String {
    if truthy(value) {
        json_str(value)
    } else {
        String::new()
    }
}

/// CPython `len(x)` for the container types that appear in the input.
fn len_of(value: &Value) -> usize {
    match value {
        Value::Array(a) => a.len(),
        Value::String(s) => s.chars().count(),
        Value::Object(o) => o.len(),
        _ => 0,
    }
}

fn read_lossy(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn vr_to_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Number(i.into()),
        ValueRef::Real(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
    }
}

/// One statement's rows. A failing statement yields no rows, mirroring the
/// Python `try/except` that leaves already-assigned fields untouched.
fn query(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut stmt = match conn.prepare(sql) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let ncols = stmt.column_count();
    let mut rows = match stmt.query([]) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    loop {
        match rows.next() {
            Ok(Some(row)) => {
                let mut record = Vec::with_capacity(ncols);
                for i in 0..ncols {
                    let value = row.get_ref(i).map(vr_to_value).unwrap_or(Value::Null);
                    record.push(value);
                }
                out.push(record);
            }
            Ok(None) => break,
            Err(_) => return Vec::new(),
        }
    }
    out
}

/// Every regular file under `root`, recursively (the Python `rglob("*")` walk).
fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => stack.push(path),
                Ok(meta) if meta.is_file() => files.push(path),
                _ => {}
            }
        }
    }
    files
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn mtime(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn main() {
    let home = PathBuf::from(
        std::env::var("HERMES_HOME").unwrap_or_else(|_| "/opt/galion/hermes".to_string()),
    );
    let db_path = std::env::var("HERMES_STATE_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join("state.db"));

    let cache = home.join("context-cache.json");
    let activity = home.join("activity.jsonl");
    let mem = home.join("memories").join("MEMORY.md");
    let file_cache = home.join("cache");
    let task_md = home.join("tasks").join("one-galion.md");
    let task_run = home.join("tasks").join("one-galion.run");
    let yaml = home.join("context_length_cache.yaml");
    let models_dev = home.join("models_dev_cache.json");

    let mut out = serde_json::json!({
        "context": {"cached_sessions": 0, "cached_turns": 0, "cache_bytes": 0, "model_window": 0},
        "cache": {"read": 0, "write": 0, "input": 0, "output": 0, "reasoning": 0, "files": 0, "file_bytes": 0},
        "history": [],
        "sessions": [],
        "mind": {"line": "", "sessions": 0, "messages": 0},
        "tools": [],
        "models": [],
        "roles": {"user": 0, "assistant": 0, "tool": 0},
        "road": [],
        "stores": [],
        "recent": [],
        "cached": [],
        "tasks": [],
    });

    // ---- context-cache.json ------------------------------------------------
    if cache.exists() {
        if let Some(text) = read_lossy(&cache) {
            if let Ok(raw) = serde_json::from_str::<Value>(&text) {
                let mut turns = 0usize;
                if raw.is_object() {
                    let map = raw.as_object().expect("object");
                    for item in map.values() {
                        if item.is_object() {
                            turns += len_of(item.get("turns").unwrap_or(&Value::Null));
                        }
                    }
                    out["context"]["cached_sessions"] = jint(map.len() as i64);
                    let mut cached = Vec::new();
                    for (key, item) in map {
                        let entry = if item.is_object() {
                            serde_json::json!({
                                "id": json_str(&Value::String(key.clone())).chars().take(80).collect::<String>(),
                                "model": json_str_or_empty(item.get("model").unwrap_or(&Value::Null)),
                                "turns": len_of(item.get("turns").unwrap_or(&Value::Null)),
                            })
                        } else {
                            serde_json::json!({
                                "id": key.chars().take(80).collect::<String>(),
                                "model": "",
                                "turns": 0,
                            })
                        };
                        cached.push(entry);
                    }
                    out["cached"] = Value::Array(cached);
                }
                out["context"]["cached_turns"] = jint(turns as i64);
                out["context"]["cache_bytes"] = jint(file_size(&cache) as i64);
            }
        }
    }

    // ---- context_length_cache.yaml ----------------------------------------
    {
        let text = if yaml.exists() {
            read_lossy(&yaml).unwrap_or_default()
        } else {
            String::new()
        };
        let mut digits = String::new();
        for ch in text.trim().chars().rev() {
            if ch.is_ascii_digit() {
                digits.insert(0, ch);
            } else if !digits.is_empty() {
                break;
            }
        }
        if !digits.is_empty() {
            if let Ok(n) = digits.parse::<i64>() {
                out["context"]["model_window"] = jint(n);
            }
        }
    }

    // ---- FILE_CACHE --------------------------------------------------------
    {
        let files = if file_cache.exists() {
            walk_files(&file_cache)
        } else {
            Vec::new()
        };
        out["cache"]["files"] = jint(files.len() as i64);
        let total: u64 = files.iter().map(|p| file_size(p)).sum();
        out["cache"]["file_bytes"] = jint(total as i64);

        let mut groups: Vec<(String, u64, u64)> = Vec::new();
        for path in &files {
            let rel = path.strip_prefix(&file_cache).unwrap_or(path);
            let kind = if rel.components().count() > 1 {
                rel.components()
                    .next()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .unwrap_or_else(|| "catalog".to_string())
            } else {
                "catalog".to_string()
            };
            let size = file_size(path);
            match groups.iter_mut().find(|(name, _, _)| *name == kind) {
                Some(bucket) => {
                    bucket.1 += 1;
                    bucket.2 += size;
                }
                None => groups.push((kind, 1, size)),
            }
        }
        groups.sort_by_key(|g| std::cmp::Reverse(g.2));

        let mut stores: Vec<Value> = groups
            .iter()
            .map(|(name, n, bytes)| serde_json::json!({"name": name, "n": n, "bytes": bytes}))
            .collect();
        if models_dev.exists() {
            stores.push(serde_json::json!({
                "name": "models.dev",
                "n": 1,
                "bytes": file_size(&models_dev),
            }));
        }
        out["stores"] = Value::Array(stores);

        let mut sorted: Vec<&PathBuf> = files.iter().collect();
        sorted.sort_by_key(|a| std::cmp::Reverse(mtime(a)));
        let recent: Vec<Value> = sorted
            .iter()
            .take(8)
            .map(|path| {
                let rel = path.strip_prefix(&file_cache).unwrap_or(path);
                serde_json::json!({
                    "name": rel.to_string_lossy(),
                    "bytes": file_size(path),
                    "at": mtime(path),
                })
            })
            .collect();
        out["recent"] = Value::Array(recent);
    }

    // ---- state.db (READ-ONLY) ---------------------------------------------
    if let Ok(conn) = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        let usage = query(
            &conn,
            "select coalesce(sum(input_tokens),0), coalesce(sum(output_tokens),0), \
             coalesce(sum(cache_read_tokens),0), coalesce(sum(cache_write_tokens),0), \
             coalesce(sum(reasoning_tokens),0) from session_model_usage",
        );
        if let Some(row) = usage.first() {
            let get = |i: usize| row.get(i).unwrap_or(&Value::Null);
            out["cache"]["input"] = jint(int_val(get(0)));
            out["cache"]["output"] = jint(int_val(get(1)));
            out["cache"]["read"] = jint(int_val(get(2)));
            out["cache"]["write"] = jint(int_val(get(3)));
            out["cache"]["reasoning"] = jint(int_val(get(4)));
        }
        let count = |sql: &str| -> Option<i64> {
            query(&conn, sql)
                .first()
                .and_then(|r| r.first())
                .map(int_val)
        };
        if let Some(n) = count("select count(*) from sessions") {
            out["mind"]["sessions"] = jint(n);
        }
        if let Some(n) = count("select count(*) from messages") {
            out["mind"]["messages"] = jint(n);
        }

        let road_sql = |limit: i64| {
            format!(
                "select s.id, coalesce(s.source,''), coalesce(s.model,''), count(m.id), max(m.timestamp) \
                 from sessions s left join messages m on m.session_id = s.id \
                 group by s.id having count(m.id) > 0 \
                 order by max(m.timestamp) desc limit {limit}"
            )
        };
        let build_road = |rows: &Vec<Vec<Value>>| -> Value {
            Value::Array(
                rows.iter()
                    .map(|row| {
                        let get = |i: usize| row.get(i).unwrap_or(&Value::Null).clone();
                        serde_json::json!({
                            "id": get(0),
                            "source": get(1),
                            "model": get(2),
                            "messages": int_val(&get(3)),
                            "at": secs(&get(4)),
                        })
                    })
                    .collect(),
            )
        };
        let sessions = query(&conn, &road_sql(6));
        if !sessions.is_empty() {
            out["sessions"] = build_road(&sessions);
        }
        let road = query(&conn, &road_sql(12));
        if !road.is_empty() {
            out["road"] = build_road(&road);
        }

        let tools: Vec<Value> = query(
            &conn,
            "select tool_name, count(*) from messages \
             where tool_name is not null and tool_name != '' \
             group by 1 order by 2 desc limit 12",
        )
        .iter()
        .map(|row| {
            serde_json::json!({
                "name": row.first().unwrap_or(&Value::Null),
                "n": int_val(row.get(1).unwrap_or(&Value::Null)),
            })
        })
        .collect();
        out["tools"] = Value::Array(tools);

        let models: Vec<Value> = query(
            &conn,
            "select model, count(*), sum(input_tokens), sum(output_tokens), \
             sum(cache_read_tokens), sum(reasoning_tokens) \
             from session_model_usage group by 1 order by sum(input_tokens) desc limit 6",
        )
        .iter()
        .map(|row| {
            let get = |i: usize| row.get(i).unwrap_or(&Value::Null);
            serde_json::json!({
                "name": json_str_or_empty(get(0)),
                "calls": int_val(get(1)),
                "input": int_val(get(2)),
                "output": int_val(get(3)),
                "read": int_val(get(4)),
                "reasoning": int_val(get(5)),
            })
        })
        .collect();
        out["models"] = Value::Array(models);

        let role_rows = query(&conn, "select role, count(*) from messages group by 1");
        let role = |want: &str| -> i64 {
            role_rows
                .iter()
                .find(|row| {
                    row.first()
                        .map(|v| json_str_or_empty(v) == want)
                        .unwrap_or(false)
                })
                .map(|row| int_val(row.get(1).unwrap_or(&Value::Null)))
                .unwrap_or(0)
        };
        out["roles"]["user"] = jint(role("user"));
        out["roles"]["assistant"] = jint(role("assistant"));
        out["roles"]["tool"] = jint(role("tool"));

        let tasks: Vec<Value> = query(
            &conn,
            "select delegation_id, state, dispatched_at from async_delegations \
             order by dispatched_at desc limit 4",
        )
        .iter()
        .map(|row| {
            let get = |i: usize| row.get(i).unwrap_or(&Value::Null);
            serde_json::json!({
                "id": get(0),
                "state": json_str_or_empty(get(1)),
                "title": "",
                "at": secs(get(2)),
            })
        })
        .collect();
        out["tasks"] = Value::Array(tasks);
    }

    // ---- activity.jsonl ----------------------------------------------------
    if activity.exists() {
        if let Some(text) = read_lossy(&activity) {
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(5);
            let mut hist = Vec::new();
            let mut ok = true;
            for line in &lines[start..] {
                match serde_json::from_str::<Value>(line) {
                    Ok(item) => {
                        hist.push(serde_json::json!({
                            "at": secs(item.get("at").unwrap_or(&Value::Null)),
                            "kind": json_str_or_empty(item.get("kind").unwrap_or(&Value::Null)),
                            "text": clip(
                                &json_str_or_empty(item.get("text").unwrap_or(&Value::Null)),
                                90,
                            ),
                        }));
                    }
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                out["history"] = Value::Array(hist);
            }
        }
    }

    // ---- MEMORY.md ---------------------------------------------------------
    if mem.exists() {
        if let Some(text) = read_lossy(&mem) {
            let head = text.split('§').next().unwrap_or_default().to_string();
            out["mind"]["line"] = Value::String(clip(&head, 180));
        }
    }

    // ---- tasks/one-galion.md ----------------------------------------------
    if task_md.exists() {
        if let Some(text) = read_lossy(&task_md) {
            let title = text.lines().next().unwrap_or_default().trim().to_string();
            let state = if task_run.exists() {
                read_lossy(&task_run).unwrap_or_default().trim().to_string()
            } else {
                String::new()
            };
            let entry = serde_json::json!({
                "id": "one-galion",
                "state": clip(&state, 24),
                "title": clip(&title, 72),
                "at": 0,
            });
            if let Some(array) = out["tasks"].as_array_mut() {
                array.insert(0, entry);
            }
        }
    }

    println!("{}", to_json_compat(&out, false));
}
