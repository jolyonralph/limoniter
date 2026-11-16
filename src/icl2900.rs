//! ICL 2900 machine code, disassembled into SFL, the 2900's assembly language, as a listing: each instruction's
//! byte address, its halfwords in hex, and the instruction.
//!
//! From ICL's *2900 Order Code* (Technical Literature 6594899, Issue 1, 1978): the function codes are its
//! Appendix 1, and the operand decoding its section 1 (figures 5 to 13). Bits are numbered from 0, the most
//! significant, as ICL numbers them. An instruction is 16 or 32 bits: bits 0-6 the function (written as the even
//! hex number it makes with an eighth bit of zero), and the rest the operand, read one of three ways:
//!
//! - **Primary** (most functions): k in bits 7-8. k = 0 is a 7-bit signed literal, 1 is LNB+n, 2 is the descriptor
//!   at LNB+n; k = 3 is decoded further by k' (bits 9-10: direct, or through a descriptor in DR, in store, or in
//!   store modified by B) and k'' (bits 11-13: the register). k'' = 6 or 7 is a 16-bit instruction, anything else
//!   (and k' = 1, k'' = 7) has an 18-bit N in bits 14-31; k'' = 1 is a bit string (section 1.12).
//! - **Secondary** (the 16 store-to-store functions, #80-#86, #90-#96, #A0-#A6, #B0-#B6): h in bit 7 (the length is
//!   n+1, or the DR string's), q in bit 8 (32 bits, with a mask byte and a literal or filler byte), n in 9-15.
//! - **Tertiary** (the conditional jumps JCC, JAT and JAF): a 4-bit mask in bits 7-10, and k''' in bits 11-13;
//!   6 and 7 are 16-bit, the rest have N.
//!
//! Operands are written as SFL writes them, the variant after the mnemonic (`LB.L 8`, `ST.T`, `LB -19`), as in
//! the manual's Table 1 and its examples. A literal operand of a jump (J, JLK, CALL, DEBJ, and the tertiary
//! jumps' N) counts halfwords from the instruction, and is written as the address it reaches.

use std::collections::BTreeSet;

/// Every function, by its code: ICL's Appendix 1, checked against the page images.
const FUNCTIONS: [(u8, &str); 123] = [
    (0x02, "JCC"), (0x04, "JAT"), (0x06, "JAF"), (0x08, "TEST"), (0x0C, "CLR"), (0x0E, "SET"),
    (0x10, "VAL"), (0x12, "CYD"), (0x14, "INCA"), (0x16, "MODD"), (0x18, "PRCL"), (0x1A, "J"), (0x1C, "JLK"), (0x1E, "CALL"),
    (0x20, "ADB"), (0x22, "SBB"), (0x24, "DEBJ"), (0x26, "CPB"), (0x28, "SIG"), (0x2A, "MYB"), (0x2C, "VMY"), (0x2E, "CPIB"),
    (0x30, "LCT"), (0x32, "MPSR"), (0x34, "CPSR"), (0x36, "STCT"), (0x38, "EXIT"), (0x3A, "ESEX"), (0x3C, "OUT"), (0x3E, "ACT"),
    (0x40, "SL"), (0x42, "SLSS"), (0x44, "SLSD"), (0x46, "SLSQ"), (0x48, "ST"), (0x4A, "STUH"), (0x4C, "STXN"), (0x4E, "IDLE"),
    (0x50, "SLD"), (0x52, "SLB"), (0x54, "TDEC"), (0x56, "INCT"), (0x58, "STD"), (0x5A, "STB"), (0x5C, "STLN"), (0x5E, "STSF"),
    (0x60, "L"), (0x62, "LSS"), (0x64, "LSD"), (0x66, "LSQ"), (0x68, "RRTC"), (0x6A, "LUH"), (0x6C, "RALN"), (0x6E, "ASF"),
    (0x70, "LDRL"), (0x72, "LDA"), (0x74, "LDTB"), (0x76, "LDB"), (0x78, "LD"), (0x7A, "LB"), (0x7C, "LLN"), (0x7E, "LXN"),
    (0x80, "TCH"), (0x82, "ANDS"), (0x84, "ORS"), (0x86, "NEQS"), (0x88, "EXPA"), (0x8A, "AND"), (0x8C, "OR"), (0x8E, "NEQ"),
    (0x90, "PK"), (0x92, "INS"), (0x94, "SUPK"), (0x98, "COMA"), (0x9A, "DDV"), (0x9C, "DRDV"), (0x9E, "DMDV"),
    (0xA0, "SWEQ"), (0xA2, "SWNE"), (0xA4, "CPS"), (0xA6, "TTR"), (0xA8, "FLT"), (0xAA, "IDV"), (0xAC, "IRDV"), (0xAE, "IMDV"),
    (0xB0, "MVL"), (0xB2, "MV"), (0xB4, "CHOV"), (0xB8, "FIX"), (0xBA, "RDV"), (0xBC, "RRDV"), (0xBE, "RDVD"),
    (0xC0, "UAD"), (0xC2, "USB"), (0xC4, "URSB"), (0xC6, "UCP"), (0xC8, "USH"), (0xCA, "ROT"), (0xCC, "SHS"), (0xCE, "SHZ"),
    (0xD0, "DAD"), (0xD2, "DSB"), (0xD4, "DRSB"), (0xD6, "DCP"), (0xD8, "DSH"), (0xDA, "DMY"), (0xDC, "DMYD"), (0xDE, "CBIN"),
    (0xE0, "IAD"), (0xE2, "ISB"), (0xE4, "IRSB"), (0xE6, "ICP"), (0xE8, "ISH"), (0xEA, "IMY"), (0xEC, "IMYD"), (0xEE, "CDEC"),
    (0xF0, "RAD"), (0xF2, "RSB"), (0xF4, "RRSB"), (0xF6, "RCP"), (0xF8, "RSC"), (0xFA, "RMY"), (0xFC, "RMYD"),
];

