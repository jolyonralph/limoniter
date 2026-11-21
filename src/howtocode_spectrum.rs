//! How To Code for the ZX Spectrum: the 48K, 128K and Next coder's guide, with its examples, in Limoniter. Made as
//! How To Code v8 (see howtocode.rs) is: its files are in the program (put there by build.rs from
//! data/howtocode-spectrum), unpacked into Limoniter's data folder to be read, and its examples copied into a folder
//! of the person's own.

use std::path::{Path, PathBuf};

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/howtocode_spectrum_files.rs"));
}

/// Every file of the guide: its path in the guide, and its bytes.
pub fn files() -> &'static [(&'static str, &'static [u8])] {
    bundled::FILES
}

/// The guide's folder, unpacked in Limoniter's data folder.
pub fn unpacked() -> Result<PathBuf, String> {
    let folder = crate::settings::data_folder().ok_or_else(|| crate::i18n::tr("There's no folder to keep Limoniter's files in.").to_string())?.join("howtocode-spectrum");
    crate::howtocode::unpack_files_to(files(), &folder)?;
    Ok(folder)
}

/// The examples, and the startup they include, copied into a folder to start from. Files already there are kept as
/// they are. Returns how many were copied.
pub fn copy_examples(to: &Path) -> Result<usize, String> {
    crate::howtocode::copy_files_examples(files(), to)
}

