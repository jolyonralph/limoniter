//! The ROMs the built-in ZX81 and Jupiter Ace need, which Limoniter can't give away: the person chooses each file
//! once, it's checked against the ROMs known by their CRC-32 (as MAME lists them), and where it is is kept in
//! `micro-roms.txt` beside the settings.
//!
//! Why they aren't built in: Amstrad's permission for emulators to carry Sinclair ROMs (Cliff Lawson,
//! comp.sys.sinclair, 31 August 1999) covers only the Spectrums, and says so of the ZX80 and ZX81; the ZX81's ROM
//! belongs to Nine Tiles, who wrote it, and has been licensed to emulators only one by one. Nobody has said who holds
//! the Jupiter Ace's since Jupiter Cantab closed. See NOTES.md for the sources.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// A machine that needs a ROM chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    Zx81,
    Ace,
}

impl Needs {
    fn id(self) -> &'static str {
        match self {
            Needs::Zx81 => "zx81",
            Needs::Ace => "ace",
        }
    }

    /// The machine's name.
    pub fn machine(self) -> &'static str {
        match self {
            Needs::Zx81 => "ZX81",
            Needs::Ace => "Jupiter Ace",
        }
    }

    /// What to say to someone who hasn't got the ROM: what it is, why it isn't here, and where it's found.
    pub fn where_to_get(self) -> String {
        match self {
            Needs::Zx81 => crate::i18n::tr("The built-in ZX81 needs the ZX81's own 8K ROM, which belongs to Nine Tiles, who wrote it, so it can't come with Limoniter. ZX81 emulators come with it (ZEsarUX, for one, as zx81.rom), or it can be read from a ZX81's ROM chip. Choose the file once, and it's kept.").into(),
            Needs::Ace => crate::i18n::tr("The built-in Jupiter Ace needs the Ace's own 8K ROM, and nobody is known to hold its rights since Jupiter Cantab closed, so it can't come with Limoniter. Jupiter Ace emulators come with it (ZEsarUX, for one, as ace.rom), or it can be read from an Ace's two ROM chips, put one after the other. Choose the file once, and it's kept.").into(),
        }
    }
}

/// The ROMs known, by the CRC-32 of their 8K and what each is.
const KNOWN_ZX81: &[(u32, &str)] = &[(0xFCBB_D617, "ZX81 ROM, first edition"), (0x4B1D_D6EB, "ZX81 ROM, second edition"), (0x522C_37B8, "ZX81 ROM, third edition")];
/// The Ace's ROM is two 4K chips; a file of both is checked half by half.
const ACE_HALVES: (u32, u32) = (0xDC84_38A5, 0x4009_F636);

/// What a ROM file is, if it's right for the machine: its name; or why it isn't.
pub fn identify(needs: Needs, bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.len() != 8192 {
        return Err(crate::i18n::tf!("That file is {} bytes; the {} ROM is 8,192.", bytes.len(), needs.machine()));
    }
    let crc = crate::z80_micro::crc32(bytes);
    match needs {
        Needs::Zx81 => KNOWN_ZX81.iter().find(|(c, _)| *c == crc).map(|(_, name)| *name).ok_or_else(|| {
            if bytes.starts_with(&[0xD3, 0xFD]) {
                crate::i18n::tf!("That looks like a ZX81 ROM, but not one Limoniter knows (its CRC-32 is {crc:08X}), so it may be changed or damaged.")
            } else {
                crate::i18n::tr("That isn't a ZX81 ROM.").to_string()
            }
        }),
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
    pub zx81: Option<PathBuf>,
    pub ace: Option<PathBuf>,
}

impl Chosen {
    fn path(&self, needs: Needs) -> Option<&Path> {
        match needs {
            Needs::Zx81 => self.zx81.as_deref(),
            Needs::Ace => self.ace.as_deref(),
        }
    }

    fn to_text(&self) -> String {
        [Needs::Zx81, Needs::Ace].into_iter().filter_map(|needs| self.path(needs).map(|p| format!("{}={}\n", needs.id(), p.display()))).collect()
    }

    fn from_text(text: &str) -> Chosen {
        let mut chosen = Chosen::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("zx81", path)) if !path.trim().is_empty() => chosen.zx81 = Some(PathBuf::from(path.trim())),
                Some(("ace", path)) if !path.trim().is_empty() => chosen.ace = Some(PathBuf::from(path.trim())),
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
        Needs::Zx81 => now.zx81 = path,
        Needs::Ace => now.ace = path,
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
        Needs::Zx81 => "LIMONITER_ZX81_ROM",
        Needs::Ace => "LIMONITER_ACE_ROM",
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
    }

    #[test]
    fn where_they_are_is_kept_as_text() {
        let chosen = Chosen { zx81: Some(PathBuf::from("/roms/zx81.rom")), ace: None };
        assert_eq!(chosen.to_text(), "zx81=/roms/zx81.rom\n");
        assert_eq!(Chosen::from_text(&chosen.to_text()), chosen);
        assert_eq!(Chosen::from_text("ace=\nnonsense\n"), Chosen::default());
    }
}
