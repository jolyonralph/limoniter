//! The colour picker's thinking, apart from its window: colours read and written in the forms web files use (hex,
//! rgb(), hsl(), oklch(), named colours) and in each older machine's own (the Amiga's $0RGB, the Spectrum's INK,
//! the CPC's firmware numbers and so on); the nearest colour a machine can show; which form a file wants; the colour
//! written at the cursor; WCAG contrast; and taking a picture of the screen to pick from.
//!
//! The machines' palettes themselves live where the picture converters keep them (`retro_convert`, `retro_screens`,
//! `next_screen`, `st_planar`, `pictures::atari8`, `atari2600_gfx`), so each is in one place only. The NES's is the
//! one not kept elsewhere, and is here.

use crate::view_extras::Form;

pub type Rgb = [u8; 3];

/// The machines whose colours the picker knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Machine {
    /// The original and enhanced chip sets: four bits a colour, written $0RGB.
    Amiga,
    /// The Advanced Graphics Architecture: eight bits a colour, written to the registers as two words.
    AmigaAga,
    Spectrum,
    Next,
    C64,
    Cpc,
    AtariSt,
    AtariSte,
    Bbc,
    Msx,
    Atari8,
    Atari2600,
    Nes,
}

impl Machine {
    pub const ALL: [Machine; 13] = [
        Machine::Amiga,
        Machine::AmigaAga,
        Machine::Spectrum,
        Machine::Next,
        Machine::C64,
        Machine::Cpc,
        Machine::AtariSt,
        Machine::AtariSte,
        Machine::Bbc,
        Machine::Msx,
        Machine::Atari8,
        Machine::Atari2600,
        Machine::Nes,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Machine::Amiga => "Amiga (OCS and ECS, 4096 colours)",
            Machine::AmigaAga => "Amiga AGA (24-bit)",
            Machine::Spectrum => "ZX Spectrum (with BRIGHT)",
            Machine::Next => "ZX Spectrum Next (9-bit)",
            Machine::C64 => "Commodore 64",
            Machine::Cpc => "Amstrad CPC (27 colours)",
            Machine::AtariSt => "Atari ST (512 colours)",
            Machine::AtariSte => "Atari STE (4096 colours)",
            Machine::Bbc => "BBC Micro",
            Machine::Msx => "MSX",
            Machine::Atari8 => "Atari 8-bit",
            Machine::Atari2600 => "Atari 2600 (NTSC)",
            Machine::Nes => "NES",
        }
    }

    /// The colours there are to see side by side: none for the machines with too many.
    pub fn grid(self) -> Vec<Rgb> {
        match self {
            Machine::Amiga | Machine::AmigaAga | Machine::AtariSte => Vec::new(),
            _ => palette(self),
        }
    }
}

/// What a file's colours are written as: a web file's, or a machine's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Web,
    Machine(Machine),
}

/// How a number is written in source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Decimal,
    /// `$` and this many hex digits (at least).
    Dollar(usize),
    /// `&` and hex, as the CPC and BBC write it.
    Ampersand(usize),
    /// `0x` and hex.
    ZeroX(usize),
    /// The AGA's low nibbles of each channel, `$0rgb`, as written to the register after the high ones.
    AgaLow,
}

/// A colour's way of being written: a web form, or a machine's number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notation {
    Web(Form),
    Machine(Machine, Style),
}

