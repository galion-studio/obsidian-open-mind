// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! Pure render helpers for the `/desk` snapshot. Every function here takes
//! text that was already read and returns text; nothing touches the disk or
//! the network, so each rule is unit-tested on its own.

use crate::scan::{escape, json_string, json_string_array};

/// Model label without the ` ctx N` suffix and ` (idle)` marker.
pub fn clean_model(raw: &str) -> String {
    let cut = raw.split(" ctx ").next().unwrap_or(raw);
    cut.replace(" (idle)", "").trim().to_string()
}

/// Last non-zero `ctx N` number in the text, or 0.
pub fn last_ctx(link: &str) -> u64 {
    let mut found = 0u64;
    let mut rest = link;
    while let Some(i) = rest.find("ctx ") {
        let tail = &rest[i + 4..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>() {
            if n > 0 {
                found = n;
            }
        }
        rest = tail;
    }
    found
}

/// The last four `events` strings of link.json as JSON array items.
pub fn events(link: &str) -> String {
    let rows = json_string_array(link, "events");
    let take = &rows[rows.len().saturating_sub(4)..];
    take.iter()
        .map(|row| format!("\"{}\"", escape(row)))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn short_gpu(raw: &str) -> String {
    if raw.contains("4060") {
        let mib = raw.split(',').nth(1).unwrap_or("").trim();
        if mib.is_empty() {
            "RTX 4060".to_string()
        } else {
            format!("RTX 4060, {mib}")
        }
    } else if raw.is_empty() {
        "—".to_string()
    } else {
        raw.chars().take(36).collect()
    }
}

pub fn short_cpu(raw: &str) -> String {
    for mark in ["i7-", "i5-", "i9-"] {
        if let Some(i) = raw.find(mark) {
            return raw[i..].chars().take(16).collect();
        }
    }
    if raw.is_empty() {
        "—".to_string()
    } else {
        raw.chars().take(28).collect()
    }
}

/// kB value of a `/proc/meminfo` line.
pub fn kb(src: &str, key: &str) -> u64 {
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            return digits.parse().unwrap_or(0);
        }
    }
    0
}

/// `avail / total GB` from `/proc/meminfo` text.
pub fn pod_ram(meminfo: &str) -> String {
    let total = kb(meminfo, "MemTotal:");
    let avail = kb(meminfo, "MemAvailable:");
    if total == 0 {
        return "—".to_string();
    }
    format!("{} / {} GB", avail / 1024 / 1024, total / 1024 / 1024)
}

/// `load1 / cpus` from `/proc/loadavg` and `/proc/cpuinfo` text.
pub fn load_line(loadavg: &str, cpuinfo: &str) -> String {
    let one = loadavg.split_whitespace().next().unwrap_or("0");
    let n = cpuinfo.matches("processor").count().max(1);
    format!("{one} / {n}")
}

/// Last 12 note lines as JSON array items. Lines that are not a single JSON
/// object (`{...}`) are skipped so one torn line cannot break the snapshot.
pub fn notes(raw: &str) -> String {
    let rows: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('{') && l.ends_with('}'))
        .collect();
    rows[rows.len().saturating_sub(12)..].join(",")
}

/// `(heard, said, turns)` from the one session log.
pub fn one_last(raw: &str) -> (String, String, usize) {
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    let last = lines.last().copied().unwrap_or("");
    (
        json_string(last, "heard"),
        json_string(last, "said"),
        lines.len(),
    )
}

/// Voice name: `jarvis` when the file says so, otherwise `samantha`.
pub fn voice(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim) {
        Some("jarvis") => "jarvis",
        _ => "samantha",
    }
}

/// Focus id: ASCII letters, digits, `-` and `_`, at most 80.
pub fn focus_id(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(80)
        .collect()
}

