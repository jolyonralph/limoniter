//! The Motorola 680x0 family, as in the Amiga, the Atari ST and TT, the Sinclair QL, the Macintosh and the Sega
//! Mega Drive: the instructions and addressing modes of the 68000, 68008, 68010, 68020 and 68030 (and the CPU32's
//! and later processors' names, which take the 68020's) as vasm writes them in Motorola syntax, nothing made
//! shorter or quicker behind the programmer's back (as vasm does with -no-opt).
//!
//! Which processor is chosen as vasm and Devpac choose it: MACHINE 68020, MC68020, CPU32 or Devpac's OPT P=68020
//! in the source, which wins over the processor chosen in the settings; with neither, the 68000. Each instruction
//! and addressing mode a processor hasn't got is refused, naming the processor that has it, so code for an A1200
//! isn't quietly assembled for an A500 that would crash on it.

use super::{Isa, Line, split_operands};

pub struct M68k;

/// The setting the processor is kept in, as the 6809's PRAGMA 6309 is: its number (68000, 68020...; the CPU32 is
/// 32).
pub const SETTING: &str = "68k";

/// A processor of the family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Processor {
    #[default]
    M68000,
    /// The 68000 with an 8-bit data bus, as in the Sinclair QL: the same instructions, more slowly.
    M68008,
    M68010,
    M68020,
    M68030,
    M68040,
    M68060,
    /// Motorola's microcontrollers' 68000s (the 68332 and others): the 68010's instructions and some of the
    /// 68020's.
    Cpu32,
}

impl Processor {
    pub const ALL: [Processor; 8] = [Processor::M68000, Processor::M68008, Processor::M68010, Processor::M68020, Processor::M68030, Processor::M68040, Processor::M68060, Processor::Cpu32];

    pub fn name(self) -> &'static str {
        match self {
            Processor::M68000 => "68000",
            Processor::M68008 => "68008",
            Processor::M68010 => "68010",
            Processor::M68020 => "68020",
            Processor::M68030 => "68030",
            Processor::M68040 => "68040",
            Processor::M68060 => "68060",
            Processor::Cpu32 => "CPU32",
        }
    }

    /// As kept in the settings and the assembler's setting.
    pub fn id(self) -> i64 {
        match self {
            Processor::M68000 => 68000,
            Processor::M68008 => 68008,
            Processor::M68010 => 68010,
            Processor::M68020 => 68020,
            Processor::M68030 => 68030,
            Processor::M68040 => 68040,
            Processor::M68060 => 68060,
            Processor::Cpu32 => 32,
        }
    }

    pub fn from_id(id: i64) -> Option<Processor> {
        Processor::ALL.into_iter().find(|p| p.id() == id)
    }

    /// From a name as MACHINE and OPT P= take it: 68020, MC68020, 020, CPU32, or one of Motorola's CPU32 chips.
    pub fn from_name(name: &str) -> Option<Processor> {
        let lower = name.trim().to_ascii_lowercase();
        let bare = lower.strip_prefix("mc").unwrap_or(&lower);
        Some(match bare {
            "68000" | "000" | "68302" | "68306" | "68307" | "6830x" | "68322" | "68356" => Processor::M68000,
            "68008" | "008" => Processor::M68008,
            "68010" | "010" => Processor::M68010,
            "68020" | "020" => Processor::M68020,
            "68030" | "030" => Processor::M68030,
            "68040" | "040" => Processor::M68040,
            "68060" | "060" => Processor::M68060,
            "cpu32" | "68328" | "68330" | "68331" | "68332" | "68333" | "68334" | "68336" | "6833x" | "68340" | "68349" | "68360" | "683xx" => Processor::Cpu32,
            _ => return None,
        })
    }

    /// Whether it has the 68010's instructions (MOVEC, MOVES, RTD, MOVE from CCR, BKPT).
    pub fn has_68010(self) -> bool {
        !matches!(self, Processor::M68000 | Processor::M68008)
    }

    /// Whether it has the 68020's instructions and addressing modes that the CPU32 has too.
    pub fn has_68020_or_cpu32(self) -> bool {
        self.has_68020()
            || self == Processor::Cpu32
    }

    /// Whether it's a 68020 or later.
    pub fn has_68020(self) -> bool {
        matches!(self, Processor::M68020 | Processor::M68030 | Processor::M68040 | Processor::M68060)
    }
}

/// The processor chosen in the settings for 680x0 source, which MACHINE in the source wins over.
static CHOSEN: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(68000);

pub fn chosen() -> Processor {
    Processor::from_id(CHOSEN.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or_default()
}

pub fn choose(processor: Processor) {
    CHOSEN.store(processor.id(), std::sync::atomic::Ordering::Relaxed);
}

/// The processor code at this line is for: what MACHINE (or the settings) chose, or the 68000.
pub fn processor(line: &Line) -> Processor {
    line.setting(SETTING).and_then(Processor::from_id).unwrap_or_default()
}

/// What a processor needs to have for an instruction or addressing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Need {
    /// A 68010 or later, or a CPU32.
    M68010,
    /// A 68020 or later, or a CPU32.
    M68020,
    /// A 68020 or later; the CPU32 hasn't it.
    M68020NotCpu32,
    /// The 68020 alone: CALLM and RTM, which the 68030 dropped.
    Only68020,
}

/// Refuses `what` (an instruction, or an address) unless the processor has it, saying which processor does.
fn needs(cpu: Processor, need: Need, what: &str) -> Result<(), String> {
    let has = match need {
        Need::M68010 => cpu.has_68010(),
        Need::M68020 => cpu.has_68020_or_cpu32(),
        Need::M68020NotCpu32 => cpu.has_68020(),
        Need::Only68020 => cpu == Processor::M68020,
    };
    if has {
        return Ok(());
    }
    let name = cpu.name();
    Err(match need {
        Need::M68010 => crate::i18n::tf!("{what} needs a 68010 or later, and this is for the {name}: MACHINE 68010 (or a later one) chooses another"),
        Need::M68020 => crate::i18n::tf!("{what} needs a 68020 or later, and this is for the {name}: MACHINE 68020 (or a later one) chooses another"),
        Need::M68020NotCpu32 if cpu == Processor::Cpu32 => crate::i18n::tf!("{what} needs a 68020 or later: the CPU32 hasn't got it"),
        Need::M68020NotCpu32 => crate::i18n::tf!("{what} needs a 68020 or later, and this is for the {name}: MACHINE 68020 (or a later one) chooses another"),
        Need::Only68020 => crate::i18n::tf!("{what} is only the 68020's (the 68030 and later haven't got it), and this is for the {name}: MACHINE 68020 chooses it"),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    /// The usual size field: byte 0, word 1, long 2.
    fn bits(self) -> u16 {
        match self {
            Size::Byte => 0,
            Size::Word => 1,
            Size::Long => 2,
        }
    }
}

/// An index register: its number, whether it's an address register, whether all 32 bits count, and its scale (0
/// to 3 for ×1, ×2, ×4 and ×8, which only the 68020 and later have).
#[derive(Clone, Debug, PartialEq)]
struct Index {
    reg: u16,
    address: bool,
    long: bool,
    scale: u16,
}

impl Index {
    /// The bits it takes in an extension word.
    fn bits(&self) -> u16 {
        (self.address as u16) << 15 | self.reg << 12 | (self.long as u16) << 11 | self.scale << 9
    }
}

/// How wide a displacement in a full extension word is: none at all, a word or a long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Width {
    Null,
    Word,
    Long,
}

impl Width {
    fn bits(self) -> u16 {
        match self {
            Width::Null => 1,
            Width::Word => 2,
            Width::Long => 3,
        }
    }
}

/// The base of a 68020 address: an address register or PC, either of which may be suppressed (ZA0, ZPC), counting
/// as 0 while the mode's still the one it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    A(u16),
    Pc,
    NoA(u16),
    NoPc,
}

/// Whether a 68020 address reads a pointer from memory, and if so whether its index is added before (pre-indexed,
/// ([bd,An,Xn],od)) or after (post-indexed, ([bd,An],Xn,od)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Indirect {
    No,
    Pre,
    Post,
}

/// A displacement in a full extension word: its expression, its width, and whether the source gave the width
/// (8.L), so it's kept even if a narrower or wider one would do.
#[derive(Clone, Debug, PartialEq)]
struct Displacement {
    value: Option<String>,
    width: Width,
    given: bool,
}