fn example(name: &str) -> String {
    let path = format!("examples/{name}");
    let bytes = files().iter().find(|(file, _)| *file == path).map(|(_, bytes)| *bytes).unwrap_or_default();
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// The smallest whole demo, startup_demo.asm, as one file: the startup put in where it's included, marked, so it
/// builds with F9 with nothing beside it. What File, New as starts a Spectrum demo with. Its comments are the
/// guide's, in English, as the guide is.
pub fn single_file_demo() -> String {
    let mut out = String::new();
    for (n, line) in example("startup_demo.asm").lines().enumerate() {
        if n == 0 {
            out.push_str("; One file: How To Code's startup_demo.asm for the ZX Spectrum, with startup.asm put in where it's\n");
            out.push_str("; included. Help, How To Code: the ZX Spectrum explains every part; Tools, ZX Spectrum and Next copies its examples.\n");
        }
        let included = line.trim().strip_prefix("include").map(|rest| rest.trim().trim_matches('"').to_string()).filter(|name| name.ends_with(".asm"));
        match included {
            Some(name) => {
                out.push_str(&format!("\n; ---- {name}, as How To Code has it ----\n\n"));
                out.push_str(example(&name).trim_end());
                out.push_str(&format!("\n\n; ---- the end of {name} ----\n\n"));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::running::{Format, Machine, Runnable};
    use crate::zx_machine::BuiltIn;

    #[test]
    fn the_guide_is_in_the_program_and_unpacks() {
        assert!(files().iter().any(|(name, _)| *name == "README.md"));
        assert!(files().iter().any(|(name, _)| *name == "examples/startup.asm"));
        let folder = std::env::temp_dir().join(format!("limoniter-htczx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        crate::howtocode::unpack_files_to(files(), &folder).unwrap();
        assert!(folder.join("04-startup-and-interrupts.md").is_file());
        assert!(folder.join("examples/startup_demo.asm").is_file());
        let project = folder.join("mine");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("startup_demo.asm"), "my own").unwrap();
        assert!(copy_examples(&project).unwrap() > 3);
        assert_eq!(std::fs::read_to_string(project.join("startup_demo.asm")).unwrap(), "my own");
        assert!(project.join("startup.asm").is_file());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_guide_links_to_files_that_are_there() {
        crate::howtocode::check_links(files());
    }

    fn examples() -> Vec<PathBuf> {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/howtocode-spectrum/examples");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "asm") && !p.ends_with("startup.asm")).collect();
        found.sort();
        found
    }

    fn text_of(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    /// Whether an example is for the Next, the 128K only, or any Spectrum, as its DEVICE says.
    fn device_of(text: &str) -> &'static str {
        let device = text.lines().find_map(|line| {
            let mut words = line.split(';').next().unwrap_or("").split_whitespace();
            words.next().filter(|w| w.eq_ignore_ascii_case("device")).and(words.next()).map(str::to_ascii_lowercase)
        });
        match device.as_deref() {
            Some("zxspectrumnext") => "next",
            Some("zxspectrum128") => "128",
            _ => "48",
        }
    }

    /// An example made as `limoniter --assemble` makes it, with its settings given these values.
    fn build_with(path: &Path, ending: &str, values: &[(&str, &str)]) -> Vec<u8> {
        let values: Vec<(String, String)> = values.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        let output = PathBuf::from(format!("out.{ending}"));
        crate::cli_assemble::assemble_for(path, &crate::cli_assemble::set_values(&text_of(path), &values), &output).unwrap_or_else(|problems| panic!("{}: {problems:?}", path.display()))
    }

    /// Every example assembles: a Spectrum's into a tape, a Next's into a .nex, with its code below the stack.
    #[test]
    fn every_example_assembles() {
        assert!(examples().len() > 5);
        for path in examples() {
            let text = text_of(&path);
            assert!(text.lines().any(|line| line.starts_with("AUTOEXIT")), "{} has its AUTOEXIT line", path.display());
            if device_of(&text) == "next" {
                assert!(build_with(&path, "nex", &[]).starts_with(b"NextV1.2"), "{}", path.display());
                continue;
            }
            let tape = build_with(&path, "tap", &[]);
            assert_eq!(&tape[..4], &[19, 0, 0, 0], "{} is a tape with a loader", path.display());
            let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, &path, &text).assembly;
            let highest = assembly.chunks.iter().filter(|c| c.address < 0xC000).map(|c| c.address + c.bytes.len() as i64).max().unwrap_or(0);
            assert!(highest <= 0xBC00, "{}'s program reaches ${highest:04X}, into the startup's stack", path.display());
        }
    }

    /// The single-file demo includes nothing, holds the startup whole, and builds into the same tape as
    /// startup_demo.asm with its include.
    #[test]
    fn the_single_file_demo_needs_nothing_beside_it() {
        let single = single_file_demo();
        assert!(!single.lines().any(|line| line.trim_start().starts_with("include")), "no include left");
        assert!(single.contains(example("startup.asm").trim_end()));
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/howtocode-spectrum/examples");
        let alone = std::env::temp_dir().join(format!("limoniter-zxsingle-{}", std::process::id())).join("demo.asm");
        let one = crate::cli_assemble::assemble_for(&alone, &single, Path::new("demo.tap")).unwrap();
        let split = build_with(&folder.join("startup_demo.asm"), "tap", &[]);
        // The tapes' names differ (each is named for its file); the code, their last block, is the same.
        let code = |tape: &[u8]| crate::basic::tape_blocks(tape).0.last().map(|block| block.to_vec());
        assert!(code(&one).is_some_and(|block| block.len() > 300));
        assert_eq!(code(&one), code(&split));
    }

    /// An example's tape run on the built-in Spectrum until it goes back to BASIC by itself (AUTOEXIT), or for
    /// `frames` if it's not to; returns the machine, and whether its main loop was reached.
    pub(crate) fn run_example(path: &Path, machine: Machine, frames: usize, autoexit: Option<usize>) -> (BuiltIn, bool) {
        let values: Vec<(&str, String)> = autoexit.map(|n| ("AUTOEXIT", n.to_string())).into_iter().collect();
        let values: Vec<(&str, &str)> = values.iter().map(|(n, v)| (*n, v.as_str())).collect();
        let tape = build_with(path, "tap", &values);
        let run = Runnable { machine, format: Format::Tap, name: "t.tap".into(), bytes: std::sync::Arc::new(tape) };
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, path, &text_of(path)).assembly;
        let main_loop = assembly.symbols["MainLoop"] as u16;
        let mut spectrum = BuiltIn::start_heard(&run).unwrap();
        spectrum.set_breakpoints([main_loop]);
        // Long enough to load a tape at the tape's own speed: a screen and the code take about 45 seconds.
        let mut reached = false;
        for _ in 0..50 * 120 {
            spectrum.frame().unwrap();
            if spectrum.stopped.is_some() {
                reached = true;
                break;
            }
        }
        spectrum.set_breakpoints([]);
        spectrum.carry_on();
        for _ in 0..frames {
            spectrum.frame().unwrap();
        }
        (spectrum, reached)
    }

    /// The example's Interrupt starts at the same time in every frame, to within the 3 T-states HALT allows, on both
    /// machines: what beam racing needs.
    /// Chapter 4 gives the times this finds.
    #[test]
    fn the_interrupt_routine_starts_at_the_same_time_every_frame() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/howtocode-spectrum/examples/startup_demo.asm");
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, &path, &text_of(&path)).assembly;
        let interrupt = assembly.symbols["Interrupt"] as u16;
        for (machine, expected) in [(Machine::Spectrum48, 170..=180), (Machine::Spectrum128, 170..=180)] {
            let (mut spectrum, reached) = run_example(&path, machine, 5, None);
            assert!(reached);
            spectrum.time(interrupt);
            let mut times = Vec::new();
            for _ in 0..10 {
                spectrum.frame().unwrap();
                times.push(spectrum.reached());
            }
            eprintln!("{machine:?}: Interrupt at {times:?}");
            // The HALT before it is a NOP over and over, 4 T-states each, so where it began moves the start by up to 3.
            let (low, high) = (times.iter().flatten().min().unwrap(), times.iter().flatten().max().unwrap());
            assert!(high - low <= 3, "{times:?}");
            assert!(times[0].is_some_and(|t| expected.contains(&t)), "{machine:?}: {times:?}");
        }
    }

    /// Writes each example to target/howtocode-spectrum, to run in an emulator: its tape (or .nex) as it is, and
    /// for the 48K and 128K, snapshots of it loaded on each and about to start, ending by itself after 500 frames.
    #[test]
    #[ignore]
    fn write_the_examples() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/howtocode-spectrum");
        std::fs::create_dir_all(&out).unwrap();
        for path in examples() {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let device = device_of(&text_of(&path));
            if device == "next" {
                std::fs::write(out.join(format!("{name}.nex")), build_with(&path, "nex", &[])).unwrap();
                continue;
            }
            std::fs::write(out.join(format!("{name}.tap")), build_with(&path, "tap", &[])).unwrap();
            let tape = build_with(&path, "tap", &[("AUTOEXIT", "500")]);
            std::fs::write(out.join(format!("{name}-exit.tap")), &tape).unwrap();
            let machines: &[(Machine, &str)] = if device == "128" { &[(Machine::Spectrum128, "128")] } else { &[(Machine::Spectrum48, "48"), (Machine::Spectrum128, "128")] };
            for (machine, suffix) in machines {
                let run = Runnable { machine: *machine, format: Format::Tap, name: "t.tap".into(), bytes: std::sync::Arc::new(tape.clone()) };
                // A tape that takes over a minute to load at the tape's speed (ch13's, with a screen) has none.
                if let Ok(snapshot) = BuiltIn::snapshot_at(&run, 0x8000) {
                    std::fs::write(out.join(format!("{name}-{suffix}.sna")), snapshot).unwrap();
                }
            }
        }
    }

    /// The input example lights up a key while it's held down: Q, in the half-row read from port $FBFE.
    #[test]
    fn the_input_example_shows_a_key_held_down() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/howtocode-spectrum/examples/ch09_input.asm");
        let (mut spectrum, reached) = run_example(&path, Machine::Spectrum48, 5, None);
        assert!(reached);
        let q = 0x5800 + 4 * 32 + 8;
        assert_eq!(spectrum.peek(q), 0b0011_1000, "Q not lit before it's pressed");
        spectrum.key(rustzx_core::zx::keys::ZXKey::Q, true);
        for _ in 0..3 {
            spectrum.frame().unwrap();
        }
        assert_eq!(spectrum.peek(q), 0b0101_0111, "Q lit");
        assert_eq!(spectrum.peek(q + 5), 0b0011_1000, "W not");
        spectrum.key(rustzx_core::zx::keys::ZXKey::Q, false);
        for _ in 0..3 {
            spectrum.frame().unwrap();
        }
        assert_eq!(spectrum.peek(q), 0b0011_1000, "Q let go");
    }

    /// Whether BASIC's 0 OK is on the screen, read in the 48K ROM's font: a 128K's editor may have its own ROM
    /// paged in, where the font isn't.
    fn back_in_basic(spectrum: &BuiltIn) -> bool {
        let rom = rustzx_core::zx::roms::ROM_48K;
        crate::zx_machine::screen_text(|address| if address < 0x4000 { rom[address as usize] } else { spectrum.peek(address) }).iter().any(|line| line.contains("0 OK"))
    }

    /// Every 48K and 128K example loads from its tape on the built-in Spectrum, reaches its main loop, and gives
    /// the machine back to BASIC (which says 0 OK) when its time is up: on a 48K and a 128K, or a 128K only if it
    /// needs one.
    #[test]
    fn every_spectrum_example_runs_and_gives_the_machine_back() {
        for path in examples() {
            let device = device_of(&text_of(&path));
            if device == "next" {
                continue;
            }
            let machines: &[Machine] = if device == "128" { &[Machine::Spectrum128] } else { &[Machine::Spectrum48, Machine::Spectrum128] };
            for &machine in machines {
                let (spectrum, reached) = run_example(&path, machine, 250, Some(100));
                assert!(reached, "{} reaches its main loop on the {machine:?}", path.display());
                // The sound chapter's examples are heard; the rest are as quiet as a program that makes no sound.
                if path.file_name().unwrap().to_string_lossy().starts_with("ch07") {
                    let quiet = run_example(&path.with_file_name("startup_demo.asm"), machine, 250, Some(100)).0.loudest();
                    assert!(spectrum.loudest() > (quiet * 3.0).max(0.05), "{} is heard on the {machine:?}: {} against {quiet}", path.display(), spectrum.loudest());
                }
                assert!(back_in_basic(&spectrum), "{} goes back to BASIC on the {machine:?}: {:?}", path.display(), spectrum.screen_text());
            }
        }
    }
}
