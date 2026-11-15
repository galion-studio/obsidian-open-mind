// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Markdown vault. Reads stay inside the root. The only writes are new files
//! under `drafts/` and appends to files that are already under `drafts/`.
//! A published note is never opened for writing. Nothing is removed.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Markdown files kept in one index walk.
pub(crate) const MAX_NOTES: usize = 5000;

/// A note larger than this is skipped on walks and refused on read.
pub(crate) const MAX_NOTE_BYTES: u64 = 256 * 1024;

/// Body accepted by [`Vault::create_draft`].
pub(crate) const MAX_BODY_BYTES: usize = 64 * 1024;

/// Tags accepted on one draft.
pub(crate) const MAX_TAGS: usize = 16;

const MAX_SLUG: usize = 80;
const MAX_REL: usize = 240;
const DRAFT_TRIES: u32 = 50;

/// Why a vault call stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    /// The root is missing or is not a directory.
    BadRoot,
    /// The path leaves the vault, is hidden, or is a symlink.
    BadPath,
    /// No note has that path.
    NotFound,
    /// The file is over [`MAX_NOTE_BYTES`].
    TooBig,
    /// The walk passed [`MAX_NOTES`].
    TooMany,
    /// The title is empty, too long, or has a control character.
    BadTitle,
    /// A tag is not a plain token, or there are too many.
    BadTag,
    /// The body is too long or contains a NUL.
    BadBody,
    /// The stamp is not Europe/Zagreb RFC 3339.
    BadStamp,
    /// The query is empty or longer than 200 characters.
    BadQuery,
    /// The wikilink target is empty or has a character we will not write.
    BadLink,
    /// Fifty draft names were already taken.
    Exists,
    /// A directory read or a file write failed.
    Io,
    /// More than one note has that title.
    Ambiguous,
}

impl VaultError {
    fn static_message(self) -> &'static str {
        match self {
            Self::BadRoot => "vault is not a directory",
            Self::BadPath => "path refused",
            Self::NotFound => "note not found",
            Self::TooBig => "note too large",
            Self::TooMany => "vault note cap",
            Self::BadTitle => "title refused",
            Self::BadTag => "tag refused",
            Self::BadBody => "body refused",
            Self::BadStamp => "timestamp refused",
            Self::BadQuery => "query refused",
            Self::BadLink => "link refused",
            Self::Exists => "draft name exhausted",
            Self::Io => "vault io failed",
            Self::Ambiguous => "note name is ambiguous",
        }
    }
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooMany => write!(f, "vault has more than {MAX_NOTES} notes"),
            other => f.write_str(other.static_message()),
        }
    }
}

impl std::error::Error for VaultError {}

/// One markdown note inside the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub path: String,
    pub title: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub created: String,
    pub updated: String,
    pub body: String,
    pub bytes: u64,
}

