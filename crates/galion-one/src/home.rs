// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! galion.app public home page (Mate for Slavik, t545u-t550u, 2026-11-01 CET).
//!
//! The page is compiled into the binary (`include_bytes!`), with a gzip copy
//! made by `gzip -9 -n -k -f home/index.html` (a test checks it still matches
//! the HTML). It is served only on the home hosts (`ONE_HOME_HOSTS`, comma
//! list, default `galion.app,www.galion.app`); one.galion.app and loopback
//! keep the Galion One app. `ONE_HOME_HOSTS=` (empty) switches the home page
//! off without a new binary. Brotli is left to the Cloudflare edge.
//!
//! Paid plans (t548u/t550u): one universal subscription, `plus` (EUR 20 / month)
//! and `pro` (EUR 220 / month). Checkout belongs to galion-accounts
//! (`/api/checkout?plan=plus|pro`, another worker). The page shows
//! "Coming soon" until that answers: home.js sends `HEAD /api/checkout?plan=..`
//! and only a 2xx/3xx turns the button into a link; that probe runs only with
//! `ONE_CHECKOUT=probe` (off by default, so no 404 noise today). `ONE_CHECKOUT=on` renders
//! the links server-side (for visitors without JS) once accounts is live.
//! `ONE_WAITLIST_URL` (https: or mailto:) turns the waiting buttons into
//! "Coming soon: join the waitlist" links. See docs/payments/galion-home-checkout.md.

pub const HTML: &[u8] = include_bytes!("../home/index.html");
pub const HTML_GZ: &[u8] = include_bytes!("../home/index.html.gz");
pub const JS: &[u8] = include_bytes!("../home/home.js");
pub const MARK: &[u8] = include_bytes!("../home/galion-mark.svg");
pub const ACCOUNT: &[u8] = include_bytes!("../home/account.html");

const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub const PLANS: [&str; 2] = ["plus", "pro"];

pub fn host_on(host: &str) -> bool {
    let list =
        std::env::var("ONE_HOME_HOSTS").unwrap_or_else(|_| "galion.app,www.galion.app".to_string());
    super::v2_host_in(&list, host)
}

fn safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("mailto:"))
        && url.len() < 512
        && url.bytes().all(|b| {
            b.is_ascii_graphic() && !matches!(b, b'"' | b'<' | b'>' | b'\\' | b'`' | b'\'')
        })
}

/// The page with the plan buttons filled in, or None when nothing is
/// configured (then the precompressed copy is served as is).
pub fn render(get: &dyn Fn(&str) -> Option<String>) -> Option<Vec<u8>> {
    let mode = get("ONE_CHECKOUT")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let live = mode == "on";
    let probe = mode == "probe";
    let wait = get("ONE_WAITLIST_URL").filter(|u| safe_url(u));
    if !live && !probe && wait.is_none() {
        return None;
    }
    let mut html = String::from_utf8_lossy(HTML).into_owned();
    if probe {
        html = html.replacen(
            "<html lang=\"en\">",
            "<html lang=\"en\" data-checkout=\"probe\">",
            1,
        );
    }
    if live {
        if let (Some(a), Some(b)) = (html.find("<!--soon-->"), html.find("<!--/soon-->")) {
            html.replace_range(a..b + "<!--/soon-->".len(), "");
        }
    }
    for plan in PLANS {
        if probe && wait.is_none() {
            break;
        }
        let button = if live {
            format!("<a class=\"btn pri\" href=\"/api/checkout?plan={plan}\">Subscribe</a>")
        } else {
            let w = wait.as_deref().unwrap_or("").replace('&', "&amp;");
            format!(
                "<a class=\"btn\" href=\"{w}\" rel=\"noopener\" data-plan=\"{plan}\">Coming soon: join the waitlist</a>"
            )
        };
        let open = format!("<!--pay:{plan}-->");
        if let Some(a) = html.find(&open) {
            if let Some(end) = html[a..].find("<!--/pay-->") {
                html.replace_range(a..a + end + "<!--/pay-->".len(), &button);
            }
        }
    }
    Some(html.into_bytes())
}

