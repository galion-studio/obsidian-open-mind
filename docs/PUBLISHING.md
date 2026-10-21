# Publishing: the gate

Written 2026-10-18T19:35:28+02:00 (Europe/Zagreb). Public release deadline: **2026-10-31** (Europe/Zagreb).

The repository `galion-studio/obsidian-open-mind` is created **private** and stays private until
the stack passes its own checks. "Publishing" means exactly one action: changing the
repository visibility from private to public. Nothing else counts as publishing, and the
release site and the marketing drafts are not deployed or posted as part of this step.

## The gate (all must be true)

1. **`self-eval` is green on `main`.** The workflow `.github/workflows/self-eval.yml` runs on
   every push and pull request and must pass on the latest `main` commit:
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - no Python: no `*.py` file and no python shebang anywhere in the tree
   - no CJK characters in tracked text
   - gitleaks secret scan over the full git history
   - license check: `LICENSE` present and MIT, workspace `license = "MIT"`
   - Zagreb timestamps: best effort, warning only (not blocking)
2. **The owner approves.** Slavik / Mate say "publish" explicitly, after reading the latest
   self-eval run. A green run alone never flips the repository.
3. **`RELEASE-CHECKLIST.md` has no open blocking item**, `__CONTACT_EMAIL__` in
   `SECURITY.md` / `CODE_OF_CONDUCT.md` is filled in, and the `TODO(verify)` items in
   `README.md` are resolved or stated as known gaps.

If the gate is not met by 2026-10-31, the repository stays private and the date moves; the
gate does not.

## Flipping to public

> **Update 2026-10-20T20:15:00+02:00 (swarm/launch-gates):** do not flip this
> repository. Its history still contains `contrib/`, the `*.redacted` secret
> inventory, `stack/kms/` and committed binaries, which `.gitattributes`
> export-ignore only removes from `git archive`. Create the public repository
> from the tree produced by `scripts/public-export.sh` (fresh history) instead;
> the settings below then apply to that new repository. See
> `docs/launch/PUBLIC-EXPORT.md` and `docs/launch/GATES-OCT31.md`.

Settings → General → Danger Zone → Change visibility → Public (owner only). Then, in
Settings → Code security:

- **Secret scanning and push protection** are turned on automatically for public
  repositories, free of charge. Confirm both show "Enabled".
- **Private vulnerability reporting**: enable it, so `SECURITY.md`'s "Report a vulnerability"
  link works.
- **Dependabot alerts / security updates**: enable (`.github/dependabot.yml` already covers
  `cargo` and `github-actions`).
- **Branch protection on `main`**: require the `self-eval` checks and a pull request.

While the repository is **private**, secret scanning and push protection are **not** free:
they need GitHub Advanced Security (Secret Protection) on a paid plan. Until the flip, the
gitleaks job in `self-eval` is the secret gate. GitHub Actions on a private repository uses
the account's included minutes; if they run out, self-eval does not run and the gate cannot
be met until it does.

## After publishing

- The OpenSSF Scorecard workflow (`.github/workflows/scorecard.yml`) starts publishing
  results, and the Scorecard badge in `README.md` starts rendering.
- A release is cut by pushing a `v*` tag. `.github/workflows/release.yml` re-runs the full
  self-eval gate first, then builds, writes a CycloneDX SBOM, signs every artifact with
  Sigstore cosign (keyless) and creates the GitHub Release with the SBOM and the
  `.sigstore.json` bundles attached.
- REUSE: the README badge is a placeholder until the tree has `LICENSES/` and SPDX headers.
