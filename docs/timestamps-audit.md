# Timestamp audit

- DESK-ID: openmind.B39
- Generated: 2026-10-19T16:10:07+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Read-only. No Rust file edited. No commit.

Scan under `crates/` (no `target/`):

```
rg -n "Utc::now|SystemTime::now" crates -g "*.rs"
rg -n "UNIX_EPOCH|utc_stamp_full|utc_stamp_clock|fn clock|fn unix_secs|fn now\(" crates -g "*.rs"
```

`Utc::now` appears once, inside `zagreb-time`, and that call already feeds `now_rfc3339`. Every other row below is a `SystemTime::now` or a print of a unix epoch / UTC stamp. `Instant::now` durations (elapsed milliseconds) are not clocks and are not listed.

The fix on every output row is the same: feed `zagreb-time`. Use `zagreb_time::now_rfc3339()` for the current instant (Europe/Zagreb RFC 3339, seconds, explicit `+01:00` or `+02:00`, never `Z`, never `+00:00`). Use `zagreb_time::from_unix(secs)` for a stored unix second. Use `zagreb_time::label(secs)` for a human line. Do not print a bare epoch integer, epoch millis, or a UTC `HH:MM:SS` (`unix_secs % 86400` is UTC, two hours behind Zagreb while CEST is in force).

## Clock calls and what they print

