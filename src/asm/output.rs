//! What an assembled program is saved as, to run: for a Spectrum, a tape whose BASIC loader loads the code and
//! calls it; for a Spectrum Next, a .nex file the Next's browser runs.

use super::Assembly;
use crate::basic;

/// The lowest address a tape's loader can load code at: above the BASIC loader itself and the stack.
pub const LOWEST_FOR_TAPE: i64 = 24000;

/// A .tap: a BASIC loader (CLEAR, LOAD "" CODE, RANDOMIZE USR), then the code as one block.
pub fn spectrum_tape(assembly: &Assembly, stem: &str) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low < LOWEST_FOR_TAPE {
        return Err(crate::i18n::tf!("The code starts at {low} (${low:04X}), below {LOWEST_FOR_TAPE}, where the BASIC loader and the system need the memory. Put ORG 32768 (or anywhere from {LOWEST_FOR_TAPE}) before it."));
    }
    if high > 0x10000 {
        return Err(crate::i18n::tr("The code goes past the top of the Spectrum's 64K.").into());
    }
    let start = assembly.start().unwrap_or(low);
    let name: String = stem.chars().take(10).collect();
    let loader = format!("10 CLEAR VAL \"{}\": LOAD \"\"CODE : RANDOMIZE USR VAL \"{}\"", low - 1, start);
    let program = basic::tokenise_spectrum(&[loader], basic::ZxDialect::Spectrum)?;
    let mut tape = basic::spectrum_tape(&program, &name, Some(10));
    let code = assembly.image(low, high);
    let mut header = vec![3u8];
    let mut title: Vec<u8> = name.bytes().filter(|b| (32..127).contains(b)).take(10).collect();
    title.resize(10, b' ');
    header.extend(title);
    header.extend((code.len() as u16).to_le_bytes());
    header.extend((low as u16).to_le_bytes());
    header.extend(32768u16.to_le_bytes());
    tape.extend(basic::tap_block(0x00, &header));
    tape.extend(basic::tap_block(0xFF, &code));
    Ok(tape)
}

/// A Commodore program (.prg): its load address, then the bytes. Code that doesn't start at $0801, where BASIC
/// programs go, has a BASIC line put there to start it, as 10 SYS 49152, so that RUN starts it.
pub fn commodore_program(assembly: &Assembly) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if high > 0x10000 {
        return Err(crate::i18n::tr("The code goes past the top of the 64K.").into());
    }
    const BASIC: i64 = 0x0801;
    let start = assembly.start().unwrap_or(low);
    if low <= BASIC {
        let mut prg = (low as u16).to_le_bytes().to_vec();
        prg.extend(assembly.image(low, high));
        return Ok(prg);
    }
    // 10 SYS start: the link to the next line, the line number, SYS's token, the address, and the end.
    let digits = start.to_string();
    let line_end = BASIC + 2 + 2 + 1 + 1 + digits.len() as i64 + 1;
    if low < line_end + 2 {
        return Err(crate::i18n::tf!("The code starts at ${low:04X}, too close after BASIC's start at $0801 for a line to start it. Start it at $0801 with a BASIC line of its own, or at $0810 or above."));
    }
    let mut prg = (BASIC as u16).to_le_bytes().to_vec();
    prg.extend((line_end as u16).to_le_bytes());
    prg.extend(10u16.to_le_bytes());
    prg.push(0x9E);
    prg.push(b' ');
    prg.extend(digits.bytes());
    prg.push(0);
    prg.extend([0, 0]);
    prg.resize(2 + (low - BASIC) as usize, 0);
    prg.extend(assembly.image(low, high));
    Ok(prg)
}

/// The BBC Micro's MOS calls, which no Commodore KERNAL routine shares an address with: OSFIND, OSGBPB, OSBPUT,
/// OSBGET, OSARGS, OSFILE, OSRDCH, OSASCI, OSWRCR, OSWRCH, OSWORD, OSBYTE and OSCLI. (OSNEWL, &FFE7, is left out:
/// it's the C64's CLALL too.)
const BBC_MOS_CALLS: [i64; 13] = [0xFFCE, 0xFFD1, 0xFFD4, 0xFFD7, 0xFFDA, 0xFFDD, 0xFFE0, 0xFFE3, 0xFFEC, 0xFFEE, 0xFFF1, 0xFFF4, 0xFFF7];

/// The machines 6502 source is run on, when F9 assembles it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Micro6502 {
    C64,
    Bbc,
    Atari2600,
}

