// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Minimal ELF reader for the release checks that `scripts/ucpu-gate.sh`
//! does with readelf: machine, `PT_INTERP`, `DT_NEEDED`, `.symtab`.
//! Little-endian ELF32 and ELF64 only (every tier-1/2 target). Every offset
//! is bounds-checked; a malformed file is an error, never a panic.

/// `e_machine` values for the targets we ship.
pub const EM_ARM: u16 = 40;
pub const EM_X86_64: u16 = 62;
pub const EM_AARCH64: u16 = 183;

const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_NEEDED: u64 = 1;
const SHT_SYMTAB: u32 = 2;

/// What the release gate needs to know about one binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfInfo {
    pub class64: bool,
    /// `ET_EXEC` (2) or `ET_DYN` (3, static-pie).
    pub e_type: u16,
    pub machine: u16,
    pub has_interp: bool,
    pub needed: usize,
    pub has_symtab: bool,
}

impl ElfInfo {
    /// Static = no program interpreter and no shared-library dependency.
    pub fn is_static(&self) -> bool {
        !self.has_interp && self.needed == 0
    }
}

/// True when `b` starts with the ELF magic.
pub fn is_elf(b: &[u8]) -> bool {
    b.len() >= 4 && &b[..4] == b"\x7fELF"
}

/// Rust target triple -> expected `e_machine`.
pub fn machine_for_target(target: &str) -> Option<u16> {
    let arch = target.split('-').next().unwrap_or("");
    match arch {
        "x86_64" => Some(EM_X86_64),
        "aarch64" => Some(EM_AARCH64),
        a if a.starts_with("armv7") || a == "arm" => Some(EM_ARM),
        _ => None,
    }
}

/// Human name of an `e_machine` value (readelf's wording).
pub fn machine_name(m: u16) -> &'static str {
    match m {
        EM_X86_64 => "Advanced Micro Devices X86-64",
        EM_AARCH64 => "AArch64",
        EM_ARM => "ARM",
        _ => "other",
    }
}

struct R<'a> {
    b: &'a [u8],
    c64: bool,
}

impl R<'_> {
    fn u16(&self, o: u64) -> Result<u16, String> {
        let o = usize::try_from(o).map_err(|_| "offset overflow")?;
        let s = self
            .b
            .get(o..o.saturating_add(2))
            .ok_or("truncated ELF (u16)")?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&self, o: u64) -> Result<u32, String> {
        let o = usize::try_from(o).map_err(|_| "offset overflow")?;
        let s = self
            .b
            .get(o..o.saturating_add(4))
            .ok_or("truncated ELF (u32)")?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&self, o: u64) -> Result<u64, String> {
        let o = usize::try_from(o).map_err(|_| "offset overflow")?;
        let s = self
            .b
            .get(o..o.saturating_add(8))
            .ok_or("truncated ELF (u64)")?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    /// Native word: u64 on ELF64, u32 on ELF32.
    fn word(&self, o: u64) -> Result<u64, String> {
        if self.c64 {
            self.u64(o)
        } else {
            self.u32(o).map(u64::from)
        }
    }
}

