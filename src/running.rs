//! What runs where: the machines programs run on, the files they load, and working out which machine a file is
//! for. Nothing here starts anything; see RUNNING.md for how the pieces fit.

use crate::basic;
use std::sync::Arc;

/// A machine a program runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Machine {
    Spectrum48,
    Spectrum128,
    SpectrumPlus3,
    Pentagon,
    Next,
    C64,
    C128,
    Vic20,
    Plus4,
    Pet,
    Bbc,
    Msx,
    Cpc,
    AtariSt,
    /// The Tandy Color Computer (and the Dragon, which runs the same machine code).
    CoCo,
    /// A 48K Spectrum with the OpticOS ROM in place of Sinclair's, for OpticBASIC programs.
    OpticOs,
    /// An Amiga: which model is chosen with its Kickstart (see `amiga_run`).
    Amiga,
    /// The Atari 2600 (VCS), whose programs are its cartridges.
    Atari2600,
    /// An IBM PC or compatible: DOS .COM programs, and floppies that boot.
    Pc,
    /// The Dragon 32 and 64, whose Dragon DOS binaries XRoar runs.
    Dragon32,
    Dragon64,
    /// The Vectrex, whose programs are its cartridges.
    Vectrex,
    /// Sega's Mega Drive (the Genesis in America), its 68000 with a Z80 for sound.
    MegaDrive,
    /// Sega's Master System (and the Mark III), and the Game Gear: a Z80 and Sega's own video chip.
    MasterSystem,
    GameGear,
    /// Sega's SG-1000 and the ColecoVision: a Z80, with TI's TMS9918A for video, as the MSX has.
    Sg1000,
    ColecoVision,
    /// The fantasy consoles: PICO-8 and TIC-80, whose carts are kept as text.
    Pico8,
    Tic80,
    /// RGC BASIC's desktop interpreter.
    RgcBasic,
    /// The Game Boy (and the Game Boy Color, which runs its cartridges too).
    GameBoy,
    /// The Nintendo Entertainment System (the Famicom).
    Nes,
    /// The Sinclair ZX80 and ZX81, and the Jupiter Ace, which Limoniter has built in (with the person's own ROMs).
    Zx80,
    Zx81,
    JupiterAce,
    /// The machines built in for teaching: the Manchester Baby (1948), EDSAC (1949) and the PDP-8 (1965), whose
    /// programs are their source, assembled (or read in by EDSAC's Initial Orders) when they're run.
    Ssem,
    Edsac,
    Pdp8,
    /// The Sinclair QL: a 68008 and QDOS, whose programs are executables EXEC starts as jobs.
    Ql,
    /// The Apple II (and the IIe), whose programs come on floppies that boot.
    Apple2,
    /// Atari's 8-bit computers: the 400, 800, XL and XE.
    Atari8,
    /// The Atari 7800, whose programs are its cartridges.
    Atari7800,
    /// Nintendo's Super NES (the Super Famicom): a 65816, whose programs are its cartridges.
    Snes,
    /// A CP/M 2.2 computer (an 8080 or Z80 with CP/M), whose programs are .COM files; RunCPM stands in for it.
    Cpm,
    /// The Agon Light: an eZ80 running MOS, with BBC BASIC.
    Agon,
    /// The Neo6502: a 65C02 with a Raspberry Pi Pico beside it for video, sound and files.
    Neo6502,
}

impl Machine {
    pub fn name(self) -> &'static str {
        match self {
            Machine::Spectrum48 => "ZX Spectrum 48K",
            Machine::Spectrum128 => "ZX Spectrum 128K",
            Machine::SpectrumPlus3 => "ZX Spectrum +3",
            Machine::Pentagon => "Pentagon (TR-DOS)",
            Machine::Next => "ZX Spectrum Next",
            Machine::C64 => "Commodore 64",
            Machine::C128 => "Commodore 128",
            Machine::Vic20 => "Commodore VIC-20",
            Machine::Plus4 => "Commodore 16 and Plus/4",
            Machine::Pet => "Commodore PET",
            Machine::Bbc => "BBC Micro",
            Machine::Msx => "MSX",
            Machine::Cpc => "Amstrad CPC",
            Machine::AtariSt => "Atari ST",
            Machine::CoCo => "Tandy Color Computer",
            Machine::OpticOs => "ZX Spectrum 48K with OpticOS",
            Machine::Amiga => "Amiga",
            Machine::Atari2600 => "Atari 2600",
            Machine::Pc => "IBM PC (DOS)",
            Machine::Dragon32 => "Dragon 32",
            Machine::Dragon64 => "Dragon 64",
            Machine::Vectrex => "Vectrex",
            Machine::MegaDrive => "Sega Mega Drive / Genesis",
            Machine::MasterSystem => "Sega Master System",
            Machine::GameGear => "Sega Game Gear",
            Machine::Sg1000 => "Sega SG-1000",
            Machine::ColecoVision => "ColecoVision",
            Machine::Pico8 => "PICO-8",
            Machine::Tic80 => "TIC-80",
            Machine::RgcBasic => "RGC BASIC",
            Machine::GameBoy => "Game Boy",
            Machine::Nes => "NES",
            Machine::Zx80 => "ZX80",
            Machine::Zx81 => "ZX81",
            Machine::JupiterAce => "Jupiter Ace",
            Machine::Ssem => "Manchester Baby (SSEM)",
            Machine::Edsac => "EDSAC",
            Machine::Pdp8 => "PDP-8",
            Machine::Ql => "Sinclair QL",
            Machine::Apple2 => "Apple II",
            Machine::Atari8 => "Atari 8-bit",
            Machine::Atari7800 => "Atari 7800",
            Machine::Snes => "SNES",
            Machine::Cpm => "CP/M",
            Machine::Agon => "Agon Light",
            Machine::Neo6502 => "Neo6502",
        }
    }

    /// Its name in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Machine::Spectrum48 => "spectrum48",
            Machine::Spectrum128 => "spectrum128",
            Machine::SpectrumPlus3 => "spectrum+3",
            Machine::Pentagon => "pentagon",
            Machine::Next => "next",
            Machine::C64 => "c64",
            Machine::C128 => "c128",
            Machine::Vic20 => "vic20",
            Machine::Plus4 => "plus4",
            Machine::Pet => "pet",
            Machine::Bbc => "bbc",
            Machine::Msx => "msx",
            Machine::Cpc => "cpc",
            Machine::AtariSt => "atarist",
            Machine::CoCo => "coco",
            Machine::OpticOs => "opticos",
            Machine::Amiga => "amiga",
            Machine::Atari2600 => "atari2600",
            Machine::Pc => "pc",
            Machine::Dragon32 => "dragon32",
            Machine::Dragon64 => "dragon64",
            Machine::Vectrex => "vectrex",
            Machine::MegaDrive => "megadrive",
            Machine::MasterSystem => "mastersystem",
            Machine::GameGear => "gamegear",
            Machine::Sg1000 => "sg1000",
            Machine::ColecoVision => "colecovision",
            Machine::Pico8 => "pico8",
            Machine::Tic80 => "tic80",
            Machine::RgcBasic => "rgcbasic",
            Machine::GameBoy => "gameboy",
            Machine::Nes => "nes",
            Machine::Zx80 => "zx80",
            Machine::Zx81 => "zx81",
            Machine::JupiterAce => "jupiterace",
            Machine::Ssem => "ssem",
            Machine::Edsac => "edsac",
            Machine::Pdp8 => "pdp8",
            Machine::Ql => "ql",
            Machine::Apple2 => "apple2",
            Machine::Atari8 => "atari8",
            Machine::Atari7800 => "atari7800",
            Machine::Snes => "snes",
            Machine::Cpm => "cpm",
            Machine::Agon => "agon",
            Machine::Neo6502 => "neo6502",
        }
    }

    /// A Spectrum of the kind the built-in machine, and any Spectrum emulator, runs.
    pub fn is_classic_spectrum(self) -> bool {
        matches!(self, Machine::Spectrum48 | Machine::Spectrum128 | Machine::SpectrumPlus3 | Machine::Pentagon)
    }
}

