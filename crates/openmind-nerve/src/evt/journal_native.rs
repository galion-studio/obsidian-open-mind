// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! In-process systemd journal follower (feature `journal-native`).
//!
//! Replaces the `journalctl -f -o json` child with [`sdjournal`]'s
//! `LiveJournal` when `NERVE_EVT_JOURNALCTL=native` or `native:/path`.
//! Entries become the same JSON field map `parse_entry` already understands.
//!
//! Cursor tokens are sdjournal's own format (not journalctl `__CURSOR`);
//! do not mix backends across a restart without clearing the cursor.

use super::journal::{parse_entry, Msg, FAILURE_IDS, LIFECYCLE_IDS};
use sdjournal::{
    Cursor, JournalConfig, LiveFilter, LiveJournal, LiveSubscription, MmapPolicy,
    SubscriptionOptions,
};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Build the JSON object `parse_entry` expects from raw journal fields.
pub fn fields_to_value<'a, I>(
    fields: I,
    cursor: Option<String>,
    realtime_usec: Option<u64>,
) -> Value
where
    I: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    let mut map = Map::new();
    for (k, v) in fields {
        map.insert(
            k.to_string(),
            Value::String(String::from_utf8_lossy(v).into_owned()),
        );
    }
    if let Some(c) = cursor {
        map.insert("__CURSOR".into(), Value::String(c));
    }
    if let Some(us) = realtime_usec {
        map.insert("__REALTIME_TIMESTAMP".into(), Value::String(us.to_string()));
    }
    Value::Object(map)
}

fn low_mem_config() -> JournalConfig {
    JournalConfig {
        mmap_policy: MmapPolicy::Never,
        max_open_files: 8,
        poll_interval: Duration::from_millis(500),
        live_channel_capacity: 256,
        max_live_batch_entries: 64,
        ..Default::default()
    }
}

fn open_live(spec: &str) -> Result<LiveJournal, String> {
    let cfg = low_mem_config();
    if let Some(path) = spec.strip_prefix("native:") {
        let p = PathBuf::from(path);
        if !p.is_dir() {
            return Err(format!("native journal dir missing: {}", p.display()));
        }
        LiveJournal::open_dir_with_config(&p, cfg).map_err(|e| e.to_string())
    } else if spec == "native" {
        LiveJournal::open_default_with_config(cfg).map_err(|e| e.to_string())
    } else {
        Err(format!("not a native journal spec: {spec}"))
    }
}

/// Match groups mirroring `journal::args`.
pub fn build_filter(live: &LiveJournal, units: &[String]) -> LiveFilter {
    let mut filter = live.filter();
    for id in FAILURE_IDS {
        filter.or_group(|g| {
            g.match_exact("MESSAGE_ID", id.as_bytes());
        });
    }
    for unit in units {
        for id in LIFECYCLE_IDS {
            let u = unit.as_str();
            filter.or_group(|g| {
                g.match_exact("_PID", b"1");
                g.match_exact("UNIT", u.as_bytes());
                g.match_exact("MESSAGE_ID", id.as_bytes());
            });
        }
    }
    for prio in [b"0" as &[u8], b"1", b"2", b"3"] {
        filter.or_group(|g| {
            g.match_exact("_TRANSPORT", b"kernel");
            g.match_exact("PRIORITY", prio);
        });
    }
    filter
}

fn subscribe(
    live: &mut LiveJournal,
    units: &[String],
    after: Option<&str>,
) -> Result<LiveSubscription, String> {
    let filter = build_filter(live, units);
    let mut opts = SubscriptionOptions::new(filter);
    if let Some(c) = after {
        let cur = Cursor::parse(c).map_err(|e| format!("cursor parse: {e}"))?;
        opts.after_cursor(cur);
    }
    live.subscribe_with_options(opts).map_err(|e| e.to_string())
}

