// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The message bus. Broadcasts go out through named signal towers (the
//! Tallneck / Spire model): every function subscribed to a tower hears it.
//! Direct links go to one function. Every inbox is bounded; overflow is
//! dropped and counted (backpressure). Contained functions are quarantined:
//! nothing is delivered to them and nothing they send is accepted.

use crate::json::Json;
use crate::system::FunctionId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Who sent a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Endpoint {
    /// Outside the hierarchy (an unknown signal).
    External,
    MasterOverride,
    Gaia,
    Function(FunctionId),
}

impl Endpoint {
    pub fn name(self) -> String {
        match self {
            Endpoint::External => "external".to_string(),
            Endpoint::MasterOverride => "master-override".to_string(),
            Endpoint::Gaia => "GAIA".to_string(),
            Endpoint::Function(f) => f.name().to_string(),
        }
    }
}

/// Where a message goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Through one signal tower to its subscribers.
    Tower(String),
    /// Through every tower to every non-quarantined function.
    AllTowers,
    /// A direct link to one function.
    Direct(FunctionId),
    /// Up to GAIA.
    Gaia,
}

impl Route {
    pub fn name(&self) -> String {
        match self {
            Route::Tower(t) => format!("tower:{t}"),
            Route::AllTowers => "all-towers".to_string(),
            Route::Direct(f) => format!("direct:{}", f.name()),
            Route::Gaia => "gaia".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub seq: u64,
    pub tick: u64,
    pub from: Endpoint,
    pub route: Route,
    pub topic: String,
    pub body: String,
}

impl Message {
    pub fn to_json(&self) -> Json {
        Json::obj()
            .with("seq", self.seq)
            .with("tick", self.tick)
            .with("from", self.from.name())
            .with("route", self.route.name())
            .with("topic", self.topic.as_str())
            .with("body", self.body.as_str())
    }
}

/// A broadcast tower and who listens to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tower {
    pub name: String,
    pub subscribers: BTreeSet<FunctionId>,
}

/// Result of one send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Delivery {
    pub delivered: usize,
    pub dropped: usize,
    pub blocked: usize,
}

#[derive(Clone, Debug)]
pub struct Bus {
    towers: Vec<Tower>,
    inboxes: BTreeMap<FunctionId, VecDeque<Message>>,
    gaia_inbox: VecDeque<Message>,
    quarantined: BTreeSet<FunctionId>,
    capacity: usize,
    seq: u64,
    pub dropped: u64,
    pub blocked: u64,
    pub delivered: u64,
    log: VecDeque<Message>,
    log_cap: usize,
}

impl Bus {
    /// A bus with the default tower layout.
    pub fn new(capacity: usize) -> Bus {
        use FunctionId::*;
        let mut bus = Bus {
            towers: Vec::new(),
            inboxes: FunctionId::ALL
                .iter()
                .map(|f| (*f, VecDeque::new()))
                .collect(),
            gaia_inbox: VecDeque::new(),
            quarantined: BTreeSet::new(),
            capacity: capacity.max(1),
            seq: 0,
            dropped: 0,
            blocked: 0,
            delivered: 0,
            log: VecDeque::new(),
            log_cap: 256,
        };
        // The Spire: MINERVA's global transmission tower, everyone listens.
        bus.add_tower("spire", &FunctionId::ALL);
        // Regional Tallnecks.
        bus.add_tower(
            "tallneck-life",
            &[Artemis, Demeter, Eleuthia, Poseidon, Aether],
        );
        bus.add_tower("tallneck-forge", &[Hephaestus, Minerva, Hades, Poseidon]);
        bus.add_tower("tallneck-archive", &[Apollo, Minerva, Aether, Eleuthia]);
        bus
    }

    pub fn add_tower(&mut self, name: &str, subscribers: &[FunctionId]) {
        self.towers.retain(|t| t.name != name);
        self.towers.push(Tower {
            name: name.to_string(),
            subscribers: subscribers.iter().copied().collect(),
        });
    }

    pub fn towers(&self) -> &[Tower] {
        &self.towers
    }

    pub fn quarantine(&mut self, f: FunctionId) {
        self.quarantined.insert(f);
        // Pending mail to a quarantined function is discarded.
        if let Some(q) = self.inboxes.get_mut(&f) {
            self.blocked += q.len() as u64;
            q.clear();
        }
    }

    pub fn release(&mut self, f: FunctionId) {
        self.quarantined.remove(&f);
    }

    pub fn is_quarantined(&self, f: FunctionId) -> bool {
        self.quarantined.contains(&f)
    }

    fn push(&mut self, to: FunctionId, msg: &Message, d: &mut Delivery) {
        if self.quarantined.contains(&to) {
            d.blocked += 1;
            return;
        }
        let cap = self.capacity;
        let q = self.inboxes.entry(to).or_default();
        if q.len() >= cap {
            d.dropped += 1;
        } else {
            q.push_back(msg.clone());
            d.delivered += 1;
        }
    }

