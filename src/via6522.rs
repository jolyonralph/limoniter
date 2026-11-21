//! The MOS 6522 Versatile Interface Adapter, a cycle at a time, as the built-in Vectrex has it: its two ports, two
//! timers (timer 1 able to drive PB7, which is how the Vectrex times its beam's moves), the shift register (whose
//! CB2 output turns the Vectrex's beam on and off as text and dotted lines are drawn), CA2 and CB2 as outputs, and the
//! interrupt flags and enables.
//!
//! The registers and what each does are Rockwell's R6522 data sheet (document 29651N87) and the Vectrex
//! Programmer's Manual's description of the 6522 as the Vectrex wires it (transcribed at
//! roadsidethoughts.com/vectrex/vectrex-6522-interface-adapter.htm). The shift register's φ2 rate (a bit every two
//! cycles, CB1 changing every cycle) is as MAME's 6522 (src/devices/machine/6522via.cpp) times it.

/// The interrupt flags' bits.
pub const IFR_CA2: u8 = 0x01;
pub const IFR_CA1: u8 = 0x02;
pub const IFR_SR: u8 = 0x04;
pub const IFR_CB2: u8 = 0x08;
pub const IFR_CB1: u8 = 0x10;
pub const IFR_T2: u8 = 0x20;
pub const IFR_T1: u8 = 0x40;

#[derive(Clone, Debug)]
pub struct Via {
    pub orb: u8,
    pub ora: u8,
    pub ddrb: u8,
    pub ddra: u8,
    t1_counter: u16,
    t1_latch: u16,
    /// Whether timer 1 will still flag reaching 0 (in one-shot mode it does so once for each start).
    t1_armed: bool,
    /// PB7 as timer 1 drives it when ACR bit 7 says so: low from the timer's start until it runs out.
    pub t1_pb7: bool,
    t2_counter: u16,
    t2_latch_low: u8,
    t2_armed: bool,
    pub sr: u8,
    /// Bits shifted since the shift register was last written or read (8 when it's done).
    sr_bits: u8,
    /// The shift clock's level, which changes every cycle at φ2's rate or as timer 2's low byte runs out.
    sr_clock: bool,
    /// Cycles until the shift clock changes, when timer 2 times it.
    sr_countdown: u16,
    /// CB2 as the shift register drives it: the last bit shifted out.
    pub cb2_shifted: bool,
    pub acr: u8,
    pub pcr: u8,
    pub ifr: u8,
    pub ier: u8,
    /// What's outside on each port's pins that are inputs.
    pub port_a_in: u8,
    pub port_b_in: u8,
}

impl Default for Via {
    fn default() -> Via {
        Via {
            orb: 0,
            ora: 0,
            ddrb: 0,
            ddra: 0,
            t1_counter: 0,
            t1_latch: 0,
            t1_armed: false,
            t1_pb7: true,
            t2_counter: 0,
            t2_latch_low: 0,
            t2_armed: false,
            sr: 0,
            sr_bits: 8,
            sr_clock: false,
            sr_countdown: 0,
            cb2_shifted: false,
            acr: 0,
            pcr: 0,
            ifr: 0,
            ier: 0,
            port_a_in: 0xFF,
            port_b_in: 0xFF,
        }
    }
}

impl Via {
    /// Whether the chip is asking for an interrupt (its IRQ line, active low on the chip, as true here).
    pub fn irq(&self) -> bool {
        self.ifr & self.ier & 0x7F != 0
    }

    fn flag(&mut self, bits: u8) {
        self.ifr |= bits;
    }

    fn clear(&mut self, bits: u8) {
        self.ifr &= !bits;
    }

    /// Port B as its pins have it: outputs as written (PB7 from timer 1 when ACR bit 7 gives it to the timer), the
    /// inputs as they come in.
    pub fn port_b(&self) -> u8 {
        let mut value = (self.orb & self.ddrb) | (self.port_b_in & !self.ddrb);
        if self.acr & 0x80 != 0 {
            value = (value & 0x7F) | if self.t1_pb7 { 0x80 } else { 0 };
        }
        value
    }

    /// Port A's pins.
    pub fn port_a(&self) -> u8 {
        (self.ora & self.ddra) | (self.port_a_in & !self.ddra)
    }

