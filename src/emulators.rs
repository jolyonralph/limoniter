//! The emulators Limoniter knows: which machines each runs, the files it takes, how to start it with a file, how
//! it can be reached while it runs, and where to look for it. Each command line here was checked against the
//! emulator's own help or source.

use crate::running::{Format, Machine};
use std::path::{Path, PathBuf};

/// How an emulator can be reached while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    None,
    /// ZEsarUX's remote command protocol (ZRCP): text commands over TCP.
    Zrcp,
    /// VICE's binary monitor: framed binary commands over TCP.
    ViceMonitor,
    /// A GDB stub: the GDB remote serial protocol over TCP, as XRoar has.
    Gdb,
}

pub struct Emulator {
    pub id: &'static str,
    pub name: &'static str,
    /// The program names it goes by, without .exe, best first. VICE has one per machine; see `program_for`.
    pub programs: &'static [&'static str],
    /// Words its installed folder's name contains, to look in on Windows.
    pub folder_hints: &'static [&'static str],
    /// Its app's name on a Mac, and the program inside it.
    pub mac_apps: &'static [(&'static str, &'static str)],
    pub machines: &'static [Machine],
    pub formats: &'static [Format],
    pub link: Link,
    /// Where to get it, for when it isn't installed.
    pub website: &'static str,
}

use Format::*;
use Machine::*;

