// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Where this process listens and which addresses its own probes dial.
// Ported from VPS commit 2f5a730 ("ONE-PROBE: spare One probes its own bind").
//
// What was wrong: a spare process started with `ONE_BIND=127.0.0.1:18091` and
// `ONE_MOUTH_BIND=127.0.0.1:18002` still dialled the live `127.0.0.1:8091`
// for its `one` health pulse and the live mouth on port 8002 for `voice` and
// for `/synthesize`. `ONE_MOUTH_BIND` moved only the listen socket.
//
// Behaviour now (same as the VPS fix, without the VPS-only config file):
//   * the listeners bind `ONE_BIND` (default `127.0.0.1:8091`) and
//     `ONE_MOUTH_BIND` (default `127.0.0.1:8002`), as before;
//   * the `one` self probe dials the `ONE_BIND` address;
//   * the mouth client dials `GALION_MOUTH` when it parses, otherwise the
//     mouth listen address this process was told to use;
//   * the mouth counts as local (synthesise in-process, never POST our own
//     `/synthesize`) when it is loopback on port 8002 (today's rule) or when
//     it is exactly our own mouth listen address.
// Extra over the VPS version: `GALION_MEMORY` moves the memory-gateway
// probe off the live 8420, and a wildcard bind (`0.0.0.0` / `::`) is dialled
// on loopback, so the probe never targets the unspecified address.
//
// Everything takes an env lookup closure, so tests never touch the process
// environment, never listen and never connect.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

pub const DEFAULT_BIND: &str = "127.0.0.1:8091";
pub const DEFAULT_MOUTH_BIND: &str = "127.0.0.1:8002";
/// Today's mouth port. A loopback mouth on this port is always "local".
pub const MOUTH_PORT: u16 = 8002;
pub const DEFAULT_MEMORY: &str = "127.0.0.1:8420";

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The process environment as an [`Env`].
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Listen address: the named variable trimmed (blank counts as unset),
/// otherwise `default`. Handed to `TcpListener::bind` unchanged.
pub fn listen_addr(env: Env, name: &str, default: &str) -> String {
    match env(name) {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => default.to_string(),
    }
}

/// `ONE_BIND` listen address.
pub fn one_bind(env: Env) -> String {
    listen_addr(env, "ONE_BIND", DEFAULT_BIND)
}

/// `ONE_MOUTH_BIND` listen address.
pub fn mouth_bind(env: Env) -> String {
    listen_addr(env, "ONE_MOUTH_BIND", DEFAULT_MOUTH_BIND)
}

/// Resolve `host:port` to the first socket address. Blank or unparsable
/// gives `None`. A wildcard IP is mapped to loopback of the same family.
pub fn dial_addr(raw: &str) -> Option<SocketAddr> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut addr = raw.to_socket_addrs().ok()?.next()?;
    if addr.ip().is_unspecified() {
        addr.set_ip(match addr.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        });
    }
    Some(addr)
}

fn default_addr(raw: &str) -> SocketAddr {
    raw.parse().expect("built-in default address parses")
}

/// The `one` self probe dials the address this process listens on.
pub fn one_probe_target(env: Env) -> SocketAddr {
    dial_addr(&one_bind(env)).unwrap_or_else(|| default_addr(DEFAULT_BIND))
}

/// Mouth client: `GALION_MOUTH` when it parses, otherwise our own mouth
/// listen address, otherwise today's `127.0.0.1:8002`.
pub fn mouth_client_target(env: Env) -> SocketAddr {
    if let Some(addr) = env("GALION_MOUTH").as_deref().and_then(dial_addr) {
        return addr;
    }
    dial_addr(&mouth_bind(env)).unwrap_or_else(|| default_addr(DEFAULT_MOUTH_BIND))
}

/// True when `addr` is somebody else's mouth (POST its `/synthesize`).
/// False for today's loopback :8002 and for our own mouth listen address.
/// Memory gateway probe target: `GALION_MEMORY` (`host:port`) when it parses,
/// otherwise today's `127.0.0.1:8420`. Lets a spare One read a fixture
/// memory gateway instead of the live one.
pub fn memory_target(env: Env) -> SocketAddr {
    env("GALION_MEMORY")
        .as_deref()
        .and_then(dial_addr)
        .unwrap_or_else(|| default_addr(DEFAULT_MEMORY))
}

