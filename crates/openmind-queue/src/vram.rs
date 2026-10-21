// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The VRAM pool: which devices POSEIDON may route GPU work to, how much
//! VRAM each has free, and how far the pool is from the owner's capacity
//! goal of at least [`VRAM_GOAL_GB`] GB.
//!
//! This is a model. Registering a device rents nothing and spends nothing.
//! Only `approved` and healthy devices take work; a `planned` device (for
//! example a RunPod pod nobody has paid for) counts towards the goal as
//! planned capacity and is never routed to. A device with a non-zero price
//! can only become `approved` through an approved row in the
//! `openmind-provider` registry (the owner's approval), see
//! [`VramPool::sync_providers`].

use openmind_provider::{
    Approval as ProviderApproval, ComputeProvider, Health as ProviderHealth, ProviderKind,
    ProviderRecord,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the pool reads from one `openmind-provider` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderView {
    pub id: String,
    pub kind: ProviderKind,
    pub approval: ProviderApproval,
    pub health: ProviderHealth,
    pub vram_gb: Option<u32>,
}

impl ProviderView {
    /// LAN rows need a `join` that said up; other rows are healthy unless down.
    pub fn healthy(&self) -> bool {
        match self.kind {
            ProviderKind::Lan => self.health == ProviderHealth::Up,
            _ => self.health != ProviderHealth::Down,
        }
    }
}

impl From<&ProviderRecord> for ProviderView {
    fn from(r: &ProviderRecord) -> Self {
        ProviderView {
            id: r.id().to_string(),
            kind: r.kind(),
            approval: r.approval(),
            health: r.health(),
            vram_gb: r.vram_gb(),
        }
    }
}

/// The owner's VRAM capacity target (GB). Reaching it needs spend approval.
pub const VRAM_GOAL_GB: u64 = 1500;

/// The pseudo device for CPU offload.
pub const CPU: &str = "cpu";

/// Where a device sits. Routing prefers them in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// A GPU on this machine or a dedicated GPU host.
    Gpu,
    /// A GPU machine on the LAN.
    Lan,
    /// A rented remote GPU (RunPod and the like).
    Remote,
}

/// Approval state of a device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    /// Known or wanted, not approved. Never routed to.
    #[default]
    Planned,
    /// Approved and routable when healthy.
    Approved,
    /// Refused. Never routed to and not counted.
    Rejected,
}

fn yes() -> bool {
    true
}

/// One device in the pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    pub kind: DeviceKind,
    pub vram_gb: u32,
    /// `openmind-provider` row id this device belongs to (may be empty).
    #[serde(default)]
    pub provider: String,
    /// Price in cents per hour, a number only (nothing is ever charged).
    #[serde(default)]
    pub cents_per_hour: u64,
    #[serde(default)]
    pub state: DeviceState,
    #[serde(default = "yes")]
    pub healthy: bool,
}

impl Device {
    pub fn routable(&self) -> bool {
        self.state == DeviceState::Approved && self.healthy && self.id != CPU
    }

    /// Check the id and sizes. Returns a reason on failure.
    pub fn validate(&self) -> Result<(), String> {
        let ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b));
        if !ok || self.id == CPU {
            return Err("device id must be 1-64 chars of [A-Za-z0-9-_.:] and not `cpu`".into());
        }
        if self.vram_gb == 0 || self.vram_gb > 4096 {
            return Err("vram_gb must be 1..=4096".into());
        }
        Ok(())
    }
}

/// Capacity summary against the goal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub devices: usize,
    pub routable_devices: usize,
    pub approved_gb: u64,
    pub planned_gb: u64,
    pub used_gb: u64,
    pub free_gb: u64,
    pub goal_gb: u64,
    /// GB still missing from the goal counting approved devices only.
    pub gap_gb: u64,
    /// Price of the approved devices, cents per hour (numbers only).
    pub approved_cents_per_hour: u64,
    /// Price of the planned devices, cents per hour: what reaching the plan
    /// would cost. Needs the owner's spend approval.
    pub planned_cents_per_hour: u64,
}

/// The pool. `used` is rebuilt from leased jobs by the engine.
#[derive(Clone, Debug, Default)]
pub struct VramPool {
    devices: BTreeMap<String, Device>,
    used: BTreeMap<String, u64>,
}

impl VramPool {
    pub fn upsert(&mut self, d: Device) {
        self.devices.insert(d.id.clone(), d);
    }

    pub fn remove(&mut self, id: &str) -> Option<Device> {
        self.devices.remove(id)
    }

    pub fn get(&self, id: &str) -> Option<&Device> {
        self.devices.get(id)
    }

