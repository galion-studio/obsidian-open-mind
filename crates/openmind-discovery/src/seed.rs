// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Static seed candidates.
//!
//! Ports follow the short list documented for a later opt-in probe:
//! 7787 (openmind SRV placeholder), 11434 (ollama), 50052 (llama.cpp
//! rpc-server), 52415 (exo). Every host is loopback. This module dials nothing.

use crate::{Candidate, Origin, APPROVAL};
use std::collections::BTreeMap;

const PORT_OPENMIND: u16 = 7787;
const PORT_OLLAMA: u16 = 11434;
const PORT_LLAMA_RPC: u16 = 50052;
const PORT_EXO: u16 = 52415;

/// Built-in candidates. Order is stable and is not a scan result.
pub fn static_seeds() -> Vec<Candidate> {
    vec![
        seed(
            "seed-openmind",
            "openmind",
            "_openmind._tcp.local.",
            PORT_OPENMIND,
        ),
        seed("seed-ollama", "ollama", "_ollama._tcp.local.", PORT_OLLAMA),
        seed(
            "seed-llama-rpc",
            "llama-rpc",
            "_llama-rpc._tcp.local.",
            PORT_LLAMA_RPC,
        ),
        seed("seed-exo", "exo", "_exo._tcp.local.", PORT_EXO),
    ]
}

fn seed(id: &str, label: &str, service_type: &str, port: u16) -> Candidate {
    Candidate {
        id: id.to_owned(),
        instance: format!("{label}.{service_type}"),
        service_type: service_type.to_owned(),
        host: "127.0.0.1".to_owned(),
        port: Some(port),
        addrs: vec!["127.0.0.1".to_owned()],
        txt: BTreeMap::from([
            ("caps".to_owned(), "cpu".to_owned()),
            ("role".to_owned(), "seed".to_owned()),
            ("v".to_owned(), "0".to_owned()),
        ]),
        origin: Origin::Seed,
        approval: APPROVAL.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_are_loopback_and_pending() {
        let rows = static_seeds();
        assert_eq!(rows.len(), 4);
        let ids: Vec<_> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            ["seed-openmind", "seed-ollama", "seed-llama-rpc", "seed-exo"]
        );
        let ports: Vec<_> = rows.iter().map(|row| row.port).collect();
        assert_eq!(
            ports,
            [
                Some(PORT_OPENMIND),
                Some(PORT_OLLAMA),
                Some(PORT_LLAMA_RPC),
                Some(PORT_EXO)
            ]
        );
        for row in &rows {
            assert_eq!(row.origin, Origin::Seed);
            assert_eq!(row.approval, APPROVAL);
            assert_eq!(row.host, "127.0.0.1");
            assert_eq!(row.addrs, vec!["127.0.0.1".to_string()]);
            assert!(row.instance.ends_with(row.service_type.as_str()));
            assert_eq!(row.txt.get("role").map(String::as_str), Some("seed"));
            assert!(row.host.is_ascii());
        }
    }
}
