// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Pure incident logic: open on first non-pass, resolve on pass, one open
//! incident per service. Time is passed in, so tests use a fake clock.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Healthy,
    Degraded,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incident {
    pub service: String,
    pub severity: &'static str,
    pub description: String,
    pub detected_at: String,
    pub resolved_at: Option<String>,
}

impl Incident {
    pub fn is_open(&self) -> bool {
        self.resolved_at.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Opened(String),
    Resolved(String),
}

/// Apply one observation. `stamp` is the caller's Zagreb RFC 3339 time.
pub fn apply(
    incidents: &mut Vec<Incident>,
    service: &str,
    status: Status,
    detail: &str,
    stamp: &str,
) -> Option<Event> {
    let open = incidents
        .iter()
        .position(|i| i.service == service && i.is_open());
    match (status, open) {
        (Status::Healthy, Some(ix)) => {
            incidents[ix].resolved_at = Some(stamp.to_string());
            Some(Event::Resolved(service.to_string()))
        }
        (Status::Healthy, None) | (_, Some(_)) => None,
        (s, None) => {
            incidents.push(Incident {
                service: service.to_string(),
                severity: if s == Status::Critical { "high" } else { "low" },
                description: detail.to_string(),
                detected_at: stamp.to_string(),
                resolved_at: None,
            });
            Some(Event::Opened(service.to_string()))
        }
    }
}
