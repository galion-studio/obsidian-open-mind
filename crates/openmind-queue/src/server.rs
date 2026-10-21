// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The POSEIDON queue service: the engine plus the durable store behind a
//! small loopback HTTP API. One mutex guards the state; every change is
//! written and synced to the log before it is applied and answered.
//!
//! Threads: one acceptor, one short-lived thread per connection (capped),
//! and one ticker that expires leases and wakes long-polling `lease` calls.
//! The ticker sleeps on a condvar until the next deadline (at most 1 s), so
//! an idle server uses no CPU.

use crate::auth::{self, Policy, Verdict};
use crate::engine::{Config, Engine, Job, JobSpec, Miss, Record, Refusal, GLOBAL};
use crate::refusals::{Refusals, Sidecar, SidecarReport};
use crate::store::{OpenReport, Store};
use crate::vram::{Device, ProviderView, VramPool};
use crate::{gaia, http, now_ms, zagreb_ms};
use openmind_provider::{Approval, Registry};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Longest a `lease` call may wait for work.
pub const MAX_WAIT_MS: u64 = 30_000;

#[derive(Clone, Debug)]
pub struct Options {
    pub bind: String,
    pub data: PathBuf,
    pub cfg: Config,
    /// Compact once the log has this many lines and 4x more than live state.
    pub compact_min: u64,
    /// An `openmind-provider` registry JSON whose approvals gate devices.
    pub providers: Option<PathBuf>,
    pub allow_remote: bool,
    pub max_conns: usize,
    /// Shared-secret file (mode 0600/0400, owned by this user). Required for
    /// a non-loopback bind; when set, non-loopback peers must send
    /// `Authorization: Bearer <secret>`.
    pub token_file: Option<PathBuf>,
    /// Also require the token from loopback peers (default: no, so local
    /// clients behave exactly as before).
    pub token_on_loopback: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            bind: "127.0.0.1:19120".into(),
            data: PathBuf::from("poseidon-data"),
            cfg: Config::default(),
            compact_min: 10_000,
            providers: None,
            allow_remote: false,
            max_conns: 256,
            token_file: None,
            token_on_loopback: false,
        }
    }
}

type ProviderRows = Vec<ProviderView>;

struct State {
    engine: Engine,
    store: Store,
    opened: OpenReport,
    started_ms: u64,
    providers: ProviderRows,
    compactions: u64,
    /// Refusal counters across `kill -9` (see [`crate::refusals`]).
    sidecar: Sidecar,
    sidecar_opened: SidecarReport,
}

struct Shared {
    st: Mutex<State>,
    cv: Condvar,
    stop: AtomicBool,
    conns: AtomicUsize,
    opts: Options,
    auth: Policy,
    /// Per-verdict counters, in AUTH_ORDER order.
    auth_counts: [AtomicU64; 5],
}

const AUTH_ORDER: [Verdict; 5] = [
    Verdict::Loopback,
    Verdict::Token,
    Verdict::Missing,
    Verdict::Invalid,
    Verdict::NoSecret,
];

fn auth_json(s: &Shared) -> Value {
    let mut counts = serde_json::Map::new();
    for (v, c) in AUTH_ORDER.iter().zip(s.auth_counts.iter()) {
        counts.insert(v.name().into(), json!(c.load(Ordering::Relaxed)));
    }
    json!({"mode": s.auth.mode(), "token_on_loopback": s.auth.require_on_loopback,
           "secret_configured": s.auth.secret.is_some(), "verdicts": counts})
}

pub struct Handle {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Handle {
    /// Stop accepting, stop the ticker, and wait for both.
    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.cv.notify_all();
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }

