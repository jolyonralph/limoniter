//! The ROMs the built-in ZX80, ZX81, Jupiter Ace and Vectrex need, which Limoniter can't give away: the person chooses each file
//! once, it's checked against the ROMs known by their CRC-32 (as MAME lists them), and where it is is kept in
//! `micro-roms.txt` beside the settings.
//!
//! Why they aren't built in: Amstrad's permission for emulators to carry Sinclair ROMs (Cliff Lawson,
//! comp.sys.sinclair, 31 August 1999) covers only the Spectrums, and says so of the ZX80 and ZX81; the ZX81's ROM
//! belongs to Nine Tiles, who wrote it, and has been licensed to emulators only one by one. Nobody has said who holds
//! the Jupiter Ace's since Jupiter Cantab closed. The Vectrex's BIOS (its "executive" ROM, with Mine Storm) was
//! General Consumer Electronics' and then Milton Bradley's, whose rights passed to Hasbro; it has been said to be free
//! to share for non-commercial use, but nothing from Hasbro saying so has been found, so it's asked for like the
//! others. See NOTES.md for the sources.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// A machine that needs a ROM chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    Zx80,
    Zx81,
    Ace,
    Vectrex,
}

impl Needs {
    /// The endings a ROM file for the machine comes with, for the file picker. Vectrex dumps are often `.vec`, as
    /// its cartridges are.
    pub fn file_endings(self) -> &'static [&'static str] {
        match self {
            Needs::Vectrex => &["rom", "bin", "vec"],
            _ => &["rom", "bin"],
        }
    }

    fn id(self) -> &'static str {
        match self {
            Needs::Zx80 => "zx80",
            Needs::Zx81 => "zx81",
            Needs::Ace => "ace",
            Needs::Vectrex => "vectrex",
        }
    }


    /// The machine's name.
    pub fn machine(self) -> &'static str {
        match self {
            Needs::Zx80 => "ZX80",
            Needs::Zx81 => "ZX81",
            Needs::Ace => "Jupiter Ace",
            Needs::Vectrex => "Vectrex",
        }
    }

    /// What to say to someone who hasn't got the ROM: what it is, why it isn't here, and where it's found.
    pub fn where_to_get(self) -> String {
        match self {
            Needs::Zx80 => crate::i18n::tr("The built-in ZX80 needs the ZX80's own 4K ROM, which belongs to Amstrad, whose permission for emulators covers only the Spectrums' ROMs, so it can't come with Limoniter. ZX80 emulators come with it (ZEsarUX, for one, as zx80.rom), or it can be read from a ZX80's ROM chip. Choose the file once, and it's kept.").into(),
            Needs::Zx81 => crate::i18n::tr("The built-in ZX81 needs the ZX81's own 8K ROM, which belongs to Nine Tiles, who wrote it, so it can't come with Limoniter. ZX81 emulators come with it (ZEsarUX, for one, as zx81.rom), or it can be read from a ZX81's ROM chip. Choose the file once, and it's kept.").into(),
            Needs::Ace => crate::i18n::tr("The built-in Jupiter Ace needs the Ace's own 8K ROM, and nobody is known to hold its rights since Jupiter Cantab closed, so it can't come with Limoniter. Jupiter Ace emulators come with it (ZEsarUX, for one, as ace.rom), or it can be read from an Ace's two ROM chips, put one after the other. Choose the file once, and it's kept.").into(),
            Needs::Vectrex => crate::i18n::tr("The built-in Vectrex needs the Vectrex's own 8K BIOS (its executive ROM, with Mine Storm), which belongs to Hasbro since Milton Bradley, so it can't come with Limoniter. Vectrex emulators are given it (MAME's vectrex set has it as exec_rom.bin), or it can be read from a Vectrex's ROM chip. Choose the file once, and it's kept. A cartridge that drives the hardware itself, calling nothing in the BIOS, runs without it.").into(),
        }
    }
}

