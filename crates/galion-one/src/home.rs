// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! galion.app public home page (Mate for Slavik, t545u–t550u + t880u, 2026-11-06 CET).
//!
//! The page is compiled into the binary (`include_bytes!`), with a gzip copy
//! made by `gzip -9 -n -k -f home/index.html` (a test checks it still matches
//! the HTML). It is served only on the home hosts (`ONE_HOME_HOSTS`, comma
//! list, default `galion.app,www.galion.app`); one.galion.app and loopback
//! keep the Galion One app. `ONE_HOME_HOSTS=` (empty) switches the home page
//! off without a new binary. Brotli is left to the Cloudflare edge.
//!
//! Pricing (owner config `home/pricing.json`): Galion OS free forever under
//! the perpetual open MIT licence; Galion One `$20` / `€20` per month;
//! Galion Research `$220` / `€220` one-time. Same nominal numbers, not
//! FX-converted. Currency is chosen server-side from `CF-IPCountry` or
//! `x-vercel-ip-country`, with `?currency=usd|eur` for testing. No third-party
//! geo API, no cookies. The HTML is filled on the server so there is no
//! client currency flicker.
//!
//! Checkout belongs to galion-accounts (`/api/checkout?plan=one|research`).
//! The page shows "Coming soon" until that answers: home.js sends
//! `HEAD /api/checkout?plan=..` and only a 2xx/3xx turns the button into a
//! link; that probe runs only with `ONE_CHECKOUT=probe`. `ONE_CHECKOUT=on`
//! renders the links server-side. `ONE_WAITLIST_URL` turns waiting buttons
//! into waitlist links. See docs/payments/galion-home-checkout.md.

use crate::pricing::{self, Currency};

pub const HTML: &[u8] = include_bytes!("../home/index.html");
pub const HTML_GZ: &[u8] = include_bytes!("../home/index.html.gz");
pub const JS: &[u8] = include_bytes!("../home/home.js");
pub const MARK: &[u8] = include_bytes!("../home/galion-mark.svg");
/// Friendly placeholder for `/account` while galion-accounts is off or down.
pub const ACCOUNT: &[u8] = include_bytes!("../home/account.html");
/// galion.app/operating-system: the public Galion OS page (2026-11-01 CET).
/// Plain body, no script; the Cloudflare edge compresses it.
pub const OS_PAGE: &[u8] = include_bytes!("../os/operating-system.html");
/// galion.app/obsidian: the Obsidian OpenMind stack page for Galion OS.
pub const OBSIDIAN: &[u8] = include_bytes!("../os/obsidian.html");

const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Checkout plan ids (must match `<!--pay:…-->` markers and home.js).
pub const PLANS: [&str; 2] = ["one", "research"];


pub fn host_on(host: &str) -> bool {
    let list =
        std::env::var("ONE_HOME_HOSTS").unwrap_or_else(|_| "galion.app,www.galion.app".to_string());
    super::v2_host_in(&list, host)
}

/// Country string for pricing: Cloudflare first, then Vercel.
pub fn pricing_country<'a>(cf_ip_country: &'a str, vercel_ip_country: &'a str) -> &'a str {
    pricing::country_from_headers(
        (!cf_ip_country.is_empty()).then_some(cf_ip_country),
        (!vercel_ip_country.is_empty()).then_some(vercel_ip_country),
    )
}

fn safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("mailto:"))
        && url.len() < 512
        && url.bytes().all(|b| {
            b.is_ascii_graphic() && !matches!(b, b'"' | b'<' | b'>' | b'\\' | b'`' | b'\'')
        })
}

fn replace_marker(html: &mut String, open: &str, close: &str, value: &str) {
    if let Some(a) = html.find(open) {
        if let Some(end) = html[a..].find(close) {
            html.replace_range(a..a + end + close.len(), value);
        }
    }
}