impl Notation {
    /// What to write when the file has no colour there already.
    pub fn default_for(context: Context) -> Notation {
        match context {
            Context::Web => Notation::Web(Form::Hex6),
            Context::Machine(m) => Notation::Machine(m, default_style(m)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Notation::Web(Form::Hex3) => "#rgb",
            Notation::Web(Form::Hex6) => "#rrggbb",
            Notation::Web(Form::Hex8) => "#rrggbbaa",
            Notation::Web(Form::Rgb) => "rgb()",
            Notation::Web(Form::Rgba) => "rgba()",
            Notation::Web(Form::Hsl) => "hsl()",
            Notation::Web(Form::Oklch) => "oklch()",
            Notation::Web(Form::Named) => "A colour name",
            Notation::Web(Form::Amiga) => "$0RGB",
            Notation::Machine(_, Style::AgaLow) => "AGA low nibbles $0rgb",
            Notation::Machine(Machine::AmigaAga, _) => "AGA high nibbles $0RGB",
            Notation::Machine(Machine::Amiga, _) => "$0RGB",
            Notation::Machine(Machine::AtariSt | Machine::AtariSte, _) => "$0RGB",
            Notation::Machine(_, Style::Decimal) => "A number",
            Notation::Machine(_, Style::Dollar(_)) => "A $ hex number",
            Notation::Machine(_, Style::Ampersand(_)) => "An & hex number",
            Notation::Machine(_, Style::ZeroX(_)) => "A 0x hex number",
        }
    }
}

fn default_style(m: Machine) -> Style {
    match m {
        Machine::Amiga | Machine::AmigaAga | Machine::AtariSt | Machine::AtariSte => Style::Dollar(4),
        Machine::Next | Machine::Atari8 | Machine::Atari2600 | Machine::Nes => Style::Dollar(2),
        Machine::Spectrum | Machine::C64 | Machine::Cpc | Machine::Bbc | Machine::Msx => Style::Decimal,
    }
}

// What a file is for.

/// Whether `word` is in `text` as a word of its own.
fn has_word(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    text.match_indices(word).any(|(at, _)| {
        let before = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = bytes.get(at + word.len()).is_none_or(|b| !b.is_ascii_alphanumeric());
        before && after
    })
}

/// The colours a file is written in, from its language, its name and what it says. `text` is the start of the file.
pub fn detect(language: &str, file_name: &str, text: &str) -> Context {
    let lang = language.to_ascii_lowercase();
    let lower = text.to_ascii_lowercase();
    let name = file_name.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let amiga = |lower: &str| {
        if has_word(lower, "aga") || lower.contains("fmode") || lower.contains("loct") { Machine::AmigaAga } else { Machine::Amiga }
    };
    if lang.contains("680x0") || lang.contains("68000") {
        if any(&["$ff8240", "$ffff8240", "$ffff 8240", "$ff8250"]) {
            return Context::Machine(if has_word(&lower, "ste") || any(&["$ff8e", "$ffff8e"]) { Machine::AtariSte } else { Machine::AtariSt });
        }
        return Context::Machine(amiga(&lower));
    }
    if lang.contains("amos") || lang.contains("blitz") || lang == "amiga e" || lang.starts_with("amiga") {
        return Context::Machine(amiga(&lower));
    }
    if matches!(lang.as_str(), "c" | "c++" | "objective-c" | "objective-c++") && any(&["<exec/", "<proto/", "<intuition/", "<graphics/", "<hardware/custom", "<clib/", "<dos/dos.h"]) {
        return Context::Machine(amiga(&lower));
    }
    if lang.contains("sinclair") || lang.contains("boriel") || lang.contains("zx") {
        return Context::Machine(if any(&["nextreg", "layer 2", "layer2"]) { Machine::Next } else { Machine::Spectrum });
    }
    if lang.contains("z80") {
        let machine = if any(&["nextreg", "$243b", "layer2"]) {
            Machine::Next
        } else if has_word(&lower, "cpc") || lower.contains("gate array") || lower.contains("&7f00") {
            Machine::Cpc
        } else if has_word(&lower, "msx") || any(&["vdp", "$99"]) && has_word(&lower, "bios") {
            Machine::Msx
        } else {
            Machine::Spectrum
        };
        return Context::Machine(machine);
    }
    if lang.contains("commodore") || lang.contains("trse") || lang.contains("rgc") {
        return Context::Machine(Machine::C64);
    }
    if lang.contains("bbc") || lang.contains("6502") && any(&["osbyte", "oswrch", "&fe21", "&fe20"]) {
        return Context::Machine(Machine::Bbc);
    }
    if lang.contains("6502") {
        if any(&["$d020", "$d021", "$d800", "$d011", "vicii", "vic-ii"]) {
            return Context::Machine(Machine::C64);
        }
        if any(&["colpf", "colbk", "$d01a", "$d016"]) && !any(&["$d020"]) {
            return Context::Machine(Machine::Atari8);
        }
        if any(&["colubk", "colup0", "colupf", "wsync"]) {
            return Context::Machine(Machine::Atari2600);
        }
        if any(&["$2001", "$2006", "$2007", "ppuctrl", "ppumask", "ppuaddr"]) {
            return Context::Machine(Machine::Nes);
        }
    }
    // A file's name can say it where its language can't.
    if name.ends_with(".scr") || name.ends_with(".sna") {
        return Context::Machine(Machine::Spectrum);
    }
    if name.ends_with(".nes") {
        return Context::Machine(Machine::Nes);
    }
    Context::Web
}

// The machines' palettes.

/// The NES picture chip's 64 colours (the 2C02), as the FBX "smooth" palette has them.
pub const NES: [u32; 64] = [
    0x626262, 0x001FB2, 0x2404C8, 0x5200B2, 0x730076, 0x800024, 0x730B00, 0x522800, 0x244400, 0x005700, 0x005C00, 0x005324, 0x003C76, 0x000000, 0x000000, 0x000000, //
    0xABABAB, 0x0D57FF, 0x4B30FF, 0x8A13FF, 0xBC08D6, 0xD21269, 0xC72E00, 0x9D5400, 0x607B00, 0x209800, 0x00A300, 0x009942, 0x007DB4, 0x000000, 0x000000, 0x000000, //
    0xFFFFFF, 0x53AEFF, 0x9085FF, 0xD365FF, 0xFF57FF, 0xFF5DCF, 0xFF7757, 0xFA9E00, 0xBDC700, 0x7AE700, 0x43F611, 0x26EF7E, 0x2CD5F6, 0x4D4D4D, 0x000000, 0x000000, //
    0xFFFFFF, 0xB6E1FF, 0xCED1FF, 0xE9C3FF, 0xFFBCFF, 0xFFBDF4, 0xFFC6C3, 0xFFD59A, 0xE9E681, 0xCEF481, 0xB6FB9A, 0xA9FAC3, 0xA9F0F4, 0xB8B8B8, 0x000000, 0x000000,
];

/// The CPC's hardware colour numbers (the Gate Array's five bits) for each firmware number 0 to 26.
pub const CPC_HARDWARE: [u8; 27] = [0x14, 0x04, 0x15, 0x1C, 0x18, 0x1D, 0x0C, 0x05, 0x0D, 0x16, 0x06, 0x17, 0x1E, 0x00, 0x1F, 0x0E, 0x07, 0x0F, 0x12, 0x02, 0x13, 0x1A, 0x19, 0x1B, 0x0A, 0x03, 0x0B];

fn from_u32(c: u32) -> Rgb {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// A machine's colours by their numbers, for the machines with a list of them (the others give none).
pub fn palette(m: Machine) -> Vec<Rgb> {
    use crate::{pictures, retro_convert, retro_screens, st_planar};
    match m {
        Machine::Amiga | Machine::AmigaAga | Machine::AtariSte => Vec::new(),
        Machine::Spectrum => retro_convert::SPECTRUM.to_vec(),
        Machine::Next => (0..512u16).map(crate::next_screen::rgb).collect(),
        Machine::C64 => retro_screens::C64.iter().map(|c| [c.r(), c.g(), c.b()]).collect(),
        Machine::Cpc => retro_convert::CPC.to_vec(),
        Machine::AtariSt => (0..512u16).map(|i| st_planar::colour_of_word((i >> 6 & 7) << 8 | (i >> 3 & 7) << 4 | (i & 7), false)).collect(),
        Machine::Bbc => retro_convert::BBC.to_vec(),
        Machine::Msx => retro_convert::TMS9918.to_vec(),
        Machine::Atari8 => (0..=255u8).map(pictures::atari8::colour).collect(),
        Machine::Atari2600 => (0..128u8).map(|i| crate::atari2600_gfx::Tv::Ntsc.rgb(i * 2)).collect(),
        Machine::Nes => NES.iter().map(|c| from_u32(*c)).collect(),
    }
}

/// A colour a machine can show, and the number that asks for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapped {
    pub value: u32,
    pub rgb: Rgb,
}

fn nearest_in(colours: &[Rgb], rgb: Rgb) -> usize {
    let labs: Vec<[f32; 3]> = colours.iter().map(|c| crate::quantise::oklab(crate::quantise::as_f32(*c))).collect();
    crate::quantise::nearest(&labs, crate::quantise::oklab(crate::quantise::as_f32(rgb))).0
}

/// The colour of the machine nearest to `rgb`. A machine's own colour comes back as itself.
pub fn snap(m: Machine, rgb: Rgb) -> Snapped {
    use crate::st_planar::{Levels, colour_of_word, st_word};
    match m {
        Machine::Amiga => {
            let n = |v: u8| (u32::from(v) * 15 + 127) / 255;
            let value = n(rgb[0]) << 8 | n(rgb[1]) << 4 | n(rgb[2]);
            Snapped { value, rgb: rgb.map(|v| ((u32::from(v) * 15 + 127) / 255 * 17) as u8) }
        }
        Machine::AmigaAga => Snapped { value: u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2]), rgb },
        Machine::Next => {
            let value = crate::next_screen::nearest_value(rgb);
            Snapped { value: u32::from(value), rgb: crate::next_screen::rgb(value) }
        }
        Machine::AtariSt | Machine::AtariSte => {
            let ste = m == Machine::AtariSte;
            let word = st_word(rgb, if ste { Levels::Ste } else { Levels::St });
            Snapped { value: u32::from(word), rgb: colour_of_word(word, ste) }
        }
        _ => {
            let colours = palette(m);
            let index = nearest_in(&colours, rgb);
            let value = if m == Machine::Atari2600 { index as u32 * 2 } else { index as u32 };
            Snapped { value, rgb: colours[index] }
        }
    }
}

/// The colour a machine's number means, or None if the machine has no such number.
pub fn decode(m: Machine, value: u32) -> Option<Rgb> {
    use crate::st_planar::colour_of_word;
    match m {
        Machine::Amiga => (value <= 0xFFF).then(|| [(value >> 8 & 15) as u8 * 17, (value >> 4 & 15) as u8 * 17, (value & 15) as u8 * 17]),
        Machine::AmigaAga => (value <= 0xFF_FFFF).then(|| from_u32(value)),
        Machine::Next => (value <= 0x1FF).then(|| crate::next_screen::rgb(value as u16)),
        Machine::AtariSt | Machine::AtariSte => (value <= 0xFFF).then(|| colour_of_word(value as u16, m == Machine::AtariSte)),
        Machine::Atari2600 => (value <= 255).then(|| crate::atari2600_gfx::Tv::Ntsc.rgb(value as u8)),
        _ => palette(m).get(value as usize).copied(),
    }
}

