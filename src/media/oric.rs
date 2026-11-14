//! Oric-1 and Atmos tapes in `.tap` files: the bytes the tape carried, each file after its leader.
//!
//! A file is a run of &16 sync bytes (at least three), &24, then a 9-byte header: two unused bytes, the type (&00 a
//! BASIC program, &80 machine code, &40 an array), whether it runs once loaded (non-zero if so), the end address
//! and the start address (each high byte first), and an unused byte; then the name, ending with a zero; then the
//! bytes from the start address to the end address, both included. This is how the Oric's ROM writes a tape, as
//! the Oric Atmos manual and the tape tools of the OSDK (osdk.org) describe it. A file of zeros or noise between
//! files is skipped, to the next leader.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

struct TapeFile {
    name: String,
    open_name: String,
    kind: String,
    detail: String,
    data: Vec<u8>,
}

pub struct OricTape {
    files: Vec<TapeFile>,
    notes: Vec<String>,
}

/// Where the next file's &24 is from `from`: after at least three &16s.
fn next_file(data: &[u8], from: usize) -> Option<usize> {
    let mut run = 0;
    for (at, &byte) in data.iter().enumerate().skip(from) {
        match byte {
            0x16 => run += 1,
            0x24 if run >= 3 => return Some(at),
            _ => run = 0,
        }
    }
    None
}

/// Whether a `.tap` file starts as an Oric tape does: its sync bytes and then a file's &24.
pub fn looks_like(head: &[u8]) -> bool {
    head.starts_with(&[0x16, 0x16, 0x16]) && next_file(head, 0).is_some_and(|at| head[..at].iter().all(|&b| b == 0x16))
}

impl OricTape {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(16 * 1024 * 1024) as usize)?;
        let mut files = Vec::new();
        let mut notes = Vec::new();
        let mut at = 0;
        while let Some(marker) = next_file(&data, at) {
            let skipped = &data[at..marker];
            if skipped.iter().any(|&b| b != 0x16 && b != 0) {
                notes.push(format!("{} bytes before the file at byte {marker} aren't part of a file.", skipped.iter().filter(|&&b| b != 0x16).count()));
            }
            let Some(header) = data.get(marker + 1..marker + 10) else {
                notes.push(format!("The tape ends in the header of the file at byte {marker}."));
                break;
            };
            let (kind, autorun) = (header[2], header[3]);
            let end = usize::from(u16::from_be_bytes([header[4], header[5]]));
            let start = usize::from(u16::from_be_bytes([header[6], header[7]]));
            let name_at = marker + 10;
            let Some(name_length) = data.get(name_at..).and_then(|rest| rest.iter().take(17).position(|&b| b == 0)) else {
                notes.push(format!("The file at byte {marker} has no end to its name."));
                break;
            };
            let name: String = data[name_at..name_at + name_length].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' }).collect();
            let name = if name.trim().is_empty() { format!("File {}", files.len() + 1) } else { tidy_name(&name) };
            let body_at = name_at + name_length + 1;
            if end < start {
                notes.push(format!("{name} ends at &{end:04X}, before it starts at &{start:04X}, so it's left out."));
                at = body_at;
                continue;
            }
            let length = end - start + 1;
            let body = &data[body_at.min(data.len())..(body_at + length).min(data.len())];
            if body.len() < length {
                notes.push(format!("{name} should be {length} bytes, but the tape ends after {}.", body.len()));
            }
            let runs = if autorun != 0 { ", runs when loaded" } else { "" };
            let (what, ending) = match kind {
                0x00 => ("BASIC program", "bas"),
                0x80 => ("Machine code", "bin"),
                0x40 => ("Array", "dat"),
                _ => ("File", "bin"),
            };
            files.push(TapeFile { open_name: format!("{name}.{ending}"), kind: what.into(), detail: format!("&{start:04X} to &{end:04X}{runs}"), data: body.to_vec(), name });
            at = body_at + body.len();
        }
        let trailing = &data[at.min(data.len())..];
        if trailing.iter().any(|&b| b != 0 && b != 0x16) {
            notes.push(format!("{} bytes after the last file aren't part of one.", trailing.len()));
        }
        if files.is_empty() {
            return Err("No Oric files were found on this tape.".into());
        }
        Ok(Self { files, notes })
    }
}

impl Volume for OricTape {
    fn describe(&self) -> String {
        format!("Oric tape (TAP) \u{B7} {} file(s)", self.files.len())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let mut entry = Entry::file(file.name.clone(), file.data.len() as u64, index as u64);
                entry.open_name = file.open_name.clone();
                entry.kind = file.kind.clone();
                entry.detail = file.detail.clone();
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.files.get(entry.location as usize).map(|file| file.data.clone()).ok_or_else(|| "That file isn't on the tape.".into())
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }
}

/// The first BASIC program on an Oric tape, for listing a `.tap` file as a whole.
pub fn first_program(data: &[u8]) -> Option<Vec<u8>> {
    let tape = OricTape::open(Arc::new(data.to_vec())).ok()?;
    tape.files.into_iter().find(|file| file.kind == "BASIC program").map(|file| file.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tape file: leader, &24, the header, the name, the bytes.
    fn file(kind: u8, autorun: u8, start: u16, name: &[u8], body: &[u8]) -> Vec<u8> {
        let end = start + body.len() as u16 - 1;
        let mut out = vec![0x16; 4];
        out.push(0x24);
        out.extend([0, 0, kind, autorun]);
        out.extend(end.to_be_bytes());
        out.extend(start.to_be_bytes());
        out.push(0);
        out.extend_from_slice(name);
        out.push(0);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn an_oric_tape_holds_basic_and_machine_code() {
        // 10 PRINT "HI", at &0501.
        let program = [0x0C, 0x05, 0x0A, 0x00, 0xBA, b' ', b'"', b'H', b'I', b'"', 0x00, 0x00, 0x00];
        let mut tape = file(0x00, 0xC7, 0x0501, b"HELLO", &program);
        tape.extend([0u8; 5]);
        tape.extend(file(0x80, 0x00, 0x9800, b"", &[0xA9, 0x00, 0x60]));
        assert!(looks_like(&tape));
        let image = super::super::open(Arc::new(tape.clone()), "games.tap").unwrap();
        assert_eq!(image.describe(), "Oric tape (TAP) \u{B7} 2 file(s)");
        let files = image.list(None).unwrap();
        assert_eq!((files[0].open_name.as_str(), files[0].detail.as_str()), ("HELLO.bas", "&0501 to &050D, runs when loaded"));
        assert_eq!((files[1].open_name.as_str(), files[1].kind.as_str()), ("File 2.bin", "Machine code"));
        assert_eq!(image.read(&files[1]).unwrap(), [0xA9, 0x00, 0x60]);
        assert!(image.notes().is_empty(), "{:?}", image.notes());
        let bytes = image.read(&files[0]).unwrap();
        assert_eq!(crate::basic::list(&bytes, "bas", crate::charset::utf8()).lines, ["10 PRINT \"HI\""]);
        assert_eq!(crate::basic::list(&tape, "tap", crate::charset::utf8()).lines, ["10 PRINT \"HI\""], "a whole tape lists its program");
        for cut in 0..tape.len() {
            if let Ok(image) = super::super::open(Arc::new(tape[..cut].to_vec()), "cut.tap") {
                for entry in image.list(None).unwrap() {
                    let _ = image.read(&entry);
                }
            }
        }
    }
}
