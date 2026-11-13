//! What an assembled program is saved as, to run: for a Spectrum, a tape whose BASIC loader loads the code and
//! calls it; for a Spectrum Next, a .nex file the Next's browser runs.

use super::Assembly;
use crate::basic;

/// The lowest address a tape's loader can load code at: above the BASIC loader itself and the stack.
pub const LOWEST_FOR_TAPE: i64 = 24000;

/// A .tap: a BASIC loader (CLEAR, LOAD "" CODE, RANDOMIZE USR), then the code as one block.
pub fn spectrum_tape(assembly: &Assembly, stem: &str) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err("Nothing was assembled.".into());
    };
    if low < LOWEST_FOR_TAPE {
        return Err(format!("The code starts at {low} (${low:04X}), below {LOWEST_FOR_TAPE}, where the BASIC loader and the system need the memory. Put ORG 32768 (or anywhere from {LOWEST_FOR_TAPE}) before it."));
    }
    if high > 0x10000 {
        return Err("The code goes past the top of the Spectrum's 64K.".into());
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
        return Err("Nothing was assembled.".into());
    };
    if high > 0x10000 {
        return Err("The code goes past the top of the 64K.".into());
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
        return Err(format!("The code starts at ${low:04X}, too close after BASIC's start at $0801 for a line to start it. Start it at $0801 with a BASIC line of its own, or at $0810 or above."));
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

/// A .nex for the Spectrum Next (version 1.2 of the format): the 64K the program sees, as banks 5, 2 and 0 at
/// $4000, $8000 and $C000, started at its entry with the stack below $FFFE.
pub fn next_file(assembly: &Assembly, stack: Option<i64>) -> Result<Vec<u8>, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err("Nothing was assembled.".into());
    };
    if low < 0x4000 {
        return Err(format!("The code starts at ${low:04X}, in the ROM. Put it at $4000 or above (ORG $8000 is usual)."));
    }
    if high > 0x10000 {
        return Err("The code goes past the top of the 64K the Next starts with.".into());
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