/// The largest number a machine's code takes, used to tell whether a number in a file could be a colour.
fn most(m: Machine) -> u32 {
    match m {
        Machine::Amiga | Machine::AtariSt | Machine::AtariSte => 0xFFF,
        Machine::AmigaAga => 0xFF_FFFF,
        Machine::Next => 0x1FF,
        Machine::Atari8 | Machine::Atari2600 => 255,
        Machine::Spectrum | Machine::C64 | Machine::Msx => 15,
        Machine::Cpc => 26,
        Machine::Bbc => 7,
        Machine::Nes => 63,
    }
}

/// A machine's number written in a style.
pub fn write_number(m: Machine, value: u32, style: Style) -> String {
    match (m, style) {
        (Machine::AmigaAga, Style::AgaLow) => format!("$0{:X}{:X}{:X}", value >> 16 & 15, value >> 8 & 15, value & 15),
        (Machine::AmigaAga, _) => format!("$0{:X}{:X}{:X}", value >> 20 & 15, value >> 12 & 15, value >> 4 & 15),
        (Machine::Next, Style::Dollar(_) | Style::Ampersand(_) | Style::ZeroX(_)) => {
            // The eight-bit form when it says everything; the ninth bit's number when it doesn't.
            let eight = value >> 1;
            let plain = next_nine(eight) == value;
            let (prefix, digits, number) = if plain { (prefix_of(style), 2, eight) } else { (prefix_of(style), 3, value) };
            format!("{prefix}{number:0digits$X}")
        }
        (_, Style::Decimal) => value.to_string(),
        (_, Style::Dollar(w) | Style::Ampersand(w) | Style::ZeroX(w)) => format!("{}{value:0w$X}", prefix_of(style)),
        (_, Style::AgaLow) => value.to_string(),
    }
}

/// The Next's nine-bit number for an eight-bit RRRGGGBB byte: the lowest blue bit is on when either of the others
/// is, as at reset.
pub fn next_nine(eight: u32) -> u32 {
    eight << 1 | u32::from(eight & 3 != 0)
}

fn prefix_of(style: Style) -> &'static str {
    match style {
        Style::Dollar(_) | Style::AgaLow => "$",
        Style::Ampersand(_) => "&",
        Style::ZeroX(_) => "0x",
        Style::Decimal => "",
    }
}

/// How a machine asks for a colour, in its own words: each line a label and the text. `value` is a `snap`'s.
pub fn code_lines(m: Machine, value: u32) -> Vec<(&'static str, String)> {
    match m {
        Machine::Amiga => vec![("Colour register", format!("move.w #${value:03X},COLOR00")), ("Copper list", format!("dc.w $0180,${value:04X}"))],
        Machine::AmigaAga => vec![
            ("High nibbles", write_number(m, value, Style::Dollar(4))),
            ("Low nibbles", write_number(m, value, Style::AgaLow)),
            ("In the registers", "Write the high word, set LOCT in BPLCON3, then the low word".to_string()),
        ],
        Machine::Spectrum => {
            let (ink, bright) = (value & 7, value >> 3 & 1);
            vec![("BASIC", format!("INK {ink}: BRIGHT {bright}")), ("Attribute byte (ink on black)", format!("${:02X}", 0x40 * bright + ink))]
        }
        Machine::Next => vec![
            ("RRRGGGBB byte", format!("${:02X}", value >> 1)),
            ("Blue's lowest bit", (value & 1).to_string()),
            ("9-bit number", format!("${value:03X}")),
        ],
        Machine::C64 => vec![("Colour number", value.to_string()), ("BASIC", format!("POKE 53281,{value}"))],
        Machine::Cpc => {
            let hardware = CPC_HARDWARE.get(value as usize).copied().unwrap_or(0);
            vec![("Firmware number", value.to_string()), ("Hardware number", format!("&{hardware:02X} (the Gate Array is sent &{:02X})", 0x40 | hardware)), ("BASIC", format!("INK 0,{value}"))]
        }
        Machine::AtariSt => vec![("Palette word", format!("${value:04X}")), ("Assembly", format!("move.w #${value:04X},$ffff8240"))],
        Machine::AtariSte => vec![("Palette word", format!("${value:04X}")), ("Assembly", format!("move.w #${value:04X},$ffff8240"))],
        Machine::Bbc => vec![("Colour number", value.to_string()), ("BASIC", format!("VDU 19,0,{value};0;"))],
        Machine::Msx => vec![("Colour number", value.to_string()), ("BASIC", format!("COLOR {value}"))],
        Machine::Atari8 => vec![("Colour byte", format!("${value:02X}")), ("BASIC", format!("SETCOLOR 2,{},{}", value >> 4, value & 15))],
        Machine::Atari2600 => vec![("Colour byte", format!("${value:02X}")), ("Assembly", format!("lda #${value:02X}"))],
        Machine::Nes => vec![("Palette byte", format!("${value:02X}")), ("Assembly", format!("lda #${value:02X}"))],
    }
}

// The web's forms.

