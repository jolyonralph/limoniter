//! Atari 8-bit binary load files (.xex, .com, .exe, .obx, and AUTORUN.SYS and the like off a DOS disk), seen as the
//! segments they load: each a range of memory and the bytes for it, opened as a file of its own.
//!
//! As Atari DOS 2 loads them: the file starts with $FF $FF, then each segment is its first and last address (two
//! bytes each, low first) and the bytes between them, inclusive. Another $FF $FF may come before any segment. Two
//! addresses in page 2 are special: a segment that loads $02E0-$02E1 (RUNAD) says where to start the program once
//! it's all loaded, and one that loads $02E2-$02E3 (INITAD) says where to call as soon as that segment is in, so a
//! file can run code part way through loading (a title screen, a check for enough memory) and go on loading after.

use super::{Entry, Source, Volume};
use std::sync::Arc;

const RUNAD: u16 = 0x02E0;
const INITAD: u16 = 0x02E2;

/// One segment: where it loads, and where its bytes are in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub first: u16,
    pub last: u16,
    /// Where its bytes start in the file.
    pub at: usize,
    /// The run address it sets, if it loads RUNAD.
    pub run: Option<u16>,
    /// The init address it sets, if it loads INITAD.
    pub init: Option<u16>,
}

impl Segment {
    pub fn length(&self) -> usize {
        usize::from(self.last - self.first) + 1
    }

    /// The word a segment loads at a vector's address, if it covers both its bytes.
    fn word_at(&self, file: &[u8], address: u16) -> Option<u16> {
        if address < self.first || address + 1 > self.last {
            return None;
        }
        let at = self.at + usize::from(address - self.first);
        Some(u16::from_le_bytes([*file.get(at)?, *file.get(at + 1)?]))
    }
}

/// A binary load file's segments, and anything odd about how it ends. Err if it isn't one at all.
pub fn segments(file: &[u8]) -> Result<(Vec<Segment>, Option<String>), String> {
    if !file.starts_with(&[0xFF, 0xFF]) {
        return Err("This isn't an Atari binary load file: it doesn't start with $FF $FF.".into());
    }
    let mut out = Vec::new();
    let mut at = 2;
    let note = loop {
        if at == file.len() {
            break None;
        }
        // Another $FF $FF header may come before any segment.
        if file.get(at..at + 2) == Some(&[0xFF, 0xFF]) {
            at += 2;
            continue;
        }
        let Some(header) = file.get(at..at + 4) else {
            break Some(format!("The file has {} byte{} after its last segment, too few to be another; they're left out.", file.len() - at, if file.len() - at == 1 { "" } else { "s" }));
        };
        let (first, last) = (u16::from_le_bytes([header[0], header[1]]), u16::from_le_bytes([header[2], header[3]]));
        if last < first {
            if out.is_empty() {
                return Err(format!("This isn't an Atari binary load file: its first segment would end (${last:04X}) before it starts (${first:04X})."));
            }
            break Some(format!("After segment {}, the file has {} bytes that aren't a segment (they'd end at ${last:04X}, before they start at ${first:04X}); they're left out.", out.len(), file.len() - at));
        }
        let mut segment = Segment { first, last, at: at + 4, run: None, init: None };
        let end = segment.at + segment.length();
        if end > file.len() {
            if out.is_empty() && file.len() - segment.at < 1 {
                return Err("This isn't an Atari binary load file: it ends before its first segment's bytes.".into());
            }
            let have = file.len() - segment.at;
            let missing = format!("Segment {} (${first:04X}-${last:04X}) is cut short: the file ends {} bytes into its {}. It's given as far as it goes.", out.len() + 1, have, segment.length());
            segment.last = first + (have.max(1) - 1) as u16;
            if have > 0 {
                segment.run = segment.word_at(file, RUNAD);
                segment.init = segment.word_at(file, INITAD);
                out.push(segment);
            }
            break Some(missing);
        }
        segment.run = segment.word_at(file, RUNAD);
        segment.init = segment.word_at(file, INITAD);
        out.push(segment);
        at = end;
    };
    Ok((out, note))
}

