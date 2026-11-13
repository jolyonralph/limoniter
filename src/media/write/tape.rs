//! Spectrum tapes, TAP and TZX. A file is a header block and the data block after it, as SAVE writes them; a file
//! added goes on the end, a deleted one has both its blocks taken out, and a renamed one has its header rewritten
//! with a new checksum. A TZX keeps every other block it has (its descriptions, pauses and the rest) where they are.

use super::{Addition, Entry, ImageWriter, Kind};
use crate::basic;

const PROGRAM: &str = "Program";
const CODE: &str = "Code";
const SCREEN: &str = "Screen";
const HEADERLESS: &str = "Headerless block";
const TAPE: &str = "Tape blocks";

/// A blank TZX: its signature and version, 1.20.
pub fn blank_tzx() -> Vec<u8> {
    b"ZXTape!\x1A\x01\x14".to_vec()
}

pub struct Tape {
    bytes: Vec<u8>,
    tzx: bool,
}

/// Where a data block is in the image: the whole of it, from its length or block ID to its checksum, and its data.
#[derive(Clone, Copy, Debug)]
struct Block {
    start: usize,
    end: usize,
    data: usize,
    length: usize,
}

impl Tape {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let tzx = bytes.starts_with(b"ZXTape!\x1A");
        let tape = Self { bytes, tzx };
        let (_, problem) = basic::tape_blocks(&tape.bytes);
        if let Some(problem) = problem {
            return Err(format!("The tape can't be changed safely: {problem}"));
        }
        Ok(tape)
    }

    /// The data blocks, where they are, read as the tape reader reads them.
    fn blocks(&self) -> Vec<Block> {
        basic::tape_block_places(&self.bytes).0.into_iter().map(|(start, data, length)| Block { start, end: data + length, data, length }).collect()
    }

    /// The tape's files as the reader lists them: a header with its data, or a data block on its own.
    fn files(&self) -> Vec<Vec<Block>> {
        let mut files = Vec::new();
        let mut pending = None;
        for block in self.blocks() {
            if block.length < 2 {
                continue;
            }
            let (flag, body) = (self.bytes[block.data], block.length - 2);
            if flag == 0 && body == 17 {
                pending = Some(block);
                continue;
            }
            match pending.take() {
                Some(header) => files.push(vec![header, block]),
                None => files.push(vec![block]),
            }
        }
        files
    }

    fn file(&self, entry: &Entry) -> Result<Vec<Block>, String> {
        self.files().into_iter().nth(entry.location as usize).ok_or_else(|| "That file isn't on the tape.".into())
    }

    /// A block as it's written on this tape: a TAP block, or a TZX standard-speed block with a second's pause.
    fn written(&self, flag: u8, data: &[u8]) -> Vec<u8> {
        let tap = basic::tap_block(flag, data);
        if !self.tzx {
            return tap;
        }
        let mut out = vec![0x10];
        out.extend(1000u16.to_le_bytes());
        out.extend(tap);
        out
    }
}

/// A name in the Spectrum's set, ten characters, padded with spaces.
fn tape_name(name: &str) -> [u8; 10] {
    let mut out = [b' '; 10];
    let chars = name.chars().filter_map(|c| match c {
        '\u{A3}' => Some(0x60),
        '\u{A9}' => Some(0x7F),
        ' '..='~' if c != '`' => Some(c as u8),
        _ => None,
    });
    for (slot, byte) in out.iter_mut().zip(chars) {
        *slot = byte;
    }
    out
}

/// A +3DOS file's header fields: its type, length, and two parameters (autostart or address; program length).
fn plus3_header(bytes: &[u8]) -> Option<(u8, u16, u16, u16)> {
    if !bytes.starts_with(b"PLUS3DOS") || bytes.len() < 128 {
        return None;
    }
    let word = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    Some((bytes[15], word(16), word(18), word(20)))
}

/// Whether bytes are a listing typed as text, rather than a tokenised program.
fn is_listing(bytes: &[u8]) -> bool {
    !bytes.is_empty() && std::str::from_utf8(bytes).is_ok_and(|text| text.lines().any(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())) && !text.contains('\0'))
}