/// Note text: newlines flattened, cut to 160 chars. `None` when blank.
pub fn note_text(text: &str) -> Option<String> {
    let text = text.replace(['\n', '\r'], " ");
    let text: String = text.chars().take(160).collect();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Notes file content after dropping the last non-blank line.
pub fn drop_last(raw: &str) -> Option<String> {
    let mut rows: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    rows.pop()?;
    let mut out = rows.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Some(out)
}

/// Mindpack stdout if it is a JSON object, else `{}`.
pub fn mindpack_json(stdout: &str) -> String {
    let text = stdout.trim();
    if text.starts_with('{') && text.ends_with('}') {
        text.to_string()
    } else {
        "{}".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_and_ctx() {
        assert_eq!(clean_model("galion:latest ctx 8192"), "galion:latest");
        assert_eq!(clean_model("galion:latest (idle)"), "galion:latest");
        assert_eq!(clean_model(""), "");
        assert_eq!(
            last_ctx(r#"{"model":"g ctx 4096","events":["m ctx 0","x ctx 2048"]}"#),
            2048
        );
        assert_eq!(last_ctx("no context"), 0);
        assert_eq!(last_ctx("ctx "), 0);
    }

    #[test]
    fn events_keeps_last_four_and_escapes() {
        let link = r#"{"events": ["a", "b", "c", "d", "e \"q\""]}"#;
        assert_eq!(events(link), r#""b","c","d","e \"q\"""#);
        assert_eq!(events("{}"), "");
        assert_eq!(events(r#"{"events":["x]y"]}"#), r#""x]y""#);
    }

    #[test]
    fn gpu_and_cpu_labels() {
        assert_eq!(
            short_gpu("NVIDIA GeForce RTX 4060 Laptop GPU, 8188 MiB"),
            "RTX 4060, 8188 MiB"
        );
        assert_eq!(short_gpu("RTX 4060"), "RTX 4060");
        assert_eq!(short_gpu(""), "—");
        assert_eq!(short_gpu(&"x".repeat(50)).len(), 36);
        assert_eq!(
            short_cpu("13th Gen Intel(R) Core(TM) i7-13620H"),
            "i7-13620H"
        );
        assert_eq!(short_cpu(""), "—");
        assert_eq!(short_cpu(&"y".repeat(40)).len(), 28);
    }

    #[test]
    fn proc_text_parsers() {
        let mem = "MemTotal:       65830400 kB\nMemFree: 1 kB\nMemAvailable:   33554432 kB\n";
        assert_eq!(pod_ram(mem), "32 / 62 GB");
        assert_eq!(pod_ram(""), "—");
        assert_eq!(
            load_line("0.42 0.3 0.2 1/100 7", "processor : 0\nprocessor : 1\n"),
            "0.42 / 2"
        );
        assert_eq!(load_line("", ""), "0 / 1");
    }

    #[test]
    fn notes_skip_torn_lines_and_keep_twelve() {
        let mut raw = String::from("{\"at\":1,\"text\":\"torn\n");
        for i in 0..15 {
            raw.push_str(&format!("{{\"at\":{i},\"text\":\"n{i}\"}}\n\n"));
        }
        let out = notes(&raw);
        assert!(out.starts_with("{\"at\":3,"), "{out}");
        assert!(out.ends_with("\"n14\"}"));
        assert_eq!(out.matches("},{").count(), 11);
        assert_eq!(notes(""), "");
    }

    #[test]
    fn one_session_voice_and_focus() {
        let raw =
            "{\"heard\":\"a\",\"said\":\"b\"}\n\n{\"heard\":\"hi \\\"x\\\"\",\"said\":\"yo\"}\n";
        assert_eq!(one_last(raw), ("hi \"x\"".into(), "yo".into(), 2));
        assert_eq!(one_last(""), (String::new(), String::new(), 0));
        assert_eq!(voice(Some("jarvis\n")), "jarvis");
        assert_eq!(voice(Some("other")), "samantha");
        assert_eq!(voice(None), "samantha");
        assert_eq!(focus_id("ab c/../d_e-1!"), "abcd_e-1");
        assert_eq!(focus_id(&"z".repeat(100)).len(), 80);
    }

    #[test]
    fn note_text_and_drop() {
        assert_eq!(note_text("a\nb"), Some("a b".into()));
        assert_eq!(note_text("  \n "), None);
        assert_eq!(note_text(&"é".repeat(200)).unwrap().chars().count(), 160);
        assert_eq!(drop_last("a\nb\n\n"), Some("a\n".into()));
        assert_eq!(drop_last("a\n"), Some(String::new()));
        assert_eq!(drop_last("\n"), None);
    }

    #[test]
    fn mindpack_must_be_an_object() {
        assert_eq!(mindpack_json(" {\"a\":1}\n"), "{\"a\":1}");
        assert_eq!(mindpack_json("Traceback"), "{}");
        assert_eq!(mindpack_json("{\"torn\":"), "{}");
        assert_eq!(mindpack_json(""), "{}");
    }
}
