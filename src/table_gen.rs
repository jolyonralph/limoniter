//! Tables for demo code, worked out ahead so the 68000 (or Z80, or 6502) only looks them up: sine and cosine,
//! a straight ramp and easing curves, multiplication and division, perspective (1/z), and pseudo-random
//! numbers from a seed. They go into the source in the form the assembler or language in hand reads (68000 dc
//! lines, Z80 or x86 db, 6502 !byte or .byte, 6809 fcb, BASIC DATA lines, or a C, PHP, Python, JavaScript or Rust
//! array), or out as a binary file.
//!
//! How values are rounded: each is worked out as a real number, multiplied by 2 to the power of the fraction
//! bits, then rounded to the nearest whole number, halves away from zero (2.5 to 3, -2.5 to -3). A value that
//! still doesn't fit the size chosen is held at the nearest end of its range, and the window says how many
//! were. Dividing by zero gives the largest value the size holds.

use eframe::egui::{self, Color32, RichText, Sense, Ui, pos2, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Sine,
    Cosine,
    Ramp,
    Easing,
    Multiply,
    Divide,
    Perspective,
    Random,
}

impl Kind {
    pub const ALL: [Kind; 8] = [Kind::Sine, Kind::Cosine, Kind::Ramp, Kind::Easing, Kind::Multiply, Kind::Divide, Kind::Perspective, Kind::Random];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Sine => "Sine",
            Kind::Cosine => "Cosine",
            Kind::Ramp => "Straight ramp",
            Kind::Easing => "Easing curve",
            Kind::Multiply => "Multiplication (n \u{D7} factor)",
            Kind::Divide => "Division (number \u{F7} n)",
            Kind::Perspective => "Perspective (scale \u{F7} z)",
            Kind::Random => "Random",
        }
    }

    /// The label the table is given in the source.
    fn label(self) -> &'static str {
        match self {
            Kind::Sine => "sine",
            Kind::Cosine => "cosine",
            Kind::Ramp => "ramp",
            Kind::Easing => "ease",
            Kind::Multiply => "times",
            Kind::Divide => "divide",
            Kind::Perspective => "perspective",
            Kind::Random => "random",
        }
    }
}

/// How an easing curve goes from 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Curve {
    #[default]
    InOutSine,
    InSine,
    OutSine,
    InQuad,
    OutQuad,
    InOutQuad,
    InCubic,
    OutCubic,
    InOutCubic,
    Smoothstep,
    OutBounce,
}

impl Curve {
    pub const ALL: [Curve; 11] = [Curve::InOutSine, Curve::InSine, Curve::OutSine, Curve::InQuad, Curve::OutQuad, Curve::InOutQuad, Curve::InCubic, Curve::OutCubic, Curve::InOutCubic, Curve::Smoothstep, Curve::OutBounce];

    pub fn name(self) -> &'static str {
        match self {
            Curve::InOutSine => "Sine, in and out",
            Curve::InSine => "Sine, in",
            Curve::OutSine => "Sine, out",
            Curve::InQuad => "Square, in",
            Curve::OutQuad => "Square, out",
            Curve::InOutQuad => "Square, in and out",
            Curve::InCubic => "Cube, in",
            Curve::OutCubic => "Cube, out",
            Curve::InOutCubic => "Cube, in and out",
            Curve::Smoothstep => "Smoothstep",
            Curve::OutBounce => "Bounce, out",
        }
    }

    /// The curve at t, from 0 to 1: 0 at 0 and 1 at 1. "In" starts slowly, "out" ends slowly.
    pub fn at(self, t: f64) -> f64 {
        use std::f64::consts::PI;
        match self {
            Curve::InOutSine => (1.0 - (PI * t).cos()) / 2.0,
            Curve::InSine => 1.0 - (PI * t / 2.0).cos(),
            Curve::OutSine => (PI * t / 2.0).sin(),
            Curve::InQuad => t * t,
            Curve::OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Curve::InOutQuad => {
                if t < 0.5 { 2.0 * t * t } else { 1.0 - 2.0 * (1.0 - t) * (1.0 - t) }
            }
            Curve::InCubic => t * t * t,
            Curve::OutCubic => 1.0 - (1.0 - t).powi(3),
            Curve::InOutCubic => {
                if t < 0.5 { 4.0 * t * t * t } else { 1.0 - 4.0 * (1.0 - t).powi(3) }
            }
            Curve::Smoothstep => t * t * (3.0 - 2.0 * t),
            Curve::OutBounce => {
                // Four arcs of falling height, as a ball dropped onto the floor.
                let (n, d) = (7.5625, 2.75);
                if t < 1.0 / d {
                    n * t * t
                } else if t < 2.0 / d {
                    let t = t - 1.5 / d;
                    n * t * t + 0.75
                } else if t < 2.5 / d {
                    let t = t - 2.25 / d;
                    n * t * t + 0.9375
                } else {
                    let t = t - 2.625 / d;
                    n * t * t + 0.984375
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    Byte,
    #[default]
    Word,
    Long,
}

impl Size {
    pub fn bytes(self) -> usize {
        match self {
            Size::Byte => 1,
            Size::Word => 2,
            Size::Long => 4,
        }
    }

    /// The smallest and largest value a size holds, signed or not.
    pub fn range(self, signed: bool) -> (i64, i64) {
        let bits = self.bytes() as u32 * 8;
        if signed { (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1) } else { (0, (1i64 << bits) - 1) }
    }

    fn c_type(self, signed: bool) -> &'static str {
        match (self, signed) {
            (Size::Byte, true) => "int8_t",
            (Size::Byte, false) => "uint8_t",
            (Size::Word, true) => "int16_t",
            (Size::Word, false) => "uint16_t",
            (Size::Long, true) => "int32_t",
            (Size::Long, false) => "uint32_t",
        }
    }
}

/// Everything a table is made from. Each kind uses some of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub kind: Kind,
    pub length: usize,
    pub size: Size,
    pub signed: bool,
    /// Every value is multiplied by 2 to this power before rounding, for fixed-point numbers.
    pub fraction_bits: u32,
    /// Sine and cosine: offset + amplitude × sin(360° × periods × n / length + phase).
    pub amplitude: f64,
    pub offset: f64,
    pub phase: f64,
    pub periods: f64,
    /// Ramps and easing: from start at the first entry to end at the last.
    pub start: f64,
    pub end: f64,
    pub curve: Curve,
    /// Multiplication and division: n runs from first, one more each entry.
    pub first: i64,
    pub factor: f64,
    pub dividend: f64,
    /// Perspective: scale ÷ (near + n × step).
    pub scale: f64,
    pub near: f64,
    pub step: f64,
    /// Random: whole numbers from low to high, both included.
    pub seed: u32,
    pub low: i64,
    pub high: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            kind: Kind::Sine,
            length: 256,
            size: Size::Word,
            signed: true,
            fraction_bits: 0,
            amplitude: 16384.0,
            offset: 0.0,
            phase: 0.0,
            periods: 1.0,
            start: 0.0,
            end: 255.0,
            curve: Curve::InOutSine,
            first: 0,
            factor: 40.0,
            dividend: 32768.0,
            scale: 16384.0,
            near: 64.0,
            step: 1.0,
            seed: 1,
            low: 0,
            high: 255,
        }
    }
}

