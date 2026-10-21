// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/keys` (console-ide `src/routes/_console/keys.tsx`, live tree
//! 2026-10-20 21:25 CEST): API keys. List from `GET /api/v1/keys`; Mint and
//! Revoke go to `POST /api/v1/keys` (admin cookie forwarded, as the TSX's
//! `fetch` sent it).
//!
//! Without JavaScript:
//! - Mint is a same-origin-checked `<form method=post>` with a name field and
//!   one checkbox per scope (`chat` ticked by default, as the TSX started);
//!   the server sends `{"name":…,"scopes":[…]}` upstream with the name
//!   trimmed, control characters removed and cut to 64 characters, and only
//!   scopes from [`SCOPES`];
//! - Revoke is one small form per row carrying only the key id (checked
//!   against [`id_ok`], else nothing is sent); the server sends
//!   `{"action":"revoke","id":…}`;
//! - the page never sends `action: "check"` and never echoes a key it did not
//!   just receive. The fresh secret is shown once, in the answer to the mint
//!   post (`Cache-Control: no-store`), never logged, never put in a URL.
//!
//! Fix over the TSX: when the list does not load (live today: `401 phone
//! required` for an unpaired browser) the page says so instead of
//! "No keys.", and a refused mint/revoke says so instead of an empty key.

use super::p3::{cap, miss, plain, s, zagreb_iso};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// Scopes a key may carry (`KEY_SCOPES` in `lib/keys.ts`).
pub const SCOPES: &[&str] = &["chat", "tools", "files", "admin"];

/// Longest key name the store keeps (`name.trim().slice(0, 64)`).
pub const MAX_NAME: usize = 64;

/// The two form actions.
pub const OPS: &[&str] = &["mint", "revoke"];

/// Key ids are `randomBytes(4).toString("hex")`: 8 lowercase hex digits.
/// Accept 1..=32 hex digits so a format change upstream does not lock the
/// page out, and nothing else.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The name as sent upstream: trimmed, control characters dropped, at most
/// [`MAX_NAME`] characters.
pub fn clean_name(raw: &str) -> String {
    let kept: String = raw.chars().filter(|c| !c.is_control()).collect();
    kept.trim()
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The ticked scopes, in [`SCOPES`] order, unknown values ignored.
pub fn pick_scopes<'a>(ticked: impl Iterator<Item = &'a str>) -> Vec<&'static str> {
    let ticked: Vec<&str> = ticked.collect();
    SCOPES
        .iter()
        .copied()
        .filter(|s| ticked.contains(s))
        .collect()
}

/// The result of a form post, shown above the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// A key was minted; the secret, shown once.
    Minted { name: String, key: String },
    /// Revoke answered `{ok}`.
    Revoked(bool),
    /// The upstream refused or did not answer.
    Refused(String),
}

fn row(k: &Value) -> String {
    let scopes: Vec<String> = k
        .get("scopes")
        .as_array()
        .iter()
        .filter_map(Value::as_str)
        .map(plain)
        .collect();
    let revoked = !k.get("revoked_at").is_null() && !s(k, "revoked_at").is_empty();
    let id = s(k, "id");
    let created = s(k, "created_at");
    let mut out = format!(
        "<li class=\"p3-key\"><span class=\"p3-kn\"><span class=\"p3-name ell\">{}</span><span class=\"p3-sub\">{}… · {}{}{}</span></span>",
        esc(&cap(&plain(s(k, "name")), MAX_NAME)),
        esc(&cap(&plain(s(k, "prefix")), 16)),
        esc(&scopes.join(" ")),
        if created.is_empty() {
            String::new()
        } else {
            format!(" · {}", esc(&zagreb_iso(created)))
        },
        if revoked { " · revoked" } else { "" },
    );
    if !revoked && id_ok(id) {
        out.push_str(&format!(
            "<form method=\"post\" action=\"/keys\"><input type=\"hidden\" name=\"op\" value=\"revoke\"><input type=\"hidden\" name=\"id\" value=\"{}\"><button class=\"btn ghost\" type=\"submit\">Revoke</button></form>",
            esc(id)
        ));
    }
    out.push_str("</li>");
    out
}

