// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The file-type table: extension -> kind, MIME type and editor language.
//! One table for the console explorer and the Hermes tools (`files_kinds`).
//! Names without a useful extension (dotfiles, `Dockerfile`, `Makefile`) have
//! their own table, and [`sniff`] reads magic bytes when neither matches.

use serde_json::{json, Value};

/// How a file is previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Image,
    Video,
    Audio,
    Pdf,
    Markdown,
    /// Source code or config: syntax highlighted, editable.
    Code,
    /// Plain text: editable.
    Text,
    /// zip / rar / tar(.gz): listed, extractable.
    Archive,
    /// Other archives we name but cannot open (7z, xz, ...): metadata + hex.
    Packed,
    Font,
    /// Office documents and the like: metadata + hex + download.
    Document,
    /// Anything else: metadata + hex dump.
    Binary,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Pdf => "pdf",
            Kind::Markdown => "markdown",
            Kind::Code => "code",
            Kind::Text => "text",
            Kind::Archive => "archive",
            Kind::Packed => "packed",
            Kind::Font => "font",
            Kind::Document => "document",
            Kind::Binary => "binary",
        }
    }

    /// Text kinds the editor opens.
    pub fn is_text(self) -> bool {
        matches!(self, Kind::Markdown | Kind::Code | Kind::Text)
    }

    /// What the preview pane does with it.
    pub fn preview(self) -> &'static str {
        match self {
            Kind::Folder => "list",
            Kind::Image => "img",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Pdf => "pdf",
            Kind::Markdown => "markdown",
            Kind::Code | Kind::Text => "editor",
            Kind::Archive => "archive",
            Kind::Packed | Kind::Font | Kind::Document | Kind::Binary => "hex",
        }
    }
}

