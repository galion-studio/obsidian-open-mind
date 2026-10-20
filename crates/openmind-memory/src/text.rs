// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Text helpers ported from the gateway (`utils/sanitize.ts`,
//! `core/hooks/auto-recall.ts`, `core/scene/scene-navigation.ts`).
//! The TS code uses regexes; these are hand-written matchers with the same
//! semantics (lazy matches = first closing occurrence), so no regex crate.

/// JS `\s` (close enough: Unicode White_Space plus BOM).
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    while let Some(c) = s[i..].chars().next() {
        if !is_js_space(c) {
            break;
        }
        i += c.len_utf8();
    }
    i
}

/// Remove every non-overlapping match. `m(s, i)` returns the end of a match
/// starting at byte `i`. `line_start` restricts matches to the start of a line.
fn remove_matches(s: &str, line_start: bool, m: impl Fn(&str, usize) -> Option<usize>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let at_line_start = i == 0 || s.as_bytes()[i - 1] == b'\n';
        if !line_start || at_line_start {
            if let Some(end) = m(s, i) {
                if end > i {
                    i = end;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap_or('\0');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `start[\s\S]*?end` (plus optional trailing `\s*`).
fn block(start: &str, end: &str, trailing_ws: bool) -> impl Fn(&str, usize) -> Option<usize> {
    let (start, end) = (start.to_string(), end.to_string());
    move |s: &str, i: usize| {
        if !s[i..].starts_with(start.as_str()) {
            return None;
        }
        let from = i + start.len();
        let e = s[from..].find(end.as_str())? + from + end.len();
        Some(if trailing_ws { skip_ws(s, e) } else { e })
    }
}

fn data_image(s: &str, i: usize) -> Option<usize> {
    // /data:image\/[a-z+]+;base64,[A-Za-z0-9+/=]+/gi
    let rest = &s[i..];
    if rest.len() < 11
        || !rest.is_char_boundary(11)
        || !rest[..11].eq_ignore_ascii_case("data:image/")
    {
        return None;
    }
    let mut j = i + 11;
    let b = s.as_bytes();
    let st = j;
    while j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'+') {
        j += 1;
    }
    if j == st {
        return None;
    }
    let tag = ";base64,";
    if s.len() < j + tag.len()
        || !s.is_char_boundary(j + tag.len())
        || !s[j..j + tag.len()].eq_ignore_ascii_case(tag)
    {
        return None;
    }
    j += tag.len();
    let st = j;
    while j < b.len()
        && (b[j].is_ascii_alphanumeric() || b[j] == b'+' || b[j] == b'/' || b[j] == b'=')
    {
        j += 1;
    }
    if j == st {
        None
    } else {
        Some(j)
    }
}

const UNTRUSTED_LABELS: [&str; 6] = [
    "Conversation info",
    "Sender",
    "Thread starter",
    "Replied message",
    "Forwarded message context",
    "Chat history since last reply",
];

fn untrusted_json(s: &str, i: usize) -> Option<usize> {
    let label = UNTRUSTED_LABELS.iter().find(|l| s[i..].starts_with(**l))?;
    let j = skip_ws(s, i + label.len());
    if !s[j..].starts_with("(untrusted") {
        return None;
    }
    // [\s\S]*?\):\s*```json
    let mut k = j + "(untrusted".len();
    loop {
        let p = s[k..].find("):")? + k;
        let q = skip_ws(s, p + 2);
        if s[q..].starts_with("```json") {
            // \s*[\s\S]*?```
            let r = q + 7;
            let e = s[r..].find("```")? + r + 3;
            return Some(e);
        }
        k = p + 1;
    }
}

fn session_json(s: &str, i: usize) -> Option<usize> {
    // ```json\s*\{[\s\S]*?"session[\s\S]*?\}\s*```
    if !s[i..].starts_with("```json") {
        return None;
    }
    let j = skip_ws(s, i + 7);
    if !s[j..].starts_with('{') {
        return None;
    }
    let p = s[j + 1..].find("\"session")? + j + 1 + 8;
    let mut k = p;
    loop {
        let b = s[k..].find('}')? + k;
        let q = skip_ws(s, b + 1);
        if s[q..].starts_with("```") {
            return Some(q + 3);
        }
        k = b + 1;
    }
}

fn reply_to(s: &str, i: usize) -> Option<usize> {
    // \[\[reply_to[^\]]*\]\]\s*
    if !s[i..].starts_with("[[reply_to") {
        return None;
    }
    let j = i + 10;
    let close = s[j..].find(']')? + j;
    if !s[close..].starts_with("]]") {
        return None;
    }
    Some(skip_ws(s, close + 2))
}

fn bracket_prefix(s: &str, i: usize) -> Option<usize> {
    // ^\[[\w\d\-:+ ]+\]\s*   (JS \w = [A-Za-z0-9_])
    let b = s.as_bytes();
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while j < b.len()
        && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'-' | b':' | b'+' | b' '))
    {
        j += 1;
    }
    if j == i + 1 || b.get(j) != Some(&b']') {
        return None;
    }
    Some(skip_ws(s, j + 1))
}

fn media_attached(s: &str, i: usize) -> Option<usize> {
    if !s[i..].starts_with("[media attached:") {
        return None;
    }
    let j = i + 16;
    let e = s[j..].find(']')? + j + 1;
    Some(skip_ws(s, e))
}

fn system_line(s: &str, i: usize) -> Option<usize> {
    // ^System:\s*\[[\s\S]*?$  (multiline: lazy up to the first line end)
    if !s[i..].starts_with("System:") {
        return None;
    }
    let j = skip_ws(s, i + 7);
    if !s[j..].starts_with('[') {
        return None;
    }
    Some(s[j..].find('\n').map(|p| p + j).unwrap_or(s.len()))
}

/// Collapse 3+ consecutive newlines to exactly two.
pub fn collapse_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0;
    for c in s.chars() {
        if c == '\n' {
            run += 1;
            if run <= 2 {
                out.push(c);
            }
        } else {
            run = 0;
            out.push(c);
        }
    }
    out
}

