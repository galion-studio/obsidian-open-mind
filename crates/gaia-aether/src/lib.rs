// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! AETHER: the atmosphere, here everything that carries signals. It owns
//! messaging, the nerve (signal lines between components), the voice relay,
//! Wi-Fi identity, LAN discovery, tunnels, the origin front door and the
//! Zagreb clock. It does no network I/O: messages go over the GAIA bus,
//! discovery only classifies addresses it is given, tunnels are plans.

use gaia_core::{
    fnv1a, scaled, Context, Directive, FunctionId, Health, Json, Outcome, Report, Route,
    SubordinateFunction, System, Verb,
};
use std::net::Ipv4Addr;

/// Voice frames: 40 ms of 48 kHz mono s16 = 1,920 samples x 2 bytes.
pub const FRAME_BYTES: u64 = 1_920 * 2;
pub const FRAMES_PER_SEC: u64 = 25;
/// Jitter buffer cap in frames (matches galion-voice-core).
pub const JITTER_FRAMES: u64 = 25;
/// Peers remembered at most.
pub const MAX_PEERS: usize = 64;

const ORIGIN_ROUTES: &[&str] = &[
    "/",
    "/health",
    "/grinder",
    "/desk/notes",
    "/link.json",
    "/splash",
];

#[derive(Debug, Default)]
pub struct Aether {
    sent: u64,
    peers: Vec<Ipv4Addr>,
    identity: Option<String>,
}

impl Aether {
    pub fn new() -> Aether {
        Aether::default()
    }

    /// Deterministic node identity from a seed (Wi-Fi / omnode name).
    pub fn identity_for(seed: &str) -> String {
        format!("galion-{:08x}", fnv1a(seed.as_bytes()) as u32)
    }

    /// Only private or link-local peers are accepted on the LAN.
    pub fn lan_peer(addr: Ipv4Addr) -> bool {
        addr.is_private() || addr.is_link_local()
    }

    pub fn origin_status(path: &str) -> u16 {
        if ORIGIN_ROUTES.contains(&path) {
            200
        } else {
            404
        }
    }
}

