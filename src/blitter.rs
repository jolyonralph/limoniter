//! The Amiga blitter's minterms, worked out: a logic expression over its three sources A, B and C (written as C
//! would, `A&B | ~A&C`, or in the classic notation, `AB + aC`, where a lower-case letter is NOT) becomes the
//! eight-bit function the blitter calls LF, and from it, the shifts and the channels in use, a BLTCON0 value.
//! It works the other way too: a BLTCON0 and BLTCON1 taken apart into their fields.
//!
//! LF has a bit for each of the eight ways A, B and C can be: bit 7 when all three are set (ABC), down to bit 0
//! when none is (abc), the bit's number being A×4 + B×2 + C. So A on its own is $F0, B $CC and C $AA, and any
//! expression's LF is that expression worked out bitwise on those three bytes.

use eframe::egui::{self, RichText, Ui};

/// LF for the sources on their own.
pub const A: u8 = 0xF0;
pub const B: u8 = 0xCC;
pub const C: u8 = 0xAA;

/// The minterms' names, from bit 7 (ABC) down to bit 0 (abc).
pub fn minterm_name(bit: u8) -> String {
    ["A", "B", "C"].iter().enumerate().map(|(i, letter)| if bit & (4 >> i) != 0 { letter.to_string() } else { letter.to_ascii_lowercase() }).collect()
}

/// Functions blitter code uses most.
pub const COMMON: &[(&str, u8)] = &[
    ("Copy A (D = A)", 0xF0),
    ("Copy B (D = B)", 0xCC),
    ("Copy C (D = C)", 0xAA),
    ("Cookie-cut (D = AB + aC)", 0xCA),
    ("Clear (D = 0)", 0x00),
    ("Set every bit (D = 1)", 0xFF),
    ("Invert A (D = a)", 0x0F),
    ("Invert C (D = c)", 0x55),
    ("A OR C", 0xFA),
    ("A XOR C (line drawing)", 0x5A),
    ("A AND C", 0xA0),
    ("Clear C where A is set (D = aC)", 0x0A),
];