/// Port of `sanitizeText` (strip injected context/metadata from a message).
pub fn sanitize_text(text: &str) -> String {
    let mut t = text.to_string();
    for tag in [
        "relevant-memories",
        "user-persona",
        "relevant-scenes",
        "scene-navigation",
        "current_task_context",
    ] {
        t = remove_matches(
            &t,
            false,
            block(&format!("<{tag}>"), &format!("</{tag}>"), false),
        );
    }
    t = remove_matches(
        &t,
        false,
        block("<history_task_context", "</history_task_context>", false),
    );
    t = remove_matches(&t, false, untrusted_json);
    t = remove_matches(&t, false, session_json);
    t = remove_matches(&t, false, reply_to);
    t = remove_matches(&t, false, block("¥¥[", "]¥¥", false));
    t = remove_matches(&t, true, bracket_prefix);
    t = remove_matches(&t, false, media_attached);
    t = remove_matches(
        &t,
        false,
        block(
            "To send an image back,",
            "Keep caption in the text body.",
            true,
        ),
    );
    t = remove_matches(&t, true, system_line);
    t = remove_matches(&t, false, data_image);
    t = t.replace('\0', "");
    collapse_newlines(&t).trim().to_string()
}

/// Replace inline base64 images with `[image]` (L0 extraction step).
pub fn replace_data_images(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some(e) = data_image(text, i) {
            out.push_str("[image]");
            i = e;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or('\0');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Port of `stripCodeBlocks`: drop fenced blocks from assistant replies.
pub fn strip_code_blocks(text: &str) -> String {
    let t = remove_matches(text, false, |s, i| {
        if !s[i..].starts_with("```") {
            return None;
        }
        let nl = s[i + 3..].find('\n')? + i + 3;
        let e = s[nl + 1..].find("```")? + nl + 1 + 3;
        Some(e)
    });
    collapse_newlines(&t).trim().to_string()
}

fn is_framework_noise(text: &str) -> bool {
    let t = text.trim();
    if t == "(session bootstrap)"
        || t.starts_with("A new session was started via")
        || t.starts_with("Pre-compaction memory flush")
    {
        return true;
    }
    if let Some(rest) = t.strip_prefix('✅') {
        if rest.trim_start().starts_with("New session started") {
            return true;
        }
    }
    if let Some(rest) = t.strip_prefix("NO_REPLY") {
        if rest.chars().all(is_js_space) {
            return true;
        }
    }
    false
}

/// Port of `shouldCaptureL0`.
pub fn should_capture_l0(text: &str) -> bool {
    !text.trim().is_empty() && !is_framework_noise(text) && !text.starts_with('/')
}

fn is_fact_block_start(line: &str) -> bool {
    // /^- id:\s*\S+/
    match line.strip_prefix("- id:") {
        Some(rest) => rest
            .trim_start_matches(is_js_space)
            .chars()
            .next()
            .is_some(),
        None => false,
    }
}

fn session_tag(line: &str) -> Option<&str> {
    // /^\s*session:\s*(\S+)\s*$/
    let t = line.trim_start_matches(is_js_space);
    let rest = t.strip_prefix("session:")?;
    let v = rest.trim_matches(is_js_space);
    if v.is_empty() || v.chars().any(is_js_space) {
        None
    } else {
        Some(v)
    }
}

/// Port of `filterPersonaBlocksBySession`.
pub fn filter_persona_blocks_by_session(text: &str, session_id: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let current = session_id.trim();
    let mut preamble: Vec<&str> = Vec::new();
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.split('\n') {
        if is_fact_block_start(line) {
            blocks.push(vec![line]);
        } else if let Some(b) = blocks.last_mut() {
            b.push(line);
        } else {
            preamble.push(line);
        }
    }
    if blocks.is_empty() {
        return text.to_string();
    }
    let kept: Vec<String> = blocks
        .iter()
        .filter(|b| {
            b.iter()
                .find_map(|l| session_tag(l))
                .map(|s| s == current)
                .unwrap_or(false)
        })
        .map(|b| b.join("\n").trim_end().to_string())
        .filter(|b| !b.is_empty())
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let head = preamble.join("\n").trim_end().to_string();
    let body = kept.join("\n\n");
    if head.is_empty() {
        return body;
    }
    format!("{head}\n\n{body}")
}

/// Port of `filterRecallTextBySession`.
pub fn filter_recall_text_by_session(text: &str, session_id: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    // MEMORY_TOOLS_GUIDE_BLOCK: [ \t]*<memory-tools-guide>[\s\S]*?</memory-tools-guide>[ \t]* -> "\n"
    let mut without_guide = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("<memory-tools-guide>") {
        let Some(q) = rest[p..].find("</memory-tools-guide>") else {
            break;
        };
        let end = p + q + "</memory-tools-guide>".len();
        let before = rest[..p].trim_end_matches([' ', '\t']);
        without_guide.push_str(before);
        without_guide.push('\n');
        rest = rest[end..].trim_start_matches([' ', '\t']);
    }
    without_guide.push_str(rest);

    let mut matched = false;
    let mut out = String::new();
    let mut rest = without_guide.as_str();
    while let Some(p) = rest.find("<user-persona>") {
        let inner_start = p + "<user-persona>".len();
        let Some(q) = rest[inner_start..].find("</user-persona>") else {
            break;
        };
        matched = true;
        let inner = &rest[inner_start..inner_start + q];
        let inner = inner.strip_prefix('\n').unwrap_or(inner);
        let inner = inner.strip_suffix('\n').unwrap_or(inner);
        out.push_str(&rest[..p]);
        let body = filter_persona_blocks_by_session(inner, session_id);
        if !body.trim().is_empty() {
            out.push_str(&format!("<user-persona>\n{}\n</user-persona>", body.trim()));
        }
        rest = &rest[inner_start + q + "</user-persona>".len()..];
    }
    out.push_str(rest);
    let scoped = if matched {
        out
    } else {
        filter_persona_blocks_by_session(&without_guide, session_id)
    };
    collapse_newlines(&scoped).trim().to_string()
}

pub const NAV_HEADER: &str = "---\n## 🗺️ Scene Navigation (Scene Index)";
const NAV_FOOTER_LOCAL: &str = "📌 \u{4F7F}\u{7528}\u{8BF4}\u{660E}：\n- Path \u{662F} scene block \u{7684}\u{7EDD}\u{5BF9}\u{8DEF}\u{5F84}，\u{53EF}\u{76F4}\u{63A5}\u{4F7F}\u{7528} **read** \u{5DE5}\u{5177}\u{8BFB}\u{53D6}\u{5B8C}\u{6574}\u{5185}\u{5BB9}（\u{53C2}\u{6570}: filePath）\n- \u{70ED}\u{5EA6}：\u{8BE5}\u{573A}\u{666F}\u{88AB}\u{8BB0}\u{5FC6}\u{547D}\u{4E2D}\u{7684}\u{7D2F}\u{8BA1}\u{6B21}\u{6570}，\u{8D8A}\u{9AD8}\u{8D8A}\u{91CD}\u{8981}\n- Summary：\u{573A}\u{666F}\u{7684}\u{6838}\u{5FC3}\u{8981}\u{70B9}\u{6458}\u{8981}";

/// Port of `stripSceneNavigation`.
pub fn strip_scene_navigation(persona: &str) -> &str {
    match persona.find(NAV_HEADER) {
        Some(i) => persona[..i].trim_end(),
        None => persona,
    }
}

/// Filesystem-safe id: chars outside `[A-Za-z0-9._-]` become `_`.
pub fn sanitize_session_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Port of `sceneFilenameForSession`.
pub fn scene_filename_for_session(session_id: &str) -> String {
    let mut stem = sanitize_session_id(session_id.trim());
    if stem.is_empty() {
        stem = "_".into();
    }
    while let Some(r) = stem.strip_prefix("scene-") {
        stem = r.to_string();
    }
    if stem.is_empty() {
        stem = "_".into();
    }
    format!("scene-{stem}.md")
}

#[derive(Debug, Clone)]
pub struct SceneEntry {
    pub filename: String,
    pub summary: String,
    pub heat: f64,
    pub updated: String,
}

/// JS `Number#toString` for the values we print (integers and simple floats).
pub fn js_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn heat_emoji(h: f64) -> &'static str {
    if h >= 1000.0 {
        " 🔥🔥🔥🔥🔥"
    } else if h >= 500.0 {
        " 🔥🔥🔥🔥"
    } else if h >= 200.0 {
        " 🔥🔥🔥"
    } else if h >= 100.0 {
        " 🔥🔥"
    } else if h >= 50.0 {
        " 🔥"
    } else {
        ""
    }
}

/// Port of `generateSceneNavigation` (local storage variant).
pub fn generate_scene_navigation(entries: &[SceneEntry], data_dir: &str) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| {
        b.heat
            .partial_cmp(&a.heat)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let blocks: Vec<String> = sorted
        .iter()
        .map(|e| {
            let path = std::path::Path::new(data_dir)
                .join("scene_blocks")
                .join(&e.filename);
            let upd = if e.updated.is_empty() {
                String::new()
            } else {
                format!(" | **\u{66F4}\u{65B0}**: {}", e.updated)
            };
            format!(
                "### Path: {}\n**\u{70ED}\u{5EA6}**: {}{}{}\nSummary: {}",
                path.display(),
                js_num(e.heat),
                heat_emoji(e.heat),
                upd,
                e.summary
            )
        })
        .collect();
    format!(
        "{NAV_HEADER}\n*\u{4EE5}\u{4E0B}\u{662F}\u{5F53}\u{524D}\u{573A}\u{666F}\u{8BB0}\u{5FC6}\u{7684}\u{7D22}\u{5F15}，\u{53EF}\u{6839}\u{636E}\u{9700}\u{8981} read \u{8BFB}\u{53D6}\u{8BE6}\u{7EC6}\u{5185}\u{5BB9}\u{3002}*\n\n{}\n\n{NAV_FOOTER_LOCAL}",
        blocks.join("\n\n")
    )
}

/// Port of `formatTimestamp`: `YYYY-MM-DD` or `YYYY-MM-DD HH:MM`.
pub fn format_timestamp(ts: &str) -> Option<String> {
    let b = ts.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        r.clone()
            .all(|i| b.get(i).is_some_and(|c| c.is_ascii_digit()))
    };
    if !(digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10))
    {
        return None;
    }
    let date = &ts[..10];
    if b.get(10) == Some(&b'T') && digits(11..13) && b.get(13) == Some(&b':') && digits(14..16) {
        let time = &ts[11..16];
        if time != "00:00" {
            return Some(format!("{date} {time}"));
        }
    }
    Some(date.to_string())
}

