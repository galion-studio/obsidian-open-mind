// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Local file attach for the Galion Research prompt.
//!
//! `POST /hermes/attach` and `POST /research/attach` keep a copy on this
//! machine. The handler does not open a path from the request, does not
//! dial the Hermes gateway, and does not put the bytes in the redirect.
//! The display name is a single safe component. The file on disk is named
//! with a hex id under a private directory.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Largest file kept. Matches the desktop picker's cap.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Request body cap: the file, plus a short multipart envelope.
pub const MAX_BODY: usize = MAX_BYTES + 8 * 1024;

/// How many files one process keeps. Older ones are not dropped; the next
/// upload is refused.
pub const MAX_FILES: usize = 32;

/// One extension this attach will keep.
struct Kind {
    ext: &'static str,
    kind: &'static str,
}

/// Text, PDF, and the picture types the desktop picker already allows.
const ALLOWED: &[Kind] = &[
    Kind {
        ext: "txt",
        kind: "text",
    },
    Kind {
        ext: "md",
        kind: "text",
    },
    Kind {
        ext: "json",
        kind: "text",
    },
    Kind {
        ext: "csv",
        kind: "text",
    },
    Kind {
        ext: "log",
        kind: "text",
    },
    Kind {
        ext: "pdf",
        kind: "pdf",
    },
    Kind {
        ext: "jpg",
        kind: "image",
    },
    Kind {
        ext: "jpeg",
        kind: "image",
    },
    Kind {
        ext: "png",
        kind: "image",
    },
    Kind {
        ext: "webp",
        kind: "image",
    },
];

/// Why an upload was not kept. The token is the only thing that reaches a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Zero bytes.
    Empty,
    /// Larger than [`MAX_BYTES`].
    TooLarge,
    /// Extension is not on the allow-list.
    Type,
    /// Bytes do not match the extension.
    Magic,
    /// The name contains a separator or `..`.
    Traversal,
    /// The name is empty or not a single safe component.
    BadName,
    /// The body was not one multipart file.
    Bad,
    /// [`MAX_FILES`] already kept.
    Full,
    /// The local directory could not take the copy.
    Io,
}

impl Code {
    /// Stable query token. No path and no file bytes.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Code::Empty => "empty",
            Code::TooLarge => "too-large",
            Code::Type => "type",
            Code::Magic => "magic",
            Code::Traversal => "traversal",
            Code::BadName => "bad-name",
            Code::Bad => "bad",
            Code::Full => "full",
            Code::Io => "io",
        }
    }

    /// Sentence for the page. Fixed text, nothing from the request.
    #[must_use]
    pub fn note(self) -> &'static str {
        match self {
            Code::Empty => "That file is empty.",
            Code::TooLarge => "That file is over the 8 MiB limit.",
            Code::Type => "That file type is not allowed.",
            Code::Magic => "That file does not match its type.",
            Code::Traversal => "That file name was refused.",
            Code::BadName => "That file name is not usable.",
            Code::Bad => "That upload was not a single file.",
            Code::Full => "The local attach folder is full.",
            Code::Io => "That file could not be kept on this machine.",
        }
    }
}

/// Sentence for a known `attach` query token. Anything else is ignored.
#[must_use]
pub fn fault_note(token: &str) -> Option<&'static str> {
    Some(match token {
        "empty" => Code::Empty.note(),
        "too-large" => Code::TooLarge.note(),
        "type" => Code::Type.note(),
        "magic" => Code::Magic.note(),
        "traversal" => Code::Traversal.note(),
        "bad-name" => Code::BadName.note(),
        "bad" => Code::Bad.note(),
        "full" => Code::Full.note(),
        "io" => Code::Io.note(),
        _ => return None,
    })
}

/// `POST` targets that store a file locally.
#[must_use]
pub fn is_attach_path(path: &str) -> bool {
    matches!(path, "/hermes/attach" | "/research/attach")
}

/// Body cap for one UI request. Only the attach posts may exceed [`crate::MAX_FORM`].
#[must_use]
pub fn body_limit(method: &str, path: &str) -> usize {
    if method == "POST" && is_attach_path(path) {
        MAX_BODY
    } else if method == "POST" && path == crate::connectors::PATH {
        // A pasted token is up to 4096 bytes, plus the csrf field.
        16 * 1024
    } else {
        crate::MAX_FORM
    }
}

