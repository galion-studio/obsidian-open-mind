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

use openmind_provider::{Approval as ProviderApproval, Health as ProviderHealth};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
        *self.used.entry(id.to_string()).or_default() += u64::from(gb);
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
                    s.approved_gb += gb;
                    s.approved_cents_per_hour += d.cents_per_hour;
                    if d.routable() {
                        s.routable_devices += 1;
                        s.free_gb += self.free(&d.id);
                    }
                }
                DeviceState::Planned => {
                    s.planned_gb += gb;
                    s.planned_cents_per_hour += d.cents_per_hour;
                }
                DeviceState::Rejected => {}
            }
        }
        s.used_gb = self.used.values().sum();
        s.gap_gb = VRAM_GOAL_GB.saturating_sub(s.approved_gb);
        s
    }

    /// Mirror approval and health from `openmind-provider` rows onto the
    /// devices that name a provider. Returns the changed devices (the caller
    /// makes them durable). A device whose provider row is missing becomes
    /// `planned`: no row, no approval.
    pub fn sync_providers(
        &self,
        rows: &[(String, ProviderApproval, ProviderHealth)],
    ) -> Vec<Device> {
        let mut out = Vec::new();
        for d in self.devices.values() {
            if d.provider.is_empty() {
                continue;
            }
            let row = rows.iter().find(|(id, _, _)| *id == d.provider);
            let (state, healthy) = match row {
                None => (DeviceState::Planned, d.healthy),
                Some((_, a, h)) => (
                    match a {
                        ProviderApproval::Approved => DeviceState::Approved,
                        ProviderApproval::Pending => DeviceState::Planned,
                        ProviderApproval::Rejected => DeviceState::Rejected,
                    },
                    *h != ProviderHealth::Down,
                ),
            };
            if state != d.state || healthy != d.healthy {
                let mut n = d.clone();
                n.state = state;
                n.healthy = healthy;
                out.push(n);
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
            (
                "pa".to_string(),
                ProviderApproval::Approved,
                ProviderHealth::Up,
            ),
            (
                "pb".to_string(),
                ProviderApproval::Approved,
                ProviderHealth::Down,
            ),
        ];
        let ch = p.sync_providers(&rows);
        assert_eq!(ch.len(), 3);
        assert_eq!(ch[0].state, DeviceState::Approved);
        assert!(!ch[1].healthy);
        assert_eq!(ch[2].state, DeviceState::Planned);
    }
}