/// Fill currency amounts / note, then optional checkout buttons.
pub fn render_page(
    currency: Currency,
    get: &dyn Fn(&str) -> Option<String>,
) -> Vec<u8> {
    let mode = get("ONE_CHECKOUT")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let live = mode == "on";
    let probe = mode == "probe";
    let wait = get("ONE_WAITLIST_URL").filter(|u| safe_url(u));

    let mut html = String::from_utf8_lossy(HTML).into_owned();
    let p = pricing::prices(currency);

    // Server-side currency: set on <html> so CSS/content match with no client flicker.
    html = html.replacen(
        "<html lang=\"en\" data-currency=\"usd\">",
        &format!("<html lang=\"en\" data-currency=\"{}\">", currency.as_str()),
        1,
    );

    let one_amt = format!(
        "{}<span>/ {}</span>",
        pricing::amount_html(currency, p.one),
        p.one_period
    );
    let research_amt = format!(
        "{}<span> {}</span>",
        pricing::amount_html(currency, p.research),
        p.research_period
    );
    replace_marker(&mut html, "<!--ONE_AMT-->", "<!--/ONE_AMT-->", &one_amt);
    replace_marker(
        &mut html,
        "<!--RESEARCH_AMT-->",
        "<!--/RESEARCH_AMT-->",
        &research_amt,
    );
    replace_marker(
        &mut html,
        "<!--PRICE_NOTE-->",
        "<!--/PRICE_NOTE-->",
        pricing::note_html(currency),
    );

    if probe {
        html = html.replacen(
            &format!("<html lang=\"en\" data-currency=\"{}\">", currency.as_str()),
            &format!(
                "<html lang=\"en\" data-currency=\"{}\" data-checkout=\"probe\">",
                currency.as_str()
            ),
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
        if !live && wait.is_none() {
            break;
        }
        let label = if plan == "research" {
            "Buy"
        } else {
            "Subscribe"
        };
        let button = if live {
            format!("<a class=\"btn pri\" href=\"/api/checkout?plan={plan}\">{label}</a>")
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
    html.into_bytes()
}

/// Back-compat helper used by older tests: checkout-only render, USD default.
pub fn render(get: &dyn Fn(&str) -> Option<String>) -> Option<Vec<u8>> {
    let mode = get("ONE_CHECKOUT")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let wait = get("ONE_WAITLIST_URL").filter(|u| safe_url(u));
    if mode != "on" && mode != "probe" && wait.is_none() {
        return None;
    }
    Some(render_page(Currency::Usd, get))
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

fn redirect(location: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nCache-Control: public, max-age=3600\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

/// A full HTTP response for a home-page route, or None to fall through to
/// the Galion One routes (fonts, /space, APIs and so on).
///
/// `country` is the raw edge country header value (may be empty).
/// `query` is the raw URL query string (may be empty); `currency=usd|eur` overrides.
pub fn respond(
    method: &str,
    path: &str,
    accept: &str,
    if_none_match: &str,
    country: &str,
    query: &str,
) -> Option<Vec<u8>> {
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        return None;
    }

    if matches!(path, "/pricing" | "/pricing/") {
        return Some(redirect("/#pricing"));
    }
    // Live 404 today; send people to the Galion One section on the home page.
    if matches!(path, "/galion-one" | "/galion-one/") {
        return Some(redirect("/#galion"));
    }

    let currency = pricing::resolve(country, query);
    let override_on = query
        .split('&')
        .any(|p| p.split('=').next().unwrap_or("").eq_ignore_ascii_case("currency"));

    // Home HTML is always filled server-side (currency + optional checkout) so
    // the visitor never sees a wrong currency flash. Edge compresses the body.
    let home_owned: Option<Vec<u8>> = matches!(path, "/" | "/index.html")
        .then(|| render_page(currency, &|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())));

    let (kind, plain, gz, cache, vary_currency): (&str, &[u8], Option<&[u8]>, &str, bool) =
        match path {
            "/" | "/index.html" => (
                "text/html; charset=utf-8",
                home_owned.as_deref().unwrap(),
                None,
                if override_on {
                    "private, max-age=60"
                } else {
                    "public, max-age=300"
                },
                true,
            ),
            "/account" | "/account/" => ("text/html; charset=utf-8", ACCOUNT, None, "no-store", false),
            "/operating-system" | "/operating-system/" => {
                ("text/html; charset=utf-8", OS_PAGE, None, "public, max-age=300", false)
            }
            "/obsidian" | "/obsidian/" => {
                ("text/html; charset=utf-8", OBSIDIAN, None, "public, max-age=300", false)
            }
            "/home.js" => (
                "text/javascript; charset=utf-8",
                JS,
                None,
                "public, max-age=3600",
                false,
            ),
            "/galion-mark.svg" => ("image/svg+xml", MARK, None, "public, max-age=86400", false),
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
    if vary_currency && !override_on {
        // So shared caches keep USD and EUR copies apart.
        extra.push_str("Vary: CF-IPCountry, X-Vercel-Ip-Country\r\n");
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
    // home_owned must live through the body copy above.
    drop(home_owned);
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

    fn body_of(resp: &[u8]) -> &str {
        let s = std::str::from_utf8(resp).unwrap();
        s.split("\r\n\r\n").nth(1).unwrap_or("")
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
        assert!(s.contains("Galion Research"));
        assert!(!s.contains("Hermes Research") && !s.contains("Hermes research"));
        assert!(s.contains("G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network"));
        assert!(s.contains("Developed and distributed by Galion Labs"));
        assert!(s.contains("Perpetual Open MIT License"));
        assert!(s.contains("Copyright © 2026 Galion Labs. All rights reserved."));
        assert!(s.contains("data-currency=\"usd\""));
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
            "nothing configured: currency-only path uses render_page"
        );
        let live = |k: &str| (k == "ONE_CHECKOUT").then(|| "on".to_string());
        let s = String::from_utf8(render(&live).unwrap()).unwrap();
        assert!(s.contains("href=\"/api/checkout?plan=one\""));
        assert!(s.contains("href=\"/api/checkout?plan=research\""));
        let probe = |k: &str| (k == "ONE_CHECKOUT").then(|| "probe".to_string());
        let s = String::from_utf8(render(&probe).unwrap()).unwrap();
        assert!(s.contains("data-checkout=\"probe\""));
        assert!(
            s.contains("<!--pay:one-->"),
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
    fn currency_rendered_server_side() {
        let usd = String::from_utf8(render_page(Currency::Usd, &|_| None)).unwrap();
        assert!(usd.contains("data-currency=\"usd\""));
        assert!(usd.contains("&#36;20"));
        assert!(usd.contains("&#36;220"));
        assert!(usd.contains("Prices in US dollars."));
        assert!(!usd.contains("$"));

        let eur = String::from_utf8(render_page(Currency::Eur, &|_| None)).unwrap();
        assert!(eur.contains("data-currency=\"eur\""));
        assert!(eur.contains("&euro;20"));
        assert!(eur.contains("&euro;220"));
        assert!(eur.contains("Prices in euros."));

        let r = respond("GET", "/", "", "", "DE", "").unwrap();
        let b = body_of(&r);
        assert!(b.contains("data-currency=\"eur\""));
        assert!(b.contains("&euro;20"));
        assert!(std::str::from_utf8(&r).unwrap().contains("Vary: CF-IPCountry"));

        let r = respond("GET", "/", "", "", "US", "").unwrap();
        assert!(body_of(&r).contains("data-currency=\"usd\""));

        let r = respond("GET", "/", "", "", "DE", "currency=usd").unwrap();
        assert!(body_of(&r).contains("data-currency=\"usd\""));
        assert!(std::str::from_utf8(&r).unwrap().contains("Cache-Control: private"));
    }

    #[test]
    fn os_pages_carry_footer() {
        for (path, page, head) in [
            (
                "/operating-system",
                OS_PAGE,
                "<h1><span class=\"g\">Galion Operating System</span></h1>",
            ),
            (
                "/obsidian",
                OBSIDIAN,
                "Type or speak, then watch what happens.",
            ),
        ] {
            let s = std::str::from_utf8(page).unwrap();
            assert!(page.len() < 28 * 1024, "{path} weight {}", page.len());
            assert!(s.contains(head), "{path}");
            for bad in [
                "http://",
                "cdn.",
                "googleapis",
                "<script",
                "<iframe",
                "@import",
                "url(",
            ] {
                assert!(!s.contains(bad), "{path}: {bad}");
            }
            assert!(s.contains("prefers-reduced-motion:reduce"), "{path}");
            assert!(s.contains("https://github.com/galion-studio/galion-operating-system"));
            assert!(s.contains("G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network"));
            assert!(s.contains("Developed and distributed by Galion Labs"));
            assert!(s.contains("Perpetual Open MIT License"));
            assert!(s.contains("Copyright © 2026 Galion Labs. All rights reserved."));
            for p in [path.to_string(), format!("{path}/")] {
                let r = respond("GET", &p, "gzip", "", "", "").unwrap();
                assert!(
                    r.starts_with(b"HTTP/1.1 200 OK\r\n") && r.ends_with(page),
                    "{p}"
                );
            }
            let r = respond("GET", path, "", &etag(page), "", "").unwrap();
            assert!(r.starts_with(b"HTTP/1.1 304"));
        }
        let s = std::str::from_utf8(OS_PAGE).unwrap();
        assert!(s.contains("Galion OS is a free, open-source operating system built on Linux and Rust. It runs on any device, from a laptop to a tiny Raspberry Pi. MIT licensed."));
        assert!(s.contains(
            "One window. One gradient. One prompt field. Type or speak, then watch what happens."
        ));
        assert!(std::str::from_utf8(OBSIDIAN)
            .unwrap()
            .contains("href=\"/operating-system\""));
        assert!(respond("POST", "/obsidian", "", "", "", "").is_none());
    }

    #[test]
    fn routes() {
        let r = String::from_utf8_lossy(&respond("GET", "/", "gzip, br", "", "", "").unwrap())
            .into_owned();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n"));
        // Dynamic currency page: no precompressed gzip (edge compresses).
        assert!(!r.contains("Content-Encoding: gzip\r\n"));
        let r = respond("GET", "/", "gzip;q=0", "", "US", "").unwrap();
        assert!(body_of(&r).contains("data-currency=\"usd\""));
        let head = respond("HEAD", "/", "", "", "", "").unwrap();
        assert!(String::from_utf8_lossy(&head).ends_with("\r\n\r\n"));
        let page = render_page(Currency::Usd, &|_| None);
        let tag = etag(&page);
        let r = respond("GET", "/", "gzip", &tag, "US", "").unwrap();
        assert!(String::from_utf8_lossy(&r).starts_with("HTTP/1.1 304"));
        let r = respond("GET", "/pricing", "", "", "", "").unwrap();
        assert!(String::from_utf8_lossy(&r).contains("Location: /#pricing\r\n"));
        let r = respond("GET", "/galion-one", "", "", "", "").unwrap();
        assert!(String::from_utf8_lossy(&r).contains("Location: /#galion\r\n"));
        let r = String::from_utf8_lossy(&respond("GET", "/account", "", "", "", "").unwrap())
            .into_owned();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n") && r.contains("Cache-Control: no-store\r\n"));
        assert!(respond("GET", "/home.js", "", "", "", "").is_some());
        assert!(respond("GET", "/font/jost.woff2", "", "", "", "").is_none());
        assert!(
            respond("HEAD", "/api/checkout", "", "", "", "").is_none(),
            "accounts owns it"
        );
        assert!(respond("POST", "/", "", "", "", "").is_none());
    }

    #[test]
    fn account_has_footer() {
        let s = std::str::from_utf8(ACCOUNT).unwrap();
        assert!(s.contains("G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network"));
        assert!(s.contains("Developed and distributed by Galion Labs"));
    }
}
