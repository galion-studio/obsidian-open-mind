// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Where the admin desk attaches a cue, and the silence opt-in.
//!
//! Resonance is the default recording ([`crate::sound::Voice::default`]).
//! Silence is a cookie the person turns on. Bell strikes are not attached
//! to approval, task-done or error.

use crate::html::{esc, url_enc};
use crate::icons::icon;
use crate::sound::{self, Admin, Sound};

pub const SILENCE_COOKIE: &str = "galion_silence";

pub fn silenced(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Empty when silence is on, so the browser does not fetch or play.
pub fn tag(sound: Sound, silence: bool) -> String {
    if silence {
        return String::new();
    }
    format!(
        "<audio src=\"{}\" autoplay preload=\"auto\" aria-hidden=\"true\"></audio>",
        sound.path()
    )
}

pub fn admin_tag(kind: Admin, silence: bool) -> String {
    let sound = sound::admin(kind);
    debug_assert!(!sound.is_bell());
    tag(sound, silence)
}

/// A finished admin action. Refusals and failures are the error cue.
pub fn result_kind(note: &str) -> Admin {
    let n = note.to_ascii_lowercase();
    if n.contains("\"ok\":false")
        || n.contains("\"error\"")
        || n.contains("failed")
        || n.contains("refused")
        || n.contains("unknown")
        || n.contains("nothing was sent")
        || n.starts_with("http ")
    {
        Admin::Error
    } else {
        Admin::TaskDone
    }
}

/// Header control. Sound stays on until this is followed.
pub fn silence_toggle(path: &str, silence: bool) -> String {
    let (to, label) = if silence {
        ("off", "Turn sound on")
    } else {
        ("on", "Turn on silence")
    };
    format!(
        "<a class=\"iconbtn\" href=\"/_ui/silence?to={to}&amp;back={back}\" aria-label=\"{label}\">{i}</a>",
        back = esc(&url_enc(path)),
        i = icon("voice", "i"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{bell, Window};

    #[test]
    fn silence_omits_the_element_and_bells_stay_off_the_admin_cues() {
        assert!(!silenced(None));
        assert!(!silenced(Some("0")));
        assert!(silenced(Some("1")));
        assert!(admin_tag(Admin::ApprovalNeeded, true).is_empty());
        let html = admin_tag(Admin::Error, false);
        assert!(html.contains("/_ui/sound/error.wav"));
        assert!(!html.contains("bell-"));
        assert!(html.contains("aria-hidden=\"true\""));
        assert_eq!(result_kind("scan: HTTP 200"), Admin::TaskDone);
        assert_eq!(result_kind("heal: refused, HTTP 401"), Admin::Error);
        assert_eq!(
            result_kind("Heal: enter the Defender PIN (nothing was sent)."),
            Admin::Error
        );
        assert_eq!(result_kind("command failed: down"), Admin::Error);
        assert!(bell(Window::Open).is_bell());
        let link = silence_toggle("/agents", false);
        assert!(link.contains("aria-label=\"Turn on silence\""));
        assert!(link.contains("back=/agents"));
        assert!(silence_toggle("/a b", true).contains("Turn sound on"));
        assert!(silence_toggle("/a b", true).contains("back=/a%20b"));
    }
}