/// A table made: its values, and how many had to be held at the ends of the range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub values: Vec<i64>,
    pub held: usize,
}

/// The longest table made.
pub const MOST: usize = 65536;

/// Rounds to the nearest whole number, halves away from zero.
pub fn round(value: f64) -> f64 {
    value.round()
}

/// The pseudo-random numbers: Marsaglia's xorshift with shifts 13, 17 and 5, from the seed (0 is taken as 1, as
/// xorshift would stay at 0 for ever).
pub struct Xorshift(u32);

impl Xorshift {
    pub fn new(seed: u32) -> Self {
        Xorshift(seed.max(1))
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

pub fn generate(settings: &Settings) -> Table {
    use std::f64::consts::PI;
    let s = settings;
    let length = s.length.clamp(1, MOST);
    let (low, high) = s.size.range(s.signed);
    let scale = 2f64.powi(s.fraction_bits.min(31) as i32);
    let mut table = Table::default();
    let mut random = Xorshift::new(s.seed);
    for i in 0..length {
        let n = i as f64;
        // The real value; None for a division by zero.
        let real = match s.kind {
            Kind::Sine | Kind::Cosine => {
                let angle = 2.0 * PI * s.periods * n / length as f64 + s.phase.to_radians();
                Some(s.offset + s.amplitude * if s.kind == Kind::Sine { angle.sin() } else { angle.cos() })
            }
            Kind::Ramp | Kind::Easing => {
                let t = if length > 1 { n / (length - 1) as f64 } else { 0.0 };
                let t = if s.kind == Kind::Easing { s.curve.at(t) } else { t };
                // Multiplying before dividing keeps a whole-number ramp exact.
                Some(if s.kind == Kind::Ramp && length > 1 { s.start + (s.end - s.start) * n / (length - 1) as f64 } else { s.start + (s.end - s.start) * t })
            }
            Kind::Multiply => Some((s.first + i as i64) as f64 * s.factor),
            Kind::Divide => {
                let divisor = (s.first + i as i64) as f64;
                (divisor != 0.0).then(|| s.dividend / divisor)
            }
            Kind::Perspective => {
                let z = s.near + n * s.step;
                (z != 0.0).then(|| s.scale / z)
            }
            Kind::Random => {
                let (from, to) = (s.low.min(s.high), s.low.max(s.high));
                let span = (to - from + 1) as u64;
                // Scaling the 32 bits by the span, rather than taking a remainder, keeps every value equally likely.
                table.values.push((from + ((u64::from(random.next()) * span) >> 32) as i64).clamp(low, high));
                continue;
            }
        };
        let value = match real {
            Some(real) => {
                let rounded = round(real * scale);
                if rounded < low as f64 || rounded > high as f64 || rounded.is_nan() {
                    table.held += 1;
                }
                if rounded.is_nan() { 0 } else { (rounded.clamp(low as f64, high as f64)) as i64 }
            }
            None => high,
        };
        table.values.push(value);
    }
    table
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Base {
    #[default]
    Hex,
    Decimal,
}

/// A value as the assembler would have it: hex in two's complement for its size, decimal with its sign.
fn asm_number(value: i64, size: Size, base: Base) -> String {
    match base {
        Base::Decimal => value.to_string(),
        Base::Hex => {
            let digits = size.bytes() * 2;
            let mask = if size == Size::Long { 0xFFFF_FFFF } else { (1i64 << (digits * 4)) - 1 };
            format!("${:0digits$X}", value & mask)
        }
    }
}

/// As C has it: hex with a minus sign for negative numbers, so the value stays the same in a signed type.
fn c_number(value: i64, size: Size, base: Base) -> String {
    match base {
        Base::Decimal => value.to_string(),
        Base::Hex => {
            let digits = size.bytes() * 2;
            if value < 0 { format!("-0x{:0digits$X}", -value) } else { format!("0x{value:0digits$X}") }
        }
    }
}

/// dc.b, dc.w or dc.l lines, so many values to a line, under a label when there is one.
#[cfg(test)]
pub fn as_source(values: &[i64], size: Size, base: Base, per_line: usize, label: &str) -> String {
    as_directives(values, size, base, per_line, label, Form::Motorola)
}

/// Data lines for an assembler: its own directive for the size, its own way of writing hex, and its own label.
fn as_directives(values: &[i64], size: Size, base: Base, per_line: usize, label: &str, form: Form) -> String {
    let directive = form.directives()[match size {
        Size::Byte => 0,
        Size::Word => 1,
        Size::Long => 2,
    }];
    let mut out = String::new();
    if !label.is_empty() {
        out.push_str(&format!("{label}{}\n", if form.label_colon() { ":" } else { "" }));
    }
    for line in values.chunks(per_line.max(1)) {
        let items: Vec<String> = line
            .iter()
            .map(|v| {
                let number = asm_number(*v, size, base);
                if form == Form::Nasm { number.replacen('$', "0x", 1) } else { number }
            })
            .collect();
        out.push_str(&format!("        {directive:<6}  {}\n", items.join(",")));
    }
    out
}

/// BASIC DATA lines from line 1000 in tens, in decimal, which every BASIC reads, with the label in a REM first.
pub fn as_basic(values: &[i64], per_line: usize, label: &str) -> String {
    let mut out = String::new();
    let mut number = 1000;
    if !label.is_empty() {
        out.push_str(&format!("{number} REM {label}\n"));
        number += 10;
    }
    for line in values.chunks(per_line.max(1)) {
        out.push_str(&format!("{number} DATA {}\n", line.iter().map(i64::to_string).collect::<Vec<_>>().join(",")));
        number += 10;
    }
    out
}

/// An array in a scripting language or Rust: its opening, the values as that language writes numbers, its close.
fn as_array(values: &[i64], size: Size, signed: bool, base: Base, per_line: usize, name: &str, form: Form) -> String {
    let name = if name.is_empty() { "table" } else { name };
    let (open, close) = match form {
        Form::Php => (format!("${name} = ["), "];".to_string()),
        Form::Python => (format!("{name} = ["), "]".to_string()),
        Form::JavaScript => (format!("const {name} = ["), "];".to_string()),
        _ => {
            let kind = match (size, signed) {
                (Size::Byte, true) => "i8",
                (Size::Byte, false) => "u8",
                (Size::Word, true) => "i16",
                (Size::Word, false) => "u16",
                (Size::Long, true) => "i32",
                (Size::Long, false) => "u32",
            };
            (format!("const {}: [{kind}; {}] = [", name.to_uppercase(), values.len()), "];".to_string())
        }
    };
    let lines: Vec<String> = values.chunks(per_line.max(1)).map(|line| format!("    {}", line.iter().map(|v| c_number(*v, size, base)).collect::<Vec<_>>().join(", "))).collect();
    format!("{open}\n{},\n{close}\n", lines.join(",\n"))
}

/// A C array of the stdint.h type for the size.
pub fn as_c(values: &[i64], size: Size, signed: bool, base: Base, per_line: usize, name: &str) -> String {
    let name = if name.is_empty() { "table" } else { name };
    let mut out = format!("const {} {name}[{}] = {{\n", size.c_type(signed), values.len());
    let lines: Vec<String> = values.chunks(per_line.max(1)).map(|line| format!("    {}", line.iter().map(|v| c_number(*v, size, base)).collect::<Vec<_>>().join(", "))).collect();
    out.push_str(&lines.join(",\n"));
    out.push_str("\n};\n");
    out
}

/// The values as bytes: big-endian as the 68000 has them, or little-endian (Z80, 6502, x86).
pub fn as_binary(values: &[i64], size: Size, big_endian: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * size.bytes());
    for value in values {
        let bytes = (*value as u32).to_be_bytes();
        let own = &bytes[4 - size.bytes()..];
        if big_endian {
            out.extend_from_slice(own);
        } else {
            out.extend(own.iter().rev());
        }
    }
    out
}

/// Values written in a form: an assembler's data lines, BASIC DATA lines, or a language's array.
pub fn write(values: &[i64], size: Size, signed: bool, base: Base, per_line: usize, label: &str, form: Form) -> String {
    match form {
        Form::Basic => as_basic(values, per_line, label),
        Form::C => as_c(values, size, signed, base, per_line, label),
        Form::Php | Form::Python | Form::JavaScript | Form::Rust => as_array(values, size, signed, base, per_line, label, form),
        assembler => as_directives(values, size, base, per_line, label, assembler),
    }
}

/// How the table goes into the source: which assembler's data lines, or which language's array.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    /// 68000 assemblers (vasm, Devpac, ASM-One): dc.b, dc.w, dc.l.
    #[default]
    Motorola,
    /// Z80 assemblers (sjasmplus, Pasmo, Limoniter's own): db, dw, dd.
    Z80,
    /// ACME, for the 6502: !byte, !word, !32.
    Acme,
    /// 64tass and ca65, for the 6502: .byte, .word, .dword.
    Dot,
    /// 6809 assemblers (lwasm): fcb, fdb, fqb.
    M6809,
    /// NASM, for x86: db, dw, dd, with hex as 0x.
    Nasm,
    Basic,
    C,
    Php,
    Python,
    JavaScript,
    Rust,
}

impl Form {
    pub const ALL: [Form; 12] = [Form::Motorola, Form::Z80, Form::Acme, Form::Dot, Form::M6809, Form::Nasm, Form::Basic, Form::C, Form::Php, Form::Python, Form::JavaScript, Form::Rust];