/// LF from an expression. Letters A, B and C are the sources and a, b and c their opposites; NOT is `~`, `!` or
/// a `'` after; AND is `&`, `*`, `.` or nothing (`AB`); OR is `|` or `+`; XOR is `^`; `0` and `1` are constants.
/// NOT binds tightest, then AND, then XOR, then OR. A leading `D =` is allowed.
pub fn parse(text: &str) -> Result<u8, String> {
    let mut body = text.trim();
    if let Some(rest) = body.strip_prefix(['D', 'd']).map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
        body = rest;
    }
    let mut parser = Parser { chars: body.chars().filter(|c| !c.is_whitespace()).collect(), at: 0 };
    if parser.chars.is_empty() {
        return Err("Type an expression over A, B and C, such as AB + aC.".into());
    }
    let value = parser.or()?;
    match parser.peek() {
        None => Ok(value),
        Some(')') => Err("There's a ) without its (.".into()),
        Some(c) => Err(format!("'{c}' isn't understood there.")),
    }
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    /// Takes one of the characters given (and a doubled one, as C's && and ||).
    fn take(&mut self, any: &[char]) -> bool {
        match self.peek() {
            Some(c) if any.contains(&c) => {
                self.at += 1;
                if matches!(c, '&' | '|') && self.peek() == Some(c) {
                    self.at += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn or(&mut self) -> Result<u8, String> {
        let mut value = self.xor()?;
        while self.take(&['|', '+']) {
            value |= self.xor()?;
        }
        Ok(value)
    }

    fn xor(&mut self) -> Result<u8, String> {
        let mut value = self.and()?;
        while self.take(&['^']) {
            value ^= self.and()?;
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<u8, String> {
        let mut value = self.not()?;
        loop {
            // An operator, or a source written straight after (AB).
            if self.take(&['&', '*', '.']) || matches!(self.peek(), Some('A' | 'B' | 'C' | 'a' | 'b' | 'c' | '0' | '1' | '(' | '~' | '!')) {
                value &= self.not()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn not(&mut self) -> Result<u8, String> {
        if self.take(&['~', '!']) {
            return Ok(!self.not()?);
        }
        let mut value = self.primary()?;
        while self.take(&['\'']) {
            value = !value;
        }
        Ok(value)
    }

    fn primary(&mut self) -> Result<u8, String> {
        let Some(c) = self.peek() else {
            return Err("The expression stops before it's finished.".into());
        };
        self.at += 1;
        match c {
            'A' => Ok(A),
            'B' => Ok(B),
            'C' => Ok(C),
            'a' => Ok(!A),
            'b' => Ok(!B),
            'c' => Ok(!C),
            '0' => Ok(0),
            '1' => Ok(0xFF),
            '(' => {
                let value = self.or()?;
                if self.take(&[')']) { Ok(value) } else { Err("A ( isn't closed.".into()) }
            }
            ')' => Err("There's a ) where a source should be.".into()),
            other => Err(format!("'{other}' isn't a source: the blitter's are A, B and C (a, b and c for NOT).")),
        }
    }
}

/// A product of sources: for each of A, B and C, whether it must be set, clear, or either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cube {
    /// Bit 2 for A, 1 for B, 0 for C: set where the source appears.
    care: u8,
    /// Where it appears, whether it's set.
    value: u8,
}

impl Cube {
    /// The minterms it covers, as LF bits.
    fn covers(self) -> u8 {
        (0..8u8).filter(|row| row & self.care == self.value).fold(0, |lf, row| lf | 1 << row)
    }

    fn literals(self) -> u32 {
        self.care.count_ones()
    }
}

/// The shortest sum of products giving LF: the fewest terms, then the fewest letters.
fn cover(lf: u8) -> Vec<Cube> {
    let cubes: Vec<Cube> = (0..8u8).flat_map(|care| (0..8u8).filter(move |value| value & !care == 0).map(move |value| Cube { care, value })).collect();
    let implicants: Vec<Cube> = cubes.iter().copied().filter(|c| c.covers() & !lf == 0).collect();
    // Prime: no bigger implicant holds it.
    let primes: Vec<Cube> = implicants.iter().copied().filter(|c| !implicants.iter().any(|d| d != c && d.covers() & c.covers() == c.covers())).collect();
    let mut best: Option<(u32, u32, Vec<Cube>)> = None;
    for set in 0u32..1 << primes.len() {
        let chosen: Vec<Cube> = primes.iter().enumerate().filter(|(i, _)| set & 1 << i != 0).map(|(_, c)| *c).collect();
        if chosen.iter().fold(0, |m, c| m | c.covers()) != lf {
            continue;
        }
        let (terms, letters) = (chosen.len() as u32, chosen.iter().map(|c| c.literals()).sum());
        if best.as_ref().is_none_or(|(t, l, _)| (terms, letters) < (*t, *l)) {
            best = Some((terms, letters, chosen));
        }
    }
    let mut terms = best.map(|(_, _, c)| c).unwrap_or_default();
    // The term holding the highest minterm first, so cookie-cut reads AB + aC.
    terms.sort_by_key(|c| std::cmp::Reverse((7 - c.covers().leading_zeros(), c.covers())));
    terms
}

/// The letters of a product: A, B, C as they must be (lower case for NOT), or in C's way with ~ and &.
fn product(cube: Cube, classic: bool) -> String {
    let mut parts = Vec::new();
    for (i, letter) in ['A', 'B', 'C'].into_iter().enumerate() {
        let bit = 4 >> i;
        if cube.care & bit == 0 {
            continue;
        }
        let set = cube.value & bit != 0;
        parts.push(match (classic, set) {
            (true, true) => letter.to_string(),
            (true, false) => letter.to_ascii_lowercase().to_string(),
            (false, true) => letter.to_string(),
            (false, false) => format!("~{letter}"),
        });
    }
    if parts.is_empty() { "1".into() } else { parts.join(if classic { "" } else { "&" }) }
}

/// LF as the shortest expression in the classic notation: AB + aC.
pub fn classic(lf: u8) -> String {
    if lf == 0 {
        return "0".into();
    }
    cover(lf).into_iter().map(|c| product(c, true)).collect::<Vec<_>>().join(" + ")
}

/// LF as the shortest expression in C's notation: A&B | ~A&C.
pub fn c_style(lf: u8) -> String {
    if lf == 0 {
        return "0".into();
    }
    cover(lf).into_iter().map(|c| product(c, false)).collect::<Vec<_>>().join(" | ")
}

/// LF as every minterm it has, in full: ABC + ABc + ...
pub fn minterms(lf: u8) -> String {
    if lf == 0 {
        return "0".into();
    }
    (0..8u8).rev().filter(|bit| lf & 1 << bit != 0).map(minterm_name).collect::<Vec<_>>().join(" + ")
}

/// Whether the function depends on a source (0 for A, 1 for B, 2 for C): if not, the channel needn't be on.
pub fn uses(lf: u8, source: usize) -> bool {
    let bit = 4u8 >> source;
    (0..8u8).filter(|row| row & bit == 0).any(|row| (lf >> row) & 1 != (lf >> (row | bit)) & 1)
}

/// The sources a function reads, as letters ("A, C"), or "none".
pub fn sources_read(lf: u8) -> String {
    let used: Vec<&str> = ["A", "B", "C"].iter().enumerate().filter(|(i, _)| uses(lf, *i)).map(|(_, l)| *l).collect();
    if used.is_empty() { "none".into() } else { used.join(", ") }
}

/// BLTCON0 from its fields: A's shift in bits 15 to 12, USEA to USED in 11 to 8, LF in 7 to 0.
pub fn bltcon0(a_shift: u8, channels: [bool; 4], lf: u8) -> u16 {
    let mut value = u16::from(a_shift & 15) << 12 | u16::from(lf);
    for (i, on) in channels.iter().enumerate() {
        if *on {
            value |= 0x800 >> i;
        }
    }
    value
}

/// How area fill is set in BLTCON1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fill {
    #[default]
    None,
    Inclusive,
    Exclusive,
}

/// BLTCON1 for an area (not line) blit: B's shift in bits 15 to 12, exclusive fill bit 4, inclusive fill bit 3,
/// fill carry in bit 2, descending bit 1.
pub fn bltcon1(b_shift: u8, fill: Fill, carry_in: bool, descending: bool) -> u16 {
    let mut value = u16::from(b_shift & 15) << 12;
    value |= match fill {
        Fill::None => 0,
        Fill::Inclusive => 0x08,
        Fill::Exclusive => 0x10,
    };
    if carry_in {
        value |= 0x04;
    }
    if descending {
        value |= 0x02;
    }
    value
}

/// A BLTCON0 and BLTCON1 taken apart, a line to each field.
pub fn decode(con0: u16, con1: u16) -> Vec<String> {
    let lf = con0 as u8;
    let on = |bit: u16| con0 & bit != 0;
    let channels: Vec<&str> = [(0x800, "A"), (0x400, "B"), (0x200, "C"), (0x100, "D")].iter().filter(|(bit, _)| on(*bit)).map(|(_, l)| *l).collect();
    let channels = if channels.is_empty() { "none".to_string() } else { channels.join(", ") };
    let line = con1 & 1 != 0;
    let mut out = vec![format!("BLTCON0 ${con0:04X}")];
    if line {
        out.push(format!("  Start pixel (bits 15\u{2013}12): {}", con0 >> 12));
    } else {
        out.push(format!("  A shift (bits 15\u{2013}12): {}", con0 >> 12));
    }
    out.push(format!("  Channels on (USEA\u{2013}USED, bits 11\u{2013}8): {channels}"));
    out.push(format!("  LF (bits 7\u{2013}0): ${lf:02X}, D = {}  (C style: {})", classic(lf), c_style(lf)));
    out.push(format!("  The function reads: {}", sources_read(lf)));
    for (i, letter) in ["A", "B", "C"].iter().enumerate() {
        if uses(lf, i) && !on(0x800 >> i) {
            out.push(format!("  Note: the function reads {letter} but USE{letter} is off, so {letter} comes from its data register."));
        }
    }
    out.push(format!("BLTCON1 ${con1:04X}"));
    if line {
        out.push("  Line mode (bit 0, LINE): on".into());
        out.push(format!("  Texture start (bits 15\u{2013}12): {}", con1 >> 12));
        let bit = |n: u16| if con1 & 1 << n != 0 { 1 } else { 0 };
        out.push(format!("  SIGN (bit 6): {}", bit(6)));
        out.push(format!("  SUD, SUL, AUL (bits 4, 3, 2), the octant: {}, {}, {}", bit(4), bit(3), bit(2)));
        out.push(format!("  SING, one dot a line, for filling (bit 1): {}", bit(1)));
    } else {
        out.push("  Line mode (bit 0, LINE): off, an area blit".into());
        out.push(format!("  B shift (bits 15\u{2013}12): {}", con1 >> 12));
        let fill = match (con1 & 0x10 != 0, con1 & 0x08 != 0) {
            (false, false) => "none",
            (false, true) => "inclusive (IFE, bit 3)",
            (true, false) => "exclusive (EFE, bit 4)",
            (true, true) => "both IFE and EFE set, which the blitter doesn't expect",
        };
        out.push(format!("  Fill: {fill}"));
        out.push(format!("  Fill carry in (FCI, bit 2): {}", if con1 & 4 != 0 { "1" } else { "0" }));
        out.push(format!("  Descending (DESC, bit 1): {}", if con1 & 2 != 0 { "yes, addresses go down" } else { "no, addresses go up" }));
        if con1 & 0x0FE0 != 0 {
            out.push(format!("  Bits 11\u{2013}5 should be 0 but are ${:03X}", (con1 >> 5) & 0x7F));
        }
    }
    out
}

/// A number typed as $1234, 0x1234, %0101 or 1234 (hex when it has a $ or 0x; decimal otherwise).
pub fn read_number(text: &str) -> Option<u16> {
    let t = text.trim().trim_start_matches('#');
    if let Some(hex) = t.strip_prefix('$').or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = t.strip_prefix('%') {
        u16::from_str_radix(bin, 2).ok()
    } else {
        t.parse().ok()
    }
}

/// How the values go into the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    /// Two move.w, to BLTCON0 and BLTCON1.
    #[default]
    MoveWords,
    /// One move.l to BLTCON0, filling BLTCON1 too.
    MoveLong,
    /// dc.w, for a table.
    Words,
}

/// The lines put at the cursor.
pub fn source_lines(form: Form, con0: u16, con1: u16, register: &str) -> String {
    let lf = con0 as u8;
    let note = format!("; D = {}", classic(lf));
    match form {
        Form::MoveWords => format!("        move.w  #${con0:04X},BLTCON0({register})  {note}\n        move.w  #${con1:04X},BLTCON1({register})\n"),
        Form::MoveLong => format!("        move.l  #${con0:04X}{con1:04X},BLTCON0({register})  {note}, BLTCON1 too\n"),
        Form::Words => format!("        dc.w    ${con0:04X},${con1:04X}  {note}\n"),
    }
}

/// The calculator's window.
pub struct Blitter {
    pub expression: String,
    pub lf: u8,
    problem: Option<String>,
    pub a_shift: u8,
    pub b_shift: u8,
    /// USEA to USED.
    pub channels: [bool; 4],
    /// Channels follow the function (and D is on).
    pub automatic: bool,
    pub fill: Fill,
    pub carry_in: bool,
    pub descending: bool,
    pub form: Form,
    pub register: String,
    con0_text: String,
    con1_text: String,
}

impl Default for Blitter {
    fn default() -> Self {
        let mut blitter = Blitter {
            expression: "AB + aC".into(),
            lf: 0xCA,
            problem: None,
            a_shift: 0,
            b_shift: 0,
            channels: [true; 4],
            automatic: true,
            fill: Fill::None,
            carry_in: false,
            descending: false,
            form: Form::MoveWords,
            register: "a6".into(),
            con0_text: "$0FCA".into(),
            con1_text: "$0000".into(),
        };
        blitter.follow_function();
        blitter
    }
}

/// What the window asks the editor for.
pub enum Asked {
    Insert(String),
}

impl Blitter {
    pub fn con0(&self) -> u16 {
        bltcon0(self.a_shift, self.channels, self.lf)
    }

    pub fn con1(&self) -> u16 {
        bltcon1(self.b_shift, self.fill, self.carry_in, self.descending)
    }

    fn follow_function(&mut self) {
        if self.automatic {
            self.channels = [uses(self.lf, 0), uses(self.lf, 1), uses(self.lf, 2), true];
        }
    }

    fn set_lf(&mut self, lf: u8) {
        self.lf = lf;
        self.expression = classic(lf);
        self.problem = None;
        self.follow_function();
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.label(RichText::new("The function").strong());
        ui.horizontal(|ui| {
            ui.label("D =");
            let edit = ui.add(egui::TextEdit::singleline(&mut self.expression).desired_width(220.0).font(egui::TextStyle::Monospace));
            crate::access::called(edit.clone(), "The expression over A, B and C");
            if edit.changed() {
                match parse(&self.expression) {
                    Ok(lf) => {
                        self.lf = lf;
                        self.problem = None;
                        self.follow_function();
                    }
                    Err(problem) => self.problem = Some(problem),
                }
            }
            egui::ComboBox::from_id_salt("blitter-common").selected_text("Common").show_ui(ui, |ui| {
                for (name, lf) in COMMON {
                    if ui.selectable_label(self.lf == *lf, *name).clicked() {
                        self.set_lf(*lf);
                    }
                }
            });
        });
        ui.label(RichText::new("A&B | ~A&C and AB + aC both work: a lower-case letter is NOT; + or | is OR, ^ XOR, & or nothing AND.").small().color(crate::theme::text_dim()));
        if let Some(problem) = &self.problem {
            ui.colored_label(crate::theme::rainbow_mistake(), problem);
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for bit in (0..8u8).rev() {
                let mut on = self.lf & 1 << bit != 0;
                if ui.checkbox(&mut on, RichText::new(minterm_name(bit)).monospace()).on_hover_text(format!("Bit {bit} of LF")).changed() {
                    self.set_lf(self.lf ^ 1 << bit);
                }
            }
        });
        egui::Grid::new("blitter-results").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            ui.label("LF");
            ui.label(RichText::new(format!("${:02X}  ({}, %{:08b})", self.lf, self.lf, self.lf)).monospace().strong());
            ui.end_row();
            ui.label("Shortest");
            ui.label(RichText::new(format!("{}    {}", classic(self.lf), c_style(self.lf))).monospace());
            ui.end_row();
            ui.label("Minterms");
            ui.label(RichText::new(minterms(self.lf)).monospace());
            ui.end_row();
            ui.label("Reads");
            ui.label(sources_read(self.lf));
            ui.end_row();
        });
        ui.separator();
        ui.label(RichText::new("BLTCON0 and BLTCON1").strong());
        ui.horizontal(|ui| {
            ui.label("A shift");
            ui.add(egui::DragValue::new(&mut self.a_shift).range(0..=15));
            ui.label("B shift");
            ui.add(egui::DragValue::new(&mut self.b_shift).range(0..=15));
            ui.checkbox(&mut self.descending, "Descending").on_hover_text("Addresses go down: for overlapping copies upwards, and fills, which work from the end");
        });
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.automatic, "Channels the function reads").on_hover_text("USEA to USEC follow the function, and D is on").changed() {
                self.follow_function();
            }
            ui.add_enabled_ui(!self.automatic, |ui| {
                for (i, name) in ["A", "B", "C", "D"].iter().enumerate() {
                    ui.checkbox(&mut self.channels[i], *name);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Fill");
            ui.selectable_value(&mut self.fill, Fill::None, "None");
            ui.selectable_value(&mut self.fill, Fill::Inclusive, "Inclusive");
            ui.selectable_value(&mut self.fill, Fill::Exclusive, "Exclusive");
            ui.add_enabled(self.fill != Fill::None, egui::Checkbox::new(&mut self.carry_in, "Carry in"));
        });
        let (con0, con1) = (self.con0(), self.con1());
        ui.label(RichText::new(format!("BLTCON0 ${con0:04X}    BLTCON1 ${con1:04X}")).monospace().strong().size(15.0));
        ui.horizontal(|ui| {
            ui.label("Put in as");
            ui.selectable_value(&mut self.form, Form::MoveWords, "move.w").on_hover_text("move.w to each");
            ui.selectable_value(&mut self.form, Form::MoveLong, "move.l").on_hover_text("One move.l, to BLTCON0 and BLTCON1 together");
            ui.selectable_value(&mut self.form, Form::Words, "dc.w");
            ui.label("off");
            ui.add(egui::TextEdit::singleline(&mut self.register).desired_width(28.0)).on_hover_text("The register holding $DFF000");
        });
        ui.label(RichText::new(source_lines(self.form, con0, con1, &self.register).trim_end()).monospace().color(crate::theme::text_dim()));
        ui.horizontal(|ui| {
            if ui.button("Insert at the cursor").clicked() {
                asked = Some(Asked::Insert(source_lines(self.form, con0, con1, &self.register)));
            }
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(source_lines(self.form, con0, con1, &self.register));
            }
            if ui.button("Take apart below").on_hover_text("Shows these values' fields below").clicked() {
                self.con0_text = format!("${con0:04X}");
                self.con1_text = format!("${con1:04X}");
            }
        });
        ui.separator();
        ui.label(RichText::new("Take a value apart").strong());
        ui.horizontal(|ui| {
            ui.label("BLTCON0");
            ui.add(egui::TextEdit::singleline(&mut self.con0_text).desired_width(70.0).font(egui::TextStyle::Monospace));
            ui.label("BLTCON1");
            ui.add(egui::TextEdit::singleline(&mut self.con1_text).desired_width(70.0).font(egui::TextStyle::Monospace));
        });
        match (read_number(&self.con0_text), read_number(&self.con1_text)) {
            (Some(con0), Some(con1)) => {
                for line in decode(con0, con1) {
                    ui.label(RichText::new(line).monospace());
                }
                if ui.button("Use this function above").clicked() {
                    self.set_lf(con0 as u8);
                    self.a_shift = (con0 >> 12) as u8;
                }
            }
            _ => {
                ui.label("Type each as $hex, %binary or decimal, up to $FFFF.");
            }
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An expression as a tree, worked out a row of the truth table at a time: a check on the bytewise parser
    /// that shares nothing with it.
    #[derive(Clone, Debug)]
    enum Tree {
        Source(usize),
        Const(bool),
        Not(Box<Tree>),
        And(Box<Tree>, Box<Tree>),
        Or(Box<Tree>, Box<Tree>),
        Xor(Box<Tree>, Box<Tree>),
    }

    impl Tree {
        fn at(&self, a: bool, b: bool, c: bool) -> bool {
            match self {
                Tree::Source(0) => a,
                Tree::Source(1) => b,
                Tree::Source(_) => c,
                Tree::Const(v) => *v,
                Tree::Not(t) => !t.at(a, b, c),
                Tree::And(x, y) => x.at(a, b, c) && y.at(a, b, c),
                Tree::Or(x, y) => x.at(a, b, c) || y.at(a, b, c),
                Tree::Xor(x, y) => x.at(a, b, c) != y.at(a, b, c),
            }
        }

        fn table(&self) -> u8 {
            (0..8u8).filter(|row| self.at(row & 4 != 0, row & 2 != 0, row & 1 != 0)).fold(0, |lf, row| lf | 1 << row)
        }

        /// Fully bracketed, in C's way or the classic one (NOT on a source is lower case there).
        fn write(&self, classic: bool) -> String {
            match self {
                Tree::Source(i) => ["A", "B", "C"][*i].into(),
                Tree::Const(v) => if *v { "1" } else { "0" }.into(),
                Tree::Not(t) => match (classic, &**t) {
                    (true, Tree::Source(i)) => ["a", "b", "c"][*i].into(),
                    (true, _) => format!("({})'", t.write(classic)),
                    (false, _) => format!("~({})", t.write(classic)),
                },
                Tree::And(x, y) => if classic { format!("({})({})", x.write(classic), y.write(classic)) } else { format!("({} & {})", x.write(classic), y.write(classic)) },
                Tree::Or(x, y) => format!("({} {} {})", x.write(classic), if classic { "+" } else { "|" }, y.write(classic)),
                Tree::Xor(x, y) => format!("({} ^ {})", x.write(classic), y.write(classic)),
            }
        }
    }

    /// Every tree of up to two levels over the sources and the constants: every operator joining every pair.
    fn trees() -> Vec<Tree> {
        let leaves: Vec<Tree> = (0..3).map(Tree::Source).chain([Tree::Const(false), Tree::Const(true)]).collect();
        let grow = |from: &[Tree]| {
            let mut next = from.to_vec();
            next.extend(from.iter().map(|t| Tree::Not(Box::new(t.clone()))));
            for x in from {
                for y in from {
                    next.push(Tree::And(Box::new(x.clone()), Box::new(y.clone())));
                    next.push(Tree::Or(Box::new(x.clone()), Box::new(y.clone())));
                    next.push(Tree::Xor(Box::new(x.clone()), Box::new(y.clone())));
                }
            }
            next
        };
        grow(&grow(&leaves))
    }

    /// Deeper trees, chosen by a fixed sequence of pseudo-random numbers.
    fn random_tree(seed: &mut u32, depth: u32) -> Tree {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        let pick = *seed % if depth == 0 { 5 } else { 9 };
        let mut next = || Box::new(random_tree(seed, depth.saturating_sub(1)));
        match pick {
            0..=2 => Tree::Source(pick as usize),
            3 => Tree::Const(false),
            4 => Tree::Const(true),
            5 => Tree::Not(next()),
            6 => Tree::And(next(), next()),
            7 => Tree::Or(next(), next()),
            _ => Tree::Xor(next(), next()),
        }
    }

    /// LF as a tree: the OR of its minterms, each the AND of the three sources or their opposites.
    fn minterm_tree(lf: u8) -> Tree {
        let literal = |source: usize, set: bool| if set { Tree::Source(source) } else { Tree::Not(Box::new(Tree::Source(source))) };
        (0..8u8)
            .filter(|row| lf & 1 << row != 0)
            .map(|row| Tree::And(Box::new(Tree::And(Box::new(literal(0, row & 4 != 0)), Box::new(literal(1, row & 2 != 0)))), Box::new(literal(2, row & 1 != 0))))
            .fold(Tree::Const(false), |all, term| Tree::Or(Box::new(all), Box::new(term)))
    }

    #[test]
    fn the_sources_alone_are_the_usual_values() {
        assert_eq!(parse("A"), Ok(0xF0));
        assert_eq!(parse("B"), Ok(0xCC));
        assert_eq!(parse("C"), Ok(0xAA));
        assert_eq!(parse("a"), Ok(0x0F));
        assert_eq!(parse("0"), Ok(0x00));
        assert_eq!(parse("1"), Ok(0xFF));
    }

    #[test]
    fn well_known_functions_give_their_values() {
        for (text, lf) in [
            ("AB + aC", 0xCA),
            ("AB+aC", 0xCA),
            ("A&B | ~A&C", 0xCA),
            ("A && B || !A && C", 0xCA),
            ("D = AB + aC", 0xCA),
            ("d=A*B+A'*C", 0xCA),
            ("A.B + a.C", 0xCA),
            ("A|C", 0xFA),
            ("A + C", 0xFA),
            ("A^C", 0x5A),
            ("Ac + aC", 0x5A),
            ("A&C", 0xA0),
            ("aC", 0x0A),
            ("~A&C", 0x0A),
            ("~C", 0x55),
            ("C'", 0x55),
            ("~(A|B|C)", 0x01),
            ("abc", 0x01),
            ("ABC", 0x80),
            ("A(B + C)", 0xE0),
            ("A + BC", 0xF8),
            ("A ^ B ^ C", 0x96),
            ("A | B ^ C", 0xF6),
            ("~~A", 0xF0),
            ("A''", 0xF0),
        ] {
            assert_eq!(parse(text), Ok(lf), "{text}");
        }
    }

    #[test]
    fn precedence_is_not_then_and_then_xor_then_or() {
        // A | B & C is A | (B & C), not (A | B) & C.
        assert_eq!(parse("A | B & C"), parse("A | (B & C)"));
        assert_ne!(parse("A | B & C"), parse("(A | B) & C"));
        // A ^ B & C is A ^ (B & C).
        assert_eq!(parse("A ^ B & C"), parse("A ^ (B & C)"));
        // A | B ^ C is A | (B ^ C).
        assert_eq!(parse("A | B ^ C"), parse("A | (B ^ C)"));
        // ~A & B is (~A) & B.
        assert_eq!(parse("~A & B"), parse("a & B"));
    }

    #[test]
    fn mistakes_are_explained() {
        for bad in ["", "A +", "(A", "A)", "D", "AX", "A ++ B", "A ^^ B", "~"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_function_written_every_way_reads_back_as_itself() {
        for lf in 0..=255u8 {
            for text in [classic(lf), c_style(lf), minterms(lf)] {
                assert_eq!(parse(&text), Ok(lf), "${lf:02X} written as {text}");
            }
            // Each minterm ticked is the bit it names.
            let ticked = (0..8u8).filter(|bit| lf & 1 << bit != 0).fold(0u8, |m, bit| m | parse(&minterm_name(bit)).unwrap());
            assert_eq!(ticked, lf);
        }
    }

    #[test]
    fn every_tree_written_every_way_has_its_truth_table() {
        let mut all = trees();
        let mut seed = 2026;
        all.extend((0..20_000).map(|_| random_tree(&mut seed, 5)));
        all.extend((0..=255u8).map(minterm_tree));
        assert!(all.len() > 40_000, "{}", all.len());
        let mut seen = [false; 256];
        for tree in &all {
            let table = tree.table();
            seen[usize::from(table)] = true;
            for classic in [true, false] {
                let text = tree.write(classic);
                assert_eq!(parse(&text), Ok(table), "{text}");
            }
        }
        // Every one of the 256 functions is among them, each checked against a row-by-row truth table.
        assert!(seen.iter().all(|s| *s), "{} functions reached", seen.iter().filter(|s| **s).count());
        for lf in 0..=255u8 {
            assert_eq!(minterm_tree(lf).table(), lf);
        }
    }

    #[test]
    fn the_shortest_forms_are_shortest() {
        assert_eq!(classic(0xCA), "AB + aC");
        assert_eq!(c_style(0xCA), "A&B | ~A&C");
        assert_eq!(classic(0xF0), "A");
        assert_eq!(classic(0x0F), "a");
        assert_eq!(classic(0xFA), "A + C");
        assert_eq!(classic(0x5A), "Ac + aC");
        assert_eq!(classic(0xFF), "1");
        assert_eq!(classic(0x00), "0");
        assert_eq!(classic(0x80), "ABC");
        // No function of three sources needs more than four terms (the odd and even parity functions do).
        for lf in 0..=255u8 {
            let terms = if lf == 0 { 0 } else { classic(lf).split(" + ").count() };
            assert!(terms <= 4, "${lf:02X}: {}", classic(lf));
        }
        assert_eq!(classic(0x96).split(" + ").count(), 4);
        // And no shorter cover exists: checked against every set of up to four products, by brute force.
        let products: Vec<u8> = (0..27u32)
            .map(|n| {
                let pick = [n % 3, n / 3 % 3, n / 9];
                [A, B, C].iter().zip(pick).fold(0xFF, |m, (s, p)| match p {
                    0 => m,
                    1 => m & s,
                    _ => m & !s,
                })
            })
            .collect();
        let mut fewest = [u8::MAX; 256];
        fewest[0] = 0;
        for &x in &products {
            fewest[usize::from(x)] = fewest[usize::from(x)].min(1);
            for &y in &products {
                fewest[usize::from(x | y)] = fewest[usize::from(x | y)].min(2);
                for &z in &products {
                    fewest[usize::from(x | y | z)] = fewest[usize::from(x | y | z)].min(3);
                    for &w in &products {
                        fewest[usize::from(x | y | z | w)] = fewest[usize::from(x | y | z | w)].min(4);
                    }
                }
            }
        }
        for lf in 1..=255u8 {
            assert_eq!(classic(lf).split(" + ").count(), usize::from(fewest[usize::from(lf)]), "${lf:02X}: {}", classic(lf));
        }
    }

    #[test]
    fn channels_follow_what_the_function_reads() {
        assert_eq!(sources_read(0xCA), "A, B, C");
        assert_eq!(sources_read(0xF0), "A");
        assert_eq!(sources_read(0xFA), "A, C");
        assert_eq!(sources_read(0x00), "none");
        assert_eq!(sources_read(0xFF), "none");
        // Against the tree: a source is read exactly when flipping it can change the result.
        for lf in 0..=255u8 {
            for source in 0..3 {
                let flips = (0..8u8).any(|row| (lf >> row) & 1 != (lf >> (row ^ (4 >> source))) & 1);
                assert_eq!(uses(lf, source), flips);
            }
        }
    }

    #[test]
    fn bltcon_values_are_put_together_and_taken_apart() {
        // A cookie-cut bob with A shifted by 4, every channel on.
        assert_eq!(bltcon0(4, [true; 4], 0xCA), 0x4FCA);
        // A plain copy A to D.
        assert_eq!(bltcon0(0, [true, false, false, true], 0xF0), 0x09F0);
        // A clear: only D.
        assert_eq!(bltcon0(0, [false, false, false, true], 0x00), 0x0100);
        assert_eq!(bltcon1(4, Fill::None, false, false), 0x4000);
        assert_eq!(bltcon1(0, Fill::Inclusive, false, true), 0x000A);
        assert_eq!(bltcon1(0, Fill::Exclusive, true, true), 0x0016);
        let lines = decode(0x4FCA, 0x4002).join("\n");
        assert!(lines.contains("A shift (bits 15\u{2013}12): 4"), "{lines}");
        assert!(lines.contains("A, B, C, D"), "{lines}");
        assert!(lines.contains("D = AB + aC"), "{lines}");
        assert!(lines.contains("B shift (bits 15\u{2013}12): 4"), "{lines}");
        assert!(lines.contains("Descending (DESC, bit 1): yes"), "{lines}");
        let line = decode(0xBB4A, 0x0051).join("\n");
        assert!(line.contains("Line mode (bit 0, LINE): on"), "{line}");
        assert!(line.contains("Start pixel (bits 15\u{2013}12): 11"), "{line}");
        assert!(line.contains("SIGN (bit 6): 1"), "{line}");
        assert!(line.contains("SUD, SUL, AUL (bits 4, 3, 2), the octant: 1, 0, 0"), "{line}");
        let warned = decode(0x01F0, 0).join("\n");
        assert!(warned.contains("USEA is off"), "{warned}");
        assert_eq!(read_number("$0FCA"), Some(0x0FCA));
        assert_eq!(read_number("0x0fca"), Some(0x0FCA));
        assert_eq!(read_number("%1010"), Some(10));
        assert_eq!(read_number("4042"), Some(4042));
        assert_eq!(read_number("$10000"), None);
    }

    #[test]
    fn source_is_written_three_ways() {
        assert_eq!(source_lines(Form::MoveWords, 0x0FCA, 0x0000, "a6"), "        move.w  #$0FCA,BLTCON0(a6)  ; D = AB + aC\n        move.w  #$0000,BLTCON1(a6)\n");
        assert_eq!(source_lines(Form::MoveLong, 0x09F0, 0x0000, "a5"), "        move.l  #$09F00000,BLTCON0(a5)  ; D = A, BLTCON1 too\n");
        assert_eq!(source_lines(Form::Words, 0x0100, 0x0002, "a6"), "        dc.w    $0100,$0002  ; D = 0\n");
    }

    #[test]
    fn the_window_starts_on_cookie_cut() {
        let blitter = Blitter::default();
        assert_eq!(blitter.con0(), 0x0FCA);
        assert_eq!(blitter.con1(), 0);
    }
}
