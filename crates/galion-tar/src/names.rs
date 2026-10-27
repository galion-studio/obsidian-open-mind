// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! uid/gid -> user/group name lookups for the GNU writer's uname/gname.

use std::path::{Path, PathBuf};

/// Name source. An unknown id returns an empty name (GNU tar stores an
/// empty uname/gname when `getpwuid`/`getgrgid` fail).
pub trait Names {
    fn user(&mut self, uid: u32) -> Vec<u8>;
    fn group(&mut self, gid: u32) -> Vec<u8>;
}

/// No lookups: every uname/gname is empty.
pub struct NoNames;

impl Names for NoNames {
    fn user(&mut self, _: u32) -> Vec<u8> {
        Vec::new()
    }
    fn group(&mut self, _: u32) -> Vec<u8> {
        Vec::new()
    }
}

/// Pure-std lookups in `<root>/etc/passwd` and `<root>/etc/group` (first
/// matching line wins, like the NSS `files` backend). Equals
/// `getpwuid`/`getgrgid` on hosts whose nsswitch uses only `files`, which is
/// the VPS and the box; callers that must honour other NSS sources plug their
/// own [`Names`].
pub struct EtcNames {
    root: PathBuf,
}

impl Default for EtcNames {
    fn default() -> Self {
        EtcNames::at("/")
    }
}

impl EtcNames {
    pub fn at(root: impl AsRef<Path>) -> Self {
        EtcNames {
            root: root.as_ref().to_path_buf(),
        }
    }

    fn find(&self, file: &str, id: u32) -> Vec<u8> {
        let Ok(data) = std::fs::read(self.root.join("etc").join(file)) else {
            return Vec::new();
        };
        let want = id.to_string();
        for line in data.split(|&b| b == b'\n') {
            if line.starts_with(b"#") || line.starts_with(b"+") || line.starts_with(b"-") {
                continue;
            }
            let mut f = line.split(|&b| b == b':');
            let (Some(name), Some(_pw), Some(n)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            if !name.is_empty() && n == want.as_bytes() {
                return name.to_vec();
            }
        }
        Vec::new()
    }
}

impl Names for EtcNames {
    fn user(&mut self, uid: u32) -> Vec<u8> {
        self.find("passwd", uid)
    }
    fn group(&mut self, gid: u32) -> Vec<u8> {
        self.find("group", gid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etc_files_first_match_and_misses() {
        let d = std::env::temp_dir().join(format!("galion-tar-names-{}", std::process::id()));
        std::fs::create_dir_all(d.join("etc")).unwrap();
        std::fs::write(
            d.join("etc/passwd"),
            "# c\nroot:x:0:0:root:/root:/bin/sh\nbad\ngalion:x:1000:1000::/h:/bin/sh\ndup:x:1000:1:::\n",
        )
        .unwrap();
        std::fs::write(d.join("etc/group"), "root:x:0:\nwheel:x:10:galion\n").unwrap();
        let mut n = EtcNames::at(&d);
        assert_eq!(n.user(0), b"root");
        assert_eq!(n.user(1000), b"galion");
        assert_eq!(n.user(7), b"");
        assert_eq!(n.group(10), b"wheel");
        assert_eq!(n.group(1000), b"");
        assert_eq!(EtcNames::at(d.join("nope")).user(0), b"");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
