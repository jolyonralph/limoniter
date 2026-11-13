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
        machines: &[Spectrum48, Spectrum128, SpectrumPlus3, Pentagon, Next],
        formats: &[Tap, Tzx, Z80, Sna, Nex, Dsk, Trd, Scl],
        link: Link::Zrcp,
        website: "https://github.com/chernandezba/zesarux",
    },
    Emulator {
        id: "fuse",
        name: "Fuse",
        programs: &["fuse", "fuse-sdl", "fuse-gtk"],
        folder_hints: &["fuse"],
        mac_apps: &[("Fuse.app", "Fuse")],
        machines: &[Spectrum48, Spectrum128, SpectrumPlus3, Pentagon],
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
];

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
                    _ => "48k",
                };
                args.extend(["--machine".into(), id.into()]);
                if let Some(port) = port {
                    args.extend(["--enable-remoteprotocol".into(), "--remoteprotocol-port".into(), port.to_string()]);
                }
                args.push(file);
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
            "openmsx" => args.extend([if format == Cas { "-cassetteplayer" } else { "-carta" }.into(), file]),
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

fn find(emulator: &Emulator) -> Option<PathBuf> {
    let exe = |name: &str| if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    // On the path, as a package manager puts it.
    if let Some(paths) = std::env::var_os("PATH") {
        for folder in std::env::split_paths(&paths) {
            for program in emulator.programs {
                let candidate = folder.join(exe(program));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    // As an app, on a Mac.
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        for root in [Some(PathBuf::from("/Applications")), home.map(|h| h.join("Applications"))].into_iter().flatten() {
            for (app, program) in emulator.mac_apps {
                let candidate = root.join(app).join("Contents/MacOS").join(program);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    // In a folder of its own, as Windows installers and unzipped downloads leave it: one or two levels down from
    // the usual places, in a folder whose name says what it is.
    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(variable) {
            let root = PathBuf::from(value);
            if variable == "LOCALAPPDATA" {
                roots.push(root.join("Programs"));
            } else if variable == "USERPROFILE" {
                roots.extend(["Downloads", "Desktop", "Emulators", "emulators"].map(|f| root.join(f)));
            } else {
                roots.push(root);
            }
        }
    }
    if cfg!(windows) {
        roots.extend(["C:\\Emulators", "C:\\emulators", "C:\\Games", "D:\\Emulators", "E:\\emulators"].map(PathBuf::from));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for folder in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            let name = folder.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !emulator.folder_hints.iter().any(|hint| name.contains(hint)) {
                continue;
            }
            for inside in [folder.clone(), folder.join("bin")] {
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
        assert_eq!(by_id("fuse").unwrap().arguments(SpectrumPlus3, Dsk, Path::new("/tmp/d.dsk"), None, "--full-screen"), ["--full-screen", "--machine", "plus3", "--auto-load", "/tmp/d.dsk"]);
        let vice = by_id("vice").unwrap();
        assert_eq!(vice.arguments(C64, Prg, Path::new("/tmp/a.prg"), Some(6502), "-default"), ["-default", "-binarymonitor", "-binarymonitoraddress", "ip4://127.0.0.1:6502", "-autostartprgmode", "1", "-autostart", "/tmp/a.prg"], "-default can't switch the monitor off");
        assert_eq!(by_id("b2").unwrap().arguments(Bbc, Ssd, Path::new("/tmp/a.ssd"), None, ""), ["-0", "/tmp/a.ssd", "-b"]);
        assert_eq!(by_id("openmsx").unwrap().arguments(Msx, Cas, Path::new("/tmp/a.cas"), None, "-machine Philips_NMS_8250"), ["-machine", "Philips_NMS_8250", "-cassetteplayer", "/tmp/a.cas"]);
        assert_eq!(by_id("cspect").unwrap().arguments(Next, Nex, Path::new("/tmp/a.nex"), None, ""), ["-zxnext", "-w3", "/tmp/a.nex"]);
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