/// Start the native follower thread. Same `Msg` contract as the journalctl child.
pub fn start(
    spec: String,
    units_shared: Arc<Mutex<Vec<String>>>,
    restart: Arc<AtomicBool>,
    tx: Sender<super::runtime::Input>,
) {
    let send = move |m: Msg| tx.send(super::runtime::Input::Journal(m)).is_ok();
    let _ = std::thread::Builder::new()
        .name("nerve-evt-journal-native".into())
        .spawn(move || {
            let mut cursor: Option<String> = None;
            let mut backoff = 5u64;
            loop {
                let list = units_shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let mut live = match open_live(&spec) {
                    Ok(j) => j,
                    Err(e) => {
                        if !send(Msg::Source(
                            "journal".into(),
                            format!("native open ({spec}): {e}"),
                        )) {
                            return;
                        }
                        std::thread::sleep(Duration::from_secs(backoff));
                        backoff = (backoff * 2).min(300);
                        continue;
                    }
                };
                let sub = match subscribe(&mut live, &list, cursor.as_deref()) {
                    Ok(s) => s,
                    Err(e) => {
                        if !send(Msg::Source(
                            "journal".into(),
                            format!("native subscribe: {e}"),
                        )) {
                            return;
                        }
                        std::thread::sleep(Duration::from_secs(backoff));
                        backoff = (backoff * 2).min(300);
                        continue;
                    }
                };
                let eng = match std::thread::Builder::new()
                    .name("nerve-evt-sdjournal".into())
                    .spawn(move || {
                        let _ = live.run();
                    }) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = send(Msg::Source(
                            "journal".into(),
                            format!("native engine spawn: {e}"),
                        ));
                        std::thread::sleep(Duration::from_secs(backoff));
                        backoff = (backoff * 2).min(300);
                        continue;
                    }
                };
                backoff = 5;
                let mut stop_reason = "native journal follower ended";
                let mut requested = false;
                loop {
                    if restart.swap(false, Ordering::SeqCst) {
                        requested = true;
                        stop_reason = "native restart (unit list)";
                        break;
                    }
                    match sub.recv_timeout(Duration::from_millis(500)) {
                        Ok(Ok(entry)) => {
                            let cur = entry.cursor().ok().map(|c| c.to_string());
                            if let Some(ref c) = cur {
                                cursor = Some(c.clone());
                            }
                            let v = fields_to_value(
                                entry.iter_fields(),
                                cur,
                                Some(entry.realtime_usec()),
                            );
                            if let Some(ev) = parse_entry(&v) {
                                if !send(Msg::Event(ev)) {
                                    drop(sub);
                                    let _ = eng.join();
                                    return;
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            let _ =
                                send(Msg::Source("journal".into(), format!("native read: {e}")));
                            break;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                drop(sub);
                let _ = eng.join();
                if requested {
                    let _ = stop_reason;
                    continue;
                }
                if !send(Msg::Source("journal".into(), stop_reason.into())) {
                    return;
                }
                std::thread::sleep(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(300);
            }
        });
}

/// True when `NERVE_EVT_JOURNALCTL` selects the in-process backend.
pub fn is_native_spec(bin: &str) -> bool {
    bin == "native" || bin.starts_with("native:")
}

/// Resolve a journal directory for tests (may be empty of *.journal files).
pub fn test_journal_dir() -> Option<PathBuf> {
    for p in ["/var/log/journal", "/run/log/journal"] {
        let path = Path::new(p);
        if path.is_dir() {
            return Some(path.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evt::journal::{ID_RESTART_SCHEDULED, ID_STARTED};
    use crate::evt::Severity;

    #[test]
    fn fields_to_value_feeds_parse_entry() {
        let fields = [
            ("MESSAGE_ID", ID_RESTART_SCHEDULED.as_bytes()),
            ("UNIT", b"openmind-queue.service" as &[u8]),
            (
                "MESSAGE",
                b"openmind-queue.service: Scheduled restart job, restart counter is at 1."
                    as &[u8],
            ),
            ("N_RESTARTS", b"1" as &[u8]),
        ];
        let v = fields_to_value(
            fields,
            Some("s=native-test".into()),
            Some(1_761_058_000_000_000),
        );
        assert_eq!(v["__CURSOR"], "s=native-test");
        let ev = parse_entry(&v).expect("event");
        assert_eq!(ev.kind, "unit.restart_scheduled");
        assert_eq!(ev.severity, Severity::High);
        assert_eq!(ev.t, 1_761_058_000);
    }

    #[test]
    fn lifecycle_untracked_still_none() {
        let fields = [
            ("MESSAGE_ID", ID_STARTED.as_bytes()),
            ("UNIT", b"session-71580.scope" as &[u8]),
            ("MESSAGE", b"Started" as &[u8]),
        ];
        let v = fields_to_value(fields, None, None);
        assert!(parse_entry(&v).is_none());
    }

    #[test]
    fn is_native_spec_shapes() {
        assert!(is_native_spec("native"));
        assert!(is_native_spec("native:/tmp/j"));
        assert!(!is_native_spec("journalctl"));
        assert!(!is_native_spec("replay:/tmp/x"));
    }
}
