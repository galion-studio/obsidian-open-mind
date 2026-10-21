// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Host facts without shelling out where Linux already offers them.
//
// * Process list: `/proc/<pid>/cmdline` instead of `ps -eo args`. Same text
//   to match against (argv joined by spaces; nginx rewrites its argv to
//   `nginx: master process ...`, which is what `ps` showed too). No procps
//   needed, works in the musl static build on a bare box.
// * GPU: `nvidia-smi` stays a subprocess (NVML is a C library; there is no
//   permissive pure-Rust NVIDIA query without FFI to it). It is optional:
//   `ONE_NVIDIA_SMI=off` (or `0`, `none`, `false`) never runs it, any other
//   value is the binary to run, unset means `nvidia-smi` from PATH. A missing
//   binary, a non-zero exit, unparsable output or a run longer than
//   `GPU_TIMEOUT` all give `None` (no GPU), never a panic and never a hang.
// * PDF text: the built-in pure-Rust reader (pdf_text.rs, inflate.rs, pdf_crypt.rs, std
//   only, no crates, no SIMD/asm) replaced the `pdftotext` subprocess
//   (swarm/one-no-shellout, 2026-10-21). `ONE_PDF_TEXT` picks the engine
//   (`rust` default, `rust+pdftotext`, `pdftotext`, `off`); `ONE_PDFTOTEXT`
//   still names the poppler binary for the two modes that use it, which keep
//   the timeout and the private temp file. lopdf 0.45 / pdf-extract 0.12
//   were rejected on 2026-10-20 (RustCrypto deps that pick AES-NI/SHA-NI at
//   run time, ~20 crates); see docs/third-party/one-no-shellout.md.
// * `curl`: the HTTPS edge probes (main.rs `load_edge`) and the default
//   `https://` OpenRouter model list keep it. Pure-Rust TLS works (rustls +
//   rustls-rustcrypto, spike in docs/one-no-shellout.md) but needs ~60 crates,
//   an alpha provider and force-soft cfgs to avoid AES-NI/SHA-NI/AVX; owner
//   decision. A loopback `http://` OpenRouter base (local proxy or stand-in)
//   is fetched in-process by http_plain.rs, no curl. curl absent = code 0.

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Command lines of every process we can read, argv joined by single spaces.
/// Kernel threads (empty cmdline) and processes that vanish mid-scan are
/// skipped. `root` is `/proc` outside tests.
pub fn cmdlines(root: &str) -> Vec<String> {
    let Ok(dir) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(raw) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let line = raw
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

/// The `ps -eo args`-like text the unit checks match against.
pub fn process_text() -> String {
    cmdlines("/proc").join("\n")
}

pub const GPU_TIMEOUT: Duration = Duration::from_secs(3);

/// Which helper binary to run, or `None` when switched off. Unset or blank
/// means `default` from PATH; `off`/`0`/`none`/`false` switch it off; any
/// other value is the binary to run.
pub fn helper_bin(setting: Option<&str>, default: &str) -> Option<String> {
    match setting.map(str::trim) {
        None | Some("") => Some(default.to_string()),
        Some(v)
            if matches!(
                v.to_ascii_lowercase().as_str(),
                "off" | "0" | "none" | "false"
            ) =>
        {
            None
        }
        Some(v) => Some(v.to_string()),
    }
}

/// Which nvidia-smi to run, or `None` when switched off.
pub fn nvidia_smi_bin(setting: Option<&str>) -> Option<String> {
    helper_bin(setting, "nvidia-smi")
}

/// Run `bin args` with stdin closed and stderr dropped; stdout is drained on
/// a thread (so a chatty helper cannot fill the pipe and stall), and the
/// child is killed once `timeout` passes. A missing binary, a non-zero exit
/// or a timeout are all `None`.
pub fn run_capped(bin: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // The pipe closes when the child exits or is killed, so this returns.
    let buf = reader.join().ok()?;
    if status?.success() {
        Some(buf)
    } else {
        None
    }
}

/// `name, memory.used, memory.total, utilization.gpu` (first GPU) from
/// `--format=csv,noheader,nounits` output.
pub fn parse_gpu(text: &str) -> Option<(String, u32, u32, u32)> {
    let first = text.lines().find(|l| !l.trim().is_empty())?;
    let bits: Vec<&str> = first.split(',').map(str::trim).collect();
    if bits.len() < 4 || bits[0].is_empty() {
        return None;
    }
    let num = |s: &str| s.parse::<u32>().ok();
    Some((
        bits[0].to_string(),
        num(bits[1])?,
        num(bits[2])?,
        num(bits[3]).unwrap_or(0),
    ))
}

/// Run nvidia-smi with a timeout. Any failure is `None`.
pub fn gpu(setting: Option<&str>, timeout: Duration) -> Option<(String, u32, u32, u32)> {
    let bin = nvidia_smi_bin(setting)?;
    let out = run_capped(
        &bin,
        &[
            "--query-gpu=name,memory.used,memory.total,utilization.gpu",
            "--format=csv,noheader,nounits",
        ],
        timeout,
    )?;
    parse_gpu(&String::from_utf8_lossy(&out))
}

pub const PDF_TIMEOUT: Duration = Duration::from_secs(5);

/// First-page text of a PDF via poppler's `pdftotext`, optional like
/// nvidia-smi: `ONE_PDFTOTEXT=off` skips it, any other value is the binary.
/// The bytes go to a fresh private file (create_new, 0600, unique name in
/// `tmp_dir`) that is removed afterwards, instead of the old shared
/// `/tmp/galion-one-page.pdf` that any local user could pre-create or read.
/// (Poppler 25 also reads `-` from stdin, but older poppler on the hosts may
/// not, so a file path keeps working everywhere.)
pub fn pdf_first_page(
    bytes: &[u8],
    setting: Option<&str>,
    tmp_dir: &std::path::Path,
    timeout: Duration,
) -> Option<String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let bin = helper_bin(setting, "pdftotext")?;
    let path = tmp_dir.join(format!(
        "galion-one-{}-{}.pdf",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .ok()?;
    let written = file.write_all(bytes).is_ok();
    drop(file);
    let out = if written {
        let p = path.to_string_lossy().into_owned();
        run_capped(&bin, &["-f", "1", "-l", "1", "-q", &p, "-"], timeout)
    } else {
        None
    };
    let _ = fs::remove_file(&path);
    Some(String::from_utf8_lossy(&out?).into_owned())
}

/// Which PDF text engine runs (`ONE_PDF_TEXT`):
/// * unset, blank, `rust`: the built-in pure-Rust reader only (pdf_text.rs),
///   no subprocess at all;
/// * `rust+pdftotext` (or `fallback`): Rust first, `pdftotext` only when the
///   Rust reader finds no text (encrypted file, CID font without ToUnicode);
/// * `pdftotext`: the old behaviour, subprocess only;
/// * `off`/`0`/`none`/`false`: no PDF text.
///
/// Unknown values mean `rust` (the safe choice: nothing gets executed).
/// `ONE_PDFTOTEXT` still names the binary (and `off` there disables it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfEngine {
    Rust,
    RustThenPdftotext,
    Pdftotext,
    Off,
}

pub fn pdf_engine(setting: Option<&str>) -> PdfEngine {
    match setting
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
        .unwrap_or("")
    {
        "rust+pdftotext" | "fallback" => PdfEngine::RustThenPdftotext,
        "pdftotext" => PdfEngine::Pdftotext,
        "off" | "0" | "none" | "false" => PdfEngine::Off,
        _ => PdfEngine::Rust,
    }
}

/// The built-in reader on a worker thread, given at most `timeout` (its own
/// caps bound the work; the timeout is a second fence).
pub fn pdf_first_page_rust(bytes: &[u8], timeout: Duration) -> Option<String> {
    let owned = bytes.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("pdf-text".into())
        .spawn(move || {
            let _ = tx.send(crate::pdf_text::first_page_text(
                &owned,
                crate::pdf_text::DEFAULT_LIMITS,
            ));
        })
        .ok()?;
    match rx.recv_timeout(timeout).ok()? {
        crate::pdf_text::Outcome::Text(t) => Some(t),
        _ => None,
    }
}

/// First-page text by the chosen engine; also says which engine produced
/// it ("rust", "pdftotext" or "none").
pub fn pdf_text(
    bytes: &[u8],
    engine: PdfEngine,
    pdftotext: Option<&str>,
    tmp_dir: &std::path::Path,
    timeout: Duration,
) -> (Option<String>, &'static str) {
    let nonempty = |t: Option<String>| t.filter(|s| !s.trim().is_empty());
    match engine {
        PdfEngine::Off => (None, "none"),
        PdfEngine::Rust => match nonempty(pdf_first_page_rust(bytes, timeout)) {
            Some(t) => (Some(t), "rust"),
            None => (None, "none"),
        },
        PdfEngine::Pdftotext => {
            match nonempty(pdf_first_page(bytes, pdftotext, tmp_dir, timeout)) {
                Some(t) => (Some(t), "pdftotext"),
                None => (None, "none"),
            }
        }
        PdfEngine::RustThenPdftotext => {
            if let Some(t) = nonempty(pdf_first_page_rust(bytes, timeout)) {
                return (Some(t), "rust");
            }
            match nonempty(pdf_first_page(bytes, pdftotext, tmp_dir, timeout)) {
                Some(t) => (Some(t), "pdftotext"),
                None => (None, "none"),
            }
        }
    }
}

/// Tests that write a helper script and then run it share this lock: a
/// fork in a parallel test can briefly hold the script open for writing,
/// which makes exec fail with ETXTBSY ("Text file busy").
#[cfg(test)]
pub(crate) static EXEC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_scan_sees_this_test_process() {
        let me = std::process::id().to_string();
        let mine = fs::read(format!("/proc/{me}/cmdline"));
        if mine.is_err() {
            eprintln!("SKIP: /proc not readable here");
            return;
        }
        let all = cmdlines("/proc");
        let exe = std::env::args().next().unwrap();
        assert!(all.iter().any(|l| l.starts_with(&exe)), "{exe} not in scan");
    }

    #[test]
    fn proc_scan_joins_argv_and_skips_non_pids() {
        let root = std::env::temp_dir().join(format!("one-proc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (pid, cmd) in [
            ("12", &b"nginx: master process /usr/sbin/nginx\0"[..]),
            ("13", b"/usr/bin/cloudflared\0tunnel\0run\0"),
            ("14", b""),
            ("self", b"ignored\0"),
        ] {
            fs::create_dir_all(root.join(pid)).unwrap();
            fs::write(root.join(pid).join("cmdline"), cmd).unwrap();
        }
        fs::create_dir_all(root.join("99")).unwrap(); // vanished: no cmdline
        let mut all = cmdlines(root.to_str().unwrap());
        all.sort();
        assert_eq!(
            all,
            vec![
                "/usr/bin/cloudflared tunnel run".to_string(),
                "nginx: master process /usr/sbin/nginx".to_string(),
            ]
        );
        let _ = fs::remove_dir_all(&root);
        assert!(cmdlines("/definitely/not/here").is_empty());
    }

    #[test]
    fn nvidia_setting() {
        assert_eq!(nvidia_smi_bin(None).as_deref(), Some("nvidia-smi"));
        assert_eq!(nvidia_smi_bin(Some(" ")).as_deref(), Some("nvidia-smi"));
        for off in ["off", "OFF", "0", "none", "false"] {
            assert_eq!(nvidia_smi_bin(Some(off)), None);
        }
        assert_eq!(
            nvidia_smi_bin(Some("/opt/x/nvidia-smi")).as_deref(),
            Some("/opt/x/nvidia-smi")
        );
    }

    #[test]
    fn gpu_parse() {
        assert_eq!(
            parse_gpu("NVIDIA A40, 1234, 46068, 7\nNVIDIA A40, 0, 46068, 0\n"),
            Some(("NVIDIA A40".to_string(), 1234, 46068, 7))
        );
        assert_eq!(
            parse_gpu("NVIDIA A40, 1, 2, [N/A]\n"),
            Some(("NVIDIA A40".to_string(), 1, 2, 0))
        );
        assert_eq!(parse_gpu(""), None);
        assert_eq!(parse_gpu("No devices were found\n"), None);
        assert_eq!(parse_gpu("x, [N/A], 2, 3"), None);
    }

    #[test]
    fn gpu_absent_or_off_is_none() {
        assert_eq!(gpu(Some("off"), GPU_TIMEOUT), None);
        assert_eq!(gpu(Some("/nonexistent/nvidia-smi-xyz"), GPU_TIMEOUT), None);
        // A binary that exits non-zero.
        if std::path::Path::new("/bin/false").exists() {
            assert_eq!(gpu(Some("/bin/false"), GPU_TIMEOUT), None);
        }
    }
}

#[cfg(test)]
mod fake_smi_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(name: &str, body: &str) -> Option<String> {
        if !std::path::Path::new("/bin/sh").exists() {
            return None;
        }
        let dir = std::env::temp_dir().join(format!("one-smi-{}", std::process::id()));
        fs::create_dir_all(&dir).ok()?;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).ok()?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).ok()?;
        Some(path.to_string_lossy().into_owned())
    }

    #[test]
    fn fake_nvidia_smi_is_read_and_a_hung_one_is_killed() {
        let _exec = super::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let Some(good) = script("good", "echo 'Fake GPU, 512, 8192, 42'") else {
            eprintln!("SKIP: no /bin/sh");
            return;
        };
        assert_eq!(
            gpu(Some(&good), GPU_TIMEOUT),
            Some(("Fake GPU".to_string(), 512, 8192, 42))
        );
        let hung = script("hung", "exec sleep 30").unwrap();
        let t = Instant::now();
        assert_eq!(gpu(Some(&hung), Duration::from_millis(300)), None);
        assert!(t.elapsed() < Duration::from_secs(5));
        let _ = fs::remove_dir_all(
            std::env::temp_dir().join(format!("one-smi-{}", std::process::id())),
        );
    }
}

