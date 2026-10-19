<!-- SPDX-License-Identifier: MIT -->
# Phone instance: `omnode` + phone voice client (v0.1)

| Field | Value |
|---|---|
| Written | 2026-10-18 20:35 CEST (+02:00), Europe/Zagreb |
| Updated | 2026-10-18 21:10 CEST (+02:00), Europe/Zagreb |
| Status | **v0.1.1: sandbox hardened (seccomp + mount/user/net ns), `doctor`, install scripts, release tarballs, pair --dry-run. Still not on any phone. Nothing deployed on the VPS.** |
| Code | `edge/openmind-node/` (crate `openmind-node`, binary `omnode`, Rust, MIT) and `apps/phone-voice/` (PWA, plain HTML/JS) |
| Goal | goals.md, "Local network instances (plug-in interface)"; goals.md, "Project: Galleon Voice to Voice" |
| Related | `docs/persona-plex-v2-design.md`, `crates/persona-plex-relay/`, `docs/agentic-sandbox.md` (B26), `crates/openmind-sandbox/` (B27) |

## 1. What it is

The phone instance is the part of the Obsidian Open Mind Stack that runs **on a device inside Slavik's home network**: an Android phone (Termux), or a Linux laptop or Raspberry Pi. It has two parts:

1. **`omnode`**, a small Rust binary (about 1.2 MB, about 2.5 MB RSS). It gives the device an identity, recognises the trusted home network, discovers other devices on that LAN politely, advertises itself as `_openmind._tcp`, prints a pairing payload for Hermes, and runs agent tool commands in a CPU-only sandbox.
2. **`phone-voice`**, a tiny PWA (about 17 KB). It streams microphone audio to the Persona Plex relay on the VPS and plays the answer back.

Why it exists: the VPS is a data-centre LXC container. It **cannot see** Slavik's Wi-Fi, phone or LAN devices. Anything LAN-related has to run on a device that is on that LAN. The VPS only ever receives what that device chooses to send, after the owner approves it.

### Where the crate lives (deviation from the request)

The request asked for `crates/openmind-node/` with its own empty `[workspace]`. That breaks the OpenMind workspace: the root `Cargo.toml` has `members = ["crates/*"]`, and Cargo then fails with "multiple workspace roots found" (reproduced in a scratch copy). Without its own `[workspace]` it would silently join the workspace and change the tracked `Cargo.lock`. So the crate lives at **`edge/openmind-node/`**, which the glob does not match. It still has its own empty `[workspace]` table, and the root `Cargo.toml` is untouched. `cargo metadata --no-deps --locked` at the root still lists the same members, without `openmind-node`. Moving it under `crates/` later needs a one-line `exclude` in the root `Cargo.toml`. That is Hermes's call.

## 2. Command overview

```
omnode identity init [--no-bind] | show | bind [--force] | network
omnode discover [--seconds N] [--dry-run] [--mdns-query] [--no-msearch] [--probe] [--probe-subnet]
omnode announce [--seconds N] [--dry-run]
omnode pair [URL] [--no-qr] [--dry-run]
omnode sandbox run [--wall S] [--cpu S] [--mem MB] [--allow-net] [--require-isolation]
              [--no-landlock] [--no-seccomp] [--no-namespaces] [--keep] -- CMD...
omnode sandbox check
omnode doctor
omnode version
```

Config file: `~/.config/openmind/node.toml` (template `omnode.example.toml`), or `--config FILE` / `$OMNODE_CONFIG`. No host is hard-coded. The enroll URL, relay URL and stack salt all come from config or the command line. Unknown keys are rejected. All timestamps are Europe/Zagreb RFC 3339 with offset (`2026-10-18T20:28:47+02:00`), never `Z`. They come from a built-in EU DST rule, which is unit-tested across the 25 Oct 2026 and 28 Mar 2027 switches.

## 3. Identity and "Wi-Fi identity"