/// A search hit. `score` counts ASCII case-insensitive substring hits:
/// three for each hit in the title, two for each hit in a tag, one for each
/// hit in the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scored {
    pub note: Note,
    pub score: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchOut {
    pub query: String,
    pub matched: usize,
    pub skipped: u32,
    pub hits: Vec<Scored>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecentOut {
    pub matched: usize,
    pub skipped: u32,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Backlink {
    pub path: String,
    pub title: String,
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackOut {
    pub query: String,
    pub resolved: Option<String>,
    pub matched: usize,
    pub skipped: u32,
    pub notes: Vec<Backlink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkOut {
    /// `append` when the source note is already under `drafts/`.
    /// `proposal` when the source is published or missing: a new file under
    /// `drafts/links/`.
    pub kind: &'static str,
    pub path: String,
    pub wrote: bool,
    pub from: String,
    pub to: String,
}

struct Resolved {
    note: Option<Note>,
    ambiguous: bool,
}

/// A vault root. The stored path is canonical.
#[derive(Debug, Clone)]
pub(crate) struct Vault {
    root: PathBuf,
}

impl Vault {
    pub(crate) fn open(root: &Path) -> Result<Self, VaultError> {
        let root = fs::canonicalize(root).map_err(|_| VaultError::BadRoot)?;
        let meta = fs::symlink_metadata(&root).map_err(|_| VaultError::BadRoot)?;
        if !meta.is_dir() {
            return Err(VaultError::BadRoot);
        }
        Ok(Self { root })
    }

    pub(crate) fn search(&self, query: &str, limit: usize) -> Result<SearchOut, VaultError> {
        let query = clean_query(query)?;
        let (notes, skipped) = self.collect()?;
        let mut hits: Vec<Scored> = notes
            .into_iter()
            .filter_map(|note| {
                let score = score_note(&note, &query);
                (score > 0).then_some(Scored { note, score })
            })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.note.path.cmp(&b.note.path)));
        let matched = hits.len();
        hits.truncate(limit);
        Ok(SearchOut {
            query,
            matched,
            skipped,
            hits,
        })
    }

    pub(crate) fn read(&self, rel: &str) -> Result<Note, VaultError> {
        if !rel.ends_with(".md") {
            return Err(VaultError::BadPath);
        }
        let path = self.checked_path(rel, true)?;
        let meta = fs::symlink_metadata(&path).map_err(|_| VaultError::NotFound)?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(VaultError::BadPath);
        }
        self.read_note_file(&path, rel, &meta)
    }

    pub(crate) fn recent(&self, limit: usize) -> Result<RecentOut, VaultError> {
        let (mut notes, skipped) = self.collect()?;
        notes.sort_by(recent_order);
        let matched = notes.len();
        notes.truncate(limit);
        Ok(RecentOut {
            matched,
            skipped,
            notes,
        })
    }

    pub(crate) fn by_tag(&self, tag: &str, limit: usize) -> Result<RecentOut, VaultError> {
        if !is_tag(tag) {
            return Err(VaultError::BadTag);
        }
        let (notes, skipped) = self.collect()?;
        let mut notes: Vec<Note> = notes
            .into_iter()
            .filter(|n| n.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
            .collect();
        notes.sort_by(recent_order);
        let matched = notes.len();
        notes.truncate(limit);
        Ok(RecentOut {
            matched,
            skipped,
            notes,
        })
    }

    pub(crate) fn backlinks(&self, query: &str, limit: usize) -> Result<BackOut, VaultError> {
        let query = clean_query(query)?;
        let (notes, skipped) = self.collect()?;
        let resolved = resolve_in(&notes, &query);
        if resolved.ambiguous {
            return Err(VaultError::Ambiguous);
        }
        let mut hits = Vec::new();
        for note in &notes {
            if resolved.note.as_ref().is_some_and(|t| t.path == note.path) {
                continue;
            }
            let Some(via) = note
                .links
                .iter()
                .find(|link| link_matches(link, &query, resolved.note.as_ref()))
            else {
                continue;
            };
            hits.push(Backlink {
                path: note.path.clone(),
                title: note.title.clone(),
                via: via.clone(),
            });
        }
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        let matched = hits.len();
        hits.truncate(limit);
        Ok(BackOut {
            query,
            resolved: resolved.note.map(|n| n.path),
            matched,
            skipped,
            notes: hits,
        })
    }

    pub(crate) fn create_draft(
        &self,
        title: &str,
        body: &str,
        tags: &[String],
        stamp: &str,
    ) -> Result<Note, VaultError> {
        let title = clean_title(title)?;
        check_body(body)?;
        check_stamp(stamp)?;
        let tags = clean_tags(tags)?;
        self.ensure_dir("drafts")?;
        let rel = self.vacant("drafts", &slugify(title))?;
        let text = render(title, &tags, stamp, body);
        self.write_new(&rel, text.as_bytes())?;
        self.read(&rel)
    }

    pub(crate) fn link(&self, from: &str, to: &str, stamp: &str) -> Result<LinkOut, VaultError> {
        check_stamp(stamp)?;
        let (notes, _) = self.collect()?;
        let from_res = resolve_in(&notes, from.trim());
        let to_res = resolve_in(&notes, to.trim());
        if from_res.ambiguous || to_res.ambiguous {
            return Err(VaultError::Ambiguous);
        }
        let to_label = match &to_res.note {
            Some(note) => wiki_label(&note.path)?,
            None => wiki_label(to)?,
        };
        let from_label = match &from_res.note {
            Some(note) => wiki_label(&note.path)?,
            None => wiki_label(from)?,
        };
        if let Some(note) = from_res.note.as_ref() {
            if is_draft(&note.path) {
                return self.append_link(note, &from_label, &to_label);
            }
        }
        self.propose(&notes, &from_label, &to_label, stamp)
    }

    fn append_link(
        &self,
        note: &Note,
        from_label: &str,
        to_label: &str,
    ) -> Result<LinkOut, VaultError> {
        let needle = format!("[[{to_label}]]");
        let already = note.links.iter().any(|l| l == to_label) || note.body.contains(&needle);
        if already {
            return Ok(LinkOut {
                kind: "append",
                path: note.path.clone(),
                wrote: false,
                from: from_label.to_string(),
                to: to_label.to_string(),
            });
        }
        let path = self.checked_path(&note.path, true)?;
        let meta = fs::symlink_metadata(&path).map_err(|_| VaultError::NotFound)?;
        if meta.file_type().is_symlink() || !meta.is_file() || !is_draft(&note.path) {
            return Err(VaultError::BadPath);
        }
        let existing = fs::read(&path).map_err(|_| VaultError::Io)?;
        let prefix = if existing.ends_with(b"\n") { "" } else { "\n" };
        {
            let mut file = OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|_| VaultError::Io)?;
            writeln!(file, "{prefix}{needle}").map_err(|_| VaultError::Io)?;
            file.flush().map_err(|_| VaultError::Io)?;
        }
        Ok(LinkOut {
            kind: "append",
            path: note.path.clone(),
            wrote: true,
            from: from_label.to_string(),
            to: to_label.to_string(),
        })
    }

    fn propose(
        &self,
        notes: &[Note],
        from_label: &str,
        to_label: &str,
        stamp: &str,
    ) -> Result<LinkOut, VaultError> {
        if let Some(path) = find_proposal(notes, from_label, to_label) {
            return Ok(LinkOut {
                kind: "proposal",
                path,
                wrote: false,
                from: from_label.to_string(),
                to: to_label.to_string(),
            });
        }
        self.ensure_dir("drafts/links")?;
        let slug = proposal_slug(from_label, to_label);
        let rel = self.vacant("drafts/links", &slug)?;
        let body = format!("Proposed link from [[{from_label}]] to [[{to_label}]].");
        let text = render("Proposed link", &["link".to_string()], stamp, &body);
        self.write_new(&rel, text.as_bytes())?;
        Ok(LinkOut {
            kind: "proposal",
            path: rel,
            wrote: true,
            from: from_label.to_string(),
            to: to_label.to_string(),
        })
    }

    fn write_new(&self, rel: &str, bytes: &[u8]) -> Result<(), VaultError> {
        if !is_draft(rel) {
            return Err(VaultError::BadPath);
        }
        let path = self.checked_path(rel, false)?;
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| VaultError::Io)?;
            file.write_all(bytes).map_err(|_| VaultError::Io)?;
            file.flush().map_err(|_| VaultError::Io)?;
        }
        Ok(())
    }

    fn vacant(&self, dir: &str, slug: &str) -> Result<String, VaultError> {
        for i in 0..DRAFT_TRIES {
            let rel = if i == 0 {
                format!("{dir}/{slug}.md")
            } else {
                format!("{dir}/{slug}-{}.md", i + 1)
            };
            let path = self.checked_path(&rel, false)?;
            match fs::symlink_metadata(&path) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(rel),
                Ok(meta) if meta.file_type().is_symlink() => return Err(VaultError::BadPath),
                Ok(_) => {}
                Err(_) => return Err(VaultError::Io),
            }
        }
        Err(VaultError::Exists)
    }

    fn ensure_dir(&self, rel: &str) -> Result<(), VaultError> {
        let parts = split_rel(rel)?;
        let mut cur = self.root.clone();
        for part in parts {
            cur.push(part);
            match fs::symlink_metadata(&cur) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(VaultError::BadPath),
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(VaultError::BadPath),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&cur).map_err(|_| VaultError::Io)?;
                }
                Err(_) => return Err(VaultError::Io),
            }
        }
        Ok(())
    }

    /// `must_exist` is set for reads. A missing final component is fine when
    /// the caller is about to create that file.
    fn checked_path(&self, rel: &str, must_exist: bool) -> Result<PathBuf, VaultError> {
        let parts = split_rel(rel)?;
        let mut cur = self.root.clone();
        let last = parts.len() - 1;
        for (i, part) in parts.iter().enumerate() {
            cur.push(part);
            match fs::symlink_metadata(&cur) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(VaultError::BadPath),
                Ok(meta) if i != last && !meta.is_dir() => return Err(VaultError::BadPath),
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if must_exist || i != last {
                        return Err(VaultError::NotFound);
                    }
                }
                Err(_) => return Err(VaultError::Io),
            }
        }
        if !cur.starts_with(&self.root) {
            return Err(VaultError::BadPath);
        }
        Ok(cur)
    }

    fn collect(&self) -> Result<(Vec<Note>, u32), VaultError> {
        let mut notes = Vec::new();
        let mut skipped = 0u32;
        let mut dirs = vec![self.root.clone()];
        while let Some(dir) = dirs.pop() {
            let entries = fs::read_dir(&dir).map_err(|_| VaultError::Io)?;
            for entry in entries {
                let entry = entry.map_err(|_| VaultError::Io)?;
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    skipped += 1;
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                let ft = entry.file_type().map_err(|_| VaultError::Io)?;
                if ft.is_symlink() {
                    skipped += 1;
                    continue;
                }
                let path = entry.path();
                if ft.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if !ft.is_file() || !name.ends_with(".md") {
                    continue;
                }
                let meta = entry.metadata().map_err(|_| VaultError::Io)?;
                if meta.len() > MAX_NOTE_BYTES {
                    skipped += 1;
                    continue;
                }
                let rel = match rel_of(&self.root, &path) {
                    Ok(rel) => rel,
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                };
                match self.read_note_file(&path, &rel, &meta) {
                    Ok(note) => {
                        notes.push(note);
                        if notes.len() > MAX_NOTES {
                            return Err(VaultError::TooMany);
                        }
                    }
                    Err(VaultError::TooBig | VaultError::BadPath) => skipped += 1,
                    Err(err) => return Err(err),
                }
            }
        }
        notes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((notes, skipped))
    }

    fn read_note_file(
        &self,
        path: &Path,
        rel: &str,
        meta: &fs::Metadata,
    ) -> Result<Note, VaultError> {
        if meta.len() > MAX_NOTE_BYTES {
            return Err(VaultError::TooBig);
        }
        let bytes = fs::read(path).map_err(|_| VaultError::Io)?;
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len > MAX_NOTE_BYTES {
            return Err(VaultError::TooBig);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| VaultError::BadPath)?;
        let mtime = mtime_stamp(meta);
        Ok(parse_text(rel, text, mtime.as_deref(), len))
    }
}