impl Micro6502 {
    /// What a `; machine:` line near the top of the source says, if one does: `c64` (or `commodore 64`) or `bbc`
    /// (or `bbc micro`), after `;`, `//` or `*`, in any case.
    pub fn said_in<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Micro6502> {
        lines.into_iter().take(50).find_map(|line| {
            let line = line.trim_start();
            let comment = line.strip_prefix("//").or_else(|| line.strip_prefix(';')).or_else(|| line.strip_prefix('*'))?;
            let comment = comment.trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase();
            let value = comment.strip_prefix("machine")?.trim_start().strip_prefix(':')?.trim();
            match value {
                "c64" | "commodore 64" | "commodore64" => Some(Micro6502::C64),
                "bbc" | "bbc micro" | "bbcmicro" => Some(Micro6502::Bbc),
                "atari 2600" | "atari2600" | "2600" | "vcs" | "atari vcs" => Some(Micro6502::Atari2600),
                _ => None,
            }
        })
    }
}

/// Which machine 6502 code is for, when the code says. A BBC Micro's calls (JSR or JMP) its MOS or uses
/// its I/O pages (FRED, JIM and SHEILA, &FC00 to &FEFF); a Commodore 64's calls the KERNAL (its jump table from $FF81 to $FFF3, one every three bytes) or uses its chips
/// ($D000 to $DFFF). Code that does neither, or both, doesn't say, unless it starts below &1900, which a BBC Micro's
/// disc can't load: that's a C64's.
pub fn machine_6502(assembly: &Assembly) -> Option<Micro6502> {
    // Each instruction with an absolute address (JSR, JMP and the absolute, indexed and indirect forms), and the
    // address. A line of three bytes of data can look like one.
    let absolute = |op: u8| matches!(op & 0x0F, 0x0C..=0x0F) || matches!(op & 0x1F, 0x19 | 0x1B) || op == 0x20;
    let used = || {
        assembly.lines.iter().filter_map(move |placed| match placed.bytes[..] {
            [op, low, high] if absolute(op) => Some((op, i64::from(u16::from_le_bytes([low, high])))),
            _ => None,
        })
    };
    let called = |(op, address): (u8, i64)| matches!(op, 0x20 | 0x4C).then_some(address);
    // $FFE7, the C64's CLALL, is the BBC's OSNEWL as well, so it says neither.
    let kernal = |address: i64| (0xFF81..=0xFFF3).contains(&address) && (address - 0xFF81) % 3 == 0 && address != 0xFFE7;
    // An Atari 2600 cartridge: code that waits for WSYNC, filling a cartridge's room up to its vectors.
    if let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest())
        && assembly.lines.iter().any(|p| p.bytes[..] == [0x85, 0x02])
        && (matches!(high - low, 2048 | 4096) && high & 0xFFF == 0 || matches!(high - low, 8192 | 12288 | 16384 | 32768) && low == 0)
    {
        return Some(Micro6502::Atari2600);
    }
    let bbc = used().filter_map(called).any(|a| BBC_MOS_CALLS.contains(&a)) || used().any(|(_, a)| (0xFC00..=0xFEFF).contains(&a));
    let c64 = used().filter_map(called).any(kernal) || used().any(|(_, a)| (0xD000..=0xDFFF).contains(&a));
    match (bbc, c64) {
        (true, false) => Some(Micro6502::Bbc),
        (false, true) => Some(Micro6502::C64),
        (true, true) => None,
        (false, false) => assembly.lowest().filter(|low| *low < LOWEST_FOR_BBC_DISC).map(|_| Micro6502::C64),
    }
}

/// The lowest address machine code is loaded at from a BBC Micro's disc: PAGE with the DFS, above its workspace.
pub const LOWEST_FOR_BBC_DISC: i64 = 0x1900;

/// A BBC Micro program: where it loads, where it starts, and its bytes, for *RUN.
pub fn bbc_program(assembly: &Assembly) -> Result<(u16, u16, Vec<u8>), String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low < LOWEST_FOR_BBC_DISC {
        return Err(crate::i18n::tf!("The code starts at &{low:04X}, below &1900, where the disc system needs the memory to load it. Put it at &1900 or above (such as *=$1900 or ORG &2000)."));
    }
    if high > 0x8000 {
        return Err(crate::i18n::tf!("The code goes up to &{:04X}, past &7FFF, the top of the BBC Micro's memory.", high - 1));
    }
    let start = assembly.start().unwrap_or(low);
    Ok((low as u16, start as u16, assembly.image(low, high)))
}

