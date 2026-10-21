<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Galion OS — security, privacy and anonymity threat model

Written 2026-10-21 06:38 CEST (+02:00) by wave-4 slot 143 (galion-os-threat-model).
Status: **design document, nothing built or deployed.** It is device-independent: it holds for any
Android phone Galion OS ends up on (the handset gate in PLAN §4 is still open). Where a control
depends on the phone's hardware or Android version, the fallback is stated.

Inputs: [PLAN.md](../plan/PLAN.md) §1–§9, the benchmark plan, and read-only probes of the current
server side (§9). This is the "threat model doc" exit criterion of PLAN Phase 5, written early on
purpose: several findings below (F1, F2, F7) change what Phases 2–4 should build.

---

## 1. What we protect (assets)

| ID | Asset | Why it matters | Where it lives |
|---|---|---|---|
| K1 | HADES admin private key | Signs admin actions for the whole swarm; loss = full admin takeover | Phone only (PLAN §8) |
| K2 | Bridge credentials (tunnel key, adb pairing keys, session ids) | Whoever holds them can drive the phone | Phone + server side |
| D1 | Daily-life data (chats, photos, contacts, 2FA apps, banking app session) | Owner's private life; never Galion's business | Stock Android |
| D2 | Banking session / Revolut material | Money; "scope ≠ authority to spend" | Banking app on stock Android |
| D3 | Gold image personal blob | A full copy of D1 + K-material if done wrong | Encrypted, server side (PLAN L2) |
| D4 | Audit log of remote actions | Evidence; but can itself hold private screen/text content | Phone + server |
| M1 | Identity metadata: phone number, IMEI, serial, Google account, GAID, device name, home IP | Anonymity goal | Everywhere on the wire / in logs / in git |
| A1 | Availability of the daily phone (coexist) | Owner must be able to call, pay, navigate | Phone |

## 2. Trust boundaries

```
 [stock Android apps] ─(1)─ [Galion app UID / sandbox] ─(2)─ [root (su) domain]
                                     │(3)
                         [admin bridge on phone] ─(4)─ network ─(5)─ [server edge (sshd / tunnel)]
                                                                       │(6)
                                        [Hermes / nerve / swarm agents / Grok Bot box] ─(7)─ [git / public repo]
```

1. App ↔ app: Android app sandbox (per-app UID + SELinux). Galion must not weaken it.
2. Sandbox ↔ root: only Galion-owned paths may use `su` (PLAN L1).
3. Sandbox ↔ bridge: what the bridge may do on the phone (adb shell has far more power than the sandbox).
4. Phone ↔ network: carrier / Wi-Fi / ISP see traffic.
5. Network ↔ server edge: server provider, CDN/tunnel provider.
6. Server ↔ agents: every agent with server access is inside this boundary today.
7. Everything ↔ public git: the repo is public by design.

## 3. Adversaries

| ID | Adversary | Capability | In scope? |
|---|---|---|---|
| T1 | Thief / finder of the phone | Physical access, time, can reboot into recovery | Yes |
| T2 | Malicious or compromised app on the same phone | Normal app UID, maybe accessibility/overlay permissions | Yes |
| T3 | Local network / carrier / ISP observer | Sees IPs, DNS, timing, mDNS on shared Wi-Fi | Yes (metadata only) |
| T4 | Attacker who gets the server (root or the tunnel account) | Everything that terminates on the server | Yes — **main risk**, see F2 |
| T5 | Compromised or prompt-injected agent / AI (Hermes, swarm agent, Grok Bot box) | Whatever tools the bridge exposes to it | Yes — see F7 |
| T6 | Infrastructure providers (server host, tunnel/CDN, Google/Xiaomi telemetry) | Network + platform metadata | Partly: minimise, cannot eliminate |
| T7 | Public-repo / OSINT reader | Reads git history, docs, CI logs | Yes |
| T8 | Supply chain (crates, root modules, root-hide modules, proot/rootfs images) | Code that runs as root or in the sandbox | Yes |
| T9 | Social engineering of the owner (fake pairing code, "approve this sign") | Owner's own taps | Yes |
| T10 | State-level adversary targeting the owner | Lawful intercept, carrier cooperation, 0-days | **Out of scope for v1** (owner decision D-OS5) |

## 4. Goals turned into testable properties

