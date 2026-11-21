//! The PDP-8, built into Limoniter for teaching: Digital Equipment Corporation's 12-bit minicomputer of 1965, the
//! machine generations of people learnt on.
//!
//! It is built here from DEC's own descriptions (the PDP-8 Users Handbook and "Introduction to Programming"), as
//! Wikipedia's "PDP-8" summarises them:
//!
//! - 4096 words of 12-bit core memory, in 32 pages of 128 words; a 12-bit accumulator (AC) with a link bit (L)
//!   above it; the program counter (PC); and the switch register (SR) on the front panel. The memory cycle is 1.5
//!   microseconds: a fetch, then a defer cycle for an indirect address, then an execute cycle.
//! - its eight instructions, by bits 0 to 2: AND, TAD, ISZ, DCA, JMS and JMP, which refer to memory; IOT, which
//!   talks to devices; and OPR, the operate microinstructions. A memory reference has an indirect bit (3), a page
//!   bit (4: page zero, or the page the instruction is on) and a 7-bit offset (5 to 11). Words 0010 to 0017 are
//!   auto-index registers: an indirect reference through one adds one to it first.
//! - the operate groups. Group 1 (bit 3 clear) clears (CLA, CLL), then complements (CMA, CML), then increments
//!   (IAC), then rotates (RAR, RAL, and with bit 10 twice: RTR, RTL), so CLA CLL IAC RAL leaves 2. Group 2 (bit 3
//!   set, bit 11 clear) skips if any of SMA, SZA and SNL holds, or with bit 8 set if all of SPA, SNA and SZL do (SKP
//!   alone always), then clears AC (CLA), ORs in the switches (OSR) and halts (HLT). Group 3 (bits 3 and 11 set)
//!   works the extended arithmetic element's MQ, which the PDP-8 of 1965 had only as an option; here only its CLA
//!   is done. The byte swap (BSW) came with the PDP-8/E in 1970, so bit 10 alone does nothing here.
//! - the interrupt: with interrupts on (ION, which takes effect after the next instruction), a device's flag makes
//!   the processor store PC at 0, turn interrupts off and go on at 1.
//! - the console Teletype, a Model 33 ASR, at ten characters a second: its keyboard (and paper tape reader) as
//!   device 03 (KSF 6031 skips when a character is waiting, KCC 6032 clears AC and the flag, KRS 6034 ORs the
//!   character into AC, KRB 6036 does both) and its printer as device 04 (TSF 6041 skips when it's ready, TCF 6042
//!   clears the flag, TPC 6044 prints AC's character, TLS 6046 does both).
//! - paper tape in RIM and BIN formats, as DEC's loaders read them: RIM gives each word its address (a frame with
//!   bit 7 set, the address's top six bits, then its low six) then the word, in two frames; BIN gives an origin
//!   once, then words in pairs of frames, with a checksum of every frame as the last word. Leader and trailer are
//!   frames of 0200.
//!
//! Limoniter's PAL assembler (see `pal8`) assembles source for it.

use std::collections::VecDeque;

pub const WORDS: usize = 4096;
const MASK: u16 = 0o7777;
/// How long things take, in nanoseconds: the memory cycle, and an IOT instruction (the PDP-8's 4.25 microseconds).
pub const CYCLE_NS: u64 = 1_500;
pub const IOT_NS: u64 = 4_250;
/// A character on the Model 33 Teletype: a tenth of a second.
pub const CHARACTER_NS: u64 = 100_000_000;

/// The processor's major state, as the front panel's lights show it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Fetch,
    Defer,
    Execute,
}

