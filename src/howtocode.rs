//! How To Code v8: the Amiga demo coder's guide, with its examples, in Limoniter. Its files are in the program
//! (put there by build.rs from data/howtocode). To be read, they're unpacked into Limoniter's data folder, and
//! written again whenever the program's copy is different; its examples can be copied into a folder of the
//! person's own, to start a demo from.

use std::path::{Path, PathBuf};

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/howtocode_files.rs"));
}

/// Every file of the guide: its path in the guide, and its bytes.
pub fn files() -> &'static [(&'static str, &'static [u8])] {
    bundled::FILES
}

/// What the program's copy of a guide is, so an old unpacked one is known to be old.
fn stamp(files: &[(&str, &[u8])]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, bytes) in files {
        name.hash(&mut hasher);
        bytes.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Unpacks the guide into a folder, unless the same copy is there already.
pub fn unpack_to(folder: &Path) -> Result<(), String> {
    unpack_files_to(files(), folder)
}

/// Unpacks a guide's files into a folder, unless the same copy is there already. The Spectrum's guide is unpacked
/// this way too.
pub fn unpack_files_to(files: &[(&str, &[u8])], folder: &Path) -> Result<(), String> {
    let mark = folder.join(".version");
    let stamp = stamp(files);
    if std::fs::read_to_string(&mark).is_ok_and(|was| was == stamp) {
        return Ok(());
    }
    for (name, bytes) in files {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    std::fs::write(mark, stamp).map_err(|e| e.to_string())
}

/// The guide's folder, unpacked in Limoniter's data folder.
pub fn unpacked() -> Result<PathBuf, String> {
    let folder = crate::settings::data_folder().ok_or_else(|| crate::i18n::tr("There's no folder to keep Limoniter's files in.").to_string())?.join("howtocode");
    unpack_to(&folder)?;
    Ok(folder)
}

/// The examples, and the files they include, copied into a folder to start a demo from. Files already there are
/// kept as they are. Returns how many were copied.
pub fn copy_examples(to: &Path) -> Result<usize, String> {
    copy_files_examples(files(), to)
}

/// A guide's examples copied into a folder, keeping files already there; how many were copied.
pub fn copy_files_examples(files: &[(&str, &[u8])], to: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut copied = 0;
    for (name, bytes) in files {
        let Some(file) = name.strip_prefix("examples/") else { continue };
        let path = to.join(file);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        copied += 1;
    }
    Ok(copied)
}

/// The smallest whole demo, startup_demo.s, as one file: the startup and the two files of names it includes are put
/// in where it includes them, each marked, so it builds with F9 with nothing beside it. What File, New as starts
/// an Amiga demo with. Its comments are the guide's, in English, as the guide is.
pub fn single_file_demo() -> String {
    let example = |name: &str| -> String {
        let path = format!("examples/{name}");
        let bytes = files().iter().find(|(file, _)| *file == path).map(|(_, bytes)| *bytes).unwrap_or_default();
        String::from_utf8_lossy(bytes).replace("\r\n", "\n")
    };
    let mut out = String::new();
    for (n, line) in example("startup_demo.s").lines().enumerate() {
        if n == 0 {
            out.push_str("; One file: How To Code v8's startup_demo.s, with startup.i, hardware.i and system.i put in where it\n");
            out.push_str("; includes them. Help, How To Code explains every part; its examples are copied from Tools, Amiga.\n");
        }
        let included = line.trim().strip_prefix("include").map(|rest| rest.trim().trim_matches('"').to_string()).filter(|name| name.ends_with(".i"));
        match included {
            Some(name) => {
                out.push_str(&format!("\n; ---- {name}, as How To Code v8 has it ----\n\n"));
                out.push_str(example(&name).trim_end());
                out.push_str(&format!("\n\n; ---- the end of {name} ----\n"));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Every link from one page of a guide to another, or to a picture or an example, finds its file; each chapter is
/// in the README's contents, and starts as a chapter does. For the tests of both guides.
#[cfg(test)]
pub(crate) fn check_links(files: &[(&str, &[u8])]) {
    let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
    let readme = files.iter().find(|(name, _)| *name == "README.md").map(|(_, bytes)| String::from_utf8_lossy(bytes)).unwrap();
    for (name, bytes) in files {
        let Some(chapter) = name.strip_suffix(".md") else { continue };
        let text = String::from_utf8_lossy(bytes);
        for (at, _) in text.match_indices("](") {
            let target: String = text[at + 2..].chars().take_while(|&c| c != ')' && c != '#').collect();
            if target.contains("://") || target.is_empty() || target.starts_with('<') {
                continue;
            }
            assert!(names.contains(&target.as_str()), "{name} links to {target}, which isn't in the guide");
        }
        if let Some((number, _)) = chapter.split_once('-').filter(|(n, _)| n.len() == 2 && n.chars().all(|c| c.is_ascii_digit())) {
            let number: u32 = number.parse().unwrap();
            assert!(readme.contains(&format!("| {number} | [**")) && readme.contains(&format!("]({name})")), "{name} is in the README's contents, as a link to it");
            assert!(text.contains("[Contents](README.md)"), "{name} has its Back / Contents / Next line");
            assert!(text.starts_with(&format!("# {number}. ")), "{name} starts with its number");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_is_in_the_program_and_unpacks() {
        assert!(files().iter().any(|(name, _)| *name == "README.md"));
        assert!(files().iter().any(|(name, _)| *name == "examples/startup.i"));
        let folder = std::env::temp_dir().join(format!("limoniter-htc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        unpack_to(&folder).unwrap();
        assert!(folder.join("04-startup-and-exit.md").is_file());
        assert!(folder.join("examples/startup_demo.s").is_file());
        // The same copy isn't written again.
        std::fs::write(folder.join("README.md"), "changed").unwrap();
        unpack_to(&folder).unwrap();
        assert_eq!(std::fs::read_to_string(folder.join("README.md")).unwrap(), "changed");
        // Examples go into a folder of the person's own, not over what's there.
        let project = folder.join("mine");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("startup_demo.s"), "my own").unwrap();
        assert!(copy_examples(&project).unwrap() > 3);
        assert_eq!(std::fs::read_to_string(project.join("startup_demo.s")).unwrap(), "my own");
        assert!(project.join("startup.i").is_file());
        let _ = std::fs::remove_dir_all(&folder);
    }

    fn examples() -> Vec<PathBuf> {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/howtocode/examples");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "s")).collect();
        found.sort();
        found
    }

    /// An example made into an AmigaDOS program as `limoniter --assemble` makes it, with its settings given these
    /// values.
    fn build_with(path: &Path, values: &[(&str, &str)]) -> Vec<u8> {
        let text = std::fs::read_to_string(path).unwrap();
        let values: Vec<(String, String)> = values.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        crate::cli_assemble::assemble(path, &crate::cli_assemble::set_values(&text, &values)).unwrap_or_else(|problems| panic!("{problems:?}"))
    }

    /// Every example assembles into an AmigaDOS program, in both of the startup's ways.
    #[test]
    fn every_example_assembles() {
        for path in examples() {
            for mode in ["0", "1"] {
                assert!(build_with(&path, &[("OS_FRIENDLY", mode)]).starts_with(&[0, 0, 3, 0xF3]), "{} is a hunk executable", path.display());
            }
        }
    }

    /// Every link from one page of the guide to another, or to a picture or an example, finds its file: a chapter
    /// renumbered or renamed must take its links with it. Each chapter is in the README's table of contents, too.
    #[test]
    fn the_guide_links_to_files_that_are_there() {
        check_links(files());
    }

    /// The single-file demo includes nothing, holds each of the startup's files whole, and builds both ways into
    /// the same program as startup_demo.s with its includes.
    #[test]
    fn the_single_file_demo_needs_nothing_beside_it() {
        let single = single_file_demo();
        assert!(!single.lines().any(|line| line.trim_start().starts_with("include")), "no include left");
        for name in ["hardware.i", "system.i", "startup.i"] {
            let (_, bytes) = files().iter().find(|(file, _)| *file == format!("examples/{name}")).unwrap();
            assert!(single.contains(String::from_utf8_lossy(bytes).replace("\r\n", "\n").trim_end()), "{name} whole");
        }
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/howtocode/examples");
        for mode in ["0", "1"] {
            let values = vec![("OS_FRIENDLY".to_string(), mode.to_string())];
            let alone = std::env::temp_dir().join(format!("limoniter-single-{}", std::process::id())).join("demo.s");
            let one = crate::cli_assemble::assemble(&alone, &crate::cli_assemble::set_values(&single, &values)).unwrap();
            let split = build_with(&folder.join("startup_demo.s"), &[("OS_FRIENDLY", mode)]);
            assert_eq!(one, split, "OS_FRIENDLY={mode}");
        }
    }

    /// For the emulator's test disk: prints a line as the demo before it comes back, on any Kickstart (1.3's echo
    /// is on a disk the test disk doesn't have).
    const BACK: &str = "        section code,code\n        move.l  4.w,a6\n        lea     dos(pc),a1\n        moveq   #0,d0\n        jsr     -552(a6)\n        move.l  d0,a6\n        jsr     -60(a6)\n        move.l  d0,d1\n        lea     text(pc),a0\n        move.l  a0,d2\n        moveq   #textend-text,d3\n        jsr     -48(a6)\n        move.l  a6,a1\n        move.l  4.w,a6\n        jsr     -414(a6)\n        moveq   #0,d0\n        rts\ndos:    dc.b    \"dos.library\",0\ntext:   dc.b    \"How To Code v8: back in AmigaDOS, return code OK\",10\ntextend:\n        even\n";

    /// Writes each example's program to target/howtocode, to run in an emulator: as it is, and in each of the
    /// startup's ways, ending by itself after 500 frames.
    #[test]
    #[ignore]
    fn write_the_examples() {
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/howtocode");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("back"), crate::cli_assemble::assemble(Path::new("back.s"), BACK).unwrap()).unwrap();
        for path in examples() {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            std::fs::write(out.join(&name), build_with(&path, &[])).unwrap();
            std::fs::write(out.join(format!("{name}-takeover")), build_with(&path, &[("OS_FRIENDLY", "0"), ("AUTOEXIT", "500")])).unwrap();
            std::fs::write(out.join(format!("{name}-os")), build_with(&path, &[("OS_FRIENDLY", "1"), ("AUTOEXIT", "500")])).unwrap();
        }
    }
}