/// Whether a file's start is a binary load file's: $FF $FF and a first segment that ends after it starts. For the
/// endings other systems use too (.com, .exe, .sys, .obj), every segment the start holds has to make sense.
pub fn looks_like(head: &[u8], len: u64, ending: &str) -> bool {
    let Ok((segments, note)) = segments(head) else { return false };
    let Some(first) = segments.first() else { return false };
    if (first.at + first.length()) as u64 > len {
        return false;
    }
    match ending {
        "xex" | "obx" | "atari" => true,
        // A whole file that's all segments, or a start too short to see past that's so far.
        _ => note.is_none() || (head.len() as u64) < len && note.is_some_and(|n| n.contains("cut short")),
    }
}

/// A name a segment opens under, saying where it loads and where it's started from, for the disassembler.
fn open_name(number: usize, segment: &Segment, run: Option<u16>, inits: &[u16]) -> String {
    let mut name = format!("segment {number} ${:04X}-${:04X}", segment.first, segment.last);
    let inside = |address: u16| (segment.first..=segment.last).contains(&address);
    if let Some(run) = run.filter(|&r| inside(r)) {
        name.push_str(&format!(" run ${run:04X}"));
    }
    for &init in inits.iter().filter(|&&i| inside(i)) {
        name.push_str(&format!(" init ${init:04X}"));
    }
    format!("{name}.seg")
}

/// Where code from a segment's name starts, as `open_name` wrote it: the run and init addresses, or its first.
pub fn starts_from_name(name: &str) -> Option<(u16, Vec<u16>)> {
    let stem = name.strip_suffix(".seg")?;
    let words: Vec<&str> = stem.rsplit(" \u{203A} ").next().unwrap_or(stem).split(' ').collect();
    let hex = |w: &str| w.strip_prefix('$').and_then(|h| u16::from_str_radix(h, 16).ok());
    let at = words.iter().position(|w| *w == "segment")?;
    let (first, _) = words.get(at + 2)?.split_once('-')?;
    let first = hex(first)?;
    let starts: Vec<u16> = words.windows(2).filter(|w| matches!(w[0], "run" | "init")).filter_map(|w| hex(w[1])).collect();
    Some((first, if starts.is_empty() { vec![first] } else { starts }))
}

/// A whole binary load file laid out in memory, for disassembling: from its lowest address to its highest, gaps as
/// zeros, the vectors left out; where it loads; and where it starts (its init and run addresses, or else its first
/// segment).
pub fn memory(file: &[u8]) -> Option<(Vec<u8>, u16, Vec<u16>, String)> {
    let (segments, _) = segments(file).ok()?;
    let is_vector = |s: &Segment| s.first >= RUNAD && s.last <= INITAD + 1;
    let code: Vec<&Segment> = segments.iter().filter(|s| !is_vector(s)).collect();
    let low = code.iter().map(|s| s.first).min()?;
    let high = code.iter().map(|s| s.last).max()?;
    let mut memory = vec![0u8; usize::from(high - low) + 1];
    for s in &code {
        let from = usize::from(s.first - low);
        memory[from..from + s.length()].copy_from_slice(file.get(s.at..s.at + s.length())?);
    }
    let mut starts: Vec<u16> = segments.iter().filter_map(|s| s.init).collect();
    let run = segments.iter().filter_map(|s| s.run).next_back();
    starts.extend(run);
    starts.retain(|a| (low..=high).contains(a));
    let how = match run {
        Some(run) => format!("An Atari binary load file, {} segments from ${low:04X} to ${high:04X}, run at ${run:04X}", segments.len()),
        None => format!("An Atari binary load file, {} segments from ${low:04X} to ${high:04X}, with no run address", segments.len()),
    };
    if starts.is_empty() {
        starts.push(code[0].first);
    }
    starts.dedup();
    Some((memory, low, starts, how))
}

