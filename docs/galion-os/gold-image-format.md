<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Galion OS gold image format, v1

Status: design + reference implementation (wave-4 slot 204, 2026-10-21 18:30 CEST +02:00).
Code: `crates/galion-admin/src/image.rs` (verify, builder functions),
`galion-admin image verify` (device side, read-only), `examples/gold_image.rs` (builder side:
keygen, manifest, sign; not shipped on the device). Device-independent: nothing here touches a
phone, a bootloader or a partition. No flashing.

## 1. What a gold image is

A **gold image** is the known-good file set a Galion OS device is restored to and checked
against (system binaries such as `galion-admin` and `galion-box`, configuration, data). v1 is a
**directory** of regular files plus two metadata files at its top level:

```
<image>/
  galion-image.manifest   canonical text: header + one line per payload file
  galion-image.sig        Ed25519 signature over the manifest
  bin/galion-admin        payload ...
  etc/galion/os.toml
```

The manifest pins every payload byte (SHA-256), size and permission bits; the signature pins
the manifest. Anything not listed is a failure. A container format (tar / partition image) can
wrap this directory later without changing the manifest (see section 8).

## 2. Manifest (`galion-image.manifest`)

ASCII, `\n` line ends, exactly one canonical byte form (the verifier re-serialises the parsed
manifest and refuses anything that is not byte-identical: CRLF, leading zeros, upper-case hex,
double spaces, trailing data, missing final newline).

```
galion-gold-image v1
name galion-os-gold
version 0.1.0-s204
arch x86_64
build.commit bf33cba
build.rustc rustc 1.99.1 (58cf8f9c3 2026-10-13)
build.time 2026-10-21T18:21:14+02:00
files 7
bytes 4402300
file 0755 946712 <64 hex sha256> bin/galion-admin
file 0600 0 e3b0c442...b855 etc/galion/empty.conf
...
end
```

| Line | Rule |
|---|---|
| `galion-gold-image v1` | magic + format version; anything else is refused |
| `name` | 1-64 of `a-z A-Z 0-9 . _ -`, starts alphanumeric |
| `version` | 1-32 of `A-Z a-z 0-9 . + -` |
| `arch` | 1-16 of `A-Z a-z 0-9 _` (`x86_64`, `aarch64`, `armv7`) |
| `build.commit` | 7-64 lower-case hex (git SHA of the source the image was built from) |
| `build.rustc` | printable ASCII, 1-128, no leading/trailing space |
| `build.time` | `YYYY-MM-DDTHH:MM:SS+HH:MM`, explicit offset (Zagreb: `+02:00` / `+01:00`); `Z` is refused (project rule) |
| `files N` | number of `file` lines, at most 100 000 |
| `bytes N` | sum of all sizes; must match exactly |
| `file MODE SIZE SHA256 PATH` | MODE 4 octal digits, permission bits only (setuid/setgid/sticky refused); SIZE decimal, no leading zeros; SHA256 64 lower-case hex; PATH see below |
| `end` | last line; a cut-off manifest never parses |

Header keys appear exactly once, in the order above; unknown keys are refused (a v2 needs a new
magic line).

**Paths.** Relative, `/`-separated; each component 1+ characters of `A-Z a-z 0-9 . _ + @ -`,
never empty, `.` or `..`; whole path at most 255 bytes; `galion-image.manifest` and
`galion-image.sig` are reserved. File lines are in strictly increasing **byte** order (no
duplicates); a path may not be both a file and a parent directory of another file. Directories
are implicit (the parents of listed files); empty directories are not part of an image.

**Only regular files.** Symlinks, device nodes, FIFOs and sockets cannot be listed; the builder
refuses to create an image containing them and the verifier fails an image that has them.
(Symlinks needed on a device are created by the installer from configuration, not shipped.)

## 3. Signature (`galion-image.sig`)

```
galion-gold-image-sig v1
key cd0abef23b3d28a5
ed25519-sig <128 lower-case hex>
```

* Signed message = `galion-os/gold-image/v1\n` (domain) followed by the **exact manifest bytes**.
  The domain differs from hades-gate approvals (`galion-hades-gate/approval/v1\n`), so a HADES
  approval signature never verifies as an image signature and vice versa (unit test
  `hades_approval_signature_never_verifies`).
* Verification uses `verify_strict` (ed25519-dalek 2.2.0): no malleable signatures, no
  small-order keys or R points.
* `key` = first 16 hex of SHA-256(32-byte public key): the same fingerprint hades-gate prints
  (`admin_fingerprint`; checked 18:22: `hades-gate keygen` printed `e9a2596e1b6c0de0`,
  `gold_image sign` with that seed printed `signed by key e9a2596e1b6c0de0`). It only selects
  the error message ("wrong key" vs "bad signature"); trust comes from the signature check.

## 4. Keys: the existing hades-gate format

| File | Content | Mode | Lives |
|---|---|---|---|
| public | `ed25519-pub <64 lower-case hex>\n` | 0644 | on the device / verifier |
| seed | `ed25519-seed <64 lower-case hex>\n` | 0600, never overwritten | with the image signer only |

Same tags, same strict hex, same weak-key refusal and the same refusal to read anything under
`/opt/galion/galion-light/secrets` or `/root/galion-secrets` as `hades-gate keys.rs`. A key made
by the real `hades-gate keygen` (integration-5 0d68f46) is used by the real test. The verifier
never reads a seed (a seed passed as `--key` is refused: "not a ed25519-pub file", exit 1) and
never prints key material, only the fingerprint. **Whether the image key is the HADES admin key
or a separate image-signing key is owner decision D-IF1** (format is identical either way).

