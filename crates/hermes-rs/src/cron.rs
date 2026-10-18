// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Cron (placeholder, filled in by the cron commit).

use serde_json::Value;

#[derive(Default)]
pub struct Scheduler {
    jobs: Vec<Value>,
}

impl Scheduler {
    pub fn list(&self) -> Vec<Value> {
        self.jobs.clone()
    }
}