- `identity init` generates an **ed25519** keypair (`ed25519-dalek`, OS randomness). The 32-byte seed goes to `~/.config/openmind/node.key` with mode **600**, in a directory with mode **700**. It is created with `create_new`, so an existing key is never overwritten. `load` refuses a key file readable by group or other (the ssh rule).
- `node-state.toml` (mode 600) holds only public data: device id, public key, creation time, a per-device salt and the list of trusted network hashes.
- `identity show` prints **only** the fingerprint (`SHA256:xxxx-...`, the first 16 bytes of SHA-256 over the public key) and the device id (`omn-` + 12 hex characters).
- **Wi-Fi identity** means the device key is bound to the current network. omnode observes the network with whatever the platform allows:
  - Termux: `termux-wifi-connectioninfo`, which needs the Termux:API app and the location permission. Android masks SSID and BSSID without that permission (`<unknown ssid>`, `02:00:00:00:00:00`); both are treated as unknown.
  - Linux: `iw dev <if> link`, then `nmcli -t`, then `/proc/net/wireless` and `/sys/class/net/*/wireless` for interface names.
  - Gateway: `/proc/net/route`, else `ip route` (a route lookup only, nothing is contacted).
  - Gateway MAC: `/proc/net/arp`, else `ip neigh`. Android 10+ usually denies `/proc/net/arp`, and then the BSSID is used instead.
  - Every helper call has a 4 s timeout, and every step degrades gracefully. With nothing readable, the network is simply "unbound".
- omnode stores only `hash = SHA-256("omnet1\0" || salt || SSID || "\0" || gateway MAC)`, truncated to 32 hex characters. The anchor degrades in this order: `ssid+gateway_mac`, then `ssid+bssid`, then `gateway_mac`, then `bssid`, then `ssid`. **The raw SSID and MACs are never written to disk, advertised, or put into the pairing payload.** Tests check that the state file does not contain them.
- Salt: by default per device, so hashes cannot be linked across devices. A stack-wide salt (`[network] salt_hex`, at least 16 bytes) can be handed out at enrollment, so Hermes can tell that two devices are on the same trusted network.
- `identity init` binds the current network **only if its gateway is a private (RFC 1918) address**. On the VPS (public gateway) it correctly refused. `identity bind` adds a network later; `--force` is for a network the owner really controls but that has a public gateway.

## 4. Discovery: what it does and does not do

**Does (default `omnode discover`):**
- Passive mDNS/DNS-SD listening on UDP 5353 (group 224.0.0.251) for N seconds (default 5, max 120). This uses its own small defensive DNS parser: compression-loop guard, bounds checks, and a truncation fuzz test.
- Passive SSDP NOTIFY listening on UDP 1900 (group 239.255.255.250).
- **One** SSDP M-SEARCH packet (`--no-msearch` or `ssdp_msearch = false` turns it off). LOCATION URLs are reported but **never fetched**.
- Reads `/proc/net/arp`, and local CPU, cores, arch and RAM from `/proc` and `available_parallelism`.
- JSON output: `self`, `network` (booleans and the hash only, never the SSID), `arp`, `hosts` (IP, MAC, mDNS names, services, SSDP flag), `services` (instance, type, host, port, TXT, addresses), `service_types`, **`openmind_peers`** (any `_openmind._tcp`), `ssdp`, `notes`.

**Opt-in only:**
- `--mdns-query`: **one** active mDNS query packet, asking for `_services._dns-sd._udp.local.` and `_openmind._tcp.local.` (PTR). It is hand-built, because the browse function of the `mdns-sd` crate re-queries with backoff. `mdns-sd` is used for `announce`.
- `--probe`: a TCP connect probe of a short port list. The default list is 22, 80, 443, 8080, 11434 (ollama), 50052 (llama.cpp `rpc-server`) and 52415 (exo), at most 16 ports. By default it probes **only hosts already seen** (ARP, mDNS, SSDP) on the local /24. `--probe-subnet` widens that to .1-.254 of the local /24. Rate limit: 20 connects/s by default, capped at 50; timeout 300 ms. It is **refused** unless the current network is bound as trusted and the device has a private IPv4 address.

**Does not:** port-scan by default, touch anything outside the local /24, fetch UPnP descriptions, send ARP or ICMP sweeps, or run on the VPS. `--dry-run` opens **no sockets at all** and prints the plan instead.

**Android caveat:** Android drops Wi-Fi multicast for apps that do not hold a `MulticastLock`. Termux holds none, so passive mDNS/SSDP may see little on the phone. ARP (where readable), the opt-in probe, and discovery from a Linux device on the same LAN still work. The JSON notes say this when running under Termux.

Tested: on the build box, `announce` in one process and `discover --mdns-query` in another found the `_openmind._tcp` peer with all TXT fields. On the VPS only `--dry-run` was run.

## 5. Announce

`omnode announce [--seconds N]` advertises `<device-id>._openmind._tcp.local.` through `mdns-sd`. The host is `<device-id>.local.` with automatic addresses. TXT: `v`, `id`, `fp`, `arch`, `cores`, `ram_mb`, `platform`, `caps` (default `cpu,sandbox,voice-client`), and `name` if one is set. The SRV port (default 7787) is a placeholder: **nothing listens there in v0.1**. The network hash is not advertised. `--dry-run` prints the record without opening a socket. `--seconds 0` (the default) runs until stopped.