pub const KNOWN: &[Emulator] = &[
    Emulator {
        id: "zesarux",
        name: "ZEsarUX",
        programs: &["zesarux"],
        folder_hints: &["zesarux"],
        mac_apps: &[("ZEsarUX.app", "zesarux")],
        machines: &[Spectrum48, Spectrum128, SpectrumPlus3, Pentagon, Next, Zx80, Zx81, JupiterAce],
        formats: &[Tap, Tzx, Z80, Sna, Nex, Dsk, Trd, Scl, P, O],
        link: Link::Zrcp,
        website: "https://github.com/chernandezba/zesarux",
    },
    // EightyOne, the ZX80 and ZX81 emulator for Windows, puts a file given on its command line in its tape player
    // (main_.cpp in its source, version 1.41); the machine is the one chosen in it last, and LOAD is typed there.
    Emulator {
        id: "eightyone",
        name: "EightyOne",
        programs: &["EightyOne"],
        folder_hints: &["eightyone"],
        mac_apps: &[],
        machines: &[Zx80, Zx81],
        formats: &[O, P, Tzx],
        link: Link::None,
        website: "https://sourceforge.net/projects/eightyone-sinclair-emulator/",
    },
    Emulator {
        id: "fuse",
        name: "Fuse",
        programs: &["fuse", "fuse-sdl", "fuse-gtk"],
        folder_hints: &["fuse"],
        mac_apps: &[("Fuse.app", "Fuse")],
        machines: &[Spectrum48, Spectrum128, SpectrumPlus3, Pentagon, OpticOs],
        formats: &[Tap, Tzx, Z80, Sna, Dsk, Trd, Scl],
        link: Link::None,
        website: "https://fuse-emulator.sourceforge.net",
    },
    Emulator {
        id: "cspect",
        name: "CSpect",
        programs: &["CSpect"],
        folder_hints: &["cspect"],
        mac_apps: &[],
        machines: &[Next],
        formats: &[Nex, Sna],
        link: Link::None,
        website: "https://mdf200.itch.io/cspect",
    },
    Emulator {
        id: "vice",
        name: "VICE",
        programs: &["x64sc", "x64", "x128", "xvic", "xplus4", "xpet"],
        folder_hints: &["vice"],
        mac_apps: &[("VICE.app", "x64sc"), ("x64sc.app", "x64sc")],
        machines: &[C64, C128, Vic20, Plus4, Pet],
        formats: &[Prg, P00, T64, D64, D71, D81, Crt],
        link: Link::ViceMonitor,
        website: "https://vice-emu.sourceforge.io",
    },
    Emulator {
        id: "b2",
        name: "b2",
        programs: &["b2"],
        folder_hints: &["b2"],
        mac_apps: &[("b2.app", "b2")],
        machines: &[Bbc],
        formats: &[Ssd, Dsd],
        link: Link::None,
        website: "https://github.com/tom-seddon/b2",
    },
    Emulator {
        id: "openmsx",
        name: "openMSX",
        programs: &["openmsx"],
        folder_hints: &["openmsx"],
        mac_apps: &[("openMSX.app", "openmsx")],
        machines: &[Msx],
        formats: &[Rom, Cas],
        link: Link::None,
        website: "https://openmsx.org",
    },
    Emulator {
        id: "hatari",
        name: "Hatari",
        programs: &["hatari"],
        folder_hints: &["hatari"],
        mac_apps: &[("Hatari.app", "Hatari")],
        machines: &[AtariSt],
        formats: &[Tos, St, Msa],
        link: Link::None,
        website: "https://www.hatari-emu.org",
    },
    Emulator {
        id: "xroar",
        name: "XRoar",
        programs: &["xroar"],
        folder_hints: &["xroar"],
        mac_apps: &[("XRoar.app", "xroar")],
        machines: &[CoCo, Dragon32, Dragon64],
        formats: &[CocoBin, Vdk],
        link: Link::Gdb,
        website: "https://www.6809.org.uk/xroar/",
    },
    // The Amiga's are started with a configuration file made for the run (see `amiga_run`). uae-dap ships an FS-UAE
    // with a GDB server, named for each system, and vscode-amiga-debug a WinUAE with one (see `amiga_debug`).
    Emulator {
        id: "fs-uae",
        name: "FS-UAE",
        programs: &["fs-uae", "fs-uae-linux_x64", "fs-uae-darwin_x64", "fs-uae-win32_x64"],
        folder_hints: &["fs-uae", "fsuae"],
        mac_apps: &[("FS-UAE.app", "fs-uae")],
        machines: &[Amiga],
        formats: &[Hunk, Adf],
        link: Link::None,
        website: "https://fs-uae.net",
    },
    Emulator {
        id: "winuae",
        name: "WinUAE",
        programs: &["winuae64", "winuae", "winuae-gdb"],
        folder_hints: &["winuae"],
        mac_apps: &[],
        machines: &[Amiga],
        formats: &[Hunk, Adf],
        link: Link::None,
        website: "https://www.winuae.net",
    },
    // Copperline, a cycle-exact OCS, ECS and AGA Amiga written in Rust (1.1, October 2026): a program is
    // warp-booted with --run, its folder mounted as a drive, and debugged at its source lines through
    // copperline-ctl, its Debug Adapter Protocol server, which can step backwards too (see `dap`).
    Emulator {
        id: "copperline",
        name: "Copperline",
        programs: &["copperline", "Copperline"],
        folder_hints: &["copperline"],
        mac_apps: &[("Copperline.app", "copperline")],
        machines: &[Amiga],
        formats: &[Hunk, Adf],
        link: Link::None,
        website: "https://github.com/CopperlineHQ/Copperline",
    },
    // vamos, from amitools: no emulated machine at all, but an Amiga program's code run here, what it asks of
    // AmigaDOS done on this computer's files, and its output brought back (see `amiga_vamos`).
    Emulator {
        id: "vamos",
        name: "vamos (AmigaShell programs here)",
        programs: &["vamos"],
        folder_hints: &["amitools"],
        mac_apps: &[],
        machines: &[Amiga],
        formats: &[Hunk],
        link: Link::None,
        website: crate::amiga_vamos::WEBSITE,
    },
    // The PC: DOSBox runs a .COM from its folder, made its C: drive, and boots a floppy; QEMU boots one.
    Emulator {
        id: "dosbox",
        name: "DOSBox",
        programs: &["dosbox-staging", "dosbox-x", "dosbox", "DOSBox"],
        folder_hints: &["dosbox"],
        mac_apps: &[("dosbox-staging.app", "dosbox"), ("DOSBox Staging.app", "dosbox"), ("dosbox-x.app", "dosbox-x"), ("DOSBox.app", "DOSBox")],
        machines: &[Pc],
        formats: &[Com, Img],
        link: Link::None,
        website: "https://www.dosbox-staging.org",
    },
    Emulator {
        id: "qemu",
        name: "QEMU",
        programs: &["qemu-system-i386", "qemu-system-x86_64"],
        folder_hints: &["qemu"],
        mac_apps: &[],
        machines: &[Pc],
        formats: &[Img],
        link: Link::None,
        website: "https://www.qemu.org",
    },
    Emulator {
        id: "stella",
        name: "Stella",
        programs: &["Stella", "stella"],
        folder_hints: &["stella"],
        mac_apps: &[("Stella.app", "Stella")],
        machines: &[Atari2600],
        formats: &[A26],
        link: Link::None,
        website: "https://stella-emu.github.io",
    },
    // The Game Boy's: SameBoy, mGBA, Gambatte and BGB (on Windows), each given the cartridge.
    Emulator {
        id: "sameboy",
        name: "SameBoy",
        programs: &["sameboy", "SameBoy"],
        folder_hints: &["sameboy"],
        mac_apps: &[("SameBoy.app", "SameBoy")],
        machines: &[GameBoy],
        formats: &[Gb, Gbc],
        link: Link::None,
        website: "https://sameboy.github.io",
    },
    Emulator {
        id: "mgba",
        name: "mGBA",
        programs: &["mgba-qt", "mgba", "mGBA"],
        folder_hints: &["mgba"],
        mac_apps: &[("mGBA.app", "mGBA")],
        machines: &[GameBoy],
        formats: &[Gb, Gbc],
        link: Link::None,
        website: "https://mgba.io",
    },
    Emulator {
        id: "gambatte",
        name: "Gambatte",
        programs: &["gambatte_qt", "gambatte_sdl", "gambatte"],
        folder_hints: &["gambatte"],
        mac_apps: &[],
        machines: &[GameBoy],
        formats: &[Gb, Gbc],
        link: Link::None,
        website: "https://github.com/pokemon-speedrunning/gambatte-core",
    },
    Emulator {
        id: "bgb",
        name: "BGB",
        programs: &["bgb64", "bgb"],
        folder_hints: &["bgb"],
        mac_apps: &[],
        machines: &[GameBoy],
        formats: &[Gb, Gbc],
        link: Link::None,
        website: "https://bgb.bircd.org",
    },
    // The NES's: Mesen (which runs the Game Boy and the SNES too, each given the cartridge), FCEUX and Nestopia.
    Emulator {
        id: "mesen",
        name: "Mesen",
        programs: &["Mesen", "mesen"],
        folder_hints: &["mesen"],
        mac_apps: &[("Mesen.app", "Mesen")],
        machines: &[Machine::Nes, GameBoy, Snes],
        formats: &[Format::Nes, Gb, Gbc, Sfc, Smc],
        link: Link::None,
        website: "https://www.mesen.ca",
    },
    Emulator {
        id: "fceux",
        name: "FCEUX",
        programs: &["fceux", "fceux64", "qfceux"],
        folder_hints: &["fceux"],
        mac_apps: &[("fceux.app", "fceux")],
        machines: &[Machine::Nes],
        formats: &[Format::Nes],
        link: Link::None,
        website: "https://fceux.com",
    },
    Emulator {
        id: "nestopia",
        name: "Nestopia",
        programs: &["nestopia"],
        folder_hints: &["nestopia"],
        mac_apps: &[("Nestopia.app", "nestopia")],
        machines: &[Machine::Nes],
        formats: &[Format::Nes],
        link: Link::None,
        website: "http://0ldsk00l.ca/nestopia/",
    },
    // MAME's Atari 2600, NES, Mega Drive and SG-1000 need no ROMs of their own: the cartridge is all there is. Its
    // Master System is the Mark III, which has no BIOS either; its Vectrex, Game Gear and ColecoVision need theirs,
    // and its Game Boy the Game Boy's boot ROM. The Amstrad CPC, Apple IIe, Atari 7800 and SNES need theirs too
    // (cpc6128.zip, apple2e.zip, a7800.zip and snes.zip), as -listroms says.
    Emulator {
        id: "mame",
        name: "MAME",
        programs: &["mame", "mame64"],
        folder_hints: &["mame"],
        mac_apps: &[],
        machines: &[Atari2600, Vectrex, MegaDrive, MasterSystem, GameGear, Sg1000, ColecoVision, Machine::Nes, GameBoy, Ql, Cpc, Apple2, Atari7800, Snes],
        formats: &[A26, VecCart, MdRom, SmsRom, GgRom, SgRom, ColRom, Format::Nes, Gb, Gbc, QdosExe, Mdv, Dsk, Sna, Po, Woz, A78, Sfc],
        link: Link::None,
        // mamedev.org doesn't always answer; its releases on GitHub are the same files.
        website: "https://github.com/mamedev/mame/releases/latest",
    },
    // BlastEm, the most exact Mega Drive emulator, given the cartridge.
    Emulator {
        id: "blastem",
        name: "BlastEm",
        programs: &["blastem"],
        folder_hints: &["blastem"],
        mac_apps: &[("BlastEm.app", "blastem")],
        machines: &[MegaDrive],
        formats: &[MdRom],
        link: Link::None,
        website: "https://www.retrodev.com/blastem/",
    },
    // Emulicious: the Master System, Game Gear, SG-1000 and ColecoVision (and the Game Boy), with a debugger.
    Emulator {
        id: "emulicious",
        name: "Emulicious",
        programs: &["Emulicious", "emulicious"],
        folder_hints: &["emulicious"],
        mac_apps: &[],
        machines: &[MasterSystem, GameGear, Sg1000, ColecoVision],
        formats: &[SmsRom, GgRom, SgRom, ColRom],
        link: Link::None,
        website: "https://emulicious.net",
    },
    // Gearsystem and Gearcoleco, by the same author: the cartridge given to each.
    Emulator {
        id: "gearsystem",
        name: "Gearsystem",
        programs: &["gearsystem", "Gearsystem"],
        folder_hints: &["gearsystem"],
        mac_apps: &[("Gearsystem.app", "gearsystem")],
        machines: &[MasterSystem, GameGear, Sg1000],
        formats: &[SmsRom, GgRom, SgRom],
        link: Link::None,
        website: "https://github.com/drhelius/Gearsystem",
    },
    Emulator {
        id: "gearcoleco",
        name: "Gearcoleco",
        programs: &["gearcoleco", "Gearcoleco"],
        folder_hints: &["gearcoleco"],
        mac_apps: &[("Gearcoleco.app", "gearcoleco")],
        machines: &[ColecoVision],
        formats: &[ColRom],
        link: Link::None,
        website: "https://github.com/drhelius/Gearcoleco",
    },
    // The fantasy consoles themselves: PICO-8 is told to load the cart and run it; TIC-80 is given it to load,
    // with its start-up animation skipped.
    Emulator {
        id: "pico8",
        name: "PICO-8",
        programs: &["pico8"],
        folder_hints: &["pico-8", "pico8"],
        mac_apps: &[("PICO-8.app", "pico8")],
        machines: &[Pico8],
        formats: &[P8],
        link: Link::None,
        website: "https://www.lexaloffle.com/pico-8.php",
    },
    Emulator {
        id: "tic80",
        name: "TIC-80",
        programs: &["tic80"],
        folder_hints: &["tic-80", "tic80"],
        mac_apps: &[("tic80.app", "tic80")],
        machines: &[Tic80],
        formats: &[Tic, TicText],
        link: Link::None,
        website: "https://tic80.com",
    },
    // The Sinclair QL: sQLux runs a program from a folder made one of its drives, and a cartridge's files copied
    // into one; Q-emuLator (Windows and macOS) and MAME's QL (from 0.289) are given a cartridge, whose BOOT starts
    // it. See `ql_run`.
    Emulator {
        id: "sqlux",
        name: "sQLux",
        programs: &["sqlux", "sQLux"],
        folder_hints: &["sqlux"],
        mac_apps: &[],
        machines: &[Ql],
        formats: &[QdosExe, Mdv],
        link: Link::None,
        website: "https://github.com/SinclairQL/sQLux",
    },
    Emulator {
        id: "qemulator",
        name: "Q-emuLator",
        programs: &["Q-emuLator", "QemuLator", "qemulator"],
        folder_hints: &["q-emulator", "qemulator"],
        mac_apps: &[("Q-emuLator.app", "Q-emuLator")],
        machines: &[Ql],
        formats: &[QdosExe, Mdv],
        link: Link::None,
        website: "https://www.terdina.net/ql/q-emulator.html",
    },
    // RGC BASIC's desktop interpreter, given the program to run: basic-gfx (its window, with graphics and sound),
    // or basic (in a terminal) if that's all there is.
    Emulator {
        id: "rgc-basic",
        name: "RGC BASIC",
        programs: &["basic-gfx", "basic"],
        folder_hints: &["rgc-basic", "rgcbasic", "basic-windows", "basic-linux"],
        mac_apps: &[],
        machines: &[RgcBasic],
        formats: &[RgcBas],
        link: Link::None,
        website: "https://github.com/omiq/rgc-basic",
    },
    // ParaJVE, the Vectrex in Java, given the cartridge to load.
    Emulator {
        id: "parajve",
        name: "ParaJVE",
        programs: &["ParaJVE", "parajve"],
        folder_hints: &["parajve"],
        mac_apps: &[("ParaJVE.app", "ParaJVE")],
        machines: &[Vectrex],
        formats: &[VecCart],
        link: Link::None,
        website: "http://www.vectrex.fr/ParaJVE/",
    },
    // The Amstrad CPC: CPCEC runs what it's given (its CPCEC.TXT: "the emulator will try opening and running the
    // specified files"), typing RUN" and a disk's first program itself; Caprice32 is told what to type (its man
    // page, cap32.6: -a, --autocmd). Both come with the CPC's ROMs. MAME needs cpc6128.zip.
    Emulator {
        id: "cpcec",
        name: "CPCEC",
        programs: &["CPCEC", "cpcec"],
        folder_hints: &["cpcec"],
        mac_apps: &[],
        machines: &[Cpc],
        formats: &[Dsk, Sna, Cdt],
        link: Link::None,
        website: "http://cngsoft.no-ip.org/cpcec.htm",
    },
    Emulator {
        id: "caprice32",
        name: "Caprice32",
        programs: &["cap32", "caprice32"],
        folder_hints: &["cap32", "caprice"],
        mac_apps: &[("Caprice32.app", "cap32")],
        machines: &[Cpc],
        formats: &[Dsk, Sna, Cdt],
        link: Link::None,
        website: "https://github.com/ColinPitrat/caprice32",
    },
    // The Apple II: AppleWin (Windows), with its ROMs built in, boots a floppy from slot 6 (its help's Command
    // line page: -d1, and -s7 empty so a hard disk card in slot 7 doesn't boot first), or a larger ProDOS disk as
    // its hard disk (-h1). MAME's Apple IIe needs apple2e.zip.
    Emulator {
        id: "applewin",
        name: "AppleWin",
        programs: &["AppleWin", "Applewin"],
        folder_hints: &["applewin"],
        mac_apps: &[],
        machines: &[Apple2],
        formats: &[Dsk, Po, Woz],
        link: Link::None,
        website: "https://github.com/AppleWin/AppleWin",
    },
    // The Atari 8-bit computers: Altirra (Windows) runs a program, mounts a disk or plugs in a cartridge (its
    // /? help: /run, /disk, /cart); atari800 the same (its man page: -run, -cart, and disks as they are), with
    // Altirra's own free ROMs built in when the Atari's aren't there.
    Emulator {
        id: "altirra",
        name: "Altirra",
        programs: &["Altirra64", "AltirraARM64", "Altirra"],
        folder_hints: &["altirra"],
        mac_apps: &[],
        machines: &[Atari8],
        formats: &[Xex, Atr, Car],
        link: Link::None,
        website: "https://www.virtualdub.org/altirra.html",
    },
    Emulator {
        id: "atari800",
        name: "atari800",
        programs: &["atari800"],
        folder_hints: &["atari800"],
        mac_apps: &[],
        machines: &[Atari8],
        formats: &[Xex, Atr, Car],
        link: Link::None,
        website: "https://atari800.github.io",
    },
    // The Atari 7800: A7800, MAME's 7800 made exact, which needs no BIOS and keeps MAME's command line (its
    // README), given the system and the cartridge.
    Emulator {
        id: "a7800",
        name: "A7800",
        programs: &["a7800"],
        folder_hints: &["a7800"],
        mac_apps: &[],
        machines: &[Atari7800],
        formats: &[A78],
        link: Link::None,
        website: "https://github.com/7800-devtools/a7800",
    },
    // CP/M 2.2 as a program for a terminal: its drives are folders, A/0/ the first (see `cpm_run`). Limoniter starts
    // it in its Terminal panel, in a folder made for the run.
    Emulator {
        id: "runcpm",
        name: "RunCPM",
        programs: &["RunCPM", "runcpm"],
        folder_hints: &["runcpm"],
        mac_apps: &[],
        machines: &[Cpm],
        formats: &[CpmCom],
        link: Link::None,
        website: "https://github.com/MockbaTheBorg/RunCPM",
    },
    // The Agon Light with its own MOS and VDP firmware: given a folder as its SD card (see `agon_run`).
    Emulator {
        id: "fab-agon",
        name: "Fab Agon Emulator",
        programs: &["fab-agon-emulator"],
        folder_hints: &["fab-agon", "fab_agon"],
        mac_apps: &[],
        machines: &[Agon],
        formats: &[AgonBas],
        link: Link::None,
        website: "https://github.com/tomm/fab-agon-emulator",
    },
    // The Neo6502's, given the program's file, where it loads and where to start (see `neo_run`).
    Emulator {
        id: "neo6502",
        name: "Neo6502 emulator",
        programs: &["neo"],
        folder_hints: &["neo6502"],
        mac_apps: &[],
        machines: &[Neo6502],
        formats: &[NeoBin],
        link: Link::None,
        website: "https://github.com/paulscottrobson/neo6502-firmware",
    },
];