/// The ZX80's ROM known, by its CRC-32 (as MAME lists it): the only one Sinclair made, before the ZX81's 8K ROM was
/// offered for the ZX80 too (which is the ZX81's, so isn't a ZX80 ROM here).
const KNOWN_ZX80: &[(u32, &str)] = &[(0x4C7F_C597, "ZX80 ROM")];
/// The ROMs known, by the CRC-32 of their 8K and what each is.
const KNOWN_ZX81: &[(u32, &str)] = &[(0xFCBB_D617, "ZX81 ROM, first edition"), (0x4B1D_D6EB, "ZX81 ROM, second edition"), (0x522C_37B8, "ZX81 ROM, third edition")];
/// The Ace's ROM is two 4K chips; a file of both is checked half by half.
const ACE_HALVES: (u32, u32) = (0xDC84_38A5, 0x4009_F636);
/// The Vectrex's BIOS, by its CRC-32, as MAME lists it (exec_rom.bin, the same in the Spectrum I+ set). Milton
/// Bradley's other editions differ a little, so any 8K whose reset vector is $F000, where the BIOS starts, is taken.
const KNOWN_VECTREX: &[(u32, &str)] = &[(0xBA13_FB57, "Vectrex BIOS")];

/// What a ROM file is, if it's right for the machine: its name; or why it isn't.
pub fn identify(needs: Needs, bytes: &[u8]) -> Result<&'static str, String> {
    if needs == Needs::Vectrex {
        if bytes.len() != 8192 {
            return Err(crate::i18n::tf!("That file is {} bytes; the Vectrex BIOS is 8,192.", bytes.len()));
        }
        let crc = crate::z80_micro::crc32(bytes);
        if let Some((_, name)) = KNOWN_VECTREX.iter().find(|(c, _)| *c == crc) {
            return Ok(name);
        }
        // The 6809's reset vector, in the last two bytes.
        return if bytes[0x1FFE..] == [0xF0, 0x00] { Ok("Vectrex BIOS, another edition") } else { Err(crate::i18n::tr("That isn't a Vectrex BIOS.").into()) };
    }
    if needs == Needs::Zx80 && bytes.len() != 4096 {
        return Err(crate::i18n::tf!("That file is {} bytes; the ZX80 ROM is 4,096.", bytes.len()));
    }
    if needs != Needs::Zx80 && bytes.len() != 8192 {
        return Err(crate::i18n::tf!("That file is {} bytes; the {} ROM is 8,192.", bytes.len(), needs.machine()));
    }
    let crc = crate::z80_micro::crc32(bytes);
    match needs {
        // It starts by filling memory from &7FFF down: LD HL,$7FFF and LD A,$3F.
        Needs::Zx80 => KNOWN_ZX80.iter().find(|(c, _)| *c == crc).map(|(_, name)| *name).ok_or_else(|| {
            if bytes.starts_with(&[0x21, 0xFF, 0x7F, 0x3E, 0x3F]) {
                crate::i18n::tf!("That looks like a ZX80 ROM, but not one Limoniter knows (its CRC-32 is {crc:08X}), so it may be changed or damaged.")
            } else {
                crate::i18n::tr("That isn't a ZX80 ROM.").to_string()
            }
        }),
        Needs::Zx81 => KNOWN_ZX81.iter().find(|(c, _)| *c == crc).map(|(_, name)| *name).ok_or_else(|| {
            if bytes.starts_with(&[0xD3, 0xFD]) {
                crate::i18n::tf!("That looks like a ZX81 ROM, but not one Limoniter knows (its CRC-32 is {crc:08X}), so it may be changed or damaged.")
            } else {
                crate::i18n::tr("That isn't a ZX81 ROM.").to_string()
            }
        }),
        Needs::Vectrex => unreachable!("told above"),
        Needs::Ace => {
            if (crate::z80_micro::crc32(&bytes[..4096]), crate::z80_micro::crc32(&bytes[4096..])) == ACE_HALVES {
                Ok("Jupiter Ace ROM")
            } else {
                Err(crate::i18n::tf!("That isn't the Jupiter Ace ROM Limoniter knows (its CRC-32 is {crc:08X})."))
            }
        }
    }
}

