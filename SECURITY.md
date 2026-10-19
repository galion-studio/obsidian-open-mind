# Security Policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | yes |
| anything older | no |

The project is pre-release. Only the current 0.1.x line receives fixes until the versioning
policy is set at the release milestone.

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Preferred channel: **GitHub private vulnerability reporting**. Open the repository's
**Security** tab and choose **Report a vulnerability**
(https://github.com/galion-studio/open-mind-stack/security/advisories/new). The report stays
private between you and the maintainers until an advisory is published. (Private
vulnerability reporting is switched on in the repository settings when the repository goes
public; see `docs/PUBLISHING.md`.)

Fallback private contact: TODO. No email and no person are named in this file. The
address is filled in before publication, and it is never a personal mailbox that
someone reads by accident.

Please include, where you can:

- the affected crate and version (for example `openmind-schema` 0.1.0);
- what an attacker can do with it, in one or two sentences;
- a minimal reproduction or the exact commands you ran;
- whether the issue is already public anywhere.

What to expect: an acknowledgement, a first assessment, and then a fix or a reasoned
"will not fix". Please give the maintainers a reasonable window to ship a fix before any
public write-up.

## What never goes in an issue or a pull request

- API keys, tokens, passwords or private keys. Never paste them, not even redacted-looking
  ones. If you think a key leaked, report that privately, do not file it publicly.
- Contents of `.env` files, credential stores or key boxes.
- Personal data about other people.
- Live host names, addresses or ports of somebody's running system. Use placeholders.

## How this project handles keys

The contract types are built so that a secret has no place to sit:

- `KeySlot` carries only a slot `name`, a `present` flag and a `hint`. The secret itself
  lives outside the schema, in the key box.
- `Provider` references the key box by slot name (`key_slot`), never by value.
- A test asserts the serialized CPU stack contains no key-shaped string.

Keep it that way. A pull request that puts key material into the schema, into state files or
into logs is rejected.

## Scope

In scope: the code in this workspace. Out of scope: third-party dependencies, the hosting
environment, and anything the maintainers do not ship.

Every timestamp in this project is Europe/Zagreb time with an explicit offset.