/// The function a code is, if it's one.
pub fn function(code: u8) -> Option<&'static str> {
    FUNCTIONS.iter().find(|(c, _)| *c == code).map(|(_, name)| *name)
}

/// The primary functions that jump: a literal operand is added to PC, in halfwords.
fn jumps(code: u8) -> bool {
    matches!(code, 0x1A | 0x1C | 0x1E | 0x24)
}

fn secondary(code: u8) -> bool {
    matches!(code, 0x80..=0x86 | 0x90..=0x96 | 0xA0..=0xA6 | 0xB0..=0xB6)
}

fn tertiary(code: u8) -> bool {
    matches!(code, 0x02 | 0x04 | 0x06)
}

/// One instruction, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    /// 2 or 4 bytes.
    pub length: u32,
    /// The mnemonic and operand, with {target} where the address jumped to goes.
    pub text: String,
    /// Where a jump with a literal operand goes.
    pub target: Option<u32>,
    /// An address worked out from PC (a PC+N operand), to say beside it.
    pub refers: Option<u32>,
    /// Whether it never carries on to the next instruction: J, EXIT and ESEX.
    pub ends: bool,
}

fn sign_extend(value: u32, bits: u32) -> i64 {
    let shift = 32 - bits;
    i64::from(((value << shift) as i32) >> shift)
}

fn relative(at: u32, halfwords: i64) -> u32 {
    (i64::from(at) + 2 * halfwords) as u32
}