/// Port of `formatMemoryLine`.
pub fn format_memory_line(
    kind: &str,
    content: &str,
    scene: &str,
    start: Option<&str>,
    end: Option<&str>,
    point: Option<&str>,
) -> String {
    let tag = if scene.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}|{scene}")
    };
    let mut line = format!("- [{tag}] {content}");
    let f = |o: Option<&str>| o.filter(|s| !s.is_empty()).and_then(format_timestamp);
    match (f(start), f(end), f(point)) {
        (Some(s), Some(e), _) => {
            line.push_str(&format!(" (\u{6D3B}\u{52A8}\u{65F6}\u{95F4}: {s} ~ {e})"))
        }
        (Some(s), None, _) => {
            line.push_str(&format!(" (\u{6D3B}\u{52A8}\u{65F6}\u{95F4}: {s}\u{8D77})"))
        }
        (None, Some(e), _) => {
            line.push_str(&format!(" (\u{6D3B}\u{52A8}\u{65F6}\u{95F4}: \u{81F3}{e})"))
        }
        (None, None, Some(p)) => {
            line.push_str(&format!(" (\u{6D3B}\u{52A8}\u{65F6}\u{95F4}: {p})"))
        }
        _ => {}
    }
    line
}

/// JS `String#slice(0, n)` on UTF-16 units, rounded down to a char boundary.
pub fn js_slice_utf16(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// JS `String#length` (UTF-16 units).
pub fn js_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_injected_blocks() {
        let t = "<relevant-memories>x</relevant-memories>hello [[reply_to:1]] world\n\n\n\nSystem: [cron] ping\n[2026-10-18 20:00] hi data:image/png;base64,AAAA";
        assert_eq!(sanitize_text(t), "hello world\n\nhi");
    }

    #[test]
    fn sanitize_untrusted_json() {
        let t = "Sender (untrusted metadata):\n```json\n{\"a\":1}\n```\nreal text";
        assert_eq!(sanitize_text(t), "real text");
    }

    #[test]
    fn code_blocks() {
        assert_eq!(strip_code_blocks("a\n```rs\nfn x(){}\n```\nb"), "a\n\nb");
    }

    #[test]
    fn persona_session_filter() {
        let t =
            "# Persona\n- id: a1\n  fact one\n  session: s1\n- id: a2\n  fact two\n  session: s2";
        assert_eq!(
            filter_persona_blocks_by_session(t, "s1"),
            "# Persona\n\n- id: a1\n  fact one\n  session: s1"
        );
        assert_eq!(filter_persona_blocks_by_session(t, "zz"), "");
        assert_eq!(filter_persona_blocks_by_session("plain", "zz"), "plain");
    }

    #[test]
    fn recall_text_filter() {
        let t = "<memory-tools-guide>g</memory-tools-guide>\n<user-persona>\n- id: x\n  session: s9\n</user-persona>\n\n\n\nend";
        assert_eq!(filter_recall_text_by_session(t, "s1"), "end");
        assert_eq!(
            filter_recall_text_by_session(t, "s9"),
            "<user-persona>\n- id: x\n  session: s9\n</user-persona>\n\nend"
        );
    }

    #[test]
    fn scene_names() {
        assert_eq!(
            scene_filename_for_session("scene-scene-a:b"),
            "scene-a_b.md"
        );
        assert_eq!(scene_filename_for_session(""), "scene-_.md");
    }

    #[test]
    fn memory_line_and_time() {
        assert_eq!(
            format_timestamp("2025-03-01T14:30:00.000Z").as_deref(), // repo-check: allow (legacy stored UTC row under test; output is the formatted label)
            Some("2025-03-01 14:30")
        );
        assert_eq!(
            format_timestamp("2025-03-01T00:00:00Z").as_deref(), // repo-check: allow (legacy stored UTC row under test; output is the formatted label)
            Some("2025-03-01")
        );
        assert_eq!(
            format_memory_line(
                "episodic",
                "trip",
                "travel",
                Some("2025-05-01"),
                Some("2025-05-10"),
                None
            ),
            "- [episodic|travel] trip (\u{6D3B}\u{52A8}\u{65F6}\u{95F4}: 2025-05-01 ~ 2025-05-10)"
        );
        assert_eq!(
            format_memory_line("persona", "x", "", None, None, None),
            "- [persona] x"
        );
    }

    #[test]
    fn utf16_slice() {
        assert_eq!(js_slice_utf16("ab😀c", 3), "ab");
        assert_eq!(js_len("ab😀c"), 5);
    }

    #[test]
    fn capture_filter() {
        assert!(!should_capture_l0("/new"));
        assert!(!should_capture_l0("NO_REPLY  "));
        assert!(should_capture_l0("hello"));
    }
}