/// One row: (extension, kind, MIME, editor language or "").
pub type Row = (&'static str, Kind, &'static str, &'static str);

use Kind::*;

/// Lower-case extensions (without the dot).
pub const EXTENSIONS: &[Row] = &[
    // images (browser-native)
    ("png", Image, "image/png", ""),
    ("jpg", Image, "image/jpeg", ""),
    ("jpeg", Image, "image/jpeg", ""),
    ("jfif", Image, "image/jpeg", ""),
    ("gif", Image, "image/gif", ""),
    ("webp", Image, "image/webp", ""),
    ("avif", Image, "image/avif", ""),
    ("bmp", Image, "image/bmp", ""),
    ("ico", Image, "image/x-icon", ""),
    ("svg", Image, "image/svg+xml", "xml"),
    ("apng", Image, "image/apng", ""),
    // images the browser may not show: previewed through an ffmpeg poster
    ("tif", Image, "image/tiff", ""),
    ("tiff", Image, "image/tiff", ""),
    ("heic", Image, "image/heic", ""),
    ("heif", Image, "image/heif", ""),
    // video
    ("mp4", Video, "video/mp4", ""),
    ("m4v", Video, "video/mp4", ""),
    ("mov", Video, "video/quicktime", ""),
    ("webm", Video, "video/webm", ""),
    ("mkv", Video, "video/x-matroska", ""),
    ("avi", Video, "video/x-msvideo", ""),
    ("ogv", Video, "video/ogg", ""),
    ("3gp", Video, "video/3gpp", ""),
    ("ts", Code, "text/plain; charset=utf-8", "typescript"),
    ("mts", Video, "video/mp2t", ""),
    // audio
    ("mp3", Audio, "audio/mpeg", ""),
    ("m4a", Audio, "audio/mp4", ""),
    ("aac", Audio, "audio/aac", ""),
    ("wav", Audio, "audio/wav", ""),
    ("flac", Audio, "audio/flac", ""),
    ("ogg", Audio, "audio/ogg", ""),
    ("oga", Audio, "audio/ogg", ""),
    ("opus", Audio, "audio/ogg", ""),
    ("weba", Audio, "audio/webm", ""),
    // documents
    ("pdf", Pdf, "application/pdf", ""),
    ("md", Markdown, "text/markdown; charset=utf-8", "markdown"),
    (
        "markdown",
        Markdown,
        "text/markdown; charset=utf-8",
        "markdown",
    ),
    ("mdx", Markdown, "text/markdown; charset=utf-8", "markdown"),
    ("txt", Text, "text/plain; charset=utf-8", ""),
    ("log", Text, "text/plain; charset=utf-8", ""),
    ("csv", Text, "text/csv; charset=utf-8", ""),
    ("tsv", Text, "text/tab-separated-values; charset=utf-8", ""),
    ("rst", Text, "text/plain; charset=utf-8", ""),
    ("adoc", Text, "text/plain; charset=utf-8", ""),
    ("srt", Text, "text/plain; charset=utf-8", ""),
    ("vtt", Text, "text/vtt; charset=utf-8", ""),
    ("doc", Document, "application/msword", ""),
    (
        "docx",
        Document,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "",
    ),
    ("xls", Document, "application/vnd.ms-excel", ""),
    (
        "xlsx",
        Document,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "",
    ),
    ("ppt", Document, "application/vnd.ms-powerpoint", ""),
    (
        "pptx",
        Document,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "",
    ),
    (
        "odt",
        Document,
        "application/vnd.oasis.opendocument.text",
        "",
    ),
    (
        "ods",
        Document,
        "application/vnd.oasis.opendocument.spreadsheet",
        "",
    ),
    ("epub", Document, "application/epub+zip", ""),
    ("rtf", Document, "application/rtf", ""),
    // code and config
    ("rs", Code, "text/plain; charset=utf-8", "rust"),
    ("toml", Code, "text/plain; charset=utf-8", "toml"),
    ("lock", Code, "text/plain; charset=utf-8", "toml"),
    ("json", Code, "application/json", "json"),
    ("jsonl", Code, "text/plain; charset=utf-8", "json"),
    ("ndjson", Code, "text/plain; charset=utf-8", "json"),
    ("json5", Code, "text/plain; charset=utf-8", "json"),
    ("yaml", Code, "text/plain; charset=utf-8", "yaml"),
    ("yml", Code, "text/plain; charset=utf-8", "yaml"),
    ("js", Code, "text/plain; charset=utf-8", "javascript"),
    ("mjs", Code, "text/plain; charset=utf-8", "javascript"),
    ("cjs", Code, "text/plain; charset=utf-8", "javascript"),
    ("jsx", Code, "text/plain; charset=utf-8", "jsx"),
    ("tsx", Code, "text/plain; charset=utf-8", "tsx"),
    ("html", Code, "text/plain; charset=utf-8", "html"),
    ("htm", Code, "text/plain; charset=utf-8", "html"),
    ("css", Code, "text/plain; charset=utf-8", "css"),
    ("scss", Code, "text/plain; charset=utf-8", "css"),
    ("xml", Code, "text/plain; charset=utf-8", "xml"),
    ("plist", Code, "text/plain; charset=utf-8", "xml"),
    ("py", Code, "text/plain; charset=utf-8", "python"),
    ("sh", Code, "text/plain; charset=utf-8", "shell"),
    ("bash", Code, "text/plain; charset=utf-8", "shell"),
    ("zsh", Code, "text/plain; charset=utf-8", "shell"),
    ("fish", Code, "text/plain; charset=utf-8", "shell"),
    ("sql", Code, "text/plain; charset=utf-8", "sql"),
    ("ini", Code, "text/plain; charset=utf-8", "ini"),
    ("cfg", Code, "text/plain; charset=utf-8", "ini"),
    ("conf", Code, "text/plain; charset=utf-8", "ini"),
    ("env", Code, "text/plain; charset=utf-8", "ini"),
    ("properties", Code, "text/plain; charset=utf-8", "ini"),
    ("service", Code, "text/plain; charset=utf-8", "ini"),
    ("timer", Code, "text/plain; charset=utf-8", "ini"),
    ("socket", Code, "text/plain; charset=utf-8", "ini"),
    ("go", Code, "text/plain; charset=utf-8", "go"),
    ("c", Code, "text/plain; charset=utf-8", "c"),
    ("h", Code, "text/plain; charset=utf-8", "c"),
    ("cc", Code, "text/plain; charset=utf-8", "cpp"),
    ("cpp", Code, "text/plain; charset=utf-8", "cpp"),
    ("hpp", Code, "text/plain; charset=utf-8", "cpp"),
    ("java", Code, "text/plain; charset=utf-8", "java"),
    ("kt", Code, "text/plain; charset=utf-8", "kotlin"),
    ("kts", Code, "text/plain; charset=utf-8", "kotlin"),
    ("cs", Code, "text/plain; charset=utf-8", "csharp"),
    ("lua", Code, "text/plain; charset=utf-8", "lua"),
    ("rb", Code, "text/plain; charset=utf-8", "ruby"),
    ("diff", Code, "text/plain; charset=utf-8", "diff"),
    ("patch", Code, "text/plain; charset=utf-8", "diff"),
    ("nginx", Code, "text/plain; charset=utf-8", "nginx"),
    (
        "dockerfile",
        Code,
        "text/plain; charset=utf-8",
        "dockerfile",
    ),
    ("swift", Code, "text/plain; charset=utf-8", ""),
    ("php", Code, "text/plain; charset=utf-8", ""),
    ("pl", Code, "text/plain; charset=utf-8", ""),
    ("vue", Code, "text/plain; charset=utf-8", "html"),
    ("svelte", Code, "text/plain; charset=utf-8", "html"),
    ("proto", Code, "text/plain; charset=utf-8", ""),
    ("graphql", Code, "text/plain; charset=utf-8", ""),
    ("tf", Code, "text/plain; charset=utf-8", ""),
    ("nix", Code, "text/plain; charset=utf-8", ""),
    ("pem", Code, "text/plain; charset=utf-8", ""),
    ("pub", Code, "text/plain; charset=utf-8", ""),
    ("bak", Text, "text/plain; charset=utf-8", ""),
    ("off", Text, "text/plain; charset=utf-8", ""),
    // archives we open
    ("zip", Archive, "application/zip", ""),
    ("rar", Archive, "application/vnd.rar", ""),
    ("cbz", Archive, "application/zip", ""),
    ("cbr", Archive, "application/vnd.rar", ""),
    ("jar", Archive, "application/java-archive", ""),
    (
        "apk",
        Archive,
        "application/vnd.android.package-archive",
        "",
    ),
    ("tar", Archive, "application/x-tar", ""),
    ("tgz", Archive, "application/gzip", ""),
    // archives and packed data we only name
    ("gz", Packed, "application/gzip", ""),
    ("7z", Packed, "application/x-7z-compressed", ""),
    ("xz", Packed, "application/x-xz", ""),
    ("bz2", Packed, "application/x-bzip2", ""),
    ("zst", Packed, "application/zstd", ""),
    ("iso", Packed, "application/x-iso9660-image", ""),
    ("deb", Packed, "application/vnd.debian.binary-package", ""),
    ("rpm", Packed, "application/x-rpm", ""),
    ("dmg", Packed, "application/x-apple-diskimage", ""),
    // fonts
    ("ttf", Font, "font/ttf", ""),
    ("otf", Font, "font/otf", ""),
    ("woff", Font, "font/woff", ""),
    ("woff2", Font, "font/woff2", ""),
    // binaries and data
    ("wasm", Binary, "application/wasm", ""),
    (
        "exe",
        Binary,
        "application/vnd.microsoft.portable-executable",
        "",
    ),
    ("so", Binary, "application/octet-stream", ""),
    ("bin", Binary, "application/octet-stream", ""),
    ("gguf", Binary, "application/octet-stream", ""),
    ("safetensors", Binary, "application/octet-stream", ""),
    ("onnx", Binary, "application/octet-stream", ""),
    ("db", Binary, "application/vnd.sqlite3", ""),
    ("sqlite", Binary, "application/vnd.sqlite3", ""),
    ("sqlite3", Binary, "application/vnd.sqlite3", ""),
];

/// Whole file names (lower-case) that have no telling extension.
pub const NAMES: &[Row] = &[
    (
        "dockerfile",
        Code,
        "text/plain; charset=utf-8",
        "dockerfile",
    ),
    (
        "containerfile",
        Code,
        "text/plain; charset=utf-8",
        "dockerfile",
    ),
    ("makefile", Code, "text/plain; charset=utf-8", "shell"),
    ("justfile", Code, "text/plain; charset=utf-8", "shell"),
    ("cargo.lock", Code, "text/plain; charset=utf-8", "toml"),
    ("license", Text, "text/plain; charset=utf-8", ""),
    ("readme", Text, "text/plain; charset=utf-8", ""),
    ("authors", Text, "text/plain; charset=utf-8", ""),
    ("codeowners", Text, "text/plain; charset=utf-8", ""),
    (".env", Code, "text/plain; charset=utf-8", "ini"),
    (".gitignore", Code, "text/plain; charset=utf-8", "shell"),
    (".gitattributes", Code, "text/plain; charset=utf-8", "shell"),
    (".gitmodules", Code, "text/plain; charset=utf-8", "ini"),
    (".dockerignore", Code, "text/plain; charset=utf-8", "shell"),
    (".editorconfig", Code, "text/plain; charset=utf-8", "ini"),
    (".npmrc", Code, "text/plain; charset=utf-8", "ini"),
    (".bashrc", Code, "text/plain; charset=utf-8", "shell"),
    (".bash_profile", Code, "text/plain; charset=utf-8", "shell"),
    (".profile", Code, "text/plain; charset=utf-8", "shell"),
    (".zshrc", Code, "text/plain; charset=utf-8", "shell"),
    (".vimrc", Code, "text/plain; charset=utf-8", ""),
    (".htaccess", Code, "text/plain; charset=utf-8", "nginx"),
    (".prettierrc", Code, "text/plain; charset=utf-8", "json"),
    (".eslintrc", Code, "text/plain; charset=utf-8", "json"),
];

/// What one name maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeInfo {
    pub kind: Kind,
    pub mime: &'static str,
    pub lang: &'static str,
    /// True when the type came from the content, not the name.
    pub sniffed: bool,
}

