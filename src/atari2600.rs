//! The Atari 2600 (VCS): its cartridges, told apart by size and by how they switch banks, and disassembled into
//! source for DASM (as 2600 programmers write it), with the TIA's and RIOT's registers named, RAM given names,
//! each instruction's cycles, and every scanline's code that runs past the 76 cycles a line has pointed out.
//!
//! The 2600 has no ROM of its own: the cartridge is all the code there is, seen at $1000-$1FFF (and every
//! mirror of it, $F000 being usual). Cartridges larger than 4K switch banks into that window when the code
//! touches an address near its top (F8, F6, F4 and the Superchip's SC versions, FA), or in other ways
//! (E0, E7, FE, 3F, DPC). The source assembles back to exactly the cartridge, with DASM or Limoniter's own
//! assembler.

use crate::asm::m6502::Mode;
use std::collections::{BTreeMap, BTreeSet};

/// How a cartridge switches its banks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    /// 2K, seen twice in the 4K window.
    K2,
    K4,
    /// Atari's: 8K, 16K and 32K in 4K banks, chosen by touching $1FF8, $1FF6 or $1FF4 on; SC adds the
    /// Superchip's 128 bytes of RAM at the start of each bank.
    F8,
    F8Sc,
    F6,
    F6Sc,
    F4,
    F4Sc,
    /// CBS RAM Plus: 12K in three banks, with 256 bytes of RAM.
    Fa,
    /// Parker Brothers: 8K in 1K slices, three switchable and the last fixed.
    E0,
    /// M Network: 16K in 2K slices, and 2K of RAM.
    E7,
    /// Activision: 8K, switched by where the stack is when JSR and RTS run.
    Fe,
    /// Tigervision: 2K slices, chosen by writing to $3F.
    ThreeF,
    /// Pitfall II's DPC chip: 8K in two banks, and 2K of graphics read through it.
    Dpc,
}

impl Scheme {
    pub const ALL: [Scheme; 14] = [Scheme::K2, Scheme::K4, Scheme::F8, Scheme::F8Sc, Scheme::F6, Scheme::F6Sc, Scheme::F4, Scheme::F4Sc, Scheme::Fa, Scheme::E0, Scheme::E7, Scheme::Fe, Scheme::ThreeF, Scheme::Dpc];

    pub fn name(self) -> &'static str {
        match self {
            Scheme::K2 => "2K",
            Scheme::K4 => "4K",
            Scheme::F8 => "F8 (8K)",
            Scheme::F8Sc => "F8SC (8K, Superchip)",
            Scheme::F6 => "F6 (16K)",
            Scheme::F6Sc => "F6SC (16K, Superchip)",
            Scheme::F4 => "F4 (32K)",
            Scheme::F4Sc => "F4SC (32K, Superchip)",
            Scheme::Fa => "FA (12K, CBS RAM Plus)",
            Scheme::E0 => "E0 (8K, Parker Brothers)",
            Scheme::E7 => "E7 (16K, M Network)",
            Scheme::Fe => "FE (8K, Activision)",
            Scheme::ThreeF => "3F (Tigervision)",
            Scheme::Dpc => "DPC (Pitfall II)",
        }
    }

    /// The sizes a cartridge of this kind can be.
    pub fn fits(self, size: usize) -> bool {
        match self {
            Scheme::K2 => size == 2048,
            Scheme::K4 => size == 4096,
            Scheme::F8 | Scheme::F8Sc | Scheme::E0 | Scheme::Fe => size == 8192,
            Scheme::F6 | Scheme::F6Sc | Scheme::E7 => size == 16384,
            Scheme::F4 | Scheme::F4Sc => size == 32768,
            Scheme::Fa => size == 12288,
            Scheme::ThreeF => size >= 4096 && size.is_multiple_of(2048) && size <= 512 * 1024,
            Scheme::Dpc => size == 10240 || size == 10495,
        }
    }

    /// The first hotspot and how many banks it chooses between, for those switched by touching an address.
    fn hotspots(self) -> Option<(u16, u16)> {
        match self {
            Scheme::F8 | Scheme::F8Sc | Scheme::Dpc => Some((0x1FF8, 2)),
            Scheme::F6 | Scheme::F6Sc => Some((0x1FF6, 4)),
            Scheme::F4 | Scheme::F4Sc => Some((0x1FF4, 8)),
            Scheme::Fa => Some((0x1FF8, 3)),
            _ => None,
        }
    }

    /// How many bytes at the start of each bank are the cartridge's RAM, not ROM: never code.
    fn ram(self) -> u16 {
        match self {
            Scheme::F8Sc | Scheme::F6Sc | Scheme::F4Sc => 256,
            Scheme::Fa => 512,
            _ => 0,
        }
    }
}