/// A kind of file a machine loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Tap,
    Tzx,
    Z80,
    Sna,
    /// A file with a +3DOS header: a +3 or Next BASIC program, or code.
    Plus3dos,
    Nex,
    Dsk,
    Trd,
    Scl,
    Prg,
    P00,
    T64,
    D64,
    D71,
    D81,
    Crt,
    Ssd,
    Dsd,
    Rom,
    Cas,
    /// An Atari ST program (.TOS, or .PRG on the ST itself).
    Tos,
    /// A Color Computer binary, as LOADM loads it (Disk Extended Color BASIC's .BIN).
    CocoBin,
    /// An Amiga executable (AmigaDOS's hunks), which has no ending of its own on the Amiga.
    Hunk,
    /// An Amiga floppy disk.
    Adf,
    /// An Atari ST floppy disk, sector by sector.
    St,
    /// An Atari ST floppy disk packed by the Magic Shadow Archiver.
    Msa,
    /// An Atari 2600 cartridge, its ROM as it is.
    A26,
    /// A DOS .COM program: its bytes, loaded at 100h.
    Com,
    /// A PC floppy disk, sector by sector, that boots.
    Img,
    /// A Vectrex cartridge, its ROM as it is.
    VecCart,
    /// A Mega Drive cartridge (.md, and .gen), its ROM as it is.
    MdRom,
    /// A Master System cartridge.
    SmsRom,
    /// A Game Gear cartridge.
    GgRom,
    /// An SG-1000 cartridge.
    SgRom,
    /// A ColecoVision cartridge.
    ColRom,
    /// A PICO-8 cart, as text.
    P8,
    /// A TIC-80 cart as it's saved, and as text: its code (Lua) with its data in comments at the end.
    Tic,
    TicText,
    /// An RGC BASIC program, as text.
    RgcBas,
    /// A Game Boy cartridge, its ROM as it is, header and all.
    Gb,
    /// The same, for the Game Boy Color.
    Gbc,
    /// An NES cartridge: the iNES (or NES 2.0) header, then its program's ROM and its tiles'.
    Nes,
    /// A ZX81 program: its memory from &4009, as SAVE writes it.
    P,
    /// A ZX80 program: its memory from &4000, as SAVE writes it.
    O,
    /// Forth source, typed into a Jupiter Ace.
    Forth,
    /// A Dragon DOS disk, kept as a VDK file (PC-Dragon's, which XRoar reads).
    Vdk,
    /// Source for the teaching machines: the Manchester Baby's, EDSAC's tape, and PAL for the PDP-8; and a PDP-8
    /// paper tape in RIM format.
    Ssem,
    EdsacTape,
    Pal,
    Rim,
    /// A QL program, kept with its QDOS header as QL emulators read it: the XTcc trailer after its bytes.
    QdosExe,
    /// A QL microdrive cartridge, in QLAY's format.
    Mdv,
    /// An Amstrad CPC tape: a TZX file under the CPC's own ending.
    Cdt,
    /// An Apple II floppy with its sectors in ProDOS's order (a .dsk, or .do, has them in DOS 3.3's), or a larger
    /// ProDOS disk.
    Po,
    /// An Apple II floppy as its magnetic flux is, bit by bit (WOZ 1 or 2), copy protection and all.
    Woz,
    /// An Atari 8-bit program as DOS loads it: segments of bytes, each with where it goes.
    Xex,
    /// An Atari 8-bit floppy, sector by sector, with a 16-byte header.
    Atr,
    /// An Atari 8-bit cartridge with its 16-byte header saying which kind.
    Car,
    /// An Atari 7800 cartridge with its 128-byte A78 header.
    A78,
    /// A SNES cartridge, its ROM as it is (.sfc), or after a copier's 512-byte header (.smc).
    Sfc,
    Smc,
    /// A CP/M .COM program: its bytes, loaded at 100h (the same ending as a DOS one, told apart by its machine).
    CpmCom,
    /// An Agon Light BBC BASIC program, as text.
    AgonBas,
    /// A Neo6502 program's bytes as they go in memory, loaded and started where the run says.
    NeoBin,
}