fn recent_order(a: &Note, b: &Note) -> std::cmp::Ordering {
    match (a.updated.is_empty(), b.updated.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => b.updated.cmp(&a.updated).then(a.path.cmp(&b.path)),
    }
}

fn is_draft(path: &str) -> bool {
    path == "drafts" || path.starts_with("drafts/")
}

fn find_proposal(notes: &[Note], from: &str, to: &str) -> Option<String> {
    notes.iter().find_map(|note| {
        let under = note.path.starts_with("drafts/links/");
        let has_from = note.links.iter().any(|l| l == from);
        let has_to = note.links.iter().any(|l| l == to);
        (under && has_from && has_to).then(|| note.path.clone())
    })
}

fn resolve_in(notes: &[Note], query: &str) -> Resolved {
    let query = query.trim();
    if let Ok(parts) = split_rel(query) {
        let rel = parts.join("/");
        let with_md = if rel.ends_with(".md") {
            rel.clone()
        } else {
            format!("{rel}.md")
        };
        if let Some(note) = notes.iter().find(|n| n.path == rel || n.path == with_md) {
            return Resolved {
                note: Some(note.clone()),
                ambiguous: false,
            };
        }
    }
    let mut hits = notes
        .iter()
        .filter(|n| n.title.eq_ignore_ascii_case(query))
        .cloned();
    match (hits.next(), hits.next()) {
        (None, _) => Resolved {
            note: None,
            ambiguous: false,
        },
        (Some(note), None) => Resolved {
            note: Some(note),
            ambiguous: false,
        },
        (Some(_), Some(_)) => Resolved {
            note: None,
            ambiguous: true,
        },
    }
}

