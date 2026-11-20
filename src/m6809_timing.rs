//! How long 6809 and 6309 instructions take, for timing code on the Dragon, the Color Computer (the CoCo 3 above
//! all, where a 6309 is a common upgrade) and the Vectrex. The 6309 runs in one of two modes: emulation mode, where
//! every instruction takes as long as it does on a 6809, and native mode (LDMD #1), where most take a cycle or two
//! fewer. Which applies is told from the source: PRAGMA 6309 counts native mode, as lwasm's listings do, PRAGMA
//! 6809 a 6809's, and LDMD switches from the next line on. With neither, a 6809's counts are shown.
//!
//! The figures are Motorola's for the 6809 (the MC6809-MC6809E Programming Manual's opcode map and table of indexed
//! modes) and Darren Atkinson's 6809 and 6309 Programmer's Reference for the 6309's own instructions and its native
//! mode, checked against lwasm's table (see the tests).

use crate::asm::{Assembly, Placed};

/// Which timings apply: a 6809's, which a 6309 keeps in its emulation mode, or the 6309's native mode's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Emulation,
    Native,
}

/// What an instruction takes: the least and most cycles (a conditional long branch takes one more when taken on a
/// 6809), and for TFM how many more for each byte it moves; `waits` for SYNC and CWAI, which wait for an interrupt
/// and so take at least this long.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Timing {
    pub least: u32,
    pub most: u32,
    pub per_byte: u32,
    pub waits: bool,
}

impl Timing {
    fn fixed(cycles: u32) -> Timing {
        Timing { least: cycles, most: cycles, ..Timing::default() }
    }

    pub fn add(self, other: Timing) -> Timing {
        Timing { least: self.least + other.least, most: self.most + other.most, per_byte: self.per_byte + other.per_byte, waits: self.waits || other.waits }
    }

    /// "5", "5/6" where it may take more, "6+3n" for TFM, or "≥4" for one that waits.
    pub fn text(self) -> String {
        self.shown('/')
    }

    /// The same for a total: "12" or "12–15".
    pub fn range(self) -> String {
        self.shown('\u{2013}')
    }

    fn shown(self, between: char) -> String {
        let mut text = if self.least == self.most { self.least.to_string() } else { format!("{}{between}{}", self.least, self.most) };
        if self.per_byte > 0 {
            text = format!("{text}+{}n", self.per_byte);
        }
        if self.waits {
            text = format!("\u{2265}{text}");
        }
        text
    }
}