impl Format {
    /// What kind of thing a file of this format is, when it holds the program rather than being it.
    pub fn medium(self) -> Option<&'static str> {
        match self {
            Format::Tap | Format::Tzx | Format::Cas | Format::T64 => Some("tape"),
            Format::Dsk | Format::Trd | Format::Scl | Format::D64 | Format::D71 | Format::D81 | Format::Ssd | Format::Dsd | Format::Adf | Format::St | Format::Msa | Format::Img | Format::Vdk => Some("disk"),
            Format::Mdv => Some("cartridge"),
            Format::Cdt => Some("tape"),
            Format::Po | Format::Woz | Format::Atr => Some("disk"),
            Format::Car | Format::A78 | Format::Sfc | Format::Smc => Some("cartridge"),
            Format::Crt | Format::Rom | Format::A26 | Format::VecCart | Format::MdRom | Format::SmsRom | Format::GgRom | Format::SgRom | Format::ColRom | Format::Gb | Format::Gbc | Format::Nes => Some("cartridge"),
            _ => None,
        }
    }

    pub const ALL: [Format; 64] = [
        Format::Tap,
        Format::Tzx,
        Format::Z80,
        Format::Sna,
        Format::Plus3dos,
        Format::Nex,
        Format::Dsk,
        Format::Trd,
        Format::Scl,
        Format::Prg,
        Format::P00,
        Format::T64,
        Format::D64,
        Format::D71,
        Format::D81,
        Format::Crt,
        Format::Ssd,
        Format::Dsd,
        Format::Rom,
        Format::Cas,
        Format::Tos,
        Format::CocoBin,
        Format::Hunk,
        Format::Adf,
        Format::St,
        Format::Msa,
        Format::A26,
        Format::Com,
        Format::Img,
        Format::VecCart,
        Format::MdRom,
        Format::SmsRom,
        Format::GgRom,
        Format::SgRom,
        Format::ColRom,
        Format::P8,
        Format::Tic,
        Format::TicText,
        Format::RgcBas,
        Format::Gb,
        Format::Gbc,
        Format::Nes,
        Format::P,
        Format::O,
        Format::Forth,
        Format::Vdk,
        Format::Ssem,
        Format::EdsacTape,
        Format::Pal,
        Format::Rim,
        Format::QdosExe,
        Format::Mdv,
        Format::Cdt,
        Format::Po,
        Format::Woz,
        Format::Xex,
        Format::Atr,
        Format::Car,
        Format::A78,
        Format::Sfc,
        Format::Smc,
        Format::CpmCom,
        Format::AgonBas,
        Format::NeoBin,
    ];

    /// The file ending it's written with.
    pub fn ending(self) -> &'static str {
        match self {
            Format::Tap => "tap",
            Format::Tzx => "tzx",
            Format::Z80 => "z80",
            Format::Sna => "sna",
            Format::Plus3dos => "bas",
            Format::Nex => "nex",
            Format::Dsk => "dsk",
            Format::Trd => "trd",
            Format::Scl => "scl",
            Format::Prg => "prg",
            Format::P00 => "p00",
            Format::T64 => "t64",
            Format::D64 => "d64",
            Format::D71 => "d71",
            Format::D81 => "d81",
            Format::Crt => "crt",
            Format::Ssd => "ssd",
            Format::Dsd => "dsd",
            Format::Rom => "rom",
            Format::Cas => "cas",
            Format::Tos => "tos",
            Format::CocoBin => "bin",
            Format::Hunk => "exe",
            Format::Adf => "adf",
            Format::St => "st",
            Format::Msa => "msa",
            Format::A26 => "a26",
            Format::Com => "com",
            Format::Img => "img",
            Format::VecCart => "vec",
            Format::MdRom => "md",
            Format::SmsRom => "sms",
            Format::GgRom => "gg",
            Format::SgRom => "sg",
            Format::ColRom => "col",
            Format::P8 => "p8",
            Format::Tic => "tic",
            Format::TicText => "lua",
            Format::RgcBas => "bas",
            Format::Gb => "gb",
            Format::Gbc => "gbc",
            Format::Nes => "nes",
            Format::P => "p",
            Format::O => "o",
            Format::Forth => "fth",
            Format::Vdk => "vdk",
            Format::Ssem => "ssem",
            Format::EdsacTape => "edsac",
            Format::Pal => "pal",
            Format::Rim => "rim",
            // A QL program is named as the QL names them, hello_exe, with no ending of its own.
            Format::QdosExe => "exe",
            Format::Mdv => "mdv",
            Format::Cdt => "cdt",
            Format::Po => "po",
            Format::Woz => "woz",
            Format::Xex => "xex",
            Format::Atr => "atr",
            Format::Car => "car",
            Format::A78 => "a78",
            Format::Sfc => "sfc",
            Format::Smc => "smc",
            Format::CpmCom => "com",
            Format::AgonBas => "bas",
            Format::NeoBin => "bin",
        }
    }

    pub fn from_ending(ending: &str) -> Option<Format> {
        let ending = ending.to_ascii_lowercase();
        // A Mega Drive cartridge is as often .gen (for Genesis), and an SG-1000's .sc (for the SC-3000); an Apple II
        // floppy in DOS 3.3's order is a .do as often as a .dsk.
        match ending.as_str() {
            "gen" => return Some(Format::MdRom),
            "sc" => return Some(Format::SgRom),
            "do" => return Some(Format::Dsk),
            _ => {}
        }
        Format::ALL.into_iter().find(|format| format.ending() == ending)
    }
}

/// Something ready to run: the machine it's for, and the file that machine loads, under the name it goes by.
#[derive(Clone, Debug)]
pub struct Runnable {
    pub machine: Machine,
    pub format: Format,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
}

impl Runnable {
    /// A file as it is, if the machine it's for can be told from it.
    pub fn from_file(name: &str, bytes: Vec<u8>) -> Option<Runnable> {
        // An OpticBASIC program, as taken off its tape, goes back on one for OpticOS to load.
        if crate::opticbasic::is_program(&bytes) {
            let stem = file_stem(name);
            let tape = crate::opticbasic::tape(&bytes[crate::opticbasic::SIGNATURE.len()..], &stem, None);
            return Some(Runnable { machine: Machine::OpticOs, format: Format::Tap, name: format!("{stem}.tap"), bytes: Arc::new(tape) });
        }
        // A QL program, kept with its header (XTcc after it, or Q-emuLator's before it), is known by that.
        if crate::ql::is_program(&bytes) {
            return Some(Runnable { machine: Machine::Ql, format: Format::QdosExe, name: name.to_string(), bytes: Arc::new(bytes) });
        }
        // An Amiga program has no ending: it's known by how it starts.
        if !name.contains('.') && machine_for(Format::Hunk, &bytes).is_some() {
            return Some(Runnable { machine: Machine::Amiga, format: Format::Hunk, name: format!("{name}.exe"), bytes: Arc::new(bytes) });
        }
        let ending = name.rsplit_once('.').map(|(_, ending)| ending)?;
        if matches!(ending.to_ascii_lowercase().as_str(), "prg" | "ttp" | "app" | "gtp") && is_atari_program(&bytes) {
            return Some(Runnable { machine: Machine::AtariSt, format: Format::Tos, name: name.to_string(), bytes: Arc::new(bytes) });
        }
        // A ZX81 program is also a .81, or a .p81 with its name first; a ZX80 program is also a .80.
        let format = match ending.to_ascii_lowercase().as_str() {
            "81" | "p81" => Format::P,
            "80" => Format::O,
            _ => Format::from_ending(ending)?,
        };
        // A .COM that starts with a jump to the code after it, as CP/M programs usually do, is CP/M's: a DOS one
        // that did would start with a short jump (EB), which is not the 8080's C3.
        let format = if format == Format::Com && looks_like_cpm(&bytes) { Format::CpmCom } else { format };
        let machine = machine_for(format, &bytes)?;
        Some(Runnable { machine, format, name: name.to_string(), bytes: Arc::new(bytes) })
    }