fn link_matches(link: &str, query: &str, target: Option<&Note>) -> bool {
    if link.eq_ignore_ascii_case(query) {
        return true;
    }
    let Some(target) = target else {
        return false;
    };
    if link == target.path || link.eq_ignore_ascii_case(&target.title) {
        return true;
    }
    if target
        .path
        .strip_suffix(".md")
        .is_some_and(|stem| link == stem)
    {
        return true;
    }
    target
        .path
        .rsplit('/')
        .next()
        .and_then(|base| base.strip_suffix(".md"))
        .is_some_and(|stem| link.eq_ignore_ascii_case(stem))
}

fn score_note(note: &Note, query: &str) -> usize {
    let tags: usize = note.tags.iter().map(|tag| count_ci(tag, query)).sum();
    count_ci(&note.title, query) * 3 + tags * 2 + count_ci(&note.body, query)
}

pub(crate) fn snippet_of(note: &Note, query: &str) -> String {
    if find_ci(&note.body, query).is_some() {
        snippet(&note.body, query)
    } else {
        clip_chars(&note.title, 160)
    }
}

fn snippet(text: &str, query: &str) -> String {
    let Some(at) = find_ci(text, query) else {
        return clip_chars(text, 160);
    };
    let start = retreat_chars(text, at, 40);
    let after = at + query.len();
    let end = advance_chars(text, after.min(text.len()), 80);
    let mut out = String::new();
    if start > 0 {
        out.push_str("...");
    }
    out.push_str(&text[start..end]);
    if end < text.len() {
        out.push_str("...");
    }
    out
}

