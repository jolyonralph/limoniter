//! Running BBC BASIC on the Agon Light in fab-agon-emulator, which runs the Agon's own MOS and VDP firmware.
//!
//! BASIC source is the Agon's when a line near its top is a REM saying so (REM machine: agon), since an Agon
//! program is plain text like any other BBC BASIC. The emulator is given a folder made for the run as its SD card
//! (`--sdcard folder`, from its README), holding a copy of the card it comes with (MOS and its tools), the program,
//! and an `autoexec.txt`, which MOS runs as it starts, that starts BASIC with the program.
//!
//! What this relies on, from the Agon's and the emulator's documentation as remembered, not checked here: the
//! emulator's `--sdcard` option; MOS running `autoexec.txt` from the card's top folder at start; and BBC BASIC
//! (`bbcbasic`) taking the name of a program as its argument and running it. eZ80 assembly isn't run: Limoniter's
//! Z80 assembler has no eZ80 ADL mode (24-bit addresses and the .LIL, .SIS suffixes), which MOS programs need.

use crate::running::{Format, Machine, Runnable};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The most a copied SD card is let grow to: MOS's files are small, and a card full of the person's own is not
/// meant to be copied.
const MOST_COPIED: u64 = 64 << 20;

/// Whether BASIC text says it's for the Agon: a REM near its top with `machine: agon` in it.
pub fn says_agon<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    lines.into_iter().take(30).any(|line| {
        let line = line.trim_start().trim_start_matches(|c: char| c.is_ascii_digit()).trim_start().to_ascii_lowercase();
        let Some(rest) = line.strip_prefix("rem").or_else(|| line.strip_prefix('>')) else {
            return false;
        };
        let rest = rest.trim_start_matches([' ', '\t', '.']);
        rest.strip_prefix("machine").map(str::trim_start).and_then(|r| r.strip_prefix(':')).is_some_and(|v| v.trim().starts_with("agon"))
    })
}

/// A name for the program on the card: letters, digits, hyphens and underscores only, so that MOS's command line
/// takes it as one word.
pub fn file_name(stem: &str) -> String {
    let safe: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' }).collect();
    let safe = if safe.trim_matches('_').is_empty() { "program".to_string() } else { safe };
    format!("{safe}.bas")
}

/// The program as the Agon loads it: its lines, each ended by a line feed.
pub fn program<S: AsRef<str>>(lines: &[S], stem: &str) -> Runnable {
    let text: String = lines.iter().map(|l| format!("{}\n", l.as_ref().trim_end_matches('\r'))).collect();
    Runnable { machine: Machine::Agon, format: Format::AgonBas, name: file_name(stem), bytes: Arc::new(text.into_bytes()) }
}

/// What MOS runs as it starts.
pub fn autoexec(name: &str) -> String {
    format!("bbcbasic {name}\n")
}

/// Copies a folder and what's in it, giving up (with what's copied so far kept) past a size.
fn copy_tree(from: &Path, to: &Path, copied: &mut u64) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if path.is_dir() {
            copy_tree(&path, &target, copied)?;
        } else if path.is_file() {
            let size = entry.metadata().map_or(0, |m| m.len());
            if *copied + size > MOST_COPIED {
                continue;
            }
            *copied += size;
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Makes the SD card for a run in `folder`, from the one the emulator comes with (`sdcard` beside it, `home` being
/// the emulator's folder), and gives back the program's path on it.
pub fn prepare(folder: &Path, home: Option<&Path>, run: &Runnable) -> Result<PathBuf, String> {
    let made = |e: std::io::Error| crate::i18n::tf!("Couldn't make the copy to run.\n\n{e}");
    let card = folder.join("sdcard");
    std::fs::create_dir_all(&card).map_err(made)?;
    if let Some(home) = home {
        copy_tree(&home.join("sdcard"), &card, &mut 0).map_err(made)?;
    }
    let file = card.join(&run.name);
    std::fs::write(&file, run.bytes.as_slice()).map_err(made)?;
    std::fs::write(card.join("autoexec.txt"), autoexec(&run.name)).map_err(made)?;
    Ok(file)
}

/// The emulator's arguments: the card made for the run (the folder the program is in).
pub fn arguments(file: &Path) -> Vec<String> {
    let card = file.parent().map(|p| p.display().to_string()).unwrap_or_default();
    vec!["--sdcard".into(), card]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_says_it_is_the_agons() {
        assert!(says_agon(["10 REM machine: agon", "20 PRINT 1"]));
        assert!(says_agon(["REM Machine: Agon Light"]));
        assert!(says_agon(["   5 rem machine:agon"]));
        assert!(!says_agon(["10 REM machine: bbc"]));
        assert!(!says_agon(["10 PRINT \"REM machine: agon\""]));
    }

    #[test]
    fn the_program_is_text_with_a_safe_name() {
        let run = program(&["10 PRINT \"HI\"\r", "20 END"], "my game.v2");
        assert_eq!((run.machine, run.format, run.name.as_str()), (Machine::Agon, Format::AgonBas, "my_game_v2.bas"));
        assert_eq!(run.bytes.as_slice(), b"10 PRINT \"HI\"\n20 END\n");
        assert_eq!(file_name("..."), "program.bas");
    }

    #[test]
    fn the_card_has_the_program_mos_and_an_autoexec() {
        let base = std::env::temp_dir().join(format!("limoniter-agon-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (home, folder) = (base.join("fab"), base.join("run"));
        std::fs::create_dir_all(home.join("sdcard").join("mos")).unwrap();
        std::fs::write(home.join("sdcard").join("mos").join("tool.bin"), b"tool").unwrap();
        std::fs::write(home.join("sdcard").join("autoexec.txt"), b"old\n").unwrap();
        let run = program(&["10 END"], "demo");
        let file = prepare(&folder, Some(&home), &run).unwrap();
        assert_eq!(file, folder.join("sdcard").join("demo.bas"));
        assert_eq!(std::fs::read(&file).unwrap(), b"10 END\n");
        assert_eq!(std::fs::read(folder.join("sdcard").join("mos").join("tool.bin")).unwrap(), b"tool");
        assert_eq!(std::fs::read_to_string(folder.join("sdcard").join("autoexec.txt")).unwrap(), "bbcbasic demo.bas\n");
        assert_eq!(arguments(&file), ["--sdcard".to_string(), folder.join("sdcard").display().to_string()]);
        // With no card of its own to copy, the card is just the program.
        let bare = prepare(&base.join("bare"), None, &run).unwrap();
        assert!(bare.is_file());
        let _ = std::fs::remove_dir_all(&base);
    }
}