/// The machine.
#[derive(Clone, Debug)]
pub struct Pdp8 {
    pub memory: Vec<u16>,
    pub ac: u16,
    pub link: bool,
    pub pc: u16,
    /// The memory address and memory buffer registers, and the instruction register, as the lights show them.
    pub ma: u16,
    pub mb: u16,
    pub ir: u16,
    pub state: State,
    /// The switch register.
    pub sr: u16,
    /// Whether it's running (the RUN light), and whether it was stopped by HLT.
    pub run: bool,
    pub halted: bool,
    pub ion: bool,
    ion_soon: bool,
    /// The keyboard (and reader): characters waiting to be read, the one in the buffer, and its flag.
    pub keyboard: VecDeque<u8>,
    pub key_buffer: u8,
    pub key_flag: bool,
    next_key_at: u64,
    /// The printer: everything printed, its flag, and when the character it's printing will be done.
    pub printed: Vec<u8>,
    pub print_flag: bool,
    printing_until: Option<u64>,
    /// Time since it was switched on, in nanoseconds, and instructions done.
    pub time: u64,
    pub done: u64,
}

impl Default for Pdp8 {
    fn default() -> Self {
        Pdp8 { memory: vec![0; WORDS], ac: 0, link: false, pc: 0o200, ma: 0, mb: 0, ir: 0, state: State::Fetch, sr: 0, run: false, halted: false, ion: false, ion_soon: false, keyboard: VecDeque::new(), key_buffer: 0, key_flag: false, next_key_at: 0, printed: Vec::new(), print_flag: false, printing_until: None, time: 0, done: 0 }
    }
}

impl Pdp8 {
    /// Words put in memory.
    pub fn load(&mut self, words: &[(u16, u16)]) {
        for (address, word) in words {
            self.memory[usize::from(*address & MASK)] = *word & MASK;
        }
    }

    /// The devices catch up with the time: the printer finishing, and the next key or tape character arriving.
    fn devices(&mut self) {
        if let Some(until) = self.printing_until
            && self.time >= until
        {
            self.printing_until = None;
            self.print_flag = true;
        }
        if !self.key_flag
            && self.time >= self.next_key_at
            && let Some(c) = self.keyboard.pop_front()
        {
            self.key_buffer = c;
            self.key_flag = true;
            self.next_key_at = self.time + CHARACTER_NS;
        }
    }

    /// Does one instruction (taking an interrupt first, if one is waiting).
    pub fn step(&mut self) {
        self.devices();
        if self.ion && (self.key_flag || self.print_flag) {
            self.ion = false;
            self.memory[0] = self.pc;
            self.pc = 1;
            self.time += CYCLE_NS;
        }
        if self.ion_soon {
            self.ion_soon = false;
            self.ion = true;
        }
        self.ma = self.pc;
        self.mb = self.memory[usize::from(self.pc)];
        self.state = State::Fetch;
        self.ir = self.mb >> 9;
        self.pc = (self.pc + 1) & MASK;
        self.time += CYCLE_NS;
        self.done += 1;
        let instruction = self.mb;
        match self.ir {
            0..=5 => self.memory_reference(instruction),
            6 => {
                self.iot(instruction);
                self.time += IOT_NS - CYCLE_NS;
            }
            _ => self.operate(instruction),
        }
    }

    /// Runs (if it's running) for at most `ns` nanoseconds of its time, or `most` instructions, stopping early at
    /// any of `breakpoints`; whether it stopped at one.
    pub fn run_for(&mut self, ns: u64, most: u64, breakpoints: &[u16]) -> bool {
        let until = self.time + ns;
        let mut count = 0;
        while self.run && self.time < until && count < most {
            self.step();
            count += 1;
            if breakpoints.contains(&self.pc) {
                self.run = false;
                return true;
            }
        }
        false
    }