const UNKNOWN: TypeInfo = TypeInfo {
    kind: Binary,
    mime: "application/octet-stream",
    lang: "",
    sniffed: false,
};

fn row(r: &Row) -> TypeInfo {
    TypeInfo {
        kind: r.1,
        mime: r.2,
        lang: r.3,
        sniffed: false,
    }
}

/// Extension of a name, lower-case, without the dot. A leading dot alone
/// (`.env`) is not an extension.
pub fn extension(name: &str) -> Option<String> {
    let base = name.trim_start_matches('.');
    let i = base.rfind('.')?;
    let e = &base[i + 1..];
    (!e.is_empty()).then(|| e.to_ascii_lowercase())
}

/// Type from the name only. `None` when the name says nothing.
pub fn by_name(name: &str) -> Option<TypeInfo> {
    let lower = name.to_ascii_lowercase();
    if let Some(r) = NAMES.iter().find(|r| r.0 == lower) {
        return Some(row(r));
    }
    // `.env.local`, `.env.production`
    if lower.starts_with(".env.") {
        return NAMES.iter().find(|r| r.0 == ".env").map(row);
    }
    // `name.tar.gz` is a tar archive we open.
    if lower.ends_with(".tar.gz") {
        return Some(TypeInfo {
            kind: Archive,
            mime: "application/gzip",
            lang: "",
            sniffed: false,
        });
    }
    let e = extension(name)?;
    EXTENSIONS.iter().find(|r| r.0 == e).map(row)
}

