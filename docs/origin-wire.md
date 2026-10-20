<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# origin wire (not wired)

- DESK-ID: openmind.ORIGIN-WIRE
- Written: 2026-10-19T18:12:24+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: `crates/openmind-origin` (DESK-ID openmind.B56)
- Scope: how a future caller would ask for one HTTP 302. This file does not wire it.

## galion-one does not call it yet

galion-one does not call it yet, so port 443 and any origin host are unchanged.

`crates/galion-one/Cargo.toml` depends on `openmind-config` only. `Cargo.lock` lists the same single dependency for `galion-one`. No line in `crates/galion-one` names `openmind-origin`, `openmind_origin`, or `redirect_response`. This file does not edit galion-one. No other workspace `Cargo.toml` depends on `openmind-origin`. `Cargo.lock` records the package and no package depends on it. The only calls of `redirect_response` are the doctest in `crates/openmind-origin/src/lib.rs` and `crates/openmind-origin/tests/redirect.rs`.

`galion-one` still listens where it already listens. The defaults in `crates/galion-one/src/main.rs` are `127.0.0.1:8091` (`ONE_BIND`) and `127.0.0.1:8002` (`ONE_MOUTH_BIND`). Neither default is port 443. This note does not change those addresses.

The process-list row in `load_units` is not this wire. It pushes the id `origin` when the process args contain the light helper name, with the note `light`. The comment there says the crate does not launch that helper. The check does not import `openmind-origin` and does not build a 302. `openmind-survival` names `origin` in a paused list for a network loss. That string is not a call either.

Port 443 was not bound by this note. At 2026-10-19T18:12:24+02:00, `ss -lnt` showed no listener whose local port was exactly 80 or exactly 443. No listener was added, stopped, or retargeted. The cloudflared ingress file was not opened for edit and was not rewritten. No origin hostname was added, removed, or pointed at a new port.

## What the crate already builds

`openmind-origin` is a library. The manifest description is: pure function that builds an HTTP 302 redirect. Location comes from the caller; the default is `/`. The body is empty. No listener. There is no `[[bin]]` and no `fn main`. `#![forbid(unsafe_code)]` is set. The manifest and `src/lib.rs` do not name `std::net`, `TcpListener`, `TcpStream`, `UdpSocket`, `UnixStream`, `tokio`, `hyper`, `reqwest`, `ureq`, or `mio::`.

`redirect_response(location)` returns a `RedirectResponse`. Every value has the same status:

| Field | Value |
| --- | --- |
| `status_code` | `302` (`STATUS_CODE`) |
| `status_line` | `HTTP/1.1 302 Found` (`STATUS_LINE`), no trailing CRLF on the getter |
| `location` | the argument, or `DEFAULT_LOCATION` which is `/` |
| `body` | empty (`b""`) |
| `Content-Length` | `0`, written by `Display` |
| `touches_network` | false |

An empty location uses `/`. A location that contains CR, LF, or NUL also uses `/`, so the header block stays one line. Any other string is copied as the Location value, including a path such as `/console`. Building or displaying the value does not bind a port, dial out, or touch the network.

`Display` writes this byte shape and nothing after the header end:

```text
HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n
```

## How a future caller would ask

Ask with the function already in the crate. Do not pass a port. The crate has nowhere to put one.

```text
openmind_origin::redirect_response(location)
```

`location` is a string the caller already has. The function does not read a file, an environment variable, a Host header, or a flag. The caller, which this ticket does not add, would take `to_string()` and write those bytes on a connection it already owns. `Would send` is not a mode here. There is no apply path.

Until that caller exists inside galion-one:

- galion-one is not rebuilt and is not restarted by this note.
- Port 443 stays as it was. This crate cannot listen on it.
- Any origin host stays as it was. No hostname is selected, rewritten, or given a new backend.
- The light helper is not started and not stopped. This note does not replace it.

A future edit that adds the dependency to galion-one is a different ticket. It would still not, by itself, move port 443 or an origin host, because `redirect_response` only returns bytes.

## What this file does not do

- It does not edit `crates/galion-one` or any other crate.
- It does not add `openmind-origin` to a dependency list.
- It does not bind port 443 or port 80.
- It does not change any origin host.
- It does not edit the cloudflared ingress.
- It does not call the public internet.
- It does not start, stop, or restart a process.