    /// A BASIC listing, tokenised as the program its machine loads. `stem` names it.
    pub fn from_listing<S: AsRef<str>>(lines: &[S], machine: basic::Machine, stem: &str) -> Result<Runnable, String> {
        let (machine, format, bytes) = match machine {
            basic::Machine::Spectrum => (Machine::Spectrum48, Format::Tap, basic::spectrum_tape(&basic::tokenise_spectrum(lines, basic::ZxDialect::Spectrum)?, stem, Some(0))),
            basic::Machine::Next => (Machine::Next, Format::Plus3dos, basic::plus3dos_file(&basic::tokenise_spectrum(lines, basic::ZxDialect::Next)?, Some(0))),
            basic::Machine::Commodore(cbm) => (commodore(cbm), Format::Prg, basic::tokenise_commodore(lines, cbm)?),
            basic::Machine::Bbc => (Machine::Bbc, Format::Ssd, bbc_disc(stem, &crate::basic_more::tokenise_bbc(lines)?)),
            basic::Machine::OpticOs => (Machine::OpticOs, Format::Tap, crate::opticbasic::tape(&crate::opticbasic::tokenise(lines)?, stem, None)),
            basic::Machine::BbcZ80 => return Err(crate::i18n::tr("BBC BASIC (Z80) programs can be saved, for a Z88 or an NC100, but there's no emulator to run them in yet.").into()),
            basic::Machine::Amos => return Err(crate::i18n::tr("AMOS programs can be saved (Format, Tokenised BASIC, Save as an AMOS program) and loaded in AMOS, but Limoniter can't run them in an emulator yet.").into()),
            basic::Machine::Stos => return Err(crate::i18n::tr("STOS programs can be saved (Format, Tokenised BASIC, Save as a STOS program) and loaded in STOS, but Limoniter can't run them in an emulator yet.").into()),
            basic::Machine::Zx80 => (Machine::Zx80, Format::O, crate::zx80_program::program(lines)?),
            basic::Machine::Zx81 => (Machine::Zx81, Format::P, crate::zx81_program::program(lines)?),
            basic::Machine::JupiterAce => (Machine::JupiterAce, Format::Forth, forth_text(lines)),
        };
        Ok(Runnable { machine, format, name: format!("{}.{}", file_stem(stem), format.ending()), bytes: Arc::new(bytes) })
    }
}

/// Forth source as the text typed into the Ace, a line at a time.
pub fn forth_text<S: AsRef<str>>(lines: &[S]) -> Vec<u8> {
    lines.iter().map(|l| format!("{}\n", l.as_ref())).collect::<String>().into_bytes()
}

fn commodore(cbm: basic::CbmMachine) -> Machine {
    match cbm {
        basic::CbmMachine::C64 => Machine::C64,
        basic::CbmMachine::Vic20 => Machine::Vic20,
        basic::CbmMachine::Pet => Machine::Pet,
        basic::CbmMachine::Plus4 => Machine::Plus4,
        basic::CbmMachine::C128 => Machine::C128,
    }
}

/// A name that's safe as a file name anywhere.
fn file_stem(stem: &str) -> String {
    // A name ends at its first dot: notes.bas.txt is notes.
    let stem = stem.split('.').next().unwrap_or(stem);
    let safe: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '_' }).collect();
    if safe.trim().is_empty() { "program".to_string() } else { safe.trim().to_string() }
}