    fn memory_reference(&mut self, instruction: u16) {
        let mut address = instruction & 0o177;
        if instruction & 0o200 != 0 {
            address |= self.ma & 0o7600;
        }
        if instruction & 0o400 != 0 {
            self.state = State::Defer;
            self.time += CYCLE_NS;
            if (0o10..=0o17).contains(&address) {
                let at = usize::from(address);
                self.memory[at] = (self.memory[at] + 1) & MASK;
            }
            address = self.memory[usize::from(address)];
        }
        let at = usize::from(address);
        if self.ir != 5 {
            self.state = State::Execute;
            self.time += CYCLE_NS;
            self.ma = address;
        }
        match self.ir {
            0 => self.ac &= self.memory[at],
            1 => {
                let sum = self.ac + self.memory[at];
                if sum > MASK {
                    self.link = !self.link;
                }
                self.ac = sum & MASK;
            }
            2 => {
                self.memory[at] = (self.memory[at] + 1) & MASK;
                if self.memory[at] == 0 {
                    self.pc = (self.pc + 1) & MASK;
                }
            }
            3 => {
                self.memory[at] = self.ac;
                self.ac = 0;
            }
            4 => {
                self.memory[at] = self.pc;
                self.pc = (address + 1) & MASK;
            }
            _ => self.pc = address,
        }
        if self.ir != 5 {
            self.mb = self.memory[at];
        }
    }

    fn iot(&mut self, instruction: u16) {
        let device = (instruction >> 3) & 0o77;
        let pulses = instruction & 7;
        match device {
            0 => match pulses {
                1 => self.ion_soon = true,
                2 => {
                    self.ion = false;
                    self.ion_soon = false;
                }
                _ => {}
            },
            // The keyboard and reader: IOP1 skips on the flag, IOP2 clears AC and the flag, IOP4 reads.
            0o3 => {
                if pulses & 1 != 0 && self.key_flag {
                    self.pc = (self.pc + 1) & MASK;
                }
                if pulses & 2 != 0 {
                    self.ac = 0;
                    self.key_flag = false;
                }
                if pulses & 4 != 0 {
                    self.ac |= u16::from(self.key_buffer);
                }
            }
            // The printer: IOP1 skips on the flag, IOP2 clears it, IOP4 prints.
            0o4 => {
                if pulses & 1 != 0 && self.print_flag {
                    self.pc = (self.pc + 1) & MASK;
                }
                if pulses & 2 != 0 {
                    self.print_flag = false;
                }
                if pulses & 4 != 0 {
                    self.printed.push((self.ac & 0o377) as u8);
                    self.printing_until = Some(self.time + CHARACTER_NS);
                }
            }
            _ => {}
        }
    }

    fn operate(&mut self, instruction: u16) {
        if instruction & 0o400 == 0 {
            // Group 1: clear, complement, increment, rotate.
            if instruction & 0o200 != 0 {
                self.ac = 0;
            }
            if instruction & 0o100 != 0 {
                self.link = false;
            }
            if instruction & 0o40 != 0 {
                self.ac ^= MASK;
            }
            if instruction & 0o20 != 0 {
                self.link = !self.link;
            }
            if instruction & 1 != 0 {
                let sum = self.ac + 1;
                if sum > MASK {
                    self.link = !self.link;
                }
                self.ac = sum & MASK;
            }
            let times = if instruction & 2 != 0 { 2 } else { 1 };
            match instruction & 0o14 {
                0o10 => (0..times).for_each(|_| self.rotate_right()),
                0o4 => (0..times).for_each(|_| self.rotate_left()),
                _ => {}
            }
        } else if instruction & 1 == 0 {
            // Group 2: skip, then clear, then the switches, then halt.
            let any = (instruction & 0o100 != 0 && self.ac & 0o4000 != 0) || (instruction & 0o40 != 0 && self.ac == 0) || (instruction & 0o20 != 0 && self.link);
            let skip = if instruction & 0o10 != 0 { !any } else { any };
            if skip {
                self.pc = (self.pc + 1) & MASK;
            }
            if instruction & 0o200 != 0 {
                self.ac = 0;
            }
            if instruction & 4 != 0 {
                self.ac |= self.sr;
            }
            if instruction & 2 != 0 {
                self.run = false;
                self.halted = true;
            }
        } else if instruction & 0o200 != 0 {
            // Group 3, without the extended arithmetic element: its CLA only.
            self.ac = 0;
        }
    }

