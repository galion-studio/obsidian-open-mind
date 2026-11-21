// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Shared motion for the desktop page.
//!
//! One step is 160 ms, the same length as the console chip rise. A window
//! is two steps. The wake is four steps. The easing is the console online
//! pulse curve in `crates/console-ui/src/css.rs`.
//!
//! The wake disc starts at the Galion OS one-dot rest size: seven hundredths
//! of the shorter viewport side (`Math.min(w, h) * 0.07` in
//! `desktop/boot/animations/one-dot-draw.js` on
//! `galion-studio/galion-operating-system`, branch
//! `cursor/desktop-boot-assets-080f`). The diameter in `vmin` is twice that
//! fraction of 100. The disc then scales past the viewport and the page
//! fades in. Reduced motion skips the transforms.

/// Length of one motion step, in milliseconds. Chip rise and colour shifts.
pub const STEP_MS: u32 = 160;

/// Window open and close. Two steps.
pub const WINDOW_MS: u32 = STEP_MS * 2;

/// Wake: the disc expands and the page fades in. Four steps.
pub const WAKE_MS: u32 = STEP_MS * 4;

/// How far the rest disc scales. 14 vmin times 8 is 112 vmin, past the page.
pub const WAKE_SCALE: u32 = 8;

/// Rest radius of the one-dot disc, as a fraction `NUM/DEN` of the shorter side.
pub const DOT_REST_NUM: u32 = 7;

/// Denominator of [`DOT_REST_NUM`]. 7/100 is 0.07.
pub const DOT_REST_DEN: u32 = 100;

/// Console pulse curve. The same numbers as `cubic-bezier(.23,1,.32,1)`.
pub const EASE: &str = "cubic-bezier(0.23, 1, 0.32, 1)";

/// Diameter of the rest disc, in `vmin`.
#[must_use]
pub fn disc_diameter_vmin() -> u32 {
    DOT_REST_NUM * 2 * 100 / DOT_REST_DEN
}

/// Token block, wake and window keyframes, and the reduced-motion skip.
#[must_use]
pub fn sheet() -> String {
    let diameter = disc_diameter_vmin();
    let mut css = String::new();
    css.push_str(":root {\n");
    css.push_str(&format!("  --motion-step: {STEP_MS}ms;\n"));
    css.push_str(&format!("  --motion-window: {WINDOW_MS}ms;\n"));
    css.push_str(&format!("  --motion-wake: {WAKE_MS}ms;\n"));
    css.push_str(&format!("  --motion-ease: {EASE};\n"));
    css.push_str("}\n");
    css.push_str(&format!(
        "#wake .disc {{ width: {diameter}vmin; height: {diameter}vmin; }}\n"
    ));
    css.push_str(
        "@media (prefers-reduced-motion: no-preference) {\n\
  .app:not([data-awake=\"1\"]) { opacity: 0; pointer-events: none; }\n\
  .app[data-awake=\"1\"] { opacity: 1; transition: opacity var(--motion-wake) var(--motion-ease); }\n\
  #wake[data-phase=\"expand\"] .disc { animation: wake-disc var(--motion-wake) var(--motion-ease) forwards; }\n\
  #wake[data-phase=\"expand\"] { animation: wake-veil var(--motion-wake) var(--motion-ease) forwards; }\n\
  .win[data-state=\"open\"] .sheet { animation: win-open var(--motion-window) var(--motion-ease); }\n\
  .win[data-state=\"close\"] .sheet { animation: win-close var(--motion-window) var(--motion-ease) forwards; }\n\
  .panel { transition: opacity var(--motion-window) var(--motion-ease); }\n\
  :focus-visible { transition: outline-color var(--motion-step) var(--motion-ease); }\n\
}\n",
    );
    css.push_str(&format!(
        "@keyframes wake-disc {{\n  0% {{ transform: scale(1); opacity: 1; }}\n  70% {{ transform: scale(6); opacity: 1; }}\n  100% {{ transform: scale({WAKE_SCALE}); opacity: 0; }}\n}}\n"
    ));
    css.push_str(
        "@keyframes wake-veil {\n  0%, 70% { opacity: 1; }\n  100% { opacity: 0; }\n}\n\
@keyframes win-open {\n  from { opacity: 0; transform: translateY(8px) scale(0.98); }\n  to { opacity: 1; transform: none; }\n}\n\
@keyframes win-close {\n  from { opacity: 1; transform: none; }\n  to { opacity: 0; transform: translateY(8px) scale(0.98); }\n}\n\
@media (prefers-reduced-motion: reduce) {\n  #wake { display: none; }\n  .app { opacity: 1; }\n  .win .sheet, .disc, .chip { animation: none; }\n}\n",
    );
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_step_builds_the_window_and_the_wake() {
        assert_eq!(STEP_MS, 160);
        assert_eq!(WINDOW_MS, 320);
        assert_eq!(WAKE_MS, 640);
        assert_eq!(WINDOW_MS, STEP_MS * 2);
        assert_eq!(WAKE_MS, STEP_MS * 4);
        assert_eq!(disc_diameter_vmin(), 14);
        assert_eq!(DOT_REST_NUM * 100 / DOT_REST_DEN, 7);
        assert!(WAKE_SCALE * disc_diameter_vmin() > 100);
        assert_eq!(EASE, "cubic-bezier(0.23, 1, 0.32, 1)");
        let console = include_str!("../../console-ui/src/css.rs");
        assert!(console.contains("cubic-bezier(.23,1,.32,1)"));
        let css = sheet();
        assert!(css.contains("--motion-step: 160ms"));
        assert!(css.contains("--motion-window: 320ms"));
        assert!(css.contains("--motion-wake: 640ms"));
        assert!(css.contains("cubic-bezier(0.23, 1, 0.32, 1)"));
        assert!(css.contains("width: 14vmin"));
        assert!(css.contains("scale(8)"));
        assert!(css.contains("prefers-reduced-motion: no-preference"));
        assert!(css.contains("prefers-reduced-motion: reduce"));
        assert!(css.contains("wake-disc"));
        assert!(css.contains("70% { transform: scale(6); opacity: 1; }"));
        assert!(css.contains("0%, 70% { opacity: 1; }"));
        assert!(css.contains("win-open"));
        assert!(css.contains("win-close"));
    }
}