pub fn mouth_is_remote(addr: SocketAddr, env: Env) -> bool {
    if addr.ip().is_loopback() && addr.port() == MOUTH_PORT {
        return false;
    }
    match dial_addr(&mouth_bind(env)) {
        Some(local) => addr != local,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    fn sa(raw: &str) -> SocketAddr {
        raw.parse().unwrap()
    }

    #[test]
    fn defaults_are_todays_live_addresses() {
        let env = env_from(&[]);
        assert_eq!(one_bind(&env), "127.0.0.1:8091");
        assert_eq!(mouth_bind(&env), "127.0.0.1:8002");
        assert_eq!(one_probe_target(&env), sa("127.0.0.1:8091"));
        assert_eq!(mouth_client_target(&env), sa("127.0.0.1:8002"));
        assert!(!mouth_is_remote(mouth_client_target(&env), &env));
    }

    #[test]
    fn probe_and_mouth_client_follow_forced_bind_and_mouth() {
        // The VPS 2f5a730 test, on the env-only settings. Parses only.
        let env = env_from(&[
            ("ONE_BIND", "127.0.0.1:18091"),
            ("ONE_MOUTH_BIND", "127.0.0.1:18002"),
        ]);
        let probe = one_probe_target(&env);
        let mouth = mouth_client_target(&env);
        assert_eq!(probe, sa("127.0.0.1:18091"));
        assert_eq!(mouth, sa("127.0.0.1:18002"));
        assert_ne!(probe.port(), 8091);
        assert_ne!(mouth.port(), 8002);
        assert!(!mouth_is_remote(mouth, &env));
    }

    #[test]
    fn blank_values_count_as_unset() {
        let env = env_from(&[
            ("ONE_BIND", "   "),
            ("ONE_MOUTH_BIND", ""),
            ("GALION_MOUTH", " "),
        ]);
        assert_eq!(one_probe_target(&env), sa("127.0.0.1:8091"));
        assert_eq!(mouth_client_target(&env), sa("127.0.0.1:8002"));
    }

    #[test]
    fn galion_mouth_wins_over_the_mouth_bind_and_is_remote() {
        let env = env_from(&[
            ("ONE_MOUTH_BIND", "127.0.0.1:18002"),
            ("GALION_MOUTH", "10.0.0.7:8002"),
        ]);
        let mouth = mouth_client_target(&env);
        assert_eq!(mouth, sa("10.0.0.7:8002"));
        assert!(mouth_is_remote(mouth, &env));
    }

    #[test]
    fn unparsable_galion_mouth_falls_back_to_own_mouth_bind() {
        let env = env_from(&[
            ("ONE_MOUTH_BIND", "127.0.0.1:18002"),
            ("GALION_MOUTH", "not an address"),
        ]);
        assert_eq!(mouth_client_target(&env), sa("127.0.0.1:18002"));
    }

    #[test]
    fn galion_mouth_on_loopback_8002_stays_local() {
        // Today's rule, unchanged: loopback :8002 is our own mouth.
        let env = env_from(&[("GALION_MOUTH", "127.0.0.1:8002")]);
        assert!(!mouth_is_remote(mouth_client_target(&env), &env));
    }

    #[test]
    fn galion_mouth_on_another_loopback_port_is_remote() {
        // A second local mouth on another port is a different process.
        let env = env_from(&[("GALION_MOUTH", "127.0.0.1:19002")]);
        assert!(mouth_is_remote(mouth_client_target(&env), &env));
    }

    #[test]
    fn wildcard_binds_are_dialled_on_loopback() {
        let env = env_from(&[
            ("ONE_BIND", "0.0.0.0:19050"),
            ("ONE_MOUTH_BIND", "[::]:19051"),
        ]);
        assert_eq!(one_probe_target(&env), sa("127.0.0.1:19050"));
        let mouth = mouth_client_target(&env);
        assert_eq!(mouth, sa("[::1]:19051"));
        assert!(!mouth_is_remote(mouth, &env));
    }

    #[test]
    fn memory_target_defaults_to_live_port_and_follows_galion_memory() {
        assert_eq!(memory_target(&env_from(&[])), sa("127.0.0.1:8420"));
        let moved = env_from(&[("GALION_MEMORY", "127.0.0.1:19052")]);
        assert_eq!(memory_target(&moved), sa("127.0.0.1:19052"));
        let junk = env_from(&[("GALION_MEMORY", "junk")]);
        assert_eq!(memory_target(&junk), sa("127.0.0.1:8420"));
    }

    #[test]
    fn unparsable_bind_probes_the_default() {
        // The listener would fail to bind and exit; the probe never panics.
        let env = env_from(&[("ONE_BIND", "nope")]);
        assert_eq!(one_bind(&env), "nope");
        assert_eq!(one_probe_target(&env), sa("127.0.0.1:8091"));
    }
}