/// A piece of the cartridge as the 6507 sees it: where it is in the file, how long, and the address it runs at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slice {
    pub bank: usize,
    pub offset: usize,
    pub length: usize,
    pub origin: u16,
    /// Graphics read through a chip (DPC's), never code.
    pub data_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cartridge {
    pub scheme: Scheme,
    pub bytes: Vec<u8>,
}

fn contains(bytes: &[u8], pattern: &[u8]) -> bool {
    bytes.windows(pattern.len()).any(|w| w == pattern)
}

fn count(bytes: &[u8], pattern: &[u8]) -> usize {
    bytes.windows(pattern.len()).filter(|w| *w == pattern).count()
}

/// How a cartridge switches banks, as Stella works it out: by its size, and the instructions that only one kind
/// uses.
pub fn detect(bytes: &[u8]) -> Option<Scheme> {
    let size = bytes.len();
    // The Superchip's RAM: the first 128 bytes of every bank the same, as nothing is in a ROM there.
    let superchip = || size.is_multiple_of(4096) && size >= 8192 && (1..size / 4096).all(|b| bytes[b * 4096..b * 4096 + 128] == bytes[..128]) && bytes[..128].iter().all(|&x| x == bytes[0]);
    let three_f = || count(bytes, &[0x85, 0x3F]) >= 2;
    Some(match size {
        2048 => Scheme::K2,
        4096 => Scheme::K4,
        8192 => {
            const E0: [&[u8]; 8] = [&[0x8D, 0xE0, 0x1F], &[0x8D, 0xE0, 0x5F], &[0x8D, 0xE9, 0xFF], &[0x0C, 0xE0, 0x1F], &[0xAD, 0xE0, 0x1F], &[0xAD, 0xE9, 0xFF], &[0xAD, 0xED, 0xFF], &[0xAD, 0xF3, 0xBF]];
            const FE: [&[u8]; 4] = [&[0x20, 0x00, 0xD0, 0xC6, 0xC5], &[0x20, 0xC3, 0xF8, 0xA5, 0x82], &[0xD0, 0xFB, 0x20, 0x73, 0xFE], &[0x20, 0x00, 0xF0, 0x84, 0xD6]];
            if superchip() {
                Scheme::F8Sc
            } else if E0.iter().any(|p| contains(bytes, p)) {
                Scheme::E0
            } else if three_f() {
                Scheme::ThreeF
            } else if FE.iter().any(|p| contains(bytes, p)) {
                Scheme::Fe
            } else {
                Scheme::F8
            }
        }
        10240 | 10495 => Scheme::Dpc,
        12288 => Scheme::Fa,
        16384 => {
            const E7: [&[u8]; 7] = [&[0xAD, 0xE2, 0xFF], &[0xAD, 0xE5, 0xFF], &[0xAD, 0xE5, 0x1F], &[0xAD, 0xE7, 0x1F], &[0x0C, 0xE7, 0x1F], &[0x8D, 0xE7, 0xFF], &[0x8D, 0xE7, 0x1F]];
            if superchip() {
                Scheme::F6Sc
            } else if E7.iter().any(|p| contains(bytes, p)) {
                Scheme::E7
            } else if three_f() {
                Scheme::ThreeF
            } else {
                Scheme::F6
            }
        }
        32768 => {
            if superchip() {
                Scheme::F4Sc
            } else if three_f() {
                Scheme::ThreeF
            } else {
                Scheme::F4
            }
        }
        n if n.is_multiple_of(2048) && n > 32768 && n <= 512 * 1024 && three_f() => Scheme::ThreeF,
        _ => return None,
    })
}