/// CSS's named colours (and the older spelling "grey" for each "gray", which is read but not written).
pub const NAMES: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF), ("antiquewhite", 0xFAEBD7), ("aqua", 0x00FFFF), ("aquamarine", 0x7FFFD4), ("azure", 0xF0FFFF), ("beige", 0xF5F5DC), ("bisque", 0xFFE4C4), ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD), ("blue", 0x0000FF), ("blueviolet", 0x8A2BE2), ("brown", 0xA52A2A), ("burlywood", 0xDEB887), ("cadetblue", 0x5F9EA0), ("chartreuse", 0x7FFF00), ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50), ("cornflowerblue", 0x6495ED), ("cornsilk", 0xFFF8DC), ("crimson", 0xDC143C), ("cyan", 0x00FFFF), ("darkblue", 0x00008B), ("darkcyan", 0x008B8B), ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9), ("darkgreen", 0x006400), ("darkkhaki", 0xBDB76B), ("darkmagenta", 0x8B008B), ("darkolivegreen", 0x556B2F), ("darkorange", 0xFF8C00), ("darkorchid", 0x9932CC), ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A), ("darkseagreen", 0x8FBC8F), ("darkslateblue", 0x483D8B), ("darkslategray", 0x2F4F4F), ("darkturquoise", 0x00CED1), ("darkviolet", 0x9400D3), ("deeppink", 0xFF1493), ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969), ("dodgerblue", 0x1E90FF), ("firebrick", 0xB22222), ("floralwhite", 0xFFFAF0), ("forestgreen", 0x228B22), ("fuchsia", 0xFF00FF), ("gainsboro", 0xDCDCDC), ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700), ("goldenrod", 0xDAA520), ("gray", 0x808080), ("green", 0x008000), ("greenyellow", 0xADFF2F), ("honeydew", 0xF0FFF0), ("hotpink", 0xFF69B4), ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082), ("ivory", 0xFFFFF0), ("khaki", 0xF0E68C), ("lavender", 0xE6E6FA), ("lavenderblush", 0xFFF0F5), ("lawngreen", 0x7CFC00), ("lemonchiffon", 0xFFFACD), ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080), ("lightcyan", 0xE0FFFF), ("lightgoldenrodyellow", 0xFAFAD2), ("lightgray", 0xD3D3D3), ("lightgreen", 0x90EE90), ("lightpink", 0xFFB6C1), ("lightsalmon", 0xFFA07A), ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA), ("lightslategray", 0x778899), ("lightsteelblue", 0xB0C4DE), ("lightyellow", 0xFFFFE0), ("lime", 0x00FF00), ("limegreen", 0x32CD32), ("linen", 0xFAF0E6), ("magenta", 0xFF00FF),
    ("maroon", 0x800000), ("mediumaquamarine", 0x66CDAA), ("mediumblue", 0x0000CD), ("mediumorchid", 0xBA55D3), ("mediumpurple", 0x9370DB), ("mediumseagreen", 0x3CB371), ("mediumslateblue", 0x7B68EE), ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC), ("mediumvioletred", 0xC71585), ("midnightblue", 0x191970), ("mintcream", 0xF5FFFA), ("mistyrose", 0xFFE4E1), ("moccasin", 0xFFE4B5), ("navajowhite", 0xFFDEAD), ("navy", 0x000080),
    ("oldlace", 0xFDF5E6), ("olive", 0x808000), ("olivedrab", 0x6B8E23), ("orange", 0xFFA500), ("orangered", 0xFF4500), ("orchid", 0xDA70D6), ("palegoldenrod", 0xEEE8AA), ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE), ("palevioletred", 0xDB7093), ("papayawhip", 0xFFEFD5), ("peachpuff", 0xFFDAB9), ("peru", 0xCD853F), ("pink", 0xFFC0CB), ("plum", 0xDDA0DD), ("powderblue", 0xB0E0E6),
    ("purple", 0x800080), ("rebeccapurple", 0x663399), ("red", 0xFF0000), ("rosybrown", 0xBC8F8F), ("royalblue", 0x4169E1), ("saddlebrown", 0x8B4513), ("salmon", 0xFA8072), ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57), ("seashell", 0xFFF5EE), ("sienna", 0xA0522D), ("silver", 0xC0C0C0), ("skyblue", 0x87CEEB), ("slateblue", 0x6A5ACD), ("slategray", 0x708090), ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F), ("steelblue", 0x4682B4), ("tan", 0xD2B48C), ("teal", 0x008080), ("thistle", 0xD8BFD8), ("tomato", 0xFF6347), ("turquoise", 0x40E0D0), ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3), ("white", 0xFFFFFF), ("whitesmoke", 0xF5F5F5), ("yellow", 0xFFFF00), ("yellowgreen", 0x9ACD32), ("transparent", 0x000000),
];

/// The colour a name stands for ("grey" read as "gray"). "transparent" is read as black with no opacity by the caller.
pub fn named(name: &str) -> Option<Rgb> {
    let name = name.to_ascii_lowercase().replace("grey", "gray");
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, c)| from_u32(*c))
}

/// The name nearest a colour, and whether it's exactly that colour.
pub fn nearest_name(rgb: Rgb) -> (&'static str, bool) {
    let list: Vec<(&str, Rgb)> = NAMES.iter().filter(|(n, _)| *n != "transparent").map(|(n, c)| (*n, from_u32(*c))).collect();
    if let Some((n, _)) = list.iter().find(|(_, c)| *c == rgb) {
        return (n, true);
    }
    let colours: Vec<Rgb> = list.iter().map(|(_, c)| *c).collect();
    (list[nearest_in(&colours, rgb)].0, false)
}