    /// CA2 as an output: low only when the PCR says to hold it low (bits 1-3 are 110).
    pub fn ca2(&self) -> bool {
        self.pcr & 0x0E != 0x0C
    }

    /// CB2: the shift register's when it's shifting out (ACR bits 2-4 are 1xx), else low only when the PCR holds
    /// it low (bits 5-7 are 110).
    pub fn cb2(&self) -> bool {
        if self.acr & 0x10 != 0 {
            return self.cb2_shifted;
        }
        self.pcr & 0xE0 != 0xC0
    }

    /// Reads a register (0 to 15), with what reading does: reading timer 1's or 2's low counter clears its flag;
    /// reading the shift register starts it again.
    pub fn read(&mut self, register: u8) -> u8 {
        match register & 0x0F {
            0x0 => {
                self.clear(IFR_CB1 | IFR_CB2);
                self.port_b()
            }
            0x1 => {
                self.clear(IFR_CA1 | IFR_CA2);
                self.port_a()
            }
            0xF => self.port_a(),
            0x2 => self.ddrb,
            0x3 => self.ddra,
            0x4 => {
                self.clear(IFR_T1);
                self.t1_counter as u8
            }
            0x5 => (self.t1_counter >> 8) as u8,
            0x6 => self.t1_latch as u8,
            0x7 => (self.t1_latch >> 8) as u8,
            0x8 => {
                self.clear(IFR_T2);
                self.t2_counter as u8
            }
            0x9 => (self.t2_counter >> 8) as u8,
            0xA => {
                self.clear(IFR_SR);
                self.start_shifting();
                self.sr
            }
            0xB => self.acr,
            0xC => self.pcr,
            0xD => {
                let any = if self.irq() { 0x80 } else { 0 };
                (self.ifr & 0x7F) | any
            }
            _ => self.ier | 0x80,
        }
    }

    pub fn write(&mut self, register: u8, value: u8) {
        match register & 0x0F {
            0x0 => {
                self.orb = value;
                self.clear(IFR_CB1 | IFR_CB2);
            }
            0x1 => {
                self.ora = value;
                self.clear(IFR_CA1 | IFR_CA2);
            }
            0xF => self.ora = value,
            0x2 => self.ddrb = value,
            0x3 => self.ddra = value,
            0x4 | 0x6 => self.t1_latch = (self.t1_latch & 0xFF00) | u16::from(value),
            0x5 => {
                // The latch to the counter, and the count begins: PB7 goes low until it runs out.
                self.t1_latch = (self.t1_latch & 0x00FF) | u16::from(value) << 8;
                self.t1_counter = self.t1_latch;
                self.clear(IFR_T1);
                self.t1_armed = true;
                self.t1_pb7 = false;
            }
            0x7 => {
                self.t1_latch = (self.t1_latch & 0x00FF) | u16::from(value) << 8;
                self.clear(IFR_T1);
            }
            0x8 => self.t2_latch_low = value,
            0x9 => {
                self.t2_counter = u16::from(value) << 8 | u16::from(self.t2_latch_low);
                self.clear(IFR_T2);
                self.t2_armed = true;
            }
            0xA => {
                self.sr = value;
                self.clear(IFR_SR);
                self.start_shifting();
            }
            0xB => self.acr = value,
            0xC => self.pcr = value,
            // Writing ones to the flags clears them.
            0xD => self.ifr &= !(value & 0x7F),
            // Bit 7 says whether the ones set or clear enables.
            _ => {
                if value & 0x80 != 0 {
                    self.ier |= value & 0x7F;
                } else {
                    self.ier &= !(value & 0x7F);
                }
            }
        }
    }

    fn start_shifting(&mut self) {
        self.sr_bits = 0;
        self.sr_clock = false;
        self.sr_countdown = u16::from(self.t2_latch_low) + 2;
    }