| Goal | Property (must be true, and tested) |
|---|---|
| Secure | S1 K1 is never readable as bytes outside the device's key store; S2 no remote action without an authenticated, unexpired, owner-bound session; S3 sandbox processes cannot reach the network except the allowlisted tunnel; S4 every remote input is audited before it is executed; S5 money-moving UI cannot be driven remotely (F7). |
| Private | P1 no D1/D2 content leaves the phone except inside the encrypted gold blob; P2 audit log stores action *kinds*, not typed text or screenshots; P3 git, CALL-RECORD and nerve events contain no M1 values. |
| Anonymous | N1 every server-side record of the phone uses a node id (ed25519 public key hash) and a rotating session id; N2 no M1 values in Hermes / nerve / tunnel logs; N3 identity is unlinkable *for T7 and T5*. Not promised: hiding the phone's IP from the server owner or from T6 (see F5). |
| Coexist | C1 sandbox off after reboot; C2 CPU/RAM caps; C3 banking apps keep working (root-hide); C4 no wakelock without a toggle. |

## 5. Findings that change the plan

**F1 — proot is not a security boundary (High).**
proot works by `ptrace` path rewriting; the guest runs as the *same* Android app UID as the Galion app,
with the same file access. A hostile program inside proot can reach everything that app UID can.
A root `chroot` is worse: it runs as root. Unprivileged user namespaces are disabled on most Android
kernels, so the namespace sandbox that works on a Linux server cannot be assumed on the phone.
→ Treat v1 isolation as: *a dedicated Galion app UID* (separate from any app holding K1 or the bridge) +
SELinux app domain + a seccomp filter installed by the launcher before exec + no `su` for sandbox
processes. proot is only a filesystem convenience. Keep the HADES signer and the bridge in a
**different** app/UID from the Linux sandbox. Prove with a device-independent test (qemu-aarch64 /
emulator): a sandboxed process tries to open the signer's files, `ptrace` its parent, open a socket to a
non-allowlisted address, and call `su` — all must fail. (Slot 144 galion-os-sandbox-proto is building the
launcher; this is its acceptance list.)

**F2 — adb over a reverse tunnel hands the phone to whoever holds the server (Critical).**
adb shell (uid `shell`) can inject input, install apps, read shared storage and capture the screen.
With PLAN L4 step 1 as written, a server compromise (T4) or any agent with server root equals full phone
control. Today's server facts (§9): SSH accepts key-only root logins, TCP forwarding is allowed, and
every agent on the shared Grok Bot box can use the server root key. → (a) the tunnel terminates on a dedicated unprivileged server
account with `permitopen` restricted to one loopback port, no shell, no root; (b) wireless debugging is
on only during an owner-present session and auto-off after it (timeout on the phone side, not trusting
the server to close it); (c) long-term, replace raw adb with the `galion-admin` bridge protocol: a small
command set, each command signed (HADES-style) and checked on the phone, so a stolen tunnel gives an
attacker a socket, not a shell; (d) swarm agents never get the adb key directly — they ask Hermes,
which asks the bridge.

**F3 — bootloader unlock removes verified boot (High, accepted risk).**
After unlock, T1 can boot a modified image (evil maid). File-based encryption still protects data at
rest while the phone is locked after reboot (before first unlock). → strong PIN/passphrase (not pattern),
"lockdown" habit when the phone leaves the owner's hands, K1 behind user authentication (F6), and a
written acceptance by the owner (D-OS1). Relocking with root is generally not possible; do not try.