    pub fn name(self) -> &'static str {
        match self {
            Form::Motorola => "68000: dc.b, dc.w, dc.l",
            Form::Z80 => "Z80: db, dw, dd",
            Form::Acme => "6502, ACME: !byte, !word",
            Form::Dot => "6502, 64tass or ca65: .byte, .word",
            Form::M6809 => "6809: fcb, fdb",
            Form::Nasm => "x86, NASM: db, dw, dd",
            Form::Basic => "BASIC DATA lines",
            Form::C => "C array",
            Form::Php => "PHP array",
            Form::Python => "Python list",
            Form::JavaScript => "JavaScript array",
            Form::Rust => "Rust array",
        }
    }

    fn directives(self) -> [&'static str; 3] {
        match self {
            Form::Z80 | Form::Nasm => ["db", "dw", "dd"],
            Form::Acme => ["!byte", "!word", "!32"],
            Form::Dot => [".byte", ".word", ".dword"],
            Form::M6809 => ["fcb", "fdb", "fqb"],
            _ => ["dc.b", "dc.w", "dc.l"],
        }
    }

    /// Whether a label is written with a colon; ACME, 64tass and lwasm take a label by its place at the start.
    fn label_colon(self) -> bool {
        !matches!(self, Form::Acme | Form::Dot | Form::M6809)
    }

