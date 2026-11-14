//! Cassette images in `.cas` files: the TRS-80's, the MSX's and the Color Computer's and Dragon's, which share the
//! ending and nothing else. Each is the bytes the tape carried, not its sound, and each file on the tape is shown
//! as a file.
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
//!
//! A Color Computer or Dragon tape is blocks, each after a leader of &55s and the sync byte &3C: a type (0 the name
//! block that starts a file, 1 data, &FF the end of the file), a length, up to 255 bytes and a checksum. A name
//! block gives an eight-letter name, the file's kind (0 BASIC, 1 data, 2 machine code), whether it's ASCII, and
//! where it runs and loads. This is how the Color Computer's and Dragon's manuals and ToolShed's cecb read them. A
//! tokenised BASIC program is given as Disk BASIC or Dragon DOS would keep it, so it lists; which of the two
//! machines' BASICs it's in is told from its tokens.

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
    head.starts_with(&MSX_BLOCK) || looks_like_coco(head) || trs80_header(head, 0).is_some()
}

impl Cas {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(16 * 1024 * 1024) as usize)?;
        if data.starts_with(&MSX_BLOCK) {
            let (files, notes) = msx_files(&data);
            return Ok(Self { what: format!("MSX cassette (CAS) \u{B7} {} file(s)", files.len()), msx: true, files, notes });
        }
        if looks_like_coco(&data) {
            let (files, notes) = coco_files(&data);
            return Ok(Self { what: format!("Color Computer or Dragon cassette (CAS) \u{B7} {} file(s)", files.len()), msx: false, files, notes });
        }
        // A CoCo or Dragon leader with no file in the standard format after it is a tape with its own loader, not
        // a TRS-80's, whose leader is followed by its sync byte.
        let leader = data.iter().take_while(|&&b| b == 0x55).count();
        if leader >= 16 && trs80_header(&data, 0).is_none_or(|(bit, _)| bit / 8 > leader + 64) {
            return Err("This tape starts with a Color Computer or Dragon leader, but no file in their standard format follows it: it may load with a loader of its own.".into());
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

// ---------- Color Computer and Dragon ----------

/// A byte from a bit position on a Color Computer or Dragon tape, whose bits were sent lowest first: a `.cas` file
/// made from a recording isn't always lined up on bytes.
fn lsb_byte(data: &[u8], bit: usize) -> Option<u8> {
    let (at, shift) = (bit / 8, bit % 8);
    let low = *data.get(at)?;
    if shift == 0 {
        return Some(low);
    }
    Some(((u16::from(*data.get(at + 1)?) << 8 | u16::from(low)) >> shift) as u8)
}

/// A block on a CoCo or Dragon tape: its type (0 a file's name block, 1 data, &FF the end), its bytes, and whether
/// its checksum is right.
struct CocoBlock {
    kind: u8,
    bytes: Vec<u8>,
    good: bool,
}

/// The blocks of a CoCo or Dragon tape: each &3C sync after the leader of &55s, then the block's type, its length,
/// its bytes and a checksum, the sum of all of those. A sync is looked for at every bit, as the bytes may not be
/// lined up, and the leader's last byte is often spoilt; a block is believed when its checksum is right, or when
/// it follows a clean leader (and is then marked bad). Anything else is taken to be other bytes, such as a
/// program's own loader's.
fn coco_blocks(data: &[u8], most: usize) -> Vec<CocoBlock> {
    let mut blocks = Vec::new();
    let mut bit = 0;
    let end = data.len() * 8;
    while bit + 24 <= end && blocks.len() < most {
        if lsb_byte(data, bit) != Some(0x3C) {
            bit += 1;
            continue;
        }
        let (Some(kind), Some(length)) = (lsb_byte(data, bit + 8), lsb_byte(data, bit + 16)) else { break };
        let length = usize::from(length);
        let bytes: Option<Vec<u8>> = (0..=length).map(|i| lsb_byte(data, bit + 24 + i * 8)).collect();
        let fits = matches!((kind, length), (0, 15..) | (1, _) | (0xFF, 0));
        let Some(mut bytes) = bytes.filter(|_| fits) else {
            bit += 1;
            continue;
        };
        let checksum = bytes.pop().unwrap_or_default();
        let good = bytes.iter().fold(kind.wrapping_add(length as u8), |sum, &b| sum.wrapping_add(b)) == checksum;
        // A name block says its file is one of the four kinds, binary or ASCII.
        let named = kind != 0 || (bytes[8] <= 3 && matches!(bytes[9], 0 | 0xFF));
        if !named || (!good && (bit < 8 || lsb_byte(data, bit - 8) != Some(0x55))) {
            bit += 1;
            continue;
        }
        blocks.push(CocoBlock { kind, bytes, good });
        bit += 24 + (length + 1) * 8;
    }
    blocks
}

/// Whether a `.cas` file is a Color Computer or Dragon tape: a name block, with a right checksum, near its start.
fn looks_like_coco(head: &[u8]) -> bool {
    coco_blocks(&head[..head.len().min(16384)], 1).first().is_some_and(|block| block.kind == 0 && block.good)
}

/// Whether a tokenised 6809 BASIC program is the Dragon's rather than the Color Computer's: in the Dragon's BASIC,
/// GO is followed by its own tokens for TO and SUB (&BC and &BD, against &A5 and &A6), and IF by its THEN (&BF,
/// against &A7). Bytes in quotes, which may be graphics characters, aren't counted. Above 0 for the Dragon, below
/// for the Color Computer.
fn dragon_evidence(program: &[u8]) -> i32 {
    let (mut dragon, mut coco) = (0, 0);
    let mut at = 0;
    // Each line: a link, a line number, the line up to a zero.
    while let Some(line) = program.get(at + 4..).and_then(|rest| rest.iter().position(|&b| b == 0).map(|end| &rest[..end])) {
        at += 4 + line.len() + 1;
        let mut quoted = false;
        let mut previous = 0;
        for &byte in line {
            match (previous, byte) {
                (_, b'"') => quoted = !quoted,
                _ if quoted => {}
                (0x81, 0xBC | 0xBD) | (_, 0xBF) => dragon += 1,
                (0x81, 0xA5 | 0xA6) | (_, 0xA7) => coco += 1,
                _ => {}
            }
            previous = byte;
        }
    }
    dragon - coco
}

/// A CoCo or Dragon BASIC program from tape, given as its Disk BASIC (or Dragon DOS) file, so it lists as one.
fn coco_basic_file(program: &[u8], load: u16, dragon: bool) -> (Vec<u8>, &'static str) {
    let length = (program.len() as u16).to_be_bytes();
    if dragon {
        // Dragon DOS's header: &55, type 1, where it loads, its length, where it runs, &AA.
        let mut file = vec![0x55, 1];
        file.extend(load.to_be_bytes());
        file.extend(length);
        file.extend([0x8B, 0x8D, 0xAA]);
        file.extend_from_slice(program);
        (file, "BASIC program (Dragon)")
    } else {
        let mut file = vec![0xFF];
        file.extend(length);
        file.extend_from_slice(program);
        (file, "BASIC program (Color Computer)")
    }
}

fn coco_files(data: &[u8]) -> (Vec<TapeFile>, Vec<String>) {
    let (mut files, mut notes) = (Vec::new(), Vec::new());
    let blocks = coco_blocks(data, usize::MAX);
    // Each tokenised BASIC program's place in the files, where it was in memory, and which machine its tokens say.
    let mut basic = Vec::new();
    let mut index = 0;
    while index < blocks.len() {
        let number = files.len() + 1;
        let header = blocks[index].kind == 0;
        let name_block = header.then(|| &blocks[index]);
        if header {
            index += 1;
        }
        // The file's data blocks, up to the end block or the next file's name block.
        let mut bytes = Vec::new();
        let (mut bad, mut ended) = (0, false);
        while let Some(block) = blocks.get(index) {
            match block.kind {
                0 => break,
                0xFF => {
                    index += 1;
                    ended = true;
                    break;
                }
                _ => {
                    bytes.extend_from_slice(&block.bytes);
                    bad += usize::from(!block.good);
                    index += 1;
                }
            }
        }
        let Some(name_block) = name_block else {
            notes.push(format!("{} bytes of data on the tape have no name block before them.", bytes.len()));
            files.push(TapeFile { name: format!("Block {number}"), open_name: format!("Block {number}.bin"), kind: "Data with no name".into(), detail: format!("{} bytes", bytes.len()), data: bytes });
            continue;
        };
        let head = &name_block.bytes;
        let name = tidy_name(&head[..8].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' }).collect::<String>());
        let name = if name == "_" { format!("File {number}") } else { name };
        let (kind, ascii) = (head[8], head[9] == 0xFF);
        let (run, load) = (u16::from_be_bytes([head[11], head[12]]), u16::from_be_bytes([head[13], head[14]]));
        if !name_block.good {
            notes.push(format!("{name}'s name block fails its checksum, so what it says of the file may be wrong."));
        }
        if bad > 0 {
            notes.push(format!("{bad} of {name}'s blocks fail their checksums, so some of its bytes are wrong."));
        }
        if !ended {
            notes.push(format!("{name} has no end block, so it may be cut short."));
        }
        let file = match (kind, ascii) {
            (0, true) => TapeFile { open_name: format!("{name}.bas"), kind: "BASIC program (ASCII)".into(), detail: String::new(), data: bytes, name },
            (0, false) => {
                // Where the program was in memory, from its first line's link: it ends where the second line starts.
                let first_end = bytes.get(4..).and_then(|rest| rest.iter().position(|&b| b == 0)).map(|end| end + 5);
                let start = first_end.zip(bytes.get(..2)).map_or(0, |(end, link)| u16::from_be_bytes([link[0], link[1]]).wrapping_sub(end as u16));
                basic.push((files.len(), start, dragon_evidence(&bytes)));
                TapeFile { open_name: format!("{name}.bas"), kind: String::new(), detail: String::new(), data: bytes, name }
            }
            (2, _) => TapeFile { open_name: format!("{name}.bin"), kind: "Machine code".into(), detail: format!("loads at &{load:04X}, runs at &{run:04X}"), data: bytes, name },
            (1, true) => TapeFile { open_name: format!("{name}.txt"), kind: "Data (ASCII)".into(), detail: String::new(), data: bytes, name },
            _ => TapeFile { open_name: format!("{name}.dat"), kind: "Data".into(), detail: String::new(), data: bytes, name },
        };
        files.push(file);
    }
    // A program whose tokens don't say which machine it's for goes with the others on the tape.
    let lean: i32 = basic.iter().map(|(_, _, evidence)| evidence.signum()).sum();
    for (index, start, evidence) in basic {
        let (data, what) = coco_basic_file(&files[index].data, start, evidence > 0 || (evidence == 0 && lean > 0));
        (files[index].data, files[index].kind) = (data, what.into());
    }
    (files, notes)
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

    /// A CoCo or Dragon tape file: a leader and name block, data blocks of up to 255 bytes, and the end block.
    fn coco_file(name: &[u8; 8], kind: u8, load: u16, run: u16, data: &[u8]) -> Vec<u8> {
        let block = |kind: u8, bytes: &[u8]| {
            let mut out = vec![0x55; 2];
            out.extend([0x3C, kind, bytes.len() as u8]);
            out.extend(bytes);
            out.push(bytes.iter().fold(kind.wrapping_add(bytes.len() as u8), |sum, &b| sum.wrapping_add(b)));
            out
        };
        let mut head = name.to_vec();
        head.extend([kind, 0, 0]);
        head.extend(run.to_be_bytes());
        head.extend(load.to_be_bytes());
        let mut tape = vec![0x55; 128];
        tape.extend(block(0, &head));
        tape.extend([0x55; 128]);
        for chunk in data.chunks(255) {
            tape.extend(block(1, chunk));
        }
        tape.extend(block(0xFF, &[]));
        tape
    }

    /// A 6809 BASIC program from &1E01, `10 IF A THEN 10` and `20 GOTO 10` (stored with no spaces), in the given
    /// tokens for THEN and TO.
    fn basic_6809(then: u8, to: u8) -> Vec<u8> {
        let lines: [&[u8]; 2] = [&[0x85, b'A', then, b'1', b'0'], &[0x81, to, b'1', b'0']];
        let mut out = Vec::new();
        let mut address = 0x1E01u16;
        for (number, body) in [10u16, 20].iter().zip(lines) {
            address += 5 + body.len() as u16;
            out.extend(address.to_be_bytes());
            out.extend(number.to_be_bytes());
            out.extend(body);
            out.push(0);
        }
        out.extend([0, 0]);
        out
    }

    #[test]
    fn a_color_computer_or_dragon_tape_gives_its_files() {
        let code: Vec<u8> = (0..600).map(|i| i as u8).collect();
        let mut tape = coco_file(b"DRAGON  ", 0, 0, 0, &basic_6809(0xBF, 0xBC));
        tape.extend(coco_file(b"COCO    ", 0, 0, 0, &basic_6809(0xA7, 0xA5)));
        tape.extend(coco_file(b"GAME    ", 2, 0x0600, 0x0612, &code));
        assert!(looks_like(&tape));
        let cas = super::super::open(Arc::new(tape.clone()), "games.cas").unwrap();
        assert_eq!(cas.describe(), "Color Computer or Dragon cassette (CAS) \u{B7} 3 file(s)");
        assert!(cas.notes().is_empty(), "{:?}", cas.notes());
        let files = cas.list(None).unwrap();
        assert_eq!(files.iter().map(|f| f.kind.as_str()).collect::<Vec<_>>(), ["BASIC program (Dragon)", "BASIC program (Color Computer)", "Machine code"]);
        let dragon = crate::basic::list(&cas.read(&files[0]).unwrap(), "bas", crate::charset::utf8());
        assert_eq!((dragon.dialect, dragon.lines.as_slice()), ("Dragon BASIC", ["10 IFATHEN10".to_string(), "20 GOTO10".to_string()].as_slice()));
        let coco = crate::basic::list(&cas.read(&files[1]).unwrap(), "bas", crate::charset::utf8());
        assert_eq!(coco.lines, ["10 IFATHEN10", "20 GOTO10"]);
        assert_eq!((files[2].open_name.as_str(), files[2].detail.as_str()), ("GAME.bin", "loads at &0600, runs at &0612"));
        assert_eq!(cas.read(&files[2]).unwrap(), code);

        // The same tape a bit out of step, as some made from recordings are: its bits are read across the bytes.
        let mut shifted = vec![0u8; tape.len() + 1];
        for (i, &byte) in tape.iter().enumerate() {
            shifted[i] |= byte << 3;
            shifted[i + 1] |= byte >> 5;
        }
        let cas = super::super::open(Arc::new(shifted), "shifted.cas").unwrap();
        assert_eq!(cas.read(&cas.list(None).unwrap()[2]).unwrap(), code);

        // A spoilt block says so; cut short anywhere, it never panics.
        let mut spoilt = tape.clone();
        let at = tape.len() - 40;
        spoilt[at] ^= 0xFF;
        let cas = super::super::open(Arc::new(spoilt), "spoilt.cas").unwrap();
        assert!(cas.notes().iter().any(|n| n.contains("fail their checksums")), "{:?}", cas.notes());
        for cut in 0..tape.len() {
            if let Ok(cas) = super::super::open(Arc::new(tape[..cut].to_vec()), "cut.cas") {
                for entry in cas.list(None).unwrap() {
                    let _ = cas.read(&entry);
                }
            }
        }
        // A leader with nothing standard after it is a tape with its own loader.
        let mut own = vec![0x55; 200];
        own.extend((0..2000).map(|i| (i * 7) as u8));
        assert!(Cas::open(Arc::new(own)).is_err());
    }
}
