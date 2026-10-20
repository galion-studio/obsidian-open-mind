// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Spend tracker: USD per session (parent + its subagents), USD and call
//! count per Europe/Zagreb day, persisted to a small JSON file. Checked
//! before every provider call; recorded after. Free (`:free`) models cost 0
//! but still count as calls.

use crate::config::SpendCfg;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Default, Clone)]
struct State {
    day: String,
    day_usd: f64,
    day_calls: u64,
    sessions: BTreeMap<String, f64>,
    models: BTreeMap<String, f64>,
}

pub struct SpendTracker {
    cfg: SpendCfg,
    path: Option<PathBuf>,
    state: Mutex<State>,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpendBlock {
    Session { spent: f64, cap: f64 },
    Daily { spent: f64, cap: f64 },
    Calls { n: u64, cap: u64 },
}

impl SpendBlock {
    pub fn message(&self) -> String {
        match self {
            SpendBlock::Session { spent, cap } => {
                format!("session spend cap reached (${spent:.4} of ${cap:.2})")
            }
            SpendBlock::Daily { spent, cap } => {
                format!("daily spend cap reached (${spent:.4} of ${cap:.2})")
            }
            SpendBlock::Calls { n, cap } => {
                format!("daily provider call cap reached ({n} of {cap})")
            }
        }
    }
}

impl SpendTracker {
    pub fn new(cfg: SpendCfg, path: Option<PathBuf>) -> Self {
        Self::with_clock(cfg, path, Box::new(crate::time::now_secs))
    }

    pub fn with_clock(
        cfg: SpendCfg,
        path: Option<PathBuf>,
        clock: Box<dyn Fn() -> i64 + Send + Sync>,
    ) -> Self {
        let mut st = State::default();
        if let Some(p) = &path {
            if let Ok(text) = std::fs::read_to_string(p) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    st.day = v["day"].as_str().unwrap_or("").to_string();
                    st.day_usd = v["day_usd"].as_f64().unwrap_or(0.0);
                    st.day_calls = v["day_calls"].as_u64().unwrap_or(0);
                    if let Some(o) = v["sessions"].as_object() {
                        for (k, x) in o {
                            st.sessions.insert(k.clone(), x.as_f64().unwrap_or(0.0));
                        }
                    }
                    if let Some(o) = v["models"].as_object() {
                        for (k, x) in o {
                            st.models.insert(k.clone(), x.as_f64().unwrap_or(0.0));
                        }
                    }
                }
            }
        }
        SpendTracker {
            cfg,
            path,
            state: Mutex::new(st),
            clock,
        }
    }

    fn roll(&self, st: &mut State) {
        let today = crate::time::zagreb_day((self.clock)());
        if st.day != today {
            st.day = today;
            st.day_usd = 0.0;
            st.day_calls = 0;
            st.models.clear();
            // session totals survive the day boundary: a session cap is per session
        }
    }

    pub fn check(&self, session: &str) -> Result<(), SpendBlock> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.roll(&mut st);
        let c = &self.cfg;
        if c.daily_call_cap > 0 && st.day_calls >= c.daily_call_cap {
            return Err(SpendBlock::Calls {
                n: st.day_calls,
                cap: c.daily_call_cap,
            });
        }
        if c.daily_usd_cap > 0.0 && st.day_usd >= c.daily_usd_cap {
            return Err(SpendBlock::Daily {
                spent: st.day_usd,
                cap: c.daily_usd_cap,
            });
        }
        let s = st.sessions.get(session).copied().unwrap_or(0.0);
        if c.session_usd_cap > 0.0 && s >= c.session_usd_cap {
            return Err(SpendBlock::Session {
                spent: s,
                cap: c.session_usd_cap,
            });
        }
        Ok(())
    }

    pub fn record(&self, session: &str, model: &str, usd: f64) {
        let usd = if model.ends_with(":free") || !usd.is_finite() || usd < 0.0 {
            0.0
        } else {
            usd
        };
        let snapshot = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            self.roll(&mut st);
            st.day_calls += 1;
            st.day_usd += usd;
            *st.sessions.entry(session.to_string()).or_default() += usd;
            *st.models.entry(model.to_string()).or_default() += usd;
            if st.sessions.len() > 2000 {
                // keep the file small on phones: drop the cheapest sessions
                let mut v: Vec<(String, f64)> =
                    st.sessions.iter().map(|(k, x)| (k.clone(), *x)).collect();
                v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                for (k, _) in v.into_iter().take(500) {
                    st.sessions.remove(&k);
                }
            }
            st.clone()
        };
        self.persist(&snapshot);
    }

    pub fn session_usd(&self, session: &str) -> f64 {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.sessions.get(session).copied().unwrap_or(0.0)
    }

    pub fn status(&self) -> Value {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.roll(&mut st);
        json!({
            "day": st.day, "day_usd": st.day_usd, "day_calls": st.day_calls,
            "caps": {"session_usd": self.cfg.session_usd_cap, "daily_usd": self.cfg.daily_usd_cap, "daily_calls": self.cfg.daily_call_cap},
            "models": st.models,
        })
    }

    fn persist(&self, st: &State) {
        let Some(p) = &self.path else { return };
        let v = json!({"day": st.day, "day_usd": st.day_usd, "day_calls": st.day_calls, "sessions": st.sessions, "models": st.models});
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    #[test]
    fn caps_and_day_roll() {
        let now = Arc::new(AtomicI64::new(
            crate::time::parse_rfc3339("2026-10-18T12:00:00+02:00").unwrap(),
        ));
        let n2 = now.clone();
        let t = SpendTracker::with_clock(
            SpendCfg {
                session_usd_cap: 0.10,
                daily_usd_cap: 0.15,
                daily_call_cap: 10,
            },
            None,
            Box::new(move || n2.load(Ordering::SeqCst)),
        );
        assert!(t.check("a").is_ok());
        t.record("a", "anthropic/claude-opus-5.5", 0.11);
        assert!(matches!(t.check("a"), Err(SpendBlock::Session { .. })));
        assert!(t.check("b").is_ok());
        t.record("b", "x", 0.05);
        assert!(matches!(t.check("b"), Err(SpendBlock::Daily { .. })));
        t.record("c", "google/gemma:free", 5.0);
        assert_eq!(t.session_usd("c"), 0.0);
        // next Zagreb day: daily resets, session cap stays
        now.fetch_add(86_400, Ordering::SeqCst);
        assert!(t.check("b").is_ok());
        assert!(t.check("a").is_err());
    }

    #[test]
    fn call_cap() {
        let t = SpendTracker::new(
            SpendCfg {
                session_usd_cap: 0.0,
                daily_usd_cap: 0.0,
                daily_call_cap: 2,
            },
            None,
        );
        t.record("s", "m", 0.0);
        t.record("s", "m", 0.0);
        assert!(matches!(
            t.check("s"),
            Err(SpendBlock::Calls { n: 2, cap: 2 })
        ));
    }
}