    /// The one to start with for source in this language, by its highlighting's name.
    pub fn for_language(language: &str) -> Option<Form> {
        Some(match language {
            "Assembly Z80" => Form::Z80,
            "Assembly 6502" => Form::Acme,
            "Assembly 6809" => Form::M6809,
            "Assembly 680x0" => Form::Motorola,
            "Assembly x86_64" | "Assembly 80x86" => Form::Nasm,
            "C" | "C++" => Form::C,
            "PHP" => Form::Php,
            "Python" => Form::Python,
            "JavaScript" | "TypeScript" => Form::JavaScript,
            "Rust" => Form::Rust,
            _ if language.contains("BASIC") || language == "BASIC" => Form::Basic,
            _ => return None,
        })
    }
}

/// The generator's window.
pub struct Generator {
    pub settings: Settings,
    pub base: Base,
    pub per_line: usize,
    pub label: String,
    pub form: Form,
    pub big_endian: bool,
    table: Table,
    made_from: Option<Settings>,
}

impl Default for Generator {
    fn default() -> Self {
        Generator { settings: Settings::default(), base: Base::Hex, per_line: 8, label: String::new(), form: Form::Motorola, big_endian: true, table: Table::default(), made_from: None }
    }
}

/// What the window asks the editor for.
pub enum Asked {
    Insert(String),
    /// The bytes, and a name for the file.
    Save(Vec<u8>, String),
}

impl Generator {
    pub fn table(&mut self) -> &Table {
        if self.made_from.as_ref() != Some(&self.settings) {
            self.table = generate(&self.settings);
            self.made_from = Some(self.settings.clone());
        }
        &self.table
    }

    fn label(&self) -> String {
        if self.label.trim().is_empty() { self.settings.kind.label().to_string() } else { self.label.trim().to_string() }
    }