## 5. Verification order (`galion-admin image verify DIR --key PUBFILE [--json]`)

1. Read `galion-image.sig` (≤ 1 KiB) and `galion-image.manifest` (≤ 8 MiB) with `O_NOFOLLOW`;
   both must be regular files.
2. Parse the signature file strictly; compare the key fingerprint (mismatch → **wrong key**).
3. `verify_strict(domain ‖ manifest)` (failure → **bad signature**). Nothing in the manifest is
   trusted or even parsed before this point.
4. Strict canonical parse of the manifest (signed but malformed → **rejected**).
5. Walk the directory without following symlinks: symlink / special file → `not-regular`; file
   not listed → `unlisted`; directory holding no listed file → `unlisted-dir`; depth > 32 → fail.
6. For every listed file: open `O_NOFOLLOW|O_NONBLOCK`, `fstat` must be a regular file, stream
   at most `size + 1` bytes through SHA-256 (256 KiB buffer). Short → `truncated`; longer →
   `grown`; equal size but different digest → `sha256`; permission bits differ → `mode`;
   absent → `missing`.

Output: `image ok: ...` (exit 0), `image FAILED: N problem(s)` with one line per problem
(exit 4), `image REJECTED: <reason>` (exit 4), I/O or key-file error (exit 1), usage (exit 2).
`--json` prints one object (`ok`, header fields, `files`, `bytes`, `bytes_hashed`, `key`,
`manifest_sha256`, `ms`, `problems[] {code,path,detail}` or `rejected`). Every run appends one
`image.verify` record to the hash-chained audit log when a state dir is usable (best effort; it
never changes the verdict). The image directory is never written (real test compares a full
byte+mode snapshot before and after).

## 6. Threats covered and not covered

| Attack / fault | Result | Test |
|---|---|---|
| bit flip anywhere in a payload file | `sha256` | unit (5 positions), real (4 positions incl. first/last byte), 512 MiB file last-MiB flip |
| truncation (1 byte, 4 KiB, to 0) / growth | `truncated` / `grown` | unit + real |
| file removed / added / empty dir added | `missing` / `unlisted` / `unlisted-dir` | unit + real |
| listed file swapped for a symlink to identical bytes | `not-regular (symlink)` | unit + real |
| permission change (0644 → 0666) | `mode` | unit + real |
| manifest edited, cut, CRLF'd, extended | bad signature | unit (5) + real (2) |
| signature byte changed / file deleted | bad signature / metadata | real |
| wrong public key | wrong key | unit + real (second real key pair) |
| attacker re-labels own signature with the right fingerprint | bad signature | unit |
| HADES approval-domain signature over the manifest | bad signature | unit |
| setuid bit, `..`, absolute path, reserved name, upper-case hex, `Z` time | parse refused | unit (15 cases) |

Not covered by v1 (by design or later work): **rollback** to an older correctly-signed image
(needs a monotonic version counter on the device: D-IF3); **TOCTOU** between verify and use
(verify reports a state, it does not lock it; the installer must verify what it copies);
ownership (uid/gid) is not pinned (device-side installer sets it); extended attributes /
SELinux labels; key revocation/rotation (D-IF1); a compromised signing seed.

## 7. Performance (box, 2026-10-21 18:21-18:22 CEST, load average ~21 on 8 vCPU)

| Run | Result |
|---|---|
| x86_64-musl release, 7-file real image (4.4 MB) | verify 14 ms in-process, 20 ms wall |
| x86_64, 512 MiB pseudo-random file | 3 runs 3607 / 4010 / 6052 ms, median **128 MiB/s**; manifest build 4376 ms |
| aarch64-musl under qemu-aarch64, 128 MiB | median 2345 ms = 55 MiB/s (emulated) |
| armv7-musleabihf under qemu-arm, 128 MiB | median 2677 ms = 48 MiB/s (emulated) |
| reference: coreutils `sha256sum` (SHA-NI hardware) 256 MiB | 0.40 s wall |

SHA-256 is `sha2` with `force-soft` (no SHA-NI / ARMv8 crypto instructions) because of the
project's no-SIMD/no-asm rule. That is ~10x slower than hardware SHA; a 2 GiB system image would
take ~16 s on this (loaded) box. Owner decision D-IF2: keep force-soft, or allow the CPU SHA
extensions for this one hash.

## 8. Next steps (not done here)

* Container: deterministic tar (the shared std-only tar crate from the openmind repo, slot 122)
  of the directory with the manifest first, so a stream can be verified before extraction.
* Installer: copy → verify the copy → atomically switch; keep the previous image for rollback.
* Version counter (anti-rollback) and key rotation record (D-IF1, D-IF3).
* Builder in CI: reproducible build → `gold_image manifest` → sign on the admin machine only.

## 9. Owner decisions

* **D-IF1 signing key**: (a) reuse the HADES admin key (one key to guard, but one compromise
  covers both); (b) separate image-signing key in the same format (recommended: different blast
  radius; costs one more key to keep offline). Either works without code change.
* **D-IF2 hash speed**: keep portable `force-soft` SHA-256 (rule-compliant, ~128 MiB/s here) or
  allow hardware SHA extensions just for image hashing (~10x faster, breaks no-SIMD rule).
* **D-IF3 anti-rollback**: add a monotonic `build.serial` header + device-side counter in v2,
  or accept rollback-to-older-signed-image risk for now.
* **D-IF4 builder location**: builder stays a dev-side example (`gold_image`, recommended: the
  device can only verify) or becomes `galion-admin image build/sign` subcommands.