/// A 68020 address with a full extension word: ([bd,An,Xn*4],od) and the rest.
#[derive(Clone, Debug, PartialEq)]
struct Full {
    base: Base,
    index: Index,
    /// Whether the index is suppressed: none given (ZD0.W), or ZDn.
    suppressed: bool,
    bd: Displacement,
    indirect: Indirect,
    od: Displacement,
}

impl Full {
    fn pc(&self) -> bool {
        matches!(self.base, Base::Pc | Base::NoPc)
    }

    /// The format word.
    fn format(&self) -> u16 {
        let suppressed_base = matches!(self.base, Base::NoA(_) | Base::NoPc);
        let indirect = match self.indirect {
            Indirect::No => 0,
            Indirect::Post if !self.suppressed => 4 | self.od.width.bits(),
            _ => self.od.width.bits(),
        };
        self.index.bits() | 0x100 | (suppressed_base as u16) << 7 | (self.suppressed as u16) << 6 | self.bd.width.bits() << 4 | indirect
    }
}

/// An effective address.
#[derive(Clone, Debug, PartialEq)]
enum Ea {
    D(u16),
    A(u16),
    Indirect(u16),
    PostIncrement(u16),
    PreDecrement(u16),
    /// d16(An), and whether the source said .W.
    Displacement(u16, String, bool),
    /// d8(An,Xn), and whether the source said .B.
    Indexed(u16, String, Index, bool),
    AbsoluteWord(String),
    AbsoluteLong(String),
    PcDisplacement(String, bool),
    PcIndexed(String, Index, bool),
    Full(Box<Full>),
    Immediate(String),
    Sr,
    Ccr,
    Usp,
    /// A list of registers, for MOVEM: bit 0 is D0, bit 15 is A7.
    List(u16),
    /// Two data registers, Dh:Dl, as the 68020's long multiplies and divides take them.
    Pair(u16, u16),
    /// Two registers in brackets, (Rn):(Rm), as CAS2 takes them: each 0 to 7 for D0 to D7, 8 to 15 for A0 to A7.
    IndirectPair(u16, u16),
}

impl Ea {
    /// The six bits of mode and register.
    fn bits(&self) -> u16 {
        match self {
            Ea::D(r) => *r,
            Ea::A(r) => 0o10 | r,
            Ea::Indirect(r) => 0o20 | r,
            Ea::PostIncrement(r) => 0o30 | r,
            Ea::PreDecrement(r) => 0o40 | r,
            Ea::Displacement(r, ..) => 0o50 | r,
            Ea::Indexed(r, ..) => 0o60 | r,
            Ea::Full(full) => match full.base {
                Base::A(r) | Base::NoA(r) => 0o60 | r,
                Base::Pc | Base::NoPc => 0o73,
            },
            Ea::AbsoluteWord(_) => 0o70,
            Ea::AbsoluteLong(_) => 0o71,
            Ea::PcDisplacement(..) => 0o72,
            Ea::PcIndexed(..) => 0o73,
            Ea::Immediate(_) => 0o74,
            _ => 0,
        }
    }

    fn is_memory(&self) -> bool {
        !matches!(self, Ea::D(_) | Ea::A(_) | Ea::Immediate(_) | Ea::Sr | Ea::Ccr | Ea::Usp | Ea::List(_) | Ea::Pair(..) | Ea::IndirectPair(..))
    }

    /// Relative to PC: read, never written.
    fn is_pc(&self) -> bool {
        match self {
            Ea::PcDisplacement(..) | Ea::PcIndexed(..) => true,
            Ea::Full(full) => full.pc(),
            _ => false,
        }
    }

    /// Memory that may be written.
    fn is_memory_alterable(&self) -> bool {
        self.is_memory() && !self.is_pc()
    }

    /// An address with no size of its own, as JMP, LEA, CHK2 and the bit fields take: not (An)+, -(An) or a
    /// number.
    fn is_control(&self) -> bool {
        self.is_memory() && !matches!(self, Ea::PostIncrement(_) | Ea::PreDecrement(_))
    }

    /// Anything but an address register: a data register, memory, or a number.
    fn is_data(&self) -> bool {
        matches!(self, Ea::D(_) | Ea::Immediate(_)) || self.is_memory()
    }
}

fn register(text: &str) -> Option<Ea> {
    let lower = text.trim().to_ascii_lowercase();
    if lower == "sp" {
        return Some(Ea::A(7));
    }
    let (kind, number) = lower.split_at(1.min(lower.len()));
    let number: u16 = number.parse().ok().filter(|n| *n < 8)?;
    match kind {
        "d" => Some(Ea::D(number)),
        "a" => Some(Ea::A(number)),
        _ => None,
    }
}

/// A register inside an address's brackets: D0-D7, A0-A7, SP, PC, or one of them suppressed (ZD0, ZA0, ZPC),
/// with a size (.W, .L) and a scale (*2) if it's an index.
#[derive(Clone, Debug, PartialEq)]
struct Reg {
    kind: char,
    number: u16,
    zero: bool,
    long: Option<bool>,
    scale: Option<u16>,
}

impl Reg {
    /// As the base of an address: An or PC, with no size or scale.
    fn base(&self) -> Option<Base> {
        if self.long.is_some() || self.scale.is_some() {
            return None;
        }
        match (self.kind, self.zero) {
            ('a', false) => Some(Base::A(self.number)),
            ('a', true) => Some(Base::NoA(self.number)),
            ('p', false) => Some(Base::Pc),
            ('p', true) => Some(Base::NoPc),
            _ => None,
        }
    }

    fn index(&self) -> Option<Index> {
        (self.kind != 'p').then(|| Index { reg: self.number, address: self.kind == 'a', long: self.long.unwrap_or(false), scale: self.scale.unwrap_or(0) })
    }
}

fn bracket_register(text: &str) -> Option<Reg> {
    let lower = text.trim().to_ascii_lowercase();
    // Only after a register is * a scale; 4*2 is a number.
    let (core, scale) = match lower.rsplit_once('*') {
        Some((core, scale)) => {
            let scale = match scale.trim() {
                "1" => 0,
                "2" => 1,
                "4" => 2,
                "8" => 3,
                _ => return None,
            };
            (core.trim().to_string(), Some(scale))
        }
        None => (lower.clone(), None),
    };
    let (name, long) = match core.rsplit_once('.') {
        Some((name, "l")) => (name.to_string(), Some(true)),
        Some((name, "w")) => (name.to_string(), Some(false)),
        _ => (core.clone(), None),
    };
    let (name, zero) = match name.strip_prefix('z') {
        Some(rest) if !rest.is_empty() => (rest.to_string(), true),
        _ => (name, false),
    };
    if name == "pc" {
        return (long.is_none() && scale.is_none()).then_some(Reg { kind: 'p', number: 0, zero, long, scale });
    }
    match register(&name)? {
        Ea::D(n) => Some(Reg { kind: 'd', number: n, zero, long, scale }),
        Ea::A(n) => Some(Reg { kind: 'a', number: n, zero, long, scale }),
        _ => None,
    }
}

/// An expression and the size written after it (8.W, 8.L), if any.
fn sized(text: &str) -> (String, Option<char>) {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    for (suffix, size) in [(".b", 'b'), (".w", 'w'), (".l", 'l')] {
        if lower.len() > 2 && lower.ends_with(suffix) {
            return (text[..text.len() - 2].to_string(), Some(size));
        }
    }
    (text.to_string(), None)
}

/// A list of registers: d0-d3/a0/a5-a6.
fn register_list(text: &str) -> Option<u16> {
    let mut mask = 0u16;
    for part in text.split('/') {
        let bit = |name: &str| -> Option<u16> {
            match register(name)? {
                Ea::D(r) => Some(r),
                Ea::A(r) => Some(8 + r),
                _ => None,
            }
        };
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (bit(from)?, bit(to)?);
                for r in from.min(to)..=from.max(to) {
                    mask |= 1 << r;
                }
            }
            None => mask |= 1 << bit(part)?,
        }
    }
    Some(mask)
}