## 6. Pairing flow

1. **Now (v0.1, stub):** on the phone, `omnode pair https://<hermes-host>/openmind/enroll` prints:
   - a JSON payload with: device id, public key, fingerprint, network hash, whether that network is trusted, capabilities, arch, cores, RAM, platform, enroll URL, a 96-bit nonce, and created and expiry times (+10 min, Zagreb);
   - an ed25519 signature over a canonical `|`-joined string, which binds it to the intended URL and nonce;
   - a terminal QR (65 columns, EC level L) of a signed short form: `omp1|id|pk(b64url)|net|caps|nonce|expires_unix|sig(b64url)`, about 250 characters.
   - **Nothing is sent.** It only prints what it would POST. The URL must be `https://`; `http://` is allowed with a warning, for a LAN test only.
2. **Later (enrollment by Hermes, to build):** the phone POSTs the payload. Hermes verifies the signature, checks expiry and nonce reuse, and shows the fingerprint to Slavik. **No device joins without Slavik's explicit approval** (goals.md). On approval Hermes stores the public key as an OpenMind Provider row, optionally returns the stack salt, and later sets up a tunnel. The phone dials out to the VPS (for example an SSH reverse tunnel or WSS); the VPS never reaches into the LAN.
3. After enrollment, requests from the VPS to the node are signed or verified with the device key, and the key never leaves the device.

## 7. Sandbox: guarantees and limits per platform

`omnode sandbox run -- CMD` runs one agent tool command and prints a JSON report: exit code or signal, `timed_out`, duration, capped stdout and stderr, and an `isolation` block that says exactly what was enforced.

| Layer | Linux (VPS LXC, measured) | Linux laptop/Pi (build box, measured) | Android/Termux (expected) |
|---|---|---|---|
| Fresh temp work dir, mode 700, cwd = HOME = TMPDIR, removed afterwards | yes | yes | yes (`$TMPDIR` = Termux tmp) |
| Cleared env (only PATH, HOME, TMPDIR, LANG, TERM) | yes | yes | yes |
| stdin `/dev/null`, stdout and stderr captured, capped (256 KB default) | yes | yes | yes |
| rlimits: CPU s, address space, file size, nofile, core = 0 | yes | yes | yes |
| Own process group; SIGKILL to the group on wall timeout and after exit | yes | yes | yes |
| No network: user + net namespace (empty netns, only `lo`), uid/gid mapped 1:1 | **yes** | **yes** | no (namespaces unavailable to apps) |
| Landlock fs (read/exec system dirs; read-write only the work dir) + TCP deny (ABI >= 4) | **no** (kernel has no Landlock; reported) | **yes**, ABI 6, fully enforced | usually no (kernel config); reported |
| Fallback | n/a | n/a | rlimits + restricted work dir; `network: "NOT isolated"` in the report |

Defaults: wall 30 s, CPU 20 s, memory 512 MB, file size 64 MB, nofile 256, output 256 KB, no network. `--require-isolation` fails instead of falling back. `--allow-net` skips network isolation.

**Not guaranteed:** this is not a boundary against a hostile program. There is no PID namespace yet (deferred; see merge proposal), no disk quota (many files can still fill the disk), and `RLIMIT_AS` is per process. Without Landlock, the command can read anything the user can read. It is a safety net for sloppy or prompt-injected agent commands, which matches the B26 threat model in `docs/agentic-sandbox.md`. `crates/openmind-sandbox` (B27, another job) implements the v0 library. omnode's sandbox already covers the B26 "v1" network-namespace step; the two should be merged into one crate later (see to-do).

## 7b. v0.1.1 sandbox hardening (2026-10-18 21:05 CEST (+02:00))

Added on top of v0.1:

- **seccomp-BPF deny-list** (raw libc, no extra crate): ptrace, mount/umount, module load, bpf, keyctl, kexec, process_vm_*, etc. return EPERM; foreign syscall ABI kills the process. `PR_SET_NO_NEW_PRIVS` always. Report field `seccomp: "deny-list"|"not-enforced"|"disabled"`.
- **Mount namespace** with `MS_REC|MS_PRIVATE` when user namespaces work.
- **User + net namespaces** unchanged (empty netns = real no-network).
- **PID namespace deferred**: unprivileged NEWPID + double-fork broke wall-timeout process-group kills (grandchild orphaned). Documented in `edge/openmind-node/docs/sandbox-merge-proposal.md`. Report notes say so.
- Merge proposal (do not merge): unify with Hermes `crates/openmind-sandbox` — see that doc.