/// Parse the headers of an ELF file held in memory.
pub fn inspect(b: &[u8]) -> Result<ElfInfo, String> {
    if !is_elf(b) {
        return Err("not an ELF file".into());
    }
    let class = *b.get(4).ok_or("truncated ELF")?;
    let data = *b.get(5).ok_or("truncated ELF")?;
    if data != 1 {
        return Err("big-endian ELF is not supported".into());
    }
    let c64 = match class {
        1 => false,
        2 => true,
        _ => return Err(format!("bad ELF class {class}")),
    };
    let r = R { b, c64 };
    let e_type = r.u16(16)?;
    let machine = r.u16(18)?;
    let (phoff, shoff, phentsize, phnum, shentsize, shnum) = if c64 {
        (
            r.u64(32)?,
            r.u64(40)?,
            r.u16(54)?,
            r.u16(56)?,
            r.u16(58)?,
            r.u16(60)?,
        )
    } else {
        (
            u64::from(r.u32(28)?),
            u64::from(r.u32(32)?),
            r.u16(42)?,
            r.u16(44)?,
            r.u16(46)?,
            r.u16(48)?,
        )
    };
    let mut has_interp = false;
    let mut needed = 0usize;
    for i in 0..u64::from(phnum) {
        let ph = phoff.saturating_add(i.saturating_mul(u64::from(phentsize)));
        let p_type = r.u32(ph)?;
        // p_offset / p_filesz positions differ between ELF32 and ELF64.
        let (off, filesz) = if c64 {
            (r.u64(ph.saturating_add(8))?, r.u64(ph.saturating_add(32))?)
        } else {
            (
                u64::from(r.u32(ph.saturating_add(4))?),
                u64::from(r.u32(ph.saturating_add(16))?),
            )
        };
        match p_type {
            PT_INTERP => has_interp = true,
            PT_DYNAMIC => {
                let ent = if c64 { 16 } else { 8 };
                let n = filesz / ent;
                for k in 0..n {
                    let d = off.saturating_add(k.saturating_mul(ent));
                    let tag = r.word(d)?;
                    if tag == 0 {
                        break; // DT_NULL
                    }
                    if tag == DT_NEEDED {
                        needed += 1;
                    }
                }
            }
            _ => {}
        }
    }
    let mut has_symtab = false;
    for i in 0..u64::from(shnum) {
        let sh = shoff.saturating_add(i.saturating_mul(u64::from(shentsize)));
        if r.u32(sh.saturating_add(4))? == SHT_SYMTAB {
            has_symtab = true;
        }
    }
    Ok(ElfInfo {
        class64: c64,
        e_type,
        machine,
        has_interp,
        needed,
        has_symtab,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny synthetic ELF64: header + 2 program headers (+ dynamic table).
    fn synth(machine: u16, interp: bool, needed: usize, symtab: bool) -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        b[6] = 1;
        b[16..18].copy_from_slice(&3u16.to_le_bytes());
        b[18..20].copy_from_slice(&machine.to_le_bytes());
        let phoff = 64u64;
        let phnum = 2u16;
        b[32..40].copy_from_slice(&phoff.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[56..58].copy_from_slice(&phnum.to_le_bytes());
        let dyn_off = 64 + 56 * 2;
        let dyn_len = 16 * (needed as u64 + 2);
        let mut ph = |t: u32, off: u64, sz: u64| {
            let mut p = vec![0u8; 56];
            p[..4].copy_from_slice(&t.to_le_bytes());
            p[8..16].copy_from_slice(&off.to_le_bytes());
            p[32..40].copy_from_slice(&sz.to_le_bytes());
            b.extend(p);
        };
        ph(if interp { PT_INTERP } else { 1 }, 0, 0);
        ph(PT_DYNAMIC, dyn_off, dyn_len);
        for _ in 0..needed {
            b.extend(1u64.to_le_bytes());
            b.extend(7u64.to_le_bytes());
        }
        b.extend(30u64.to_le_bytes()); // DT_FLAGS: not NEEDED
        b.extend(8u64.to_le_bytes());
        b.extend([0u8; 16]); // DT_NULL
        let shoff = b.len() as u64;
        b[40..48].copy_from_slice(&shoff.to_le_bytes());
        b[58..60].copy_from_slice(&64u16.to_le_bytes());
        b[60..62].copy_from_slice(&2u16.to_le_bytes());
        for t in [0u32, if symtab { SHT_SYMTAB } else { 3 }] {
            let mut s = vec![0u8; 64];
            s[4..8].copy_from_slice(&t.to_le_bytes());
            b.extend(s);
        }
        b
    }

    #[test]
    fn static_stripped() {
        let i = inspect(&synth(EM_AARCH64, false, 0, false)).unwrap();
        assert!(i.is_static() && !i.has_symtab && i.class64);
        assert_eq!(i.machine, EM_AARCH64);
        assert_eq!(i.e_type, 3);
    }

    #[test]
    fn dynamic_and_symtab_detected() {
        let i = inspect(&synth(EM_X86_64, true, 3, true)).unwrap();
        assert!(i.has_interp);
        assert_eq!(i.needed, 3);
        assert!(i.has_symtab && !i.is_static());
    }

    #[test]
    fn truncated_is_error_not_panic() {
        let full = synth(EM_X86_64, false, 1, false);
        for cut in [3, 5, 17, 40, 63, 70, 150, full.len() - 1] {
            let _ = inspect(&full[..cut]);
        }
        assert!(inspect(b"#!/bin/sh\n").is_err());
        assert!(inspect(&full[..40]).is_err());
    }

    #[test]
    fn host_test_binary_is_dynamic_gnu() {
        // This test binary is built for the host (x86_64-unknown-linux-gnu on
        // the box and in CI): a real negative control for the static check.
        let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let i = inspect(&me).unwrap();
        if cfg!(all(target_env = "gnu", target_os = "linux")) {
            assert!(!i.is_static(), "{i:?}");
        }
        if cfg!(target_arch = "x86_64") {
            assert_eq!(i.machine, EM_X86_64);
        }
    }

    #[test]
    fn targets() {
        assert_eq!(
            machine_for_target("x86_64-unknown-linux-musl"),
            Some(EM_X86_64)
        );
        assert_eq!(
            machine_for_target("aarch64-unknown-linux-musl"),
            Some(EM_AARCH64)
        );
        assert_eq!(
            machine_for_target("armv7-unknown-linux-musleabihf"),
            Some(EM_ARM)
        );
        assert_eq!(machine_for_target("riscv64gc-unknown-linux-gnu"), None);
        assert_eq!(machine_name(EM_AARCH64), "AArch64");
    }
}