fn linear(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn gamma(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A colour in Oklch: lightness (0 to 1), chroma and hue (degrees).
pub fn to_oklch(rgb: Rgb) -> (f64, f64, f64) {
    let [r, g, b] = rgb.map(linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    let lightness = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
    let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    let chroma = a.hypot(bb);
    let hue = if chroma < 1e-4 { 0.0 } else { bb.atan2(a).to_degrees().rem_euclid(360.0) };
    (lightness, if chroma < 1e-4 { 0.0 } else { chroma }, hue)
}

/// The sRGB colour of an Oklch one, squeezed into what sRGB can show.
pub fn from_oklch(lightness: f64, chroma: f64, hue: f64) -> Rgb {
    let (a, b) = (chroma * hue.to_radians().cos(), chroma * hue.to_radians().sin());
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        gamma(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        gamma(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        gamma(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s),
    ]
}

pub fn oklch_text(rgb: Rgb) -> String {
    let (l, c, h) = to_oklch(rgb);
    format!("oklch({:.1}% {c:.3} {h:.1})", l * 100.0)
}

fn rgb_to_hsl(rgb: Rgb) -> (f64, f64, f64) {
    let [r, g, b] = rgb.map(|v| f64::from(v) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    (h * 60.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// A colour (and its opacity, 0 to 255) written in a web form.
pub fn write_web(rgb: Rgb, alpha: u8, form: Form) -> String {
    let [r, g, b] = rgb;
    match form {
        Form::Hex3 if rgb.iter().all(|v| v % 17 == 0) => format!("#{:X}{:X}{:X}", r / 17, g / 17, b / 17),
        Form::Hex3 | Form::Hex6 => format!("#{r:02X}{g:02X}{b:02X}"),
        Form::Hex8 => format!("#{r:02X}{g:02X}{b:02X}{alpha:02X}"),
        Form::Rgb => format!("rgb({r}, {g}, {b})"),
        Form::Rgba => format!("rgba({r}, {g}, {b}, {:.2})", f64::from(alpha) / 255.0),
        Form::Hsl => {
            let (h, s, l) = rgb_to_hsl(rgb);
            format!("hsl({h:.0}, {:.0}%, {:.0}%)", s * 100.0, l * 100.0)
        }
        Form::Oklch => oklch_text(rgb),
        Form::Named => nearest_name(rgb).0.to_string(),
        Form::Amiga => write_number(Machine::Amiga, snap(Machine::Amiga, rgb).value, Style::Dollar(4)),
    }
}

/// A colour typed in any web form (or a hex number without its #), and its opacity.
pub fn parse_web(text: &str) -> Option<(Rgb, u8)> {
    let text = text.trim().trim_end_matches(';').trim();
    if text.is_empty() {
        return None;
    }
    let hex = text.strip_prefix('#').unwrap_or(text);
    if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) && (text.starts_with('#') || hex.len() != 3 || hex.chars().any(|c| c.is_ascii_digit())) && (text.starts_with('#') || named(text).is_none()) {
        let n = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0);
        return Some(match hex.len() {
            3 | 4 => {
                let d: Vec<u8> = hex.chars().map(|c| n(&c.to_string()) * 17).collect();
                ([d[0], d[1], d[2]], d.get(3).copied().unwrap_or(255))
            }
            6 => ([n(&hex[0..2]), n(&hex[2..4]), n(&hex[4..6])], 255),
            _ => ([n(&hex[0..2]), n(&hex[2..4]), n(&hex[4..6])], n(&hex[6..8])),
        });
    }
    let lower = text.to_ascii_lowercase();
    if let Some(open) = lower.find('(')
        && lower.ends_with(')')
    {
        let function = lower[..open].trim();
        let inside = &lower[open + 1..lower.len() - 1];
        let parts: Vec<&str> = inside.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
        if parts.len() < 3 {
            return None;
        }
        let number = |s: &str| s.trim_end_matches("deg").trim_end_matches('%').parse::<f64>().ok();
        let percent = |s: &str| s.ends_with('%');
        let alpha = match parts.get(3) {
            Some(a) => {
                let v = number(a)?;
                (if percent(a) { v / 100.0 } else { v }).clamp(0.0, 1.0)
            }
            None => 1.0,
        };
        let alpha = (alpha * 255.0).round() as u8;
        let rgb = match function {
            "rgb" | "rgba" => {
                let level = |s: &str| -> Option<u8> {
                    let v = number(s)?;
                    Some((if percent(s) { v * 2.55 } else { v }).round().clamp(0.0, 255.0) as u8)
                };
                [level(parts[0])?, level(parts[1])?, level(parts[2])?]
            }
            "hsl" | "hsla" => hsl_to_rgb(number(parts[0])?, number(parts[1])? / 100.0, number(parts[2])? / 100.0),
            "oklch" => {
                let l = number(parts[0])?;
                let l = if percent(parts[0]) { l / 100.0 } else { l };
                from_oklch(l.clamp(0.0, 1.0), number(parts[1])?.max(0.0), number(parts[2])?)
            }
            _ => return None,
        };
        return Some((rgb, alpha));
    }
    named(&lower).map(|c| (c, if lower == "transparent" { 0 } else { 255 }))
}

// Contrast.

/// A colour's relative luminance, as WCAG 2 has it.
pub fn luminance(rgb: Rgb) -> f64 {
    let [r, g, b] = rgb.map(linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2's contrast ratio between two colours: 1 (the same) to 21 (black on white).
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Which levels a ratio reaches: AA and AAA for normal text, then for large text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grades {
    pub aa: bool,
    pub aaa: bool,
    pub aa_large: bool,
    pub aaa_large: bool,
}

pub fn grades(ratio: f64) -> Grades {
    // WCAG judges the ratio as shown to one place, so 4.499 doesn't pass as 4.5 would.
    Grades { aa: ratio >= 4.5, aaa: ratio >= 7.0, aa_large: ratio >= 3.0, aaa_large: ratio >= 4.5 }
}

// Colours written in a file.

/// A colour found written in a line: where (by character), what it is and how it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct Literal {
    pub from: usize,
    pub to: usize,
    pub rgb: Rgb,
    pub alpha: u8,
    pub notation: Notation,
}

/// The colour written where the cursor is (`caret`, a character's place in `line`), if there is one: a web form in
/// any file; a name in a web file; or in a machine's file a number that machine would take for a colour.
pub fn literal_at(line: &str, caret: usize, context: Context) -> Option<Literal> {
    // In a machine's source `#200` is a number being loaded, not a web colour, so the hex forms are left out there.
    let hex_form = |f: Form| matches!(f, Form::Hex3 | Form::Hex6 | Form::Hex8);
    if let Some(w) = crate::view_extras::colours_in(line).into_iter().find(|w| caret >= w.from && caret <= w.to && !(context != Context::Web && hex_form(w.form))) {
        let [r, g, b, a] = w.colour.to_srgba_unmultiplied();
        // A $0RGB the swatches found beside a colour register is the Amiga's, whatever the file is thought to be for.
        let notation = if w.form == Form::Amiga { Notation::Machine(Machine::Amiga, Style::Dollar(4)) } else { Notation::Web(w.form) };
        return Some(Literal { from: w.from, to: w.to, rgb: [r, g, b], alpha: a, notation });
    }
    let chars: Vec<char> = line.chars().collect();
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '$' | '&' | '_');
    let mut start = caret.min(chars.len());
    while start > 0 && word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = caret.min(chars.len());
    while end < chars.len() && word(chars[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    let token: String = chars[start..end].iter().collect();
    match context {
        Context::Web => {
            let (rgb, alpha) = (named(&token)?, if token.eq_ignore_ascii_case("transparent") { 0 } else { 255 });
            Some(Literal { from: start, to: end, rgb, alpha, notation: Notation::Web(Form::Named) })
        }
        Context::Machine(m) => {
            let (style, digits, base) = if let Some(h) = token.strip_prefix('$') {
                (Style::Dollar(h.len()), h, 16)
            } else if let Some(h) = token.strip_prefix('&') {
                (Style::Ampersand(h.len()), h, 16)
            } else if let Some(h) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
                (Style::ZeroX(h.len()), h, 16)
            } else {
                (Style::Decimal, token.as_str(), 10)
            };
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(base)) {
                return None;
            }
            let mut value = u32::from_str_radix(digits, base).ok()?;
            // The Next's two digits are the eight-bit byte; three are the whole nine bits.
            if m == Machine::Next && digits.len() <= 2 && base == 16 {
                value = next_nine(value);
            }
            let amiga_like = matches!(m, Machine::Amiga | Machine::AmigaAga | Machine::AtariSt | Machine::AtariSte);
            // A palette word is four digits, as the registers and copper lists have it.
            if amiga_like && (!matches!(style, Style::Dollar(4)) || value > 0xFFF) {
                return None;
            }
            if value > most(m) {
                return None;
            }
            let rgb = match m {
                // AGA's high word alone is only half a colour: its high nibbles, shown as such.
                Machine::AmigaAga => decode(Machine::Amiga, value)?,
                _ => decode(m, value)?,
            };
            Some(Literal { from: start, to: end, rgb, alpha: 255, notation: Notation::Machine(m, style) })
        }
    }
}

/// A colour written in a notation. A machine's is its nearest colour; the text says nothing of how near.
pub fn write(rgb: Rgb, alpha: u8, notation: Notation) -> String {
    match notation {
        Notation::Web(form) => write_web(rgb, alpha, form),
        Notation::Machine(m, style) => write_number(m, snap(m, rgb).value, style),
    }
}

// Taking a picture of the screen.

/// A picture of the screen.
pub struct Shot {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Shot {
    pub fn pixel(&self, x: usize, y: usize) -> Option<Rgb> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y * self.width + x) * 4;
        Some([self.rgba[at], self.rgba[at + 1], self.rgba[at + 2]])
    }
}

/// A PNG's pixels.
pub fn decode_png(bytes: &[u8]) -> Result<Shot, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("The picture is too big.")?;
    let mut data = vec![0; size];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let mut rgba = Vec::with_capacity(width * height * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(&data[..width * height * 4]),
        png::ColorType::Rgb => data[..width * height * 3].as_chunks::<3>().0.iter().for_each(|p| rgba.extend_from_slice(&[p[0], p[1], p[2], 255])),
        png::ColorType::Grayscale => data[..width * height].iter().for_each(|&g| rgba.extend_from_slice(&[g, g, g, 255])),
        png::ColorType::GrayscaleAlpha => data[..width * height * 2].as_chunks::<2>().0.iter().for_each(|p| rgba.extend_from_slice(&[p[0], p[0], p[0], 255])),
        png::ColorType::Indexed => return Err("The picture has colours this can't read.".to_string()),
    }
    Ok(Shot { width, height, rgba })
}

/// The programs that can take a picture of the screen on this system, each with its words, `{file}` standing for
/// where the PNG goes. Tried in turn until one makes a file.
pub fn grab_programs(file: &str) -> Vec<(String, Vec<String>)> {
    let strings = |list: &[&str]| list.iter().map(|s| s.replace("{file}", file)).collect::<Vec<String>>();
    if cfg!(windows) {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; $m=New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($m); $g.CopyFromScreen($b.Left,$b.Top,0,0,$m.Size); $m.Save('{}',[System.Drawing.Imaging.ImageFormat]::Png)",
            file.replace('\'', "''")
        );
        return vec![("powershell".to_string(), vec!["-NoProfile".into(), "-NonInteractive".into(), "-WindowStyle".into(), "Hidden".into(), "-Command".into(), script])];
    }
    if cfg!(target_os = "macos") {
        return vec![("screencapture".to_string(), strings(&["-x", "-t", "png", "{file}"]))];
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let mut list = vec![("grim", vec!["{file}"]), ("gnome-screenshot", vec!["-f", "{file}"]), ("spectacle", vec!["-b", "-n", "-f", "-o", "{file}"])];
    let x11 = vec![("scrot", vec!["-o", "{file}"]), ("maim", vec!["{file}"]), ("import", vec!["-window", "root", "{file}"]), ("gnome-screenshot", vec!["-f", "{file}"])];
    if wayland {
        list.extend(x11);
    } else {
        list = x11;
        list.push(("spectacle", vec!["-b", "-n", "-f", "-o", "{file}"]));
    }
    list.into_iter().map(|(p, a)| (p.to_string(), strings(&a))).collect()
}

/// Takes a picture of the whole screen with whatever program the system has for it. Slow (a second or so), so it's
/// run off to one side. On Wayland there is no way for a program to look at the screen itself, so it asks a
/// screenshot tool the desktop provides; and says so plainly when there is none.
pub fn grab_screen() -> Result<Shot, String> {
    let file = std::env::temp_dir().join(format!("limoniter-screen-{}.png", std::process::id()));
    let name = file.to_string_lossy().into_owned();
    let mut last = None;
    for (program, args) in grab_programs(&name) {
        let _ = std::fs::remove_file(&file);
        let made = crate::platform::command(&program).args(&args).env_remove("SDL_VIDEODRIVER").output();
        match made {
            Ok(out) if out.status.success() && file.is_file() => {
                let bytes = std::fs::read(&file).map_err(|e| e.to_string());
                let _ = std::fs::remove_file(&file);
                return decode_png(&bytes?);
            }
            Ok(out) => last = Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(_) => {}
        }
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    Err(match (wayland, last.filter(|l| !l.is_empty())) {
        (true, _) => crate::i18n::tr("This desktop is Wayland, which doesn't let a program look at the screen. Install grim or the desktop's own screenshot tool, or pick the colour with that tool and paste it in.").to_string(),
        (false, Some(why)) => crate::i18n::tf!("The screen couldn't be copied: {why}"),
        (false, None) => crate::i18n::tr("There's no program here to take a picture of the screen. Install scrot, maim, ImageMagick or gnome-screenshot.").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb, by: i32) -> bool {
        a.iter().zip(b).all(|(x, y)| (i32::from(*x) - i32::from(y)).abs() <= by)
    }

    #[test]
    fn contrast_matches_wcags_own_examples() {
        assert!((contrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-9);
        assert!((contrast([255, 255, 255], [255, 255, 255]) - 1.0).abs() < 1e-9);
        // #767676 on white is the lightest grey that passes AA for normal text; #777 just fails.
        let grey = |v: u8| [v, v, v];
        assert!((contrast(grey(0x76), [255; 3]) - 4.54).abs() < 0.005);
        assert!(grades(contrast(grey(0x76), [255; 3])).aa);
        assert!(!grades(contrast(grey(0x77), [255; 3])).aa);
        assert!((contrast(grey(0x77), [255; 3]) - 4.48).abs() < 0.005);
        // Order doesn't matter.
        assert_eq!(contrast([10, 20, 30], [200, 210, 220]), contrast([200, 210, 220], [10, 20, 30]));
        let g = grades(21.0);
        assert!(g.aa && g.aaa && g.aa_large && g.aaa_large);
        let g = grades(3.2);
        assert!(!g.aa && !g.aaa && g.aa_large && !g.aaa_large);
        let g = grades(5.0);
        assert!(g.aa && !g.aaa && g.aa_large && g.aaa_large);
    }

    #[test]
    fn web_forms_are_read_and_written_and_round_trip() {
        assert_eq!(parse_web("#ff8000"), Some(([255, 128, 0], 255)));
        assert_eq!(parse_web("#f80"), Some(([255, 136, 0], 255)));
        assert_eq!(parse_web("#ff800080"), Some(([255, 128, 0], 128)));
        assert_eq!(parse_web("rgb(0, 128, 255)"), Some(([0, 128, 255], 255)));
        assert_eq!(parse_web("rgb(0 128 255 / 50%)"), Some(([0, 128, 255], 128)));
        assert_eq!(parse_web("rgb(100%, 0%, 0%)"), Some(([255, 0, 0], 255)));
        assert_eq!(parse_web("hsl(120, 100%, 50%)"), Some(([0, 255, 0], 255)));
        assert_eq!(parse_web("hsl(240deg 100% 50%)"), Some(([0, 0, 255], 255)));
        assert_eq!(parse_web("rebeccapurple"), Some(([0x66, 0x33, 0x99], 255)));
        assert_eq!(parse_web("DarkGrey"), Some(([0xA9; 3], 255)));
        assert_eq!(parse_web("transparent").map(|c| c.1), Some(0));
        assert_eq!(parse_web("nonsense"), None);
        assert_eq!(parse_web("rgb(1, 2)"), None);
        // Oklch of pure red, from CSS Color 4's own figures: about 62.8% 0.2577 29.2.
        let (rgb, _) = parse_web("oklch(62.8% 0.2577 29.23)").unwrap();
        assert!(close(rgb, [255, 0, 0], 2), "{rgb:?}");
        for rgb in [[255, 0, 0], [12, 99, 200], [250, 250, 250], [0, 0, 0], [255, 255, 255], [128, 64, 32]] {
            let text = oklch_text(rgb);
            let (back, _) = parse_web(&text).unwrap();
            assert!(close(back, rgb, 2), "{text} gave {back:?}, not {rgb:?}");
            for form in [Form::Hex6, Form::Hex8, Form::Rgb, Form::Rgba, Form::Hsl] {
                let written = write_web(rgb, 255, form);
                let (back, alpha) = parse_web(&written).unwrap();
                // hsl() is written in whole degrees and percents, so it only comes back near.
                assert!(close(back, rgb, if form == Form::Hsl { 4 } else { 1 }), "{written}");
                assert_eq!(alpha, 255);
            }
        }
        assert_eq!(write_web([255, 0, 0], 255, Form::Hex3), "#F00");
        assert_eq!(write_web([255, 1, 0], 255, Form::Hex3), "#FF0100");
        assert_eq!(write_web([255, 128, 0], 128, Form::Hex8), "#FF800080");
        assert_eq!(write_web([0, 255, 0], 255, Form::Hsl), "hsl(120, 100%, 50%)");
    }

    #[test]
    fn names_are_found_and_the_nearest_is_shown() {
        assert!(NAMES.len() > 130, "the whole CSS list is here");
        assert_eq!(nearest_name([255, 0, 0]), ("red", true));
        assert_eq!(nearest_name([0x66, 0x33, 0x99]), ("rebeccapurple", true));
        let (name, exact) = nearest_name([250, 5, 3]);
        assert_eq!((name, exact), ("red", false));
        assert_eq!(nearest_name([0, 255, 255]).0, "aqua", "the first of two names for a colour is kept");
        assert_eq!(write_web([254, 0, 0], 255, Form::Named), "red");
    }

    #[test]
    fn the_amigas_colours_are_four_bits_and_written_as_0rgb() {
        assert_eq!(snap(Machine::Amiga, [255, 255, 255]), Snapped { value: 0xFFF, rgb: [255; 3] });
        assert_eq!(snap(Machine::Amiga, [0x12, 0x99, 0xEE]).value, 0x19E);
        assert_eq!(snap(Machine::Amiga, [0x12, 0x99, 0xEE]).rgb, [0x11, 0x99, 0xEE]);
        assert_eq!(write([0x11, 0x99, 0xEE], 255, Notation::Machine(Machine::Amiga, Style::Dollar(4))), "$019E");
        assert_eq!(decode(Machine::Amiga, 0x019E), Some([0x11, 0x99, 0xEE]));
        // AGA: eight bits a channel, written as the high nibbles then the low ones.
        let aga = snap(Machine::AmigaAga, [0x12, 0xAB, 0xEF]);
        assert_eq!(aga.rgb, [0x12, 0xAB, 0xEF]);
        assert_eq!(write_number(Machine::AmigaAga, aga.value, Style::Dollar(4)), "$01AE");
        assert_eq!(write_number(Machine::AmigaAga, aga.value, Style::AgaLow), "$02BF");
        assert_eq!(decode(Machine::AmigaAga, 0x12ABEF), Some([0x12, 0xAB, 0xEF]));
        // And the OCS value the picked colour would be on the older chips.
        assert_eq!(snap(Machine::Amiga, [0x12, 0xAB, 0xEF]).value, 0x1AE);
    }

    #[test]
    fn every_machines_own_colours_snap_to_themselves() {
        for m in Machine::ALL {
            let colours = palette(m);
            for (i, c) in colours.iter().enumerate() {
                let s = snap(m, *c);
                assert_eq!(s.rgb, *c, "{m:?} colour {i}");
                assert_eq!(decode(m, s.value), Some(*c), "{m:?} colour {i} by its number");
            }
        }
        assert_eq!(palette(Machine::Spectrum).len(), 16);
        assert_eq!(palette(Machine::Cpc).len(), 27);
        assert_eq!(palette(Machine::C64).len(), 16);
        assert_eq!(palette(Machine::Next).len(), 512);
        assert_eq!(palette(Machine::AtariSt).len(), 512);
        assert_eq!(palette(Machine::Atari8).len(), 256);
        assert_eq!(palette(Machine::Atari2600).len(), 128);
        assert_eq!(palette(Machine::Nes).len(), 64);
    }

    #[test]
    fn the_nearest_colour_on_each_machine_is_the_one_a_person_would_choose() {
        // The Spectrum: a bright red is BRIGHT red (index 10, 8 + 2), a dull one the normal red.
        assert_eq!(snap(Machine::Spectrum, [255, 10, 0]).value, 10);
        assert_eq!(snap(Machine::Spectrum, [200, 0, 10]).value, 2);
        // The C64's white and black, and a red nearer Pepto's red than anything.
        assert_eq!(snap(Machine::C64, [250, 250, 250]).value, 1);
        assert_eq!(snap(Machine::C64, [0x68, 0x37, 0x2B]).value, 2);
        // The CPC has 27: full green is firmware 18 (bright green), half red-and-full-blue is the mauve, 5.
        assert_eq!(snap(Machine::Cpc, [0, 255, 0]).value, 18);
        assert_eq!(snap(Machine::Cpc, [0x80, 0, 0xFF]).value, 5);
        // The BBC's eight are the corners of the cube.
        assert_eq!(snap(Machine::Bbc, [250, 240, 10]).value, 3);
        // The Next's 9 bits: white is 511 and writes as the byte $FF.
        let white = snap(Machine::Next, [255; 3]);
        assert_eq!(white.value, 0x1FF);
        assert_eq!(write_number(Machine::Next, white.value, Style::Dollar(2)), "$FF");
        // A colour whose lowest blue bit differs from the default needs the nine-bit number.
        let odd = snap(Machine::Next, crate::next_screen::rgb(0b111_000_001));
        assert_eq!(write_number(Machine::Next, odd.value, Style::Dollar(2)), "$1C1");
        assert_eq!(decode(Machine::Next, 0x1FF), Some([255; 3]));
        let byte = literal_at("nextreg $41,$E3", 15, Context::Machine(Machine::Next)).unwrap();
        assert_eq!(byte.rgb, crate::next_screen::rgb(0x1C7), "the byte's blue bit comes out as the Next has it at reset");
        assert_eq!(literal_at("$1C1", 4, Context::Machine(Machine::Next)).unwrap().rgb, crate::next_screen::rgb(0x1C1));
        // The ST has eight levels a channel and the STE sixteen.
        assert_eq!(snap(Machine::AtariSt, [255, 0, 0]).value, 0x700);
        assert_eq!(snap(Machine::AtariSte, [255, 0, 0]).value, 0xF00);
        assert_eq!(snap(Machine::AtariSte, [0x11, 0, 0]).value, 0x800, "the STE's lowest bit is the top of its nibble");
        assert_eq!(decode(Machine::AtariSt, 0x070), Some([0, 255, 0]));
        // Atari 8-bit colour bytes and the 2600's even values.
        let s = snap(Machine::Atari8, [0, 0, 0]);
        assert_eq!(s.rgb, [0, 0, 0]);
        assert_eq!(snap(Machine::Atari2600, [0, 0, 0]).value % 2, 0);
        // The NES's white is $20 or $30, and the first is the one found.
        assert_eq!(snap(Machine::Nes, [255; 3]).value, 0x20);
    }

    #[test]
    fn machines_codes_are_written_as_their_programs_want_them() {
        let lines = |m, v| code_lines(m, v).into_iter().map(|(_, t)| t).collect::<Vec<_>>();
        assert_eq!(lines(Machine::Amiga, 0x19E)[0], "move.w #$19E,COLOR00");
        assert_eq!(lines(Machine::Amiga, 0x19E)[1], "dc.w $0180,$019E");
        assert_eq!(lines(Machine::Spectrum, 10), ["INK 2: BRIGHT 1", "$42"]);
        // The CPC's firmware 0 (black) is hardware &14; its Gate Array is sent &54.
        assert!(lines(Machine::Cpc, 0)[1].starts_with("&14") && lines(Machine::Cpc, 0)[1].contains("&54"));
        assert_eq!(lines(Machine::Cpc, 26)[2], "INK 0,26");
        // The hardware numbers have no repeats, and each firmware colour is the one the number names.
        let mut seen = CPC_HARDWARE.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 27);
        assert_eq!(lines(Machine::Atari2600, 0x1E)[0], "$1E");
        assert_eq!(lines(Machine::Next, 0x1FF)[2], "$1FF");
    }

    #[test]
    fn the_file_says_which_colours_it_wants() {
        let d = |lang: &str, text: &str| detect(lang, "x", text);
        assert_eq!(d("CSS", ""), Context::Web);
        assert_eq!(d("HTML", "<p style=\"color: red\">"), Context::Web);
        assert_eq!(d("Assembly 680x0", "\tmove.w #$0f00,COLOR00"), Context::Machine(Machine::Amiga));
        assert_eq!(d("Assembly 680x0", "; for the A1200 (AGA)\n\tmove.w #$0f00,COLOR00"), Context::Machine(Machine::AmigaAga));
        assert_eq!(d("Assembly 680x0", "\tmove.w #$777,$ffff8240"), Context::Machine(Machine::AtariSt));
        assert_eq!(d("Assembly 680x0", "; STE\n\tmove.w #$777,$ffff8240"), Context::Machine(Machine::AtariSte));
        assert_eq!(d("BASIC (AMOS)", ""), Context::Machine(Machine::Amiga));
        assert_eq!(d("BASIC (Blitz Basic)", ""), Context::Machine(Machine::Amiga));
        assert_eq!(d("C", "#include <proto/exec.h>"), Context::Machine(Machine::Amiga));
        assert_eq!(d("C", "#include <stdio.h>"), Context::Web);
        assert_eq!(d("Assembly Z80", "ld a,2"), Context::Machine(Machine::Spectrum));
        assert_eq!(d("Assembly Z80", "nextreg $40,1"), Context::Machine(Machine::Next));
        assert_eq!(d("Assembly Z80", "; Amstrad CPC\nld bc,$7f00"), Context::Machine(Machine::Cpc));
        assert_eq!(d("BASIC (Sinclair and Next)", "10 INK 2"), Context::Machine(Machine::Spectrum));
        assert_eq!(d("BASIC (Commodore)", "10 POKE 53281,0"), Context::Machine(Machine::C64));
        assert_eq!(d("Assembly 6502", "lda #0\nsta $d020"), Context::Machine(Machine::C64));
        assert_eq!(d("Assembly 6502", "sta COLUBK\nsta WSYNC"), Context::Machine(Machine::Atari2600));
        assert_eq!(d("Assembly 6502", "sta COLPF2"), Context::Machine(Machine::Atari8));
        assert_eq!(d("Assembly 6502", "sta $2006"), Context::Machine(Machine::Nes));
        assert_eq!(d("Assembly 6502", "jsr oswrch"), Context::Machine(Machine::Bbc));
        assert_eq!(d("Assembly 6502", "lda #1"), Context::Web, "a 6502 program that doesn't say which machine isn't guessed");
    }

    #[test]
    fn the_colour_under_the_cursor_is_found_in_its_form_and_written_back_in_it() {
        let web = Context::Web;
        let line = "  color: rgb(0, 128, 255); background: #fff;";
        let at = literal_at(line, 14, web).unwrap();
        assert_eq!((at.from, at.to, at.rgb), (9, 25, [0, 128, 255]));
        assert_eq!(at.notation, Notation::Web(Form::Rgb));
        assert_eq!(write([255, 0, 0], 255, at.notation), "rgb(255, 0, 0)");
        let hex = literal_at(line, 40, web).unwrap();
        assert_eq!(hex.notation, Notation::Web(Form::Hex3));
        assert_eq!(write([0, 0, 0], 255, hex.notation), "#000");
        // Replacing it in the line, by character.
        let replaced: String = line.chars().take(at.from).chain(write([255, 0, 0], 255, at.notation).chars()).chain(line.chars().skip(at.to)).collect();
        assert_eq!(replaced, "  color: rgb(255, 0, 0); background: #fff;");
        // A name, only in a web file.
        let named_at = literal_at("border: 1px solid tomato;", 20, web).unwrap();
        assert_eq!((named_at.from, named_at.to, named_at.rgb), (18, 24, [255, 99, 71]));
        assert_eq!(write([0, 0, 255], 255, named_at.notation), "blue");
        assert!(literal_at("border: 1px solid tomato;", 20, Context::Machine(Machine::C64)).is_none());
        assert!(literal_at("margin: 0;", 3, web).is_none());
        // The Amiga's $0RGB, in a copper list and a move, and its own form written back; a three-digit number isn't one.
        let amiga = Context::Machine(Machine::Amiga);
        let l = literal_at("\tdc.w $0180,$0f80", 14, amiga).unwrap();
        assert_eq!((l.from, l.to, l.rgb), (12, 17, [255, 136, 0]));
        assert_eq!(write([0, 0, 0], 255, l.notation), "$0000");
        assert!(literal_at("\tdc.w $0180,$0f80", 8, amiga).is_some(), "$0180 is four digits too: it's the register, but the line can't say so");
        assert!(literal_at("\tmove.l #$12345678,d0", 12, amiga).is_none());
        let m = literal_at("\tmove.w #$0abc,COLOR03", 12, amiga).unwrap();
        assert_eq!(m.rgb, [0xAA, 0xBB, 0xCC]);
        // The C64's number, in the style it was written.
        let c64 = Context::Machine(Machine::C64);
        let n = literal_at("lda #6", 6, c64).unwrap();
        assert_eq!((n.from, n.to, n.rgb), (5, 6, palette(Machine::C64)[6]));
        assert_eq!(write(palette(Machine::C64)[2], 255, n.notation), "2");
        assert!(literal_at("lda #200", 7, c64).is_none(), "200 is no C64 colour");
        let h = literal_at("sta $0e", 6, c64).unwrap();
        assert_eq!(write(palette(Machine::C64)[1], 255, h.notation), "$01");
        // The NES's byte, in hex; the CPC's with an ampersand.
        let nes = literal_at("lda #$21", 8, Context::Machine(Machine::Nes)).unwrap();
        assert_eq!(write([255; 3], 255, nes.notation), "$20");
        let cpc = literal_at("INK 1,&1A", 8, Context::Machine(Machine::Cpc)).unwrap();
        assert_eq!(cpc.notation, Notation::Machine(Machine::Cpc, Style::Ampersand(2)));
        assert_eq!(write(palette(Machine::Cpc)[26], 255, cpc.notation), "&1A");
        // A swatch's own scanner finds a $0RGB beside a colour register in any file, and it's the Amiga's.
        let copper = literal_at("COLOR00 $0f00", 10, web).unwrap();
        assert_eq!(copper.notation, Notation::Machine(Machine::Amiga, Style::Dollar(4)));
    }

    /// A PNG of 3 by 2 pixels, made here, read back as pixels to pick from.
    #[test]
    fn a_picture_of_the_screen_is_read_and_a_pixel_picked_from_it() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 3, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
        }
        let shot = decode_png(&bytes).unwrap();
        assert_eq!((shot.width, shot.height), (3, 2));
        assert_eq!(shot.pixel(2, 0), Some([0, 0, 255]));
        assert_eq!(shot.pixel(1, 1), Some([4, 5, 6]));
        assert_eq!(shot.pixel(3, 0), None);
        assert!(decode_png(b"not a picture").is_err());
        // The picked pixel then goes into the contexts' forms.
        assert_eq!(write(shot.pixel(0, 0).unwrap(), 255, Notation::Web(Form::Hex6)), "#FF0000");
        assert_eq!(snap(Machine::Amiga, shot.pixel(1, 0).unwrap()).value, 0x0F0);
    }

    /// The programs tried to take a picture are only listed here: running one would flash a window on a desktop.
    #[test]
    fn the_screen_grabbing_programs_are_named_without_being_run() {
        let list = grab_programs("/tmp/x.png");
        assert!(!list.is_empty());
        assert!(list.iter().all(|(_, args)| args.iter().any(|a| a.contains("x.png"))));
    }
}
