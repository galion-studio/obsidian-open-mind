// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Status line of the pure redirect, and proof it does not touch the network.

use openmind_origin::{redirect_response, DEFAULT_LOCATION, STATUS_CODE, STATUS_LINE};
use std::path::PathBuf;

const HOME: &str = "HTTP/1.1 302 Found\r\nLocation: /\r\nContent-Length: 0\r\n\r\n";
const CONSOLE: &str = "HTTP/1.1 302 Found\r\nLocation: /console\r\nContent-Length: 0\r\n\r\n";

#[test]
fn status_line_is_302_found() {
    let home = redirect_response("");
    let console = redirect_response("/console");
    assert_eq!(home.status_line(), STATUS_LINE);
    assert_eq!(console.status_line(), STATUS_LINE);
    assert_eq!(home.status_code(), STATUS_CODE);
    assert_eq!(STATUS_CODE, 302);

    let raw = home.to_string();
    let (status, rest) = raw.split_once("\r\n").expect("status line");
    assert_eq!(status, "HTTP/1.1 302 Found");
    assert!(rest.starts_with("Location: /\r\n"));
    assert_eq!(raw, HOME);
    assert_eq!(console.to_string(), CONSOLE);

    let (head, body) = raw.split_once("\r\n\r\n").expect("header end");
    assert_eq!(head.lines().count(), 3);
    assert!(body.is_empty());
    assert!(home.body().is_empty());
    assert!(console.body().is_empty());
}

#[test]
fn location_comes_from_the_argument_and_defaults_to_slash() {
    assert_eq!(DEFAULT_LOCATION, "/");
    assert_eq!(redirect_response("").location(), "/");
    assert_eq!(redirect_response("/").location(), "/");
    assert_eq!(redirect_response(DEFAULT_LOCATION).location(), "/");
    assert_eq!(redirect_response("/console").location(), "/console");
    assert_eq!(
        redirect_response("https://example.test/go").location(),
        "https://example.test/go"
    );
    assert_eq!(redirect_response("/ok\r\nEvil: 1").location(), "/");
    assert_eq!(redirect_response("/ok\nEvil: 1").location(), "/");
    assert_eq!(redirect_response("/ok\0").location(), "/");
    assert_eq!(
        redirect_response("").to_string(),
        redirect_response("/").to_string()
    );
    assert!(!redirect_response("/console").touches_network());
}

#[test]
fn sources_do_not_name_sockets_or_listeners() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("manifest");
    for token in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
        "UnixStream",
        "reqwest",
        "ureq",
        "hyper",
        "tokio",
        "mio::",
    ] {
        assert!(!lib.contains(token), "{token} in lib.rs");
        assert!(!manifest.contains(token), "{token} in Cargo.toml");
    }
    assert!(!manifest.contains("[[bin]]"));
    assert!(!lib.contains("fn main"));
}

#[test]
fn redirect_response_does_not_touch_the_network() {
    // Socket descriptors of this process. The namespace tcp table is shared
    // and its timer columns move without this function dialing, so it is not
    // a stable signal.
    let before_fd = fd_targets();
    let before_sockets = socket_fds(&before_fd);
    assert!(!before_fd.is_empty(), "proc fd table was not readable");

    let built = [
        redirect_response(""),
        redirect_response("/"),
        redirect_response("/console"),
        redirect_response("https://example.test/go"),
    ];
    for response in &built {
        assert_eq!(response.status_line(), STATUS_LINE);
        assert!(!response.touches_network());
        assert!(response.body().is_empty());
        let raw = response.to_string();
        assert!(raw.starts_with("HTTP/1.1 302 Found\r\n"));
        let body = raw.split_once("\r\n\r\n").expect("header end").1;
        assert!(body.is_empty());
    }

    let after_fd = fd_targets();
    assert_eq!(before_sockets, socket_fds(&after_fd));
    assert_eq!(before_fd, after_fd);
}

fn socket_fds(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| target.starts_with("socket:"))
        .cloned()
        .collect()
}

fn fd_targets() -> Vec<String> {
    let mut names = Vec::new();
    let dir = std::fs::read_dir("/proc/self/fd").expect("fd");
    for entry in dir.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            names.push(target.to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}

#[test]
fn redirect_response_returns_302_with_location_empty_body_and_binds_no_port() {
    let before_sockets = socket_fds(&fd_targets());
    let before_listen = listening_ports(&before_sockets);

    let response = redirect_response("/console");
    let raw = response.to_string();
    let (status, rest) = raw.split_once("\r\n").expect("status line");
    assert_eq!(status, "HTTP/1.1 302 Found");
    assert!(
        rest.split("\r\n").any(|line| match line.split_once(':') {
            Some(("Location", value)) => !value.trim().is_empty(),
            _ => false,
        }),
        "Location header missing"
    );
    let (_, body) = raw.split_once("\r\n\r\n").expect("header end");
    assert!(body.is_empty());
    assert!(response.body().is_empty());

    let home = redirect_response("");
    let home_raw = home.to_string();
    assert!(home_raw.starts_with("HTTP/1.1 302 Found\r\n"));
    assert!(home_raw.contains("\r\nLocation: /\r\n"));
    assert!(home.body().is_empty());
    let (_, home_body) = home_raw.split_once("\r\n\r\n").expect("header end");
    assert!(home_body.is_empty());

    let after_sockets = socket_fds(&fd_targets());
    let after_listen = listening_ports(&after_sockets);
    assert_eq!(before_sockets, after_sockets, "socket table changed");
    assert_eq!(
        before_listen, after_listen,
        "redirect_response bound a port"
    );
}

fn listening_ports(socket_targets: &[String]) -> Vec<String> {
    let mut inodes = Vec::new();
    for target in socket_targets {
        if let Some(inode) = target
            .strip_prefix("socket:[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if inode != "0" {
                inodes.push(inode.to_owned());
            }
        }
    }
    let mut ports = Vec::new();
    for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 10 || cols[3] != "0A" {
                continue;
            }
            if inodes.iter().any(|inode| inode == cols[9]) {
                ports.push(cols[1].to_owned());
            }
        }
    }
    ports.sort();
    ports
}
