//! Cassette images in `.cas` files: the TRS-80's and the MSX's, which share the ending and nothing else. Both are
//! the bytes the tape carried, not its sound, and each file on the tape is shown as a file.
//!
//! An MSX tape is a run of blocks, each after the eight bytes &1F &A6 &DE &BA &CC &13 &7D &74 that stand for the
//! tone before it. A file starts with a header block: ten bytes saying its kind (&D3 a tokenised BASIC program
//! saved by CSAVE, &EA an ASCII file, &D0 a binary saved by BSAVE) and a six-letter name. A BASIC program's block
//! that follows is the program as it was in memory from &8001; a binary's starts with where it loads, where it
//! ends and where it runs; an ASCII file follows in blocks of 256 bytes until one with &1A, its end, in it. This
//! is how openMSX and the MSX Technical Handbook describe the format.
//!
//! A TRS-80 tape is each file's leader and sync byte, then the file: at 500 baud a leader of zeros and &A5; at
//! 1500 baud (the Model III's high speed) &55s and &7F, after which every byte has its start bit kept, so the
//! file isn't lined up on bytes. A BASIC program starts &D3 &D3 &D3 and a one-letter name, then its lines up to a
//! link of zero. A SYSTEM (machine code) file starts &55 and a six-letter name, then blocks of &3C, a length (0
//! for 256), a load address, the bytes and a checksum, and ends &78 and where it runs. This is how trs80-tool
//! (github.com/lkesteloot/trs80) reads them. A SYSTEM file is given as the /CMD file that loads the same bytes.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

const MSX_BLOCK: [u8; 8] = [0x1F, 0xA6, 0xDE, 0xBA, 0xCC, 0x13, 0x7D, 0x74];

/// A file found on a tape.
struct TapeFile {
    name: String,
    open_name: String,
    kind: String,
    detail: String,
    data: Vec<u8>,
}

pub struct Cas {
    what: String,
    msx: bool,
    files: Vec<TapeFile>,
    notes: Vec<String>,
}

/// Whether a `.cas` file's start is an MSX or a TRS-80 tape's.
pub fn looks_like(head: &[u8]) -> bool {
    head.starts_with(&MSX_BLOCK) || trs80_header(head, 0).is_some()
}

impl Cas {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(16 * 1024 * 1024) as usize)?;
        if data.starts_with(&MSX_BLOCK) {
            let (files, notes) = msx_files(&data);
            return Ok(Self { what: format!("MSX cassette (CAS) \u{B7} {} file(s)", files.len()), msx: true, files, notes });
        }
        let (files, notes, speeds) = trs80_files(&data);
        if files.is_empty() {
            return Err("No TRS-80 or MSX files were found on this tape.".into());
        }
        Ok(Self { what: format!("TRS-80 cassette (CAS, {speeds}) \u{B7} {} file(s)", files.len()), msx: false, files, notes })
    }
}

impl Volume for Cas {
    fn describe(&self) -> String {
        self.what.clone()
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

    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        self.msx.then_some("msx")
    }
}

// ---------- MSX ----------

/// The blocks of an MSX tape: what follows each block marker up to the next, with the zeros that pad a block out to
/// a multiple of eight bytes still on.
fn msx_blocks(data: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> = (0..data.len().saturating_sub(7)).filter(|&at| data[at..at + 8] == MSX_BLOCK).collect();
    starts.iter().enumerate().map(|(i, &at)| &data[at + 8..starts.get(i + 1).copied().unwrap_or(data.len())]).collect()
}

/// Where an MSX BASIC program from a tape ends: after the zero link that ends it. The program is as it was in memory,
/// from &8001 on most machines but &C001 on one with 16K, so its links are followed from wherever they show it was.
/// None if they don't lead to its end.
fn msx_program_end(program: &[u8]) -> Option<usize> {
    let mut saved = vec![0xFF];
    saved.extend_from_slice(program);
    let found = crate::basic_more::microsoft_lines(&saved).filter(|found| found.whole)?;
    // The last line's zero, then the end link's two, less the &FF put before.
    found.lines.last().map(|(_, body)| (body.end + 2).min(program.len()))
}