pub fn body(list: Result<&Value, (Option<u16>, String)>, done: Option<&Done>) -> String {
    let mut out = format!(
        "<div class=\"page w46\"><header><p class=\"k\">Keys</p><h1>{}<span class=\"ell\">API keys</span></h1>\
<p class=\"lede\">The secret is shown once. The disk keeps a hash. Hermes cannot mint, revoke, or read this store.</p></header>",
        icon("keys", "i i-md")
    );
    out.push_str(
        "<form class=\"p3-mint\" method=\"post\" action=\"/keys\"><input type=\"hidden\" name=\"op\" value=\"mint\">\
<label class=\"p3-field\"><span>Name</span><input name=\"name\" maxlength=\"64\" autocomplete=\"off\" spellcheck=\"false\"></label>\
<fieldset class=\"p3-scopes\"><legend class=\"sr-only\">Scopes</legend>",
    );
    for sc in SCOPES {
        out.push_str(&format!(
            "<label><input type=\"checkbox\" name=\"scope\" value=\"{sc}\"{}> {sc}</label>",
            if *sc == "chat" { " checked" } else { "" }
        ));
    }
    out.push_str(
        "</fieldset><div class=\"p3-top\"><span>Mint</span><button class=\"btn primary\" type=\"submit\">Mint</button></div></form>",
    );
    match done {
        Some(Done::Minted { name, key }) => out.push_str(&format!(
            "<div class=\"p3-fresh\"><p>Copy it now. It will not be shown again.</p><p class=\"p3-sub\">{}</p><pre class=\"p3-pre\">{}</pre></div>",
            esc(&cap(&plain(name), MAX_NAME)),
            esc(key)
        )),
        Some(Done::Revoked(true)) => out.push_str("<p class=\"note\">revoked</p>"),
        Some(Done::Revoked(false)) => out.push_str(
            "<p class=\"note p3-bad\">Nothing revoked: the id is unknown or already revoked.</p>",
        ),
        Some(Done::Refused(why)) => out.push_str(&format!(
            "<p class=\"note p3-bad\">{}</p>",
            esc(&cap(&plain(why), 200))
        )),
        None => {}
    }
    match list {
        Ok(v) => {
            let keys = v.get("keys").as_array();
            out.push_str("<ul class=\"p3-list\">");
            if keys.is_empty() {
                out.push_str("<li class=\"p3-key muted\">No keys.</li>");
            }
            for k in keys {
                out.push_str(&row(k));
            }
            out.push_str("</ul>");
        }
        Err((code, err)) => out.push_str(&miss("The key list", code, &err)),
    }
    out.push_str("</div>");
    out
}

/// What the mint answer gives the page. Only a `gal_sk_` secret of sane
/// length and alphabet is shown; anything else is reported as a refusal.
pub fn minted(status: u16, answer: &Value) -> Done {
    let key = s(answer, "key");
    let sane = key.starts_with("gal_sk_")
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if status == 200 && sane {
        Done::Minted {
            name: s(answer, "name").to_string(),
            key: key.to_string(),
        }
    } else {
        let err = s(answer, "error");
        Done::Refused(if err.is_empty() {
            format!("Mint refused: HTTP {status}.")
        } else {
            format!("Mint refused: HTTP {status} ({err}).")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn ids_names_scopes() {
        assert!(id_ok("a1b2c3d4"));
        assert!(!id_ok(""));
        assert!(!id_ok("a1b2\"><x"));
        assert!(!id_ok("../keys"));
        assert!(!id_ok(&"a".repeat(33)));
        assert_eq!(clean_name("  my\r\nkey  "), "mykey");
        assert_eq!(clean_name(&"ž".repeat(70)).chars().count(), 64);
        assert_eq!(clean_name("   "), "");
        assert_eq!(
            pick_scopes(["admin", "chat", "root", "chat"].into_iter()),
            vec!["chat", "admin"]
        );
        assert!(pick_scopes(std::iter::empty()).is_empty());
    }

    #[test]
    fn list_and_forms() {
        let v = json::parse(br#"{"ok":true,"keys":[{"id":"a1b2c3d4","name":"phone <1>","prefix":"gal_sk_abcde","scopes":["chat","tools"],"created_at":"2026-10-21T03:00:00.000Z","revoked_at":null},{"id":"00ff00ff","name":"old","prefix":"gal_sk_zzzzz","scopes":["admin"],"created_at":"2026-10-01T10:00:00.000Z","revoked_at":"2026-10-02T10:00:00.000Z"}]}"#).unwrap(); // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
        let h = body(Ok(&v), None);
        assert!(h.contains("phone &lt;1&gt;"));
        assert!(h.contains("gal_sk_abcde… · chat tools · 2026-10-21 05:00 CEST"));
        assert!(h.contains("· revoked"));
        assert_eq!(
            h.matches("value=\"revoke\"").count(),
            1,
            "revoked row has no button"
        );
        assert!(h.contains("name=\"id\" value=\"a1b2c3d4\""));
        assert_eq!(h.matches("type=\"checkbox\"").count(), 4);
        assert_eq!(h.matches(" checked>").count(), 1);
        assert!(!h.contains("<script"));
        assert!(!h.contains("Copy it now"));
    }

    #[test]
    fn refusals_and_fresh_key() {
        let h = body(Err((Some(401), "phone required".into())), None);
        assert!(h.contains("The key list did not load: HTTP 401 (phone required)."));
        assert!(!h.contains("No keys."));
        let ok = json::parse(br#"{"id":"a","name":"n","prefix":"gal_sk_AAAA","scopes":["chat"],"key":"gal_sk_AAAA-_bc"}"#).unwrap();
        let d = minted(200, &ok);
        assert_eq!(
            d,
            Done::Minted {
                name: "n".into(),
                key: "gal_sk_AAAA-_bc".into()
            }
        );
        let h = body(Ok(&json::parse(br#"{"keys":[]}"#).unwrap()), Some(&d));
        assert!(h.contains("Copy it now. It will not be shown again."));
        assert!(h.contains("<pre class=\"p3-pre\">gal_sk_AAAA-_bc</pre>"));
        assert!(h.contains("No keys."));
        let bad = json::parse(br#"{"error":"phone required"}"#).unwrap();
        assert_eq!(
            minted(401, &bad),
            Done::Refused("Mint refused: HTTP 401 (phone required).".into())
        );
        let weird = json::parse(br#"{"key":"<script>"}"#).unwrap();
        assert!(matches!(minted(200, &weird), Done::Refused(_)));
        assert!(body(Ok(&bad), Some(&Done::Revoked(false))).contains("Nothing revoked"));
        assert!(body(Ok(&bad), Some(&Done::Revoked(true))).contains(">revoked</p>"));
    }
}