/// Content check on the first bytes (up to 8 KiB is plenty).
pub fn sniff(head: &[u8]) -> TypeInfo {
    let s = |kind, mime| TypeInfo {
        kind,
        mime,
        lang: "",
        sniffed: true,
    };
    let starts = |m: &[u8]| head.starts_with(m);
    if starts(b"\x89PNG\r\n\x1a\n") {
        return s(Image, "image/png");
    }
    if starts(b"\xff\xd8\xff") {
        return s(Image, "image/jpeg");
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return s(Image, "image/gif");
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return s(Image, "image/webp");
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return s(Audio, "audio/wav");
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        if brand == b"M4A " {
            return s(Audio, "audio/mp4");
        }
        if brand == b"avif" || brand == b"heic" {
            return s(Image, "image/avif");
        }
        return s(Video, "video/mp4");
    }
    if starts(b"\x1a\x45\xdf\xa3") {
        return s(Video, "video/webm");
    }
    if starts(b"OggS") {
        return s(Audio, "audio/ogg");
    }
    if starts(b"fLaC") {
        return s(Audio, "audio/flac");
    }
    if starts(b"ID3") || (head.len() > 1 && head[0] == 0xff && head[1] & 0xe0 == 0xe0) {
        return s(Audio, "audio/mpeg");
    }
    if starts(b"%PDF-") {
        return s(Pdf, "application/pdf");
    }
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        return s(Archive, "application/zip");
    }
    if starts(b"Rar!\x1a\x07") {
        return s(Archive, "application/vnd.rar");
    }
    if head.len() > 262 && &head[257..262] == b"ustar" {
        return s(Archive, "application/x-tar");
    }
    if starts(b"\x1f\x8b") {
        return s(Packed, "application/gzip");
    }
    if starts(b"7z\xbc\xaf\x27\x1c") {
        return s(Packed, "application/x-7z-compressed");
    }
    if starts(b"\xfd7zXZ\x00") {
        return s(Packed, "application/x-xz");
    }
    if starts(b"\x7fELF") {
        return s(Binary, "application/x-elf");
    }
    if starts(b"SQLite format 3\x00") {
        return s(Binary, "application/vnd.sqlite3");
    }
    if looks_text(head) {
        return s(Text, "text/plain; charset=utf-8");
    }
    UNKNOWN
}