/// Page the upload returns to. Derived from the path, not from the body.
#[must_use]
pub fn back_path(path: &str) -> &'static str {
    if path == "/research/attach" {
        "/research"
    } else {
        "/hermes"
    }
}

/// One kept file, for the chip under the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// Sanitized display name.
    pub name: String,
    /// `text`, `pdf`, or `image`.
    pub kind: &'static str,
    /// Size in bytes.
    pub size: usize,
}

/// Files kept for this process. The directory is created on the first
/// accepted file, private to this process, and is not a path from the request.
pub struct Store {
    root: Mutex<Option<PathBuf>>,
    max_bytes: usize,
    max_files: usize,
    items: Mutex<Vec<Chip>>,
    seq: AtomicU64,
    open: bool,
}

impl Store {
    /// Empty until the first accepted file. No directory is created before that.
    #[must_use]
    pub fn create() -> Self {
        Self::at(None, MAX_BYTES, MAX_FILES, true)
    }

    fn at(root: Option<PathBuf>, max_bytes: usize, max_files: usize, open: bool) -> Self {
        Self {
            root: Mutex::new(root),
            max_bytes,
            max_files,
            items: Mutex::new(Vec::new()),
            seq: AtomicU64::new(1),
            open,
        }
    }

    fn root(&self) -> Result<PathBuf, Code> {
        if !self.open {
            return Err(Code::Io);
        }
        let mut slot = self.root.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(path) = slot.clone() {
            return Ok(path);
        }
        for _ in 0..4 {
            let dir = std::env::temp_dir().join(format!("galion-research-{}", rand_hex()));
            if create_private_dir(&dir).is_ok() {
                *slot = Some(dir.clone());
                return Ok(dir);
            }
        }
        Err(Code::Io)
    }

    /// Chips in arrival order. Empty when the store could not be created.
    #[must_use]
    pub fn list(&self) -> Vec<Chip> {
        self.items
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Check the upload and, if it passes, keep a copy under a hex id.
    ///
    /// # Errors
    /// See [`Code`]. Nothing is written on the error paths.
    pub fn save(&self, name: &str, bytes: &[u8]) -> Result<Chip, Code> {
        if !self.open {
            return Err(Code::Io);
        }
        let name = clean_name(name)?;
        if bytes.is_empty() {
            return Err(Code::Empty);
        }
        if bytes.len() > self.max_bytes {
            return Err(Code::TooLarge);
        }
        let kind = classify(&name, bytes)?;
        let mut items = self.items.lock().unwrap_or_else(|err| err.into_inner());
        if items.len() >= self.max_files {
            return Err(Code::Full);
        }
        let root = self.root()?;
        let id = self.alloc_id();
        write_private(&root, &id, bytes)?;
        let chip = Chip {
            name,
            kind,
            size: bytes.len(),
        };
        items.push(chip.clone());
        Ok(chip)
    }

    fn alloc_id(&self) -> String {
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{n:08x}{tick:08x}")
    }
}

/// Read one `file` part from a `multipart/form-data` body.
///
/// # Errors
/// [`Code::Bad`] when the body is not that one file. [`Code::TooLarge`] when
/// the part itself is over [`MAX_BYTES`].
pub fn read_upload(content_type: &str, body: &[u8]) -> Result<(String, Vec<u8>), Code> {
    let boundary = boundary(content_type).ok_or(Code::Bad)?;
    let marker = format!("--{boundary}");
    let marker = marker.as_bytes();
    if body.len() < marker.len() + 4 {
        return Err(Code::Bad);
    }
    let mut cursor = if body.starts_with(marker) {
        marker.len()
    } else {
        let mut lead = b"\r\n".to_vec();
        lead.extend_from_slice(marker);
        let Some(at) = find_sub(body, &lead) else {
            return Err(Code::Bad);
        };
        at + lead.len()
    };
    let mut found: Option<(String, Vec<u8>)> = None;
    let mut parts = 0usize;
    loop {
        if parts > 4 {
            return Err(Code::Bad);
        }
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        if body.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        }
        let Some(split) = find_sub(&body[cursor..], b"\r\n\r\n") else {
            return Err(Code::Bad);
        };
        let head = &body[cursor..cursor + split];
        if head.len() > 2048 {
            return Err(Code::Bad);
        }
        cursor += split + 4;
        let next = find_sub(&body[cursor..], marker).ok_or(Code::Bad)?;
        let mut data = body[cursor..cursor + next].to_vec();
        if data.ends_with(b"\r\n") {
            data.truncate(data.len() - 2);
        }
        cursor += next + marker.len();
        parts += 1;
        let head = std::str::from_utf8(head).map_err(|_| Code::Bad)?;
        if let Some(name) = file_name(head)? {
            if found.is_some() {
                return Err(Code::Bad);
            }
            if data.len() > MAX_BYTES {
                return Err(Code::TooLarge);
            }
            found = Some((name, data));
        }
    }
    found.ok_or(Code::Bad)
}