fn rendered() -> Option<&'static [u8]> {
    static SLOT: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| render(&|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())))
        .as_deref()
}

pub(crate) fn etag(body: &[u8]) -> String {
    // FNV-1a 64: a stable validator for the compiled-in bytes.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("\"g{h:016x}\"")
}

fn gzip_ok(accept: &str) -> bool {
    accept.split(',').any(|part| {
        let mut it = part.split(';');
        let name = it.next().unwrap_or("").trim();
        let zero = it.any(|q| {
            let q = q.trim().replace(' ', "");
            q == "q=0" || q == "q=0.0" || q == "q=0.00" || q == "q=0.000"
        });
        name.eq_ignore_ascii_case("gzip") && !zero
    })
}

/// A full HTTP response for a home-page route, or None to fall through to
/// the Galion One routes (fonts, /space, APIs and so on).
pub fn respond(method: &str, path: &str, accept: &str, if_none_match: &str) -> Option<Vec<u8>> {
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        return None;
    }
    let (kind, plain, gz, cache): (&str, &[u8], Option<&[u8]>, &str) = match path {
        "/" | "/index.html" => match rendered() {
            // Configured buttons: plain body (the Cloudflare edge compresses it).
            Some(page) => (
                "text/html; charset=utf-8",
                page,
                None,
                "public, max-age=300",
            ),
            None => (
                "text/html; charset=utf-8",
                HTML,
                Some(HTML_GZ),
                "public, max-age=300",
            ),
        },
        // Placeholder only: the real account system is another worker's job (t549u).
        "/account" | "/account/" => ("text/html; charset=utf-8", ACCOUNT, None, "no-store"),
        "/pricing" | "/pricing/" => {
            return Some(
                b"HTTP/1.1 301 Moved Permanently\r\nLocation: /#pricing\r\nContent-Length: 0\r\nCache-Control: public, max-age=3600\r\nConnection: close\r\n\r\n".to_vec(),
            );
        }
        "/home.js" => (
            "text/javascript; charset=utf-8",
            JS,
            None,
            "public, max-age=3600",
        ),
        "/galion-mark.svg" => ("image/svg+xml", MARK, None, "public, max-age=86400"),
        _ => return None,
    };
    let tag = etag(plain);
    let mut extra = String::new();
    let body: &[u8] = match gz {
        Some(z) if gzip_ok(accept) => {
            extra.push_str("Content-Encoding: gzip\r\n");
            z
        }
        _ => plain,
    };
    if gz.is_some() {
        extra.push_str("Vary: Accept-Encoding\r\n");
    }
    let common = format!(
        "Content-Type: {kind}\r\nCache-Control: {cache}\r\nETag: {tag}\r\n{extra}X-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nContent-Security-Policy: {CSP}\r\nConnection: close\r\n"
    );
    if if_none_match.split(',').any(|t| t.trim() == tag) {
        return Some(format!("HTTP/1.1 304 Not Modified\r\n{common}\r\n").into_bytes());
    }
    let mut out = format!(
        "HTTP/1.1 200 OK\r\n{common}Content-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    if !head_only {
        out.extend_from_slice(body);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gunzip(z: &[u8]) -> Vec<u8> {
        assert_eq!(&z[..3], &[0x1f, 0x8b, 8], "gzip magic");
        assert_eq!(z[3], 0, "made with gzip -n: no name or extra fields");
        let (out, res) = crate::inflate::inflate_raw(&z[10..z.len() - 8], 1 << 22);
        assert!(res.is_ok());
        out
    }

    #[test]
    fn gzip_copy_matches_html() {
        assert_eq!(
            gunzip(HTML_GZ),
            HTML,
            "run: gzip -9 -n -k -f home/index.html"
        );
    }

    #[test]
    fn page_is_small_and_self_contained() {
        let total = HTML.len() + JS.len() + MARK.len() + ACCOUNT.len();
        assert!(total < 300 * 1024, "page weight {total}");
        let s = std::str::from_utf8(HTML).unwrap();
        for bad in [
            "http://",
            "cdn.",
            "googleapis",
            "<iframe",
            "stripe.com",
            "$",
        ] {
            assert!(!s.contains(bad), "{bad}");
        }
        assert!(s.contains("https://one.galion.app"));
        assert!(s.contains("id=\"pricing\""));
        assert!(s.contains("href=\"/account\""));
        for plan in PLANS {
            assert!(s.contains(&format!("<!--pay:{plan}-->")), "{plan}");
            assert!(s.contains(&format!("data-plan=\"{plan}\"")), "{plan}");
        }
    }

    #[test]
    fn hosts() {
        assert!(super::super::v2_host_in(
            "galion.app,www.galion.app",
            "galion.app"
        ));
        assert!(super::super::v2_host_in(
            "galion.app,www.galion.app",
            "WWW.galion.app:443"
        ));
        assert!(!super::super::v2_host_in(
            "galion.app,www.galion.app",
            "one.galion.app"
        ));
        assert!(!super::super::v2_host_in("", "galion.app"));
    }

    #[test]
    fn plan_buttons() {
        assert!(
            render(&|_| None).is_none(),
            "nothing configured: static page"
        );
        let live = |k: &str| (k == "ONE_CHECKOUT").then(|| "on".to_string());
        let s = String::from_utf8(render(&live).unwrap()).unwrap();
        assert!(s.contains("href=\"/api/checkout?plan=plus\""));
        assert!(s.contains("href=\"/api/checkout?plan=pro\""));
        let probe = |k: &str| (k == "ONE_CHECKOUT").then(|| "probe".to_string());
        let s = String::from_utf8(render(&probe).unwrap()).unwrap();
        assert!(s.contains("<html lang=\"en\" data-checkout=\"probe\">"));
        assert!(
            s.contains("<!--pay:plus-->"),
            "probe keeps Coming soon until accounts answers"
        );
        let wait = |k: &str| (k == "ONE_WAITLIST_URL").then(|| "mailto:hi@example.org".to_string());
        let s = String::from_utf8(render(&wait).unwrap()).unwrap();
        assert_eq!(s.matches("join the waitlist").count(), 2);
        let bad = |k: &str| (k == "ONE_WAITLIST_URL").then(|| "javascript:alert(1)".to_string());
        assert!(render(&bad).is_none());
        assert!(!safe_url("https://x.example/\"onmouseover=1"));
    }

    #[test]
    fn routes() {
        let r = String::from_utf8_lossy(&respond("GET", "/", "gzip, br", "").unwrap()).into_owned();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(r.contains("Content-Encoding: gzip\r\n"));
        assert!(r.contains("Vary: Accept-Encoding\r\n"));
        let r = respond("GET", "/", "gzip;q=0", "").unwrap();
        assert!(!String::from_utf8_lossy(&r).contains("Content-Encoding"));
        assert!(r.ends_with(HTML));
        let head = respond("HEAD", "/", "", "").unwrap();
        assert!(String::from_utf8_lossy(&head).ends_with("\r\n\r\n"));
        let tag = etag(HTML);
        let r = respond("GET", "/", "gzip", &tag).unwrap();
        assert!(String::from_utf8_lossy(&r).starts_with("HTTP/1.1 304"));
        let r = respond("GET", "/pricing", "", "").unwrap();
        assert!(String::from_utf8_lossy(&r).contains("Location: /#pricing\r\n"));
        let r = String::from_utf8_lossy(&respond("GET", "/account", "", "").unwrap()).into_owned();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n") && r.contains("Cache-Control: no-store\r\n"));
        assert!(respond("GET", "/home.js", "", "").is_some());
        assert!(respond("GET", "/font/jost.woff2", "", "").is_none());
        assert!(
            respond("HEAD", "/api/checkout", "", "").is_none(),
            "accounts owns it"
        );
        assert!(respond("POST", "/", "", "").is_none());
    }
}
