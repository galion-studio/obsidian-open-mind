# MANIFEST — contrib/grokbot

Generated: 2026-10-18 19:00 CEST (+02:00) (Europe/Zagreb). License: MIT (`LICENSE`).

## Sources
- `mcp/vps-mcp/` ← Grok Bot box `~/mcp/vps-mcp` (excluded: `target/`, `.git/`; added `license = "MIT"` to Cargo.toml)
- `skills/` ← `~/skills/` README + 6 skills (excluded: `imported-from-vps/`, third-party/VPS origin)
- `workflows/` ← `~/workflows/` README + 4 workflows, `vps/bin/*.sh`, `vps/services.list` (excluded: `imported-from-vps/`, `vps/data/`, `*.bak*`, live `vps/config.env`, live `vps/stack-map.md`, `vps/study/bin`, `vps/study/src`)
- `workflows/vps/config.env.example` ← derived from live config.env: host/user → `__FILL_ME_HOST__`/`__FILL_ME_USER__`, alias/key path genericised
- `workflows/vps/stack-map.template.md` ← new blank template (live stack map not shipped)
- `docs/` ← `~/workflows/vps/{main.md,quant-office-design.md,hermes-openrouter-proposal.md,study/A-foundation.md}`

## Scrubbing
- Real VPS IP → `__VPS_HOST__`, hostname → `__VPS_HOSTNAME__`, box home path → `~/`.
- Grep for `sk-`, `API_KEY=`, `BEGIN .* PRIVATE`, the IP, long tokens, CJK: no key material or secret values found (`API_KEY=` hits are placeholders only; `sk-` hit is prose).

## Files (sha256)