/// Each instruction's cycles before any indexed mode's extra, as (prefix and opcode, a 6809's or emulation mode's,
/// native mode's). Indexed forms are their base; PSHS and the rest are before the registers they move.
const BASE: &[(u16, u8, u8)] = &[
    // page 00
    (0x0000, 6, 5), (0x0001, 6, 6), (0x0002, 6, 6), (0x0003, 6, 5), (0x0004, 6, 5), (0x0005, 6, 6), (0x0006, 6, 5), (0x0007, 6, 5),
    (0x0008, 6, 5), (0x0009, 6, 5), (0x000A, 6, 5), (0x000B, 6, 6), (0x000C, 6, 5), (0x000D, 6, 4), (0x000E, 3, 2), (0x000F, 6, 5),
    (0x0012, 2, 1), (0x0013, 4, 3), (0x0014, 4, 4), (0x0016, 5, 4), (0x0017, 9, 7), (0x0019, 2, 1), (0x001A, 3, 2), (0x001C, 3, 2),
    (0x001D, 2, 1), (0x001E, 8, 5), (0x001F, 6, 4), (0x0020, 3, 3), (0x0021, 3, 3), (0x0022, 3, 3), (0x0023, 3, 3), (0x0024, 3, 3),
    (0x0025, 3, 3), (0x0026, 3, 3), (0x0027, 3, 3), (0x0028, 3, 3), (0x0029, 3, 3), (0x002A, 3, 3), (0x002B, 3, 3), (0x002C, 3, 3),
    (0x002D, 3, 3), (0x002E, 3, 3), (0x002F, 3, 3), (0x0030, 4, 4), (0x0031, 4, 4), (0x0032, 4, 4), (0x0033, 4, 4), (0x0034, 5, 4),
    (0x0035, 5, 4), (0x0036, 5, 4), (0x0037, 5, 4), (0x0039, 5, 4), (0x003A, 3, 1), (0x003B, 6, 6), (0x003C, 22, 20), (0x003D, 11, 10),
    (0x003F, 19, 21), (0x0040, 2, 1), (0x0043, 2, 1), (0x0044, 2, 1), (0x0046, 2, 1), (0x0047, 2, 1), (0x0048, 2, 1), (0x0049, 2, 1),
    (0x004A, 2, 1), (0x004C, 2, 1), (0x004D, 2, 1), (0x004F, 2, 1), (0x0050, 2, 1), (0x0053, 2, 1), (0x0054, 2, 1), (0x0056, 2, 1),
    (0x0057, 2, 1), (0x0058, 2, 1), (0x0059, 2, 1), (0x005A, 2, 1), (0x005C, 2, 1), (0x005D, 2, 1), (0x005F, 2, 1), (0x0060, 6, 6),
    (0x0061, 7, 7), (0x0062, 7, 7), (0x0063, 6, 6), (0x0064, 6, 6), (0x0065, 7, 7), (0x0066, 6, 6), (0x0067, 6, 6), (0x0068, 6, 6),
    (0x0069, 6, 6), (0x006A, 6, 6), (0x006B, 7, 7), (0x006C, 6, 6), (0x006D, 6, 5), (0x006E, 3, 3), (0x006F, 6, 6), (0x0070, 7, 6),
    (0x0071, 7, 7), (0x0072, 7, 7), (0x0073, 7, 6), (0x0074, 7, 6), (0x0075, 7, 7), (0x0076, 7, 6), (0x0077, 7, 6), (0x0078, 7, 6),
    (0x0079, 7, 6), (0x007A, 7, 6), (0x007B, 7, 7), (0x007C, 7, 6), (0x007D, 7, 5), (0x007E, 4, 3), (0x007F, 7, 6), (0x0080, 2, 2),
    (0x0081, 2, 2), (0x0082, 2, 2), (0x0083, 4, 3), (0x0084, 2, 2), (0x0085, 2, 2), (0x0086, 2, 2), (0x0088, 2, 2), (0x0089, 2, 2),
    (0x008A, 2, 2), (0x008B, 2, 2), (0x008C, 4, 3), (0x008D, 7, 6), (0x008E, 3, 3), (0x0090, 4, 3), (0x0091, 4, 3), (0x0092, 4, 3),
    (0x0093, 6, 4), (0x0094, 4, 3), (0x0095, 4, 3), (0x0096, 4, 3), (0x0097, 4, 3), (0x0098, 4, 3), (0x0099, 4, 3), (0x009A, 4, 3),
    (0x009B, 4, 3), (0x009C, 6, 4), (0x009D, 7, 6), (0x009E, 5, 4), (0x009F, 5, 4), (0x00A0, 4, 4), (0x00A1, 4, 4), (0x00A2, 4, 4),
    (0x00A3, 6, 5), (0x00A4, 4, 4), (0x00A5, 4, 4), (0x00A6, 4, 4), (0x00A7, 4, 4), (0x00A8, 4, 4), (0x00A9, 4, 4), (0x00AA, 4, 4),
    (0x00AB, 4, 4), (0x00AC, 6, 5), (0x00AD, 7, 6), (0x00AE, 5, 5), (0x00AF, 5, 5), (0x00B0, 5, 4), (0x00B1, 5, 4), (0x00B2, 5, 4),
    (0x00B3, 7, 5), (0x00B4, 5, 4), (0x00B5, 5, 4), (0x00B6, 5, 4), (0x00B7, 5, 4), (0x00B8, 5, 4), (0x00B9, 5, 4), (0x00BA, 5, 4),
    (0x00BB, 5, 4), (0x00BC, 7, 5), (0x00BD, 8, 7), (0x00BE, 6, 5), (0x00BF, 6, 5), (0x00C0, 2, 2), (0x00C1, 2, 2), (0x00C2, 2, 2),
    (0x00C3, 4, 3), (0x00C4, 2, 2), (0x00C5, 2, 2), (0x00C6, 2, 2), (0x00C8, 2, 2), (0x00C9, 2, 2), (0x00CA, 2, 2), (0x00CB, 2, 2),
    (0x00CC, 3, 3), (0x00CD, 5, 5), (0x00CE, 3, 3), (0x00D0, 4, 3), (0x00D1, 4, 3), (0x00D2, 4, 3), (0x00D3, 6, 4), (0x00D4, 4, 3),
    (0x00D5, 4, 3), (0x00D6, 4, 3), (0x00D7, 4, 3), (0x00D8, 4, 3), (0x00D9, 4, 3), (0x00DA, 4, 3), (0x00DB, 4, 3), (0x00DC, 5, 4),
    (0x00DD, 5, 4), (0x00DE, 5, 4), (0x00DF, 5, 4), (0x00E0, 4, 4), (0x00E1, 4, 4), (0x00E2, 4, 4), (0x00E3, 6, 5), (0x00E4, 4, 4),
    (0x00E5, 4, 4), (0x00E6, 4, 4), (0x00E7, 4, 4), (0x00E8, 4, 4), (0x00E9, 4, 4), (0x00EA, 4, 4), (0x00EB, 4, 4), (0x00EC, 5, 5),
    (0x00ED, 5, 5), (0x00EE, 5, 5), (0x00EF, 5, 5), (0x00F0, 5, 4), (0x00F1, 5, 4), (0x00F2, 5, 4), (0x00F3, 7, 5), (0x00F4, 5, 4),
    (0x00F5, 5, 4), (0x00F6, 5, 4), (0x00F7, 5, 4), (0x00F8, 5, 4), (0x00F9, 5, 4), (0x00FA, 5, 4), (0x00FB, 5, 4), (0x00FC, 6, 5),
    (0x00FD, 6, 5), (0x00FE, 6, 5), (0x00FF, 6, 5),
    // page 10
    (0x1021, 5, 5), (0x1022, 5, 5), (0x1023, 5, 5), (0x1024, 5, 5), (0x1025, 5, 5), (0x1026, 5, 5), (0x1027, 5, 5), (0x1028, 5, 5),
    (0x1029, 5, 5), (0x102A, 5, 5), (0x102B, 5, 5), (0x102C, 5, 5), (0x102D, 5, 5), (0x102E, 5, 5), (0x102F, 5, 5), (0x1030, 4, 4),
    (0x1031, 4, 4), (0x1032, 4, 4), (0x1033, 4, 4), (0x1034, 4, 4), (0x1035, 4, 4), (0x1036, 4, 4), (0x1037, 4, 4), (0x1038, 6, 6),
    (0x1039, 6, 6), (0x103A, 6, 6), (0x103B, 6, 6), (0x103F, 20, 22), (0x1040, 3, 2), (0x1043, 3, 2), (0x1044, 3, 2), (0x1046, 3, 2),
    (0x1047, 3, 2), (0x1048, 3, 2), (0x1049, 3, 2), (0x104A, 3, 2), (0x104C, 3, 2), (0x104D, 3, 2), (0x104F, 3, 2), (0x1053, 3, 2),
    (0x1054, 3, 2), (0x1056, 3, 2), (0x1059, 3, 2), (0x105A, 3, 2), (0x105C, 3, 2), (0x105D, 3, 2), (0x105F, 3, 2), (0x1080, 5, 4),
    (0x1081, 5, 4), (0x1082, 5, 4), (0x1083, 5, 4), (0x1084, 5, 4), (0x1085, 5, 4), (0x1086, 4, 4), (0x1088, 5, 4), (0x1089, 5, 4),
    (0x108A, 5, 4), (0x108B, 5, 4), (0x108C, 5, 4), (0x108E, 4, 4), (0x1090, 7, 5), (0x1091, 7, 5), (0x1092, 7, 5), (0x1093, 7, 5),
    (0x1094, 7, 5), (0x1095, 7, 5), (0x1096, 6, 5), (0x1097, 6, 5), (0x1098, 7, 5), (0x1099, 7, 5), (0x109A, 7, 5), (0x109B, 7, 5),
    (0x109C, 7, 5), (0x109E, 6, 5), (0x109F, 6, 5), (0x10A0, 7, 6), (0x10A1, 7, 6), (0x10A2, 7, 6), (0x10A3, 7, 6), (0x10A4, 7, 6),
    (0x10A5, 7, 6), (0x10A6, 6, 6), (0x10A7, 6, 6), (0x10A8, 7, 6), (0x10A9, 7, 6), (0x10AA, 7, 6), (0x10AB, 7, 6), (0x10AC, 7, 6),
    (0x10AE, 6, 6), (0x10AF, 6, 6), (0x10B0, 8, 6), (0x10B1, 8, 6), (0x10B2, 8, 6), (0x10B3, 8, 6), (0x10B4, 8, 6), (0x10B5, 8, 6),
    (0x10B6, 7, 6), (0x10B7, 7, 6), (0x10B8, 8, 6), (0x10B9, 8, 6), (0x10BA, 8, 6), (0x10BB, 8, 6), (0x10BC, 8, 6), (0x10BE, 7, 6),
    (0x10BF, 7, 6), (0x10CE, 4, 4), (0x10DC, 8, 7), (0x10DD, 8, 7), (0x10DE, 6, 5), (0x10DF, 6, 5), (0x10EC, 8, 8), (0x10ED, 8, 8),
    (0x10EE, 6, 6), (0x10EF, 6, 6), (0x10FC, 9, 8), (0x10FD, 9, 8), (0x10FE, 7, 6), (0x10FF, 7, 6),
    // page 11
    (0x1130, 7, 6), (0x1131, 7, 6), (0x1132, 7, 6), (0x1133, 7, 6), (0x1134, 7, 6), (0x1135, 7, 6), (0x1136, 7, 6), (0x1137, 8, 7),
    (0x1138, 6, 6), (0x1139, 6, 6), (0x113A, 6, 6), (0x113B, 6, 6), (0x113C, 4, 4), (0x113D, 5, 5), (0x113F, 20, 22), (0x1143, 3, 2),
    (0x114A, 3, 2), (0x114C, 3, 2), (0x114D, 3, 2), (0x114F, 3, 2), (0x1153, 3, 2), (0x115A, 3, 2), (0x115C, 3, 2), (0x115D, 3, 2),
    (0x115F, 3, 2), (0x1180, 3, 3), (0x1181, 3, 3), (0x1183, 5, 4), (0x1186, 3, 3), (0x118B, 3, 3), (0x118C, 5, 4), (0x118D, 25, 25),
    (0x118E, 34, 34), (0x118F, 28, 28), (0x1190, 5, 4), (0x1191, 5, 4), (0x1193, 7, 5), (0x1196, 5, 4), (0x1197, 5, 4), (0x119B, 5, 4),
    (0x119C, 7, 5), (0x119D, 27, 26), (0x119E, 36, 35), (0x119F, 30, 29), (0x11A0, 5, 5), (0x11A1, 5, 5), (0x11A3, 7, 6), (0x11A6, 5, 5),
    (0x11A7, 5, 5), (0x11AB, 5, 5), (0x11AC, 7, 6), (0x11AD, 27, 27), (0x11AE, 36, 36), (0x11AF, 30, 30), (0x11B0, 6, 5), (0x11B1, 6, 5),
    (0x11B3, 8, 6), (0x11B6, 6, 5), (0x11B7, 6, 5), (0x11BB, 6, 5), (0x11BC, 8, 6), (0x11BD, 28, 27), (0x11BE, 37, 36), (0x11BF, 31, 30),
    (0x11C0, 3, 3), (0x11C1, 3, 3), (0x11C6, 3, 3), (0x11CB, 3, 3), (0x11D0, 5, 4), (0x11D1, 5, 4), (0x11D6, 5, 4), (0x11D7, 5, 4),
    (0x11DB, 5, 4), (0x11E0, 5, 5), (0x11E1, 5, 5), (0x11E6, 5, 5), (0x11E7, 5, 5), (0x11EB, 5, 5), (0x11F0, 6, 5), (0x11F1, 6, 5),
    (0x11F6, 6, 5), (0x11F7, 6, 5), (0x11FB, 6, 5),
];

