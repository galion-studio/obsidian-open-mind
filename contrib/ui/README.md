# UI upgrades (ws/ui, 2026-10-18)

- `galion-one/admin.html`, `galion-one/index.html`: the One front-end assets now live in
  `/opt/galion/galion-light/one/` on the VPS (not otherwise tracked in this repo).
  admin: Galion palette (brand/tokens.css), Rust-services status panel (`/api/v1/services`),
  pairing form with clear errors (wrong code / 15-minute lockout), skip link, labels,
  aria-live, focus-visible, responsive two-column grid. index: pairing error messages,
  `role=status` live region, placeholder/required on the code input.
- `console-ide-lint-typecheck.patch`: fixes applied to `/opt/galion/console-ide`
  (not in this repo): `tsc --noEmit` and `eslint --quiet` both clean; console pairing
  codes are uppercased before sha256 like Galion One.
- `crates/tuner/www`: brand accent, inline Galion favicon, focus-visible, reduced motion,
  small-screen sizing.