/// Decodes the instruction at the start of `bytes`, which is at byte address `at`. None if it isn't one: a
/// function code not assigned, or a 32-bit instruction cut short.
pub fn decode(bytes: &[u8], at: u32) -> Option<Instruction> {
    let first = u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]);
    let code = (first >> 8) as u8 & 0xFE;
    let name = function(code)?;
    let second = bytes.get(2..4).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let word = second.map(|s| (u32::from(first) << 16) | u32::from(s));
    // The 18-bit N of a 32-bit instruction: bits 14-31.
    let big_n = word.map(|w| w & 0x3FFFF);
    let ends = matches!(code, 0x1A | 0x38 | 0x3A);
    let short = |text: String| Some(Instruction { length: 2, text, target: None, refers: None, ends });
    let long = |text: String, target: Option<u32>, refers: Option<u32>| Some(Instruction { length: 4, text, target, refers, ends });
    if secondary(code) {
        let (h, q, n) = ((first >> 8) & 1, (first >> 7) & 1, first & 0x7F);
        let length = if h == 0 { format!("L={}", n + 1) } else { "L=DR".to_string() };
        if q == 0 {
            return short(format!("{name} {length}"));
        }
        let s = second?;
        return long(format!("{name} {length}, MASK=X'{:02X}', LIT=X'{:02X}'", s >> 8, s & 0xFF), None, None);
    }
    if tertiary(code) {
        let (mask, k3) = ((first >> 5) & 0xF, (first >> 2) & 7);
        return match k3 {
            6 => short(format!("{name}.D {mask}")),
            7 => short(format!("{name}.MD {mask}")),
            _ => {
                let n = big_n?;
                match k3 {
                    0 => {
                        let target = relative(at, sign_extend(n, 18));
                        long(format!("{name} {{target}}, {mask}"), Some(target), None)
                    }
                    4 => {
                        let n = sign_extend(n, 18);
                        long(format!("{name}.P {n}, {mask}"), None, Some(relative(at, n)))
                    }
                    _ => long(format!("{name}.{} {n}, {mask}", ["", "D", "L", "X", "", "C"][usize::from(k3)]), None, None),
                }
            }
        };
    }
    // Primary.
    let k = (first >> 7) & 3;
    let n = u32::from(first & 0x7F);
    match k {
        0 => {
            let literal = sign_extend(n, 7);
            if jumps(code) {
                let target = relative(at, literal);
                return Some(Instruction { length: 2, text: format!("{name} {{target}}"), target: Some(target), refers: None, ends });
            }
            return short(format!("{name} {literal}"));
        }
        1 => return short(format!("{name}.L {n}")),
        2 => return short(format!("{name}.IL {n}")),
        _ => {}
    }
    let (k1, k2) = ((first >> 5) & 3, (first >> 2) & 7);
    if k2 >= 6 && !(k1 == 1 && k2 == 7) {
        let variant = match (k1, k2) {
            (0, 6) => "T",
            (0, _) => "B",
            (1, _) => "DT",
            (2, 6) => "IT",
            (2, _) => "D",
            (_, 6) => "MIT",
            _ => "MD",
        };
        return short(format!("{name}.{variant}"));
    }
    let w = word?;
    let n = big_n?;
    if k2 == 1 {
        // A bit string within a word: b its first bit, l one less than its length, s whether it's signed.
        let (b, l, s, n) = ((w >> 13) & 0x1F, (w >> 8) & 0x1F, (w >> 7) & 1, w & 0x7F);
        let variant = ["L", "B", "X", "D"][k1 as usize];
        return long(format!("{name}.{variant} {n}, Z'{b}, {}, {s}'", l + 1), None, None);
    }
    match (k1, k2) {
        (0, 0) => {
            let literal = sign_extend(n, 18);
            if jumps(code) {
                return long(format!("{name} {{target}}"), Some(relative(at, literal)), None);
            }
            long(format!("{name} {literal}"), None, None)
        }
        (3, 0) => long(format!("{name}.GB"), None, None),
        (_, 4) => {
            let n = sign_extend(n, 18);
            long(format!("{name}.{}P {n}", ["", "D", "I", "MI"][k1 as usize]), None, Some(relative(at, n)))
        }
        _ => {
            let variant = match (k1, k2) {
                (0, 2) => "L",
                (0, 3) => "X",
                (0, _) => "C",
                (1, 0) => "D",
                (1, 2) => "DL",
                (1, 3) => "DX",
                (1, 5) => "DC",
                (1, _) => "B",
                (2, 0) => "G",
                (2, 2) => "IL",
                (2, 3) => "IX",
                (2, _) => "IC",
                (_, 2) => "MIL",
                (_, 3) => "MIX",
                _ => "MIC",
            };
            long(format!("{name}.{variant} {n}"), None, None)
        }
    }
}