fn operand(text: &str) -> Result<Ea, String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(value) = text.strip_prefix('#') {
        return Ok(Ea::Immediate(value.to_string()));
    }
    match lower.as_str() {
        "sr" => return Ok(Ea::Sr),
        "ccr" => return Ok(Ea::Ccr),
        "usp" => return Ok(Ea::Usp),
        _ => {}
    }
    if let Some(r) = register(text) {
        return Ok(r);
    }
    // Dh:Dl, and CAS2's (Rn):(Rm).
    if let Some((high, low)) = lower.split_once(':') {
        if let (Some(Ea::D(h)), Some(Ea::D(l))) = (register(high), register(low)) {
            return Ok(Ea::Pair(h, l));
        }
        let inside = |t: &str| t.trim().strip_prefix('(').and_then(|t| t.strip_suffix(')')).and_then(register);
        let code = |ea: Ea| match ea {
            Ea::D(r) => Some(r),
            Ea::A(r) => Some(8 + r),
            _ => None,
        };
        if let (Some(first), Some(second)) = (inside(high).and_then(code), inside(low).and_then(code)) {
            return Ok(Ea::IndirectPair(first, second));
        }
    }
    if (lower.contains('/') || lower.contains('-')) && !lower.contains('(')
        && let Some(mask) = register_list(&lower)
    {
        return Ok(Ea::List(mask));
    }
    if let Some(inner) = lower.strip_prefix("-(").and_then(|r| r.strip_suffix(')')) {
        return match register(inner) {
            Some(Ea::A(r)) => Ok(Ea::PreDecrement(r)),
            _ => Err(crate::i18n::tf!("-({inner}) needs an address register")),
        };
    }
    if let Some(inner) = lower.strip_prefix('(').and_then(|r| r.strip_suffix(")+")) {
        return match register(inner) {
            Some(Ea::A(r)) => Ok(Ea::PostIncrement(r)),
            _ => Err(crate::i18n::tf!("({inner})+ needs an address register")),
        };
    }
    if lower.ends_with(')')
        && let Some(open) = matching_open(text)
    {
        let outside = text[..open].trim();
        let inside = text[open + 1..text.len() - 1].trim();
        if let Some(ea) = bracketed(text, outside, inside)? {
            return Ok(ea);
        }
    }
    // An address, long unless .w says otherwise.
    if let Some(address) = lower.strip_suffix(".w") {
        return Ok(Ea::AbsoluteWord(text[..address.len()].to_string()));
    }
    if let Some(address) = lower.strip_suffix(".l") {
        return Ok(Ea::AbsoluteLong(text[..address.len()].to_string()));
    }
    Ok(Ea::AbsoluteLong(text.to_string()))
}

/// An address in brackets, `outside` being what's before them: (An), d(An), (d,An,Xn), the same from PC, and the
/// 68020's ([bd,An,Xn],od) and the rest. None if there's no register inside, so it's a number in brackets.
fn bracketed(text: &str, outside: &str, inside: &str) -> Result<Option<Ea>, String> {
    let not_address = || crate::i18n::tf!("{text} isn't an address the 680x0 has");
    // What's in the square brackets, and what comes after them.
    let (indirect, first, after): (bool, Vec<String>, Vec<String>) = if let Some(rest) = inside.strip_prefix('[') {
        let close = rest.find(']').ok_or_else(not_address)?;
        let after = rest[close + 1..].trim();
        let after = match after.strip_prefix(',') {
            Some(after) => split_operands(after),
            None if after.is_empty() => Vec::new(),
            None => return Err(not_address()),
        };
        (true, split_operands(&rest[..close]), after)
    } else {
        (false, split_operands(inside), Vec::new())
    };
    // The base displacement, the base register and the index, in the order vasm reads them: a register that can
    // be a base comes first, and a register that can only be an index has no base before it.
    let (mut bd, mut base, mut index): (Option<String>, Option<Base>, Option<Reg>) = (None, None, None);
    for item in &first {
        match bracket_register(item) {
            Some(reg) if base.is_none() && index.is_none() && reg.base().is_some() => base = reg.base(),
            Some(reg) if index.is_none() && reg.kind != 'p' => index = Some(reg),
            Some(_) => return Err(not_address()),
            None if bd.is_none() && base.is_none() && index.is_none() => bd = Some(item.clone()),
            // (An,d) is taken as (d,An), as vasm takes it.
            None if bd.is_none() && outside.is_empty() && !indirect => bd = Some(item.clone()),
            None => return Err(not_address()),
        }
    }
    if base.is_none() && index.is_none() && !indirect {
        // A number in brackets, such as (4+2).
        return Ok(None);
    }
    let mut od: Option<String> = None;
    let mut post = false;
    for item in &after {
        match bracket_register(item) {
            Some(reg) if index.is_none() && od.is_none() && reg.kind != 'p' => {
                index = Some(reg);
                post = true;
            }
            None if od.is_none() => od = Some(item.clone()),
            _ => return Err(not_address()),
        }
    }
    if !outside.is_empty() {
        // d(An,Xn) as (d,An,Xn); before ([...]), an outer displacement, as Devpac takes it.
        if indirect && od.is_none() {
            od = Some(outside.to_string());
        } else if !indirect && bd.is_none() {
            bd = Some(outside.to_string());
        } else {
            return Err(not_address());
        }
    }
    let (bd, bd_size) = match bd {
        Some(d) => {
            let (d, size) = sized(&d);
            (Some(d), size)
        }
        None => (None, None),
    };
    let zero_index = index.as_ref().is_some_and(|x| x.zero);
    let default_size = if index.is_some() { 'b' } else { 'w' };
    let size = bd_size.unwrap_or(default_size);
    let zero_base = matches!(base, None | Some(Base::NoA(_) | Base::NoPc));
    // The 68000's own modes, and its brief index with the 68020's scale.
    if !indirect && !zero_base && ((index.is_none() && size == 'w') || (index.is_some() && size == 'b' && !zero_index)) {
        let given = bd_size.is_some();
        return Ok(Some(match (base, index.as_ref().and_then(Reg::index)) {
            (Some(Base::A(r)), None) => match bd {
                Some(d) => Ea::Displacement(r, d, given),
                None => Ea::Indirect(r),
            },
            (Some(Base::Pc), None) => Ea::PcDisplacement(bd.unwrap_or_else(|| "0".into()), given),
            (Some(Base::A(r)), Some(x)) => Ea::Indexed(r, bd.unwrap_or_else(|| "0".into()), x, given),
            (Some(Base::Pc), Some(x)) => Ea::PcIndexed(bd.unwrap_or_else(|| "0".into()), x, given),
            _ => return Err(not_address()),
        }));
    }
    // Everything else takes a full extension word.
    let base = base.unwrap_or(Base::NoA(0));
    let (index, suppressed) = match &index {
        Some(reg) => (reg.index().ok_or_else(not_address)?, reg.zero),
        None => (Index { reg: 0, address: false, long: false, scale: 0 }, true),
    };
    let bd = Displacement { width: if bd.is_none() { Width::Null } else if size == 'l' { Width::Long } else { Width::Word }, value: bd, given: bd_size.is_some() };
    let (od, od_size) = match od {
        Some(d) => {
            let (d, size) = sized(&d);
            (Some(d), size)
        }
        None => (None, None),
    };
    let od = Displacement { width: if od.is_none() { Width::Null } else if od_size == Some('l') { Width::Long } else { Width::Word }, value: od, given: od_size.is_some() };
    let indirect = match (indirect, post) {
        (false, _) => Indirect::No,
        (true, false) => Indirect::Pre,
        (true, true) => Indirect::Post,
    };
    Ok(Some(Ea::Full(Box::new(Full { base, index, suppressed, bd, indirect, od }))))
}