#[cfg(test)]
mod pdf_tests {
    use super::*;

    /// A one-page PDF saying `Hello Galion One`, with a correct xref.
    fn tiny_pdf() -> Vec<u8> {
        let stream = "BT /F1 24 Tf 72 720 Td (Hello Galion One) Tj ET";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
        }
        let xref = pdf.len();
        pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
        for o in offs {
            pdf.push_str(&format!("{o:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        pdf.into_bytes()
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("one-pdf-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn pdftotext_off_or_missing_is_none_and_leaves_no_file() {
        let _exec = super::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("off");
        assert_eq!(
            pdf_first_page(&tiny_pdf(), Some("off"), &dir, PDF_TIMEOUT),
            None
        );
        assert_eq!(
            pdf_first_page(
                &tiny_pdf(),
                Some("/nonexistent/pdftotext-xyz"),
                &dir,
                PDF_TIMEOUT
            ),
            None
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            0,
            "temp file left behind"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_pdftotext_reads_page_one_when_installed() {
        let _exec = super::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if Command::new("pdftotext").arg("-v").output().is_err() {
            eprintln!("SKIP: pdftotext not installed");
            return;
        }
        let dir = scratch("real");
        let text = pdf_first_page(&tiny_pdf(), None, &dir, PDF_TIMEOUT).unwrap();
        assert!(text.contains("Hello Galion One"), "{text:?}");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            0,
            "temp file left behind"
        );
        // Garbage is a non-zero exit, so None, and still no file left.
        assert_eq!(pdf_first_page(b"not a pdf", None, &dir, PDF_TIMEOUT), None);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn chatty_helper_does_not_stall_on_a_full_pipe() {
        let _exec = super::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !std::path::Path::new("/bin/sh").exists() {
            eprintln!("SKIP: no /bin/sh");
            return;
        }
        // 1 MiB on stdout is far past the 64 KiB pipe buffer.
        let out = run_capped(
            "/bin/sh",
            &["-c", "head -c 1048576 /dev/zero"],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.len(), 1_048_576);
    }

    #[test]
    fn engine_setting_parses_and_unknown_means_rust() {
        assert_eq!(pdf_engine(None), PdfEngine::Rust);
        assert_eq!(pdf_engine(Some(" ")), PdfEngine::Rust);
        assert_eq!(pdf_engine(Some("RUST")), PdfEngine::Rust);
        assert_eq!(
            pdf_engine(Some("rust+pdftotext")),
            PdfEngine::RustThenPdftotext
        );
        assert_eq!(pdf_engine(Some("fallback")), PdfEngine::RustThenPdftotext);
        assert_eq!(pdf_engine(Some("pdftotext")), PdfEngine::Pdftotext);
        for off in ["off", "0", "none", "FALSE"] {
            assert_eq!(pdf_engine(Some(off)), PdfEngine::Off);
        }
        assert_eq!(pdf_engine(Some("/bin/sh -c evil")), PdfEngine::Rust);
    }

    #[test]
    fn rust_engine_needs_no_binary_and_fallback_runs_only_when_needed() {
        let _exec = super::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("engine");
        let marker = dir.join("ran");
        let fake = dir.join("fake-pdftotext");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\ntouch '{}'\necho 'from the fake pdftotext'\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        let fake_s = fake.to_string_lossy().into_owned();
        let t = Duration::from_secs(5);
        // Rust: text, binary never run (even when one is configured).
        let (txt, by) = pdf_text(&tiny_pdf(), PdfEngine::Rust, Some(&fake_s), &dir, t);
        assert_eq!((txt.as_deref(), by), (Some("Hello Galion One"), "rust"));
        // Fallback mode, Rust succeeds: still no subprocess.
        let (_, by) = pdf_text(
            &tiny_pdf(),
            PdfEngine::RustThenPdftotext,
            Some(&fake_s),
            &dir,
            t,
        );
        assert_eq!(by, "rust");
        assert!(!marker.exists(), "pdftotext ran although Rust had the text");
        // Fallback mode, password-protected file: the binary runs.
        let locked = fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/pdf/gs-userpw.pdf"
        ))
        .unwrap();
        let (_, by) = pdf_text(&locked, PdfEngine::Rust, Some(&fake_s), &dir, t);
        assert_eq!(by, "none");
        let (txt, by) = pdf_text(
            &locked,
            PdfEngine::RustThenPdftotext,
            Some(&fake_s),
            &dir,
            t,
        );
        assert_eq!(
            (txt.as_deref().map(str::trim), by),
            (Some("from the fake pdftotext"), "pdftotext")
        );
        assert!(marker.exists());
        // Off: nothing, no subprocess.
        fs::remove_file(&marker).unwrap();
        assert_eq!(
            pdf_text(&tiny_pdf(), PdfEngine::Off, Some(&fake_s), &dir, t),
            (None, "none")
        );
        assert!(!marker.exists());
        // Only the fake script and nothing else (temp PDFs removed).
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("fake-pdftotext")]);
        let _ = fs::remove_dir_all(&dir);
    }
}
