//! Atari 8-bit programs (the 400, 800, XL and XE): the binary file Atari DOS loads (.xex), made from assembled 6502
//! code, and a starter program.
//!
//! The file is Atari DOS 2's binary load format: $FF $FF, then segments, each its first address and its last (low
//! byte first), then the bytes between. A segment that puts an address at RUNAD ($02E0) says where the program
//! starts once it's all loaded; one at INITAD ($02E2) would be called as soon as it's loaded. Altirra's /run and
//! atari800's -run load it with no DOS at all.

use crate::asm::Assembly;

/// Where DOS looks for the program's start, once it's loaded.
const RUNAD: u16 = 0x02E0;

/// An .xex from assembled code: each run of bytes a segment, then RUNAD set to where it starts (the END line's
/// label, or the lowest address).
pub fn xex(assembly: &Assembly) -> Result<Vec<u8>, String> {
    let Some(low) = assembly.lowest() else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    let mut file = vec![0xFF, 0xFF];
    let mut chunks: Vec<(i64, &[u8])> = assembly.chunks.iter().filter(|c| !c.bytes.is_empty()).map(|c| (c.address, &c.bytes[..])).collect();
    chunks.sort_by_key(|(address, _)| *address);
    for (address, bytes) in chunks {
        if address < 0 || address + bytes.len() as i64 > 0x10000 {
            return Err(crate::i18n::tf!("The code at ${address:X} goes outside the Atari's 64K."));
        }
        let first = address as u16;
        let last = (address + bytes.len() as i64 - 1) as u16;
        file.extend(first.to_le_bytes());
        file.extend(last.to_le_bytes());
        file.extend_from_slice(bytes);
    }
    let start = assembly.start().unwrap_or(low) as u16;
    file.extend(RUNAD.to_le_bytes());
    file.extend((RUNAD + 1).to_le_bytes());
    file.extend(start.to_le_bytes());
    Ok(file)
}

/// File, New as, Atari 8-bit (6502): a line printed through the screen editor, on a coloured screen.
pub const STARTER: &str = r#"; machine: atari8
; An Atari 8-bit program, for the 400, 800, XL and XE. F9 makes the .xex file DOS loads (its segments, then where
; it starts) and runs it in an emulator: Altirra or atari800. It prints a line through the screen editor, E:,
; which the operating system has open as channel 0, with CIO, the OS's way into every device.

COLOR2  = $02C6                 ; the text's background, copied to the GTIA every frame
COLOR4  = $02C8                 ; the border
ICCOM   = $0342                 ; channel 0's command, its buffer's address and its length
ICBAL   = $0344
ICBLL   = $0348
CIOV    = $E456                 ; CIO, for every channel
PUTCHR  = $0B                   ; the command to put characters

        org $2000
start:
        lda #$34
        sta COLOR4              ; a red border
        lda #$84
        sta COLOR2              ; on blue
        ldx #0                  ; channel 0 (its number times 16)
        lda #PUTCHR
        sta ICCOM,x
        lda #<message
        sta ICBAL,x
        lda #>message
        sta ICBAL+1,x
        lda #<length
        sta ICBLL,x
        lda #>length
        sta ICBLL+1,x
        jsr CIOV
forever:
        jmp forever

; ATASCII is ASCII for letters and digits; $9B ends a line.
message:
        .byte "Hello from Limoniter", $9B
length = * - message

        end start
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The segments of an .xex: where each goes and its bytes. None if it isn't one, or ends partway through one.
    fn segments(bytes: &[u8]) -> Option<Vec<(u16, Vec<u8>)>> {
        let word = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        if word(0)? != 0xFFFF {
            return None;
        }
        let mut at = 2;
        let mut out = Vec::new();
        while at < bytes.len() {
            // A segment may have $FFFF before it again.
            if word(at)? == 0xFFFF {
                at += 2;
            }
            let (first, last) = (word(at)?, word(at + 2)?);
            if last < first {
                return None;
            }
            let length = usize::from(last - first) + 1;
            out.push((first, bytes.get(at + 4..at + 4 + length)?.to_vec()));
            at += 4 + length;
        }
        Some(out)
    }

    fn assemble(source: &str) -> Assembly {
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("atari.asm"), source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        assembly
    }

    #[test]
    fn the_starter_becomes_an_xex_that_starts_where_it_should() {
        let assembly = assemble(STARTER);
        let file = xex(&assembly).unwrap();
        // $FFFF, then the program's segment at $2000, then RUNAD's: $02E0 to $02E1, holding $2000.
        assert_eq!(&file[..4], &[0xFF, 0xFF, 0x00, 0x20]);
        let parts = segments(&file).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, 0x2000);
        assert_eq!(parts[0].1, assembly.image(0x2000, assembly.highest().unwrap()));
        assert_eq!(parts[1], (0x02E0, vec![0x00, 0x20]));
        // Its last segment's end address, as the format has it: the last byte's, not the one after.
        let end = 0x2000 + parts[0].1.len() as u16 - 1;
        assert_eq!(&file[4..6], &end.to_le_bytes());
        assert!(parts[0].1.ends_with(b"Hello from Limoniter\x9B"));
        assert_eq!(crate::running::Runnable::from_file("hello.xex", file).map(|r| (r.machine, r.format)), Some((crate::running::Machine::Atari8, crate::running::Format::Xex)));
    }

    #[test]
    fn each_run_of_bytes_is_a_segment() {
        let assembly = assemble(" org $3000\nmain: rts\n org $0600\n .byte 1,2,3\n end main\n");
        let parts = segments(&xex(&assembly).unwrap()).unwrap();
        assert_eq!(parts, vec![(0x0600, vec![1, 2, 3]), (0x3000, vec![0x60]), (0x02E0, vec![0x00, 0x30])]);
        assert!(segments(&[0xFF, 0xFF, 0x00, 0x20, 0x10, 0x20]).is_none(), "cut short");
        assert!(segments(b"not one").is_none());
    }
}