/// A real Next on DeZog's debug cable, standing where an emulator would in a session.
pub static NEXT_CABLE: Emulator = Emulator {
    id: "next-cable",
    name: "ZX Spectrum Next (debug cable)",
    programs: &[],
    folder_hints: &[],
    mac_apps: &[],
    machines: &[Next, Spectrum48, Spectrum128],
    formats: &[],
    link: Link::None,
    website: "https://github.com/maziac/dezogif",
};

pub fn by_id(id: &str) -> Option<&'static Emulator> {
    KNOWN.iter().find(|emulator| emulator.id == id)
}

impl Emulator {
    /// Whether it can run this, as it is.
    pub fn runs(&self, machine: Machine, format: Format) -> bool {
        self.machines.contains(&machine) && self.formats.contains(&format)
    }

    /// The program to start for a machine. VICE has one program per machine, side by side, so the one found is
    /// swapped for its neighbour.
    pub fn program_for(&self, found: &Path, machine: Machine) -> PathBuf {
        if self.id != "vice" {
            return found.to_path_buf();
        }
        let wanted = match machine {
            C128 => "x128",
            Vic20 => "xvic",
            Plus4 => "xplus4",
            Pet => "xpet",
            _ => return found.to_path_buf(),
        };
        let ending = found.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let sibling = found.with_file_name(format!("{wanted}{ending}"));
        if sibling.exists() { sibling } else { found.to_path_buf() }
    }