    fn rotate_right(&mut self) {
        let out = self.ac & 1 != 0;
        self.ac = (self.ac >> 1) | if self.link { 0o4000 } else { 0 };
        self.link = out;
    }

    fn rotate_left(&mut self) {
        let out = self.ac & 0o4000 != 0;
        self.ac = ((self.ac << 1) & MASK) | u16::from(self.link);
        self.link = out;
    }

    // The front panel's keys.

    /// LOAD ADD: the switches into PC.
    pub fn load_address(&mut self) {
        self.pc = self.sr & MASK;
        self.ma = self.pc;
    }

    /// DEP: the switches into the word at PC, which then moves on.
    pub fn deposit(&mut self) {
        self.ma = self.pc;
        self.mb = self.sr & MASK;
        self.memory[usize::from(self.pc)] = self.mb;
        self.pc = (self.pc + 1) & MASK;
    }

    /// EXAM: the word at PC into the lights, and PC moves on.
    pub fn examine(&mut self) {
        self.ma = self.pc;
        self.mb = self.memory[usize::from(self.pc)];
        self.pc = (self.pc + 1) & MASK;
    }

    /// START: clears AC, the link and the flags, and runs from PC.
    pub fn start(&mut self) {
        self.ac = 0;
        self.link = false;
        self.ion = false;
        self.ion_soon = false;
        self.key_flag = false;
        self.print_flag = false;
        self.halted = false;
        self.run = true;
    }

    /// CONT: runs on from where it stopped.
    pub fn carry_on(&mut self) {
        self.halted = false;
        self.run = true;
    }

    /// A key typed, or a tape character read: with the top bit set, as the Teletype sends it.
    pub fn type_key(&mut self, c: u8) {
        self.keyboard.push_back(c | 0o200);
    }

    /// What the teleprinter has printed, as text: seven-bit ASCII, carriage returns dropped.
    pub fn teletype_text(&self) -> String {
        self.printed.iter().map(|c| c & 0o177).filter(|c| *c != 0o15 && *c != 0 && *c != 0o177).map(char::from).collect()
    }
}

/// A word in octal, four digits, as the PDP-8's are written.
pub fn octal(word: u16) -> String {
    format!("{:04o}", word & MASK)
}

/// Words as a BIN tape: leader, an origin wherever the addresses jump, the words, the checksum and trailer.
pub fn bin_tape(words: &[(u16, u16)]) -> Vec<u8> {
    let mut tape = vec![0o200u8; 64];
    let mut sum: u32 = 0;
    let frame = |tape: &mut Vec<u8>, byte: u8, sum: &mut u32| {
        tape.push(byte);
        *sum += u32::from(byte);
    };
    let mut next: Option<u16> = None;
    for (address, word) in words {
        if next != Some(*address) {
            frame(&mut tape, 0o100 | (address >> 6 & 0o77) as u8, &mut sum);
            frame(&mut tape, (address & 0o77) as u8, &mut sum);
        }
        frame(&mut tape, (word >> 6 & 0o77) as u8, &mut sum);
        frame(&mut tape, (word & 0o77) as u8, &mut sum);
        next = Some((address + 1) & MASK);
    }
    let check = (sum & 0o7777) as u16;
    tape.push((check >> 6 & 0o77) as u8);
    tape.push((check & 0o77) as u8);
    tape.extend(std::iter::repeat_n(0o200u8, 64));
    tape
}

/// Words as a RIM tape: each with its address.
#[cfg(test)]
pub fn rim_tape(words: &[(u16, u16)]) -> Vec<u8> {
    let mut tape = vec![0o200u8; 64];
    for (address, word) in words {
        tape.extend([0o100 | (address >> 6 & 0o77) as u8, (address & 0o77) as u8, (word >> 6 & 0o77) as u8, (word & 0o77) as u8]);
    }
    tape.extend(std::iter::repeat_n(0o200u8, 64));
    tape
}