/// The machine a file is for, from the kind of file and what's in it.
pub fn machine_for(format: Format, bytes: &[u8]) -> Option<Machine> {
    Some(match format {
        // The Jupiter Ace's tapes are also .tap and .tzx, with a longer header; a ZX81 tape in a TZX has its own
        // blocks, and none of the Spectrum's.
        Format::Tap if crate::jupiter_ace::is_ace_tape(bytes) => Machine::JupiterAce,
        Format::Tzx if crate::jupiter_ace::is_ace_tzx(bytes) => Machine::JupiterAce,
        Format::Tzx if basic::tape_blocks(bytes).0.is_empty() && !crate::media::spectrum::zx81_programs(bytes).is_empty() => Machine::Zx81,
        Format::Tap | Format::Tzx => Machine::Spectrum48,
        // .p is also Pascal and Prolog source: a ZX81 program's system variables say what it is.
        Format::P if crate::basic_extra::zx81_skip(bytes).is_ok() => Machine::Zx81,
        Format::P => return None,
        // .o is also any compiler's object file: a ZX80 program's system variables say what it is.
        Format::O if crate::basic_extra::zx80_length(bytes).is_ok() => Machine::Zx80,
        Format::O => return None,
        Format::Forth if std::str::from_utf8(bytes).is_ok() => Machine::JupiterAce,
        Format::Forth => return None,
        // The teaching machines' source is text; a RIM tape is frames of six bits, with leader.
        Format::Ssem | Format::EdsacTape | Format::Pal if std::str::from_utf8(bytes).is_err() => return None,
        Format::Ssem => Machine::Ssem,
        Format::EdsacTape => Machine::Edsac,
        Format::Pal => Machine::Pdp8,
        Format::Rim if crate::pdp8::read_rim(bytes).is_ok() => Machine::Pdp8,
        Format::Rim => return None,
        // A Dragon DOS disk: its directory's map says how many tracks it has, and the same again inverted.
        Format::Vdk if crate::dragon_dos::Disk::from_vdk(bytes).is_some() => Machine::Dragon32,
        Format::Vdk => return None,
        // .z80 is also assembler source: a snapshot's header has control codes, and text has none.
        Format::Z80 if !bytes.iter().take(30).any(|b| *b < 9 || (14..32).contains(b)) => return None,
        Format::Z80 => z80_machine(bytes),
        // An Amstrad CPC snapshot says so at its start; the Spectrum's has no signature.
        Format::Sna if bytes.starts_with(b"MV - SNA") => Machine::Cpc,
        Format::Sna => {
            if bytes.len() == 49179 {
                Machine::Spectrum48
            } else {
                Machine::Spectrum128
            }
        }
        Format::Plus3dos => {
            if !bytes.starts_with(b"PLUS3DOS") {
                return None;
            }
            // A BASIC program is the Next's if it's NextBASIC; anything else a +3 runs too.
            let listed = basic::list(bytes, "bas", crate::charset::utf8());
            if listed.dialect == basic::ZxDialect::Next.name() { Machine::Next } else { Machine::SpectrumPlus3 }
        }
        Format::Nex => Machine::Next,
        // A Dragon DOS binary: 55, its type (2), where it loads, how long it is and where it starts, then AA.
        Format::CocoBin if bytes.first() == Some(&0x55) && bytes.get(1) == Some(&0x02) && bytes.get(8) == Some(&0xAA) => Machine::Dragon32,
        // Blocks of 0, a length, an address and the bytes, ended by FF 00 00 and where it starts.
        Format::CocoBin => {
            let mut at = 0;
            loop {
                match bytes.get(at)? {
                    0x00 => at += 5 + usize::from(u16::from_be_bytes([*bytes.get(at + 1)?, *bytes.get(at + 2)?])),
                    0xFF if at + 5 == bytes.len() && bytes.get(at + 1..at + 3) == Some(&[0, 0][..]) => break Machine::CoCo,
                    _ => return None,
                }
            }
        }
        // A Vectrex cartridge starts with its copyright line, "g GCE" and a year, as its BIOS checks.
        Format::VecCart if bytes.starts_with(b"g GCE") => Machine::Vectrex,
        Format::VecCart => return None,
        Format::MdRom if crate::sega::is_mega_drive(bytes) => Machine::MegaDrive,
        Format::MdRom => return None,
        // A Master System cartridge needn't have its header (the Japanese ones don't), so the ending says.
        Format::SmsRom => Machine::MasterSystem,
        Format::GgRom => Machine::GameGear,
        Format::SgRom => Machine::Sg1000,
        Format::ColRom if crate::sega::is_coleco(bytes) => Machine::ColecoVision,
        Format::ColRom => return None,
        Format::P8 if bytes.starts_with(b"pico-8 cartridge") => Machine::Pico8,
        Format::P8 => return None,
        Format::Tic => Machine::Tic80,
        // A .lua is a TIC-80 cart only when it says so at its top (-- script:), or carries the cart's data.
        Format::TicText if crate::fantasy_carts::console_of("cart.lua", &String::from_utf8_lossy(bytes)) == Some(crate::fantasy_carts::Console::Tic80) => Machine::Tic80,
        Format::TicText => return None,
        // A .bas is RGC BASIC's only when it reads as it (see `highlight::looks_like_rgc_basic`).
        Format::RgcBas if std::str::from_utf8(bytes).is_ok_and(|text| crate::highlight::looks_like_rgc_basic(&text.lines().map(std::sync::Arc::from).collect::<Vec<_>>())) => Machine::RgcBasic,
        Format::RgcBas => return None,
        // A Game Boy cartridge has the logo its boot ROM checks; an NES one starts NES and $1A.
        Format::Gb | Format::Gbc if crate::gameboy::is_cartridge(bytes) => Machine::GameBoy,
        Format::Gb | Format::Gbc => return None,
        Format::Nes if crate::nes::Header::read(bytes).is_some() => Machine::Nes,
        Format::Nes => return None,
        Format::Dsk => dsk_machine(bytes),
        Format::Trd | Format::Scl => Machine::Pentagon,
        Format::Prg => commodore_by_load(bytes.get(0..2)?),
        Format::P00 => commodore_by_load(bytes.get(26..28)?),
        Format::T64 | Format::D64 | Format::D71 | Format::D81 | Format::Crt => Machine::C64,
        Format::Ssd | Format::Dsd => Machine::Bbc,
        Format::Rom => {
            if bytes.starts_with(b"AB") {
                Machine::Msx
            } else {
                return None;
            }
        }
        Format::Cas => Machine::Msx,
        Format::Hunk if bytes.starts_with(&[0, 0, 3, 0xF3]) => Machine::Amiga,
        Format::Hunk => return None,
        // A double-density disk, or a high-density one.
        Format::Adf if matches!(bytes.len(), 901_120 | 1_802_240) => Machine::Amiga,
        Format::Adf => return None,
        Format::Tos => {
            if !bytes.starts_with(&[0x60, 0x1A]) {
                return None;
            }
            Machine::AtariSt
        }
        // A double-sided 720K or single-sided 360K floppy (or another of 9 to 11 sectors a track) whose boot sector
        // says 512-byte sectors; .st is Smalltalk source too, which this rules out.
        Format::St if bytes.len().is_multiple_of(512) && (327_680..=1_064_960).contains(&bytes.len()) && bytes.get(11..13) == Some(&[0, 2][..]) => Machine::AtariSt,
        Format::St => return None,
        Format::Msa if bytes.starts_with(&[0x0E, 0x0F]) => Machine::AtariSt,
        Format::Msa => return None,
        Format::A26 if crate::atari2600::detect(bytes).is_some() => Machine::Atari2600,
        Format::A26 => return None,
        // A .COM fits in a segment, less the 256 bytes before it.
        Format::Com if !bytes.is_empty() && bytes.len() <= 0xFF00 => Machine::Pc,
        Format::Com => return None,
        // A PC floppy that boots: 55 AA at the end of its first sector.
        Format::Img if PC_FLOPPIES.contains(&bytes.len()) && bytes.get(510..512) == Some(&[0x55, 0xAA][..]) => Machine::Pc,
        Format::Img => return None,
        Format::QdosExe if crate::ql::is_program(bytes) => Machine::Ql,
        Format::QdosExe => return None,
        Format::Mdv if crate::media::ql::looks_like(bytes, bytes.len() as u64) => Machine::Ql,
        Format::Mdv => return None,
        // A CPC tape is a TZX, which starts ZXTape! and $1A.
        Format::Cdt if bytes.starts_with(b"ZXTape!\x1A") => Machine::Cpc,
        Format::Cdt => return None,
        // .po is also gettext's translations, which are text: a ProDOS disk's volume directory, in block 2, starts
        // with its storage type, $F, in the top of its fifth byte.
        Format::Po if bytes.len() >= APPLE_FLOPPY && bytes.len().is_multiple_of(512) && bytes[0x404] >> 4 == 0x0F => Machine::Apple2,
        Format::Po => return None,
        Format::Woz if (bytes.starts_with(b"WOZ1") || bytes.starts_with(b"WOZ2")) && bytes.get(4..8) == Some(&[0xFF, 0x0A, 0x0D, 0x0A][..]) => Machine::Apple2,
        Format::Woz => return None,
        // An Atari DOS binary starts $FFFF, then its first segment's addresses.
        Format::Xex if bytes.len() >= 6 && bytes.starts_with(&[0xFF, 0xFF]) => Machine::Atari8,
        Format::Xex => return None,
        // An ATR's header starts $0296, the sum of NICKATARI's letters.
        Format::Atr if bytes.len() >= 16 && bytes.starts_with(&[0x96, 0x02]) => Machine::Atari8,
        Format::Atr => return None,
        Format::Car if bytes.len() > 16 && bytes.starts_with(b"CART") => Machine::Atari8,
        Format::Car => return None,
        Format::A78 if crate::atari7800::Header::read(bytes).is_some() => Machine::Atari7800,
        Format::A78 => return None,
        // A SNES cartridge is 32K banks; a copier put 512 bytes of its own before them.
        Format::Sfc | Format::Smc if bytes.len() >= 0x8000 && matches!(bytes.len() % 0x8000, 0 | 512) => Machine::Snes,
        Format::Sfc | Format::Smc => return None,
        // CP/M's .COM is told from DOS's by `looks_like_cpm`, when a file is opened; a Neo6502's bytes and an
        // Agon's BASIC come only from F9 (their endings are shared with other things).
        Format::CpmCom if looks_like_cpm(bytes) => Machine::Cpm,
        Format::CpmCom | Format::AgonBas | Format::NeoBin => return None,
    })
}