    /// The arguments that start it running a file on a machine, reachable on `port` if it can be reached. The
    /// person's own extra options come first, so that one that resets everything (as VICE's -default does) can't
    /// undo what Limoniter asks for, and one that has to be first (as ZEsarUX's --noconfigfile does) is.
    pub fn arguments(&self, machine: Machine, format: Format, file: &Path, port: Option<u16>, extra: &str) -> Vec<String> {
        let file = file.display().to_string();
        let mut args = split_options(extra);
        match self.id {
            "zesarux" => {
                let id = match machine {
                    Spectrum128 => "128k",
                    SpectrumPlus3 => "P341",
                    Pentagon => "Pentagon",
                    Next => "TBBlue",
                    Zx80 => "ZX80",
                    Zx81 => "ZX81",
                    JupiterAce => "ACE",
                    _ => "48k",
                };
                args.extend(["--machine".into(), id.into()]);
                if let Some(port) = port {
                    args.extend(["--enable-remoteprotocol".into(), "--remoteprotocol-port".into(), port.to_string()]);
                }
                args.push(file);
            }
            // OpticOS: its ROM in a 48K, and the tape inserted for LOAD "" to be typed, as Fuse's auto-load types
            // the Sinclair ROM's keyword.
            "fuse" if machine == OpticOs => {
                args.extend(["--machine".into(), "48".into(), "--rom-48".into(), crate::zx_machine::opticos_rom_file().display().to_string(), "--tape".into(), file]);
            }
            "fuse" => {
                let id = match machine {
                    Spectrum128 => "128",
                    SpectrumPlus3 => "plus3",
                    Pentagon => "pentagon",
                    _ => "48",
                };
                args.extend(["--machine".into(), id.into(), "--auto-load".into(), file]);
            }
            "cspect" => args.extend(["-zxnext".into(), "-w3".into(), file]),
            // Hatari makes the program's folder a drive and runs it, or boots from a floppy (see `st_run`).
            "hatari" => args.push(file),
            "vice" => {
                if let Some(port) = port {
                    args.extend(["-binarymonitor".into(), "-binarymonitoraddress".into(), format!("ip4://127.0.0.1:{port}")]);
                }
                // A program is put straight into memory and run, which needs no disk drive set up.
                if matches!(format, Prg | P00) {
                    args.extend(["-autostartprgmode".into(), "1".into()]);
                }
                args.extend(["-autostart".into(), file]);
            }
            "b2" => args.extend(["-0".into(), file, "-b".into()]),
            // XRoar needs the machine's BASIC ROMs, found in its ROM folder or named in the options. A Dragon DOS
            // disk needs the DragonDOS cartridge's ROM too, and has the command to run its program typed (see
            // `dragon_run`).
            "xroar" => {
                let id = match machine {
                    Dragon32 => "dragon32",
                    Dragon64 => "dragon64",
                    _ => "cocous",
                };
                args.extend(["-machine".into(), id.into()]);
                if let Some(port) = port {
                    args.extend(["-gdb".into(), "-gdb-port".into(), port.to_string()]);
                }
                if format == Vdk {
                    args.extend(crate::dragon_run::disk_arguments(Path::new(&file)));
                } else {
                    args.extend(["-run".into(), file]);
                }
            }
            "openmsx" => args.extend([if format == Cas { "-cassetteplayer" } else { "-carta" }.into(), file]),
            // The file is the configuration made for the run.
            "fs-uae" => args.push(file),
            "winuae" => args.extend(["-f".into(), file]),
            // Copperline's machine (the model and its Kickstart) is in the configuration made beside the program
            // or disk for the run (see `amiga_run`). A program is booted straight into with --run, its folder a
            // drive; a disk goes in DF0:.
            "copperline" => {
                let config = Path::new(&file).with_file_name(crate::amiga_run::COPPERLINE_CONFIG);
                args.extend(["--config".into(), config.display().to_string()]);
                if format == Hunk {
                    args.push("--run".into());
                }
                args.push(file);
            }
            // The CPC: a disk in its first drive, with RUN" and its program typed once it's started, or a snapshot.
            "mame" if machine == Cpc => args.extend(crate::cpc_run::mame_arguments(Path::new(&file), format)),
            // The Apple IIe boots the floppy in its first drive, from the Disk II card in slot 6.
            "mame" if machine == Apple2 => args.extend(["apple2e".into(), "-flop1".into(), file, "-skip_gameinfo".into()]),
            // The American 7800, or the European one for a cartridge whose header says PAL.
            "mame" | "a7800" if machine == Atari7800 => {
                let system = crate::atari7800::mame_system(&std::fs::read(&file).unwrap_or_default());
                args.extend([system.into(), "-cart".into(), file, "-skip_gameinfo".into()]);
            }
            "mame" if machine == Snes => args.extend(["snes".into(), "-cart".into(), file, "-skip_gameinfo".into()]),
            "fab-agon" => args.extend(crate::agon_run::arguments(Path::new(&file))),
            "neo6502" => args.extend(crate::neo_run::arguments(Path::new(&file))),
            "cpcec" => args.push(file),
            "caprice32" => args.extend(crate::cpc_run::caprice_arguments(Path::new(&file), format)),
            // A floppy boots from slot 6; a ProDOS disk larger than a floppy is the hard disk in slot 7.
            "applewin" if format == Po && std::fs::metadata(&file).is_ok_and(|m| m.len() > crate::running::APPLE_FLOPPY as u64) => args.extend(["-h1".into(), file]),
            "applewin" => args.extend(["-d1".into(), file, "-s7".into(), "empty".into()]),
            "altirra" => args.extend([match format { Car => "/cart", Atr => "/disk", _ => "/run" }.into(), file]),
            "atari800" if format == Xex => args.extend(["-run".into(), file]),
            "atari800" if format == Car => args.extend(["-cart".into(), file]),
            "atari800" => args.push(file),
            // The QL needs its ROMs (ql.zip); the cartridge goes in the first microdrive, and a script beside it
            // presses F1 at the QL's first screen so it boots (see `ql_run`).
            "mame" if format == Mdv => args.extend(crate::ql_run::mame_arguments(Path::new(&file))),
            // The run's folder is made sQLux's drive WIN8, and the program EXECed from it.
            "sqlux" => args.extend(crate::ql_run::sqlux_arguments(Path::new(&file))),
            // The Vectrex needs its BIOS ROM (vectrex.zip) in MAME's ROM folder; the 2600 needs none.
            "mame" => {
                let system = match format {
                    VecCart => "vectrex",
                    MdRom => "genesis",
                    // The Mark III: the Japanese Master System, which runs its cartridges with no BIOS to find.
                    SmsRom => "sg1000m3",
                    GgRom => "gamegear",
                    SgRom => "sg1000",
                    ColRom => "coleco",
                    Format::Nes => "nes",
                    // The Game Boy needs its boot ROM (gameboy.zip, or gbcolor.zip for the Color).
                    Gb => "gameboy",
                    Gbc => "gbcolor",
                    _ => "a2600",
                };
                args.extend([system.into(), "-cart".into(), file, "-skip_gameinfo".into()]);
            }
            // A .COM given to DOSBox is run, its folder made drive C:; a floppy is booted.
            "dosbox" if format == Img => args.extend(["-c".into(), format!("boot \"{file}\"")]),
            "dosbox" => args.push(file),
            "pico8" => args.extend(["-run".into(), file]),
            "tic80" => args.extend([file, "--skip".into()]),
            "qemu" => args.extend(["-fda".into(), file, "-boot".into(), "a".into()]),
            _ => args.push(file),
        }
        args
    }
}