/// A 2600 cartridge, by its name (.a26, or .bin of a size one comes in) and its bytes.
pub fn recognise(name: &str, bytes: &[u8]) -> Option<Cartridge> {
    let ending = std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !matches!(ending.as_str(), "a26" | "bin" | "2600") {
        return None;
    }
    let scheme = detect(bytes)?;
    // A .bin is only taken for one when its last bank's reset vector points into the cartridge.
    let vector = u16::from_le_bytes([bytes[bytes.len() - 4], bytes[bytes.len() - 3]]);
    if ending == "bin" && (vector & 0x1000 == 0 || scheme == Scheme::K2 && vector & 0x0800 == 0) {
        return None;
    }
    Some(Cartridge { scheme, bytes: bytes.to_vec() })
}

impl Cartridge {
    /// The pieces the code is in, each at the address it runs at: where a bank's reset vector says it was
    /// written to run ($F000, $D000, $1000...), or $F000.
    pub fn slices(&self) -> Vec<Slice> {
        let size = self.bytes.len();
        let window = |offset: usize, length: usize| -> u16 {
            // The high bits of the reset vector at the end of a 4K (or 2K) bank say which mirror it was written for.
            let end = offset + length;
            let vector = if end >= 4 { u16::from_le_bytes([self.bytes[end - 4], self.bytes[end - 3]]) } else { 0xF000 };
            if vector & 0x1000 != 0 { vector & 0xF000 } else { 0xF000 }
        };
        let slice = |bank: usize, offset: usize, length: usize, origin: u16| Slice { bank, offset, length, origin, data_only: false };
        match self.scheme {
            Scheme::K2 => vec![slice(0, 0, 2048, window(0, 2048) | 0x800)],
            Scheme::K4 | Scheme::F8 | Scheme::F8Sc | Scheme::F6 | Scheme::F6Sc | Scheme::F4 | Scheme::F4Sc | Scheme::Fa => (0..size / 4096).map(|b| slice(b, b * 4096, 4096, window(b * 4096, 4096))).collect(),
            Scheme::Fe => vec![slice(0, 0, 4096, 0xD000), slice(1, 4096, 4096, 0xF000)],
            Scheme::E0 => (0..8).map(|b| slice(b, b * 1024, 1024, if b == 7 { 0xFC00 } else { 0xF000 })).collect(),
            Scheme::E7 => (0..8).map(|b| slice(b, b * 2048, 2048, if b == 7 { 0xF800 } else { 0xF000 })).collect(),
            Scheme::ThreeF => (0..size / 2048).map(|b| slice(b, b * 2048, 2048, if b == size / 2048 - 1 { 0xF800 } else { 0xF000 })).collect(),
            Scheme::Dpc => {
                let mut slices: Vec<Slice> = (0..2).map(|b| slice(b, b * 4096, 4096, window(b * 4096, 4096))).collect();
                slices.push(Slice { bank: 2, offset: 8192, length: size - 8192, origin: 0, data_only: true });
                slices
            }
        }
    }

    pub fn describe(&self) -> String {
        let slices = self.slices();
        let code = slices.iter().filter(|s| !s.data_only).count();
        let what = if code == 1 { String::new() } else { format!(", {code} banks of {}K", slices[0].length / 1024) };
        format!("An Atari 2600 cartridge: {}K, {}{what}", self.bytes.len() / 1024, self.scheme.name())
    }
}

/// File, New as, Atari 2600: a whole NTSC frame (vertical sync, the vertical blank timed by the RIOT, 192 lines
/// of rainbow bands that roll, and overscan), as a 4K cartridge for DASM or Limoniter's own assembler.
pub const STARTER: &str = "; machine: atari 2600
; A frame of NTSC for the Atari 2600: rainbow bands that roll up the screen.
; F9 builds the 4K cartridge and runs it in Stella or MAME.

        processor 6502

VSYNC   = $00
VBLANK  = $01
WSYNC   = $02
COLUBK  = $09
INTIM   = $0284
TIM64T  = $0296

        seg.u variables
        org $80
frame   ds 1

        seg code
        org $F000
