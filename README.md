# OpenMind

The Rust core of the Galion Obsidian desk. One Cargo workspace, MIT licensed.

It holds the shared contract types (`openmind-schema`), the Europe/Zagreb time
helper (`zagreb-time`), and (from the later tickets) the Galion One, galion-desk
and room servers plus the `desk-hermes` and `link-watch` Rust replacements for
the old Python helpers.

## Build

The box is shared and busy, so always build chilled out:

```sh
export PATH=/root/.cargo/bin:$PATH
nice -n 19 cargo build -j 2 --release --workspace
```

Run the tests the same way:

```sh
nice -n 19 cargo test -j 2 --workspace
```

## Layout

- `crates/openmind-schema` - the stack contract types (serde).
- `crates/zagreb-time` - every timestamp is Europe/Zagreb RFC 3339 with an
  explicit offset (`+02:00` CEST / `+01:00` CET), never `Z`.

## Rules

1. No Python. New code is Rust.
2. Every timestamp is Europe/Zagreb RFC 3339 with offset - never server (UTC) time.
3. CPU-light: builds run with `nice -n 19` and `-j 2`.
4. No stray CJK characters in any file.
5. No secrets in files, logs or replies.
6. Money rails (Revolut / real money) are not approved and never appear.