fn msx_files(data: &[u8]) -> (Vec<TapeFile>, Vec<String>) {
    let blocks = msx_blocks(data);
    let (mut files, mut notes) = (Vec::new(), Vec::new());
    let mut index = 0;
    let headerless = |index: usize, block: &[u8]| TapeFile { name: format!("Block {}", index + 1), open_name: format!("Block {}.bin", index + 1), kind: "Block with no header".into(), detail: format!("{} bytes", block.len()), data: block.to_vec() };
    while index < blocks.len() {
        let block = blocks[index];
        let kind = block.first().copied().filter(|&k| block.len() >= 16 && matches!(k, 0xD3 | 0xEA | 0xD0) && block[..10].iter().all(|&b| b == k));
        let Some(kind) = kind else {
            files.push(headerless(index, block));
            index += 1;
            continue;
        };
        let name: String = block[10..16].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' }).collect::<String>().trim_end().to_string();
        let name = tidy_name(&name);
        let Some(&body) = blocks.get(index + 1) else {
            notes.push(format!("The tape ends after {name}'s header, before its contents."));
            break;
        };
        index += 2;
        match kind {
            0xD3 => {
                let end = msx_program_end(body);
                if end.is_none() {
                    notes.push(format!("{name}'s lines don't link up to an end, so all its block is kept."));
                }
                let mut program = vec![0xFF];
                program.extend_from_slice(&body[..end.unwrap_or(body.len())]);
                files.push(TapeFile { open_name: format!("{name}.bas"), kind: "BASIC program (CSAVE)".into(), detail: String::new(), data: program, name });
            }
            0xD0 => {
                if body.len() < 6 {
                    notes.push(format!("{name}'s block is too short to say where it loads."));
                    continue;
                }
                let word = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
                let (start, end, run) = (word(0), word(2), word(4));
                let length = usize::from(end.wrapping_sub(start)) + 1;
                if 6 + length > body.len() || end < start {
                    notes.push(format!("{name} should run from &H{start:04X} to &H{end:04X}, but its block is shorter, so it's cut short."));
                }
                // Given as BSAVE puts a binary on disk: &FE, then the three addresses and the bytes.
                let mut file = vec![0xFE];
                file.extend_from_slice(&body[..(6 + length).min(body.len())]);
                files.push(TapeFile { open_name: format!("{name}.bin"), kind: "Binary (BSAVE)".into(), detail: format!("&H{start:04X} to &H{end:04X}, runs at &H{run:04X}"), data: file, name });
            }
            _ => {
                // ASCII: blocks of 256 bytes up to the one with the end-of-file mark.
                let mut text = Vec::new();
                let mut part = body;
                loop {
                    let chunk = &part[..part.len().min(256)];
                    if let Some(stop) = chunk.iter().position(|&b| b == 0x1A) {
                        text.extend_from_slice(&chunk[..stop]);
                        break;
                    }
                    text.extend_from_slice(chunk);
                    match blocks.get(index) {
                        Some(next) if !is_msx_header(next) => {
                            part = next;
                            index += 1;
                        }
                        _ => {
                            notes.push(format!("{name} has no end-of-file mark, so it may be cut short."));
                            break;
                        }
                    }
                }
                files.push(TapeFile { open_name: format!("{name}.bas"), kind: "ASCII file".into(), detail: String::new(), data: text, name });
            }
        }
    }
    (files, notes)
}

fn is_msx_header(block: &[u8]) -> bool {
    block.len() >= 16 && matches!(block[0], 0xD3 | 0xEA | 0xD0) && block[..10].iter().all(|&b| b == block[0])
}

// ---------- TRS-80 ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Speed {
    /// 250 or 500 baud: whole bytes after the leader.
    Low,
    /// 1500 baud: each byte after the sync has a start bit before it.
    High,
}

/// The next file's start on a TRS-80 tape from `from` (a byte), as a bit position after its sync byte, and its speed.
fn trs80_header(data: &[u8], from: usize) -> Option<(usize, Speed)> {
    let mut recent: u32 = 0xFFFF_FFFF;
    for (at, &byte) in data.iter().enumerate().skip(from) {
        let before = recent;
        recent = (recent << 8) | u32::from(byte);
        // Low speed: three zeros of the leader and &A5, on byte boundaries.
        if recent == 0x0000_00A5 {
            return Some(((at + 1) * 8, Speed::Low));
        }
        // High speed: &55 &55 &55 &7F anywhere in the bits.
        let wide = (u64::from(before) << 8) | u64::from(byte);
        for shift in 0..8 {
            if (wide >> shift) as u32 == 0x5555_557F {
                return Some(((at + 1) * 8 - shift, Speed::High));
            }
        }
    }
    None
}