/// Accept one upload into `store`.
///
/// # Errors
/// See [`Code`].
pub fn keep(store: &Store, content_type: &str, body: &[u8]) -> Result<Chip, Code> {
    let (name, bytes) = read_upload(content_type, body)?;
    store.save(&name, &bytes)
}

fn boundary(content_type: &str) -> Option<&str> {
    let mut parts = content_type.split(';');
    let kind = parts.next()?.trim();
    if !kind.eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    for part in parts {
        let part = part.trim();
        let value = part.strip_prefix("boundary=")?.trim().trim_matches('"');
        if (1..=70).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'"' && b != b';')
        {
            return Some(value);
        }
    }
    None
}

fn file_name(head: &str) -> Result<Option<String>, Code> {
    let mut disp = None;
    for line in head.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-disposition") {
            disp = Some(value.trim());
        }
    }
    let Some(disp) = disp else {
        return Ok(None);
    };
    if !disp.to_ascii_lowercase().contains("form-data") {
        return Err(Code::Bad);
    }
    let field = param(disp, "name").ok_or(Code::Bad)?;
    if field != "file" {
        return Ok(None);
    }
    if let Some(star) = param(disp, "filename*") {
        return Ok(Some(decode_star(&star)?));
    }
    param(disp, "filename").map(Some).ok_or(Code::Bad)
}

fn param(disp: &str, key: &str) -> Option<String> {
    let mut rest = disp;
    let needle = format!("{key}=");
    while let Some(at) = rest.find(&needle) {
        let before = rest[..at].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '*') {
            rest = &rest[at + needle.len()..];
            continue;
        }
        let value = &rest[at + needle.len()..];
        if let Some(quoted) = value.strip_prefix('"') {
            let mut out = String::new();
            let mut chars = quoted.chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    out.push(chars.next().unwrap_or('\\'));
                } else if ch == '"' {
                    return Some(out);
                } else {
                    out.push(ch);
                }
            }
            return None;
        }
        let end = value.find([';', ' ', '\t']).unwrap_or(value.len());
        return Some(value[..end].to_string());
    }
    None
}

fn decode_star(raw: &str) -> Result<String, Code> {
    let encoded = raw
        .strip_prefix("UTF-8''")
        .or_else(|| raw.strip_prefix("utf-8''"))
        .ok_or(Code::BadName)?;
    let mut out = Vec::new();
    let bytes = encoded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(Code::BadName);
            }
            let h = hex(bytes[i + 1]).ok_or(Code::BadName)?;
            let l = hex(bytes[i + 2]).ok_or(Code::BadName)?;
            out.push((h << 4) | l);
            i += 3;
        } else if bytes[i].is_ascii() && bytes[i] != b'+' {
            out.push(bytes[i]);
            i += 1;
        } else {
            return Err(Code::BadName);
        }
    }
    String::from_utf8(out).map_err(|_| Code::BadName)
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// A single display name: no separator, no `..`, an allowed extension.
///
/// # Errors
/// [`Code::Traversal`], [`Code::BadName`], or [`Code::Type`].
pub fn clean_name(raw: &str) -> Result<String, Code> {
    if raw.is_empty() || raw.len() > 200 {
        return Err(Code::BadName);
    }
    if raw.chars().any(|c| c.is_control()) {
        return Err(Code::BadName);
    }
    if raw.contains('/') || raw.contains('\\') || raw.contains("..") {
        return Err(Code::Traversal);
    }
    let name = raw.trim();
    if name.is_empty()
        || name.starts_with('.')
        || name.len() > 80
        || name.ends_with('.')
        || name.ends_with(' ')
    {
        return Err(Code::BadName);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
    {
        return Err(Code::BadName);
    }
    let ext = extension(name).ok_or(Code::Type)?;
    if ALLOWED.iter().any(|item| item.ext == ext) {
        Ok(name.to_string())
    } else {
        Err(Code::Type)
    }
}

fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 8 || !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

fn classify(name: &str, bytes: &[u8]) -> Result<&'static str, Code> {
    let ext = extension(name).ok_or(Code::Type)?;
    let spec = ALLOWED
        .iter()
        .find(|item| item.ext == ext)
        .ok_or(Code::Type)?;
    if magic_ok(&ext, bytes) {
        Ok(spec.kind)
    } else {
        Err(Code::Magic)
    }
}