**F4 — root-hide is an arms race and a supply-chain risk (High).**
Banking apps use device-integrity attestation; root-hide modules are third-party code running as root
(T8), and a failed hide breaks C3. → minimal set of root modules, pinned by hash and recorded in the
gold-image skeleton; every change re-tested with the banking app (PLAN L0); keep an unrooted fallback
(the plan's "unrooted twin"). If attestation can no longer be passed, coexist wins: Galion OS falls back
to a non-root mode (no `su`), it does not fight the bank.

**F5 — "anonymous" must be defined, or it will be over-promised (Medium).**
The tunnel's origin IP is the phone's carrier/Wi-Fi IP; the server's sshd logs record source addresses of
every connection (§9: ~1,250 accepted SSH sessions in 24 h are logged with source addresses). The tunnel/CDN
provider and the platform vendor (T6) also see metadata. → v1 anonymity means N1–N3: no identity in
Hermes/nerve/git, node ids everywhere, and unlinkability for readers and agents. Hiding the phone's
network location from the server owner (who is the phone owner) is not a goal; hiding it from T3/T6
would need Tor or a VPN chain — owner decision D-OS5, not v1.

**F6 — HADES key: hardware-backed, non-exportable, user-present (High).**
Root cannot extract a TEE/StrongBox key, but it **can use it** (signing oracle) while the phone is
unlocked. → generate K1 inside the Android key store as non-exportable; require user authentication
per use (biometric/PIN) for high-risk signs (enrolment, key rotation, anything touching money paths or
HADES security mode); low-risk signs may use a short auth window. Ed25519 in the platform key store needs
Android 13+; on older phones the fallback is ECDSA P-256 in TEE/StrongBox, which means the HADES verifier
must accept P-256 too (owner decision D-OS3). Never a software key file; never in adb backup (the plan's
"exclude keystore" stands). Backup policy = a second enrolled key on a second device, not a copy of K1.

**F7 — screen steering bypasses the "no money moves" rule (Critical).**
The API ban on Revolut writes does nothing if an agent can tap "Send" in the banking app over scrcpy.
On-screen text (SMS, notifications, web pages, chat) can prompt-inject an AI that sees the screen (T5).
→ the bridge enforces, on the phone: (a) input injection is refused while the foreground package is in
a deny-list (banking, payment, authenticator, password manager, system settings for accounts/security);
(b) remote steer only with the owner present (PLAN L4 step 2) and a visible on-screen indicator;
(c) screen capture is blanked for the same deny-list (FLAG_SECURE apps already blank; do not bypass it);
(d) the audit log records each injected action *before* it runs (S4). This is the single most important
control in this document.

**F8 — the gold image is a full copy of the owner's life (High).**
Server file permissions cannot be the confidentiality control: the server already has a world-readable
config file found by an earlier scenario run (mode 666, outside this repo). → encrypt the personal blob
**on the phone** before upload with a key that never reaches the server (e.g. derived from an owner
passphrase, or a second device's key); the server only stores ciphertext + checksum; the public
skeleton is generated from an allowlist of fields, not by deleting fields from a full dump.

**F9 — audit logs leak what they record (Medium).**
Recording every tap is right; recording what was typed or what the screen showed is a privacy leak.
→ log action kind, coordinates/keycode class, foreground package, session id, node id, Zagreb timestamp;
never typed text, clipboard, or screenshots. Retention period is an owner decision (D-OS4).

**F10 — wireless debugging is chatty on the local network (Low/Medium).**
Android's wireless debugging advertises itself over mDNS on the local Wi-Fi with a service name derived
from the device, and pairing codes can be shoulder-surfed. → pair only on a trusted network, turn
wireless debugging off after the session, prefer the reverse tunnel started from the phone (outbound)
over any inbound listener.

**F11 — the box with adb is part of the phone's trust base (High).**
The Grok Bot box is one shared machine running many agents. Any adb/scrcpy key placed there is usable by
all of them. → no persistent adb key on the box; per-session pairing that expires; bridge commands go
through Hermes with HADES-style signatures instead.

## 6. Threats per component (STRIDE short form)

| Component | Spoofing | Tampering | Repudiation | Info disclosure | DoS | Elevation | Main control |
|---|---|---|---|---|---|---|---|
| L0 root / boot | evil-maid image (T1) | modules (T8) | — | data at rest after unlock | bootloop | root by design | F3, F4, pinned modules |
| L1 galion-admin CLI | fake CLI binary | config edit | — | logs | — | `su` misuse | signed builds, `su` only for Galion paths |
| L2 gold image | — | image swap on server | — | personal blob | — | restore of hostile image | F8 + checksum signed by K1 |
| L3 sandbox | — | rootfs from untrusted mirror | — | same-UID file reads | CPU/RAM/heat | escape to app UID / root | F1, cgroup caps, off by default |
| L4 bridge / adb | stolen tunnel, fake pairing (T9) | command injection | who tapped? | screen content | tunnel flapping | adb shell power | F2, F7, F9, F11 |
| HADES key | forged signs | — | signs without trace | key bytes | key store lockout | signing oracle under root | F6 |
| Hermes / nerve events | fake node | event forgery | — | M1 in logs | event flood | — | node id + signature, N1–N2 |
| Benchmarks / Revolut | mock vs live confusion | fixture tamper | — | session material | — | live write path | mocks only; F7 deny-list |
| Public repo | — | malicious PR | — | M1 / server address in history | — | CI secrets | CONTRIBUTING rules, scan in CI (§7) |

## 7. Controls and how each is tested (device-independent first)

| # | Control | Test (before any real phone) | Phase |
|---|---|---|---|
| C-01 | Sandbox cannot read signer/bridge files, cannot `ptrace`, cannot `su`, cannot open non-allowlisted sockets | Rust GALION_REAL=1 test under qemu-aarch64 and an Android emulator (F1) | 3 |
| C-02 | Sandbox off after reboot | Emulator reboot test: no Galion process after boot until toggled | 3 |
| C-03 | Bridge refuses injection while a deny-listed package is foreground | Emulator with a dummy app named like a banking package; injected tap must be refused + audited | 4 |
| C-04 | Bridge commands must carry a valid signature and unexpired session | Replay, wrong key, expired session, reordered command — all refused | 4 |
| C-05 | Tunnel account cannot open a shell or forward to anything but one port | Read-only `sshd -T -C user=<tunnel>` check + a negative connect test, on a throwaway server account, never the live root account | 4 |
| C-06 | K1 non-exportable, auth-bound | Emulator key store: export attempt fails; sign without auth fails for high-risk class | 4 |
| C-07 | Gold blob is ciphertext on the server | Upload a synthetic blob; server-side check that it does not parse as an archive and the key is absent from the server | 2 |
| C-08 | No M1 values in git / docs / events | CI scan (Rust tool) for IMEI-like (15 digits + Luhn), serial-like, phone-number, IPv4 public, e-mail patterns, with an allowlist; run on every branch | now |
| C-09 | Audit log has no typed text / screenshots | Inject a known marker string via the bridge; grep the log for it → must be absent | 4 |
| C-10 | Coexist: jank/thermal | Emulator first, then real phone with owner present (PLAN Phase 3) | 3 |

## 8. Owner decisions (Slavik)

| ID | Decision | Recommendation |
|---|---|---|
| D-OS1 | Accept that bootloader unlock removes verified boot (evil-maid risk) | Accept, with strong PIN and F6 user-auth on K1 |
| D-OS2 | Raw adb over the tunnel as a stop-gap, or wait for the signed bridge protocol | Raw adb only in owner-present sessions on a dedicated non-root tunnel account; signed bridge before any unattended use |
| D-OS3 | HADES on phones without platform Ed25519: accept ECDSA P-256 in the verifier? | Yes, verifier accepts both; Ed25519 preferred |
| D-OS4 | Audit-log retention | 90 days on phone, 1 year server-side (action kinds only) |
| D-OS5 | Network anonymity (Tor/VPN chain) and state-level adversary | Out of scope for v1; revisit after Phase 5 |
| D-OS6 | Deny-list for remote input (F7): which apps | Banking/payment, authenticators, password managers, account/security settings; owner can add, agents cannot remove |
| D-OS7 | Gold-image encryption key custody | Owner passphrase + second-device key; never on the server |

## 9. Evidence: read-only server probes (2026-10-21 06:35 CEST, +02:00)

Server-side facts this model relies on. Read-only (status reads, `sshd -T`, journal counts, nerve GET).
No addresses, hostnames or secrets recorded here, only counts and settings.

| Probe | Result | Used in |
|---|---|---|
| Kernel LSMs | `lockdown,capability,yama,apparmor` — no Landlock, so server-side sandboxing relies on namespaces | F1 (phone may have even less) |
| Listening TCP sockets | only SSH on all interfaces; 14 service ports on 127.0.0.1 plus the local DNS stub and loopback mail | F2 (tunnel must stay loopback) |
| `sshd -T` | `permitrootlogin without-password`, `passwordauthentication no`, `kbdinteractiveauthentication no`, `allowtcpforwarding yes`, `gatewayports no`, `x11forwarding yes`, `maxauthtries 3` | F2, C-05 |
| SSH journal, last 24 h | 9,122 lines; 502 failed/invalid-user attempts; 1,254 accepted sessions (all logged with source address) | F2, F5 |
| fail2ban | active | F2 |
| Nerve `/state` (13,091 bytes, 15 components) | 0 non-loopback IPv4 values; `host` and `tunnel` fields null; component names are service names only | N1/N2 are met today for the server side; a phone node must keep it so |
| Hermes journal, last 24 h | no non-loopback IPv4, no phone-number-like strings | N2 |
| Phone-image directory planned in PLAN L2 | does not exist yet | F8 can be designed before anything is stored |
| A config file outside this repo | world-readable (mode 666), already reported by an earlier scenario run | F8 (do not rely on server permissions) |

Cortex is not deployed on the server; the nerve was used for observation (standing rule
no_skip_extensive_testing).

## 10. What this document does not do

- No phone was touched; no handset model is assumed.
- Nothing on the server was changed. Hardening the live SSH configuration (F2) is a separate owner
  decision and a separate task; this doc only recommends a dedicated tunnel account.
- No code. C-08 (identity-pattern scan) is the first control that can be built now, in Rust.