/// Where the bracket that ends the text opens.
fn matching_open(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn fits_word(v: i64) -> bool {
    (-0x8000..=0x7FFF).contains(&v)
}

fn fits_byte(v: i64) -> bool {
    (-0x80..=0x7F).contains(&v)
}

/// An address made what the processor needs for its values, as vasm makes it with nothing optimised: on a 68020
/// (or a CPU32), a displacement too big for its mode becomes the full extension word's, wider; and what the
/// processor hasn't got is refused. `cpc` is where the instruction's extension words start, as vasm reckons a
/// PC-relative displacement when choosing its size.
fn settle(ea: Ea, text: &str, line: &mut Line, cpc: i64, cpu: Processor) -> Result<Ea, String> {
    let wide = cpu.has_68020_or_cpu32();
    let full = |base: Base, bd: &str, width: Width| Full { base, index: Index { reg: 0, address: false, long: false, scale: 0 }, suppressed: true, bd: Displacement { value: Some(bd.to_string()), width, given: false }, indirect: Indirect::No, od: Displacement { value: None, width: Width::Null, given: false } };
    let ea = match ea {
        Ea::Displacement(r, d, false) if wide && !fits_word(line.eval(&d)?) => Ea::Full(Box::new(full(Base::A(r), &d, Width::Long))),
        Ea::PcDisplacement(d, false) if wide && !fits_word(line.eval(&d)? - cpc) => Ea::Full(Box::new(full(Base::Pc, &d, Width::Long))),
        Ea::Indexed(r, d, x, false) if wide && !fits_byte(line.eval(&d)?) => Ea::Full(Box::new(Full { index: x, suppressed: false, ..full(Base::A(r), &d, Width::Word) })),
        Ea::PcIndexed(d, x, false) if wide && !fits_byte(line.eval(&d)? - cpc) => Ea::Full(Box::new(Full { index: x, suppressed: false, ..full(Base::Pc, &d, Width::Word) })),
        other => other,
    };
    let ea = match ea {
        Ea::Full(mut f) => {
            // A word that won't hold the displacement becomes a long, unless the source said it's a word.
            if let (Width::Word, false, Some(d)) = (f.bd.width, f.bd.given, &f.bd.value) {
                let v = line.eval(d)?;
                if !fits_word(if f.pc() { v - cpc } else { v }) {
                    f.bd.width = Width::Long;
                }
            }
            if let (Width::Word, false, Some(d)) = (f.od.width, f.od.given, &f.od.value)
                && !fits_word(line.eval(d)?)
            {
                f.od.width = Width::Long;
            }
            Ea::Full(f)
        }
        other => other,
    };
    match &ea {
        Ea::Full(f) if f.indirect != Indirect::No => needs(cpu, Need::M68020NotCpu32, &crate::i18n::tf!("Memory indirect addressing, {text},"))?,
        Ea::Full(_) => needs(cpu, Need::M68020, &crate::i18n::tf!("The address {text}"))?,
        Ea::Indexed(_, _, x, _) | Ea::PcIndexed(_, x, _) if x.scale != 0 => needs(cpu, Need::M68020, &crate::i18n::tf!("The scaled index in {text}"))?,
        _ => {}
    }
    Ok(ea)
}

/// The extension words an address needs. `at` is where the first of them goes, for PC relative ones.
fn extension(ea: &Ea, size: Size, line: &mut Line, at: i64) -> Result<Vec<u16>, String> {
    let checking = line.checking();
    let brief = |x: &Index, d: i64| -> Result<u16, String> {
        if checking && !fits_byte(d) {
            return Err(crate::i18n::tf!("the offset {d} doesn't fit in 8 bits (-128 to 127)"));
        }
        Ok(x.bits() | (d as u8 as u16))
    };
    let word = |v: i64, what: &str| -> Result<u16, String> {
        if checking && !(-32768..=65535).contains(&v) {
            return Err(crate::i18n::tf!("{what} {v} doesn't fit in 16 bits"));
        }
        Ok(v as u16)
    };
    let displacement = |d: &Displacement, line: &mut Line, minus: i64| -> Result<Vec<u16>, String> {
        let Some(value) = &d.value else { return Ok(Vec::new()) };
        let v = line.eval(value)? - minus;
        Ok(match d.width {
            Width::Null => Vec::new(),
            Width::Word => {
                if checking && !fits_word(v) {
                    return Err(crate::i18n::tf!("the displacement {v} doesn't fit in 16 bits"));
                }
                vec![v as u16]
            }
            Width::Long => vec![(v >> 16) as u16, v as u16],
        })
    };
    Ok(match ea {
        Ea::Displacement(_, d, _) => vec![word(line.eval(d)?, "the offset")?],
        Ea::Indexed(_, d, x, _) => vec![brief(x, line.eval(d)?)?],
        // A short address is sign-extended, so $FFFF8240.W is the Atari ST's $FF8240, as vasm takes it.
        Ea::AbsoluteWord(a) => {
            let v = line.eval(a)?;
            vec![word(if (0xFFFF_8000..=0xFFFF_FFFF).contains(&v) { v - 0x1_0000_0000 } else { v }, "the address")?]
        }
        Ea::AbsoluteLong(a) => {
            let v = line.eval(a)?;
            vec![(v >> 16) as u16, v as u16]
        }
        // From PC the offset is signed: one of 40000 would go backwards.
        Ea::PcDisplacement(d, _) => {
            let offset = line.eval(d)? - at;
            if checking && !fits_word(offset) {
                return Err(crate::i18n::tf!("the offset from PC {offset} doesn't fit in 16 bits (-32768 to 32767)"));
            }
            vec![offset as u16]
        }
        Ea::PcIndexed(d, x, _) => vec![brief(x, line.eval(d)? - at)?],
        Ea::Full(f) => {
            // From PC (even a suppressed one, as vasm writes it), the base displacement is from the format word.
            let mut words = vec![f.format()];
            words.extend(displacement(&f.bd, line, if f.pc() { at } else { 0 })?);
            words.extend(displacement(&f.od, line, 0)?);
            words
        }
        Ea::Immediate(v) => {
            let v = line.eval(v)?;
            match size {
                Size::Byte => vec![v as u8 as u16],
                Size::Word => vec![v as u16],
                Size::Long => vec![(v >> 16) as u16, v as u16],
            }
        }
        _ => Vec::new(),
    })
}

fn condition(name: &str) -> Option<u16> {
    Some(match name {
        "t" => 0,
        "f" => 1,
        "hi" => 2,
        "ls" => 3,
        "cc" | "hs" => 4,
        "cs" | "lo" => 5,
        "ne" => 6,
        "eq" => 7,
        "vc" => 8,
        "vs" => 9,
        "pl" => 10,
        "mi" => 11,
        "ge" => 12,
        "lt" => 13,
        "gt" => 14,
        "le" => 15,
        _ => return None,
    })
}

const PLAIN: [(&str, u16); 7] = [("rts", 0x4E75), ("rte", 0x4E73), ("rtr", 0x4E77), ("nop", 0x4E71), ("reset", 0x4E70), ("trapv", 0x4E76), ("illegal", 0x4AFC)];

const OTHERS: [&str; 65] = [
    "move", "movea", "moveq", "movem", "movep", "lea", "pea", "clr", "neg", "negx", "not", "tst", "add", "adda", "addi", "addq", "addx", "sub", "suba", "subi", "subq", "subx", "and", "andi", "or", "ori", "eor", "eori", "cmp", "cmpa", "cmpi", "cmpm", "mulu", "muls", "divu", "divs", "asl", "asr", "lsl", "lsr", "rol", "ror", "roxl", "roxr", "btst", "bchg", "bclr", "bset", "jmp", "jsr", "stop", "trap", "link", "unlk", "swap", "ext", "exg", "chk", "tas", "nbcd", "abcd", "sbcd", "dbra", "bsr", "bra",
];

/// The 68010's instructions, and the 68020's and 68030's (but for the conditional traps, told by their names).
const LATER: [&str; 23] = ["movec", "moves", "rtd", "bkpt", "bfchg", "bfclr", "bfexts", "bfextu", "bfffo", "bfins", "bfset", "bftst", "cas", "cas2", "chk2", "cmp2", "divsl", "divul", "extb", "pack", "unpk", "rtm", "callm"];

/// MOVEC's control registers: the code, and which processors have the register (the 68040's and 68060's own
/// come with their MMUs, later).
const CONTROL: [(&str, u16, &[Processor]); 8] = {
    use Processor::*;
    const ALL: &[Processor] = &[M68010, M68020, M68030, M68040, M68060, Cpu32];
    [("sfc", 0x000, ALL), ("dfc", 0x001, ALL), ("usp", 0x800, ALL), ("vbr", 0x801, ALL), ("cacr", 0x002, &[M68020, M68030, M68040, M68060]), ("caar", 0x802, &[M68020, M68030]), ("msp", 0x803, &[M68020, M68030, M68040]), ("isp", 0x804, &[M68020, M68030, M68040])]
};

/// A control register's name, from its code in MOVEC, if the processor has it.
pub fn control_register(code: u16, cpu: Processor) -> Option<&'static str> {
    CONTROL.iter().find(|(_, c, has)| *c == code && has.contains(&cpu)).map(|(name, ..)| *name)
}

/// The mnemonic without its size, and the size.
fn split_size(mnemonic: &str) -> (&str, Option<Size>) {
    match mnemonic.rsplit_once('.') {
        Some((name, "b")) => (name, Some(Size::Byte)),
        Some((name, "w")) => (name, Some(Size::Word)),
        Some((name, "l")) => (name, Some(Size::Long)),
        Some((name, "s")) => (name, Some(Size::Byte)),
        _ => (mnemonic, None),
    }
}

fn is_branch(name: &str) -> bool {
    name == "bra" || name == "bsr" || name.strip_prefix('b').and_then(condition).is_some_and(|c| c > 1)
}