/// An instruction's opcode with its page prefix (0x10 or 0x11) before it.
fn opcode(bytes: &[u8]) -> Option<u16> {
    match *bytes.first()? {
        page @ (0x10 | 0x11) => Some(u16::from(page) << 8 | u16::from(*bytes.get(1)?)),
        op => Some(u16::from(op)),
    }
}

/// Where an indexed instruction's postbyte is, or None if the instruction isn't indexed.
fn postbyte_at(code: u16) -> Option<usize> {
    match code {
        // OIM, AIM, EIM and TIM: the immediate value comes before the postbyte.
        0x61 | 0x62 | 0x65 | 0x6B => Some(2),
        0x30..=0x33 | 0x60..=0x6F | 0xA0..=0xAF | 0xE0..=0xEF => Some(1),
        0x10A0..=0x10AF | 0x10E0..=0x10EF | 0x11A0..=0x11AF | 0x11E0..=0x11EF => Some(2),
        _ => None,
    }
}

/// The cycles an indexed postbyte adds to its instruction's base: Motorola's table F-2 for the 6809's modes, and
/// Atkinson's indexed addressing mode table for the 6309's (from W, and with E, F or W as the offset) and for
/// native mode. None for a postbyte that means nothing.
pub fn indexed_extra(post: u8, mode: Mode) -> Option<u32> {
    let pick = |emulation: u32, native: u32| if mode == Mode::Native { native } else { emulation };
    // A 5-bit offset.
    if post & 0x80 == 0 {
        return Some(1);
    }
    // The 6309's modes from W, in codes the 6809 leaves unused, take the same in both modes; and [address].
    match post {
        0x8F => return Some(0),
        0xAF => return Some(2),
        0xCF | 0xEF => return Some(1),
        0x90 => return Some(3),
        0xB0 => return Some(5),
        0xD0 | 0xF0 => return Some(4),
        0x9F => return Some(pick(5, 4)),
        _ => {}
    }
    // What the mode adds, and what it adds indirect (None where it can't be).
    let (plain, indirect) = match post & 0x0F {
        // ,R+ and ,-R.
        0x0 | 0x2 => (pick(2, 1), None),
        // ,R++ and ,--R.
        0x1 | 0x3 => (pick(3, 2), Some(pick(6, 5))),
        // ,R.
        0x4 => (0, Some(3)),
        // An 8-bit offset, from R or PC, or A, B, E, F or W added to R.
        0x5 | 0x6 | 0x7 | 0x8 | 0xA | 0xC | 0xE => (1, Some(4)),
        // A 16-bit offset.
        0x9 => (pick(4, 3), Some(pick(7, 6))),
        // D added to R.
        0xB => (pick(4, 2), Some(pick(7, 5))),
        // A 16-bit offset from PC.
        0xD => (pick(5, 3), Some(pick(8, 6))),
        _ => return None,
    };
    if post & 0x10 != 0 { indirect } else { Some(plain) }
}