/// A listing of `bytes`, which are at byte address `origin`, decoded one instruction after another: what isn't an
/// instruction is shown as its halfword. Addresses jumped to get labels.
pub fn disassemble(bytes: &[u8], origin: u32) -> String {
    let end = u64::from(origin) + bytes.len() as u64;
    let inside = |a: u32| u64::from(a) >= u64::from(origin) && u64::from(a) < end;
    let mut decoded = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let at = origin.wrapping_add(offset as u32);
        let found = decode(&bytes[offset..], at);
        let length = found.as_ref().map_or(2, |i| i.length as usize).min(bytes.len() - offset);
        decoded.push((at, offset, length, found));
        offset += length;
    }
    let labels: BTreeSet<u32> = decoded.iter().filter_map(|(.., i)| i.as_ref()?.target).filter(|t| inside(*t)).collect();
    let name = |a: u32| if inside(a) && labels.contains(&a) { format!("L{a:08X}") } else { format!("X'{a:08X}'") };
    let mut lines = vec![
        "; ICL 2900 code, disassembled by Limoniter into SFL: byte address, halfwords, instruction.".to_string(),
        "; Jumps' literal operands are shown as the address they reach.".to_string(),
        String::new(),
    ];
    for (at, offset, length, found) in decoded {
        let hex: String = bytes[offset..offset + length].chunks(2).map(|c| c.iter().map(|b| format!("{b:02X}")).collect::<String>()).collect::<Vec<_>>().join(" ");
        if labels.contains(&at) {
            lines.push(format!("{:<20}L{at:08X}:", ""));
        }
        let (text, ends) = match &found {
            Some(i) => {
                let mut text = i.text.replace("{target}", &i.target.map(name).unwrap_or_default());
                if let Some(refers) = i.refers {
                    text = format!("{text:<32}; X'{refers:08X}'");
                }
                (text, i.ends)
            }
            None => (format!("{:<32}; not an instruction", format!("X'{hex}'").replace(' ', "")), false),
        };
        lines.push(format!("{at:08X}  {hex:<9}   {text}"));
        if ends {
            lines.push(String::new());
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        decode(bytes, 0x1000).unwrap().text
    }

    #[test]
    fn the_manuals_examples_decode_as_it_writes_them() {
        // Figure 14: LB -19 is #7A6D, and LB.L 8 is #7A88.
        assert_eq!(text(&[0x7A, 0x6D]), "LB -19");
        assert_eq!(text(&[0x7A, 0x88]), "LB.L 8");
        // LB.IL 5: k = 2, so bit 7 is set.
        assert_eq!(text(&[0x7B, 0x05]), "LB.IL 5");
    }

    #[test]
    fn every_function_code_is_even_and_known_once() {
        let codes: BTreeSet<u8> = FUNCTIONS.iter().map(|(c, _)| *c).collect();
        assert_eq!(codes.len(), FUNCTIONS.len());
        assert!(codes.iter().all(|c| c % 2 == 0 && *c != 0 && *c != 0xFE));
        assert_eq!(FUNCTIONS.iter().filter(|(c, _)| secondary(*c)).count(), 14, "16 store-to-store codes, #96 and #B6 unassigned");
        assert_eq!(function(0x1A), Some("J"));
        assert_eq!(function(0x96), None);
    }

    /// A 32-bit primary instruction from its fields.
    fn primary32(code: u8, k1: u32, k2: u32, n: u32) -> [u8; 4] {
        ((u32::from(code) << 24) | (3 << 23) | (k1 << 21) | (k2 << 18) | (n & 0x3FFFF)).to_be_bytes()
    }

    fn primary16(code: u8, k1: u16, k2: u16) -> [u8; 2] {
        ((u16::from(code) << 8) | (3 << 7) | (k1 << 5) | (k2 << 2)).to_be_bytes()
    }

    #[test]
    fn each_operand_form_of_figure_7_is_written_as_sfl_writes_it() {
        assert_eq!(text(&primary32(0x60, 0, 0, 0x3FFFF)), "L -1");
        assert_eq!(text(&primary32(0x60, 0, 2, 5)), "L.L 5");
        assert_eq!(text(&primary32(0x78, 0, 3, 4)), "LD.X 4");
        assert_eq!(text(&primary32(0x60, 0, 5, 4)), "L.C 4");
        assert_eq!(text(&primary32(0x60, 1, 0, 3)), "L.D 3");
        assert_eq!(text(&primary32(0x60, 1, 3, 3)), "L.DX 3");
        assert_eq!(text(&primary32(0x60, 1, 7, 3)), "L.B 3", "B+N, the one 32-bit form with k'' = 7");
        assert_eq!(text(&primary32(0x60, 2, 0, 9)), "L.G 9", "image store location N");
        assert_eq!(text(&primary32(0x60, 2, 3, 6)), "L.IX 6");
        assert_eq!(text(&primary32(0x60, 3, 0, 0)), "L.GB");
        assert_eq!(text(&primary32(0x60, 3, 2, 4)), "L.MIL 4");
        assert_eq!(text(&primary32(0x60, 3, 5, 4)), "L.MIC 4");
        assert_eq!(text(&primary16(0x48, 0, 6)), "ST.T");
        assert_eq!(text(&primary16(0x7A, 0, 7)), "LB.B");
        assert_eq!(text(&primary16(0x60, 1, 6)), "L.DT");
        assert_eq!(text(&primary16(0x60, 2, 6)), "L.IT");
        assert_eq!(text(&primary16(0x60, 2, 7)), "L.D");
        assert_eq!(text(&primary16(0x60, 3, 6)), "L.MIT");
        assert_eq!(text(&primary16(0x60, 3, 7)), "L.MD");
        assert_eq!(decode(&primary16(0x60, 0, 6), 0).unwrap().length, 2);
        // PC+N is in halfwords, signed, and the address it names is said.
        let pc = decode(&primary32(0x60, 0, 4, 0x3FFFE), 0x1000).unwrap();
        assert_eq!((pc.text.as_str(), pc.refers), ("L.P -2", Some(0x0FFC)));
    }

    #[test]
    fn a_bit_string_is_written_with_its_first_bit_length_and_sign() {
        // The manual's example 3: CLR.L 4, Z'7, 12, 0' clears bits 7 to 18 of the word at LNB+4.
        let word = (0x0Cu32 << 24) | (3 << 23) | (1 << 18) | (7 << 13) | (11 << 8) | 4;
        assert_eq!(text(&word.to_be_bytes()), "CLR.L 4, Z'7, 12, 0'");
    }

    #[test]
    fn jumps_reach_their_targets_in_halfwords() {
        // J -2 from #100 goes back 4 bytes.
        let j = decode(&[0x1A, 0x7E], 0x100).unwrap();
        assert_eq!((j.target, j.ends, j.length), (Some(0xFC), true, 2));
        let jcc = decode(&((0x02u32 << 24) | (8 << 21) | 4).to_be_bytes(), 0x100).unwrap();
        assert_eq!((jcc.text.as_str(), jcc.target, jcc.ends), ("JCC {target}, 8", Some(0x108), false));
        assert_eq!(text(&((0x04u16 << 8) | (2 << 5) | (6 << 2)).to_be_bytes()), "JAT.D 2");
        // A literal on anything else is just a number.
        assert_eq!(decode(&[0x60, 0x7E], 0).unwrap().target, None);
    }

    #[test]
    fn store_to_store_functions_give_their_length_mask_and_literal() {
        assert_eq!(text(&[0xB2, 0x0B]), "MV L=12");
        assert_eq!(text(&[0xB3, 0x00]), "MV L=DR");
        assert_eq!(text(&[0xB0, 0x8B, 0x00, 0x40]), "MVL L=12, MASK=X'00', LIT=X'40'");
    }

    #[test]
    fn a_listing_labels_what_is_jumped_to_and_shows_what_isnt_code() {
        // LB -19; J back to it; then a halfword that isn't an instruction.
        let listing = disassemble(&[0x7A, 0x6D, 0x1A, 0x7F, 0x00, 0x00], 0x2000);
        assert!(listing.contains("L00002000:"), "{listing}");
        assert!(listing.contains("00002000  7A6D        LB -19"), "{listing}");
        assert!(listing.contains("00002002  1A7F        J L00002000"), "{listing}");
        assert!(listing.contains("X'0000'") && listing.contains("not an instruction"), "{listing}");
        // A 32-bit instruction cut short at the end isn't one.
        assert!(disassemble(&primary32(0x60, 0, 2, 5)[..2], 0).contains("not an instruction"));
    }
}