fn clip_chars(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        return text.to_string();
    }
    let mut out: String = text.chars().take(n).collect();
    out.push_str("...");
    out
}

fn retreat_chars(text: &str, at: usize, n: usize) -> usize {
    let mut start = at;
    let mut seen = 0;
    for (i, _) in text[..at].char_indices().rev() {
        start = i;
        seen += 1;
        if seen == n {
            break;
        }
    }
    start
}

fn advance_chars(text: &str, at: usize, n: usize) -> usize {
    let mut end = at;
    let mut seen = 0;
    for (i, ch) in text[at..].char_indices() {
        end = at + i + ch.len_utf8();
        seen += 1;
        if seen == n {
            break;
        }
    }
    end
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let bytes = hay.as_bytes();
    let want = needle.as_bytes();
    let mut i = 0;
    while i + want.len() <= bytes.len() {
        let end = i + want.len();
        if hay.is_char_boundary(i)
            && hay.is_char_boundary(end)
            && bytes[i..end]
                .iter()
                .zip(want)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Some(i);
        }
        i += 1;
        while i < bytes.len() && !hay.is_char_boundary(i) {
            i += 1;
        }
    }
    None
}

fn count_ci(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut rest = hay;
    let mut n = 0;
    while let Some(at) = find_ci(rest, needle) {
        n += 1;
        let next = at + needle.len();
        if next > rest.len() {
            break;
        }
        rest = &rest[next..];
    }
    n
}