/// The cycles PSHS, PULS, PSHU and PULU spend on the registers in their postbyte: one for each byte moved.
fn moved(mask: u8) -> u32 {
    (0..8).filter(|bit| mask & (1 << bit) != 0).map(|bit| if bit >= 4 { 2 } else { 1 }).sum()
}

/// An instruction's cycles in a mode, from its bytes. None for bytes that aren't an instruction.
pub fn cycles(bytes: &[u8], mode: Mode) -> Option<Timing> {
    let code = opcode(bytes)?;
    let at = BASE.binary_search_by_key(&code, |entry| entry.0).ok()?;
    let (_, emulation, native) = BASE[at];
    let native_mode = mode == Mode::Native;
    let mut timing = Timing::fixed(u32::from(if native_mode { native } else { emulation }));
    if let Some(at) = postbyte_at(code) {
        let extra = indexed_extra(*bytes.get(at)?, mode)?;
        timing.least += extra;
        timing.most += extra;
    }
    match code {
        0x34..=0x37 => {
            let extra = moved(*bytes.get(1)?);
            timing.least += extra;
            timing.most += extra;
        }
        // A conditional long branch takes one more when it's taken, on a 6809 and in emulation mode.
        0x1022..=0x102F if !native_mode => timing.most += 1,
        // RTI: 6 after a FIRQ, which stacked only CC and PC; more after the others, which stacked everything (E and
        // F too in native mode).
        0x3B => timing.most = if native_mode { 17 } else { 15 },
        // SYNC and CWAI wait for an interrupt.
        0x13 | 0x3C => timing.waits = true,
        // TFM moves a byte every 3 cycles.
        0x1138..=0x113B => timing.per_byte = 3,
        // MULD takes 2 more when either number is negative (as measured on a real 6309 by the 6809Decoder project).
        0x118F | 0x119F | 0x11AF | 0x11BF => timing.most += 2,
        _ => {}
    }
    Some(timing)
}