/// The bytes of a file from a bit position: whole bytes at low speed, every nine bits less the start bit at high.
fn trs80_bytes(data: &[u8], bit: usize, speed: Speed) -> Vec<u8> {
    if speed == Speed::Low {
        return data.get(bit / 8..).unwrap_or_default().to_vec();
    }
    let bit_at = |n: usize| data.get(n / 8).is_some_and(|b| b & (0x80 >> (n % 8)) != 0);
    let count = (data.len() * 8).saturating_sub(bit) / 9;
    (0..count).map(|i| (0..8).fold(0u8, |byte, k| byte << 1 | u8::from(bit_at(bit + i * 9 + 1 + k)))).collect()
}

/// How many bytes a Level II BASIC program takes on tape, from its &D3s to its zero link; None if it breaks off.
fn trs80_basic_length(file: &[u8]) -> Option<usize> {
    let mut at = 4;
    loop {
        let link = u16::from_le_bytes([*file.get(at)?, *file.get(at + 1)?]);
        if link == 0 {
            return Some(at + 2);
        }
        at = at + 4 + file.get(at + 4..)?.iter().position(|&b| b == 0)? + 1;
    }
}

/// A SYSTEM file as a /CMD file, how many bytes it took on tape, and anything wrong with it.
fn trs80_system(file: &[u8]) -> (Vec<u8>, usize, Option<u16>, Vec<String>) {
    let mut cmd = Vec::new();
    let mut notes = Vec::new();
    let mut at = 7;
    let mut entry = None;
    while let Some(&mark) = file.get(at) {
        match mark {
            0x3C => {
                let (Some(&length), Some(&low), Some(&high)) = (file.get(at + 1), file.get(at + 2), file.get(at + 3)) else {
                    notes.push("The tape ends partway through a block of the SYSTEM file.".into());
                    at = file.len();
                    break;
                };
                let length = if length == 0 { 256 } else { usize::from(length) };
                let Some(bytes) = file.get(at + 4..at + 4 + length) else {
                    notes.push("The tape ends partway through a block of the SYSTEM file.".into());
                    at = file.len();
                    break;
                };
                let sum = bytes.iter().fold(low.wrapping_add(high), |sum, b| sum.wrapping_add(*b));
                if file.get(at + 4 + length) != Some(&sum) {
                    notes.push(format!("The block loading at &H{:04X} doesn't match its checksum.", u16::from_le_bytes([low, high])));
                }
                // A /CMD load record: its length counts the address, and 256 + 2 is written as 2.
                cmd.extend([0x01, (length + 2) as u8, low, high]);
                cmd.extend_from_slice(bytes);
                at += 5 + length;
            }
            0x78 if at + 3 <= file.len() => {
                let run = u16::from_le_bytes([file[at + 1], file[at + 2]]);
                cmd.extend([0x02, 0x02, file[at + 1], file[at + 2]]);
                entry = Some(run);
                at += 3;
                break;
            }
            _ => {
                notes.push(format!("The SYSTEM file breaks off at byte {at} with &{mark:02X}, where a block should start."));
                break;
            }
        }
    }
    if entry.is_none() && notes.is_empty() {
        notes.push("The SYSTEM file has no address to run from: the tape ends before it.".into());
    }
    (cmd, at, entry, notes)
}

