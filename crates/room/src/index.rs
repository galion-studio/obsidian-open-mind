// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Builds the object list ("the page"): fixed live services, held hosts, and
//! the docs/papers/libraries found under the configured roots.
//!
//! Everything here is read-only. Paths whose name hints at a secret are never
//! listed, and symbolic links are never followed while walking.

use crate::config::{Config, MANIFESTS, WALK_ROOTS};
use std::fs;
use std::path::Path;

/// Most files a single walk lists (shared by all walk roots of one page).
pub const BUDGET: usize = 1800;
/// Deepest directory level a walk descends to.
pub const MAX_DEPTH: usize = 32;
/// Longest note, in characters, taken from a file's first line.
pub const NOTE_CHARS: usize = 160;

/// Directory names never walked into.
const SKIP_DIRS: [&str; 6] = [
    "node_modules",
    "target",
    "dist",
    "__pycache__",
    "venv",
    ".next",
];

/// One row of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obj {
    /// `live`, `held`, `paper`, `library`, `doc`, `api` or `light`.
    pub kind: &'static str,
    /// Display name.
    pub name: String,
    /// Full path (or host/service name for `live` and `held`).
    pub path: String,
    /// One-line note: the file's first meaningful line, or a fixed text.
    pub note: String,
}

/// Fixed rows for the services room knows about.
pub const LIVE: [(&str, &str); 7] = [
    ("one", "The mouth. Rust, 8091."),
    ("voice", "Chatterbox. Warm on this box."),
    ("whisper", "The ear."),
    ("hermes", "The agent."),
    ("vision", "The eyes."),
    ("console", "The desk."),
    ("room", "This browser. Rust, 4020."),
];

/// Host names that are named on the page but never fetched.
pub const HELD: [&str; 12] = [
    "galion.app",
    "studio.galion.app",
    "developer.galion.app",
    "api.galion.app",
    "docs.galion.app",
    "reference.galion.app",
    "voice.galion.app",
    "knowledge.galion.app",
    "galion.studio",
    "machine.galion.app",
    "merlinelement.com",
    "sentialink.com",
];

/// True when a path's text hints at a credential. Such paths are never listed
/// and never served, whatever their extension.
#[must_use]
pub fn is_held_path(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    low.contains("secret")
        || low.contains(".env")
        || low.contains("token")
        || low.contains("id_ed25519")
}

/// The full page for a config: live rows, held hosts, walked docs, manifests.
#[must_use]
pub fn index(cfg: &Config) -> Vec<Obj> {
    let mut out: Vec<Obj> = LIVE
        .iter()
        .map(|(name, note)| Obj {
            kind: "live",
            name: (*name).to_string(),
            path: (*name).to_string(),
            note: (*note).to_string(),
        })
        .collect();
    out.extend(HELD.iter().map(|host| Obj {
        kind: "held",
        name: (*host).to_string(),
        path: (*host).to_string(),
        note: "Named. The heavy face is not loaded.".to_string(),
    }));
    let mut budget = BUDGET;
    for root in WALK_ROOTS {
        walk(&cfg.root.join(root), &mut out, &mut budget);
    }
    for path in MANIFESTS {
        if let Some(obj) = classify(&cfg.root.join(path)) {
            out.push(obj);
        }
    }
    out
}

/// The page for one file or directory (`--fixture PATH`).
#[must_use]
pub fn index_at(path: &Path) -> Vec<Obj> {
    let mut out = Vec::new();
    let mut budget = BUDGET;
    if path.is_dir() {
        walk(path, &mut out, &mut budget);
    } else if let Some(obj) = classify(path) {
        out.push(obj);
    }
    out
}

/// Walks `dir` recursively in name order, pushing every classified file and
/// spending one unit of `budget` per pushed row. Hidden entries, build/vendor
/// directories and symbolic links are skipped.
pub fn walk(dir: &Path, out: &mut Vec<Obj>, budget: &mut usize) {
    walk_depth(dir, out, budget, 0);
}