    pub fn devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.values()
    }

    pub fn used(&self, id: &str) -> u64 {
        self.used.get(id).copied().unwrap_or(0)
    }

    /// Free GB on one device (0 for unknown devices).
    pub fn free(&self, id: &str) -> u64 {
        self.devices
            .get(id)
            .map_or(0, |d| u64::from(d.vram_gb).saturating_sub(self.used(id)))
    }

    pub fn hold(&mut self, id: &str, gb: u32) {
        let u = self.used.entry(id.to_string()).or_default();
        *u = u.saturating_add(u64::from(gb));
    }

    pub fn unhold(&mut self, id: &str, gb: u32) {
        if let Some(u) = self.used.get_mut(id) {
            *u = u.saturating_sub(u64::from(gb));
            if *u == 0 {
                self.used.remove(id);
            }
        }
    }

    /// Choose a device for `gb`: routable only, kind order GPU -> LAN ->
    /// remote, then best fit (least free VRAM that still fits), then id.
    pub fn place(&self, gb: u32) -> Option<String> {
        let gb = u64::from(gb);
        self.devices
            .values()
            .filter(|d| d.routable() && self.free(&d.id) >= gb)
            .min_by_key(|d| (d.kind, self.free(&d.id), d.id.clone()))
            .map(|d| d.id.clone())
    }

    /// Could `gb` fit on some routable device if it were empty?
    pub fn can_ever_fit(&self, gb: u32) -> bool {
        self.devices
            .values()
            .any(|d| d.routable() && d.vram_gb >= gb)
    }

    pub fn largest_routable_gb(&self) -> u32 {
        self.devices
            .values()
            .filter(|d| d.routable())
            .map(|d| d.vram_gb)
            .max()
            .unwrap_or(0)
    }

    /// Sums saturate: `cents_per_hour` is any u64 a client sends to
    /// `POST /vram/devices`, and two devices near u64::MAX overflowed
    /// (`/stats` panicked in debug, wrapped to a wrong price in release;
    /// slot 221 fuzz find).
    pub fn summary(&self) -> Summary {
        let mut s = Summary {
            goal_gb: VRAM_GOAL_GB,
            devices: self.devices.len(),
            ..Summary::default()
        };
        for d in self.devices.values() {
            let gb = u64::from(d.vram_gb);
            match d.state {
                DeviceState::Approved => {
                    s.approved_gb = s.approved_gb.saturating_add(gb);
                    s.approved_cents_per_hour =
                        s.approved_cents_per_hour.saturating_add(d.cents_per_hour);
                    if d.routable() {
                        s.routable_devices += 1; // bounded by the device count
                        s.free_gb = s.free_gb.saturating_add(self.free(&d.id));
                    }
                }
                DeviceState::Planned => {
                    s.planned_gb = s.planned_gb.saturating_add(gb);
                    s.planned_cents_per_hour =
                        s.planned_cents_per_hour.saturating_add(d.cents_per_hour);
                }
                DeviceState::Rejected => {}
            }
        }
        s.used_gb = self.used.values().fold(0u64, |t, u| t.saturating_add(*u));
        s.gap_gb = VRAM_GOAL_GB.saturating_sub(s.approved_gb);
        s
    }

    /// Mirror approval and health from `openmind-provider` rows onto the
    /// pool. Returns the changed or new devices (the caller makes them
    /// durable).
    ///
    /// - A device that names a provider row follows it: approved ->
    ///   `approved`, pending -> `planned`, rejected -> `rejected`; a missing
    ///   row makes it `planned` (no row, no approval).
    /// - An **approved** LAN or GPU row that advertises `vram_gb` (discovery
    ///   TXT `vram=`) and has no device yet becomes a device with the row's
    ///   id. Pending and rejected rows never create a device.
    /// - Health: a LAN row counts as healthy only when `join` recorded it
    ///   `up` (never joined or down -> not routable); other rows are healthy
    ///   unless `down`.
    pub fn sync_providers(&self, rows: &[ProviderView]) -> Vec<Device> {
        let mut out = Vec::new();
        for d in self.devices.values() {
            if d.provider.is_empty() {
                continue;
            }
            let row = rows.iter().find(|r| r.id == d.provider);
            let (state, healthy) = match row {
                None => (DeviceState::Planned, d.healthy),
                Some(r) => (
                    match r.approval {
                        ProviderApproval::Approved => DeviceState::Approved,
                        ProviderApproval::Pending => DeviceState::Planned,
                        ProviderApproval::Rejected => DeviceState::Rejected,
                    },
                    r.healthy(),
                ),
            };
            if state != d.state || healthy != d.healthy {
                let mut n = d.clone();
                n.state = state;
                n.healthy = healthy;
                out.push(n);
            }
        }
        for r in rows {
            if r.approval != ProviderApproval::Approved {
                continue;
            }
            let kind = match r.kind {
                ProviderKind::Lan => DeviceKind::Lan,
                ProviderKind::Gpu => DeviceKind::Gpu,
                ProviderKind::CpuLocal | ProviderKind::Remote => continue,
            };
            let Some(vram_gb) = r.vram_gb else { continue };
            if self.devices.contains_key(&r.id) || self.devices.values().any(|d| d.provider == r.id)
            {
                continue;
            }
            let d = Device {
                id: r.id.clone(),
                kind,
                vram_gb,
                provider: r.id.clone(),
                cents_per_hour: 0,
                state: DeviceState::Approved,
                healthy: r.healthy(),
            };
            if d.validate().is_ok() {
                out.push(d);
            }
        }
        out
    }

    /// The rule for devices set through the API: a priced device may not be
    /// marked `approved` by hand. It is demoted to `planned` and the reason
    /// is returned. Approval of paid compute comes only from an approved
    /// provider row (the owner).
    pub fn gate_manual(
        d: &mut Device,
        provider_approved: impl Fn(&str) -> bool,
    ) -> Option<&'static str> {
        if d.state == DeviceState::Approved
            && !d.provider.is_empty()
            && !provider_approved(&d.provider)
        {
            d.state = DeviceState::Planned;
            return Some("device names an openmind-provider row that is not approved (pending, denied or missing); kept as planned");
        }
        if d.state == DeviceState::Approved
            && d.cents_per_hour > 0
            && !(!d.provider.is_empty() && provider_approved(&d.provider))
        {
            d.state = DeviceState::Planned;
            return Some("paid device needs an approved openmind-provider row (owner spend approval); kept as planned");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, kind: DeviceKind, gb: u32, state: DeviceState) -> Device {
        Device {
            id: id.into(),
            kind,
            vram_gb: gb,
            provider: String::new(),
            cents_per_hour: 0,
            state,
            healthy: true,
        }
    }

    #[test]
    fn place_prefers_gpu_then_lan_then_best_fit() {
        let mut p = VramPool::default();
        p.upsert(dev("lan-24", DeviceKind::Lan, 24, DeviceState::Approved));
        p.upsert(dev("gpu-80", DeviceKind::Gpu, 80, DeviceState::Approved));
        p.upsert(dev("gpu-48", DeviceKind::Gpu, 48, DeviceState::Approved));
        p.upsert(dev("pod-96", DeviceKind::Remote, 96, DeviceState::Planned));
        assert_eq!(p.place(40).as_deref(), Some("gpu-48"));
        p.hold("gpu-48", 40);
        assert_eq!(p.place(40).as_deref(), Some("gpu-80"));
        p.hold("gpu-80", 80);
        assert_eq!(p.place(20).as_deref(), Some("lan-24"));
        // the planned pod is never routed to
        assert_eq!(p.place(90), None);
        assert!(!p.can_ever_fit(90));
        p.unhold("gpu-80", 80);
        assert_eq!(p.free("gpu-80"), 80);
    }

    #[test]
    fn unhealthy_and_rejected_not_routable() {
        let mut p = VramPool::default();
        let mut d = dev("g", DeviceKind::Gpu, 24, DeviceState::Approved);
        d.healthy = false;
        p.upsert(d);
        p.upsert(dev("r", DeviceKind::Gpu, 24, DeviceState::Rejected));
        assert_eq!(p.place(1), None);
        assert_eq!(p.largest_routable_gb(), 0);
    }

    #[test]
    fn summary_against_goal() {
        let mut p = VramPool::default();
        p.upsert(dev("a", DeviceKind::Gpu, 24, DeviceState::Approved));
        let mut pod = dev("pod", DeviceKind::Remote, 80, DeviceState::Planned);
        pod.cents_per_hour = 199;
        p.upsert(pod);
        p.hold("a", 8);
        let s = p.summary();
        assert_eq!(s.approved_gb, 24);
        assert_eq!(s.planned_gb, 80);
        assert_eq!(s.used_gb, 8);
        assert_eq!(s.free_gb, 16);
        assert_eq!(s.gap_gb, 1500 - 24);
        assert_eq!(s.planned_cents_per_hour, 199);
    }

    #[test]
    fn paid_device_cannot_be_approved_by_hand() {
        let mut d = dev("pod", DeviceKind::Remote, 80, DeviceState::Approved);
        d.cents_per_hour = 250;
        assert!(VramPool::gate_manual(&mut d, |_| true).is_some());
        assert_eq!(d.state, DeviceState::Planned);
        let mut d2 = dev("pod", DeviceKind::Remote, 80, DeviceState::Approved);
        d2.cents_per_hour = 250;
        d2.provider = "runpod-a".into();
        assert!(VramPool::gate_manual(&mut d2, |p| p == "runpod-a").is_none());
        assert_eq!(d2.state, DeviceState::Approved);
        let mut free = dev("lan", DeviceKind::Lan, 24, DeviceState::Approved);
        assert!(VramPool::gate_manual(&mut free, |_| false).is_none());
    }

    fn view(
        id: &str,
        kind: ProviderKind,
        approval: ProviderApproval,
        health: ProviderHealth,
        vram_gb: Option<u32>,
    ) -> ProviderView {
        ProviderView {
            id: id.into(),
            kind,
            approval,
            health,
            vram_gb,
        }
    }

    #[test]
    fn only_approved_discovery_rows_become_devices_and_lan_needs_join_up() {
        let mut p = VramPool::default();
        let rows = vec![
            view(
                "lan-ok",
                ProviderKind::Lan,
                ProviderApproval::Approved,
                ProviderHealth::Up,
                Some(24),
            ),
            view(
                "lan-pending",
                ProviderKind::Lan,
                ProviderApproval::Pending,
                ProviderHealth::Up,
                Some(48),
            ),
            view(
                "lan-denied",
                ProviderKind::Lan,
                ProviderApproval::Rejected,
                ProviderHealth::Up,
                Some(80),
            ),
            view(
                "lan-unjoined",
                ProviderKind::Lan,
                ProviderApproval::Approved,
                ProviderHealth::Unknown,
                Some(16),
            ),
            view(
                "lan-novram",
                ProviderKind::Lan,
                ProviderApproval::Approved,
                ProviderHealth::Up,
                None,
            ),
            view(
                "cpu",
                ProviderKind::CpuLocal,
                ProviderApproval::Approved,
                ProviderHealth::Up,
                Some(8),
            ),
        ];
        let new = p.sync_providers(&rows);
        let ids: Vec<&str> = new.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["lan-ok", "lan-unjoined"]);
        for d in new {
            p.upsert(d);
        }
        assert!(p.get("lan-ok").unwrap().routable());
        assert!(!p.get("lan-unjoined").unwrap().routable());
        assert_eq!(p.place(20).as_deref(), Some("lan-ok"));
        assert_eq!(p.place(30), None, "the 48 GB pending row is never used");
        // Second sync: no change.
        assert!(p.sync_providers(&rows).is_empty());
        // Killed (join says down): not routable. Denied: rejected.
        let mut rows2 = rows.clone();
        rows2[0].health = ProviderHealth::Down;
        for d in p.sync_providers(&rows2) {
            p.upsert(d);
        }
        assert_eq!(p.place(1), None);
        rows2[0].health = ProviderHealth::Up;
        rows2[0].approval = ProviderApproval::Rejected;
        for d in p.sync_providers(&rows2) {
            p.upsert(d);
        }
        assert_eq!(p.get("lan-ok").unwrap().state, DeviceState::Rejected);
        assert_eq!(p.place(1), None);
    }

    #[test]
    fn manual_device_naming_an_unapproved_row_stays_planned() {
        let mut d = dev("lan", DeviceKind::Lan, 24, DeviceState::Approved);
        d.provider = "lan-pending".into();
        assert!(VramPool::gate_manual(&mut d, |_| false).is_some());
        assert_eq!(d.state, DeviceState::Planned);
    }

    #[test]
    fn provider_sync_mirrors_approval_and_health() {
        let mut p = VramPool::default();
        let mut a = dev("a", DeviceKind::Remote, 48, DeviceState::Planned);
        a.provider = "pa".into();
        let mut b = dev("b", DeviceKind::Lan, 24, DeviceState::Approved);
        b.provider = "pb".into();
        let mut c = dev("c", DeviceKind::Lan, 24, DeviceState::Approved);
        c.provider = "gone".into();
        p.upsert(a);
        p.upsert(b);
        p.upsert(c);
        let rows = vec![
            view(
                "pa",
                ProviderKind::Remote,
                ProviderApproval::Approved,
                ProviderHealth::Up,
                None,
            ),
            view(
                "pb",
                ProviderKind::Lan,
                ProviderApproval::Approved,
                ProviderHealth::Down,
                None,
            ),
        ];
        let ch = p.sync_providers(&rows);
        assert_eq!(ch.len(), 3);
        assert_eq!(ch[0].state, DeviceState::Approved);
        assert!(!ch[1].healthy);
        assert_eq!(ch[2].state, DeviceState::Planned);
    }
}