fn clean_query(query: &str) -> Result<String, VaultError> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err(VaultError::BadQuery);
    }
    Ok(query.to_string())
}

fn clean_title(title: &str) -> Result<&str, VaultError> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 120 || title.chars().any(char::is_control) {
        return Err(VaultError::BadTitle);
    }
    Ok(title)
}

fn check_body(body: &str) -> Result<(), VaultError> {
    if body.len() > MAX_BODY_BYTES || body.contains('\0') {
        return Err(VaultError::BadBody);
    }
    Ok(())
}

fn check_stamp(stamp: &str) -> Result<(), VaultError> {
    zagreb_time::require_rfc3339(stamp).map_err(|_| VaultError::BadStamp)
}

fn clean_tags(tags: &[String]) -> Result<Vec<String>, VaultError> {
    if tags.len() > MAX_TAGS {
        return Err(VaultError::BadTag);
    }
    let mut out = Vec::new();
    for tag in tags {
        if !is_tag(tag) {
            return Err(VaultError::BadTag);
        }
        push_tag(&mut out, tag);
    }
    Ok(out)
}

fn wiki_label(raw: &str) -> Result<String, VaultError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().count() > 200 {
        return Err(VaultError::BadLink);
    }
    if raw
        .chars()
        .any(|c| c.is_control() || matches!(c, '[' | ']' | '|' | '#' | '\\'))
    {
        return Err(VaultError::BadLink);
    }
    if raw.starts_with('/') {
        return Err(VaultError::BadLink);
    }
    for part in raw.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.starts_with('.') {
            return Err(VaultError::BadLink);
        }
    }
    Ok(raw.to_string())
}

fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in title.chars() {
        if out.len() >= MAX_SLUG {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "note".to_string()
    } else {
        trimmed.to_string()
    }
}

fn proposal_slug(from: &str, to: &str) -> String {
    let mut slug = format!("{}--{}", slugify(from), slugify(to));
    if slug.len() > MAX_SLUG {
        slug.truncate(MAX_SLUG);
        slug = slug.trim_end_matches('-').to_string();
    }
    if slug.is_empty() {
        "link".to_string()
    } else {
        slug
    }
}

fn render(title: &str, tags: &[String], stamp: &str, body: &str) -> String {
    let mut tag_line = String::from("tags: [");
    for (i, tag) in tags.iter().enumerate() {
        if i > 0 {
            tag_line.push_str(", ");
        }
        tag_line.push_str(tag);
    }
    tag_line.push(']');
    let body = body.trim_matches(['\n', '\r']);
    format!("---\ntitle: {title}\n{tag_line}\ncreated: {stamp}\nupdated: {stamp}\n---\n\n{body}\n")
}

fn split_rel(rel: &str) -> Result<Vec<&str>, VaultError> {
    if rel.is_empty()
        || rel.len() > MAX_REL
        || rel.contains('\0')
        || rel.contains('\\')
        || rel.starts_with('/')
    {
        return Err(VaultError::BadPath);
    }
    let mut parts = Vec::new();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.starts_with('.') {
            return Err(VaultError::BadPath);
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(VaultError::BadPath);
    }
    Ok(parts)
}

