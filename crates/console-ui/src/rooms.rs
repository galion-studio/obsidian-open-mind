// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The console's room list and dock, carried over from console-ide
//! `src/lib/rooms.ts` and `src/components/ide-shell.tsx` (live tree,
//! 2026-10-20). One list drives the menu grid, the header title and the
//! "not ported yet" page.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub struct Room {
    /// In-console path, or `None` for an external link.
    pub to: Option<&'static str>,
    pub href: Option<&'static str>,
    pub label: &'static str,
    pub line: &'static str,
    pub icon: &'static str,
    pub zone: Zone,
}

const fn r(to: &'static str, label: &'static str, line: &'static str, icon: &'static str) -> Room {
    Room {
        to: Some(to),
        href: None,
        label,
        line,
        icon,
        zone: Zone::Up,
    }
}

const fn d(to: &'static str, label: &'static str, line: &'static str, icon: &'static str) -> Room {
    Room {
        to: Some(to),
        href: None,
        label,
        line,
        icon,
        zone: Zone::Down,
    }
}

pub const ROOMS: &[Room] = &[
    r(
        "/console",
        "Desk",
        "The whole project on one screen.",
        "desk",
    ),
    r(
        "/instances",
        "Instances",
        "Phone, pod, laptop. The private path.",
        "instances",
    ),
    r(
        "/hermes",
        "Hermes Research",
        "The mind. One ask, then he waits.",
        "research",
    ),
    r(
        "/context",
        "Context",
        "The window, the sessions, the tools.",
        "context",
    ),
    r(
        "/memory",
        "Memory",
        "L0 through L3, scenes, and the raw tool logs.",
        "context",
    ),
    r(
        "/bank",
        "Bank",
        "OpenRouter, River, Revolut, and how long the cash lasts.",
        "context",
    ),
    r(
        "/cache",
        "Cache",
        "Tokens, files, and what the mind kept.",
        "cache",
    ),
    r(
        "/mesh",
        "Scope",
        "The live measurement. Not history.",
        "scope",
    ),
    r(
        "/arena",
        "Arena",
        "Four throwaways. You breed. SOUL stays.",
        "arena",
    ),
    r(
        "/agents",
        "Agents",
        "Origin, tunnel, keep. No extra runtime.",
        "agents",
    ),
    r("/analytics", "Nerve", "Health of this pod.", "nerve"),
    r(
        "/services",
        "Services",
        "What is up, what is cold.",
        "services",
    ),
    r("/domains", "Edge", "Hosts on the tunnel.", "edge"),
    r("/docs", "API", "The route ledger.", "api"),
    r("/gateway", "Gateway", "Hit a live endpoint.", "gateway"),
    r(
        "/security",
        "Defence",
        "The fort, across the stack.",
        "defence",
    ),
    r("/guardian", "Guardian", "Defence. Mode 042.", "guardian"),
    r("/keys", "Keys", "This phone mints and revokes.", "keys"),
    r("/node", "Pod", "This machine.", "pod"),
    r(
        "/jupyter",
        "Jupyter",
        "The shell already on this pod.",
        "jupyter",
    ),
    r("/markers", "Markers", "What the desk recorded.", "markers"),
    r(
        "/session",
        "Session",
        "This build, downloadable.",
        "session",
    ),
    r(
        "/cloudflare",
        "Cloudflare",
        "The tunnel itself.",
        "cloudflare",
    ),
    r("/repos", "Repos", "The studio on GitHub.", "repos"),
    r("/soon", "Soon", "Public hosts stay quiet.", "soon"),
    d("/chill", "Fridge", "Asleep. Not this pass.", "fridge"),
    d("/pack", "Pack", "Chilled compose. It does not run.", "pack"),
    Room {
        to: None,
        href: Some("https://one.galion.app"),
        label: "One",
        line: "The mouth. Already running.",
        icon: "one",
        zone: Zone::Down,
    },
    d(
        "/data",
        "Data",
        "No database process. The skill reads.",
        "data",
    ),
];

/// Bottom dock (path, label).
pub const DOCK: &[(&str, &str)] = &[
    ("/console", "Desk"),
    ("/memory", "Memory"),
    ("/bank", "Bank"),
    ("/instances", "Instances"),
    ("/hermes", "Research"),
    ("/analytics", "Nerve"),
    ("/security", "Defence"),
];

/// `pathname.startsWith(to)` like the TSX `onRoom`.
pub fn on_room(path: &str, to: &str) -> bool {
    if to == "/" {
        path == "/"
    } else {
        path.starts_with(to)
    }
}

/// The room a path belongs to (falls back to the Desk like the TSX).
pub fn current(path: &str) -> &'static Room {
    ROOMS
        .iter()
        .find(|r| r.to.is_some_and(|to| on_room(path, to)))
        .unwrap_or(&ROOMS[0])
}

/// Exact room for a page path (used for "not ported yet").
pub fn exact(path: &str) -> Option<&'static Room> {
    ROOMS.iter().find(|r| r.to == Some(path))
}

pub fn icon_of(to: &str) -> Option<&'static str> {
    ROOMS.iter().find(|r| r.to == Some(to)).map(|r| r.icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_and_dock_agree() {
        assert_eq!(ROOMS.len(), 29);
        for (to, _) in DOCK {
            assert!(icon_of(to).is_some(), "{to}");
        }
        assert_eq!(current("/services").label, "Services");
        assert_eq!(current("/nowhere").label, "Desk");
        assert_eq!(current("/md/user").label, "Desk");
        assert!(exact("/agents").is_some());
        assert!(exact("/agents/x").is_none());
    }
}
