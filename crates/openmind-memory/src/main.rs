// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! Spare-port memory v0.
//!
//! Without `--serve` this process does not open a socket. With `--serve` it
//! binds `127.0.0.1` only and refuses port 8420. It stops when stdin closes
//! or a line is `stop`. It does not read the live gateway env or any key file.

use openmind_memory::{Index, Server};
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn main() {
    let code = match run(std::env::args().skip(1)) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("openmind-memory: {err}");
            2
        }
    };
    std::process::exit(code);
}

fn run<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut serve = false;
    let mut addr = "127.0.0.1:0".to_string();
    let mut path: Option<PathBuf> = None;
    let mut fixture: Option<PathBuf> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            "--serve" => serve = true,
            "--addr" => {
                addr = args
                    .next()
                    .ok_or_else(|| "--addr needs 127.0.0.1:port".to_string())?;
            }
            "--path" => {
                path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--path needs a file".to_string())?,
                ));
            }
            "--fixture" => {
                fixture = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--fixture needs a JSON file".to_string())?,
                ));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if !serve && fixture.is_none() && path.is_none() {
        print_usage();
        return Ok(());
    }
    let mut index = match &path {
        Some(file) => Index::open_path(file).map_err(|err| err.to_string())?,
        None => Index::open_memory().map_err(|err| err.to_string())?,
    };
    if let Some(file) = &fixture {
        let n = index
            .load_fixture_path(file)
            .map_err(|err| err.to_string())?;
        println!("loaded {n} fixture rows");
    }
    if !serve {
        return Ok(());
    }
    let server = Server::bind(&addr, Arc::new(Mutex::new(index))).map_err(|err| err.to_string())?;
    println!("listening {}", server.addr());
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim() == "stop" {
            break;
        }
    }
    drop(server);
    println!("stopped");
    Ok(())
}

fn print_usage() {
    println!(
        "openmind-memory: fixture-backed store and search.\n\
         \n\
         --serve                 listen until stdin closes or a line is stop\n\
         --addr 127.0.0.1:0      loopback only; port 8420 is refused\n\
         --path PATH             sqlite file (default: in-memory)\n\
         --fixture PATH          JSON array of memory and conversation rows\n\
         \n\
         Without --serve no socket is opened."
    );
}