/// Splits extra options as a shell would: on spaces, with double quotes keeping a value with spaces together.
pub fn split_options(extra: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in extra.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(current);
    }
    out
}

/// The emulators installed on this computer that Limoniter can find, with where each is.
pub fn find_installed() -> Vec<(&'static Emulator, PathBuf)> {
    let mut found = Vec::new();
    for emulator in KNOWN {
        if let Some(path) = find(emulator) {
            found.push((emulator, path));
        }
    }
    found
}

/// Any program, looked for where emulators are: assemblers use this too.
pub fn find_program(programs: &'static [&'static str], folder_hints: &'static [&'static str]) -> Option<PathBuf> {
    let seeker = Emulator { id: "", name: "", programs, folder_hints, mac_apps: &[], machines: &[], formats: &[], link: Link::None, website: "" };
    find(&seeker)
}

fn find(emulator: &Emulator) -> Option<PathBuf> {
    let mut path: Vec<PathBuf> = std::env::var_os("PATH").map(|paths| std::env::split_paths(&paths).collect()).unwrap_or_default();
    // Where package managers put programs, which a program started from the desktop may not have on its path:
    // Homebrew's folders on a Mac (an app opened from the Finder has only the system's), and the games folders
    // Debian's and Ubuntu's packages use for emulators such as MAME; and where pip puts programs, for vamos.
    let usual: &[&str] = if cfg!(target_os = "macos") { &["/opt/homebrew/bin", "/usr/local/bin"] } else if cfg!(windows) { &[] } else { &["/usr/games", "/usr/local/games"] };
    for folder in usual.iter().map(PathBuf::from).chain(crate::amiga_vamos::python_folders()) {
        if !path.contains(&folder) {
            path.push(folder);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let apps: Vec<PathBuf> = if cfg!(target_os = "macos") { [Some(PathBuf::from("/Applications")), home.map(|h| h.join("Applications"))].into_iter().flatten().collect() } else { Vec::new() };
    // Where Windows installers and unzipped downloads leave a program, in a folder of its own.
    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(variable) {
            let root = PathBuf::from(value);
            match variable {
                "LOCALAPPDATA" => roots.push(root.join("Programs")),
                "USERPROFILE" => roots.extend(["Downloads", "Desktop", "Emulators", "emulators"].map(|f| root.join(f))),
                _ => roots.push(root),
            }
        }
    }
    // Where Limoniter's own downloads go, each in a folder named for its emulator.
    roots.extend(crate::emulator_get::folder());
    if cfg!(windows) {
        roots.extend(["C:\\Emulators", "C:\\emulators", "C:\\Games", "D:\\Emulators", "E:\\emulators"].map(PathBuf::from));
        // The top of each of the computer's own drives, where a program with no installer, such as MAME, is often
        // unpacked: C:\MAME, or D:\mame0289b_64bit.
        roots.extend(fixed_drives());
    }
    find_in(emulator, &path, &apps, &roots, cfg!(windows))
}

/// The tops of the computer's own fixed drives (C:\, D:\ and so on), from the drives Windows says there are,
/// without touching any: card readers, DVD drives and network drives are left out, as an empty one can make
/// Windows ask for a disk and an asleep one can take many seconds to answer.
#[cfg(windows)]
fn fixed_drives() -> Vec<PathBuf> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows::core::PCWSTR;
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|n| mask & (1 << n) != 0)
        .map(|n| format!("{}:\\", (b'A' + n) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            // 3 is a fixed drive.
            unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) == 3 }
        })
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
fn fixed_drives() -> Vec<PathBuf> {
    Vec::new()
}