/// Whether a .COM looks like a CP/M program rather than a DOS one: it starts with JP (C3) to an address inside the
/// program, 8080 and Z80 programs' usual way to jump over their data. In 8086 code C3 is RET, which a program
/// doesn't start with.
pub fn looks_like_cpm(bytes: &[u8]) -> bool {
    match bytes {
        [0xC3, low, high, ..] => {
            let target = u16::from_le_bytes([*low, *high]) as usize;
            (0x103..0x100 + bytes.len()).contains(&target)
        }
        _ => false,
    }
}

/// The sizes of the PC's floppies: 160K to 1.44M, and 2.88M.
pub const PC_FLOPPIES: [usize; 8] = [163_840, 184_320, 327_680, 368_640, 737_280, 1_228_800, 1_474_560, 2_949_120];

/// A 1.44M floppy that boots a sector of the PC's code.
pub fn pc_boot_floppy(sector: &[u8]) -> Vec<u8> {
    let mut disk = vec![0u8; 1_474_560];
    disk[..sector.len().min(512)].copy_from_slice(&sector[..sector.len().min(512)]);
    disk
}

/// Whether a file is an Atari ST program: $601A, then sizes of its parts that fit in the file.
pub fn is_atari_program(bytes: &[u8]) -> bool {
    let long = |at: usize| bytes.get(at..at + 4).map_or(u64::MAX, |b| u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])));
    bytes.starts_with(&[0x60, 0x1A]) && bytes.len() >= 28 && 28 + long(2) + long(6) + long(14) <= bytes.len() as u64
}

/// A Commodore program says which machine it's for by where it loads.
fn commodore_by_load(load: &[u8]) -> Machine {
    match u16::from_le_bytes([load[0], load[1]]) {
        0x1C01 => Machine::C128,
        0x1001 | 0x1201 => Machine::Vic20,
        0x0401 => Machine::Pet,
        _ => Machine::C64,
    }
}

/// A Z80 snapshot's header says which machine it was taken on. Version 1 is always a 48K; later versions name
/// the hardware, numbered a little differently in version 2 and version 3.
fn z80_machine(bytes: &[u8]) -> Machine {
    let pc = bytes.get(6..8).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    if pc != 0 || bytes.len() < 35 {
        return Machine::Spectrum48;
    }
    let version2 = bytes.get(30..32).is_some_and(|b| u16::from_le_bytes([b[0], b[1]]) == 23);
    match (version2, bytes[34]) {
        (true, 3 | 4) | (false, 4..=6) | (_, 12) => Machine::Spectrum128,
        (_, 7 | 8 | 13) => Machine::SpectrumPlus3,
        (_, 9) => Machine::Pentagon,
        _ => Machine::Spectrum48,
    }
}

/// The size of an Apple II floppy: 35 tracks of sixteen 256-byte sectors.
pub const APPLE_FLOPPY: usize = 143_360;

/// A +3 disk and an Amstrad CPC disk share a container; the CPC numbers its sectors from &41 or &C1, the +3 from 1.
/// An Apple II floppy is a .dsk too, its sectors one after another with no header at all.
fn dsk_machine(bytes: &[u8]) -> Machine {
    if bytes.len() == APPLE_FLOPPY && !bytes.starts_with(b"MV - CPC") && !bytes.starts_with(b"EXTENDED CPC DSK") {
        return Machine::Apple2;
    }
    // The first track's header follows the disk's own, and lists its sectors from byte 0x18, the number third.
    match bytes.get(0x100 + 0x18 + 2) {
        Some(0x41..=0x49 | 0xC1..=0xC9) => Machine::Cpc,
        _ => Machine::SpectrumPlus3,
    }
}

/// A BBC Micro disc (an 80-track single-sided DFS image) holding one BASIC program, which boots by CHAINing it.
pub fn bbc_disc(stem: &str, program: &[u8]) -> Vec<u8> {
    let name = dfs_name(stem);
    // BASIC's addresses are in the I/O processor.
    bbc_booting(&name, program, 0xFF1900, 0xFF8023, &format!("CHAIN \"{name}\""))
}

/// A BBC Micro disc holding machine code, which boots by *RUNning it: loaded at `load` and started at `exec`.
pub fn bbc_code_disc(stem: &str, code: &[u8], load: u16, exec: u16) -> Vec<u8> {
    let name = dfs_name(stem);
    bbc_booting(&name, code, 0xFF0000 | u32::from(load), 0xFF0000 | u32::from(exec), &format!("*RUN {name}"))
}

/// A DFS file name for a file: up to seven letters and digits, in capitals.
fn dfs_name(stem: &str) -> String {
    let name: String = stem.chars().filter(|c| c.is_ascii_alphanumeric()).take(7).collect::<String>().to_ascii_uppercase();
    if name.is_empty() { "PROGRAM".to_string() } else { name }
}

