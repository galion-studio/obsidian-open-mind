// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tool list for the research vault. Read tools are listed first.
//! The two write tools are last, and their descriptions say they stay in `drafts/`.

use crate::{vault::MAX_TAGS, DEFAULT_LIMIT, MAX_LIMIT};
use serde_json::{json, Value};

/// Tools `tools/list` returns, in that order.
pub const TOOL_NAMES: &[&str] = &[
    "search_notes",
    "read_note",
    "list_recent",
    "search_tags",
    "backlinks",
    "create_draft_note",
    "link_notes",
];

pub fn tools_result() -> Value {
    json!({
        "tools": TOOL_NAMES.iter().map(|name| tool_spec(name)).collect::<Vec<_>>()
    })
}

fn tool_spec(name: &str) -> Value {
    let (description, schema) = match name {
        "search_notes" => (
            "Find notes whose title, tags or body contain the query. Reads the vault and writes nothing.",
            object(
                json!({
                    "query": text("Substring to find. ASCII case-insensitive."),
                    "limit": limit_prop()
                }),
                &["query"],
            ),
        ),
        "read_note" => (
            "Read one markdown note by its vault-relative path. Writes nothing.",
            object(
                json!({
                    "path": text("Vault-relative path ending in .md.")
                }),
                &["path"],
            ),
        ),
        "list_recent" => (
            "List notes by their updated stamp, newest first. Writes nothing.",
            object(json!({ "limit": limit_prop() }), &[]),
        ),
        "search_tags" => (
            "Find notes that carry this tag in frontmatter or as an inline #tag. Writes nothing.",
            object(
                json!({
                    "tag": text("Tag without a leading hash. ASCII letters, digits, _ and -."),
                    "limit": limit_prop()
                }),
                &["tag"],
            ),
        ),
        "backlinks" => (
            "List notes that wikilink the named note. Writes nothing.",
            object(
                json!({
                    "note": text("Vault-relative path or title."),
                    "limit": limit_prop()
                }),
                &["note"],
            ),
        ),
        "create_draft_note" => (
            "Create a new markdown note under drafts/. Does not change a published note.",
            object(
                json!({
                    "title": text("Draft title. Stored in frontmatter."),
                    "body": text("Markdown body. Optional."),
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "maxItems": MAX_TAGS,
                        "description": "Plain tags, no hash mark."
                    }
                }),
                &["title"],
            ),
        ),
        "link_notes" => (
            "Record a wikilink. Appends it to a draft, or writes a proposal under drafts/links/. Does not change a published note.",
            object(
                json!({
                    "from": text("Source note path or title."),
                    "to": text("Target note path or title.")
                }),
                &["from", "to"],
            ),
        ),
        _ => panic!("unknown tool {name}"),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema
    })
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn text(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn limit_prop() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": MAX_LIMIT,
        "description": format!("How many notes to return. Default {DEFAULT_LIMIT}.")
    })
}