/// Looks for an emulator: on the path, as a package manager puts it; as an app, in the Mac's Applications
/// folders; in each root itself, where a release unpacked where it was downloaded leaves its program (MAME's
/// unpacks into the folder it's in, Downloads, unless told otherwise); and one or two levels down from each root,
/// in a folder whose name says what it is (the name is matched whatever its capitals: MAME, mame0289b_64bit).
fn find_in(emulator: &Emulator, path: &[PathBuf], apps: &[PathBuf], roots: &[PathBuf], windows: bool) -> Option<PathBuf> {
    let exe = |name: &str| if windows { format!("{name}.exe") } else { name.to_string() };
    for folder in path {
        for program in emulator.programs {
            let candidate = folder.join(exe(program));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for root in apps {
        for (app, program) in emulator.mac_apps {
            let candidate = root.join(app).join("Contents/MacOS").join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for root in roots {
        for program in emulator.programs {
            let candidate = root.join(exe(program));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        folders.sort();
        for folder in folders {
            let name = folder.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !emulator.folder_hints.iter().any(|hint| name.contains(hint)) {
                continue;
            }
            // FS-UAE keeps its Windows program a little further down.
            for inside in [folder.clone(), folder.join("bin"), folder.join("Windows").join("x86-64")] {
                for program in emulator.programs {
                    let candidate = inside.join(exe(program));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_emulator_is_started_as_its_own_help_says() {
        let file = Path::new("/tmp/game.tap");
        let zesarux = by_id("zesarux").unwrap();
        assert_eq!(zesarux.arguments(Spectrum128, Tap, file, Some(10000), ""), ["--machine", "128k", "--enable-remoteprotocol", "--remoteprotocol-port", "10000", "/tmp/game.tap"]);
        assert_eq!(zesarux.arguments(Zx80, O, Path::new("/tmp/a.o"), None, ""), ["--machine", "ZX80", "/tmp/a.o"]);
        let eightyone = by_id("eightyone").unwrap();
        assert!(eightyone.runs(Zx80, O) && eightyone.runs(Zx81, P) && !eightyone.runs(Spectrum48, Tap));
        assert_eq!(eightyone.arguments(Zx80, O, Path::new("C:\\run\\a.o"), None, ""), ["C:\\run\\a.o"]);
        assert_eq!(by_id("fuse").unwrap().arguments(SpectrumPlus3, Dsk, Path::new("/tmp/d.dsk"), None, "--full-screen"), ["--full-screen", "--machine", "plus3", "--auto-load", "/tmp/d.dsk"]);
        let vice = by_id("vice").unwrap();
        assert_eq!(vice.arguments(C64, Prg, Path::new("/tmp/a.prg"), Some(6502), "-default"), ["-default", "-binarymonitor", "-binarymonitoraddress", "ip4://127.0.0.1:6502", "-autostartprgmode", "1", "-autostart", "/tmp/a.prg"], "-default can't switch the monitor off");
        assert_eq!(by_id("b2").unwrap().arguments(Bbc, Ssd, Path::new("/tmp/a.ssd"), None, ""), ["-0", "/tmp/a.ssd", "-b"]);
        assert_eq!(by_id("openmsx").unwrap().arguments(Msx, Cas, Path::new("/tmp/a.cas"), None, "-machine Philips_NMS_8250"), ["-machine", "Philips_NMS_8250", "-cassetteplayer", "/tmp/a.cas"]);
        assert_eq!(by_id("cspect").unwrap().arguments(Next, Nex, Path::new("/tmp/a.nex"), None, ""), ["-zxnext", "-w3", "/tmp/a.nex"]);
        assert_eq!(by_id("fs-uae").unwrap().arguments(Amiga, Hunk, Path::new("/tmp/amiga.fs-uae"), None, "--fullscreen=1"), ["--fullscreen=1", "/tmp/amiga.fs-uae"]);
        assert_eq!(by_id("winuae").unwrap().arguments(Amiga, Hunk, Path::new("C:\\run\\amiga.uae"), None, ""), ["-f", "C:\\run\\amiga.uae"]);
        // Copperline: the configuration made beside the program, and the program warp-booted with --run; a disk is
        // simply given, for DF0:.
        let copperline = by_id("copperline").unwrap();
        assert!(copperline.runs(Amiga, Hunk) && copperline.runs(Amiga, Adf));
        // Paths with / whichever system joined them (Windows joins with \\).
        let slashes = |args: Vec<String>| args.into_iter().map(|a| a.replace('\\', "/")).collect::<Vec<_>>();
        assert_eq!(slashes(copperline.arguments(Amiga, Hunk, Path::new("/tmp/run/demo"), None, "--window-scale 2")), ["--window-scale", "2", "--config", "/tmp/run/copperline.toml", "--run", "/tmp/run/demo"]);
        assert_eq!(slashes(copperline.arguments(Amiga, Adf, Path::new("/tmp/run/game.adf"), None, "")), ["--config", "/tmp/run/copperline.toml", "/tmp/run/game.adf"]);
        // The Game Boy's and the NES's are given the cartridge; MAME is told which machine it's for.
        for id in ["sameboy", "mgba", "gambatte", "bgb", "mesen"] {
            assert_eq!(by_id(id).unwrap().arguments(GameBoy, Gb, Path::new("/tmp/a.gb"), None, ""), ["/tmp/a.gb"], "{id}");
        }
        for id in ["mesen", "fceux", "nestopia"] {
            assert!(by_id(id).unwrap().runs(Machine::Nes, Format::Nes), "{id}");
            assert_eq!(by_id(id).unwrap().arguments(Machine::Nes, Format::Nes, Path::new("/tmp/a.nes"), None, ""), ["/tmp/a.nes"], "{id}");
        }
        let mame = by_id("mame").unwrap();
        assert_eq!(mame.arguments(GameBoy, Gbc, Path::new("/tmp/a.gbc"), None, ""), ["gbcolor", "-cart", "/tmp/a.gbc", "-skip_gameinfo"]);
        assert_eq!(mame.arguments(Atari2600, A26, Path::new("/tmp/a.a26"), None, ""), ["a2600", "-cart", "/tmp/a.a26", "-skip_gameinfo"]);
        // The QL: MAME's cartridge in its first microdrive, with the script that presses F1; sQLux's folder made its
        // drive WIN8, and the program EXECed from it. The person's own options still come first.
        assert!(mame.runs(Ql, Mdv) && mame.runs(Ql, QdosExe));
        assert_eq!(slashes(mame.arguments(Ql, Mdv, Path::new("/tmp/run/hello.mdv"), None, "")), ["ql", "-microtape1", "/tmp/run/hello.mdv", "-skip_gameinfo", "-autoboot_delay", "5", "-autoboot_script", "/tmp/run/ql_boot.lua"]);
        let sqlux = by_id("sqlux").unwrap();
        assert!(sqlux.runs(Ql, QdosExe) && sqlux.runs(Ql, Mdv) && !sqlux.runs(AtariSt, Tos));
        assert_eq!(slashes(sqlux.arguments(Ql, QdosExe, Path::new("/tmp/run/hello_exe"), None, "-w 2x")), ["-w", "2x", "--device", "WIN8,/tmp/run/,qdos-like", "-b", "EXEC win8_hello_exe"]);
        assert_eq!(slashes(sqlux.arguments(Ql, QdosExe, Path::new("/tmp/run/BOOT"), None, ""))[3], "LRUN win8_BOOT");
        assert_eq!(by_id("qemulator").unwrap().arguments(Ql, Mdv, Path::new("C:\\run\\hello.mdv"), None, ""), ["C:\\run\\hello.mdv"]);
    }

    #[test]
    fn the_cpc_apple_ii_atari_and_snes_emulators_are_started_as_their_documents_say() {
        let base = std::env::temp_dir().join(format!("limoniter-machines-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let slashes = |args: Vec<String>| args.into_iter().map(|a| a.replace('\\', "/")).collect::<Vec<_>>();
        // The CPC: CPCEC is given the file; Caprice32 and MAME are told what to type for a disk.
        let (_, disk) = crate::cpc_run::disk_holding("game", 0x4000, 0x4000, &[0xC9]).unwrap();
        let dsk = base.join("game.dsk");
        std::fs::write(&dsk, &disk).unwrap();
        let named = dsk.display().to_string();
        for id in ["cpcec", "caprice32", "mame"] {
            let emulator = by_id(id).unwrap();
            assert!(emulator.runs(Cpc, Dsk) && emulator.runs(Cpc, Sna), "{id}");
        }
        assert!(by_id("cpcec").unwrap().runs(Cpc, Cdt) && !by_id("mame").unwrap().runs(Cpc, Cdt), "MAME's CPC tape needs PLAY pressed in its menu");
        assert_eq!(by_id("cpcec").unwrap().arguments(Cpc, Dsk, &dsk, None, "-m2"), ["-m2", &named]);
        assert_eq!(by_id("caprice32").unwrap().arguments(Cpc, Dsk, &dsk, None, ""), ["-a", "run\"GAME", &named]);
        let mame = by_id("mame").unwrap();
        assert_eq!(mame.arguments(Cpc, Dsk, &dsk, None, ""), ["cpc6128", "-flop1", &named, "-skip_gameinfo", "-autoboot_delay", "2", "-autoboot_command", "run\"GAME\\n"]);
        // An Apple II floppy: AppleWin boots it from slot 6; a larger ProDOS disk is its hard disk. MAME's IIe.
        let applewin = by_id("applewin").unwrap();
        assert!(applewin.runs(Apple2, Dsk) && applewin.runs(Apple2, Po) && applewin.runs(Apple2, Woz));
        assert_eq!(slashes(applewin.arguments(Apple2, Dsk, Path::new("C:\\run\\a.dsk"), None, "")), ["-d1", "C:/run/a.dsk", "-s7", "empty"]);
        let big = base.join("big.po");
        std::fs::write(&big, vec![0u8; 819_200]).unwrap();
        assert_eq!(applewin.arguments(Apple2, Po, &big, None, ""), ["-h1", &big.display().to_string()]);
        assert_eq!(mame.arguments(Apple2, Woz, Path::new("/tmp/a.woz"), None, ""), ["apple2e", "-flop1", "/tmp/a.woz", "-skip_gameinfo"]);
        // The Atari 8-bits: a program run, a disk booted, a cartridge plugged in.
        let altirra = by_id("altirra").unwrap();
        assert_eq!(altirra.arguments(Atari8, Xex, Path::new("/tmp/a.xex"), None, ""), ["/run", "/tmp/a.xex"]);
        assert_eq!(altirra.arguments(Atari8, Atr, Path::new("/tmp/a.atr"), None, "/portable"), ["/portable", "/disk", "/tmp/a.atr"]);
        assert_eq!(altirra.arguments(Atari8, Car, Path::new("/tmp/a.car"), None, ""), ["/cart", "/tmp/a.car"]);
        let atari800 = by_id("atari800").unwrap();
        assert_eq!(atari800.arguments(Atari8, Xex, Path::new("/tmp/a.xex"), None, "-xl"), ["-xl", "-run", "/tmp/a.xex"]);
        assert_eq!(atari800.arguments(Atari8, Car, Path::new("/tmp/a.car"), None, ""), ["-cart", "/tmp/a.car"]);
        assert_eq!(atari800.arguments(Atari8, Atr, Path::new("/tmp/a.atr"), None, ""), ["/tmp/a.atr"]);
        // The 7800: the American system, or the European one when the cartridge's header says PAL.
        let mut header = crate::atari7800::Header { version: 3, title: "x".into(), rom_size: 0x1000, cart_type: 0, pal: false }.write().to_vec();
        header.resize(128 + 0x1000, 0);
        let ntsc = base.join("ntsc.a78");
        std::fs::write(&ntsc, &header).unwrap();
        header[57] = 1;
        let pal = base.join("pal.a78");
        std::fs::write(&pal, &header).unwrap();
        for id in ["a7800", "mame"] {
            let emulator = by_id(id).unwrap();
            assert!(emulator.runs(Atari7800, A78), "{id}");
            assert_eq!(emulator.arguments(Atari7800, A78, &ntsc, None, ""), ["a7800", "-cart", &ntsc.display().to_string(), "-skip_gameinfo"], "{id}");
            assert_eq!(emulator.arguments(Atari7800, A78, &pal, None, "")[0], "a7800p", "{id}");
        }
        // The SNES: Mesen is given the cartridge, either kind; MAME takes an .sfc.
        let mesen = by_id("mesen").unwrap();
        assert!(mesen.runs(Snes, Sfc) && mesen.runs(Snes, Smc));
        assert_eq!(mesen.arguments(Snes, Smc, Path::new("/tmp/a.smc"), None, ""), ["/tmp/a.smc"]);
        assert_eq!(mame.arguments(Snes, Sfc, Path::new("/tmp/a.sfc"), None, ""), ["snes", "-cart", "/tmp/a.sfc", "-skip_gameinfo"]);
        assert!(!mame.runs(Snes, Smc));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn emulators_are_found_where_they_are_usually_installed() {
        let base = std::env::temp_dir().join(format!("limoniter-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let make = |path: PathBuf| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        };
        // VICE unzipped into Downloads, its programs in bin; ZEsarUX on the path; Fuse as a Mac app.
        make(base.join("Downloads/GTK3VICE-3.8-win64/bin/x64sc.exe"));
        make(base.join("path/zesarux"));
        make(base.join("Applications/Fuse.app/Contents/MacOS/Fuse"));
        make(base.join("Downloads/Unrelated/b2.exe"));
        let roots = [base.join("Downloads")];
        let vice = find_in(by_id("vice").unwrap(), &[], &[], &roots, true);
        assert_eq!(vice, Some(base.join("Downloads/GTK3VICE-3.8-win64/bin/x64sc.exe")));
        assert_eq!(find_in(by_id("zesarux").unwrap(), &[base.join("path")], &[], &roots, false), Some(base.join("path/zesarux")));
        assert_eq!(find_in(by_id("fuse").unwrap(), &[], &[base.join("Applications")], &roots, false), Some(base.join("Applications/Fuse.app/Contents/MacOS/Fuse")));
        assert_eq!(find_in(by_id("b2").unwrap(), &[], &[], &roots, true), None, "a program in a folder named for something else isn't taken for it");
        assert_eq!(find_in(by_id("zesarux").unwrap(), &[], &[], &roots, true), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn mame_is_found_where_it_was_unpacked_by_hand() {
        let base = std::env::temp_dir().join(format!("limoniter-find-mame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let make = |path: PathBuf| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        };
        let mame = by_id("mame").unwrap();
        // At the top of a drive, in a folder named as MAME's release is, with capitals.
        make(base.join("D/Windows/notepad.exe"));
        make(base.join("D/MAME0289b_64bit/mame.exe"));
        assert_eq!(find_in(mame, &[], &[], &[base.join("C"), base.join("D")], true), Some(base.join("D/MAME0289b_64bit/mame.exe")));
        // Unpacked straight into Downloads, where the release was, as it is unless told otherwise.
        make(base.join("Downloads/mame.exe"));
        make(base.join("Downloads/mame0289b_x64.exe"));
        assert_eq!(find_in(mame, &[], &[], &[base.join("Downloads")], true), Some(base.join("Downloads/mame.exe")));
        // Not a program named for another emulator, in a folder with MAME's name.
        make(base.join("E/mame-tools/stella.exe"));
        assert_eq!(find_in(by_id("stella").unwrap(), &[], &[], &[base.join("E")], true), None);
        // Folder hints are written small, as folder names are made small to match them.
        for emulator in KNOWN {
            assert!(emulator.folder_hints.iter().all(|h| *h == h.to_lowercase()), "{}", emulator.id);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extra_options_split_as_a_shell_would() {
        assert_eq!(split_options(r#" -mmc="C:\My SD\card.img"  -nextrom "#), ["-mmc=C:\\My SD\\card.img", "-nextrom"]);
        assert!(split_options("   ").is_empty());
    }

    #[test]
    fn vice_starts_the_program_for_the_machine() {
        let dir = std::env::temp_dir().join(format!("limoniter-vice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ending = if cfg!(windows) { ".exe" } else { "" };
        let x64sc = dir.join(format!("x64sc{ending}"));
        std::fs::write(&x64sc, "").unwrap();
        std::fs::write(dir.join(format!("xvic{ending}")), "").unwrap();
        let vice = by_id("vice").unwrap();
        assert_eq!(vice.program_for(&x64sc, Vic20), dir.join(format!("xvic{ending}")));
        assert_eq!(vice.program_for(&x64sc, C128), x64sc, "with no x128 beside it, the one found is used");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
