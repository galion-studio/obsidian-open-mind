// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Load control for the gateway (std only):
//!
//! * [`Limits`]: knobs from env (`HERMES_RS_MAX_CONNS`, `HERMES_RS_QUEUE_*`,
//!   `HERMES_RS_RUN_TIMEOUT_S`, `HERMES_RS_HEADER_TIMEOUT_S`,
//!   `HERMES_RS_SHUTDOWN_GRACE_S`, `HERMES_RS_METRICS_OPEN`).
//! * [`SlotPool`]: `max_concurrent_runs` slots with a short bounded wait
//!   queue (backpressure) before the upstream-compatible 429.
//! * [`watchdog`]: per-run deadline that sets the turn's cancel flag.
//! * [`shutdown`]: SIGTERM/SIGINT flag for graceful drain.
//! * [`Metrics`]: counters + run-duration histogram for `/metrics` and `/status`.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    /// Open connections accepted at once; beyond that: 503 + Retry-After.
    pub max_conns: usize,
    /// How long a chat/run request may wait for a free run slot.
    pub queue_wait: Duration,
    /// Max requests waiting for a slot; beyond that: 429 at once.
    pub queue_max: usize,
    /// Wall-clock cap per agent run (0 = off); on expiry the run is cancelled.
    pub run_timeout: Duration,
    /// Time allowed to receive request line + headers + body.
    pub header_timeout: Duration,
    /// Write timeout for responses (SSE frames included).
    pub write_timeout: Duration,
    /// SIGTERM: stop accepting, wait this long for in-flight requests.
    pub shutdown_grace: Duration,
    /// `/metrics` and `/status` without the API key (default: key required).
    pub metrics_open: bool,
    /// curl `--max-time` for provider calls.
    pub provider_max_time_s: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_conns: 64,
            queue_wait: Duration::from_millis(5_000),
            queue_max: 16,
            run_timeout: Duration::from_secs(600),
            header_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(25),
            metrics_open: false,
            provider_max_time_s: 900,
        }
    }
}