impl ImageWriter for Tape {
    fn kinds(&self) -> Vec<Kind> {
        vec![
            Kind { name: PROGRAM, about: "A BASIC program, as SAVE \"name\" writes one. A listing typed as text is tokenised first.", address: false, autostart: true },
            Kind { name: CODE, about: "Bytes, as SAVE \"name\" CODE writes them, to load at an address.", address: true, autostart: false },
            Kind { name: SCREEN, about: "A 6912-byte screen, as SAVE \"name\" SCREEN$ writes one.", address: false, autostart: false },
            Kind { name: HEADERLESS, about: "A data block with no header, flag 0xFF, as a loader that reads its own blocks wants.", address: false, autostart: false },
            Kind::plain(TAPE, "Every block of another .tap file, added as they are."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (stem, ending) = super::stem_and_ending(name);
        let ending = ending.to_lowercase();
        let mut add = Addition { name: stem.to_string(), bytes: bytes.to_vec(), kind: CODE.into(), address: Some(32768), ..Default::default() };
        if let Some((kind, _, first, _)) = plus3_header(bytes) {
            match kind {
                0 => {
                    add.kind = PROGRAM.into();
                    add.autostart = (first < 10000).then_some(first);
                }
                3 => add.address = Some(first),
                _ => {}
            }
        } else if ending == "tap" {
            add.kind = TAPE.into();
        } else if ending == "scr" || (bytes.len() == 6912 && ending != "bin") {
            add.kind = SCREEN.into();
        } else if ending == "bas" || is_listing(bytes) && matches!(ending.as_str(), "txt" | "bas" | "") {
            add.kind = PROGRAM.into();
            add.autostart = is_listing(bytes).then_some(10);
        }
        add
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let header = plus3_header(&file.bytes);
        let body = if header.is_some() { &file.bytes[128..] } else { &file.bytes[..] };
        if file.kind == TAPE {
            let (blocks, problem) = basic::tape_blocks(&file.bytes);
            if let Some(problem) = problem {
                return Err(format!("{} isn't a whole tape: {problem}", file.name));
            }
            for block in blocks.iter().filter(|b| !b.is_empty()) {
                let written = self.written(block[0], &block[1..block.len().saturating_sub(1)]);
                self.bytes.extend(written);
            }
            return Ok(());
        }
        if file.kind == HEADERLESS {
            let written = self.written(0xFF, body);
            self.bytes.extend(written);
            return Ok(());
        }
        let (kind, data, first, second) = match file.kind.as_str() {
            PROGRAM => {
                let (program, length) = match header {
                    // A +3DOS program keeps its variables, and the length of the program before them.
                    Some((0, length, _, program_length)) => (body[..usize::from(length).min(body.len())].to_vec(), program_length),
                    _ if is_listing(body) => {
                        let text = String::from_utf8_lossy(body);
                        let lines: Vec<&str> = text.lines().collect();
                        let program = basic::tokenise_spectrum(&lines, basic::ZxDialect::Spectrum)?;
                        let length = program.len() as u16;
                        (program, length)
                    }
                    _ => (body.to_vec(), body.len() as u16),
                };
                (0u8, program, file.autostart.filter(|l| *l < 10000).unwrap_or(0x8000), length)
            }
            SCREEN => {
                if body.len() != 6912 {
                    return Err(format!("A screen is 6912 bytes; {} is {}.", file.name, body.len()));
                }
                (3, body.to_vec(), 16384, 0x8000)
            }
            _ => (3, body.to_vec(), file.address.unwrap_or(32768), 0x8000),
        };
        if data.len() > 0xFFFF - 2 {
            return Err(format!("{} is too big for a tape block.", file.name));
        }
        let mut head = vec![kind];
        head.extend(tape_name(&file.name));
        head.extend((data.len() as u16).to_le_bytes());
        head.extend(first.to_le_bytes());
        head.extend(second.to_le_bytes());
        let mut written = self.written(0x00, &head);
        written.extend(self.written(0xFF, &data));
        self.bytes.extend(written);
        Ok(())
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let file = self.file(entry)?;
        for block in file.iter().rev() {
            self.bytes.drain(block.start..block.end);
        }
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let file = self.file(entry)?;
        if file.len() < 2 {
            return Err("A headerless block has no name to change.".into());
        }
        let header = file[0];
        self.bytes[header.data + 2..header.data + 12].copy_from_slice(&tape_name(name));
        let sum = self.bytes[header.data..header.data + header.length - 1].iter().fold(0u8, |sum, b| sum ^ b);
        self.bytes[header.data + header.length - 1] = sum;
        Ok(())
    }

    fn reorder(&mut self, entry: &Entry, by: i32) -> Result<(), String> {
        let mut at = entry.location as usize;
        // A step at a time, each file swapping places with its neighbour, so anything between their blocks (a
        // TZX's descriptions) stays where it is.
        for _ in 0..by.unsigned_abs() {
            let files = self.files();
            let other = if by < 0 { at.checked_sub(1) } else { Some(at + 1) };
            let Some(other) = other.filter(|o| *o < files.len()) else {
                break;
            };
            let (first, second) = (at.min(other), at.max(other));
            let range = |file: &Vec<Block>| (file[0].start, file[file.len() - 1].end);
            let (a, b) = (range(&files[first]), range(&files[second]));
            let mut out = self.bytes[..a.0].to_vec();
            out.extend_from_slice(&self.bytes[b.0..b.1]);
            out.extend_from_slice(&self.bytes[a.1..b.0]);
            out.extend_from_slice(&self.bytes[a.0..a.1]);
            out.extend_from_slice(&self.bytes[b.1..]);
            self.bytes = out;
            at = other;
        }
        Ok(())
    }

    fn has_order(&self) -> bool {
        true
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened, tool};
    use super::*;

    fn program() -> Vec<u8> {
        basic::tokenise_spectrum(&["10 PRINT \"HELLO\"", "20 GO TO 10"], basic::ZxDialect::Spectrum).unwrap()
    }

    fn add(tape: &mut Tape, name: &str, bytes: &[u8]) {
        let addition = tape.guess(name, bytes);
        tape.add(None, &addition).unwrap();
    }

    #[test]
    fn files_go_on_a_tape_as_save_writes_them_and_come_off_whole() {
        for (blank, name) in [(Vec::new(), "t.tap"), (blank_tzx(), "t.tzx")] {
            let mut tape = Tape::open(blank).unwrap();
            add(&mut tape, "hello.bas", b"10 PRINT \"HELLO\"\n20 GO TO 10\n");
            add(&mut tape, "picture.scr", &[0x55; 6912]);
            add(&mut tape, "game.bin", &[1, 2, 3]);
            let volume = reopened(&tape, name);
            assert_eq!(names(&*volume, None), ["hello.bas", "picture.scr", "game.code"], "{name}");
            let hello = volume.read(&entry(&*volume, None, "hello.bas")).unwrap();
            assert_eq!(hello, basic::plus3dos_basic(&program(), program().len(), Some(10)), "{name}: the listing is tokenised");
            assert_eq!(volume.read(&entry(&*volume, None, "game.code")).unwrap(), [1, 2, 3]);
            assert_eq!(entry(&*volume, None, "game.code").detail, "3 bytes at 32768");
            // A program taken off one tape goes on another as the same program, autostart and all.
            let mut copy = Tape::open(Vec::new()).unwrap();
            add(&mut copy, "hello.bas", &hello);
            assert_eq!(reopened(&copy, "c.tap").read(&entry(&*reopened(&copy, "c.tap"), None, "hello.bas")).unwrap(), hello);

            tape.rename(None, &entry(&*volume, None, "picture.scr"), "Loading").unwrap();
            tape.reorder(&entry(&*reopened(&tape, name), None, "game.code"), -2).unwrap();
            let volume = reopened(&tape, name);
            assert_eq!(names(&*volume, None), ["game.code", "hello.bas", "Loading.scr"], "{name}");
            tape.delete(None, &entry(&*volume, None, "hello.bas")).unwrap();
            let volume = reopened(&tape, name);
            assert_eq!(names(&*volume, None), ["game.code", "Loading.scr"]);
            assert_eq!(volume.read(&entry(&*volume, None, "Loading.scr")).unwrap(), [0x55; 6912]);
        }
    }

    /// fuse-utils' tzxlist reads what's written the way the Spectrum would.
    #[test]
    fn tzxlist_reads_a_written_tape() {
        let Some(tzxlist) = tool("tzxlist") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-tape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (blank, name) in [(Vec::new(), "t.tap"), (blank_tzx(), "t.tzx")] {
            let mut tape = Tape::open(blank).unwrap();
            add(&mut tape, "hello.bas", b"10 PRINT \"HELLO\"\n");
            add(&mut tape, "game.bin", &[1, 2, 3]);
            let path = dir.join(name);
            std::fs::write(&path, tape.bytes()).unwrap();
            let out = std::process::Command::new(&tzxlist).arg(&path).output().unwrap();
            let listed = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success(), "{listed}");
            assert!(listed.contains("Program: \"hello     \""), "{listed}");
            assert!(listed.contains("Bytes: \"game      \""), "{listed}");
            assert!(!listed.contains("checksum") || !listed.contains("bad"), "{listed}");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
