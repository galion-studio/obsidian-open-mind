<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Never-unroute hostnames for the HADES route-removal plan (D-T42-4)

Branch `swarm/cortex-protected-hosts`, base `swarm/cortex-t42-hades-plans` 1ce9737. Repo change only;
nothing deployed (the live nerve does not run the plan generator).

## Decision

Mate, 2026-10-21 17:04 +02:00: the route-removal plan (`ingress-rule-removal`) must never target the
VPS itself, DNS, the mail server or the backup server. Every other hostname may be proposed.

## Where the list lives

`deploy/nerve/plans-protected-hosts.conf` (hostnames only, never addresses):

| category | names | source (read-only, VPS 2026-10-21 17:1x +02:00) |
|---|---|---|
| vps | ip50.vp3.titanaxe.com, ip50, ip50.localdomain, localhost | `hostname -f`, /etc/hostname, /etc/hosts, postfix `myhostname` |
| dns | elaine.ns.cloudflare.com, peyton.ns.cloudflare.com | authoritative NS of every tunnelled zone; the resolver is set by address only |
| mail | mx.zoho.eu, mx2.zoho.eu, mx3.zoho.eu, route1/2/3.mx.cloudflare.net | MX of galion.studio (Zoho EU) and of the other zones (Cloudflare Email Routing); postfix is local-only |
| backup | ip50.vp3.titanaxe.com, ip50 | galion-backup writes /var/backups/galion on the VPS; the laptop pulls the age copy from the VPS, so there is no separate backup host |

- Built in as the default (`include_str!`).
- `NERVE_PLANS_PROTECTED_HOSTS_FILE=<path>` replaces it at run time. A missing or invalid file (unknown
  category, an address, a bad name, an empty required category) never weakens the list: the built-in list
  stays and `cortex_plans.protected_hosts.warning` says why.
- `NERVE_PLANS_PROTECTED_HOSTS=a.example,b.example` adds names under `extra`.
- `*.example.com` protects every name under example.com (not the apex). Case-insensitive, trailing dot ignored.
- `cortex_plans.protected_hosts` (state JSON, `GET /api/v1/cortex/plans`, `openmind-nerve plans FILE`) shows
  the list in use and its source.

## What the plan does with it

- A protected hostname is never named in a `remove only ...` step; it is listed in `skipped` with
  `never-unroute hostname (<category>, D-T42-4)`.
- If every hostname routed to a closed port is protected (D-T42-4 list or the existing admin labels), the
  port is skipped entirely. Before this change a port whose hostnames were all admin-path names (ssh.*,
  admin.*) still got "remove the ingress rule(s) whose service is http://localhost:<port>", which would have
  removed those rules too. The T4.2 test `protected_names_never_get_a_plan` now expects no plan.
- Every ingress plan carries a precondition naming every never-unroute hostname, so a rule whose hostname the
  evidence did not show (`hosts: -`) is still checked by the operator before removal.

## Tests

`crates/openmind-nerve/tests/cortex_protected_hosts.rs` (6/0), unit tests in `src/protected_hosts.rs` (4/0),
updated `cortex_t42_plans.rs::protected_names_never_get_a_plan`. Every resulting plan passes the real
hades-gate safety review.

## Live check (read-only)

2026-10-21 17:14:53 +02:00: live nerve `/state` copied to the box (not committed), `openmind-nerve plans`:
3 plans (ingress 1, stray-stop 2). The ingress plan removes routes on :3011, :4001, :4060, :4444, :5530.
None of today's tunnel hostnames is on the D-T42-4 list, so the list changes no live plan today; it guards
future routes. With `NERVE_PLANS_PROTECTED_HOSTS=galion.studio` the :4001 line keeps galion.studio and
names only studio.galion.app and www.galion.studio.

## Open for the owner

- **D-PH-1 galion.studio.** It routes to :4001 (closed, 502), so it is still proposed for removal. It is not
  the VPS hostname, a DNS, mail or backup host by the VPS config. To keep it, add one line to the config
  (`extra = galion.studio`, plus `www.galion.studio` if wanted) or set `NERVE_PLANS_PROTECTED_HOSTS`.

## Box gates (2026-10-21 17:13-17:20 +02:00)

`cargo fmt --all -- --check` ok; `cargo clippy -p openmind-nerve --all-targets -- -D warnings` ok;
`cargo test -p openmind-nerve` 204/0/0; workspace (excl. openmind-sandbox, cgroup-detect, hermes-tools)
210 suites 1038/0/3; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` missing 0.