fn magic_ok(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "txt" | "md" | "json" | "csv" | "log" => {
            !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
        }
        "pdf" => bytes.starts_with(b"%PDF"),
        "jpg" | "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rand_hex() -> String {
    let mut buf = [0u8; 8];
    if let Ok(mut file) = File::open("/dev/urandom") {
        use std::io::Read;
        if file.read_exact(&mut buf).is_ok() {
            return buf.iter().map(|b| format!("{b:02x}")).collect();
        }
    }
    let n = std::process::id() as u64;
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{n:08x}{t:08x}")
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(false)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

fn write_private(root: &Path, id: &str, bytes: &[u8]) -> Result<(), Code> {
    if id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Code::Io);
    }
    let meta = fs::symlink_metadata(root).map_err(|_| Code::Io)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Code::Traversal);
    }
    let path = root.join(id);
    if path.parent() != Some(root) {
        return Err(Code::Traversal);
    }
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|_| Code::Io)?;
    if file.write_all(bytes).is_err() {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(Code::Io);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp {
        path: PathBuf,
    }

    impl Tmp {
        fn new() -> Self {
            let mut buf = [0u8; 8];
            let mut random = File::open("/dev/urandom").unwrap();
            use std::io::Read;
            random.read_exact(&mut buf).unwrap();
            let path = std::env::temp_dir().join(format!("om-attach-{}", hex_of(&buf)));
            fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn hex_of(buf: &[u8]) -> String {
        buf.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn store(dir: &Path, max: usize) -> Store {
        Store::at(Some(dir.to_path_buf()), max, 2, true)
    }

    fn part(name: &str, bytes: &str) -> (String, Vec<u8>) {
        let body = format!(
            "--bound\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n{bytes}\r\n--bound--\r\n"
        );
        (
            "multipart/form-data; boundary=bound".into(),
            body.into_bytes(),
        )
    }

    #[test]
    fn names_stay_a_single_component() {
        assert_eq!(clean_name("notes.txt").unwrap(), "notes.txt");
        assert_eq!(clean_name("NOTES.TXT").unwrap(), "NOTES.TXT");
        assert_eq!(clean_name("../../etc/passwd.txt"), Err(Code::Traversal));
        assert_eq!(clean_name("..\\secret.txt"), Err(Code::Traversal));
        assert_eq!(clean_name("a/../../x.txt"), Err(Code::Traversal));
        assert_eq!(clean_name(".env.txt"), Err(Code::BadName));
        assert_eq!(clean_name("a\nb.txt"), Err(Code::BadName));
        assert_eq!(clean_name("run.exe"), Err(Code::Type));
        assert_eq!(clean_name("page.html"), Err(Code::Type));
        assert!(fault_note("<script>").is_none());
        assert_eq!(fault_note("type"), Some(Code::Type.note()));
        assert!(!Code::Traversal.note().contains('/'));
    }

    #[test]
    fn a_good_file_is_stored_under_a_hex_id() {
        let tmp = Tmp::new();
        let outside = tmp.path.join("secret.txt");
        fs::write(&outside, b"SENTINEL-OUTSIDE").unwrap();
        let box_dir = tmp.path.join("box");
        fs::create_dir(&box_dir).unwrap();
        let files = store(&box_dir, 64);
        let (ct, body) = part("notes.txt", "hello");
        let chip = keep(&files, &ct, &body).unwrap();
        assert_eq!(chip.name, "notes.txt");
        assert_eq!(chip.kind, "text");
        assert_eq!(chip.size, 5);
        assert!(!box_dir.join("notes.txt").exists());
        assert!(!tmp.path.join("etc").exists());
        let kept: Vec<_> = fs::read_dir(&box_dir)
            .unwrap()
            .map(|ent| ent.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != "secret.txt")
            .collect();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(fs::read(box_dir.join(&kept[0])).unwrap(), b"hello");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(box_dir.join(&kept[0]))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(files.list()[0].name, "notes.txt");
        assert_eq!(fs::read(&outside).unwrap(), b"SENTINEL-OUTSIDE");
    }

    #[test]
    fn traversal_and_type_write_nothing() {
        let tmp = Tmp::new();
        let files = store(&tmp.path, 64);
        for (name, bytes) in [
            ("../../etc/passwd.txt", "hello"),
            ("notes.exe", "hello"),
            ("page.html", "<p>"),
            ("notes.pdf", "not a pdf"),
            ("a.txt", ""),
        ] {
            let (ct, body) = part(name, bytes);
            let err = keep(&files, &ct, &body).unwrap_err();
            assert!(
                matches!(
                    err,
                    Code::Traversal | Code::Type | Code::Magic | Code::Empty
                ),
                "{name} -> {err:?}"
            );
        }
        assert!(files.list().is_empty());
        let names: Vec<_> = fs::read_dir(&tmp.path)
            .unwrap()
            .map(|ent| ent.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| n.len() != 16 || !n.bytes().all(|b| b.is_ascii_hexdigit())),
            "{names:?}"
        );
        assert!(!tmp.path.join("etc").exists());
        assert!(!tmp.path.join("passwd.txt").exists());
    }

    #[test]
    fn oversize_is_refused_before_a_file_exists() {
        let tmp = Tmp::new();
        let files = store(&tmp.path, 4);
        let err = files.save("notes.txt", b"0123456789").unwrap_err();
        assert_eq!(err, Code::TooLarge);
        assert!(fs::read_dir(&tmp.path).unwrap().next().is_none());
        let (ct, body) = part("notes.txt", "hello");
        assert_eq!(keep(&files, &ct, &body).unwrap_err(), Code::TooLarge);
    }

    #[test]
    fn encoded_dotdot_is_traversal() {
        let raw = "multipart/form-data; boundary=bound";
        let body = b"--bound\r\nContent-Disposition: form-data; name=\"file\"; filename*=UTF-8''%2e%2e%2fsecret.txt\r\n\r\nhi\r\n--bound--\r\n";
        let (name, bytes) = read_upload(raw, body).unwrap();
        assert_eq!(name, "../secret.txt");
        assert_eq!(bytes, b"hi");
        let tmp = Tmp::new();
        let files = store(&tmp.path, 64);
        assert_eq!(files.save(&name, &bytes).unwrap_err(), Code::Traversal);
        assert!(fs::read_dir(&tmp.path).unwrap().next().is_none());
    }

    #[test]
    fn a_second_file_part_is_refused() {
        let body = b"--bound\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\n\r\nhi\r\n--bound\r\nContent-Disposition: form-data; name=\"file\"; filename=\"b.txt\"\r\n\r\nho\r\n--bound--\r\n";
        let err = read_upload("multipart/form-data; boundary=bound", body).unwrap_err();
        assert_eq!(err, Code::Bad);
    }

    #[test]
    fn caps_match_the_desktop_picker() {
        assert_eq!(MAX_BYTES, 8 * 1024 * 1024);
        assert!(body_limit("POST", "/hermes/attach") > crate::MAX_FORM);
        assert_eq!(body_limit("POST", "/hermes"), crate::MAX_FORM);
        assert_eq!(body_limit("POST", "/connectors"), 16 * 1024);
        assert_eq!(body_limit("GET", "/hermes/attach"), crate::MAX_FORM);
        assert!(is_attach_path("/research/attach"));
        assert!(!is_attach_path("/hermes"));
        assert_eq!(back_path("/research/attach"), "/research");
        assert_eq!(back_path("/hermes/attach"), "/hermes");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_directory_is_not_written() {
        use std::os::unix::fs::symlink;
        let tmp = Tmp::new();
        let real = tmp.path.join("real");
        fs::create_dir(&real).unwrap();
        let link = tmp.path.join("link");
        symlink(&real, &link).unwrap();
        let files = Store::at(Some(link), 64, 2, true);
        let err = files.save("notes.txt", b"hello").unwrap_err();
        assert_eq!(err, Code::Traversal);
        assert!(fs::read_dir(&real).unwrap().next().is_none());
    }
}