/// A binary load file, seen as its segments.
pub struct Xex {
    file: Vec<u8>,
    segments: Vec<Segment>,
    note: Option<String>,
}

impl Xex {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let length = usize::try_from(source.len()).ok().filter(|&l| l <= 16 * 1024 * 1024).ok_or("This file is far too large to be an Atari binary load file.")?;
        let file = source.read_vec(0, length)?;
        let (segments, note) = segments(&file)?;
        if segments.is_empty() {
            return Err("This Atari binary load file has no segments in it.".into());
        }
        Ok(Self { file, segments, note })
    }

    fn run(&self) -> Option<u16> {
        self.segments.iter().filter_map(|s| s.run).next_back()
    }
}

impl Volume for Xex {
    fn describe(&self) -> String {
        let count = self.segments.len();
        let run = self.run().map_or_else(|| "no run address".to_string(), |run| format!("runs at ${run:04X}"));
        format!("Atari 8-bit binary load file, {count} segment{} \u{B7} {run}", if count == 1 { "" } else { "s" })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        let run = self.run();
        let inits: Vec<u16> = self.segments.iter().filter_map(|s| s.init).collect();
        Ok(self
            .segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let mut entry = Entry::file(format!("{:>3}  ${:04X}-${:04X}", index + 1, segment.first, segment.last), segment.length() as u64, index as u64);
                let vectors: Vec<String> = segment.run.map(|r| format!("RUNAD: run at ${r:04X} once loaded")).into_iter().chain(segment.init.map(|i| format!("INITAD: call ${i:04X} now"))).collect();
                if vectors.is_empty() {
                    entry.kind = "Segment".into();
                    entry.detail = format!("{} bytes at ${:04X}", segment.length(), segment.first);
                } else {
                    entry.kind = if segment.run.is_some() && segment.init.is_some() {
                        "Run and init address"
                    } else if segment.run.is_some() {
                        "Run address"
                    } else {
                        "Init address"
                    }
                    .into();
                    entry.detail = vectors.join("; ");
                }
                entry.open_name = open_name(index + 1, segment, run, &inits);
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let segment = self.segments.get(entry.location as usize).ok_or("That segment isn't in the file.")?;
        self.file.get(segment.at..segment.at + segment.length()).map(<[u8]>::to_vec).ok_or_else(|| "That segment runs past the end of the file.".into())
    }

    fn notes(&self) -> Vec<String> {
        let mut notes: Vec<String> = self.note.iter().cloned().collect();
        if self.run().is_none() {
            let first = self.segments[0].first;
            notes.push(format!("It sets no run address (RUNAD), so once it's loaded Atari DOS goes back to its menu unless an init routine has started it; many loaders start such a file at its first segment, ${first:04X}."));
        }
        notes.push("Segments that set RUNAD ($02E0) or INITAD ($02E2) say where the program starts, or what's called as soon as that segment has loaded; each segment opens as the bytes it loads.".into());
        notes
    }

    /// Text in an Atari program is ATASCII.
    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        Some("atari-8bit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A binary load file: a title routine called as it loads, then the program, then where to run it.
    pub fn sample() -> Vec<u8> {
        let mut file = vec![0xFF, 0xFF, 0x00, 0x06, 0x02, 0x06, 0xA9, 0x00, 0x60];
        file.extend([0xE2, 0x02, 0xE3, 0x02, 0x00, 0x06]);
        file.extend([0xFF, 0xFF, 0x00, 0x20, 0x03, 0x20, 0x4C, 0x00, 0x20, 0xEA]);
        file.extend([0xE0, 0x02, 0xE1, 0x02, 0x00, 0x20]);
        file
    }

    #[test]
    fn segments_and_their_vectors() {
        let file = sample();
        assert!(looks_like(&file, file.len() as u64, "xex"));
        assert!(looks_like(&file, file.len() as u64, "com"));
        assert!(!looks_like(b"MZ\x90\0", 4, "exe"));
        let xex = Xex::open(Arc::new(file.clone())).unwrap();
        assert_eq!(xex.describe(), "Atari 8-bit binary load file, 4 segments \u{B7} runs at $2000");
        let entries = xex.list(None).unwrap();
        assert_eq!(entries[1].kind, "Init address");
        assert_eq!(entries[1].detail, "INITAD: call $0600 now");
        assert_eq!(entries[3].kind, "Run address");
        assert_eq!(xex.read(&entries[2]).unwrap(), [0x4C, 0x00, 0x20, 0xEA]);
        assert_eq!(entries[0].open_name, "segment 1 $0600-$0602 init $0600.seg");
        assert_eq!(entries[2].open_name, "segment 3 $2000-$2003 run $2000.seg");
        assert_eq!(starts_from_name("GAME.XEX \u{203A} segment 3 $2000-$2003 run $2000.seg"), Some((0x2000, vec![0x2000])));
        let (memory, origin, starts, _) = memory(&file).unwrap();
        assert_eq!((origin, memory.len(), starts), (0x0600, 0x2004 - 0x0600, vec![0x0600, 0x2000]));
    }

    /// Every file in the folder LIMONITER_XEX_DIR names (and its folders) is read as a binary load file, a line of
    /// its segments to standard error, to compare with another tool's: `name<TAB>first-last ...<TAB>run<TAB>init`.
    /// Run by hand: `cargo test xex_sweep -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn xex_sweep() {
        let Some(dir) = std::env::var_os("LIMONITER_XEX_DIR") else { return };
        let mut folders = vec![std::path::PathBuf::from(dir)];
        let mut paths = Vec::new();
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(folder).unwrap().flatten() {
                if item.path().is_dir() {
                    folders.push(item.path());
                } else if item.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("xex")) {
                    paths.push(item.path());
                }
            }
        }
        paths.sort();
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            match segments(&bytes) {
                Ok((segments, _)) => {
                    let ranges: Vec<String> = segments.iter().map(|s| format!("{:04X}-{:04X}", s.first, s.last)).collect();
                    let run = segments.iter().filter_map(|s| s.run).next_back().map_or("-".to_string(), |r| format!("{r:04X}"));
                    let inits: Vec<String> = segments.iter().filter_map(|s| s.init).map(|i| format!("{i:04X}")).collect();
                    eprintln!("{name}\t{}\t{run}\t{}", ranges.join(" "), inits.join(" "));
                    let _ = memory(&bytes);
                }
                Err(_) => eprintln!("{name}\tREFUSED"),
            }
        }
    }

    #[test]
    fn odd_endings_are_said_and_bad_files_refused() {
        let mut file = sample();
        file.extend([0x00, 0x30, 0x10, 0x30, 1, 2]);
        let (segments, note) = segments(&file).unwrap();
        assert_eq!(segments.last().map(|s| (s.first, s.last)), Some((0x3000, 0x3001)));
        assert!(note.unwrap().contains("cut short"));
        let mut junk = sample();
        junk.extend([0x00]);
        assert!(super::segments(&junk).unwrap().1.unwrap().contains("1 byte after"));
        assert!(super::segments(&[0xFF, 0xFF, 0x10, 0x20, 0x00, 0x20]).is_err(), "a first segment ending before it starts");
        assert!(super::segments(b"\xFF\xFF").unwrap().0.is_empty());
        assert!(Xex::open(Arc::new(b"\xFF\xFF".to_vec())).is_err());
        for cut in 0..sample().len() {
            if let Ok(xex) = Xex::open(Arc::new(sample()[..cut].to_vec())) {
                for entry in xex.list(None).unwrap() {
                    xex.read(&entry).unwrap();
                }
            }
        }
    }
}