/// Why an instruction's cycles may vary, in words, or None if they don't.
pub fn why(bytes: &[u8], mode: Mode) -> Option<String> {
    let code = opcode(bytes)?;
    let native_mode = mode == Mode::Native;
    Some(match code {
        0x1022..=0x102F if !native_mode => crate::i18n::tr("one more when taken").into(),
        0x3B if native_mode => crate::i18n::tr("6 after a FIRQ; 17 after the others, which stack every register").into(),
        0x3B => crate::i18n::tr("6 after a FIRQ; 15 after the others, which stack every register").into(),
        0x13 | 0x3C => crate::i18n::tr("at least: it waits for an interrupt").into(),
        0x1138..=0x113B => crate::i18n::tr("3 more for each byte moved; W says how many").into(),
        0x118F | 0x119F | 0x11AF | 0x11BF => crate::i18n::tr("2 more when either number is negative").into(),
        0x118D | 0x119D | 0x11AD | 0x11BD | 0x118E | 0x119E | 0x11AE | 0x11BE => crate::i18n::tr("fewer when the quotient doesn't fit").into(),
        _ => return None,
    })
}

/// Whether bytes a line made are one whole instruction (not an FCB table that happens to start with an opcode).
pub fn is_instruction(bytes: &[u8]) -> bool {
    if bytes.is_empty() || cycles(bytes, Mode::Emulation).is_none() {
        return false;
    }
    crate::disasm::m6809(&|i| bytes.get(i).copied().unwrap_or(0), 0, true).length == bytes.len()
}