| file:line | what it prints | fix |
| --- | --- | --- |
| crates/zagreb-time/src/lib.rs:36 | `now_rfc3339()` prints Europe/Zagreb RFC 3339. The only `Utc::now` in `crates/`. `fn epoch` at line 17 is a fallback instant for an impossible unix second; it is rendered with the Zagreb offset and is not printed as epoch 0 or as `Z`. | Already feeds zagreb-time. Keep this as the only `Utc::now`. |
| crates/zagreb-time/examples/clock.rs:9 | Stdout `now:   <Zagreb RFC 3339>`. Line 10 prints `winter: <from_unix>`. Line 11 prints `label: <Zagreb human label>`. | Already feeds zagreb-time. No change. |
| crates/galion-desk/src/main.rs:752 | `now()` is `SystemTime::now` as unix seconds. It does not print by itself. | Feed zagreb-time at the two call sites below. Delete this helper once those sites stop needing a raw integer. |
| crates/galion-desk/src/main.rs:335 | Appends `{"at":<unix seconds>,"text":"..."}` to `state_dir/desk-notes.jsonl`. Default path `/opt/galion/galion-light/state/desk-notes.jsonl`. | Feed zagreb-time: write `"at":"<zagreb_time::now_rfc3339()>"` (JSON string, not a bare number). |
| crates/galion-desk/src/main.rs:408 | Desk snapshot JSON field `"at":<unix seconds>` (number, no offset) on the HTTP body. | Feed zagreb-time: same string from `zagreb_time::now_rfc3339()`. |
| crates/link-watch/src/main.rs:213 | `unix_now()` is unix seconds. Not printed here. Callers format it as UTC. | Feed zagreb-time at the print sites (lines 465 and 476). Do not format the seconds yourself. |
| crates/link-watch/src/main.rs:220 | `unix_now_f64()` is a unix-second float used only to throttle the France probe (`france_at`). Not written. | Not a timestamp output. Do not print it. Leave the interval private. |
| crates/link-watch/src/main.rs:203 | `utc_stamp_full` prints `YYYY-MM-DDTHH:MM:SSZ` (UTC, trailing `Z`). | Feed zagreb-time: replace with `zagreb_time::from_unix(secs)` or `now_rfc3339()`. Never print `Z`. |
| crates/link-watch/src/main.rs:209 | `utc_stamp_clock` prints `HH:MM:SS` from `gmtime` (UTC time of day, no date, no offset). | Feed zagreb-time: `zagreb_time::label(secs)` or `now_rfc3339()`. |
| crates/link-watch/src/main.rs:465 | Event line `"<HH:MM:SS>  laptop up\|down  model ...  hermes up\|down"` written to the log and copied into `link.json` `events`. Default log `/opt/galion/galion-light/state/link-log.json`. | Feed zagreb-time: prefix the line with `zagreb_time::now_rfc3339()` (or `label`), not `utc_stamp_clock`. |
| crates/link-watch/src/main.rs:476 | JSON key `at` is the `Z` stamp, written to `link.json`. Default `/opt/galion/galion-light/state/link.json`. | Feed zagreb-time: set `at` from `zagreb_time::now_rfc3339()`. Legacy readers can use `at_local` until then; do not add new `Z` output. |
| crates/link-watch/src/main.rs:479 | JSON key `at_local` is already `zagreb_time::now_rfc3339()`. | Already feeds zagreb-time. This is the field to keep. |
| crates/openmind-survival/src/probes.rs:183 | `SystemTime::now` minus file mtime. The detail string is `age <n>s max <n>s` (a duration, not a clock). `ProbeResult.ts` is set at line 28 with `zagreb_time::now_rfc3339()`. | Do not print this `SystemTime`. The stamp already feeds zagreb-time. If a file mtime is ever shown, feed `zagreb_time::from_unix`. |
| crates/openmind-survival/tests/survival.rs:89 | Test-only. Builds an mtime two hours in the past. Prints nothing. | Not an output. Do not feed it into a printed clock. |
| crates/one-defence/tests/golden.rs:117 | Test-only. `as_nanos()` since the unix epoch is spliced into a temp db path so the name is unique. Not printed as a time. | Not a clock. Do not print the number. No zagreb-time feed. |
| crates/one-set-model/tests/temp_file.rs:20 | Test-only. Same pattern: temp dir `one-set-model-<pid>-<nanos>`. Not printed as a time. | Not a clock. Do not print the number. No zagreb-time feed. |
| crates/galion-one/src/main.rs:1091 | `warm_due` reads `state/warm.ts` as a float and compares it to unix seconds. The number is not printed here. | Keep the compare private. The file write below is the output. |
| crates/galion-one/src/main.rs:1152 | `SystemTime::now` as `as_secs_f64`, then line 1157 writes that float with `format!("{now}")` into `$GALION_HOME/state/warm.ts` (default `/workspace/galion-light/state/warm.ts`). One bare unix second, no offset. | Feed zagreb-time: write `zagreb_time::now_rfc3339()` if the file is shown. If it stays a private interval marker, do not print or serve the raw float. |
| crates/galion-one/src/main.rs:1314 | `share_id` prints the unix epoch in milliseconds as hex, then four random bytes. Used as an id (line 1342), not labeled as a time. | Not a clock display. Do not present the hex as a timestamp. If a created-at is shown beside the share, feed `zagreb_time::now_rfc3339()` and keep the id random. |
| crates/galion-one/src/main.rs:3087 | `pair` takes unix epoch milliseconds. Line 3121 writes `"lockedUntil":<millis>` into `/root/galion-secrets/admin-device.json` (mode 0600). The HTTP body is `{"error":"later"}` or `{"error":"no"}` and does not include the number. | The stored millis are an epoch. If any reader displays the lock time, feed `zagreb_time::from_unix(millis/1000)`. Do not print the raw millis. The compare itself can stay numeric and private. |
| crates/galion-one/src/main.rs:3426 | `unix_secs()` is unix seconds. `clock()` at line 3432 prints `HH:MM:SS` from `unix_secs % 86400`, which is UTC time of day, not Zagreb. | Feed zagreb-time: replace `clock()` with `zagreb_time::now_rfc3339()` or `label`. Stop printing `unix_secs()` on the wire. |
| crates/galion-one/src/main.rs:798 | In-memory nerve event: `<UTC HH:MM:SS> ear <ms> mind <ms> mouth <ms>`. | Feed zagreb-time via `now_rfc3339()` or `label` instead of `clock()`. |
| crates/galion-one/src/main.rs:812 | Appends to `/workspace/hermes/channels/one.jsonl`: `"unix":<unix seconds>,"at":"<UTC HH:MM:SS>"` plus channel, heard, said, and three millisecond counts. | Feed zagreb-time: one `"at"` string from `now_rfc3339()`. Drop the bare `"unix"` field. |
| crates/galion-one/src/main.rs:1741 | Appends to `$GALION_HOME/one/session.jsonl` (default `/workspace/galion-light/one/session.jsonl`): `"unix":<unix seconds>,"at":"<UTC HH:MM:SS>"` plus heard and said. | Feed zagreb-time: same as the voice log. One Zagreb `at`, no bare unix field. |
| crates/galion-one/src/main.rs:1948 | Nerve event `<UTC HH:MM:SS> <service> up\|down`. | Feed zagreb-time: prefix with `now_rfc3339()` or `label`. |
| crates/galion-one/src/main.rs:1955 | Nerve event `<UTC HH:MM:SS> <heal line>` when `state/heal/nerve.jsonl` has a new last line. | Feed zagreb-time: same prefix. Do not re-stamp a line that already carries a Zagreb time. |
| crates/galion-one/src/main.rs:2171 | Analytics JSON (the default `/api/v1/analytics` body) field `"at":"<UTC HH:MM:SS>"`. | Feed zagreb-time: `"at":"<now_rfc3339()>"`. |
| crates/galion-one/src/main.rs:3455 | `note()` prefixes nerve events with the same UTC `clock()`. | Feed zagreb-time: same replacement as `clock()`. |
| crates/desk-hermes/src/main.rs:174 | `mtime` is the file mtime as unix seconds. Not printed here. | Feed zagreb-time at the stdout site (line 328) via `from_unix`. |
| crates/desk-hermes/src/main.rs:328 | Stdout JSON `recent[].at` is that unix second (integer). This bin prints one JSON object and does not write a state file. | Feed zagreb-time: `"at":"<zagreb_time::from_unix(mtime)>"`. |
| crates/desk-hermes/src/main.rs:30 | `secs()` turns a JSON number into unix seconds (if the value is over 10000000000 it divides by 1000). Not printed here. | Feed zagreb-time at the three stdout sites that call `secs`. |
| crates/desk-hermes/src/main.rs:382 | Stdout `sessions[].at` and `road[].at` are `max(messages.timestamp)` as unix seconds, read-only from the Hermes db. | Feed zagreb-time: `from_unix(secs(...))`. |
| crates/desk-hermes/src/main.rs:462 | Stdout `tasks[].at` is `dispatched_at` as unix seconds. | Feed zagreb-time: `from_unix`. |
| crates/desk-hermes/src/main.rs:480 | Stdout `history[].at` is the `at` field of `activity.jsonl`, passed through `secs()` and printed as unix seconds. | Feed zagreb-time: `from_unix`. Do not pass the raw integer through. |
| crates/desk-hermes/src/main.rs:521 | Stdout task `one-galion` hard-codes `"at": 0` (the unix epoch). | Not a real instant. Omit `at`, or feed zagreb-time only when a real unix second exists. Do not print 0. |