    /// Block until the server stops (used by `serve`).
    pub fn join(mut self) {
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn load_providers(path: &Option<PathBuf>) -> io::Result<ProviderRows> {
    let Some(p) = path else {
        return Ok(Vec::new());
    };
    let reg = Registry::load(p).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(reg.providers().iter().map(ProviderView::from).collect())
}

fn lock(s: &Shared) -> MutexGuard<'_, State> {
    s.st.lock().unwrap_or_else(|p| p.into_inner())
}

/// Write the refusal counters to the sidecar if they changed. Called before
/// a response is written, so a refusal the client saw is never lost to a
/// process crash. A write error is logged and counted, never fatal: the
/// counters are observability, not queue state.
fn persist_refusals(st: &mut State) {
    let c = Refusals::of(&st.engine.stats);
    if let Err(e) = st.sidecar.save(&c) {
        eprintln!(
            "{} poseidon: refusal sidecar write failed: {e}",
            zagreb_ms(now_ms())
        );
    }
}

fn commit(st: &mut State, recs: &[Record], compact_min: u64) -> io::Result<()> {
    st.store.append(recs)?;
    for r in recs {
        st.engine.apply(r);
    }
    let live = st.engine.live_jobs() as u64 + 16;
    if st.store.lines() >= compact_min && st.store.lines() > live * 4 {
        let snap = st.engine.snapshot();
        st.store.compact(&snap)?;
        st.compactions += 1;
    }
    Ok(())
}

/// Open the store, replay it, bind and start serving.
pub fn start(opts: Options) -> io::Result<Handle> {
    let addr = opts
        .bind
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bind did not resolve"))?;
    if !addr.ip().is_loopback() && !opts.allow_remote {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to bind non-loopback {addr} without --allow-remote"),
        ));
    }
    let secret = match &opts.token_file {
        Some(p) => Some(auth::load_secret_file(p)?),
        None => None,
    };
    if !auth::is_loopback(addr.ip()) && secret.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to bind non-loopback {addr} without --token-file (no unauthenticated LAN bind)"),
        ));
    }
    if opts.token_on_loopback && secret.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--token-on-loopback needs --token-file",
        ));
    }
    let auth = Policy {
        secret,
        require_on_loopback: opts.token_on_loopback,
    };
    let (store, recs, opened) = Store::open(&opts.data)?;
    let mut engine = Engine::new(opts.cfg.clone());
    for r in &recs {
        engine.apply(r);
    }
    let (sidecar, sidecar_opened) = Sidecar::open(&opts.data)?;
    if let Some(c) = &sidecar_opened.restored {
        c.restore_into(&mut engine.stats);
    }
    if sidecar_opened.bad_slots > 0 {
        eprintln!(
            "{} poseidon: {} damaged slot(s) in {}; restored seq {}",
            zagreb_ms(now_ms()),
            sidecar_opened.bad_slots,
            sidecar.file_path().display(),
            sidecar_opened.seq
        );
    }
    let providers = load_providers(&opts.providers)?;
    let mut st = State {
        engine,
        store,
        opened,
        started_ms: now_ms(),
        providers,
        compactions: 0,
        sidecar,
        sidecar_opened,
    };
    // Carry counters that only the log held (an older data directory whose
    // last compaction wrote them) into the sidecar right away.
    persist_refusals(&mut st);
    let sync: Vec<Record> = st
        .engine
        .vram
        .sync_providers(&st.providers)
        .into_iter()
        .map(Record::Device)
        .collect();
    commit(&mut st, &sync, opts.compact_min)?;

    let listener = TcpListener::bind(addr)?;
    let addr = listener.local_addr()?;
    let shared = Arc::new(Shared {
        st: Mutex::new(st),
        cv: Condvar::new(),
        stop: AtomicBool::new(false),
        conns: AtomicUsize::new(0),
        opts,
        auth,
        auth_counts: Default::default(),
    });
    let s1 = shared.clone();
    let ticker = thread::Builder::new()
        .name("poseidon-ticker".into())
        .spawn(move || ticker(s1))?;
    let s2 = shared.clone();
    let acceptor = thread::Builder::new()
        .name("poseidon-accept".into())
        .spawn(move || accept(listener, s2))?;
    Ok(Handle {
        addr,
        shared,
        threads: vec![acceptor, ticker],
    })
}