fn is_dbcc(name: &str) -> bool {
    name == "dbra" || name.strip_prefix("db").and_then(condition).is_some()
}

fn is_scc(name: &str) -> bool {
    name.strip_prefix('s').and_then(condition).is_some()
}

/// The 68020's TRAPcc: its condition.
fn trap_condition(name: &str) -> Option<u16> {
    name.strip_prefix("trap").and_then(condition)
}

impl Isa for M68k {
    fn name(&self) -> &'static str {
        "68000"
    }

    fn big_endian(&self) -> bool {
        true
    }

    fn operand_ends_at_space(&self) -> bool {
        true
    }

    fn is_instruction(&self, word: &str) -> bool {
        let (name, _) = split_size(word);
        PLAIN.iter().any(|(n, _)| *n == name) || OTHERS.contains(&name) || LATER.contains(&name) || is_branch(name) || is_dbcc(name) || is_scc(name) || trap_condition(name).is_some()
    }

    fn is_directive(&self, word: &str) -> bool {
        word == "machine" || word == "cpu32" || word.strip_prefix("mc").and_then(Processor::from_name).is_some()
    }

    /// MACHINE 68020, MC68020 and CPU32, as vasm takes them, and Devpac's OPT P=68020.
    fn directive(&self, word: &str, operands: &str, line: &mut Line) -> Option<Result<Vec<u8>, String>> {
        let chosen = match word {
            "machine" => {
                let name = operands.trim();
                // The maths chips and the 68851 MMU are chosen beside the processor, and change nothing here yet.
                if matches!(name.to_ascii_lowercase().trim_start_matches("mc"), "68881" | "68882" | "68851") {
                    return Some(Ok(Vec::new()));
                }
                match Processor::from_name(name) {
                    Some(p) => p,
                    None => return Some(Err(crate::i18n::tf!("MACHINE takes 68000, 68008, 68010, 68020, 68030, 68040, 68060 or CPU32, and {name} isn't one"))),
                }
            }
            "cpu32" => Processor::Cpu32,
            "opt" => {
                // OPT P=68020 (or P=68020/68881); the rest of the line is the assembler's.
                for option in operands.split([',', ' ', '\t']) {
                    if let Some(name) = option.trim().to_ascii_lowercase().strip_prefix("p=")
                        && let Some(p) = name.split('/').find_map(Processor::from_name)
                    {
                        line.set_setting(SETTING, p.id());
                    }
                }
                return None;
            }
            _ => match word.strip_prefix("mc").and_then(Processor::from_name) {
                Some(p) if operands.trim().is_empty() => p,
                _ => return None,
            },
        };
        line.set_setting(SETTING, chosen.id());
        Some(Ok(Vec::new()))
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let words = encode(mnemonic, operands, line)?;
        Ok(words.iter().flat_map(|w| w.to_be_bytes()).collect())
    }
}

/// How many words an instruction has before its addresses' extension words, as vasm counts them in choosing a
/// PC-relative displacement's size.
fn opcode_words(name: &str, ops: &[Ea], size: Option<Size>) -> i64 {
    let long_multiply = matches!(name, "mulu" | "muls" | "divu" | "divs") && (size == Some(Size::Long) || matches!(ops.get(1), Some(Ea::Pair(..))));
    match name {
        "cas2" => 3,
        "movem" | "moves" | "cas" | "chk2" | "cmp2" | "callm" | "divsl" | "divul" | "bfchg" | "bfclr" | "bfexts" | "bfextu" | "bfffo" | "bfins" | "bfset" | "bftst" => 2,
        "btst" | "bchg" | "bclr" | "bset" if matches!(ops.first(), Some(Ea::Immediate(_))) => 2,
        _ if long_multiply => 2,
        _ => 1,
    }
}

/// A bit field's operand: the address, and the field's offset and width ({4:8}, {d1:d2}).
fn bit_field(text: &str, line: &mut Line) -> Result<(String, u16), String> {
    let wrong = || crate::i18n::tf!("{text} needs a bit field after it, such as {{0:8}} or {{d1:d2}}");
    let open = text.rfind('{').ok_or_else(wrong)?;
    let spec = text[open + 1..].trim().strip_suffix('}').ok_or_else(wrong)?;
    let (offset, width) = spec.split_once(':').ok_or_else(wrong)?;
    let part = |part: &str, line: &mut Line, range: std::ops::RangeInclusive<i64>| -> Result<(bool, u16), String> {
        if let Some(Ea::D(r)) = register(part) {
            return Ok((true, r));
        }
        let v = line.eval(part.trim().trim_start_matches('#'))?;
        if line.checking() && !range.contains(&v) {
            return Err(crate::i18n::tf!("a bit field's offset is 0 to 31 and its width 1 to 32, and {v} doesn't fit"));
        }
        Ok((false, (v & 31) as u16))
    };
    let (dynamic_offset, offset) = part(offset, line, 0..=31)?;
    let (dynamic_width, width) = part(width, line, 1..=32)?;
    Ok((text[..open].trim().to_string(), (dynamic_offset as u16) << 11 | offset << 6 | (dynamic_width as u16) << 5 | width))
}