/// A Color Computer binary (Disk Extended Color BASIC's LOADM file): each run of bytes after a header saying
/// where it goes, then where to start. Dragons load it too, with a DECB loader.
pub fn coco_binary(assembly: &Assembly) -> Result<Vec<u8>, String> {
    if assembly.lowest().is_none() {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    }
    let mut file = Vec::new();
    for chunk in assembly.chunks.iter().filter(|c| !c.bytes.is_empty()) {
        for (i, piece) in chunk.bytes.chunks(0xFFFF).enumerate() {
            file.push(0);
            file.extend((piece.len() as u16).to_be_bytes());
            file.extend(((chunk.address + i as i64 * 0xFFFF) as u16).to_be_bytes());
            file.extend_from_slice(piece);
        }
    }
    file.extend([0xFF, 0, 0]);
    file.extend((assembly.start().unwrap_or(0) as u16).to_be_bytes());
    Ok(file)
}

/// An Atari ST program (TOS's executable, .TOS or .PRG): a header, the code, and the table of the addresses in
/// it that TOS moves to where it loads it. `moved` is the same program assembled `shift` further on, to find them.
pub fn atari_program(assembly: &Assembly, moved: &Assembly, shift: i64) -> Result<Vec<u8>, String> {
    let Some(high) = assembly.highest() else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    let mut text = assembly.image(0, high);
    if text.len() % 2 == 1 {
        text.push(0);
    }
    let there = moved.image(shift, shift + text.len() as i64);
    // Every long that's moved by the shift is an address in the program.
    let mut places = Vec::new();
    let mut at = 0;
    while at + 4 <= text.len() {
        let here = u32::from_be_bytes([text[at], text[at + 1], text[at + 2], text[at + 3]]);
        let moved = u32::from_be_bytes([there[at], there[at + 1], there[at + 2], there[at + 3]]);
        if moved.wrapping_sub(here) == shift as u32 {
            places.push(at);
            at += 4;
        } else {
            at += 2;
        }
    }
    let mut file = vec![0x60, 0x1A];
    file.extend((text.len() as u32).to_be_bytes());
    // No data or BSS of their own (they're in the text), no symbols, and the usual flags.
    file.extend([0u8; 4 * 5]);
    file.extend([0, 0]);
    file.extend(&text);
    match places.split_first() {
        None => file.extend([0, 0, 0, 0]),
        Some((first, rest)) => {
            file.extend((*first as u32).to_be_bytes());
            let mut previous = *first;
            for place in rest {
                let mut step = place - previous;
                while step > 254 {
                    file.push(1);
                    step -= 254;
                }
                file.push(step as u8);
                previous = *place;
            }
            file.push(0);
        }
    }
    Ok(file)
}

/// A .nex for the Spectrum Next (version 1.2 of the format): the 64K the program sees, as banks 5, 2 and 0 at
/// $4000, $8000 and $C000, started at its entry with the stack below $FFFE.
pub fn next_file(assembly: &Assembly, stack: Option<i64>) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    if low < 0x4000 {
        return Err(crate::i18n::tf!("The code starts at ${low:04X}, in the ROM. Put it at $4000 or above (ORG $8000 is usual)."));
    }
    if high > 0x10000 {
        return Err(crate::i18n::tr("The code goes past the top of the 64K the Next starts with.").into());
    }
    let start = assembly.start().unwrap_or(low);
    let mut header = vec![0u8; 512];
    header[0..4].copy_from_slice(b"Next");
    header[4..8].copy_from_slice(b"V1.2");
    // Banks in the 64K: 5 at $4000, 2 at $8000, 0 at $C000; only those with code are included.
    let slots: [(usize, i64); 3] = [(5, 0x4000), (2, 0x8000), (0, 0xC000)];
    let included: Vec<(usize, i64)> = slots.iter().copied().filter(|(_, at)| low < at + 0x4000 && high > *at).collect();
    header[9] = included.len() as u8;
    header[12..14].copy_from_slice(&(stack.unwrap_or(0xFFFE) as u16).to_le_bytes());
    header[14..16].copy_from_slice(&(start as u16).to_le_bytes());
    for (bank, _) in &included {
        header[18 + bank] = 1;
    }
    let mut file = header;
    // Banks follow in the order 5, 2, 0, 1, 3, 4, 6 and on.
    for (_, at) in &included {
        file.extend(assembly.image(*at, at + 0x4000));
    }
    Ok(file)
}