fn walk_depth(dir: &Path, out: &mut Vec<Obj>, budget: &mut usize, depth: usize) {
    if *budget == 0 || depth > MAX_DEPTH {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for ent in entries {
        if *budget == 0 {
            return;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        // DirEntry::file_type does not follow symlinks.
        let Ok(kind) = ent.file_type() else { continue };
        let path = ent.path();
        if kind.is_dir() {
            walk_depth(&path, out, budget, depth + 1);
        } else if kind.is_file() {
            if let Some(obj) = classify(&path) {
                out.push(obj);
                *budget -= 1;
            }
        }
    }
}

/// Classifies one file into a page row, or `None` when it is not shown.
#[must_use]
pub fn classify(path: &Path) -> Option<Obj> {
    let text = path.to_string_lossy().to_string();
    if is_held_path(&text) {
        return None;
    }
    if !path.is_file() {
        return None;
    }
    let low = text.to_ascii_lowercase();
    let name = path.file_name()?.to_string_lossy().to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = name.to_ascii_lowercase();
    let manifest = name == "package.json" || name == "Cargo.toml";
    let kind =
        if ext == "pdf" || (ext == "md" && (low.contains("/papers/") || stem.contains("paper"))) {
            "paper"
        } else if manifest {
            "library"
        } else if ext == "md" || ext == "txt" {
            "doc"
        } else if low.contains("/api/")
            && matches!(ext.as_str(), "ts" | "tsx" | "py" | "rs" | "yml" | "yaml")
        {
            "api"
        } else if matches!(ext.as_str(), "ts" | "tsx" | "js" | "jsx")
            && ["Orb", "Scene", "Shader", "Particle", "Canvas"]
                .iter()
                .any(|k| name.contains(k))
        {
            "light"
        } else {
            return None;
        };
    let show = if manifest {
        lib_name(path).unwrap_or_else(|| name.clone())
    } else {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(name)
    };
    Some(Obj {
        kind,
        name: show,
        path: text,
        note: first_line(path),
    })
}

/// The `name` value of a `package.json` (`"name": "x"`) or a `Cargo.toml`
/// (a line `name = "x"`), found textually. `None` when absent or empty.
pub fn lib_name(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    if path.file_name().is_some_and(|n| n == "Cargo.toml") {
        text.lines()
            .map(str::trim_start)
            .filter(|line| line.starts_with("name"))
            .find_map(|line| quoted_after(line, "name", '='))
    } else {
        quoted_after(&text, "\"name\"", ':')
    }
}

/// First non-empty `"value"` that follows `key`, optional spaces, `sep`.
fn quoted_after(text: &str, key: &str, sep: char) -> Option<String> {
    for (pos, _) in text.match_indices(key) {
        let rest = text[pos + key.len()..].trim_start();
        let Some(rest) = rest.strip_prefix(sep) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        let end = rest.find('"')?;
        let value = rest[..end].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// The first meaningful line of a text file (markdown `#` stripped, front
/// matter and `import` lines skipped), cut to [`NOTE_CHARS`] characters.
/// Unreadable or non-UTF-8 files give an empty note.
#[must_use]
pub fn first_line(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    first_line_of(&text)
}

/// [`first_line`] on text already in memory.
#[must_use]
pub fn first_line_of(text: &str) -> String {
    for line in text.lines() {
        let line = line.trim().trim_start_matches('#').trim();
        if line.is_empty() || line.starts_with("---") || line.starts_with("import ") {
            continue;
        }
        return line.chars().take(NOTE_CHARS).collect();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let dir =
                std::env::temp_dir().join(format!("room-idx-{tag}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).expect("scratch");
            Scratch(dir)
        }
        fn put(&self, rel: &str, body: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(&p, body).expect("write");
            p
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn classify_kinds() {
        let s = Scratch::new("kinds");
        let cases = [
            ("a/note.md", "# Note title\nbody", Some("doc")),
            ("a/readme.txt", "plain", Some("doc")),
            ("papers/x.md", "# A paper", Some("paper")),
            ("a/my-paper.md", "# P", Some("paper")),
            ("a/scan.pdf", "", Some("paper")),
            ("pkg/package.json", "{\"name\": \"pkg-a\"}", Some("library")),
            (
                "crate/Cargo.toml",
                "[package]\nname = \"crate-b\"",
                Some("library"),
            ),
            ("src/api/route.ts", "export {}", Some("api")),
            ("src/api/conf.yaml", "a: 1", Some("api")),
            ("src/OrbView.tsx", "import x\nconst a = 1", Some("light")),
            ("src/ParticleField.js", "x", Some("light")),
            ("src/plain.ts", "x", None),
            ("src/main.rs", "fn main(){}", None),
            ("a/image.png", "", None),
        ];
        for (rel, body, want) in cases {
            let p = s.put(rel, body);
            assert_eq!(classify(&p).map(|o| o.kind), want, "{rel}");
        }
        let pkg = classify(&s.0.join("pkg/package.json")).unwrap();
        assert_eq!(pkg.name, "pkg-a");
        let krate = classify(&s.0.join("crate/Cargo.toml")).unwrap();
        assert_eq!(krate.name, "crate-b");
        let note = classify(&s.0.join("a/note.md")).unwrap();
        assert_eq!(note.name, "note");
        assert_eq!(note.note, "Note title");
        let orb = classify(&s.0.join("src/OrbView.tsx")).unwrap();
        assert_eq!(orb.note, "const a = 1");
    }

    #[test]
    fn secret_looking_paths_are_held() {
        let s = Scratch::new("held");
        for rel in [
            "secrets/a.md",
            "a/SECRET-plan.md",
            "a/.env.md",
            "a/token-notes.md",
            "a/id_ed25519.txt",
        ] {
            let p = s.put(rel, "# hidden");
            assert!(classify(&p).is_none(), "{rel}");
            assert!(is_held_path(&p.to_string_lossy()), "{rel}");
        }
        assert!(!is_held_path("/workspace/galion-src/docs/a.md"));
    }

    #[test]
    fn walk_skips_hidden_vendor_and_respects_budget() {
        let s = Scratch::new("walk");
        s.put("b.md", "# B");
        s.put("a.md", "# A");
        s.put(".hidden/x.md", "# no");
        s.put("node_modules/x.md", "# no");
        s.put("target/x.md", "# no");
        s.put("dist/x.md", "# no");
        s.put("deep/er/c.md", "# C");
        let rows = index_at(&s.0);
        let names: Vec<_> = rows.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);

        let mut out = Vec::new();
        let mut budget = 2;
        walk(&s.0, &mut out, &mut budget);
        assert_eq!(out.len(), 2);
        assert_eq!(budget, 0);
    }

    #[test]
    fn walk_does_not_follow_symlinks() {
        let s = Scratch::new("link");
        let outside = Scratch::new("outside");
        outside.put("leak.md", "# outside text");
        s.put("in.md", "# inside");
        std::os::unix::fs::symlink(&outside.0, s.0.join("dirlink")).expect("symlink");
        std::os::unix::fs::symlink(outside.0.join("leak.md"), s.0.join("filelink.md"))
            .expect("symlink");
        // A loop must not recurse forever.
        std::os::unix::fs::symlink(&s.0, s.0.join("loop")).expect("symlink");
        let rows = index_at(&s.0);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].name, "in");
    }

    #[test]
    fn walk_stops_at_max_depth() {
        let s = Scratch::new("depth");
        let mut rel = String::new();
        for _ in 0..(MAX_DEPTH + 3) {
            rel.push_str("d/");
        }
        s.put(&format!("{rel}deep.md"), "# deep");
        s.put("top.md", "# top");
        let rows = index_at(&s.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "top");
    }

    #[test]
    fn index_at_single_file_and_missing() {
        let s = Scratch::new("single");
        let p = s.put("one.md", "# One");
        assert_eq!(index_at(&p).len(), 1);
        assert!(index_at(&s.0.join("missing")).is_empty());
    }

    #[test]
    fn index_has_fixed_rows_and_reads_under_root() {
        let s = Scratch::new("root");
        s.put("galion-light/docs/guide.md", "# Guide");
        s.put(
            "galion-src/galion-app/package.json",
            "{\"name\": \"galion-app\"}",
        );
        s.put("galion-src/galion-app/src/api/x.ts", "x");
        let rows = index(&Config::with_root(&s.0));
        assert_eq!(rows.len(), LIVE.len() + HELD.len() + 3, "{rows:?}");
        assert_eq!(rows[6].name, "room");
        assert_eq!(rows[7].kind, "held");
        assert!(rows.iter().any(|o| o.kind == "doc" && o.note == "Guide"));
        assert!(rows
            .iter()
            .any(|o| o.kind == "library" && o.name == "galion-app"));
        assert!(rows.iter().any(|o| o.kind == "api"));
        let empty = index(&Config::with_root(s.0.join("nothing")));
        assert_eq!(empty.len(), LIVE.len() + HELD.len());
    }

    #[test]
    fn first_line_rules() {
        assert_eq!(first_line_of("---\ntitle: x\n---\n# Head"), "title: x");
        assert_eq!(first_line_of("\n\nimport a\n## Real"), "Real");
        assert_eq!(first_line_of(""), "");
        let long = "x".repeat(500);
        assert_eq!(first_line_of(&long).chars().count(), NOTE_CHARS);
        let wide = "\u{17e}".repeat(300);
        assert_eq!(first_line_of(&wide).chars().count(), NOTE_CHARS);
        assert_eq!(first_line(Path::new("/nonexistent/room/x.md")), "");
    }

    #[test]
    fn lib_name_edge_cases() {
        let s = Scratch::new("lib");
        let none = s.put("a/package.json", "{}");
        assert_eq!(lib_name(&none), None);
        assert_eq!(classify(&none).unwrap().name, "package.json");
        let empty = s.put("b/package.json", "{\"name\": \"\"}");
        assert_eq!(lib_name(&empty), None);
        let toml = s.put(
            "c/Cargo.toml",
            "[package]\nname=\"krate\"\n[dependencies]\nx = { package = \"y\" }",
        );
        assert_eq!(lib_name(&toml).as_deref(), Some("krate"));
        let later = s.put("d/package.json", "{\"names\": 1, \"name\" : \"real\"}");
        assert_eq!(lib_name(&later).as_deref(), Some("real"));
        let toml_none = s.put("e/Cargo.toml", "[workspace]\nmembers = []");
        assert_eq!(lib_name(&toml_none), None);
        assert!(classify(&s.0.join("missing/package.json")).is_none());
        assert!(classify(&s.0.join("missing.md")).is_none());
    }
}