/// A disc of one file, with a !BOOT that EXEC runs: `boot` is its one line.
fn bbc_booting(name: &str, program: &[u8], load: u32, exec: u32, boot: &str) -> Vec<u8> {
    const SECTOR: usize = 256;
    const SECTORS: usize = 800;
    let boot = format!("{boot}\r").into_bytes();
    let mut disc = vec![0u8; SECTORS * SECTOR];
    // The files after the two catalogue sectors: !BOOT, then the program.
    let boot_start = 2;
    let program_start = boot_start + boot.len().div_ceil(SECTOR);
    disc[boot_start * SECTOR..][..boot.len()].copy_from_slice(&boot);
    disc[program_start * SECTOR..][..program.len()].copy_from_slice(program);
    let title = b"LIMONITER   ";
    disc[..8].copy_from_slice(&title[..8]);
    disc[SECTOR..SECTOR + 4].copy_from_slice(&title[8..12]);
    // Listed highest start sector first, as DFS keeps them.
    let files: [(&str, usize, u32, u32, usize); 2] = [(name, program_start, load, exec, program.len()), ("!BOOT", boot_start, 0, 0, boot.len())];
    for (i, (file, start, load, exec, length)) in files.iter().enumerate() {
        let mut padded = [b' '; 7];
        padded[..file.len()].copy_from_slice(file.as_bytes());
        disc[8 + i * 8..][..7].copy_from_slice(&padded);
        disc[8 + i * 8 + 7] = b'$';
        let entry = &mut disc[SECTOR + 8 + i * 8..][..8];
        entry[0..2].copy_from_slice(&(*load as u16).to_le_bytes());
        entry[2..4].copy_from_slice(&(*exec as u16).to_le_bytes());
        entry[4..6].copy_from_slice(&(*length as u16).to_le_bytes());
        // The top bits of each, packed: exec, length, load, start sector.
        let high = |value: u32| ((value >> 16) & 3) as u8;
        entry[6] = (high(*exec) << 6) | ((((*length >> 16) & 3) as u8) << 4) | (high(*load) << 2) | (((*start >> 8) & 3) as u8);
        entry[7] = (*start & 0xFF) as u8;
    }
    disc[SECTOR + 5] = (files.len() * 8) as u8;
    // Boot option 3 (EXEC !BOOT), and the disc's size in sectors.
    disc[SECTOR + 6] = (3 << 4) | ((SECTORS >> 8) as u8 & 3);
    disc[SECTOR + 7] = (SECTORS & 0xFF) as u8;
    disc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_say_which_machine_they_are_for() {
        let run = |name: &str, bytes: Vec<u8>| Runnable::from_file(name, bytes).map(|r| (r.machine, r.format));
        assert_eq!(run("game.tap", vec![0; 30]), Some((Machine::Spectrum48, Format::Tap)));
        assert_eq!(run("GAME.PRG", vec![0x01, 0x08, 0, 0]), Some((Machine::C64, Format::Prg)));
        assert_eq!(run("game.prg", vec![0x01, 0x1C, 0, 0]).map(|r| r.0), Some(Machine::C128));
        assert_eq!(run("game.prg", vec![0x01, 0x10, 0, 0]).map(|r| r.0), Some(Machine::Vic20));
        assert_eq!(run("game.sna", vec![0; 49179]).map(|r| r.0), Some(Machine::Spectrum48));
        assert_eq!(run("game.sna", vec![0; 131103]).map(|r| r.0), Some(Machine::Spectrum128));
        assert_eq!(run("game.ssd", vec![0; 512]).map(|r| r.0), Some(Machine::Bbc));
        assert_eq!(run("game.rom", b"AB\0\0".to_vec()).map(|r| r.0), Some(Machine::Msx));
        assert_eq!(run("game.rom", vec![0; 4]), None, "a ROM that isn't an MSX cartridge isn't guessed at");
        assert_eq!(run("notes.txt", vec![0; 4]), None);
        assert_eq!(run("game.bas", b"10 PRINT".to_vec()), None, "a .bas without a +3DOS header isn't a Spectrum's");
        let hunks = vec![0, 0, 3, 0xF3, 0, 0, 0, 0];
        assert_eq!(run("demo", hunks.clone()), Some((Machine::Amiga, Format::Hunk)), "an Amiga program has no ending");
        assert_eq!(run("demo.exe", hunks), Some((Machine::Amiga, Format::Hunk)));
        assert_eq!(run("setup.exe", b"MZ\x90\0".to_vec()), None, "a Windows program isn't run");
        assert_eq!(run("disk.adf", vec![0; 901_120]), Some((Machine::Amiga, Format::Adf)));
        // Atari ST floppies, and a .PRG that's an ST program rather than a Commodore one.
        let floppy = crate::st_run::bootable_floppy(&[0x60, 0x1A], "x.tos", None).unwrap();
        assert_eq!(run("demo.st", floppy.clone()), Some((Machine::AtariSt, Format::St)));
        assert_eq!(run("Object.st", b"Object subclass: #Account".to_vec()), None, "Smalltalk");
        assert_eq!(run("demo.msa", crate::st_run::pack_msa(&floppy).unwrap()), Some((Machine::AtariSt, Format::Msa)));
        let mut program = vec![0x60, 0x1A, 0, 0, 0, 2];
        program.resize(30, 0);
        assert_eq!(run("DEMO.PRG", program), Some((Machine::AtariSt, Format::Tos)));
        assert_eq!(run("game.prg", vec![0x60, 0x1A, 0xA9, 0]), Some((Machine::C64, Format::Prg)), "too short for an ST program");
    }

    #[test]
    fn cpc_apple_atari_and_snes_files_say_which_machine_they_are_for() {
        let run = |name: &str, bytes: Vec<u8>| Runnable::from_file(name, bytes).map(|r| (r.machine, r.format));
        let mut cpc_sna = b"MV - SNA".to_vec();
        cpc_sna.resize(0x100 + 65536, 0);
        assert_eq!(run("game.sna", cpc_sna), Some((Machine::Cpc, Format::Sna)));
        assert_eq!(run("game.sna", vec![0; 49179]).map(|r| r.0), Some(Machine::Spectrum48), "a Spectrum's has no signature");
        assert_eq!(run("game.cdt", b"ZXTape!\x1A\x01\x14".to_vec()), Some((Machine::Cpc, Format::Cdt)));
        assert_eq!(run("game.cdt", b"text".to_vec()), None);
        // Apple II floppies: a .dsk (or .do) with no header at all, a ProDOS-ordered .po, and a WOZ.
        assert_eq!(run("game.dsk", vec![0; APPLE_FLOPPY]), Some((Machine::Apple2, Format::Dsk)));
        assert_eq!(run("GAME.DO", vec![0; APPLE_FLOPPY]), Some((Machine::Apple2, Format::Dsk)));
        let prodos = crate::media::write::Blank::AppleProdos.make("TEST");
        assert_eq!(run("game.po", prodos), Some((Machine::Apple2, Format::Po)));
        assert_eq!(run("messages.po", b"msgid \"Hello\"\nmsgstr \"Hallo\"\n".to_vec()), None, "gettext's translations");
        assert_eq!(run("game.woz", b"WOZ2\xFF\x0A\x0D\x0Arest".to_vec()), Some((Machine::Apple2, Format::Woz)));
        assert_eq!(run("game.woz", b"WOZ2 not one".to_vec()), None);
        // The Atari 8-bits' program, disk and cartridge.
        assert_eq!(run("game.xex", vec![0xFF, 0xFF, 0x00, 0x20, 0x00, 0x20, 0x60]), Some((Machine::Atari8, Format::Xex)));
        assert_eq!(run("game.atr", crate::media::write::Blank::Atari90.make("")), Some((Machine::Atari8, Format::Atr)));
        let mut car = b"CART\0\0\0\x01\0\0\0\0\0\0\0\0".to_vec();
        car.resize(16 + 8192, 0);
        assert_eq!(run("game.car", car), Some((Machine::Atari8, Format::Car)));
        assert_eq!(run("game.car", vec![0; 100]), None);
        // The SNES's cartridges, with and without a copier's header.
        assert_eq!(run("game.sfc", vec![0; 0x8000]), Some((Machine::Snes, Format::Sfc)));
        assert_eq!(run("game.smc", vec![0; 0x8000 + 512]), Some((Machine::Snes, Format::Smc)));
        assert_eq!(run("game.sfc", vec![0; 1000]), None);
        for format in [Format::Cdt, Format::Po, Format::Woz, Format::Xex, Format::Atr, Format::Car, Format::A78, Format::Sfc, Format::Smc] {
            assert_eq!(Format::from_ending(&format.ending().to_uppercase()), Some(format));
        }
    }

    #[test]
    fn a_z80_snapshot_says_which_spectrum_took_it() {
        let mut v1 = vec![0u8; 40];
        v1[6] = 0x34;
        assert_eq!(z80_machine(&v1), Machine::Spectrum48);
        let mut v2 = vec![0u8; 60];
        v2[30] = 23;
        v2[34] = 3;
        assert_eq!(z80_machine(&v2), Machine::Spectrum128);
        let mut v3 = vec![0u8; 90];
        v3[30] = 54;
        v3[34] = 3;
        assert_eq!(z80_machine(&v3), Machine::Spectrum48, "in version 3, 3 is a 48K with an M.G.T.");
        v3[34] = 4;
        assert_eq!(z80_machine(&v3), Machine::Spectrum128);
        v3[34] = 7;
        assert_eq!(z80_machine(&v3), Machine::SpectrumPlus3);
    }

    #[test]
    fn listings_become_the_programs_their_machines_load() {
        let next = Runnable::from_listing(&["10 LAYER 2,1"], basic::Machine::Next, "stars").unwrap();
        assert_eq!((next.machine, next.format, next.name.as_str()), (Machine::Next, Format::Plus3dos, "stars.bas"));
        assert_eq!(machine_for(Format::Plus3dos, &next.bytes), Some(Machine::Next), "and it reads back as the Next's");
        let tape = Runnable::from_listing(&["10 PRINT \"hi\""], basic::Machine::Spectrum, "hello world!").unwrap();
        assert_eq!((tape.machine, tape.name.as_str()), (Machine::Spectrum48, "hello world_.tap"));
        assert_eq!(Runnable::from_listing(&["10 STOP"], basic::Machine::Spectrum, "rainbow.bas.txt").unwrap().name, "rainbow.tap");
        let c64 = Runnable::from_listing(&["10 PRINT \"HI\""], basic::Machine::Commodore(basic::CbmMachine::C64), "hi").unwrap();
        assert_eq!((c64.machine, &c64.bytes[..2]), (Machine::C64, &[0x01, 0x08][..]));
    }

    #[test]
    fn a_bbc_listing_goes_on_a_disc_that_boots_it() {
        let program = crate::basic_more::tokenise_bbc(&["10 PRINT \"HELLO\"", "20 GOTO 10"]).unwrap();
        let disc = Runnable::from_listing(&["10 PRINT \"HELLO\"", "20 GOTO 10"], basic::Machine::Bbc, "hello").unwrap();
        assert_eq!((disc.machine, disc.format, disc.name.as_str()), (Machine::Bbc, Format::Ssd, "hello.ssd"));
        // Limoniter's own DFS reader finds both files, the program intact.
        let volume = crate::media::open(Arc::new(disc.bytes.to_vec()), "hello.ssd").unwrap();
        let entries = volume.list(None).unwrap();
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        assert!(names.iter().any(|n| n.contains("!BOOT")) && names.iter().any(|n| n.contains("HELLO")), "{names:?}");
        let found = entries.iter().find(|e| e.name.contains("HELLO")).unwrap();
        assert_eq!(volume.read(found).unwrap(), program);
        let boot = entries.iter().find(|e| e.name.contains("!BOOT")).unwrap();
        assert_eq!(volume.read(boot).unwrap(), b"CHAIN \"HELLO\"\r");
        assert_eq!(disc.bytes[256 + 6] >> 4, 3, "it boots by EXECing !BOOT");
    }

    #[test]
    fn bbc_machine_code_goes_on_a_disc_that_runs_it() {
        let code = [0xA9, 0x41, 0x20, 0xEE, 0xFF, 0x60];
        let disc = bbc_code_disc("hello-world", &code, 0x2000, 0x2002);
        let volume = crate::media::open(Arc::new(disc.clone()), "hello.ssd").unwrap();
        let entries = volume.list(None).unwrap();
        let program = entries.iter().find(|e| e.name.contains("HELLOWO")).unwrap_or_else(|| panic!("{:?}", entries.iter().map(|e| &e.name).collect::<Vec<_>>()));
        assert_eq!(volume.read(program).unwrap(), code);
        let boot = entries.iter().find(|e| e.name.contains("!BOOT")).unwrap();
        assert_eq!(volume.read(boot).unwrap(), b"*RUN HELLOWO\r");
        // The program's catalogue entry: loaded at &FF2000 and run at &FF2002, in the I/O processor.
        let entry = &disc[256 + 8..][..8];
        assert_eq!((u16::from_le_bytes([entry[0], entry[1]]), u16::from_le_bytes([entry[2], entry[3]]), usize::from(u16::from_le_bytes([entry[4], entry[5]]))), (0x2000, 0x2002, code.len()));
        assert_eq!((entry[6] >> 6, (entry[6] >> 2) & 3), (3, 3));
        assert_eq!(disc[256 + 6] >> 4, 3, "it boots by EXECing !BOOT");
    }

    #[test]
    fn plus3_and_cpc_disks_are_told_apart() {
        let mut disk = vec![0u8; 0x200];
        disk[0x11A] = 0xC1;
        assert_eq!(dsk_machine(&disk), Machine::Cpc);
        disk[0x11A] = 1;
        assert_eq!(dsk_machine(&disk), Machine::SpectrumPlus3);
    }
}