    /// One cycle of φ2: the timers count down, and the shift register shifts if its clock says so.
    pub fn tick(&mut self) {
        // Timer 1: on reaching 0 it flags, once in one-shot mode (PB7 going high again), or each time in
        // free-running mode, reloading from the latch and turning PB7 over.
        let (t1, rolled) = self.t1_counter.overflowing_sub(1);
        self.t1_counter = t1;
        if rolled {
            if self.acr & 0x40 != 0 {
                self.flag(IFR_T1);
                self.t1_pb7 = !self.t1_pb7;
                self.t1_counter = self.t1_latch;
            } else if self.t1_armed {
                self.flag(IFR_T1);
                self.t1_pb7 = true;
                self.t1_armed = false;
            }
        }
        // Timer 2 counts cycles unless ACR bit 5 has it count pulses on PB6, which the Vectrex doesn't use.
        if self.acr & 0x20 == 0 {
            let (t2, rolled) = self.t2_counter.overflowing_sub(1);
            self.t2_counter = t2;
            if rolled && self.t2_armed {
                self.flag(IFR_T2);
                self.t2_armed = false;
            }
        }
        self.shift();
    }

    /// The shift register's cycle. Its clock changes every cycle at φ2's rate (modes 2 and 6) or each time timer 2's
    /// low byte has counted down (modes 1, 4 and 5); a bit moves on each rising edge, so a bit takes two cycles at
    /// φ2's rate. Shifting out, the bit leaving bit 7 goes to CB2 and back in at bit 0.
    fn shift(&mut self) {
        let mode = (self.acr >> 2) & 7;
        let edge = match mode {
            2 | 6 => {
                self.sr_clock = !self.sr_clock;
                self.sr_clock
            }
            1 | 4 | 5 => {
                self.sr_countdown = self.sr_countdown.saturating_sub(1);
                if self.sr_countdown == 0 {
                    self.sr_countdown = u16::from(self.t2_latch_low) + 2;
                    self.sr_clock = !self.sr_clock;
                    self.sr_clock
                } else {
                    false
                }
            }
            // Disabled, or clocked from CB1, which isn't connected on a Vectrex.
            _ => false,
        };
        if !edge || (self.sr_bits >= 8 && mode != 4) {
            return;
        }
        match mode {
            4..=6 => {
                self.cb2_shifted = self.sr & 0x80 != 0;
                self.sr = self.sr.rotate_left(1);
            }
            // Shifting in: CB2 is the Vectrex's beam control, an output, so what comes in is taken as 0.
            _ => self.sr <<= 1,
        }
        if mode == 4 {
            return;
        }
        self.sr_bits += 1;
        if self.sr_bits == 8 {
            self.flag(IFR_SR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(via: &mut Via, n: usize) {
        for _ in 0..n {
            via.tick();
        }
    }

    #[test]
    fn timer_1_one_shot_flags_once_and_holds_pb7_low_while_it_counts() {
        let mut via = Via { acr: 0x80, ddrb: 0xFF, orb: 0xFF, ..Via::default() };
        via.write(0x4, 10);
        assert!(via.port_b() & 0x80 != 0, "PB7 is high until the timer starts");
        via.write(0x5, 0);
        assert_eq!(via.port_b() & 0x80, 0, "low as it starts");
        ticks(&mut via, 10);
        assert_eq!((via.ifr & IFR_T1, via.port_b() & 0x80), (0, 0), "still counting after 10 cycles");
        ticks(&mut via, 1);
        assert_eq!(via.ifr & IFR_T1, IFR_T1, "flagged as it passes 0");
        assert!(via.port_b() & 0x80 != 0, "and PB7 is high again");
        // Reading the low counter clears the flag; it doesn't flag again until it's started again.
        via.read(0x4);
        assert_eq!(via.ifr & IFR_T1, 0);
        ticks(&mut via, 70_000);
        assert_eq!(via.ifr & IFR_T1, 0, "one shot");
        // Without ACR bit 7, PB7 is the port's own.
        via.acr = 0;
        via.orb = 0x7F;
        via.write(0x5, 0);
        assert_eq!(via.port_b() & 0x80, 0, "written low");
    }

    #[test]
    fn timer_1_free_running_reloads_and_turns_pb7_over() {
        let mut via = Via { acr: 0xC0, ..Via::default() };
        via.write(0x4, 4);
        via.write(0x5, 0);
        let mut edges = Vec::new();
        let mut was = via.t1_pb7;
        for cycle in 1..=30 {
            via.tick();
            if via.t1_pb7 != was {
                edges.push(cycle);
                was = via.t1_pb7;
            }
        }
        // N + 1 cycles for the first, then N + 1 again each reload (4, 3, 2, 1, 0 and on through $FFFF to the latch).
        assert_eq!(edges, vec![5, 10, 15, 20, 25, 30]);
    }

    #[test]
    fn timer_2_counts_down_and_flags_once() {
        let mut via = Via::default();
        via.write(0x8, 0x30);
        via.write(0x9, 0x75);
        assert_eq!((via.read(0x9), via.clone().read(0x8)), (0x75, 0x30));
        ticks(&mut via, 30_000);
        assert_eq!(via.ifr & IFR_T2, 0);
        ticks(&mut via, 1);
        assert_eq!(via.ifr & IFR_T2, IFR_T2, "30,000 cycles and one: a Vectrex frame, at 1.5 MHz, is 20 ms");
        assert!(!via.irq(), "not enabled, so no interrupt");
        via.write(0xE, 0x80 | IFR_T2);
        assert!(via.irq());
        assert_eq!(via.read(0xD), 0x80 | IFR_T2);
        assert_eq!(via.read(0xE), 0x80 | IFR_T2);
        via.write(0xD, IFR_T2);
        assert!(!via.irq(), "cleared by writing its bit");
        ticks(&mut via, 70_000);
        assert_eq!(via.ifr & IFR_T2, 0, "it doesn't flag again until it's started again");
        via.write(0xE, IFR_T2);
        assert_eq!(via.ier, 0, "and the enable is cleared with bit 7 clear");
    }

    #[test]
    fn the_shift_register_puts_its_bits_on_cb2_a_bit_every_two_cycles() {
        // ACR $98, as the Vectrex's BIOS sets it: timer 1 on PB7, shifting out at φ2's rate.
        let mut via = Via { acr: 0x98, pcr: 0xCE, ..Via::default() };
        assert!(!via.cb2(), "the shift register has CB2");
        via.write(0xA, 0b1010_0011);
        let mut seen = Vec::new();
        for _ in 0..16 {
            via.tick();
            seen.push(u8::from(via.cb2()));
        }
        assert_eq!(seen, vec![1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(via.ifr & IFR_SR, IFR_SR, "done after eight bits");
        assert_eq!(via.sr, 0b1010_0011, "the bits went round");
        ticks(&mut via, 10);
        assert!(via.cb2(), "CB2 stays at the last bit");
        // $FF keeps the beam on until $00 is written, as the BIOS draws a line.
        via.write(0xA, 0xFF);
        ticks(&mut via, 40);
        assert!(via.cb2());
        via.write(0xA, 0);
        ticks(&mut via, 2);
        assert!(!via.cb2());
        // Without the shift register, the PCR holds CB2: low with $CE, high with $EE.
        via.acr = 0x80;
        assert!(!via.cb2());
        via.pcr = 0xEE;
        assert!(via.cb2());
        assert!(via.ca2());
        via.pcr = 0xCC;
        assert!(!via.ca2(), "CA2 held low: the Vectrex's integrators zeroed");
    }

    #[test]
    fn the_shift_register_under_timer_2_shifts_at_its_rate() {
        let mut via = Via { acr: 0x14, ..Via::default() };
        via.write(0x8, 2);
        via.write(0xA, 0x80);
        // Each half of the clock is the low latch and 2 cycles: the first bit after 4, then a bit every 8.
        ticks(&mut via, 3);
        assert!(!via.cb2_shifted);
        ticks(&mut via, 1);
        assert!(via.cb2_shifted, "the first bit");
        ticks(&mut via, 8);
        assert!(!via.cb2_shifted);
        ticks(&mut via, 6 * 8);
        assert_eq!(via.ifr & IFR_SR, IFR_SR);
    }

    #[test]
    fn ports_mix_outputs_with_what_comes_in() {
        let mut via = Via { ddrb: 0x9F, port_b_in: 0x20, ..Via::default() };
        via.write(0x0, 0xFF);
        assert_eq!(via.read(0x0), 0x9F | 0x20, "PB5 and PB6 are inputs, as on a Vectrex");
        via.write(0x3, 0x00);
        via.port_a_in = 0x5A;
        via.write(0x1, 0xFF);
        assert_eq!(via.read(0x1), 0x5A, "port A as an input reads what's put on it");
        via.write(0x3, 0xFF);
        assert_eq!(via.read(0xF), 0xFF);
    }
}