/// Where each ROM was chosen from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chosen {
    pub zx80: Option<PathBuf>,
    pub zx81: Option<PathBuf>,
    pub ace: Option<PathBuf>,
    pub vectrex: Option<PathBuf>,
}

impl Chosen {
    fn path(&self, needs: Needs) -> Option<&Path> {
        match needs {
            Needs::Zx80 => self.zx80.as_deref(),
            Needs::Zx81 => self.zx81.as_deref(),
            Needs::Ace => self.ace.as_deref(),
            Needs::Vectrex => self.vectrex.as_deref(),
        }
    }

    fn to_text(&self) -> String {
        [Needs::Zx80, Needs::Zx81, Needs::Ace, Needs::Vectrex].into_iter().filter_map(|needs| self.path(needs).map(|p| format!("{}={}\n", needs.id(), p.display()))).collect()
    }

    fn from_text(text: &str) -> Chosen {
        let mut chosen = Chosen::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("zx80", path)) if !path.trim().is_empty() => chosen.zx80 = Some(PathBuf::from(path.trim())),
                Some(("zx81", path)) if !path.trim().is_empty() => chosen.zx81 = Some(PathBuf::from(path.trim())),
                Some(("ace", path)) if !path.trim().is_empty() => chosen.ace = Some(PathBuf::from(path.trim())),
                Some(("vectrex", path)) if !path.trim().is_empty() => chosen.vectrex = Some(PathBuf::from(path.trim())),
                _ => {}
            }
        }
        chosen
    }
}

fn settings_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("micro-roms.txt"))
}

static CHOSEN: RwLock<Option<Chosen>> = RwLock::new(None);

/// The ROMs chosen, read the first time they're asked for (never from the file in tests).
pub fn chosen() -> Chosen {
    if let Some(known) = CHOSEN.read().ok().and_then(|c| c.clone()) {
        return known;
    }
    let read = if cfg!(test) { None } else { settings_file().and_then(|f| std::fs::read_to_string(f).ok()) };
    let chosen = read.map(|t| Chosen::from_text(&t)).unwrap_or_default();
    if let Ok(mut kept) = CHOSEN.write() {
        *kept = Some(chosen.clone());
    }
    chosen
}

/// Keeps a ROM's place, and saves it.
pub fn choose(needs: Needs, path: Option<PathBuf>) {
    let mut now = chosen();
    match needs {
        Needs::Zx80 => now.zx80 = path,
        Needs::Zx81 => now.zx81 = path,
        Needs::Ace => now.ace = path,
        Needs::Vectrex => now.vectrex = path,
    }
    if !cfg!(test)
        && let Some(file) = settings_file()
    {
        let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
        let _ = std::fs::write(&file, now.to_text());
    }
    if let Ok(mut kept) = CHOSEN.write() {
        *kept = Some(now);
    }
}

/// The machine's ROM, read and checked; or what's wrong, when one was chosen and has gone or changed. None when none
/// has been chosen yet.
pub fn rom(needs: Needs) -> Option<Result<Vec<u8>, String>> {
    // For the tests and for pictures of it: a ROM named in the environment, never kept.
    if let Some(path) = std::env::var_os(match needs {
        Needs::Zx80 => "LIMONITER_ZX80_ROM",
        Needs::Zx81 => "LIMONITER_ZX81_ROM",
        Needs::Ace => "LIMONITER_ACE_ROM",
        Needs::Vectrex => "LIMONITER_VECTREX_ROM",
    }) {
        return Some(std::fs::read(path).map_err(|e| e.to_string()));
    }
    let chosen = chosen();
    let path = chosen.path(needs)?;
    Some(match std::fs::read(path) {
        Ok(bytes) => identify(needs, &bytes).map(|_| bytes),
        Err(e) => Err(crate::i18n::tf!("The {} ROM chosen before, {}, can't be read: {e}", needs.machine(), path.display())),
    })
}