fn encode(mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u16>, String> {
    let (name, size) = split_size(mnemonic);
    let upper = mnemonic.to_ascii_uppercase();
    let here = line.here();
    let cpu = processor(line);
    let bare = name.to_ascii_uppercase();
    if let Some((_, op)) = PLAIN.iter().find(|(n, _)| *n == name) {
        return Ok(vec![*op]);
    }
    // MOVEC names its control registers, which aren't addresses (and might be labels elsewhere).
    if name == "movec" {
        return movec(operands, cpu);
    }
    let mut texts = split_operands(operands);
    // A bit field instruction's {offset:width}.
    let field = if name.starts_with("bf") && LATER.contains(&name) {
        let at = if name == "bfins" { 1 } else { 0 };
        let Some(text) = texts.get(at).cloned() else { return Err(crate::i18n::tf!("{upper} needs an address with a bit field")) };
        let (ea, field) = bit_field(&text, line)?;
        texts[at] = ea;
        Some(field)
    } else {
        None
    };
    let mut ops: Vec<Ea> = texts.iter().map(|t| operand(t)).collect::<Result<_, _>>()?;
    // vasm never widens JMP's and JSR's d16(PC), so neither is it here.
    if matches!(name, "jmp" | "jsr")
        && let Some(Ea::PcDisplacement(_, given)) = ops.first_mut()
    {
        *given = true;
    }
    let cpc = here + 2 * opcode_words(name, &ops, size);
    let ops: Vec<Ea> = ops.into_iter().zip(&texts).map(|(ea, text)| settle(ea, text, line, cpc, cpu)).collect::<Result<_, _>>()?;
    let cpu_name = cpu.name();
    let wrong = || Err(crate::i18n::tf!("{upper} {operands} isn't a {cpu_name} instruction"));
    let sz = size.unwrap_or(Size::Word);
    // The opcode word, then each address's extension words, the source's first.
    let with = |op: u16, eas: &[(&Ea, Size)], line: &mut Line| -> Result<Vec<u16>, String> {
        let mut words = vec![op];
        for (ea, size) in eas {
            let at = here + 2 * words.len() as i64;
            words.extend(extension(ea, *size, line, at)?);
        }
        Ok(words)
    };
    // The opcode and its own extension word, then an address's extension words.
    let with_word = |op: u16, word: u16, ea: &Ea, size: Size, line: &mut Line| -> Result<Vec<u16>, String> {
        let mut words = vec![op, word];
        words.extend(extension(ea, size, line, here + 4)?);
        Ok(words)
    };
    // The 68020's long multiplies and divides, CHK.L, EXTB.L and LINK.L; MULU.W on the 68000 gives a long from two
    // words, and quietly assembling a .W in place of a .L would make a different program.
    let long_multiply = matches!(name, "mulu" | "muls" | "divu" | "divs") && (size == Some(Size::Long) || matches!(ops.get(1), Some(Ea::Pair(..))));
    if (long_multiply || matches!(name, "divsl" | "divul")) && !cpu.has_68020_or_cpu32() {
        let only = if cpu.has_68010() { cpu_name } else { "68000" };
        return Err(crate::i18n::tf!("{upper} is a 68020 instruction: the {only} has only {}.W (MACHINE 68020 chooses a 68020)", if matches!(name, "divsl" | "divul") { &bare[..4] } else { &bare }));
    }
    if name == "chk" && size == Some(Size::Long) {
        needs(cpu, Need::M68020NotCpu32, "CHK.L")?;
    }
    // What an instruction writes to, or (for TST and CMPI) what it may examine, has to be data that could be
    // written: on the 68000 never PC-relative, nor an immediate. The 68020 lets TST and CMPI read PC-relative,
    // which is why it's an easy mistake: such code runs on an A1200 and crashes an A500. (BTST may read
    // PC-relative on any 68000, so it isn't here.)
    let examines = matches!(name, "tst" | "cmpi") || (name == "cmp" && matches!(ops.first(), Some(Ea::Immediate(_))) && !matches!(ops.get(1), Some(Ea::D(_) | Ea::A(_))));
    let writes_last = matches!(name, "clr" | "neg" | "negx" | "not" | "tst" | "addq" | "subq" | "addi" | "subi" | "andi" | "ori" | "eori" | "cmpi" | "bchg" | "bclr" | "bset" | "tas" | "nbcd")
        || examines
        || is_scc(name)
        || (matches!(name, "asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr") && ops.len() == 1)
        || (matches!(name, "add" | "sub" | "and" | "or" | "eor") && ops.len() == 2 && !matches!(ops[1], Ea::D(_) | Ea::A(_)));
    if writes_last && let Some(last) = ops.last() {
        let last_text = &texts[texts.len() - 1];
        // TST of an address register (.W or .L) or a number, and TST and CMPI of PC-relative data, are the 68020's.
        let tst_extra = name == "tst" && (matches!(last, Ea::Immediate(_)) || (matches!(last, Ea::A(_)) && sz != Size::Byte));
        if examines && (last.is_pc() || tst_extra) {
            if !cpu.has_68020_or_cpu32() {
                return Err(crate::i18n::tf!("{upper} can't use {last_text} on the {cpu_name}: that's the 68020's (MACHINE 68020 chooses one)"));
            }
        } else if last.is_pc() || matches!(last, Ea::Immediate(_)) {
            return Err(crate::i18n::tf!("{upper} can't write to {last_text}"));
        }
    }
    if is_branch(name) {
        let target = line.eval(operands)?;
        let base: u16 = match name {
            "bra" => 0x6000,
            "bsr" => 0x6100,
            _ => 0x6000 | condition(&name[1..]).unwrap_or(0) << 8,
        };
        let offset = target - (here + 2);
        if mnemonic.ends_with(".s") || mnemonic.ends_with(".b") {
            if line.checking() && (!(-128..=127).contains(&offset) || offset == 0) {
                return Err(crate::i18n::tf!("{operands} is {offset} bytes away, out of a short branch's reach; use {bare}.W"));
            }
            return Ok(vec![base | (offset as u8 as u16)]);
        }
        if size == Some(Size::Long) {
            needs(cpu, Need::M68020, &upper)?;
            return Ok(vec![base | 0xFF, (offset >> 16) as u16, offset as u16]);
        }
        if line.checking() && !(-32768..=32767).contains(&offset) {
            return Err(crate::i18n::tf!("{operands} is too far away for a branch"));
        }
        return Ok(vec![base, offset as u16]);
    }
    if is_dbcc(name) {
        let cc = if name == "dbra" { 1 } else { condition(&name[2..]).unwrap_or(1) };
        let [Ea::D(r), _] = ops.as_slice() else { return wrong() };
        let offset = line.eval(&texts[1])? - (here + 2);
        return Ok(vec![0x50C8 | cc << 8 | r, offset as u16]);
    }
    if is_scc(name) {
        let cc = condition(&name[1..]).unwrap_or(0);
        let [ea] = ops.as_slice() else { return wrong() };
        return with(0x50C0 | cc << 8 | ea.bits(), &[(ea, Size::Byte)], line);
    }
    if let Some(cc) = trap_condition(name) {
        needs(cpu, Need::M68020, &bare)?;
        return match (ops.as_slice(), size) {
            ([], None) => Ok(vec![0x50FC | cc << 8]),
            ([v @ Ea::Immediate(_)], None | Some(Size::Word)) => with(0x50FA | cc << 8, &[(v, Size::Word)], line),
            ([v @ Ea::Immediate(_)], Some(Size::Long)) => with(0x50FB | cc << 8, &[(v, Size::Long)], line),
            _ => wrong(),
        };
    }
    match (name, ops.as_slice()) {
        ("moveq", [Ea::Immediate(v), Ea::D(r)]) => {
            let v = line.eval(v)?;
            if line.checking() && !(-128..=255).contains(&v) {
                return Err(crate::i18n::tf!("MOVEQ takes -128 to 127, and {v} isn't"));
            }
            Ok(vec![0x7000 | r << 9 | (v as u8 as u16)])
        }
        ("move", [Ea::Sr, dst]) => with(0x40C0 | dst.bits(), &[(dst, Size::Word)], line),
        ("move", [Ea::Ccr, dst]) if dst.is_data() && !dst.is_pc() && !matches!(dst, Ea::Immediate(_)) => {
            needs(cpu, Need::M68010, "MOVE from CCR")?;
            with(0x42C0 | dst.bits(), &[(dst, Size::Word)], line)
        }
        ("move", [src, Ea::Ccr]) => with(0x44C0 | src.bits(), &[(src, Size::Word)], line),
        ("move", [src, Ea::Sr]) => with(0x46C0 | src.bits(), &[(src, Size::Word)], line),
        ("move", [Ea::Usp, Ea::A(r)]) => Ok(vec![0x4E68 | r]),
        ("move", [Ea::A(r), Ea::Usp]) => Ok(vec![0x4E60 | r]),
        ("move" | "movea", [src, dst]) if !matches!(src, Ea::Ccr | Ea::Sr | Ea::Usp | Ea::List(_) | Ea::Pair(..) | Ea::IndirectPair(..)) => {
            let code = match sz {
                Size::Byte => 1,
                Size::Long => 2,
                Size::Word => 3,
            };
            if matches!(dst, Ea::Immediate(_)) || dst.is_pc() {
                return Err(crate::i18n::tf!("{upper} can't write to {}", texts[1]));
            }
            let d = dst.bits();
            let op = code << 12 | (d & 7) << 9 | (d >> 3) << 6 | src.bits();
            with(op, &[(src, sz), (dst, sz)], line)
        }
        ("movem", [from, to]) => {
            // Registers to memory, or memory to registers; one register on its own is a list of one.
            let single = |ea: &Ea| match ea {
                Ea::List(mask) => Some(*mask),
                Ea::D(r) => Some(1 << r),
                Ea::A(r) => Some(1 << (8 + r)),
                _ => None,
            };
            let long = if sz == Size::Long { 0x40 } else { 0 };
            let (op, mask, ea) = match (single(from), single(to)) {
                (Some(mask), None) if to.is_memory() => {
                    // To -(An), the mask runs the other way.
                    let mask = if matches!(to, Ea::PreDecrement(_)) { mask.reverse_bits() } else { mask };
                    (0x4880 | long | to.bits(), mask, to)
                }
                (None, Some(mask)) if from.is_memory() => (0x4C80 | long | from.bits(), mask, from),
                _ => return wrong(),
            };
            with_word(op, mask, ea, sz, line)
        }
        // MOVEP: alternate bytes, as for an 8-bit peripheral; to a register 4 (word) or 5 (long), from one 6 or 7.
        ("movep", [Ea::Displacement(a, d, _), Ea::D(r)]) => Ok(vec![0x0108 | r << 9 | if sz == Size::Long { 0x140 } else { 0x100 } | a, line.eval(d)? as u16]),
        ("movep", [Ea::D(r), Ea::Displacement(a, d, _)]) => Ok(vec![0x0108 | r << 9 | if sz == Size::Long { 0x1C0 } else { 0x180 } | a, line.eval(d)? as u16]),
        ("movep", [Ea::Indirect(a), Ea::D(r)]) => Ok(vec![0x0108 | r << 9 | if sz == Size::Long { 0x140 } else { 0x100 } | a, 0]),
        ("movep", [Ea::D(r), Ea::Indirect(a)]) => Ok(vec![0x0108 | r << 9 | if sz == Size::Long { 0x1C0 } else { 0x180 } | a, 0]),
        ("lea", [src, Ea::A(r)]) => with(0x41C0 | r << 9 | src.bits(), &[(src, Size::Long)], line),
        ("pea", [src]) => with(0x4840 | src.bits(), &[(src, Size::Long)], line),
        ("tst", [Ea::A(_)]) if sz == Size::Byte => Err(crate::i18n::tr("TST.B can't take an address register: only .W and .L can").into()),
        ("clr" | "neg" | "negx" | "not" | "tst", [ea]) => {
            let base = match name {
                "clr" => 0x4200,
                "neg" => 0x4400,
                "negx" => 0x4000,
                "not" => 0x4600,
                _ => 0x4A00,
            };
            with(base | sz.bits() << 6 | ea.bits(), &[(ea, sz)], line)
        }
        ("addq" | "subq", [Ea::Immediate(v), dst]) => {
            let v = line.eval(v)?;
            if line.checking() && !(1..=8).contains(&v) {
                return Err(crate::i18n::tf!("{upper} takes 1 to 8, and {v} isn't"));
            }
            let base = if name == "addq" { 0x5000 } else { 0x5100 };
            with(base | ((v as u16) & 7) << 9 | sz.bits() << 6 | dst.bits(), &[(dst, sz)], line)
        }
        ("addx" | "subx" | "abcd" | "sbcd", [src, dst]) => {
            let base: u16 = match name {
                "addx" => 0xD100 | sz.bits() << 6,
                "subx" => 0x9100 | sz.bits() << 6,
                "abcd" => 0xC100,
                _ => 0x8100,
            };
            match (src, dst) {
                (Ea::D(y), Ea::D(x)) => Ok(vec![base | x << 9 | y]),
                (Ea::PreDecrement(y), Ea::PreDecrement(x)) => Ok(vec![base | x << 9 | 8 | y]),
                _ => wrong(),
            }
        }
        ("cmpm", [Ea::PostIncrement(y), Ea::PostIncrement(x)]) => Ok(vec![0xB108 | x << 9 | sz.bits() << 6 | y]),
        // The 68020's: a long product of two longs, or a 64-bit one in Dh:Dl; a long quotient, with the remainder
        // in Dr of Dr:Dq, of a long (DIVSL, DIVUL, or DIVS.L into one register) or of 64 bits (DIVS.L into Dr:Dq).
        ("mulu" | "muls" | "divu" | "divs" | "divsl" | "divul", [src, dst]) if long_multiply || matches!(name, "divsl" | "divul") => {
            if !src.is_data() {
                return wrong();
            }
            let signed = if matches!(name, "muls" | "divs" | "divsl") { 0x0800 } else { 0 };
            let op = if name.starts_with("mul") { 0x4C00 } else { 0x4C40 };
            let word = match (name, dst) {
                (_, Ea::D(r)) => r << 12 | signed | r,
                ("mulu" | "muls" | "divu" | "divs", Ea::Pair(high, low)) => low << 12 | signed | 0x0400 | high,
                (_, Ea::Pair(remainder, quotient)) => quotient << 12 | signed | remainder,
                _ => return wrong(),
            };
            with_word(op | src.bits(), word, src, Size::Long, line)
        }
        ("mulu" | "muls" | "divu" | "divs" | "chk", [src, Ea::D(r)]) => {
            let base = match name {
                "mulu" => 0xC0C0,
                "muls" => 0xC1C0,
                "divu" => 0x80C0,
                "divs" => 0x81C0,
                _ if sz == Size::Long => 0x4100,
                _ => 0x4180,
            };
            with(base | r << 9 | src.bits(), &[(src, sz)], line)
        }
        ("addi" | "subi" | "andi" | "ori" | "eori" | "cmpi", [Ea::Immediate(_), Ea::Ccr | Ea::Sr]) | ("and" | "or" | "eor", [Ea::Immediate(_), Ea::Ccr | Ea::Sr]) => {
            let to_sr = matches!(ops[1], Ea::Sr);
            let base = match name.trim_end_matches('i') {
                "and" => 0x023C,
                "or" => 0x003C,
                "eor" => 0x0A3C,
                _ => return wrong(),
            } | if to_sr { 0x40 } else { 0 };
            with(base, &[(&ops[0], if to_sr { Size::Word } else { Size::Byte })], line)
        }
        ("addi" | "subi" | "andi" | "ori" | "eori" | "cmpi", [src @ Ea::Immediate(_), dst]) => immediate(name, sz, src, dst, &with, line),
        ("adda" | "suba" | "cmpa", [src, Ea::A(r)]) | ("add" | "sub" | "cmp", [src, Ea::A(r)]) => {
            let base = match name.trim_end_matches('a') {
                "add" => 0xD0C0,
                "sub" => 0x90C0,
                _ => 0xB0C0,
            } | if sz == Size::Long { 0x100 } else { 0 };
            with(base | r << 9 | src.bits(), &[(src, sz)], line)
        }
        ("add" | "sub" | "and" | "or" | "cmp", [src, Ea::D(r)]) => {
            let base = arithmetic_base(name);
            with(base | r << 9 | sz.bits() << 6 | src.bits(), &[(src, sz)], line)
        }
        ("add" | "sub" | "and" | "or" | "eor", [Ea::D(r), dst]) if dst.is_memory() || name == "eor" => {
            let base = if name == "eor" { 0xB000 } else { arithmetic_base(name) };
            with(base | r << 9 | 0x100 | sz.bits() << 6 | dst.bits(), &[(dst, sz)], line)
        }
        ("add" | "sub" | "and" | "or" | "eor" | "cmp", [src @ Ea::Immediate(_), dst]) => immediate(&format!("{name}i"), sz, src, dst, &with, line),
        (shift @ ("asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr"), rest) => {
            let kind: u16 = match &shift[..shift.len() - 1] {
                "as" => 0,
                "ls" => 1,
                "rox" => 2,
                _ => 3,
            };
            let left = if shift.ends_with('l') { 0x100 } else { 0 };
            match rest {
                [Ea::Immediate(count), Ea::D(r)] => {
                    let n = line.eval(count)?;
                    if line.checking() && !(1..=8).contains(&n) {
                        return Err(crate::i18n::tf!("{upper} shifts 1 to 8 places at once, and {n} isn't"));
                    }
                    Ok(vec![0xE000 | ((n as u16) & 7) << 9 | left | sz.bits() << 6 | kind << 3 | r])
                }
                [Ea::D(count), Ea::D(r)] => Ok(vec![0xE000 | count << 9 | left | sz.bits() << 6 | 0x20 | kind << 3 | r]),
                [Ea::D(r)] => Ok(vec![0xE000 | 1 << 9 | left | sz.bits() << 6 | kind << 3 | r]),
                [ea] if ea.is_memory() => with(0xE0C0 | kind << 9 | left | ea.bits(), &[(ea, Size::Word)], line),
                _ => wrong(),
            }
        }
        (bit @ ("btst" | "bchg" | "bclr" | "bset"), [number, dst]) => {
            let kind: u16 = match bit {
                "btst" => 0,
                "bchg" => 1,
                "bclr" => 2,
                _ => 3,
            };
            match number {
                Ea::D(r) => with(0x0100 | r << 9 | kind << 6 | dst.bits(), &[(dst, Size::Byte)], line),
                Ea::Immediate(n) => {
                    let n = line.eval(n)?;
                    with_word(0x0800 | kind << 6 | dst.bits(), n as u16 & 0xFF, dst, Size::Byte, line)
                }
                _ => wrong(),
            }
        }
        ("jmp", [ea]) => with(0x4EC0 | ea.bits(), &[(ea, Size::Long)], line),
        ("jsr", [ea]) => with(0x4E80 | ea.bits(), &[(ea, Size::Long)], line),
        ("stop", [Ea::Immediate(v)]) => Ok(vec![0x4E72, line.eval(v)? as u16]),
        ("trap", [Ea::Immediate(v)]) => {
            let n = line.eval(v)?;
            if line.checking() && !(0..16).contains(&n) {
                return Err(crate::i18n::tf!("TRAP takes 0 to 15, and {n} isn't"));
            }
            Ok(vec![0x4E40 | n as u16 & 15])
        }
        ("link", [Ea::A(r), Ea::Immediate(v)]) if size == Some(Size::Long) => {
            needs(cpu, Need::M68020, "LINK.L")?;
            let v = line.eval(v)?;
            Ok(vec![0x4808 | r, (v >> 16) as u16, v as u16])
        }
        ("link", [Ea::A(r), Ea::Immediate(v)]) => Ok(vec![0x4E50 | r, line.eval(v)? as u16]),
        ("unlk", [Ea::A(r)]) => Ok(vec![0x4E58 | r]),
        ("swap", [Ea::D(r)]) => Ok(vec![0x4840 | r]),
        ("ext", [Ea::D(r)]) => Ok(vec![if sz == Size::Long { 0x48C0 } else { 0x4880 } | r]),
        ("extb", [Ea::D(r)]) if matches!(size, None | Some(Size::Long)) => {
            needs(cpu, Need::M68020, "EXTB.L")?;
            Ok(vec![0x49C0 | r])
        }
        ("exg", [Ea::D(x), Ea::D(y)]) => Ok(vec![0xC140 | x << 9 | y]),
        ("exg", [Ea::A(x), Ea::A(y)]) => Ok(vec![0xC148 | x << 9 | y]),
        ("exg", [Ea::D(x), Ea::A(y)]) | ("exg", [Ea::A(y), Ea::D(x)]) => Ok(vec![0xC188 | x << 9 | y]),
        ("tas", [ea]) => with(0x4AC0 | ea.bits(), &[(ea, Size::Byte)], line),
        ("nbcd", [ea]) => with(0x4800 | ea.bits(), &[(ea, Size::Byte)], line),
        // The 68010's.
        ("moves", [from, to]) => {
            needs(cpu, Need::M68010, "MOVES")?;
            let reg = |ea: &Ea| match ea {
                Ea::D(r) => Some(r << 12),
                Ea::A(r) => Some(0x8000 | r << 12),
                _ => None,
            };
            let (word, ea) = match (reg(from), reg(to)) {
                (Some(r), None) if to.is_memory_alterable() => (r | 0x0800, to),
                (None, Some(r)) if from.is_memory_alterable() => (r, from),
                _ => return wrong(),
            };
            with_word(0x0E00 | sz.bits() << 6 | ea.bits(), word, ea, sz, line)
        }
        ("rtd", [Ea::Immediate(v)]) => {
            needs(cpu, Need::M68010, "RTD")?;
            Ok(vec![0x4E74, line.eval(v)? as u16])
        }
        ("bkpt", [Ea::Immediate(v)]) => {
            needs(cpu, Need::M68010, "BKPT")?;
            let n = line.eval(v)?;
            if line.checking() && !(0..8).contains(&n) {
                return Err(crate::i18n::tf!("BKPT takes 0 to 7, and {n} isn't"));
            }
            Ok(vec![0x4848 | n as u16 & 7])
        }
        // The 68020's.
        (bf @ ("bftst" | "bfextu" | "bfchg" | "bfexts" | "bfclr" | "bfffo" | "bfset" | "bfins"), rest) => {
            needs(cpu, Need::M68020NotCpu32, &bare)?;
            let field = field.unwrap_or(0);
            let kind: u16 = match bf {
                "bftst" => 0,
                "bfextu" => 1,
                "bfchg" => 2,
                "bfexts" => 3,
                "bfclr" => 4,
                "bfffo" => 5,
                "bfset" => 6,
                _ => 7,
            };
            let writes = matches!(bf, "bfchg" | "bfclr" | "bfset" | "bfins");
            let (ea, reg) = match (bf, rest) {
                ("bftst" | "bfchg" | "bfclr" | "bfset", [ea]) => (ea, 0),
                ("bfextu" | "bfexts" | "bfffo", [ea, Ea::D(r)]) => (ea, *r),
                ("bfins", [Ea::D(r), ea]) => (ea, *r),
                _ => return wrong(),
            };
            if !(matches!(ea, Ea::D(_)) || (ea.is_control() && !(writes && ea.is_pc()))) {
                return wrong();
            }
            with_word(0xE8C0 | kind << 8 | ea.bits(), reg << 12 | field, ea, Size::Byte, line)
        }
        ("cas", [Ea::D(compare), Ea::D(update), ea]) if ea.is_memory_alterable() => {
            needs(cpu, Need::M68020NotCpu32, "CAS")?;
            let code = match sz {
                Size::Byte => 1,
                Size::Word => 2,
                Size::Long => 3,
            };
            with_word(0x08C0 | code << 9 | ea.bits(), update << 6 | compare, ea, sz, line)
        }
        ("cas2", [Ea::Pair(c1, c2), Ea::Pair(u1, u2), Ea::IndirectPair(r1, r2)]) if sz != Size::Byte => {
            needs(cpu, Need::M68020NotCpu32, "CAS2")?;
            Ok(vec![if sz == Size::Long { 0x0EFC } else { 0x0CFC }, r1 << 12 | u1 << 6 | c1, r2 << 12 | u2 << 6 | c2])
        }
        ("chk2" | "cmp2", [ea, reg @ (Ea::D(_) | Ea::A(_))]) if ea.is_control() => {
            needs(cpu, Need::M68020, &bare)?;
            let r = match reg {
                Ea::A(r) => 0x8000 | r << 12,
                Ea::D(r) => r << 12,
                _ => 0,
            };
            with_word(0x00C0 | sz.bits() << 9 | ea.bits(), r | if name == "chk2" { 0x0800 } else { 0 }, ea, sz, line)
        }
        ("pack" | "unpk", [from, to, Ea::Immediate(adjust)]) => {
            needs(cpu, Need::M68020NotCpu32, &bare)?;
            let base = if name == "pack" { 0x8140 } else { 0x8180 };
            let op = match (from, to) {
                (Ea::D(x), Ea::D(y)) => base | y << 9 | x,
                (Ea::PreDecrement(x), Ea::PreDecrement(y)) => base | 8 | y << 9 | x,
                _ => return wrong(),
            };
            Ok(vec![op, line.eval(adjust)? as u16])
        }
        ("rtm", [reg @ (Ea::D(_) | Ea::A(_))]) => {
            needs(cpu, Need::Only68020, "RTM")?;
            Ok(vec![0x06C0 | reg.bits()])
        }
        ("callm", [Ea::Immediate(count), ea]) if ea.is_control() => {
            needs(cpu, Need::Only68020, "CALLM")?;
            let n = line.eval(count)?;
            if line.checking() && !(0..=255).contains(&n) {
                return Err(crate::i18n::tf!("CALLM passes 0 to 255 bytes of arguments, and {n} isn't"));
            }
            with_word(0x06C0 | ea.bits(), n as u16, ea, Size::Byte, line)
        }
        _ => wrong(),
    }
}

/// MOVEC: a control register to a register, or a register to one.
fn movec(operands: &str, cpu: Processor) -> Result<Vec<u16>, String> {
    needs(cpu, Need::M68010, "MOVEC")?;
    let texts = split_operands(operands);
    let control = |text: &str| CONTROL.iter().find(|(name, ..)| text.trim().eq_ignore_ascii_case(name));
    let general = |text: &str| match register(text) {
        Some(Ea::D(r)) => Some(r << 12),
        Some(Ea::A(r)) => Some(0x8000 | r << 12),
        _ => None,
    };
    let (op, reg, (name, code, has)) = match texts.as_slice() {
        [from, to] => match (control(from), general(to), general(from), control(to)) {
            (Some(c), Some(r), ..) => (0x4E7A, r, *c),
            (_, _, Some(r), Some(c)) => (0x4E7B, r, *c),
            _ => {
                let names: Vec<String> = CONTROL.iter().map(|(n, ..)| n.to_ascii_uppercase()).collect();
                return Err(crate::i18n::tf!("MOVEC copies a control register ({}) to or from a data or address register", names.join(", ")));
            }
        },
        _ => return Err(crate::i18n::tr("MOVEC takes two operands: a control register and a data or address register").into()),
    };
    if !has.contains(&cpu) {
        let upper = name.to_ascii_uppercase();
        let them: Vec<&str> = has.iter().map(|p| p.name()).collect();
        let cpu_name = cpu.name();
        return Err(crate::i18n::tf!("The control register {upper} is the {}'s, and this is for the {cpu_name}", them.join(", ")));
    }
    Ok(vec![op, reg | code])
}

fn arithmetic_base(name: &str) -> u16 {
    match name {
        "add" => 0xD000,
        "sub" => 0x9000,
        "and" => 0xC000,
        "or" => 0x8000,
        _ => 0xB000,
    }
}

type With<'a> = dyn Fn(u16, &[(&Ea, Size)], &mut Line) -> Result<Vec<u16>, String> + 'a;

/// ADDI, SUBI, ANDI, ORI, EORI and CMPI: the value, then the destination's extension words.
fn immediate(name: &str, sz: Size, src: &Ea, dst: &Ea, with: &With, line: &mut Line) -> Result<Vec<u16>, String> {
    let base = match name {
        "addi" => 0x0600,
        "subi" => 0x0400,
        "andi" => 0x0200,
        "ori" => 0x0000,
        "eori" => 0x0A00,
        _ => 0x0C00,
    };
    with(base | sz.bits() << 6 | dst.bits(), &[(src, sz), (dst, sz)], line)
}