/// An instruction's cycles in both modes, for the instruction reference: "5", or "5 (4 native)".
pub fn both(bytes: &[u8]) -> Option<String> {
    let emulation = cycles(bytes, Mode::Emulation)?.text();
    let native = cycles(bytes, Mode::Native)?.text();
    Some(if emulation == native { emulation } else { crate::i18n::tf!("{emulation} ({native} native)") })
}

/// The mode each line of an assembly runs in, in the order of its lines. PRAGMA 6309 counts native mode from where
/// it is (as lwasm's listings do) and PRAGMA 6809 a 6809's; LDMD sets the mode from the line after it (bit 0 of its
/// value is native mode). Before either, a 6809's.
pub fn modes(assembly: &Assembly) -> Vec<Mode> {
    let mut changes = assembly.settings.iter().filter(|(_, key, _)| *key == "6309").peekable();
    let mut mode = Mode::Emulation;
    let mut out = Vec::with_capacity(assembly.lines.len());
    for (index, placed) in assembly.lines.iter().enumerate() {
        while let Some((_, _, value)) = changes.next_if(|(at, _, _)| *at <= index) {
            mode = if *value != 0 { Mode::Native } else { Mode::Emulation };
        }
        out.push(mode);
        if let [0x11, 0x3D, value] = placed.bytes[..] {
            mode = if value & 1 != 0 { Mode::Native } else { Mode::Emulation };
        }
    }
    out
}