| sha256 | bytes | path |
|---|---|---|
| `1e567a727ce6f37aa3fed1ccd042bd1d23c11e5ef4f585d2d65d13987c60f050` | 1115 | `LICENSE` |
| `417c966d638655b9105e6db954cb1f5a575d5f631c5e06b39556cf80af0611a8` | 1149 | `README.md` |
| `90ac2fc3a1944d41a46a65b40a36c16ae78f0a60c8546f1423cfdba9ab7a8025` | 10032 | `docs/hermes-openrouter-proposal.md` |
| `40042efa0098f7f957e21e23596a0e7eea8976d75f539c7a340fe0a5cd29bb9c` | 2421 | `docs/main.md` |
| `36ad3d0802a74f3f0caec6f73325d477b5efea0c2b1e80308cb8adcb2b2a33e7` | 52809 | `docs/quant-office-design.md` |
| `9625661d5810b0ad0eec6ef1316f0b4f862f5d6f98e0774faeb0b722d7ef3507` | 13782 | `docs/study/A-foundation.md` |
| `44c92e3a70ad3307b7056871c2bdb096d8bfa9373f5bf06a79bb6324a20ff2fb` | 8 | `mcp/vps-mcp/.gitignore` |
| `bf4dc0dd35f939b130138b9be8f4e8c79046efb398d1056a24017c18730e89e0` | 57845 | `mcp/vps-mcp/Cargo.lock` |
| `f7aadad5654843a3d81e6fb3f7d29d38276b4614e4a5c9d2205436a9cc5bb625` | 485 | `mcp/vps-mcp/Cargo.toml` |
| `8b5b66d99285e1548f20dd44c8d5a7f3aa9bde9a6a86a24e8d3f8f67ab85d861` | 6489 | `mcp/vps-mcp/README.md` |
| `78ab3b5d915d6dbf2fba7bd2527b8fe08ef41a5f7398c94eedf3dbe2b938e00b` | 12567 | `mcp/vps-mcp/src/config.rs` |
| `21f31dcd38357405faea54d31e0bdeec986d2ff817af4dceae5d146156f3a9c7` | 14835 | `mcp/vps-mcp/src/main.rs` |
| `ccfbd3ad4ab4f65f2b42bda677568d6c1e464873ab280967fc0227a58409c6dc` | 21669 | `mcp/vps-mcp/src/ops.rs` |
| `1bb562ff9c3ffe4e24d0d83b746986bf0a069e03e0199fd4df8a517c1bd19126` | 11065 | `mcp/vps-mcp/src/policy.rs` |
| `576232f57234098d90ce621df9a620d94e4d3ef09feb45e2bae60783d2e1af7f` | 8608 | `mcp/vps-mcp/src/ssh.rs` |
| `9eadf01d2c70ff01e46bc0b5b80fd341fe6643435f94c250ef2576132c552e2f` | 2044 | `skills/README.md` |
| `1ec14307f34b7129721b4191af2297d4f81c58b2fbf967211326e4dc03592066` | 2860 | `skills/deploy-updates/SKILL.md` |
| `8d29909bca9b64303bdbb01c13ca660513d130d6c52154f8e1b83896c33f21fa` | 3282 | `skills/logs-and-error-fixing/SKILL.md` |
| `dbc1fafc5b42fecff1d7355075b3d50bb1c32e25b5adb476c14eaa77f9e08a06` | 2986 | `skills/routine-checks/SKILL.md` |
| `7255f13ec2db9855ef5d8c20b4d85854e76ec11decbcb03e25a0f1be752bc267` | 1795 | `skills/service-health-check/SKILL.md` |
| `94b68eebeb42169d5c42aabd188b36e9db1741e69d80dbcbfc903e16e4be7019` | 2971 | `skills/stack-mapping/SKILL.md` |
| `a318ccdee8a32e4fd3f63e35e1e313e78b48388c7abb3f495499b0431fa282ee` | 2490 | `skills/vps-ssh-access/SKILL.md` |
| `b3856d5f6bb8e669328d520368b24d20564839901deb6b4a975bc68a55c21894` | 632 | `workflows/README.md` |
| `c199d1949a89fe027172fddb903ded1a7d287eba02decb866299474b13939ef4` | 1378 | `workflows/daily-health-check.md` |
| `01c25e69c2215a29a591c65542e0b9edf1968c1f5202111c482a92934c15f3dd` | 1601 | `workflows/deploy-release.md` |
| `bbab70b04447dd52c89629ff81a9e8bcc03101b991cbf512bfa0aa4809d56758` | 1532 | `workflows/first-connect-and-map.md` |
| `6df427a64e7e10d55afef073df888e83a922bdabe0c94ca770373db2fa8af61a` | 1917 | `workflows/incident-triage.md` |
| `8c9f5233b1ac5b3050896bebe1ffb1d45cd6eca5ee88fbdc0d8003513252be5d` | 3458 | `workflows/vps/bin/collect-logs.sh` |
| `3ea378b095bcffafbb0e778f41779ea6118df41b5835391c233a4cd03c517354` | 2839 | `workflows/vps/bin/common.sh` |
| `fdfad41e442ab2514d1e73fb7f23c79937fa620aa45fc5d88e04422d3e6e41c9` | 7017 | `workflows/vps/bin/deploy.sh` |
| `7e3c9230e1d57d181e8392408a7cef0089c40ae22ae2782fa0b950e719f5c224` | 2988 | `workflows/vps/bin/edit-remote-file.sh` |
| `8d15bdc7b793b28af1d23886886c325a24dd56faf0b25bc61e0e0b5f4a7a04c2` | 5246 | `workflows/vps/bin/health-check.sh` |
| `127a9af96488da88277454c9ac5d5e1e899a94791c0a0b1d27f3e806d1f9ee11` | 4912 | `workflows/vps/bin/map-stack.sh` |
| `f57c4af9494d29e3a4b0de25e9c67a580b8ee84a4d2d5c1e1155fb57d5919019` | 2319 | `workflows/vps/bin/routine-check.sh` |
| `248cafb46158183c13683b57c8044678ae940a6c84f064e626d480a7ea1f628f` | 3429 | `workflows/vps/bin/service-action.sh` |
| `7bf1c73fd5dcf23c8c48929adf60c8b2fa05a904e710a7d7d0ac9a9fcb9f7101` | 1927 | `workflows/vps/bin/set-target.sh` |
| `cf97d393b6c235f39af1feb9aa32af4e1773743b18147a5875efac898918fd7b` | 1385 | `workflows/vps/bin/vps-ssh.sh` |
| `ac9aa5c895a4ec80a951b00eaf71c26aad238bec906435ad0072d5ee3bb9b59b` | 1217 | `workflows/vps/config.env.example` |
| `bb623a2ac9293327e6452685bb27c6715b721e46d143a9df78086847ca16d26c` | 1947 | `workflows/vps/services.list` |
| `ce4b4fa700ac1ac39fc2938244db156a0ae93ae79294ab8f2892c05714252432` | 1849 | `workflows/vps/stack-map.template.md` |