    pub fn text(&mut self) -> String {
        let (size, signed, base, per_line, form, label) = (self.settings.size, self.settings.signed, self.base, self.per_line, self.form, self.label());
        let values = &self.table().values;
        write(values, size, signed, base, per_line, &label, form)
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let s = &mut self.settings;
        egui::Grid::new("table-settings").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label("Table");
            egui::ComboBox::from_id_salt("table-kind").selected_text(s.kind.name()).show_ui(ui, |ui| {
                for kind in Kind::ALL {
                    ui.selectable_value(&mut s.kind, kind, kind.name());
                }
            });
            ui.end_row();
            ui.label("Entries");
            ui.add(egui::DragValue::new(&mut s.length).range(1..=MOST));
            ui.end_row();
            let number = |ui: &mut Ui, value: &mut f64, label: &str, hint: &str| {
                ui.label(label);
                ui.add(egui::DragValue::new(value).speed(0.1).max_decimals(6)).on_hover_text(hint);
                ui.end_row();
            };
            match s.kind {
                Kind::Sine | Kind::Cosine => {
                    number(ui, &mut s.amplitude, "Amplitude", "The largest the wave goes either side of the offset");
                    number(ui, &mut s.offset, "Offset", "Added to every value: the middle of the wave");
                    number(ui, &mut s.phase, "Phase (degrees)", "Where the wave starts: 90 makes a sine a cosine");
                    number(ui, &mut s.periods, "Waves", "How many whole waves the table holds");
                }
                Kind::Ramp | Kind::Easing => {
                    if s.kind == Kind::Easing {
                        ui.label("Curve");
                        egui::ComboBox::from_id_salt("table-curve").selected_text(s.curve.name()).show_ui(ui, |ui| {
                            for curve in Curve::ALL {
                                ui.selectable_value(&mut s.curve, curve, curve.name());
                            }
                        });
                        ui.end_row();
                    }
                    number(ui, &mut s.start, "From", "The first entry");
                    number(ui, &mut s.end, "To", "The last entry");
                }
                Kind::Multiply | Kind::Divide => {
                    ui.label("n starts at");
                    ui.add(egui::DragValue::new(&mut s.first));
                    ui.end_row();
                    if s.kind == Kind::Multiply {
                        number(ui, &mut s.factor, "Factor", "Each entry is n times this: 40 gives the start of each line of a 320-pixel bitplane");
                    } else {
                        number(ui, &mut s.dividend, "Number", "Each entry is this divided by n; dividing by 0 gives the largest value");
                    }
                }
                Kind::Perspective => {
                    number(ui, &mut s.scale, "Scale", "The distance to the screen times the size: entry = scale \u{F7} z");
                    number(ui, &mut s.near, "z at the first entry", "The nearest distance");
                    number(ui, &mut s.step, "z step", "How much further each entry is");
                }
                Kind::Random => {
                    ui.label("Seed");
                    ui.add(egui::DragValue::new(&mut s.seed)).on_hover_text("The same seed always gives the same table");
                    ui.end_row();
                    ui.label("From");
                    ui.add(egui::DragValue::new(&mut s.low));
                    ui.end_row();
                    ui.label("To");
                    ui.add(egui::DragValue::new(&mut s.high));
                    ui.end_row();
                }
            }
            ui.label("Size");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.size, Size::Byte, "Byte");
                ui.selectable_value(&mut s.size, Size::Word, "Word");
                ui.selectable_value(&mut s.size, Size::Long, "Long");
                ui.checkbox(&mut s.signed, "Signed");
            });
            ui.end_row();
            if s.kind != Kind::Random {
                ui.label("Fraction bits");
                ui.add(egui::DragValue::new(&mut s.fraction_bits).range(0..=30)).on_hover_text("Fixed point: every value is multiplied by 2 to this power before rounding (14 makes 1.0 into 16384)");
                ui.end_row();
            }
        });
        let (low, high) = self.settings.size.range(self.settings.signed);
        let table = self.table().clone();
        self.preview(ui, &table.values, low, high);
        if table.held > 0 {
            ui.colored_label(crate::theme::rainbow_mistake(), format!("{} of the values didn't fit {}\u{2013}{} and were held at the nearer end.", table.held, low, high));
        }
        ui.label(RichText::new("Rounded to the nearest whole number, halves away from zero.").small().color(crate::theme::text_dim()));
        ui.separator();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("table-form").selected_text(self.form.name()).width(230.0).show_ui(ui, |ui| {
                for form in Form::ALL {
                    ui.selectable_value(&mut self.form, form, form.name());
                }
            });
            ui.separator();
            ui.selectable_value(&mut self.base, Base::Hex, "Hex");
            ui.selectable_value(&mut self.base, Base::Decimal, "Decimal");
            ui.separator();
            ui.label("Per line");
            ui.add(egui::DragValue::new(&mut self.per_line).range(1..=64));
        });
        ui.horizontal(|ui| {
            ui.label("Label");
            ui.add(egui::TextEdit::singleline(&mut self.label).hint_text(self.settings.kind.label()).desired_width(120.0));
        });
        let text = self.text();
        let first: String = text.lines().take(3).collect::<Vec<_>>().join("\n");
        ui.label(RichText::new(first).monospace().color(crate::theme::text_dim()));
        ui.horizontal(|ui| {
            if ui.button("Insert at the cursor").clicked() {
                asked = Some(Asked::Insert(text.clone()));
            }
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(text.clone());
            }
            ui.separator();
            if ui.button("Save as binary\u{2026}").clicked() {
                let bytes = as_binary(&table.values, self.settings.size, self.big_endian);
                asked = Some(Asked::Save(bytes, format!("{}.bin", self.label())));
            }
            ui.checkbox(&mut self.big_endian, "Big-endian").on_hover_text("The 68000's order, highest byte first; off for the Z80, 6502 and PC");
        });
        asked
    }

    fn preview(&self, ui: &mut Ui, values: &[i64], low: i64, high: i64) {
        let (rect, response) = ui.allocate_exact_size(vec2(360.0, 120.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        if values.is_empty() {
            return;
        }
        // Scaled to the values, taking in zero when it's near.
        let (mut min, mut max) = values.iter().fold((i64::MAX, i64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        min = min.min(0).max(low);
        max = max.max(0).min(high);
        let span = (max - min).max(1) as f32;
        let y = |v: i64| rect.max.y - 6.0 - (v - min) as f32 / span * (rect.height() - 12.0);
        let x = |i: usize| rect.min.x + 4.0 + i as f32 / (values.len().max(2) - 1) as f32 * (rect.width() - 8.0);
        let dim = crate::theme::text_dim();
        painter.line_segment([pos2(rect.min.x, y(0)), pos2(rect.max.x, y(0))], (1.0, dim.gamma_multiply(0.5)));
        // At most a point a pixel across, so long tables draw quickly.
        let step = (values.len() / rect.width() as usize).max(1);
        let points: Vec<_> = (0..values.len()).step_by(step).map(|i| pos2(x(i), y(values[i]))).collect();
        painter.add(egui::Shape::line(points, (1.5, crate::theme::accent())));
        painter.text(rect.min + vec2(4.0, 2.0), egui::Align2::LEFT_TOP, max.to_string(), egui::FontId::monospace(10.0), dim);
        painter.text(pos2(rect.min.x + 4.0, rect.max.y - 2.0), egui::Align2::LEFT_BOTTOM, min.to_string(), egui::FontId::monospace(10.0), dim);
        if let Some(at) = response.hover_pos() {
            let i = (((at.x - rect.min.x - 4.0) / (rect.width() - 8.0) * (values.len() - 1) as f32).round().max(0.0) as usize).min(values.len() - 1);
            painter.circle_filled(pos2(x(i), y(values[i])), 3.0, Color32::from_rgb(255, 200, 0));
            response.on_hover_text(format!("Entry {i}: {}", values[i]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 256-entry sine of amplitude 16384, worked out separately in Python (16384 × sin(2πn/256), rounded half
    /// away from zero; none of them is within a millionth of a half, so how the sine is worked out can't matter).
    const SINE_256_16384: [i64; 256] = [
        0, 402, 804, 1205, 1606, 2006, 2404, 2801, 3196, 3590, 3981, 4370, 4756, 5139, 5520, 5897, 6270, 6639, 7005, 7366, 7723, 8076, 8423, 8765, 9102, 9434, 9760, 10080, 10394, 10702, 11003, 11297, 11585, 11866, 12140, 12406, 12665, 12916, 13160, 13395, 13623, 13842, 14053, 14256, 14449, 14635, 14811, 14978, 15137, 15286, 15426, 15557, 15679, 15791, 15893, 15986, 16069, 16143, 16207, 16261, 16305, 16340, 16364, 16379, 16384, 16379, 16364, 16340, 16305, 16261, 16207, 16143, 16069, 15986, 15893, 15791, 15679, 15557, 15426, 15286, 15137, 14978, 14811, 14635, 14449, 14256, 14053, 13842, 13623, 13395, 13160, 12916, 12665, 12406, 12140, 11866, 11585, 11297, 11003, 10702, 10394, 10080, 9760, 9434, 9102, 8765, 8423, 8076, 7723, 7366, 7005, 6639, 6270, 5897, 5520, 5139, 4756, 4370, 3981, 3590, 3196, 2801, 2404, 2006, 1606, 1205, 804, 402, 0, -402, -804, -1205, -1606, -2006, -2404, -2801, -3196, -3590, -3981, -4370, -4756, -5139, -5520, -5897, -6270, -6639, -7005, -7366, -7723, -8076, -8423, -8765, -9102, -9434, -9760, -10080, -10394, -10702, -11003, -11297, -11585, -11866, -12140, -12406, -12665, -12916, -13160, -13395, -13623, -13842, -14053, -14256, -14449, -14635, -14811, -14978, -15137, -15286, -15426, -15557, -15679, -15791, -15893, -15986, -16069, -16143, -16207, -16261, -16305, -16340, -16364, -16379, -16384, -16379, -16364, -16340, -16305, -16261, -16207, -16143, -16069, -15986, -15893, -15791, -15679, -15557, -15426, -15286, -15137, -14978, -14811, -14635, -14449, -14256, -14053, -13842, -13623, -13395, -13160, -12916, -12665, -12406, -12140, -11866, -11585, -11297, -11003, -10702, -10394, -10080, -9760, -9434, -9102, -8765, -8423, -8076, -7723, -7366, -7005, -6639, -6270, -5897, -5520, -5139, -4756, -4370, -3981, -3590, -3196, -2801, -2404, -2006, -1606, -1205, -804, -402,
    ];

    fn made(change: impl FnOnce(&mut Settings)) -> Table {
        let mut settings = Settings::default();
        change(&mut settings);
        generate(&settings)
    }

    #[test]
    fn a_word_sine_matches_the_reference() {
        let table = made(|_| {});
        assert_eq!(table.values, SINE_256_16384);
        assert_eq!(table.held, 0);
        // The same in fixed point: amplitude 1.0 with 14 fraction bits.
        assert_eq!(made(|s| (s.amplitude, s.fraction_bits) = (1.0, 14)).values, SINE_256_16384);
    }

    #[test]
    fn a_cosine_is_a_sine_a_quarter_on() {
        let cosine = made(|s| s.kind = Kind::Cosine).values;
        let shifted: Vec<i64> = (0..256).map(|i| SINE_256_16384[(i + 64) % 256]).collect();
        assert_eq!(cosine, shifted);
        assert_eq!(made(|s| s.phase = 90.0).values, shifted);
    }

    #[test]
    fn an_unsigned_byte_sine_round_the_middle() {
        // 128 + 127 × sin: 0 to 255 for a byte, as plotters use.
        let table = made(|s| (s.size, s.signed, s.amplitude, s.offset, s.length) = (Size::Byte, false, 127.0, 128.0, 64));
        assert_eq!(table.values[0], 128);
        assert_eq!(table.values[16], 255);
        assert_eq!(table.values[48], 1);
        assert_eq!(table.held, 0);
        // Two waves in the table: entry 8 of 64 is the top of the first.
        assert_eq!(made(|s| (s.length, s.periods) = (64, 2.0)).values[8], 16384);
    }

    #[test]
    fn values_too_big_are_held_and_counted() {
        // Amplitude 32768 for a signed word: the top 32768 doesn't fit, so it's 32767; the bottom -32768 does.
        let table = made(|s| s.amplitude = 32768.0);
        assert_eq!(table.values[64], 32767);
        assert_eq!(table.values[192], -32768);
        assert_eq!(table.held, 1);
        let unsigned = made(|s| s.signed = false);
        assert_eq!(unsigned.values[192], 0);
        assert_eq!(unsigned.held, 127);
    }

    #[test]
    fn rounding_is_to_the_nearest_with_halves_away_from_zero() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(round(2.4999), 2.0);
        assert_eq!(round(-0.5), -1.0);
        assert_eq!(round(0.49), 0.0);
        let ramp = made(|s| (s.kind, s.start, s.end, s.length) = (Kind::Ramp, -2.5, 2.5, 6));
        assert_eq!(ramp.values, [-3, -2, -1, 1, 2, 3]);
        // In fixed point the rounding comes after scaling: 0.3 × 16 = 4.8 is 5.
        let fixed = made(|s| (s.kind, s.start, s.end, s.length, s.fraction_bits) = (Kind::Ramp, 0.3, 0.3, 2, 4));
        assert_eq!(fixed.values, [5, 5]);
    }

    #[test]
    fn a_ramp_goes_from_start_to_end() {
        let ramp = made(|s| (s.kind, s.start, s.end, s.length) = (Kind::Ramp, 0.0, 255.0, 256));
        assert_eq!(ramp.values, (0..256).collect::<Vec<_>>());
        let down = made(|s| (s.kind, s.start, s.end, s.length) = (Kind::Ramp, 100.0, 0.0, 5));
        assert_eq!(down.values, [100, 75, 50, 25, 0]);
        let one = made(|s| (s.kind, s.start, s.end, s.length) = (Kind::Ramp, 7.0, 9.0, 1));
        assert_eq!(one.values, [7]);
    }

    #[test]
    fn easing_curves_start_and_end_where_they_should() {
        for curve in Curve::ALL {
            assert!(curve.at(0.0).abs() < 1e-12, "{curve:?}");
            assert!((curve.at(1.0) - 1.0).abs() < 1e-12, "{curve:?}");
            let table = made(|s| (s.kind, s.curve, s.start, s.end, s.length) = (Kind::Easing, curve, 0.0, 1000.0, 101));
            assert_eq!(table.values[0], 0);
            assert_eq!(table.values[100], 1000);
        }
        // In and out curves are symmetrical about the middle.
        for curve in [Curve::InOutSine, Curve::InOutQuad, Curve::InOutCubic, Curve::Smoothstep] {
            assert!((curve.at(0.5) - 0.5).abs() < 1e-12, "{curve:?}");
        }
        let quad = made(|s| (s.kind, s.curve, s.start, s.end, s.length) = (Kind::Easing, Curve::InQuad, 0.0, 100.0, 11));
        assert_eq!(quad.values, [0, 1, 4, 9, 16, 25, 36, 49, 64, 81, 100]);
        let cube = made(|s| (s.kind, s.curve, s.start, s.end, s.length) = (Kind::Easing, Curve::OutCubic, 0.0, 1000.0, 11));
        assert_eq!(cube.values, [0, 271, 488, 657, 784, 875, 936, 973, 992, 999, 1000]);
    }

    #[test]
    fn multiplication_and_division_tables() {
        // The start of each line of a 320-pixel, one-bitplane screen.
        let lines = made(|s| (s.kind, s.length, s.factor) = (Kind::Multiply, 256, 40.0));
        assert_eq!(lines.values, (0..256).map(|n| n * 40).collect::<Vec<_>>());
        let from_one = made(|s| (s.kind, s.length, s.first, s.factor) = (Kind::Multiply, 3, 1, 3.0));
        assert_eq!(from_one.values, [3, 6, 9]);
        let divide = made(|s| (s.kind, s.length, s.dividend) = (Kind::Divide, 8, 32768.0));
        assert_eq!(divide.values, [32767, 32767, 16384, 10923, 8192, 6554, 5461, 4681]);
        // Entry 0 divides by zero and gives the largest value; entry 1 doesn't fit and is held.
        assert_eq!(divide.held, 1);
        let reciprocal = made(|s| (s.kind, s.length, s.first, s.dividend, s.signed) = (Kind::Divide, 3, 2, 65536.0, false));
        assert_eq!(reciprocal.values, [32768, 21845, 16384]);
    }

    #[test]
    fn perspective_is_scale_over_z() {
        let table = made(|s| (s.kind, s.length, s.scale, s.near, s.step) = (Kind::Perspective, 4, 16384.0, 64.0, 64.0));
        assert_eq!(table.values, [256, 128, 85, 64]);
        let through_zero = made(|s| (s.kind, s.length, s.scale, s.near, s.step) = (Kind::Perspective, 3, 100.0, -1.0, 1.0));
        assert_eq!(through_zero.values, [-100, 32767, 100]);
    }

    #[test]
    fn random_tables_come_from_the_seed() {
        // Marsaglia's xorshift32 from 1, worked out separately.
        let mut random = Xorshift::new(1);
        assert_eq!([random.next(), random.next(), random.next(), random.next(), random.next()], [270369, 67634689, 2647435461, 307599695, 2398689233]);
        let table = made(|s| (s.kind, s.length, s.seed, s.low, s.high) = (Kind::Random, 5, 1, 0, 255));
        // Each is the top byte of the number: 270369 >> 24 is 0, and so on.
        assert_eq!(table.values, [0, 4, 157, 18, 142]);
        assert_eq!(made(|s| (s.kind, s.seed) = (Kind::Random, 99)), made(|s| (s.kind, s.seed) = (Kind::Random, 99)));
        assert_ne!(made(|s| (s.kind, s.seed) = (Kind::Random, 99)), made(|s| (s.kind, s.seed) = (Kind::Random, 100)));
        let dice = made(|s| (s.kind, s.length, s.low, s.high) = (Kind::Random, 6000, 1, 6));
        assert!(dice.values.iter().all(|v| (1..=6).contains(v)));
        for face in 1..=6 {
            let count = dice.values.iter().filter(|v| **v == face).count();
            assert!((800..1200).contains(&count), "{face}: {count}");
        }
    }

    #[test]
    fn each_assembler_and_language_gets_its_own_form() {
        let values = [0, 402, -402];
        let text = |form| {
            let mut g = Generator { form, per_line: 8, label: "sine".into(), ..Generator::default() };
            g.settings.size = Size::Word;
            g.settings.signed = true;
            g.table = Table { values: values.to_vec(), ..Table::default() };
            g.made_from = Some(g.settings.clone());
            g.text()
        };
        assert_eq!(text(Form::Motorola), "sine:\n        dc.w    $0000,$0192,$FE6E\n");
        assert_eq!(text(Form::Z80), "sine:\n        dw      $0000,$0192,$FE6E\n");
        assert_eq!(text(Form::Acme), "sine\n        !word   $0000,$0192,$FE6E\n");
        assert_eq!(text(Form::Dot), "sine\n        .word   $0000,$0192,$FE6E\n");
        assert_eq!(text(Form::M6809), "sine\n        fdb     $0000,$0192,$FE6E\n");
        assert_eq!(text(Form::Nasm), "sine:\n        dw      0x0000,0x0192,0xFE6E\n");
        assert_eq!(text(Form::Basic), "1000 REM sine\n1010 DATA 0,402,-402\n");
        assert_eq!(text(Form::Php), "$sine = [\n    0x0000, 0x0192, -0x0192,\n];\n");
        assert_eq!(text(Form::Python), "sine = [\n    0x0000, 0x0192, -0x0192,\n]\n");
        assert_eq!(text(Form::JavaScript), "const sine = [\n    0x0000, 0x0192, -0x0192,\n];\n");
        assert_eq!(text(Form::Rust), "const SINE: [i16; 3] = [\n    0x0000, 0x0192, -0x0192,\n];\n");
        assert_eq!(Form::for_language("Assembly Z80"), Some(Form::Z80));
    }

    #[test]
    fn source_c_and_binary_forms() {
        let values = [0, 402, -402, -16384];
        assert_eq!(as_source(&values, Size::Word, Base::Hex, 2, "sine"), "sine:\n        dc.w    $0000,$0192\n        dc.w    $FE6E,$C000\n");
        assert_eq!(as_source(&values, Size::Word, Base::Decimal, 8, ""), "        dc.w    0,402,-402,-16384\n");
        assert_eq!(as_source(&[-1, 255], Size::Byte, Base::Hex, 8, ""), "        dc.b    $FF,$FF\n");
        assert_eq!(as_source(&[-1], Size::Long, Base::Hex, 8, ""), "        dc.l    $FFFFFFFF\n");
        assert_eq!(as_c(&values, Size::Word, true, Base::Decimal, 2, "sine"), "const int16_t sine[4] = {\n    0, 402,\n    -402, -16384\n};\n");
        assert_eq!(as_c(&values, Size::Word, true, Base::Hex, 4, "sine"), "const int16_t sine[4] = {\n    0x0000, 0x0192, -0x0192, -0x4000\n};\n");
        assert_eq!(as_c(&[200], Size::Byte, false, Base::Hex, 4, ""), "const uint8_t table[1] = {\n    0xC8\n};\n");
        assert_eq!(as_binary(&values, Size::Word, true), [0x00, 0x00, 0x01, 0x92, 0xFE, 0x6E, 0xC0, 0x00]);
        assert_eq!(as_binary(&values, Size::Word, false), [0x00, 0x00, 0x92, 0x01, 0x6E, 0xFE, 0x00, 0xC0]);
        assert_eq!(as_binary(&[0x12345678], Size::Long, true), [0x12, 0x34, 0x56, 0x78]);
        assert_eq!(as_binary(&[-2], Size::Byte, true), [0xFE]);
    }

    #[test]
    fn the_window_writes_what_it_shows() {
        let mut generator = Generator::default();
        let text = generator.text();
        assert!(text.starts_with("sine:\n        dc.w    $0000,$0192,$0324,$04B5,$0646,$07D6,$0964,$0AF1\n"), "{text}");
        assert_eq!(text.lines().count(), 1 + 32);
        generator.settings.kind = Kind::Multiply;
        assert!(generator.text().starts_with("times:\n        dc.w    $0000,$0028"));
    }
}