Measured on the build box after hardening: `sandbox check` reports user_namespace, net_namespace, mount_namespace, landlock fully-enforced (ABI 6), seccomp deny-list. On the VPS LXC: same except landlock ABI 0 (not in LSM list).

## 8. Voice flow

```
phone PWA (apps/phone-voice)  --WSS, Opus/Ogg kind 0x01-->  VPS persona-plex-relay  --tunnel-->  GPU PersonaPlex (RunPod)
        ^   speaker status 0x40, fallback 0x41, text 0x02          |
        +---------------- Opus/Ogg 0x01 back ----------------------+
fallback: relay sends 0x41 (GPU off/busy/lost)  ->  PWA text box  ->  Galion One text endpoint (config `text_url`)
```

- The PWA speaks the relay's wire protocol (`crates/persona-plex-relay/src/protocol.rs`, the moshi kinds 0x00-0x07 plus the relay-private kinds 0x40-0x42). It sends only kind 0x01: Opus at 24 kHz mono (WebCodecs `AudioEncoder`, 32 kbit/s, 20 ms frames) in Ogg pages that start with OpusHead and OpusTags pages. It waits for the 0x00 handshake before sending. The Ogg muxer and demuxer were checked against ffmpeg: ffmpeg decodes the PWA's Ogg, and the PWA parses ffmpeg's Ogg with matching CRCs.
- Push-to-talk gates the mic **to silence, not to nothing**. The model steps once per 80 ms input frame, so the stream has to keep flowing. Full duplex streams continuously; tapping mutes it (also as silence). Echo cancellation is on; use headphones.
- Status pills: connection, mic, backend (gpu / text fallback with reason), speaker (verified / uncertain / mismatch / listening, with score), driven by relay message 0x40. Until the relay sends them, they are placeholders.
- Text box: always goes to Galion One (`text_url`), because PersonaPlex takes no text input.
- Config: `config.json` next to the page (template `config.example.json`) or the in-page Settings (stored in the phone's localStorage). No host is hard-coded. Optional bearer token: browsers cannot set headers on WebSockets, so it is sent as a second subprotocol, `bearer.<token>`. **This is a draft; the relay has to agree to it.** A device-key challenge signed by omnode is the better long-term option.
- **Browser microphone access requires HTTPS** (a secure context); `http://` works only on `localhost`. Options: (a) an approved TLS hostname in front of the relay and the PWA, or (b) for testing, an SSH tunnel from the phone (Termux `ssh -L`) plus serving the page on `localhost`. WebCodecs Opus needs current Chrome or Edge on Android; Firefox on Android is unverified.
- Not deployed or served anywhere.

## 9. Security and privacy notes

- Scanning is only for networks the owner controls. The probe is opt-in, rate-limited, limited to the local /24, and refused on networks that are not bound. Auto-binding is skipped when the gateway is public.
- The private key stays on the device (mode 600, never printed, never in payloads; tested). Pairing sends only the public key and a signature.
- The raw SSID and MACs are never stored or advertised. Only salted hashes are kept; the discover output carries booleans (`ssid_known`) and the gateway IP.
- The VPS never reaches into the LAN; the phone dials out after approval.
- Voice is a soft identity signal only (persona-plex-v2 design section 6). Money is always denied. **Revolut / real money: NOT approved.**
- Rust only, no Python anywhere in this work.

## 10. Build and measurements (VPS, 2026-10-18 20:27-20:31 CEST (+02:00))

All builds ran with `nice -n 19 cargo ... -j 2`, `CARGO_TARGET_DIR=/root/.cache/omnode-target` (outside the repo).

| Item | Result |
|---|---|
| `cargo build --release` x86_64-unknown-linux-gnu | 26.6 s cold (peak RSS 294 MB); binary **1,271,832 bytes (1.21 MiB)**, `opt-level="z"`, LTO, stripped |
| `cargo build --release --target aarch64-unknown-linux-musl` | `rustup target add` + `rust-lld` linker, no other installs; 22.9 s; **1,195,384 bytes (1.14 MiB), static, AArch64 ELF**; not yet run on a device |
| `cargo test --release` | **46 passed, 0 failed** (VPS); same on the build box |
| `cargo clippy --all-targets -D warnings`, `cargo fmt --check` | clean |
| RSS `identity init` / `identity show` (temp HOME) | 2.3 MB / 2.4 MB peak |
| RSS `sandbox run -- echo ok` | 2.4 MB peak (omnode process); stdout `ok`, exit 0, 11 ms, empty netns |
| RSS `discover --dry-run --seconds 3` | 2.4 MB peak; opened no sockets |
| RSS live `discover --seconds 3` (build box only, 3 listener threads) | 3.2 MB peak |
| RSS `announce` (build box only) | 3.1 MB peak |

| v0.1.1 musl x86_64 / aarch64 (build box, 2026-10-18 21:06 CEST (+02:00)) | **1,413,952** / **1,205,512** bytes; packaged under `release/omnode-0.1.0/` with sha256 |
| RSS `sandbox check` after hardening (build box) | ~2.5 MB; seccomp deny-list + mount/user/net ns |

Phone build options: copy the static aarch64 binary into Termux (`chmod +x omnode`), or build on the phone with `pkg install rust` then `cargo build --release` in `edge/openmind-node`.

## 11. Works now vs stub

| Works now (tested) | Stub / not yet |
|---|---|
| identity init/show/bind/network, key 600, salted network hash | enrollment endpoint on Hermes (pair sends nothing) |
| discover: passive mDNS/SSDP, 1 M-SEARCH, ARP, self info, opt-in 1-packet query, opt-in rate-limited probe, dry-run | discovery from a real phone (Android multicast caveat) |
| announce `_openmind._tcp` (found by discover on the build box) | a listener on the SRV port (none in v0.1) |
| pair: signed payload + QR | tunnel from the phone to the VPS |
| sandbox: work dir, env, rlimits, group kill, user+net+mount ns, Landlock, seccomp deny-list | PID namespace (deferred), disk quota |
| PWA: UI, Opus/Ogg framing, relay kinds 0x00/0x01/0x02/0x05/0x40/0x41/0x42 | real relay listener and GPU (both off); bearer-token transport agreed with the relay |

## 12. Local-device kit (v0.1.1)

- `edge/openmind-node/install/linux.sh` — POSIX sh; fetch release tarball (`OMNODE_RELEASE_URL`) or build from source; `identity init`; `doctor`.
- `edge/openmind-node/install/termux.sh` — same for Termux; tips for Termux:API + Location permission.
- `omnode doctor` — JSON: self (CPU/RAM/arch), identity, wifi (SSID readable? via which tool?), kernel (userns, landlock ABI, seccomp available, ARP readable), sandbox probe, multicast note, advice list. No listeners.
- `omnode pair --dry-run <url>` — prints the HTTP POST plan (method, URL from config/CLI only, headers, body) and sends nothing.
- Release artifacts: `/opt/galion/openmind/release/omnode-0.1.0/` — static musl tarballs for x86_64 and aarch64 with sha256 sums.

## 13. To-do

- [x] 2026-10-18 21:10 CEST (+02:00): Harden sandbox (seccomp, mount/user/net ns); PID deferred with merge proposal.
- [x] 2026-10-18 21:10 CEST (+02:00): `omnode doctor`, install scripts, release tarballs, `pair --dry-run`.
- [ ] 2026-10-18 20:35 CEST (+02:00): Slavik: install Termux (F-Droid) + Termux:API on the phone, or name a home Linux device (laptop/Pi) for the first real run.
- [ ] 2026-10-18 20:35 CEST (+02:00): Run `omnode identity init`, `discover --seconds 5`, `announce --seconds 30` and `sandbox check` on that device; record results here.
- [ ] 2026-10-18 20:35 CEST (+02:00): Decide the HTTPS hostname (TLS front end) for the PWA and relay, or approve the SSH-tunnel-to-localhost test path. Needs Slavik's approval.
- [ ] 2026-10-18 20:35 CEST (+02:00): Provision RunPod GPU for PersonaPlex (needs RUNPOD_API_KEY and approval; see persona-plex-v2 design).
- [ ] 2026-10-18 20:35 CEST (+02:00): Hermes: build the enrollment endpoint (verify signature, expiry, nonce; approval prompt; Provider row; optional stack salt).
- [ ] 2026-10-18 20:35 CEST (+02:00): Agree the bearer/subprotocol (or device-key challenge) client auth with persona-plex-relay.
- [ ] 2026-10-18 20:35 CEST (+02:00): Merge the omnode sandbox with crates/openmind-sandbox (B27) into one crate.
- [ ] 2026-10-18 20:35 CEST (+02:00): Hermes: decide whether to move edge/openmind-node under crates/ (needs an `exclude` in the root Cargo.toml) or keep it in edge/.
- [ ] 2026-10-18 20:35 CEST (+02:00): Phone-to-VPS tunnel design (outbound only) after enrollment.