/// Reads a file chosen as a machine's ROM, and keeps it if it's right.
pub fn choose_file(needs: Needs, path: &Path) -> Result<&'static str, String> {
    let bytes = std::fs::read(path).map_err(|e| crate::i18n::tf!("{} can't be read: {e}", path.display()))?;
    let name = identify(needs, &bytes)?;
    choose(needs, Some(path.to_path_buf()));
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roms_are_told_by_their_size_and_crc() {
        assert!(identify(Needs::Zx81, &[0; 100]).unwrap_err().contains("8,192"));
        assert_eq!(identify(Needs::Zx81, &[0; 8192]).unwrap_err(), "That isn't a ZX81 ROM.");
        let mut like = vec![0u8; 8192];
        like[..2].copy_from_slice(&[0xD3, 0xFD]);
        assert!(identify(Needs::Zx81, &like).unwrap_err().contains("not one Limoniter knows"));
        assert!(identify(Needs::Ace, &[0; 8192]).unwrap_err().contains("isn't the Jupiter Ace ROM"));
        // The ZX80's is half the size.
        assert!(identify(Needs::Zx80, &[0; 8192]).unwrap_err().contains("4,096"));
        assert_eq!(identify(Needs::Zx80, &[0; 4096]).unwrap_err(), "That isn't a ZX80 ROM.");
        let mut like = vec![0u8; 4096];
        like[..5].copy_from_slice(&[0x21, 0xFF, 0x7F, 0x3E, 0x3F]);
        assert!(identify(Needs::Zx80, &like).unwrap_err().contains("not one Limoniter knows"));
        // The Vectrex's BIOS: 8K, starting at $F000.
        assert!(identify(Needs::Vectrex, &[0; 4096]).unwrap_err().contains("8,192"));
        assert_eq!(identify(Needs::Vectrex, &[0; 8192]).unwrap_err(), "That isn't a Vectrex BIOS.");
        assert!(Needs::Vectrex.file_endings().contains(&"vec"), "Vectrex BIOS dumps are often .vec files");
        assert!(!Needs::Zx81.file_endings().contains(&"vec"));
        let mut like = vec![0u8; 8192];
        like[0x1FFE..].copy_from_slice(&[0xF0, 0x00]);
        assert_eq!(identify(Needs::Vectrex, &like), Ok("Vectrex BIOS, another edition"));
        assert!(Needs::Vectrex.where_to_get().contains("exec_rom.bin"));
    }

    #[test]
    fn where_they_are_is_kept_as_text() {
        let chosen = Chosen { zx81: Some(PathBuf::from("/roms/zx81.rom")), ..Chosen::default() };
        assert_eq!(chosen.to_text(), "zx81=/roms/zx81.rom\n");
        let both = Chosen { zx80: Some(PathBuf::from("/roms/zx80.rom")), ..chosen.clone() };
        assert_eq!(both.to_text(), "zx80=/roms/zx80.rom\nzx81=/roms/zx81.rom\n");
        assert_eq!(Chosen::from_text(&both.to_text()), both);
        assert_eq!(Chosen::from_text(&chosen.to_text()), chosen);
        assert_eq!(Chosen::from_text("ace=\nnonsense\n"), Chosen::default());
        let vectrex = Chosen { vectrex: Some(PathBuf::from("/roms/exec_rom.bin")), ..Chosen::default() };
        assert_eq!(vectrex.to_text(), "vectrex=/roms/exec_rom.bin\n");
        assert_eq!(Chosen::from_text(&vectrex.to_text()), vectrex);
    }
}