/// A line's instruction, the mode it runs in and its cycles, if the line is one whole instruction.
pub fn line(assembly: &Assembly, file: usize, line: usize) -> Option<(&Placed, Mode, Timing)> {
    let index = assembly.lines.iter().position(|p| p.file == file && p.line == line && !p.bytes.is_empty())?;
    let placed = &assembly.lines[index];
    if !is_instruction(&placed.bytes) {
        return None;
    }
    let mode = *modes(assembly).get(index)?;
    Some((placed, mode, cycles(&placed.bytes, mode)?))
}

/// The words for a line's cycles: "5 cycles", or "4 cycles in native mode".
pub fn words(timing: Timing, mode: Mode) -> String {
    let text = timing.text();
    match mode {
        Mode::Emulation => crate::i18n::tf!("{text} cycles"),
        Mode::Native => crate::i18n::tf!("{text} cycles in native mode"),
    }
}

/// What to say below a line's bytes when it's pointed at: what the other mode takes, and why its cycles may vary.
pub fn more(bytes: &[u8], mode: Mode) -> String {
    let other = match mode {
        Mode::Emulation => Mode::Native,
        Mode::Native => Mode::Emulation,
    };
    let mut out = String::new();
    if let Some(why) = why(bytes, mode) {
        out.push_str(&format!("\n({why})"));
    }
    if let (Some(here), Some(there)) = (cycles(bytes, mode), cycles(bytes, other))
        && here != there
    {
        let there = there.text();
        out.push_str(&match other {
            Mode::Native => crate::i18n::tf!("\n{there} in a 6309's native mode"),
            Mode::Emulation => crate::i18n::tf!("\n{there} on a 6809, or a 6309 in emulation mode"),
        });
    }
    out
}

/// The cycles of the lines from `first` to `last` of a file of an assembly, each in the mode it runs in: their sum,
/// how many instructions there were, and whether every one of them runs in native mode.
pub fn block(assembly: &Assembly, file: usize, first: usize, last: usize) -> (Timing, usize, bool) {
    let mut total = Timing::default();
    let mut count = 0;
    let mut native = true;
    for (placed, mode) in assembly.lines.iter().zip(modes(assembly)).filter(|(p, _)| p.file == file && p.line >= first && p.line <= last && is_instruction(&p.bytes)) {
        if let Some(timing) = cycles(&placed.bytes, mode) {
            total = total.add(timing);
            count += 1;
            native &= mode == Mode::Native;
        }
    }
    (total, count, native && count > 0)
}

/// Each line's cycles, for the lines from `first` to `last` of a document of 6809 or 6309 source, from the program
/// as last assembled. Only lines that are one whole instruction are timed.
pub fn line_notes(intel: &crate::code_intel::CodeIntel, first: usize, last: usize) -> std::collections::HashMap<usize, String> {
    let Some(assembly) = &intel.assembly else {
        return std::collections::HashMap::new();
    };
    let modes = modes(assembly);
    assembly
        .lines
        .iter()
        .zip(modes)
        .filter(|(p, _)| p.file == intel.file && p.line >= first && p.line <= last && is_instruction(&p.bytes))
        .filter_map(|(p, mode)| cycles(&p.bytes, mode).map(|c| (p.line, c.text())))
        .collect()
}

#[cfg(test)]
mod tests;