fn ticker(s: Arc<Shared>) {
    let mut g = lock(&s);
    while !s.stop.load(Ordering::SeqCst) {
        let now = now_ms();
        let due = g.engine.expire_due(now);
        if !due.is_empty() {
            let cm = s.opts.compact_min;
            if let Err(e) = commit(&mut g, &due, cm) {
                eprintln!("{} poseidon: expiry write failed: {e}", zagreb_ms(now));
            }
        }
        // Power-loss window for the refusal counters: at most one tick.
        if let Err(e) = g.sidecar.sync() {
            eprintln!(
                "{} poseidon: refusal sidecar sync failed: {e}",
                zagreb_ms(now)
            );
        }
        let before = g.engine.depth().0;
        g.engine.promote(now);
        if !due.is_empty() || g.engine.depth().0 != before {
            s.cv.notify_all();
        }
        let wait = g
            .engine
            .next_wake()
            .map_or(1000, |w| w.saturating_sub(now).clamp(5, 1000));
        g =
            s.cv.wait_timeout(g, Duration::from_millis(wait))
                .unwrap_or_else(|p| p.into_inner())
                .0;
    }
}

fn accept(listener: TcpListener, s: Arc<Shared>) {
    for conn in listener.incoming() {
        if s.stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = conn else { continue };
        if s.conns.load(Ordering::SeqCst) >= s.opts.max_conns {
            let _ = http::write_response(&stream, 503, r#"{"error":"busy"}"#);
            continue;
        }
        s.conns.fetch_add(1, Ordering::SeqCst);
        let sc = s.clone();
        let spawned = thread::Builder::new()
            .name("poseidon-conn".into())
            .spawn(move || {
                handle_conn(stream, &sc);
                sc.conns.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            s.conns.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn handle_conn(stream: TcpStream, s: &Shared) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let peer = stream.peer_addr().ok().map(|a| a.ip());
    let (code, body) = match http::read_request(&stream) {
        Ok(req) => {
            let v = s.auth.check(peer, req.authorization.as_deref());
            if let Some(i) = AUTH_ORDER.iter().position(|x| *x == v) {
                s.auth_counts[i].fetch_add(1, Ordering::Relaxed);
            }
            if v.allowed() {
                route(s, &req)
            } else {
                (
                    401,
                    json!({"error":"unauthorized","detail": v.name(),
                           "hint":"non-loopback requests need Authorization: Bearer <token>"}),
                )
            }
        }
        Err(e) => (400, json!({"error":"bad_request","detail":e.to_string()})),
    };
    persist_refusals(&mut lock(s));
    let _ = http::write_response(&stream, code, &body.to_string());
}

fn refusal(r: &Refusal) -> (u16, Value) {
    (r.status(), json!({"error": r.code(), "detail": r.detail()}))
}

fn io_err(e: io::Error) -> (u16, Value) {
    (
        500,
        json!({"error":"store_write_failed","detail": e.to_string()}),
    )
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, (u16, Value)> {
    let b = if body.is_empty() {
        b"{}" as &[u8]
    } else {
        body
    };
    serde_json::from_slice(b)
        .map_err(|e| (400, json!({"error":"bad_request","detail": e.to_string()})))
}

fn job_json(j: &Job) -> Value {
    let mut v = serde_json::to_value(j).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("created".into(), json!(zagreb_ms(j.created_ms)));
        if j.state == crate::engine::JobState::Delayed {
            m.insert("ready_at_local".into(), json!(zagreb_ms(j.ready_at)));
        }
        if j.state == crate::engine::JobState::Leased {
            m.insert("deadline_local".into(), json!(zagreb_ms(j.deadline)));
        }
    }
    v
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseReq {
    #[serde(default = "default_queue")]
    queue: String,
    worker: String,
    #[serde(default)]
    lease_ms: u64,
    #[serde(default)]
    wait_ms: u64,
}

fn default_queue() -> String {
    "default".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenReq {
    token: u64,
    #[serde(default)]
    cost: Option<u64>,
    #[serde(default)]
    error: String,
    #[serde(default)]
    lease_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapReq {
    scope: String,
    cap: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuotaReq {
    agent: String,
    max_pending: u32,
    max_inflight: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchReq {
    system: String,
    verb: String,
    #[serde(default)]
    payload: String,
}

fn miss_name(m: Miss) -> &'static str {
    match m {
        Miss::Empty => "empty",
        Miss::BudgetStopped => "budget_hard_stop",
        Miss::QuotaHeld => "quota_inflight",
        Miss::VramHeld => "vram_held",
    }
}

fn stats_json(st: &State) -> Value {
    let e = &st.engine;
    let (ready, delayed, leased, dead) = e.depth();
    let agents: serde_json::Map<String, Value> = e
        .agents()
        .iter()
        .map(|(n, a)| {
            (
                n.clone(),
                json!({
                    "pending": a.pending, "inflight": a.inflight,
                    "quota": e.quota_of(n),
                    "cap": a.budget.cap, "spent": a.budget.spent,
                    "reserved": a.budget.reserved, "remaining": a.budget.remaining(),
                    "budget_blocked": a.budget_blocked,
                }),
            )
        })
        .collect();
    json!({
        "function": gaia::FUNCTION,
        "time": zagreb_ms(now_ms()),
        "started": zagreb_ms(st.started_ms),
        "depth": {"ready": ready, "delayed": delayed, "leased": leased, "dead": dead},
        "stats": e.stats,
        "global": {"cap": e.global().cap, "spent": e.global().spent, "reserved": e.global().reserved},
        "agents": agents,
        "vram": e.vram.summary(),
        "store": {"lines": st.store.lines(), "replayed": st.opened.records,
                  "truncated_bytes": st.opened.truncated_bytes, "compactions": st.compactions},
        "refusals_sidecar": {"file": crate::refusals::FILE, "seq": st.sidecar.seq(),
                  "restored_seq": st.sidecar_opened.seq, "restored": st.sidecar_opened.restored.is_some(),
                  "fresh": st.sidecar_opened.fresh, "bad_slots": st.sidecar_opened.bad_slots,
                  "writes": st.sidecar.writes, "write_errors": st.sidecar.write_errors,
                  "syncs": st.sidecar.syncs},
        "load": gaia::load(e),
    })
}

fn route(s: &Shared, req: &http::Request) -> (u16, Value) {
    let cm = s.opts.compact_min;
    let segs: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    let m = req.method.as_str();
    let now = now_ms();
    match (m, segs.as_slice()) {
        ("GET", ["healthz"]) => (
            200,
            json!({"ok": true, "function": gaia::FUNCTION, "service": "openmind-queue",
                   "version": env!("CARGO_PKG_VERSION"), "time": zagreb_ms(now)}),
        ),
        ("GET", ["stats"]) => {
            let mut v = stats_json(&lock(s));
            v["auth"] = auth_json(s);
            (200, v)
        }
        ("GET", ["gaia", "status"]) => {
            let g = lock(s);
            let load = gaia::load(&g.engine);
            let systems: Vec<Value> = gaia::SYSTEMS
                .iter()
                .map(|(n, w)| json!({"system": n, "what": w}))
                .collect();
            (
                200,
                json!({"function": gaia::FUNCTION, "crate": gaia::REGISTRY_ROW.0,
                       "health": {"integrity": 100, "load": load}, "systems": systems,
                       "time": zagreb_ms(now)}),
            )
        }
        ("POST", ["gaia", "dispatch"]) => match parse::<DispatchReq>(&req.body) {
            Err(e) => e,
            Ok(d) => {
                let g = lock(s);
                let r = gaia::dispatch(&g.engine, &d.system, &d.verb, &d.payload);
                (200, serde_json::to_value(r).unwrap_or(Value::Null))
            }
        },
        ("POST", ["jobs"]) => match parse::<JobSpec>(&req.body) {
            Err(e) => e,
            Ok(spec) => {
                let mut g = lock(s);
                match g.engine.enqueue(spec, now) {
                    Err(r) => refusal(&r),
                    Ok(rec) => {
                        let id = match &rec {
                            Record::Enqueue { id, .. } => *id,
                            _ => 0,
                        };
                        if let Err(e) = commit(&mut g, &[rec], cm) {
                            return io_err(e);
                        }
                        s.cv.notify_all();
                        (201, json!({"id": id, "time": zagreb_ms(now)}))
                    }
                }
            }
        },
        ("GET", ["jobs", id]) => {
            let Ok(id) = id.parse::<u64>() else {
                return (400, json!({"error":"bad_request","detail":"id"}));
            };
            match lock(s).engine.job(id) {
                Some(j) => (200, job_json(j)),
                None => (
                    404,
                    json!({"error":"not_found","detail":"no such live job (acked jobs are not kept)"}),
                ),
            }
        }
        ("POST", ["jobs", id, action]) => {
            let Ok(id) = id.parse::<u64>() else {
                return (400, json!({"error":"bad_request","detail":"id"}));
            };
            if *action == "redrive" {
                let mut g = lock(s);
                return match g.engine.redrive(id, now) {
                    Err(r) => refusal(&r),
                    Ok(rec) => {
                        if let Err(e) = commit(&mut g, &[rec], cm) {
                            return io_err(e);
                        }
                        s.cv.notify_all();
                        (200, json!({"id": id, "state": "ready"}))
                    }
                };
            }
            let t = match parse::<TokenReq>(&req.body) {
                Ok(t) => t,
                Err(e) => return e,
            };
            let mut g = lock(s);
            let decided = match *action {
                "ack" => g.engine.ack(id, t.token, t.cost),
                "nack" => g.engine.nack(id, t.token, t.cost, &t.error, now),
                "extend" => g.engine.extend(id, t.token, now, t.lease_ms),
                _ => return (404, json!({"error":"not_found","detail":"unknown action"})),
            };
            match decided {
                Err(r) => refusal(&r),
                Ok(rec) => {
                    let out = match &rec {
                        Record::Ack { charge, .. } => {
                            json!({"id": id, "acked": true, "charged": charge})
                        }
                        Record::Fail {
                            retry_at, charge, ..
                        } => json!({
                            "id": id, "charged": charge,
                            "state": if retry_at.is_some() {"delayed"} else {"dead"},
                            "retry_at": retry_at.map(zagreb_ms),
                        }),
                        Record::Extend { deadline, .. } => {
                            json!({"id": id, "deadline": zagreb_ms(*deadline)})
                        }
                        _ => json!({"id": id}),
                    };
                    if let Err(e) = commit(&mut g, &[rec], cm) {
                        return io_err(e);
                    }
                    s.cv.notify_all();
                    (200, out)
                }
            }
        }
        ("POST", ["lease"]) => match parse::<LeaseReq>(&req.body) {
            Err(e) => e,
            Ok(l) => lease(s, l),
        },
        ("GET", ["dlq"]) => {
            let g = lock(s);
            let v: Vec<Value> = g.engine.dead_jobs().into_iter().map(job_json).collect();
            (200, json!({"dead": v}))
        }
        ("POST", ["caps"]) => match parse::<CapReq>(&req.body) {
            Err(e) => e,
            Ok(c) => {
                if c.scope != GLOBAL && c.scope.is_empty() {
                    return (400, json!({"error":"bad_request","detail":"scope"}));
                }
                let mut g = lock(s);
                let rec = Record::Cap {
                    scope: c.scope.clone(),
                    cap: c.cap,
                };
                if let Err(e) = commit(&mut g, &[rec], cm) {
                    return io_err(e);
                }
                s.cv.notify_all();
                (
                    200,
                    json!({"scope": c.scope, "cap": c.cap, "note": "numbers only; nothing is charged"}),
                )
            }
        },
        ("POST", ["quotas"]) => match parse::<QuotaReq>(&req.body) {
            Err(e) => e,
            Ok(q) => {
                let mut g = lock(s);
                let rec = Record::Quota {
                    agent: q.agent.clone(),
                    max_pending: q.max_pending,
                    max_inflight: q.max_inflight,
                };
                if let Err(e) = commit(&mut g, &[rec], cm) {
                    return io_err(e);
                }
                s.cv.notify_all();
                (
                    200,
                    json!({"agent": q.agent, "max_pending": q.max_pending, "max_inflight": q.max_inflight}),
                )
            }
        },
        ("GET", ["vram"]) => {
            let g = lock(s);
            let devs: Vec<Value> = g
                .engine
                .vram
                .devices()
                .map(|d| {
                    json!({"device": d, "used_gb": g.engine.vram.used(&d.id),
                           "free_gb": g.engine.vram.free(&d.id), "routable": d.routable()})
                })
                .collect();
            (
                200,
                json!({"summary": g.engine.vram.summary(), "devices": devs,
                         "note": "model only: registering a device rents and spends nothing"}),
            )
        }
        ("POST", ["vram", "devices"]) => match parse::<Device>(&req.body) {
            Err(e) => e,
            Ok(mut d) => {
                if let Err(why) = d.validate() {
                    return (400, json!({"error":"bad_request","detail": why}));
                }
                let mut g = lock(s);
                let rows = g.providers.clone();
                let note = VramPool::gate_manual(&mut d, |p| {
                    rows.iter()
                        .any(|r| r.id == p && r.approval == Approval::Approved)
                });
                let rec = Record::Device(d.clone());
                if let Err(e) = commit(&mut g, &[rec], cm) {
                    return io_err(e);
                }
                s.cv.notify_all();
                (
                    200,
                    json!({"device": d, "routable": d.routable(), "note": note}),
                )
            }
        },
        ("DELETE", ["vram", "devices", id]) => {
            let mut g = lock(s);
            if g.engine.vram.get(id).is_none() {
                return (404, json!({"error":"not_found","detail":"no such device"}));
            }
            if g.engine.vram.used(id) > 0 {
                return (
                    409,
                    json!({"error":"in_use","detail":"device has leased jobs"}),
                );
            }
            let rec = Record::DeviceRemove { id: id.to_string() };
            if let Err(e) = commit(&mut g, &[rec], cm) {
                return io_err(e);
            }
            (200, json!({"removed": id}))
        }
        ("POST", ["vram", "sync-providers"]) => {
            let rows = match load_providers(&s.opts.providers) {
                Ok(r) => r,
                Err(e) => {
                    return (
                        500,
                        json!({"error":"provider_registry","detail": e.to_string()}),
                    )
                }
            };
            let mut g = lock(s);
            g.providers = rows;
            let recs: Vec<Record> = g
                .engine
                .vram
                .sync_providers(&g.providers)
                .into_iter()
                .map(Record::Device)
                .collect();
            let n = recs.len();
            if let Err(e) = commit(&mut g, &recs, cm) {
                return io_err(e);
            }
            s.cv.notify_all();
            (200, json!({"changed": n, "providers": g.providers.len()}))
        }
        ("POST", ["admin", "compact"]) => {
            let mut g = lock(s);
            let before = g.store.lines();
            let snap = g.engine.snapshot();
            if let Err(e) = g.store.compact(&snap) {
                return io_err(e);
            }
            g.compactions += 1;
            (
                200,
                json!({"lines_before": before, "lines_after": g.store.lines()}),
            )
        }
        (_, ["healthz" | "stats" | "jobs" | "lease" | "dlq" | "caps" | "quotas" | "vram", ..]) => {
            (405, json!({"error":"method_not_allowed"}))
        }
        _ => (404, json!({"error":"not_found","detail": req.path})),
    }
}

fn lease(s: &Shared, l: LeaseReq) -> (u16, Value) {
    if l.worker.is_empty() || l.worker.len() > 64 {
        return (
            400,
            json!({"error":"bad_request","detail":"worker must be 1-64 chars"}),
        );
    }
    let wait = l.wait_ms.min(MAX_WAIT_MS);
    let start = now_ms();
    let mut g = lock(s);
    loop {
        let now = now_ms();
        match g.engine.lease(&l.queue, &l.worker, now, l.lease_ms) {
            Ok(rec) => {
                let (id, token, device) = match &rec {
                    Record::Lease {
                        id, token, device, ..
                    } => (*id, *token, device.clone()),
                    _ => (0, 0, String::new()),
                };
                if let Err(e) = commit(&mut g, &[rec], s.opts.compact_min) {
                    return io_err(e);
                }
                let job = g.engine.job(id).map(job_json).unwrap_or(Value::Null);
                return (
                    200,
                    json!({"id": id, "token": token, "device": device, "job": job}),
                );
            }
            Err(miss) => {
                let waited = now.saturating_sub(start);
                if waited >= wait || s.stop.load(Ordering::SeqCst) {
                    return (200, json!({"id": null, "miss": miss_name(miss)}));
                }
                let left = (wait - waited).clamp(1, 1000);
                g =
                    s.cv.wait_timeout(g, Duration::from_millis(left))
                        .unwrap_or_else(|p| p.into_inner())
                        .0;
            }
        }
    }
}