/// The files on a TRS-80 tape, any notes, and the speeds they were saved at.
fn trs80_files(data: &[u8]) -> (Vec<TapeFile>, Vec<String>, String) {
    let (mut files, mut notes) = (Vec::new(), Vec::new());
    let mut speeds = Vec::new();
    let mut from = 0;
    while let Some((bit, speed)) = trs80_header(data, from) {
        let file = trs80_bytes(data, bit, speed);
        let label = if speed == Speed::Low { "500 baud" } else { "1500 baud" };
        if !speeds.contains(&label) {
            speeds.push(label);
        }
        let number = files.len() + 1;
        let used = if file.starts_with(&[0xD3, 0xD3, 0xD3]) && file.len() > 4 {
            let letter = char::from(file[3]);
            let name = if letter.is_ascii_graphic() { letter.to_string() } else { format!("Program {number}") };
            let length = trs80_basic_length(&file);
            if length.is_none() {
                notes.push(format!("The BASIC program {name} breaks off before its end."));
            }
            let length = length.unwrap_or(file.len());
            let mut program = vec![0xFF];
            program.extend_from_slice(&file[4..length]);
            files.push(TapeFile { open_name: format!("{}.bas", tidy_name(&name)), kind: "BASIC program".into(), detail: label.into(), data: program, name });
            length
        } else if file.first() == Some(&0x55) && file.len() > 7 {
            let name: String = file[1..7].iter().map(|&b| char::from(b & 0x7F)).collect::<String>().trim_end().to_string();
            let name = tidy_name(&name);
            let (cmd, length, run, problems) = trs80_system(&file);
            notes.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
            let detail = run.map_or_else(|| label.to_string(), |run| format!("runs at &H{run:04X}, {label}"));
            files.push(TapeFile { open_name: format!("{name}.cmd"), kind: "SYSTEM (machine code)".into(), detail, data: cmd, name });
            length
        } else {
            // Anything else runs to the next file's leader, or the end of the tape.
            let next = trs80_header(data, bit.div_ceil(8)).map_or(data.len(), |(next, _)| next / 8);
            let length = if speed == Speed::Low { next.saturating_sub(bit / 8) } else { (next * 8).saturating_sub(bit) / 9 };
            // Less the next file's leader and sync byte, if there is a next file.
            let mut bytes = file[..length.min(file.len())].to_vec();
            if next < data.len() && bytes.last() == Some(&0xA5) {
                bytes.pop();
                while bytes.last() == Some(&0) {
                    bytes.pop();
                }
            }
            files.push(TapeFile { name: format!("File {number}"), open_name: format!("File {number}.bin"), kind: "Data".into(), detail: format!("{} bytes, {label}", bytes.len()), data: bytes });
            length.max(1)
        };
        // On to the next leader, from the first whole byte after this file.
        from = if speed == Speed::Low { bit / 8 + used } else { (bit + used * 9).div_ceil(8) };
    }
    (files, notes, speeds.join(" and "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trs80_basic(lines: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = vec![0xD3, 0xD3, 0xD3, b'A'];
        let mut address = 0x42E9u16;
        for (number, body) in lines {
            address += 5 + body.len() as u16;
            out.extend(address.to_le_bytes());
            out.extend(number.to_le_bytes());
            out.extend_from_slice(body);
            out.push(0);
        }
        out.extend([0, 0]);
        out
    }

    /// Bytes as the 1500-baud tape keeps them: a leader of &55s and &7F, then every byte after a start bit.
    fn high_speed(file: &[u8]) -> Vec<u8> {
        let mut bits: Vec<bool> = Vec::new();
        for byte in [0x55u8; 8].iter().chain(&[0x7F]) {
            bits.extend((0..8).map(|k| byte & (0x80 >> k) != 0));
        }
        for byte in file {
            bits.push(false);
            bits.extend((0..8).map(|k| byte & (0x80 >> k) != 0));
        }
        // Not on a byte boundary, as such a tape usually isn't.
        let mut shifted = vec![true, false, true];
        shifted.extend(bits);
        shifted.chunks(8).map(|bits| bits.iter().enumerate().fold(0u8, |b, (k, &on)| b | (u8::from(on) << (7 - k)))).collect()
    }

    #[test]
    fn a_trs80_tape_holds_a_basic_program_and_a_system_file() {
        let program = trs80_basic(&[(10, b"\xB2\"HI\""), (20, b"\x8D 10")]);
        let mut tape = vec![0u8; 256];
        tape.push(0xA5);
        tape.extend(&program);
        tape.extend(vec![0u8; 64]);
        tape.push(0xA5);
        // SYSTEM file "GAME", one block of 3 bytes at &7000, running at &7000.
        tape.extend([0x55, b'G', b'A', b'M', b'E', b' ', b' ', 0x3C, 3, 0x00, 0x70, 1, 2, 3, 0x76, 0x78, 0x00, 0x70]);
        assert!(looks_like(&tape));
        let cas = super::super::open(Arc::new(tape), "games.cas").unwrap();
        assert!(cas.describe().contains("TRS-80") && cas.describe().contains("500 baud"), "{}", cas.describe());
        let files = cas.list(None).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].open_name, "A.bas");
        let bytes = cas.read(&files[0]).unwrap();
        assert_eq!(bytes[0], 0xFF, "given as on disk");
        assert_eq!(crate::basic::list(&bytes, "bas", crate::charset::utf8()).lines, ["10 PRINT\"HI\"", "20 GOTO 10"]);
        assert_eq!(files[1].open_name, "GAME.cmd");
        assert_eq!(cas.read(&files[1]).unwrap(), [0x01, 5, 0x00, 0x70, 1, 2, 3, 0x02, 0x02, 0x00, 0x70]);
        assert!(cas.notes().is_empty(), "{:?}", cas.notes());
    }

    #[test]
    fn a_high_speed_tape_is_read_through_its_start_bits() {
        let program = trs80_basic(&[(10, b"\xB2 1")]);
        let cas = super::super::open(Arc::new(high_speed(&program)), "fast.cas").unwrap();
        assert!(cas.describe().contains("1500 baud"), "{}", cas.describe());
        let files = cas.list(None).unwrap();
        assert_eq!(crate::basic::list(&cas.read(&files[0]).unwrap(), "bas", crate::charset::utf8()).lines, ["10 PRINT 1"]);
    }

    #[test]
    fn an_msx_tape_holds_basic_ascii_and_binary_files() {
        let mut tape = Vec::new();
        let block = |tape: &mut Vec<u8>, bytes: &[u8]| {
            tape.extend(MSX_BLOCK);
            tape.extend_from_slice(bytes);
            while !tape.len().is_multiple_of(8) {
                tape.push(0);
            }
        };
        let header = |kind: u8, name: &[u8; 6]| [vec![kind; 10], name.to_vec()].concat();
        // 10 PRINT 1, from &8001.
        block(&mut tape, &header(0xD3, b"HELLO "));
        block(&mut tape, &[0x09, 0x80, 0x0A, 0x00, 0x91, 0x20, 0x12, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0]);
        block(&mut tape, &header(0xEA, b"TEXT  "));
        let mut text = b"10 PRINT 2\r\n".to_vec();
        text.resize(256, 0x1A);
        block(&mut tape, &text);
        block(&mut tape, &header(0xD0, b"CODE  "));
        block(&mut tape, &[0x00, 0xC0, 0x02, 0xC0, 0x00, 0xC0, 0xC9, 0x00, 0xC9]);
        let cas = super::super::open(Arc::new(tape), "msx.cas").unwrap();
        assert!(cas.describe().contains("MSX"));
        let files = cas.list(None).unwrap();
        assert_eq!(files.iter().map(|f| f.open_name.as_str()).collect::<Vec<_>>(), ["HELLO.bas", "TEXT.bas", "CODE.bin"]);
        let basic = cas.read(&files[0]).unwrap();
        assert_eq!(basic.len(), 1 + 10, "cut after the end link");
        assert_eq!(crate::basic::list(&basic, "bas", crate::charset::known("msx")).lines, ["10 PRINT 1"]);
        assert_eq!(cas.read(&files[1]).unwrap(), b"10 PRINT 2\r\n");
        assert!(!crate::basic::is_program_file("bas", Some(&cas.read(&files[1]).unwrap())), "an ASCII program is text");
        assert_eq!(cas.read(&files[2]).unwrap(), [0xFE, 0x00, 0xC0, 0x02, 0xC0, 0x00, 0xC0, 0xC9, 0x00, 0xC9]);
        assert_eq!(files[2].detail, "&HC000 to &HC002, runs at &HC000");
        assert_eq!(cas.text_preset(&files[1]), Some("msx"));
    }

    #[test]
    fn damaged_tapes_say_so_and_never_panic() {
        assert!(super::super::open(Arc::new(vec![0u8; 40]), "x.cas").is_err());
        let mut tape = vec![0u8; 8];
        tape.push(0xA5);
        tape.extend([0xD3, 0xD3, 0xD3, b'X', 0x10, 0x43, 10, 0, 0xB2]);
        let cas = super::super::open(Arc::new(tape.clone()), "cut.cas").unwrap();
        assert!(cas.notes()[0].contains("breaks off"), "{:?}", cas.notes());
        for cut in 0..tape.len() {
            let _ = super::super::open(Arc::new(tape[..cut].to_vec()), "cut.cas");
        }
        let mut msx = MSX_BLOCK.to_vec();
        msx.extend([0xD0; 10]);
        msx.extend(b"SHORT ");
        msx.extend(MSX_BLOCK);
        msx.extend([0x00, 0xC0, 0xFF, 0xC0]);
        let cas = super::super::open(Arc::new(msx.clone()), "short.cas").unwrap();
        assert!(cas.notes()[0].contains("too short"), "{:?}", cas.notes());
        for cut in 0..msx.len() {
            if let Ok(cas) = super::super::open(Arc::new(msx[..cut].to_vec()), "short.cas") {
                for entry in cas.list(None).unwrap() {
                    let _ = cas.read(&entry);
                }
            }
        }
    }
}
