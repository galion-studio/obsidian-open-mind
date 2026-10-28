// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! pidfd source: one parked thread per tracked main process (stack services
//! and cloudflared, from the inventory), blocked in poll(2) on the pidfd
//! until the process exits. Independent of journald (which can rate-limit or
//! lose lines), so a dead service main process is seen even then.
//! Small stacks (64 KiB); at most 48 targets.

use super::sys::PidFd;
use super::{NerveEvent, Severity};
use serde_json::json;
use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct PidWatch {
    watching: Arc<Mutex<HashSet<u32>>>,
}

impl PidWatch {
    /// Watch new targets (pid, comm, group); already-watched pids are skipped.
    /// Returns how many were added.
    pub fn sync(
        &self,
        targets: &[(u32, String, String)],
        tx: &Sender<super::runtime::Input>,
    ) -> usize {
        let mut added = 0;
        for (pid, comm, group) in targets {
            {
                let mut w = self.watching.lock().unwrap_or_else(|e| e.into_inner());
                if w.contains(pid) || w.len() >= 48 {
                    continue;
                }
                w.insert(*pid);
            }
            let fd = match PidFd::open(*pid) {
                Ok(f) => f,
                Err(_) => {
                    self.watching
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(pid);
                    continue;
                }
            };
            let (tx, w, comm, group, pid) = (
                tx.clone(),
                self.watching.clone(),
                comm.clone(),
                group.clone(),
                *pid,
            );
            let ok = std::thread::Builder::new()
                .name(format!("nerve-evt-pid{pid}"))
                .stack_size(64 * 1024)
                .spawn(move || {
                    let r = fd.wait_exit();
                    w.lock().unwrap_or_else(|e| e.into_inner()).remove(&pid);
                    if r.is_ok() {
                        let ev = NerveEvent::new(
                            "pidfd",
                            "proc.exit",
                            Severity::Medium,
                            format!("{comm} (pid {pid}) exited"),
                        )
                        .group(&group)
                        .key(format!("proc.exit:{comm}"))
                        .data(json!({"pid": pid, "comm": comm}));
                        let _ = tx.send(super::runtime::Input::Pid(ev));
                    }
                })
                .is_ok();
            if ok {
                added += 1;
            }
        }
        added
    }

    pub fn count(&self) -> usize {
        self.watching
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}