impl SubordinateFunction for Aether {
    fn id(&self) -> FunctionId {
        FunctionId::Aether
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::Messaging,
            System::Nerve,
            System::VoiceRelay,
            System::SpeechIo,
            System::WifiIdentity,
            System::LanDiscovery,
            System::Tunnels,
            System::Origin,
            System::ZagrebClock,
        ]
    }

    fn health(&self) -> Health {
        Health::ok((self.sent.min(100)) as u8)
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        let done = |s: String| Report::for_directive(d, Outcome::Done).summary(s);
        let status = d.verb == Verb::Status;
        match d.target {
            System::Messaging => {
                let n = if status { 0 } else { scaled(8, d.intensity) };
                let body = if d.payload.is_empty() {
                    "hello"
                } else {
                    d.payload.as_str()
                };
                for _ in 0..n {
                    ctx.send(Route::Tower("tallneck-archive".into()), "chat", body);
                }
                self.sent += n;
                done(format!("{n} message(s) relayed over tallneck-archive"))
                    .metric("sent", n as i64)
            }
            System::Nerve => {
                let n = if status { 0 } else { scaled(8, d.intensity) };
                let target = d.payload.split(':').next().and_then(FunctionId::parse);
                for _ in 0..n {
                    match target {
                        Some(f) => ctx.send(Route::Direct(f), "nerve", "pulse"),
                        None => ctx.send(Route::Gaia, "nerve", "pulse"),
                    }
                }
                self.sent += n;
                done(format!(
                    "{n} pulse(s) on the nerve to {}",
                    target.map_or("GAIA", FunctionId::name)
                ))
                .metric("pulses", n as i64)
            }
            System::VoiceRelay => {
                let secs = scaled(60, d.intensity);
                let frames = secs * FRAMES_PER_SEC;
                done(format!("{secs} s of voice = {frames} frames"))
                    .metric("frames", frames as i64)
                    .metric("bytes", (frames * FRAME_BYTES) as i64)
                    .metric("jitter_buffer_bytes", (JITTER_FRAMES * FRAME_BYTES) as i64)
            }
            System::SpeechIo => {
                // Budget only: real-time factor target < 1 means speech keeps up.
                let secs = scaled(30, d.intensity);
                done(format!("{secs} s of speech queued for STT/TTS (simulated)"))
                    .metric("audio_secs", secs as i64)
                    .metric("rtf_target_x100", 100)
            }
            System::WifiIdentity => {
                let seed = if d.payload.is_empty() {
                    "omnode"
                } else {
                    d.payload.as_str()
                };
                let id = Aether::identity_for(seed);
                self.identity = Some(id.clone());
                done(format!("node identity {id}"))
            }
            System::LanDiscovery => {
                let mut accepted = 0;
                let mut rejected = 0;
                let given: Vec<Ipv4Addr> = if d.payload.is_empty() {
                    let n = scaled(16, d.intensity) as u8;
                    (1..=n).map(|i| Ipv4Addr::new(192, 168, 1, i)).collect()
                } else {
                    d.payload
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter_map(|s| s.parse().ok())
                        .collect()
                };
                for a in given {
                    if Aether::lan_peer(a) {
                        if !self.peers.contains(&a) && self.peers.len() < MAX_PEERS {
                            self.peers.push(a);
                        }
                        accepted += 1;
                    } else {
                        rejected += 1;
                    }
                }
                done(format!(
                    "{accepted} LAN peer(s), {rejected} non-private rejected (never contacted)"
                ))
                .metric("peers", self.peers.len() as i64)
                .metric("rejected", rejected)
            }
            System::Tunnels => {
                let mut r = Report::for_directive(d, Outcome::DryRun)
                    .summary("tunnel plan only; AETHER opens no connection")
                    .plan(vec![
                        "tuner-publish starts a quick tunnel to the loopback tuner".into(),
                        "publish the tunnel URL on the nerve".into(),
                        "close the tunnel when the session ends".into(),
                    ]);
                r.dry_run = true;
                r
            }
            System::Origin => {
                let path = if d.payload.is_empty() {
                    "/health"
                } else {
                    d.payload.trim()
                };
                let code = Aether::origin_status(path);
                done(format!("origin {path} -> {code}")).metric("status", i64::from(code))
            }
            System::ZagrebClock => done(format!("sim time {}", ctx.at)),
            other => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not an AETHER system")),
        }
    }

    fn tick(&mut self, ctx: &mut Context) -> Option<Report> {
        if !ctx.env.swarm_active {
            ctx.env.air = ctx.env.air.saturating_add(4).min(100);
        }
        None
    }

    fn reset(&mut self) {
        *self = Aether::default();
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("sent", self.sent)
            .with("peers", self.peers.len())
            .with("identity", self.identity.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Aether::new().systems(), FunctionId::Aether.systems());
    }

    #[test]
    fn identity_is_deterministic() {
        assert_eq!(Aether::identity_for("pi"), Aether::identity_for("pi"));
        assert_ne!(Aether::identity_for("pi"), Aether::identity_for("phone"));
        assert!(Aether::identity_for("pi").starts_with("galion-"));
    }

    #[test]
    fn lan_discovery_rejects_public() {
        let mut a = Aether::new();
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        let public = format!("{}.{}.{}.{}", 8, 8, 4, 4);
        let d = Directive::new(1, System::LanDiscovery, Verb::Run)
            .payload(format!("10.0.0.2,{public},192.168.0.9"));
        let r = a.handle(&d, &mut ctx);
        assert_eq!(r.metric_value("peers"), Some(2));
        assert_eq!(r.metric_value("rejected"), Some(1));
    }

    #[test]
    fn messaging_goes_on_the_bus_and_tunnels_are_plans() {
        let mut a = Aether::new();
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        a.handle(
            &Directive::new(1, System::Messaging, Verb::Run).intensity(100),
            &mut ctx,
        );
        assert_eq!(ctx.outbox().len(), 8);
        let r = a.handle(&Directive::new(2, System::Tunnels, Verb::Execute), &mut ctx);
        assert!(r.dry_run);
        assert_eq!(Aether::origin_status("/health"), 200);
        assert_eq!(Aether::origin_status("/admin"), 404);
    }
}