## Already Zagreb (no UTC print)

These call `zagreb_time::now_rfc3339` and do not call `Utc::now` or `SystemTime::now`:

| file:line | what it prints | fix |
| --- | --- | --- |
| crates/openmind-survival/src/probes.rs:28 | `ProbeResult.ts`, Europe/Zagreb RFC 3339. | Already feeds zagreb-time. |
| crates/guardian-tick/src/main.rs:149 | Report object `"ts"` plus each probe `ts`, same format. | Already feeds zagreb-time. |
| crates/persona-plex-relay/src/main.rs:61 | Plan lines carry `Environment.now` from `now_rfc3339()`. | Already feeds zagreb-time. |
| crates/link-watch/src/main.rs:479 | `at_local` on `link.json`. | Already feeds zagreb-time. The sibling `at` key is the UTC row above. |

## State files that still receive a UTC or epoch value

| state file | written by | value |
| --- | --- | --- |
| `/opt/galion/galion-light/state/desk-notes.jsonl` (or `DESK_STATE_DIR`) | galion-desk:335 | `"at"` unix seconds |
| desk HTTP snapshot (not a file) | galion-desk:408 | `"at"` unix seconds |
| `/opt/galion/galion-light/state/link.json` | link-watch:476 and :465 | `at` ends with `Z`; `events` start with UTC `HH:MM:SS`; `at_local` is already Zagreb |
| `/opt/galion/galion-light/state/link-log.json` | link-watch:465 | same UTC event lines |
| `$GALION_HOME/state/warm.ts` (default `/workspace/galion-light/state/warm.ts`) | galion-one:1157 | bare float unix second |
| `/workspace/hermes/channels/one.jsonl` | galion-one:812 | `"unix"` seconds and `"at"` UTC `HH:MM:SS` |
| `$GALION_HOME/one/session.jsonl` | galion-one:1741 | same pair |
| nerve event list (memory, then analytics JSON) | galion-one:798, :1948, :1955, :2171, :3455 | UTC `HH:MM:SS` |
| `/root/galion-secrets/admin-device.json` | galion-one:3121 | `"lockedUntil"` epoch milliseconds. Not read for this audit. |
| desk-hermes stdout (no file) | desk-hermes:328, :382, :462, :480, :521 | `"at"` unix seconds, or 0 |

C6 evidence: after `rg -n "Utc::now|SystemTime::now" crates -g "*.rs" | rg -v "zagreb-time"`, the remaining hits are the rows in the first table. None of those prints is Zagreb yet, except the survival `ts` field which is not produced by the `SystemTime::now` on line 183.