fn rel_of(root: &Path, path: &Path) -> Result<String, VaultError> {
    let rel = path.strip_prefix(root).map_err(|_| VaultError::BadPath)?;
    let mut parts = Vec::new();
    for component in rel.components() {
        let Component::Normal(name) = component else {
            return Err(VaultError::BadPath);
        };
        let Some(name) = name.to_str() else {
            return Err(VaultError::BadPath);
        };
        if name.is_empty() || name.starts_with('.') || name == ".." {
            return Err(VaultError::BadPath);
        }
        parts.push(name);
    }
    if parts.is_empty() {
        return Err(VaultError::BadPath);
    }
    Ok(parts.join("/"))
}

fn mtime_stamp(meta: &fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let secs = i64::try_from(secs).ok()?;
    Some(zagreb_time::from_unix(secs))
}

fn parse_text(rel: &str, text: &str, mtime: Option<&str>, bytes: u64) -> Note {
    let (front, body) = split_frontmatter(text);
    let mut title = stem(rel).to_string();
    let mut tags = Vec::new();
    let mut created = String::new();
    let mut updated_fm: Option<String> = None;
    if let Some(front) = front {
        let mut in_tags = false;
        for line in front.lines() {
            let trimmed = line.trim();
            if let Some(value) = strip_key(trimmed, "title") {
                in_tags = false;
                if !value.is_empty() {
                    title = value.to_string();
                }
            } else if let Some(value) = strip_key(trimmed, "tags") {
                if value.is_empty() {
                    in_tags = true;
                } else {
                    in_tags = false;
                    for tag in parse_tag_list(value) {
                        push_tag(&mut tags, &tag);
                    }
                }
            } else if let Some(value) = strip_key(trimmed, "updated") {
                in_tags = false;
                updated_fm = Some(value.to_string());
            } else if let Some(value) = strip_key(trimmed, "created") {
                in_tags = false;
                created = value.to_string();
            } else if in_tags {
                if let Some(value) = trimmed.strip_prefix("- ") {
                    let value = unquote(value.trim());
                    if is_tag(value) {
                        push_tag(&mut tags, value);
                    }
                } else if !trimmed.is_empty() {
                    in_tags = false;
                }
            }
        }
    }
    for line in visible_lines(body) {
        inline_tags(line, &mut tags);
    }
    let links = wikilinks(body);
    let updated = match updated_fm.as_deref() {
        Some(stamp) if zagreb_time::require_rfc3339(stamp).is_ok() => stamp.to_string(),
        _ => mtime.unwrap_or("").to_string(),
    };
    if zagreb_time::require_rfc3339(&created).is_err() {
        created.clear();
    }
    Note {
        path: rel.to_string(),
        title,
        tags,
        links,
        created,
        updated,
        body: body.trim_matches(['\n', '\r']).to_string(),
        bytes,
    }
}

fn stem(rel: &str) -> &str {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    base.strip_suffix(".md").unwrap_or(base)
}

fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let rest = if let Some(rest) = text.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = text.strip_prefix("---\r\n") {
        rest
    } else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare == "---" {
            let front = rest[..offset].trim_end_matches(['\n', '\r']);
            let body = rest[offset + line.len()..].trim_start_matches(['\n', '\r']);
            return (Some(front), body);
        }
        offset += line.len();
    }
    (None, text)
}

fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.trim_start().strip_prefix(':')?;
    Some(unquote(rest.trim()))
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let open = bytes[0];
        let close = bytes[bytes.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn parse_tag_list(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let raw = raw.strip_prefix('[').unwrap_or(raw);
    let raw = raw.strip_suffix(']').unwrap_or(raw);
    let mut out = Vec::new();
    for part in raw.split([',', ' ', '\t']) {
        let part = unquote(part.trim());
        if is_tag(part) {
            push_tag(&mut out, part);
        }
    }
    out
}

fn is_tag(raw: &str) -> bool {
    let mut chars = raw.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) {
        return false;
    }
    (1..=64).contains(&raw.len())
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn push_tag(tags: &mut Vec<String>, tag: &str) {
    if tags.iter().any(|have| have.eq_ignore_ascii_case(tag)) {
        return;
    }
    tags.push(tag.to_string());
}

fn visible_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push(line);
        }
    }
    out
}