impl Limits {
    pub fn apply_env(&mut self, get: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
        let num = |k: &str| -> Result<Option<u64>, String> {
            match get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                None => Ok(None),
                Some(v) => v
                    .parse::<u64>()
                    .map(Some)
                    .map_err(|_| format!("bad {k}: {v}")),
            }
        };
        if let Some(v) = num("HERMES_RS_MAX_CONNS")? {
            self.max_conns = (v as usize).clamp(1, 4096);
        }
        if let Some(v) = num("HERMES_RS_QUEUE_WAIT_MS")? {
            self.queue_wait = Duration::from_millis(v.min(600_000));
        }
        if let Some(v) = num("HERMES_RS_QUEUE_MAX")? {
            self.queue_max = (v as usize).min(4096);
        }
        if let Some(v) = num("HERMES_RS_RUN_TIMEOUT_S")? {
            self.run_timeout = Duration::from_secs(v);
        }
        if let Some(v) = num("HERMES_RS_HEADER_TIMEOUT_S")? {
            self.header_timeout = Duration::from_secs(v.clamp(1, 600));
        }
        if let Some(v) = num("HERMES_RS_WRITE_TIMEOUT_S")? {
            self.write_timeout = Duration::from_secs(v.clamp(1, 600));
        }
        if let Some(v) = num("HERMES_RS_SHUTDOWN_GRACE_S")? {
            self.shutdown_grace = Duration::from_secs(v.min(3600));
        }
        if let Some(v) = num("HERMES_RS_PROVIDER_MAX_TIME_S")? {
            self.provider_max_time_s = v.clamp(5, 3600);
        }
        if let Some(v) = get("HERMES_RS_METRICS_OPEN") {
            self.metrics_open = matches!(v.trim(), "1" | "true" | "yes" | "on");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- metrics

/// Upper bounds (seconds) of the run-duration histogram.
pub const RUN_BUCKETS: [f64; 12] = [
    0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];
pub const ROUTES: [&str; 7] = [
    "health", "chat", "runs", "sessions", "models", "metrics", "other",
];

#[derive(Default)]
pub struct Metrics {
    pub requests: [AtomicU64; 7],
    pub status_2xx: AtomicU64,
    pub status_4xx: AtomicU64,
    pub status_5xx: AtomicU64,
    pub rejected_conns: AtomicU64,
    pub rejected_runs: AtomicU64,
    pub queued: AtomicU64,
    pub queue_wait_us: AtomicU64,
    pub run_timeouts: AtomicU64,
    pub bad_requests: AtomicU64,
    pub runs_started: AtomicU64,
    pub runs_finished: AtomicU64,
    pub run_buckets: [AtomicU64; 13],
    pub run_us_sum: AtomicU64,
    pub conns_in_flight: AtomicUsize,
}

pub fn metrics() -> &'static Metrics {
    static M: std::sync::OnceLock<Metrics> = std::sync::OnceLock::new();
    M.get_or_init(Metrics::default)
}

pub fn route_class(path: &str) -> usize {
    let p = path.strip_prefix("/p/default").unwrap_or(path);
    if p == "/health" || p == "/v1/health" || p.starts_with("/health/") {
        0
    } else if p.starts_with("/v1/chat/") {
        1
    } else if p.starts_with("/v1/runs") {
        2
    } else if p.starts_with("/api/sessions") {
        3
    } else if p.starts_with("/v1/models") || p.starts_with("/v1/capabilities") {
        4
    } else if p == "/metrics" || p == "/status" {
        5
    } else {
        6
    }
}

impl Metrics {
    pub fn record(&self, path: &str, status: u16) {
        self.requests[route_class(path)].fetch_add(1, Ordering::Relaxed);
        match status {
            200..=299 => &self.status_2xx,
            400..=499 => &self.status_4xx,
            _ => &self.status_5xx,
        }
        .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_run(&self, d: Duration) {
        let s = d.as_secs_f64();
        let i = RUN_BUCKETS
            .iter()
            .position(|b| s <= *b)
            .unwrap_or(RUN_BUCKETS.len());
        self.run_buckets[i].fetch_add(1, Ordering::Relaxed);
        self.run_us_sum
            .fetch_add(d.as_micros() as u64, Ordering::Relaxed);
        self.runs_finished.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------- slots

/// Run slots with a bounded wait queue.
pub struct SlotPool {
    pub max: usize,
    pub active: Arc<AtomicUsize>,
    waiting: Mutex<usize>,
    cv: Condvar,
    pub queue_max: usize,
    pub wait: Duration,
}

/// Holds one agent slot; frees it (and wakes one waiter) on drop.
pub struct Slot {
    pool: Arc<SlotPool>,
    started: Instant,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.pool.active.fetch_sub(1, Ordering::SeqCst);
        metrics().record_run(self.started.elapsed());
        // Take the lock so a waiter between its check and its wait cannot miss this.
        let _g = self.pool.waiting.lock();
        self.pool.cv.notify_one();
    }
}

impl SlotPool {
    pub fn new(
        max: usize,
        active: Arc<AtomicUsize>,
        queue_max: usize,
        wait: Duration,
    ) -> Arc<Self> {
        Arc::new(SlotPool {
            max: max.max(1),
            active,
            waiting: Mutex::new(0),
            cv: Condvar::new(),
            queue_max,
            wait,
        })
    }

    fn try_take(&self) -> bool {
        let mut n = self.active.load(Ordering::SeqCst);
        while n < self.max {
            match self
                .active
                .compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(cur) => n = cur,
            }
        }
        false
    }

    pub fn waiting(&self) -> usize {
        self.waiting.lock().map(|g| *g).unwrap_or(0)
    }

    /// A slot now, or after waiting up to `wait` in a queue of at most
    /// `queue_max`; None means "answer 429".
    pub fn acquire(self: &Arc<Self>) -> Option<Slot> {
        let m = metrics();
        let slot = || {
            m.runs_started.fetch_add(1, Ordering::Relaxed);
            Slot {
                pool: self.clone(),
                started: Instant::now(),
            }
        };
        if self.try_take() {
            return Some(slot());
        }
        if self.wait.is_zero() || self.queue_max == 0 || shutdown::requested() {
            m.rejected_runs.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let t0 = Instant::now();
        let mut g = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        if *g >= self.queue_max {
            m.rejected_runs.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        *g += 1;
        m.queued.fetch_add(1, Ordering::Relaxed);
        let got = loop {
            if self.try_take() {
                break true;
            }
            let left = self.wait.saturating_sub(t0.elapsed());
            if left.is_zero() || shutdown::requested() {
                break false;
            }
            g = self
                .cv
                .wait_timeout(g, left.min(Duration::from_millis(250)))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        *g -= 1;
        drop(g);
        m.queue_wait_us
            .fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
        if got {
            Some(slot())
        } else {
            m.rejected_runs.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

// ---------------------------------------------------------------- watchdog

pub mod watchdog {
    use super::*;

    struct Armed {
        id: u64,
        at: Option<Instant>,
        cancel: Weak<AtomicBool>,
    }

    struct State {
        list: Mutex<Vec<Armed>>,
        next: AtomicU64,
    }

    fn state() -> &'static State {
        static S: std::sync::OnceLock<State> = std::sync::OnceLock::new();
        S.get_or_init(|| {
            let _ = std::thread::Builder::new()
                .name("hermes-watchdog".into())
                .stack_size(64 * 1024)
                .spawn(|| loop {
                    std::thread::sleep(Duration::from_millis(200));
                    let now = Instant::now();
                    if let Ok(mut l) = state().list.lock() {
                        l.retain(|a| {
                            if !matches!(a.at, Some(t) if t <= now) {
                                return a.cancel.strong_count() > 0;
                            }
                            if let Some(c) = a.cancel.upgrade() {
                                c.store(true, Ordering::SeqCst);
                                metrics().run_timeouts.fetch_add(1, Ordering::Relaxed);
                            }
                            false
                        });
                    }
                });
            State {
                list: Mutex::new(Vec::new()),
                next: AtomicU64::new(1),
            }
        })
    }

    /// Disarms on drop.
    pub struct Guard(u64);

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.0 == 0 {
                return;
            }
            if let Ok(mut l) = state().list.lock() {
                l.retain(|a| a.id != self.0);
            }
        }
    }

    /// Set `cancel` after `after` unless the guard is dropped first.
    /// `after == 0`: no deadline, but the run is still registered so
    /// [`fire_all`] (shutdown) can cancel it.
    pub fn arm(after: Duration, cancel: &Arc<AtomicBool>) -> Guard {
        let s = state();
        let id = s.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut l) = s.list.lock() {
            let at = (!after.is_zero()).then(|| Instant::now() + after);
            l.push(Armed {
                id,
                at,
                cancel: Arc::downgrade(cancel),
            });
        }
        Guard(id)
    }

    /// Cancel every registered run now (graceful-shutdown deadline hit).
    /// Returns how many were still alive.
    pub fn fire_all() -> usize {
        let mut n = 0;
        if let Ok(l) = state().list.lock() {
            for a in l.iter() {
                if let Some(c) = a.cancel.upgrade() {
                    c.store(true, Ordering::SeqCst);
                    n += 1;
                }
            }
        }
        n
    }

    pub fn armed() -> usize {
        state().list.lock().map(|l| l.len()).unwrap_or(0)
    }
}

// ---------------------------------------------------------------- shutdown

pub mod shutdown {
    use super::*;

    static REQUESTED: AtomicBool = AtomicBool::new(false);

    pub fn requested() -> bool {
        REQUESTED.load(Ordering::SeqCst)
    }

    pub fn request() {
        REQUESTED.store(true, Ordering::SeqCst);
    }

    #[cfg(unix)]
    mod sig {
        // libc `signal(2)`; present in glibc, musl and bionic. The handler only
        // stores to an atomic (async-signal-safe). Same numbers on every Linux arch.
        extern "C" {
            fn signal(signum: i32, handler: usize) -> usize;
        }
        extern "C" fn on_signal(_sig: i32) {
            super::REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        pub fn install() {
            const SIGINT: i32 = 2;
            const SIGTERM: i32 = 15;
            let h = on_signal as extern "C" fn(i32) as usize;
            // SAFETY: installing a handler that only touches an AtomicBool.
            unsafe {
                signal(SIGTERM, h);
                signal(SIGINT, h);
            }
        }
    }

    /// Route SIGTERM/SIGINT to the shutdown flag (unix); no-op elsewhere.
    pub fn install_handlers() {
        #[cfg(unix)]
        sig::install();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_env() {
        let mut l = Limits::default();
        let env = |k: &str| match k {
            "HERMES_RS_MAX_CONNS" => Some("8".to_string()),
            "HERMES_RS_QUEUE_WAIT_MS" => Some("0".into()),
            "HERMES_RS_RUN_TIMEOUT_S" => Some("42".into()),
            "HERMES_RS_METRICS_OPEN" => Some("1".into()),
            _ => None,
        };
        l.apply_env(&env).unwrap();
        assert_eq!(l.max_conns, 8);
        assert!(l.queue_wait.is_zero());
        assert_eq!(l.run_timeout, Duration::from_secs(42));
        assert!(l.metrics_open);
        let bad = |k: &str| (k == "HERMES_RS_QUEUE_MAX").then(|| "x".to_string());
        assert!(Limits::default().apply_env(&bad).is_err());
    }

    #[test]
    fn slots_queue_then_reject() {
        let active = Arc::new(AtomicUsize::new(0));
        let pool = SlotPool::new(1, active.clone(), 1, Duration::from_millis(300));
        let a = pool.acquire().expect("first slot");
        // Second waits; we free the first after 50 ms, so it gets the slot.
        let p2 = pool.clone();
        let h = std::thread::spawn(move || p2.acquire().is_some());
        std::thread::sleep(Duration::from_millis(50));
        drop(a);
        assert!(h.join().unwrap());
        assert_eq!(active.load(Ordering::SeqCst), 0);
        // Full queue + no free slot -> immediate None.
        let held = pool.acquire().unwrap();
        let p3 = pool.clone();
        let waiter = std::thread::spawn(move || p3.acquire().is_some());
        std::thread::sleep(Duration::from_millis(30));
        let t0 = Instant::now();
        assert!(pool.acquire().is_none(), "queue full");
        assert!(t0.elapsed() < Duration::from_millis(100));
        assert!(!waiter.join().unwrap(), "waiter times out while slot held");
        drop(held);
    }

    #[test]
    fn watchdog_cancels_and_guard_disarms() {
        let c = Arc::new(AtomicBool::new(false));
        let g = watchdog::arm(Duration::from_millis(50), &c);
        std::thread::sleep(Duration::from_millis(500));
        assert!(c.load(Ordering::SeqCst));
        drop(g);
        let c2 = Arc::new(AtomicBool::new(false));
        let g2 = watchdog::arm(Duration::from_millis(100), &c2);
        drop(g2);
        std::thread::sleep(Duration::from_millis(400));
        assert!(!c2.load(Ordering::SeqCst));
        let c3 = Arc::new(AtomicBool::new(false));
        let _g3 = watchdog::arm(Duration::ZERO, &c3);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!c3.load(Ordering::SeqCst), "no deadline");
        assert!(watchdog::fire_all() >= 1);
        assert!(c3.load(Ordering::SeqCst));
    }

    #[test]
    fn route_classes() {
        assert_eq!(ROUTES[route_class("/health")], "health");
        assert_eq!(ROUTES[route_class("/v1/chat/completions")], "chat");
        assert_eq!(ROUTES[route_class("/p/default/v1/runs/x")], "runs");
        assert_eq!(ROUTES[route_class("/metrics")], "metrics");
        assert_eq!(ROUTES[route_class("/nope")], "other");
    }
}