    /// Send a message. Returns how many inboxes got it.
    pub fn send(
        &mut self,
        tick: u64,
        from: Endpoint,
        route: Route,
        topic: &str,
        body: &str,
    ) -> Delivery {
        let mut d = Delivery::default();
        if let Endpoint::Function(f) = from {
            if self.quarantined.contains(&f) {
                d.blocked = 1;
                self.blocked += 1;
                return d;
            }
        }
        self.seq += 1;
        let msg = Message {
            seq: self.seq,
            tick,
            from,
            route: route.clone(),
            topic: topic.to_string(),
            body: body.to_string(),
        };
        let targets: Vec<FunctionId> = match &route {
            Route::Tower(name) => self
                .towers
                .iter()
                .find(|t| &t.name == name)
                .map(|t| t.subscribers.iter().copied().collect())
                .unwrap_or_default(),
            Route::AllTowers => {
                let mut all = BTreeSet::new();
                for t in &self.towers {
                    all.extend(t.subscribers.iter().copied());
                }
                all.into_iter().collect()
            }
            Route::Direct(f) => vec![*f],
            Route::Gaia => Vec::new(),
        };
        if route == Route::Gaia {
            if self.gaia_inbox.len() >= self.capacity {
                d.dropped += 1;
            } else {
                self.gaia_inbox.push_back(msg.clone());
                d.delivered += 1;
            }
        }
        for t in targets {
            if Endpoint::Function(t) == from {
                continue;
            }
            self.push(t, &msg, &mut d);
        }
        self.delivered += d.delivered as u64;
        self.dropped += d.dropped as u64;
        self.blocked += d.blocked as u64;
        if self.log.len() >= self.log_cap {
            self.log.pop_front();
        }
        self.log.push_back(msg);
        d
    }

    /// Take everything waiting for `f`.
    pub fn drain(&mut self, f: FunctionId) -> Vec<Message> {
        self.inboxes
            .get_mut(&f)
            .map(|q| q.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn drain_gaia(&mut self) -> Vec<Message> {
        self.gaia_inbox.drain(..).collect()
    }

    pub fn pending(&self, f: FunctionId) -> usize {
        self.inboxes.get(&f).map_or(0, VecDeque::len)
    }

    pub fn log(&self) -> impl Iterator<Item = &Message> {
        self.log.iter()
    }

    pub fn to_json(&self) -> Json {
        Json::obj()
            .with(
                "towers",
                Json::Arr(
                    self.towers
                        .iter()
                        .map(|t| {
                            Json::obj().with("name", t.name.as_str()).with(
                                "subscribers",
                                Json::Arr(
                                    t.subscribers.iter().map(|f| Json::from(f.name())).collect(),
                                ),
                            )
                        })
                        .collect(),
                ),
            )
            .with("capacity", self.capacity)
            .with("sent", self.seq)
            .with("delivered", self.delivered)
            .with("dropped", self.dropped)
            .with("blocked", self.blocked)
            .with(
                "quarantined",
                Json::Arr(
                    self.quarantined
                        .iter()
                        .map(|f| Json::from(f.name()))
                        .collect(),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tower_broadcast_reaches_subscribers_only() {
        let mut bus = Bus::new(8);
        let d = bus.send(
            1,
            Endpoint::Gaia,
            Route::Tower("tallneck-forge".into()),
            "t",
            "b",
        );
        assert_eq!(d.delivered, 4);
        assert_eq!(bus.pending(FunctionId::Hephaestus), 1);
        assert_eq!(bus.pending(FunctionId::Eleuthia), 0);
    }

    #[test]
    fn direct_link_and_spire() {
        let mut bus = Bus::new(8);
        bus.send(
            1,
            Endpoint::Gaia,
            Route::Direct(FunctionId::Apollo),
            "t",
            "b",
        );
        assert_eq!(bus.pending(FunctionId::Apollo), 1);
        let d = bus.send(
            1,
            Endpoint::Function(FunctionId::Minerva),
            Route::Tower("spire".into()),
            "t",
            "b",
        );
        assert_eq!(d.delivered, 8, "everyone but the sender");
    }

    #[test]
    fn quarantine_blocks_both_directions() {
        let mut bus = Bus::new(8);
        bus.send(
            1,
            Endpoint::Gaia,
            Route::Direct(FunctionId::Hades),
            "t",
            "b",
        );
        bus.quarantine(FunctionId::Hades);
        assert_eq!(bus.pending(FunctionId::Hades), 0);
        let d = bus.send(
            1,
            Endpoint::Function(FunctionId::Hades),
            Route::AllTowers,
            "t",
            "b",
        );
        assert_eq!(d.blocked, 1);
        assert_eq!(d.delivered, 0);
        let d = bus.send(1, Endpoint::Gaia, Route::AllTowers, "t", "b");
        assert_eq!(d.delivered, 8);
        assert_eq!(d.blocked, 1);
    }

    #[test]
    fn bounded_inbox_drops_overflow() {
        let mut bus = Bus::new(2);
        for _ in 0..5 {
            bus.send(
                1,
                Endpoint::Gaia,
                Route::Direct(FunctionId::Apollo),
                "t",
                "b",
            );
        }
        assert_eq!(bus.pending(FunctionId::Apollo), 2);
        assert_eq!(bus.dropped, 3);
    }
}