/// The words on a BIN tape, its checksum checked.
pub fn read_bin(tape: &[u8]) -> Result<Vec<(u16, u16)>, String> {
    // Leader first, then frames until the trailer; rubouts (0377) and field settings (11xxx000) are passed over.
    let frames: Vec<u8> = tape.iter().copied().skip_while(|b| *b == 0o200).take_while(|b| *b != 0o200).filter(|b| *b != 0o377 && b & 0o300 != 0o300).collect();
    if frames.len() < 2 || !frames.len().is_multiple_of(2) {
        return Err(crate::i18n::tr("This isn't a BIN tape: its frames don't come in pairs between the leader and the trailer.").into());
    }
    let pairs: Vec<(u8, u8)> = frames.chunks(2).map(|p| (p[0], p[1])).collect();
    let (last, body) = pairs.split_last().expect("a pair");
    let mut words = Vec::new();
    let mut sum: u32 = 0;
    let mut at: u16 = 0;
    for (high, low) in body {
        sum += u32::from(*high) + u32::from(*low);
        let value = (u16::from(high & 0o77) << 6) | u16::from(low & 0o77);
        if high & 0o100 != 0 {
            at = value;
        } else {
            words.push((at, value));
            at = (at + 1) & MASK;
        }
    }
    let check = (u16::from(last.0 & 0o77) << 6) | u16::from(last.1 & 0o77);
    if (sum & 0o7777) as u16 != check {
        return Err(crate::i18n::tf!("This BIN tape's checksum is wrong (it says {}, and the tape adds up to {}).", octal(check), octal((sum & 0o7777) as u16)));
    }
    Ok(words)
}

/// The words on a RIM tape.
pub fn read_rim(tape: &[u8]) -> Result<Vec<(u16, u16)>, String> {
    let frames: Vec<u8> = tape.iter().copied().skip_while(|b| *b == 0o200).take_while(|b| *b != 0o200).filter(|b| *b != 0o377).collect();
    if frames.is_empty() || !frames.len().is_multiple_of(4) || frames.chunks(4).any(|f| f[0] & 0o100 == 0 || f[2] & 0o100 != 0) {
        return Err(crate::i18n::tr("This isn't a RIM tape: each word should have its address before it.").into());
    }
    Ok(frames.chunks(4).map(|f| ((u16::from(f[0] & 0o77) << 6) | u16::from(f[1] & 0o77), (u16::from(f[2] & 0o77) << 6) | u16::from(f[3] & 0o77))).collect())
}