Reset:  sei
        cld
        ldx #$FF
        txs
        lda #0
Clear:  sta $00,x           ; the TIA and RAM cleared
        dex
        bne Clear

Frame:  lda #2
        sta VBLANK          ; the picture off
        sta VSYNC           ; three lines of vertical sync
        sta WSYNC
        sta WSYNC
        sta WSYNC
        lda #0
        sta VSYNC
        lda #43             ; 37 lines of vertical blank, timed: 43 x 64 cycles
        sta TIM64T
        inc frame
VBlank: lda INTIM
        bne VBlank
        sta WSYNC
        sta VBLANK          ; A is 0: the picture on

        ldx #192            ; 192 lines, a colour each
        ldy frame
Line:   sta WSYNC
        sty COLUBK
        iny
        dex
        bne Line

        lda #2
        sta WSYNC
        sta VBLANK          ; the picture off for 30 lines of overscan
        lda #35
        sta TIM64T
Overscan:
        lda INTIM
        bne Overscan
        jmp Frame

        org $FFFC
        .word Reset         ; where it starts
        .word Reset
";

// ---------- The TIA and RIOT ----------

const TIA_WRITE: [&str; 45] = [
    "VSYNC", "VBLANK", "WSYNC", "RSYNC", "NUSIZ0", "NUSIZ1", "COLUP0", "COLUP1", "COLUPF", "COLUBK", "CTRLPF", "REFP0", "REFP1", "PF0", "PF1", "PF2", "RESP0", "RESP1", "RESM0", "RESM1", "RESBL", "AUDC0", "AUDC1", "AUDF0", "AUDF1", "AUDV0", "AUDV1", "GRP0", "GRP1", "ENAM0", "ENAM1", "ENABL", "HMP0", "HMP1", "HMM0", "HMM1", "HMBL", "VDELP0", "VDELP1", "VDELBL", "RESMP0", "RESMP1", "HMOVE", "HMCLR", "CXCLR",
];

const TIA_READ: [&str; 14] = ["CXM0P", "CXM1P", "CXP0FB", "CXP1FB", "CXM0FB", "CXM1FB", "CXBLPF", "CXPPMM", "INPT0", "INPT1", "INPT2", "INPT3", "INPT4", "INPT5"];

const RIOT: [(u16, &str, bool); 10] = [
    (0x280, "SWCHA", false),
    (0x281, "SWACNT", false),
    (0x282, "SWCHB", false),
    (0x283, "SWBCNT", false),
    (0x284, "INTIM", false),
    (0x285, "TIMINT", false),
    (0x294, "TIM1T", true),
    (0x295, "TIM8T", true),
    (0x296, "TIM64T", true),
    (0x297, "T1024T", true),
];

/// The register an address is, read or written: the TIA's (at $00, as vcs.h has it) and the RIOT's.
pub fn register(address: u16, write: bool) -> Option<&'static str> {
    match address {
        0x00..=0x2C if write => Some(TIA_WRITE[usize::from(address)]),
        0x00..=0x0D if !write => Some(TIA_READ[usize::from(address)]),
        // The timers' starts are only written; the ports and the timer's count are read (and the ports' directions
        // written).
        _ => RIOT.iter().find(|(a, _, write_only)| *a == address && (write || !write_only)).map(|(_, n, _)| *n),
    }
}