/// UTF-8 (a cut multi-byte char at the end is fine) with no NUL and few
/// control bytes.
pub fn looks_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    let ok = match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && head.len() - e.valid_up_to() < 4,
    };
    if !ok {
        return false;
    }
    let ctrl = head
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t' | 0x0c | 0x1b))
        .count();
    ctrl * 100 <= head.len().max(1)
}

/// Name first, then content. An empty file of unknown type is text (so a
/// new file opens in the editor).
pub fn detect(name: &str, head: &[u8]) -> TypeInfo {
    if let Some(t) = by_name(name) {
        // A name that claims text but holds binary is shown as binary.
        if t.kind.is_text() && !head.is_empty() && !looks_text(head) {
            return UNKNOWN;
        }
        return t;
    }
    if head.is_empty() {
        return TypeInfo {
            kind: Text,
            mime: "text/plain; charset=utf-8",
            lang: "",
            sniffed: true,
        };
    }
    sniff(head)
}

/// The whole table as JSON (for `GET /api/files/kinds` and `files_kinds`).
pub fn table_json() -> Value {
    let rows = |t: &[Row]| -> Vec<Value> {
        t.iter()
            .map(|r| json!({"match": r.0, "kind": r.1.as_str(), "mime": r.2, "lang": r.3, "preview": r.1.preview()}))
            .collect()
    };
    json!({
        "extensions": rows(EXTENSIONS),
        "names": rows(NAMES),
        "sniffed": ["png","jpeg","gif","webp","wav","mp4/mov (ftyp)","webm/mkv","ogg","flac","mp3","pdf","zip","rar","tar","gzip","7z","xz","elf","sqlite","utf-8 text"],
        "previews": {
            "img": "shown inline", "video": "played inline (HTTP range), poster from ffmpeg when set up",
            "audio": "played inline", "pdf": "the browser's PDF viewer", "markdown": "rendered, or edited as source",
            "editor": "CodeMirror 6 editor (read-only unless writes are on)", "archive": "members listed, extract into a new folder",
            "hex": "metadata plus a hex dump, paged", "list": "folder listing"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_extensions() {
        assert_eq!(by_name("a.MP4").unwrap().kind, Video);
        assert_eq!(by_name("x.rs").unwrap().lang, "rust");
        assert_eq!(by_name(".env").unwrap().lang, "ini");
        assert_eq!(by_name(".env.local").unwrap().lang, "ini");
        assert_eq!(by_name("Dockerfile").unwrap().lang, "dockerfile");
        assert_eq!(by_name("backup.tar.gz").unwrap().kind, Archive);
        assert_eq!(by_name("ai video Instagram.rar").unwrap().kind, Archive);
        assert!(by_name("noext").is_none());
        assert_eq!(extension(".bashrc"), None);
        assert_eq!(extension("a.b.TXT").as_deref(), Some("txt"));
    }

    #[test]
    fn sniffing() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n....").kind, Image);
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42").kind, Video);
        assert_eq!(sniff(b"Rar!\x1a\x07\x01\x00").kind, Archive);
        assert_eq!(sniff(b"hello\nworld\n").kind, Text);
        assert_eq!(sniff(&[0u8, 1, 2, 3, 255]).kind, Binary);
        // A text name with binary content is binary.
        assert_eq!(detect("x.txt", &[0, 0, 1]).kind, Binary);
        assert_eq!(detect("new", b"").kind, Text);
    }

    #[test]
    fn table_is_unique_and_lower_case() {
        let mut seen = std::collections::HashSet::new();
        for r in EXTENSIONS {
            assert_eq!(r.0, r.0.to_ascii_lowercase());
            assert!(seen.insert(r.0), "duplicate extension {}", r.0);
        }
        assert!(table_json()["extensions"].as_array().unwrap().len() >= 140);
    }
}