/// The words on a tape of either kind: RIM if every word has its address, else BIN.
pub fn read_tape(tape: &[u8]) -> Result<Vec<(u16, u16)>, String> {
    read_rim(tape).or_else(|_| read_bin(tape))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with words from 0200, the switches set, started there and run until it halts.
    fn run(words: &[u16], sr: u16) -> Pdp8 {
        let mut pdp = Pdp8 { sr, ..Pdp8::default() };
        pdp.load(&words.iter().enumerate().map(|(i, w)| (0o200 + i as u16, *w)).collect::<Vec<_>>());
        pdp.pc = 0o200;
        pdp.start();
        pdp.run_for(u64::MAX / 2, 100_000, &[]);
        pdp
    }

    #[test]
    fn group_one_works_in_its_order() {
        // Wikipedia's example: CLA CLL IAC RAL leaves 2, as the increment comes before the rotate.
        let pdp = run(&[0o7305, 0o7402], 0);
        assert_eq!(pdp.ac, 2);
        // CMA IAC (CIA) negates: 5 becomes 7773.
        let pdp = run(&[0o7200, 0o1205, 0o7041, 0o7402, 0, 5], 0);
        assert_eq!(pdp.ac, 0o7773);
        // STL (CLL CML) sets the link; RAR moves it into the top bit.
        let pdp = run(&[0o7320, 0o7010, 0o7402], 0);
        assert_eq!((pdp.ac, pdp.link), (0o4000, false));
        // RTL rotates two places, through the link: 4000 goes to the link then to bit 11.
        let pdp = run(&[0o7300, 0o1204, 0o7006, 0o7402, 0o4000], 0);
        assert_eq!((pdp.ac, pdp.link), (1, false));
        // IAC carrying out of 7777 complements the link and leaves 0.
        let pdp = run(&[0o7340, 0o7001, 0o7402], 0);
        assert_eq!((pdp.ac, pdp.link), (0, true));
    }

    #[test]
    fn group_two_skips_and_halts() {
        // SZA skips on zero; SNA (reversed) doesn't; SMA on negative; SPA SNA skips only when AC is above zero.
        let skipped = |setup: &[u16], test: u16| {
            let mut words = setup.to_vec();
            words.extend([test, 0o7402, 0o7402]);
            // Skipped, it halts at the second HLT.
            run(&words, 0).pc == 0o200 + setup.len() as u16 + 3
        };
        assert!(skipped(&[0o7300], 0o7440), "SZA with 0");
        assert!(!skipped(&[0o7300], 0o7450), "SNA with 0");
        assert!(skipped(&[0o7240], 0o7500), "SMA with 7777");
        assert!(!skipped(&[0o7240], 0o7550), "SPA SNA with 7777");
        assert!(skipped(&[0o7301], 0o7550), "SPA SNA with 1");
        assert!(skipped(&[0o7300], 0o7410), "SKP");
        assert!(skipped(&[0o7120], 0o7420), "SNL with the link set");
        assert!(skipped(&[0o7100], 0o7430), "SZL with the link clear");
        // OSR ORs the switches in; LAS (CLA OSR) loads them; HLT stops it, with the RUN light off.
        let pdp = run(&[0o7604, 0o7402], 0o1234);
        assert_eq!(pdp.ac, 0o1234);
        assert!(pdp.halted && !pdp.run);
        assert_eq!(pdp.pc, 0o202);
    }

    #[test]
    fn memory_reference_instructions_and_their_addresses() {
        // TAD 0210 (this page), DCA 0050 (page zero), TAD I 0211 (through a pointer), AND, ISZ skipping at zero.
        let words = [
            0o1210, // 200 TAD 210: AC = 7
            0o3050, // 201 DCA 50: page zero
            0o1611, // 202 TAD I 211: the word at 0212 = 3
            0o0213, // 203 AND 213: & 2
            0o2214, // 204 ISZ 214: 7777 + 1 = 0, so skip
            0o7402, // 205
            0o7402, // 206 HLT
            0,
            0o7,    // 210
            0o212,  // 211 a pointer
            0o3,    // 212
            0o2,    // 213
            0o7777, // 214
        ];
        let pdp = run(&words, 0);
        assert_eq!(pdp.memory[0o50], 7);
        assert_eq!(pdp.ac, 2);
        assert_eq!(pdp.memory[0o214], 0);
        assert_eq!(pdp.pc, 0o207, "ISZ skipped the first HLT");
        // TAD's carry out complements the link.
        let pdp = run(&[0o1203, 0o1203, 0o7402, 0o4000], 0);
        assert_eq!((pdp.ac, pdp.link), (0, true));
    }

    #[test]
    fn jms_keeps_the_return_and_jmp_i_goes_back() {
        // JMS 210 stores 201 at 210 and goes on at 211; the subroutine adds 1 and returns with JMP I 210.
        let words = [0o4210, 0o7402, 0, 0, 0, 0, 0, 0, 0, 0o7001, 0o5610];
        let pdp = run(&words, 0);
        assert_eq!(pdp.memory[0o210], 0o201);
        assert_eq!(pdp.ac, 1);
        assert_eq!(pdp.pc, 0o202);
    }

    #[test]
    fn auto_index_registers_count_up_before_use() {
        // 0010 holds 0277; TAD I 10 reads 0300 and leaves 0010 at 0300.
        let mut words = vec![0o1410, 0o1410, 0o7402];
        words.resize(0o100, 0);
        words.extend([5, 6]);
        let mut pdp = Pdp8::default();
        pdp.load(&words.iter().enumerate().map(|(i, w)| (0o200 + i as u16, *w)).collect::<Vec<_>>());
        pdp.memory[0o10] = 0o277;
        pdp.pc = 0o200;
        pdp.start();
        pdp.run_for(1_000_000, 100, &[]);
        assert_eq!(pdp.ac, 11);
        assert_eq!(pdp.memory[0o10], 0o301);
    }

    #[test]
    fn time_goes_by_memory_cycles() {
        // A direct TAD is two cycles, an indirect one three, JMP one, an operate one.
        let pdp = run(&[0o1203, 0o1603, 0o7402, 0o203], 0);
        assert_eq!(pdp.time, (2 + 3 + 1) * CYCLE_NS);
    }

    #[test]
    fn the_teletype_prints_and_reads_at_its_own_speed() {
        // TLS, then wait on TSF, then KSF/KRB a key, then halt.
        let words = [0o7200, 0o1212, 0o6046, 0o6041, 0o5203, 0o6031, 0o5205, 0o6036, 0o7402, 0, 0o301];
        let mut pdp = Pdp8::default();
        pdp.load(&words.iter().enumerate().map(|(i, w)| (0o200 + i as u16, *w)).collect::<Vec<_>>());
        pdp.pc = 0o200;
        pdp.start();
        pdp.type_key(b'X');
        pdp.run_for(u64::MAX / 2, 10_000_000, &[]);
        assert_eq!(pdp.teletype_text(), "A");
        assert_eq!(pdp.ac, 0o200 | u16::from(b'X'));
        // The printer took a tenth of a second.
        assert!(pdp.time >= CHARACTER_NS && pdp.time < CHARACTER_NS + 1_000_000, "{}", pdp.time);
    }

    #[test]
    fn an_interrupt_stores_the_pc_at_zero_and_goes_on_at_one() {
        // ION, then a loop; the keyboard's flag interrupts it.
        let mut pdp = Pdp8::default();
        pdp.load(&[(0o200, 0o6001), (0o201, 0o5201), (1, 0o7402)]);
        pdp.pc = 0o200;
        pdp.start();
        pdp.type_key(b'A');
        pdp.run_for(1_000_000, 100, &[]);
        assert!(pdp.halted);
        assert_eq!(pdp.memory[0], 0o201);
        assert!(!pdp.ion);
    }

    #[test]
    fn the_front_panel_deposits_and_examines() {
        let mut pdp = Pdp8 { sr: 0o300, ..Pdp8::default() };
        pdp.load_address();
        pdp.sr = 0o7001;
        pdp.deposit();
        pdp.sr = 0o7402;
        pdp.deposit();
        assert_eq!(pdp.memory[0o300..0o302], [0o7001, 0o7402]);
        pdp.sr = 0o300;
        pdp.load_address();
        pdp.examine();
        assert_eq!((pdp.ma, pdp.mb, pdp.pc), (0o300, 0o7001, 0o301));
        pdp.pc = 0o300;
        pdp.start();
        pdp.run_for(1_000_000, 10, &[]);
        assert_eq!(pdp.ac, 1);
    }

    #[test]
    fn bin_and_rim_tapes_go_both_ways() {
        let words = vec![(0o200, 0o7300), (0o201, 0o1205), (0o202, 0o7402), (0o400, 0o1234)];
        let bin = bin_tape(&words);
        assert_eq!(read_bin(&bin).unwrap(), words);
        assert_eq!(read_tape(&bin).unwrap(), words);
        // The origin frame has bit 7 (0100) set: 0200 is 01 02 then 00 00.
        assert_eq!(bin[64..66], [0o102, 0o00]);
        let rim = rim_tape(&words);
        assert_eq!(read_rim(&rim).unwrap(), words);
        assert_eq!(read_tape(&rim).unwrap(), words);
        // A frame changed spoils the checksum.
        let mut spoilt = bin.clone();
        spoilt[67] ^= 1;
        assert!(read_bin(&spoilt).is_err());
    }
}