/// What a register does, said briefly, for hovering and comments.
pub fn about(name: &str) -> Option<&'static str> {
    Some(match name {
        "VSYNC" => "Vertical sync: bit 1 on for three lines starts a new frame",
        "VBLANK" => "Vertical blank: bit 1 blanks the picture; bit 6 latches the fire buttons, bit 7 grounds the paddles",
        "WSYNC" => "Wait for the end of this scanline: the CPU stops until the next line starts",
        "RSYNC" => "Resets the horizontal sync counter (for testing)",
        "NUSIZ0" | "NUSIZ1" => "A player's copies and size, and its missile's width",
        "COLUP0" | "COLUP1" => "A player's (and its missile's) colour and brightness",
        "COLUPF" => "The playfield's and ball's colour",
        "COLUBK" => "The background colour",
        "CTRLPF" => "Playfield: reflected (bit 0), score colours (1), in front of the players (2); the ball's width",
        "REFP0" | "REFP1" => "Reflects a player (bit 3)",
        "PF0" | "PF1" | "PF2" => "Playfield graphics",
        "RESP0" | "RESP1" | "RESM0" | "RESM1" | "RESBL" => "Puts an object at the beam's position now",
        "AUDC0" | "AUDC1" => "A sound channel's tone (its waveform)",
        "AUDF0" | "AUDF1" => "A sound channel's frequency (divider)",
        "AUDV0" | "AUDV1" => "A sound channel's volume",
        "GRP0" | "GRP1" => "A player's graphics, eight pixels",
        "ENAM0" | "ENAM1" | "ENABL" => "Turns a missile or the ball on (bit 1)",
        "HMP0" | "HMP1" | "HMM0" | "HMM1" | "HMBL" => "How far HMOVE moves an object (top four bits, signed)",
        "VDELP0" | "VDELP1" | "VDELBL" => "Delays an object's graphics until the other player's are written",
        "RESMP0" | "RESMP1" => "Locks a missile to its player's position",
        "HMOVE" => "Moves every object by its HM value: write it straight after WSYNC",
        "HMCLR" => "Clears every HM value",
        "CXCLR" => "Clears the collision latches",
        "CXM0P" | "CXM1P" | "CXP0FB" | "CXP1FB" | "CXM0FB" | "CXM1FB" | "CXBLPF" | "CXPPMM" => "Collisions: bits 7 and 6",
        "INPT0" | "INPT1" | "INPT2" | "INPT3" => "A paddle's position (bit 7)",
        "INPT4" | "INPT5" => "A joystick's fire button: bit 7 clear when pressed",
        "SWCHA" => "The joysticks' directions: right, left, down, up, a bit each, clear when pushed",
        "SWACNT" => "Which of SWCHA's bits are outputs",
        "SWCHB" => "The console switches: reset (bit 0), select (1), colour (3), difficulties (6, 7)",
        "SWBCNT" => "Which of SWCHB's bits are outputs",
        "INTIM" => "The timer's count",
        "TIMINT" => "The timer's interrupt flag",
        "TIM1T" => "Starts the timer counting every cycle",
        "TIM8T" => "Starts the timer counting every 8 cycles",
        "TIM64T" => "Starts the timer counting every 64 cycles (43 for the vertical blank)",
        "T1024T" => "Starts the timer counting every 1024 cycles",
        _ => return None,
    })
}

// ---------- Disassembling ----------

#[derive(Clone, Debug)]
struct Insn {
    name: &'static str,
    mode: Mode,
    bytes: Vec<u8>,
    /// The address in its operand, for the absolute and zero page ones.
    operand: Option<u16>,
    target: Option<u16>,
    falls_through: bool,
}

fn decode(at: &dyn Fn(u16) -> Option<u8>, pc: u16) -> Option<Insn> {
    let op = at(pc)?;
    let (name, mode) = crate::asm::m6502::decode(op)?;
    let length = match mode {
        Mode::Implied | Mode::Accumulator => 1,
        Mode::Immediate | Mode::ZeroPage | Mode::ZeroPageX | Mode::ZeroPageY | Mode::IndirectX | Mode::IndirectY | Mode::Relative => 2,
        _ => 3,
    };
    let bytes: Vec<u8> = (0..length).map(|i| at(pc.wrapping_add(i))).collect::<Option<_>>()?;
    let operand = match length {
        2 if mode != Mode::Immediate && mode != Mode::Relative => Some(u16::from(bytes[1])),
        3 => Some(u16::from_le_bytes([bytes[1], bytes[2]])),
        _ => None,
    };
    let branch = (mode == Mode::Relative).then(|| pc.wrapping_add(2).wrapping_add(bytes[1] as i8 as u16));
    let (falls_through, target) = match name {
        "jmp" => (false, (mode == Mode::Absolute).then_some(operand.unwrap_or(0))),
        "rts" | "rti" | "brk" => (false, None),
        "jsr" => (true, operand),
        _ => (true, branch),
    };
    Some(Insn { name, mode, bytes, operand, target, falls_through })
}