fn inline_tags(line: &str, tags: &mut Vec<String>) {
    let mut prev_space = true;
    for (i, ch) in line.char_indices() {
        if ch == '#' && prev_space {
            let rest = &line[i + 1..];
            let end = rest.find(|c: char| !is_tag_char(c)).unwrap_or(rest.len());
            let raw = &rest[..end];
            if is_tag(raw) {
                push_tag(tags, raw);
            }
        }
        prev_space = ch.is_whitespace();
    }
}

fn wikilinks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in visible_lines(text) {
        let mut rest = line;
        while let Some(start) = rest.find("[[") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find("]]") else {
                break;
            };
            let raw = &rest[..end];
            rest = &rest[end + 2..];
            let target = raw.split('|').next().unwrap_or(raw);
            let target = target.split('#').next().unwrap_or(target).trim();
            if target.is_empty() || target.len() > 200 || target.contains(['[', ']', '\n']) {
                continue;
            }
            if !out.iter().any(|have: &String| have == target) {
                out.push(target.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: &str = "2026-10-19T16:39:10+02:00";

    #[test]
    fn frontmatter_tags_links_and_fences() {
        let text = "\
---
title: Photon
tags: [wave, light]
updated: 2026-10-19T16:39:10+02:00
---

A photon is light. See [[Matter|mass]] and [[Matter#spin]].
#shown
```
#hidden
[[Nope]]
```
";
        let note = parse_text(
            "physics/photon.md",
            text,
            Some(STAMP),
            u64::try_from(text.len()).unwrap_or(0),
        );
        assert_eq!(note.title, "Photon");
        assert_eq!(
            note.tags,
            vec!["wave".to_string(), "light".to_string(), "shown".to_string()]
        );
        assert_eq!(note.links, vec!["Matter".to_string()]);
        assert_eq!(note.updated, STAMP);
        assert!(!note.tags.iter().any(|t| t == "hidden"));
    }

    #[test]
    fn yaml_tag_list_and_zulu_stamp_falls_back() {
        let zulu = format!("2026-10-19T14:39:10{}", "Z");
        let text =
            format!("---\ntitle: T\ntags:\n  - wave\n  - light\nupdated: {zulu}\n---\n\nbody\n");
        let note = parse_text("t.md", &text, Some(STAMP), 1);
        assert_eq!(note.tags, vec!["wave".to_string(), "light".to_string()]);
        assert_eq!(note.updated, STAMP);
        assert!(!note.updated.contains('Z'));
    }

    #[test]
    fn slug_and_path_rules() {
        assert_eq!(slugify("Photon notes"), "photon-notes");
        assert_eq!(slugify("..."), "note");
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert!(split_rel("../x").is_err());
        assert!(split_rel("/etc/hostname").is_err());
        assert!(split_rel("drafts/../x.md").is_err());
        assert!(split_rel(".secret.md").is_err());
        assert_eq!(
            split_rel("physics/photon.md").unwrap(),
            ["physics", "photon.md"]
        );
    }

    #[test]
    fn score_counts_title_heavier_than_body() {
        let note = Note {
            path: "a.md".into(),
            title: "Photon".into(),
            tags: vec!["wave".into()],
            links: Vec::new(),
            created: String::new(),
            updated: STAMP.into(),
            body: "A photon has a wavelength.".into(),
            bytes: 1,
        };
        assert_eq!(score_note(&note, "wavelength"), 1);
        assert_eq!(score_note(&note, "Photon"), 4);
        assert_eq!(score_note(&note, "wave"), 3);
    }
}