fn writes(name: &str) -> bool {
    matches!(name, "sta" | "stx" | "sty" | "inc" | "dec" | "asl" | "lsr" | "rol" | "ror")
}

/// Disassembles a cartridge into DASM source.
pub fn disassemble(cart: &Cartridge) -> String {
    let slices = cart.slices();
    let multi = slices.iter().filter(|s| !s.data_only).count() > 1;
    // Where each slice's code is: (slice, address) → instruction; labels by slice too.
    let mut decoded: Vec<BTreeMap<u16, Insn>> = vec![BTreeMap::new(); slices.len()];
    let mut taken: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); slices.len()];
    let mut labels: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); slices.len()];
    let inside = |s: &Slice, a: u16| !s.data_only && a >= s.origin && u32::from(a) < u32::from(s.origin) + s.length as u32;
    // An address in the cartridge's space ($1000 with any mirror) as it is in a slice, if it's there.
    let in_slice = |s: &Slice, a: u16| -> Option<u16> {
        if a & 0x1000 == 0 || s.data_only {
            return None;
        }
        let moved = (s.origin & 0xE000) | (a & 0x1FFF);
        inside(s, moved).then_some(moved)
    };
    let byte = |s: &Slice, a: u16| -> Option<u8> { inside(s, a).then(|| cart.bytes[s.offset + usize::from(a - s.origin)]) };
    let ram = cart.scheme.ram();
    let mut queue: Vec<(usize, u16)> = Vec::new();
    for (i, s) in slices.iter().enumerate() {
        if s.data_only {
            continue;
        }
        // Each bank's reset and BRK vectors, where it has them.
        if s.origin as u32 + s.length as u32 == 0x10000 || (s.origin & 0x0FFF) + s.length as u16 == 0x1000 {
            let end = s.offset + s.length;
            for at in [end - 4, end - 2] {
                let v = u16::from_le_bytes([cart.bytes[at], cart.bytes[at + 1]]);
                if let Some(a) = in_slice(s, v) {
                    queue.push((i, a));
                    labels[i].insert(a);
                }
            }
        }
    }
    let hotspots = cart.scheme.hotspots();
    while let Some((i, mut pc)) = queue.pop() {
        let s = slices[i];
        loop {
            if !inside(&s, pc) || taken[i].contains(&pc) || pc.wrapping_sub(s.origin) < ram {
                break;
            }
            let Some(insn) = decode(&|a| byte(&s, a), pc) else { break };
            let length = insn.bytes.len() as u16;
            if (1..length).any(|k| taken[i].contains(&pc.wrapping_add(k))) {
                break;
            }
            for k in 0..length {
                taken[i].insert(pc.wrapping_add(k));
            }
            if let Some(t) = insn.target.and_then(|t| in_slice(&s, t)) {
                labels[i].insert(t);
                queue.push((i, t));
            }
            // A bank switch: the code carries on at the next address, in the bank chosen.
            let mut switched = false;
            if let (Some((first, banks)), Some(a)) = (hotspots, insn.operand.filter(|_| matches!(insn.mode, Mode::Absolute))) {
                let a = a & 0x1FFF;
                if (first..first + banks).contains(&a) {
                    let bank = usize::from(a - first);
                    let next = pc.wrapping_add(length);
                    if let Some(target) = slices.get(bank).and_then(|t| in_slice(t, next | 0x1000)) {
                        labels[bank].insert(target);
                        queue.push((bank, target));
                    }
                    switched = true;
                }
            }
            let carries_on = insn.falls_through && !switched;
            decoded[i].insert(pc, insn);
            if !carries_on {
                break;
            }
            pc = pc.wrapping_add(length);
        }
    }
    // Labels for addresses used in each slice: what's read or jumped to in it.
    for (i, s) in slices.iter().enumerate() {
        let used: Vec<u16> = decoded[i].values().filter(|d| matches!(d.mode, Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY | Mode::Indirect)).filter_map(|d| d.operand).filter_map(|a| in_slice(s, a)).collect();
        for a in used {
            // A label inside an instruction goes on the instruction.
            let mut back = a;
            while back > s.origin && taken[i].contains(&back) && !decoded[i].contains_key(&back) {
                back -= 1;
            }
            labels[i].insert(if decoded[i].contains_key(&back) { back } else { a });
        }
    }
    let name = |i: usize, a: u16| if multi { format!("L{}_{a:04X}", slices[i].bank) } else { format!("L{a:04X}") };
    // The registers and RAM used, for equates at the top.
    let mut registers: BTreeMap<u16, &str> = BTreeMap::new();
    let mut variables: BTreeSet<u16> = BTreeSet::new();
    for d in decoded.iter().flat_map(|m| m.values()) {
        let Some(a) = d.operand.filter(|_| !matches!(d.mode, Mode::Immediate | Mode::Relative | Mode::Indirect)) else { continue };
        if let Some(r) = register(a, writes(d.name)) {
            registers.insert(a | if writes(d.name) { 0x8000 } else { 0 }, r);
        } else if (0x80..=0xFF).contains(&a) && matches!(d.mode, Mode::ZeroPage | Mode::ZeroPageX | Mode::ZeroPageY | Mode::IndirectX | Mode::IndirectY) {
            variables.insert(a);
        }
    }
    let mut lines = vec![
        format!("; {}.", cart.describe()),
        "; Disassembled by Limoniter: it assembles with DASM or Limoniter's own assembler to the same bytes.".to_string(),
        "; Each instruction's cycles are beside it; a scanline has 76.".to_string(),
        String::new(),
        "        processor 6502".to_string(),
        String::new(),
    ];
    let mut written: BTreeSet<&str> = BTreeSet::new();
    if !registers.is_empty() {
        lines.push("; The TIA and the RIOT, as vcs.h names them".into());
        for (key, r) in &registers {
            if written.insert(r) {
                let a = key & 0x7FFF;
                let value = if a < 0x100 { format!("${a:02X}") } else { format!("${a:04X}") };
                let said = about(r).map(|t| format!("  ; {t}")).unwrap_or_default();
                lines.push(format!("{r:<8}= {value}{said}"));
            }
        }
        lines.push(String::new());
    }
    if !variables.is_empty() {
        lines.push("; RAM used".into());
        for v in &variables {
            lines.push(format!("ram_{v:02X}  = ${v:02X}"));
        }
        lines.push(String::new());
    }
    let operand_text = |i: usize, d: &Insn| -> String {
        let s = &slices[i];
        let address = |a: u16| -> String {
            match in_slice(s, a) {
                Some(m) if m == a && labels[i].contains(&a) => name(i, a),
                Some(m) if m == a => {
                    let mut back = a;
                    while back > s.origin && !decoded[i].contains_key(&back) && taken[i].contains(&back) {
                        back -= 1;
                    }
                    if back != a && labels[i].contains(&back) { format!("{}+{}", name(i, back), a - back) } else { format!("${a:04X}") }
                }
                _ => format!("${a:04X}"),
            }
        };
        let zp = |a: u16| -> String {
            if let Some(r) = register(a, writes(d.name)) {
                r.to_string()
            } else if variables.contains(&a) {
                format!("ram_{a:02X}")
            } else {
                format!("${a:02X}")
            }
        };
        let abs = |a: u16| -> String {
            // An absolute address that's a register or RAM is written as one with .w, as a cycle was meant.
            if a < 0x100 { zp(a) } else if let Some(r) = register(a, writes(d.name)) { r.to_string() } else { address(a) }
        };
        let b = &d.bytes;
        match d.mode {
            Mode::Implied => String::new(),
            Mode::Accumulator => String::new(),
            Mode::Immediate => format!("#${:02X}", b[1]),
            Mode::ZeroPage => zp(u16::from(b[1])),
            Mode::ZeroPageX => format!("{},x", zp(u16::from(b[1]))),
            Mode::ZeroPageY => format!("{},y", zp(u16::from(b[1]))),
            Mode::Absolute => abs(d.operand.unwrap_or(0)),
            Mode::AbsoluteX => format!("{},x", abs(d.operand.unwrap_or(0))),
            Mode::AbsoluteY => format!("{},y", abs(d.operand.unwrap_or(0))),
            Mode::Indirect => format!("({})", address(d.operand.unwrap_or(0))),
            Mode::IndirectX => format!("({},x)", zp(u16::from(b[1]))),
            Mode::IndirectY => format!("({}),y", zp(u16::from(b[1]))),
            Mode::Relative => address(d.target.unwrap_or(0)),
        }
    };
    for (i, s) in slices.iter().enumerate() {
        lines.push(String::new());
        if multi || s.data_only {
            lines.push(format!("; ---- {} {} ----", if s.data_only { "Graphics" } else { "Bank" }, s.bank));
            lines.push(format!("        seg bank{}", s.bank));
            lines.push(format!("        org ${:04X}", s.offset));
            if !s.data_only {
                lines.push(format!("        rorg ${:04X}", s.origin));
            }
        } else {
            lines.push("        seg code".into());
            lines.push(format!("        org ${:04X}", s.origin));
        }
        let end = u32::from(s.origin) + s.length as u32;
        let mut pc = u32::from(s.origin);
        // Cycles since the last WSYNC, along code that runs straight on.
        let mut since: Option<u32> = None;
        while pc < end {
            let a = pc as u16;
            if labels[i].contains(&a) {
                lines.push(format!("{}:", name(i, a)));
                since = None;
            }
            if let Some(d) = decoded[i].get(&a) {
                let mut operand = operand_text(i, d);
                // A zero page address written as absolute, to take a cycle more: .w keeps it so.
                let forced = matches!(d.mode, Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY) && d.operand.is_some_and(|o| o < 0x100);
                let mnemonic = if forced { format!("{}.w", d.name) } else { d.name.to_string() };
                if d.mode == Mode::Accumulator {
                    operand.clear();
                }
                let cycles = crate::m6502_timing::cycles(&d.bytes, Some(i64::from(a)));
                let mut comment = cycles.map(|c| c.text()).unwrap_or_default();
                if let (Some(total), Some(c)) = (since, cycles) {
                    let now = total + c.least;
                    since = Some(now);
                    if now > 76 {
                        comment += &format!("  past the scanline: {now} cycles since WSYNC");
                        since = None;
                    }
                }
                // A line starts when WSYNC lets the CPU go; code that jumps away is no longer counted.
                if writes(d.name) && d.operand.and_then(|o| register(o, true)) == Some("WSYNC") {
                    since = Some(0);
                }
                if !d.falls_through {
                    since = None;
                }
                let text = if operand.is_empty() { format!("        {mnemonic}") } else { format!("        {mnemonic:<8}{operand}") };
                lines.push(format!("{text:<40}; {comment}"));
                pc += d.bytes.len() as u32;
                continue;
            }
            // Data, up to the next label or code: graphics shown as pixels when it's a short run.
            let mut stop = pc + 1;
            while stop < end && !labels[i].contains(&(stop as u16)) && !decoded[i].contains_key(&(stop as u16)) {
                stop += 1;
            }
            let at = |x: u32| cart.bytes[s.offset + (x - u32::from(s.origin)) as usize];
            // Filler (16 or more of the same byte) is room left, written as DS.
            let same = (pc..stop).take_while(|&x| at(x) == at(pc)).count() as u32;
            if same >= 16 {
                lines.push(format!("        ds {same},${:02X}", at(pc)));
                since = None;
                pc += same;
                continue;
            }
            if let Some(fill) = (pc + 1..stop).find(|&x| (x..stop).take_while(|&y| at(y) == at(x)).count() >= 16) {
                stop = fill;
            }
            let run: Vec<u8> = (pc..stop).map(at).collect();
            let graphics = (2..=48).contains(&run.len()) && !run.iter().all(|&b| b == run[0]);
            if graphics {
                for b in &run {
                    let pixels: String = (0..8).rev().map(|bit| if b >> bit & 1 == 1 { '#' } else { '.' }).collect();
                    lines.push(format!("        .byte %{b:08b}                  ; {pixels}"));
                }
            } else {
                for chunk in run.chunks(8) {
                    lines.push(format!("        .byte {}", chunk.iter().map(|b| format!("${b:02X}")).collect::<Vec<_>>().join(",")));
                }
            }
            since = None;
            pc = stop;
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests;
