//! The Atari ST family's hardware, for demo coders: the registers of the ST, Mega ST, STE, Mega STE, TT and
//! Falcon at $FF8000-$FFFFFF, each with its size, whether it's read or written, which machines have it and what
//! its bits mean; how to read a value written to one (a colour in the ST's and the STE's palettes, a timer's
//! mode, a blitter operation); the YM2149's sixteen registers behind $FF8800; and the MFP's timers worked out.
//!
//! Names, addresses and bit positions are the hardware's (as Atari named them, checked against Hatari's and
//! EmuTOS's sources); the words describing them are Limoniter's own.

use std::sync::OnceLock;

/// The machines, as bits of a set.
pub const ST: u8 = 1;
pub const MEGA_ST: u8 = 2;
pub const STE: u8 = 4;
pub const MEGA_STE: u8 = 8;
pub const TT: u8 = 16;
pub const FALCON: u8 = 32;
/// Every machine of the family.
pub const EVERY: u8 = ST | MEGA_ST | STE | MEGA_STE | TT | FALCON;
/// The STE's video and sound additions, which the later machines kept.
const STE_ON: u8 = STE | MEGA_STE | TT | FALCON;
/// The machines with the blitter.
const BLITTER: u8 = MEGA_ST | STE | MEGA_STE | FALCON;
/// The machines with the STE's joypad ports.
const JOYPADS: u8 = STE | FALCON;

/// The MFP's clock: 2.4576 MHz, from its own crystal on every machine.
pub const MFP_CLOCK: u32 = 2_457_600;
/// The YM2149's clock on the ST: 2 MHz.
pub const YM_CLOCK: u32 = 2_000_000;
/// The MFP timers' prescalers, for control values 1 to 7.
pub const PRESCALERS: [u32; 7] = [4, 10, 16, 50, 64, 100, 200];

/// The machines a filter can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Machine {
    St,
    MegaSt,
    Ste,
    MegaSte,
    Tt,
    Falcon,
}

impl Machine {
    pub const ALL: [Machine; 6] = [Machine::St, Machine::MegaSt, Machine::Ste, Machine::MegaSte, Machine::Tt, Machine::Falcon];

    pub fn name(self) -> &'static str {
        match self {
            Machine::St => "ST",
            Machine::MegaSt => "Mega ST",
            Machine::Ste => "STE",
            Machine::MegaSte => "Mega STE",
            Machine::Tt => "TT",
            Machine::Falcon => "Falcon",
        }
    }

    pub fn bit(self) -> u8 {
        match self {
            Machine::St => ST,
            Machine::MegaSt => MEGA_ST,
            Machine::Ste => STE,
            Machine::MegaSte => MEGA_STE,
            Machine::Tt => TT,
            Machine::Falcon => FALCON,
        }
    }
}

/// A set of machines in words: "every machine", "STE, Mega STE, TT, Falcon".
pub fn machines_name(set: u8) -> String {
    if set == EVERY {
        return "every machine".into();
    }
    Machine::ALL.iter().filter(|m| set & m.bit() != 0).map(|m| m.name()).collect::<Vec<_>>().join(", ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    pub fn name(self) -> &'static str {
        match self {
            Size::Byte => "byte",
            Size::Word => "word",
            Size::Long => "long",
        }
    }

    pub fn bytes(self) -> u32 {
        match self {
            Size::Byte => 1,
            Size::Word => 2,
            Size::Long => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    pub fn name(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::ReadWrite => "read/write",
        }
    }
}

/// The chips the guide groups registers by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Chip {
    System,
    Video,
    Palette,
    TtVideo,
    Videl,
    Mfp,
    Ym,
    Acia,
    Dma,
    DmaSound,
    Blitter,
    Joypads,
    Dsp,
}

impl Chip {
    pub const ALL: [Chip; 13] = [Chip::Video, Chip::Palette, Chip::Mfp, Chip::Ym, Chip::Blitter, Chip::DmaSound, Chip::Acia, Chip::Dma, Chip::Joypads, Chip::System, Chip::TtVideo, Chip::Videl, Chip::Dsp];

    pub fn name(self) -> &'static str {
        match self {
            Chip::System => "Memory, cache and bus",
            Chip::Video => "Video (Shifter, GLUE and MMU)",
            Chip::Palette => "Palette",
            Chip::TtVideo => "TT video",
            Chip::Videl => "Falcon VIDEL",
            Chip::Mfp => "MFP 68901 (interrupts and timers)",
            Chip::Ym => "YM2149 sound",
            Chip::Acia => "ACIAs (keyboard and MIDI)",
            Chip::Dma => "DMA and floppy controller",
            Chip::DmaSound => "DMA sound and Microwire",
            Chip::Blitter => "Blitter",
            Chip::Joypads => "STE joypads",
            Chip::Dsp => "Falcon DSP host port",
        }
    }
}

/// A bit, or a run of bits, of a register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub high: u8,
    pub low: u8,
    pub name: &'static str,
    pub about: &'static str,
}

impl Field {
    pub fn bits(&self) -> String {
        if self.high == self.low { format!("{}", self.high) } else { format!("{}-{}", self.high, self.low) }
    }

    pub fn value(&self, v: u32) -> u32 {
        let width = (self.high - self.low + 1) as u32;
        let mask = if width >= 32 { u32::MAX } else { (1u32 << width) - 1 };
        (v >> self.low) & mask
    }
}

const fn bit(n: u8, name: &'static str, about: &'static str) -> Field {
    Field { high: n, low: n, name, about }
}

const fn bits(high: u8, low: u8, name: &'static str, about: &'static str) -> Field {
    Field { high, low, name, about }
}

/// A register: one, or a run of the same kind (the halftone RAM, the TT's and Falcon's palettes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Register {
    pub address: u32,
    pub name: String,
    pub size: Size,
    pub access: Access,
    pub machines: u8,
    pub chip: Chip,
    pub about: String,
    pub fields: &'static [Field],
    /// For a run: how many there are; 1 for a single register.
    pub count: u32,
}

impl Register {
    /// The bytes it covers, all of a run's.
    pub fn span(&self) -> u32 {
        self.size.bytes() * self.count
    }

    pub fn holds(&self, address: u32) -> bool {
        (self.address..self.address + self.span()).contains(&address)
    }

    /// Which of a run an address is.
    pub fn index(&self, address: u32) -> u32 {
        (address - self.address) / self.size.bytes()
    }

    /// The ST palette's colour this is (0-15), if it is one.
    pub fn st_colour(&self) -> Option<u32> {
        (self.chip == Chip::Palette && (0xFF8240..0xFF8260).contains(&self.address)).then(|| (self.address - 0xFF8240) / 2)
    }

    /// The name and where it is, as the guide heads it.
    pub fn heading(&self) -> String {
        let place = if self.count > 1 { format!("${:06X}-${:06X}", self.address, self.address + self.span() - 1) } else { format!("${:06X}", self.address) };
        format!("{}  {place}  {}, {}  ({})", self.name, self.size.name(), self.access.name(), machines_name(self.machines))
    }
}

struct Builder(Vec<Register>);

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, address: u32, name: &str, size: Size, access: Access, machines: u8, chip: Chip, about: &str, fields: &'static [Field]) {
        self.0.push(Register { address, name: name.to_string(), size, access, machines, chip, about: about.to_string(), fields, count: 1 });
    }

    #[allow(clippy::too_many_arguments)]
    fn run(&mut self, address: u32, count: u32, name: &str, size: Size, access: Access, machines: u8, chip: Chip, about: &str, fields: &'static [Field]) {
        self.0.push(Register { address, name: name.to_string(), size, access, machines, chip, about: about.to_string(), fields, count });
    }
}

// ---------- The bits ----------

const MEMCONF: &[Field] = &[
    bits(3, 2, "BANK0", "Size of the first memory bank: 0 128K, 1 512K, 2 2MB"),
    bits(1, 0, "BANK1", "Size of the second memory bank, the same way"),
];

const BASE_HIGH: &[Field] = &[bits(7, 0, "A23-16", "Bits 23 to 16 of the screen's address")];
const BASE_MID: &[Field] = &[bits(7, 0, "A15-8", "Bits 15 to 8 of the screen's address")];
const BASE_LOW: &[Field] = &[bits(7, 1, "A7-1", "Bits 7 to 1 of the screen's address (bit 0 is always 0)")];
const COUNTER: &[Field] = &[bits(7, 0, "COUNT", "This byte of the address the video is reading now")];

const SYNC: &[Field] = &[
    bit(1, "50HZ", "1: 50 Hz (PAL timing); 0: 60 Hz (NTSC). Changing it at the right moment opens the borders"),
    bit(0, "EXTSYNC", "1: take sync from outside (a genlock); leave it 0"),
];

const LINE_OFFSET: &[Field] = &[bits(8, 0, "OFFSET", "Words skipped after each line, so the screen can be a window on a wider picture")];
const LINE_WIDTH: &[Field] = &[bits(9, 0, "WIDTH", "Words in each line of the picture")];

const COLOUR: &[Field] = &[
    bit(11, "R0", "Red's lowest bit (STE and later; the ST ignores it)"),
    bits(10, 8, "R", "Red's top three bits: 0 to 7"),
    bit(7, "G0", "Green's lowest bit (STE and later)"),
    bits(6, 4, "G", "Green's top three bits"),
    bit(3, "B0", "Blue's lowest bit (STE and later)"),
    bits(2, 0, "B", "Blue's top three bits"),
];

const SHIFT_MODE: &[Field] = &[bits(1, 0, "MODE", "0: low, 320x200 in 16 colours; 1: medium, 640x200 in 4; 2: high, 640x400 in black and white")];
const HSCROLL: &[Field] = &[bits(3, 0, "PIXELS", "How many pixels the picture is moved left, 0 to 15")];

const TT_SHIFT: &[Field] = &[
    bit(15, "SMEAR", "Sample-and-hold: a pixel of colour 0 shows the colour of the last one that wasn't (TT low only)"),
    bit(12, "HYPER", "Hyper mono: the TT high mode's two colours from palette entries 254 and 255"),
    bits(10, 8, "MODE", "0 ST low, 1 ST medium, 2 ST high, 4 TT medium (640x480, 16 colours), 6 TT high (1280x960 mono), 7 TT low (320x480, 256 colours)"),
    bits(3, 0, "BANK", "Which 16 of the 256 colours the ST modes use"),
];

const TT_COLOUR: &[Field] = &[bits(11, 8, "R", "Red, 0 to 15"), bits(7, 4, "G", "Green"), bits(3, 0, "B", "Blue")];

const FALCON_SHIFT: &[Field] = &[
    bit(10, "2COLOUR", "Two colours, 1 bit a pixel"),
    bit(8, "TRUECOLOUR", "True colour: 16 bits a pixel, 5 red, 6 green, 5 blue, no palette"),
    bit(4, "8PLANES", "256 colours from 8 bitplanes"),
];

const FALCON_COLOUR: &[Field] = &[bits(31, 26, "R", "Red, 0 to 63"), bits(23, 18, "G", "Green, 0 to 63"), bits(7, 2, "B", "Blue, 0 to 63")];

const FALCON_MONITOR: &[Field] = &[bits(7, 6, "MONITOR", "The monitor plugged in: 0 mono, 1 RGB, 2 VGA, 3 TV")];

const FALCON_BUS: &[Field] = &[
    bit(5, "STEBUS", "0: the bus is timed as the STE's, for old programs"),
    bit(3, "NOBLIT", "1: the blitter is off"),
    bit(2, "BLIT16", "The blitter's clock: 0 8 MHz, 1 16 MHz"),
    bit(0, "CPU16", "The processor's clock: 0 8 MHz, 1 16 MHz"),
];

const MEGA_STE_CACHE: &[Field] = &[bit(1, "16MHZ", "The processor's clock: 0 8 MHz, 1 16 MHz"), bit(0, "CACHE", "1: the 16K cache on (it only works at 16 MHz)")];

const VIDEL_VCO: &[Field] = &[bits(8, 0, "VCO", "Clock and monitor settings: which video clock, and how the lines are timed")];
const VIDEL_VMD: &[Field] = &[bit(3, "HALVE", "Pixels twice as wide"), bit(1, "INTERLACE", "Interlace"), bit(0, "DOUBLE", "Each line shown twice")];

// The MFP.

const GPIP: &[Field] = &[
    bit(7, "MONO", "Monochrome monitor detect: 0 when one is plugged in (TT: the SCSI chip)"),
    bit(6, "RI", "Serial port's ring indicator (TT: the clock)"),
    bit(5, "FDC", "The floppy or hard disk controller wants attention (low when it does)"),
    bit(4, "ACIA", "The keyboard or MIDI ACIA has something (low when it does)"),
    bit(3, "BLIT", "The blitter has finished (Falcon: the DSP)"),
    bit(2, "CTS", "Serial port's clear to send"),
    bit(1, "DCD", "Serial port's carrier detect"),
    bit(0, "BUSY", "The printer is busy"),
];

const INTERRUPTS_A: &[Field] = &[
    bit(7, "MONO", "Channel 15: monochrome monitor detect (GPIP 7), vector $13C"),
    bit(6, "RI", "Channel 14: serial ring indicator (GPIP 6), vector $138"),
    bit(5, "TIMERA", "Channel 13: Timer A, vector $134"),
    bit(4, "RXFULL", "Channel 12: serial receive buffer full, vector $130"),
    bit(3, "RXERR", "Channel 11: serial receive error, vector $12C"),
    bit(2, "TXEMPTY", "Channel 10: serial transmit buffer empty, vector $128"),
    bit(1, "TXERR", "Channel 9: serial transmit error, vector $124"),
    bit(0, "TIMERB", "Channel 8: Timer B, vector $120"),
];

const INTERRUPTS_B: &[Field] = &[
    bit(7, "FDC", "Channel 7: floppy and hard disk (GPIP 5), vector $11C"),
    bit(6, "ACIA", "Channel 6: keyboard and MIDI (GPIP 4), vector $118"),
    bit(5, "TIMERC", "Channel 5: Timer C, TOS's 200 Hz clock, vector $114"),
    bit(4, "TIMERD", "Channel 4: Timer D, the serial port's baud rate, vector $110"),
    bit(3, "BLIT", "Channel 3: blitter done (GPIP 3), vector $10C"),
    bit(2, "CTS", "Channel 2: serial clear to send (GPIP 2), vector $108"),
    bit(1, "DCD", "Channel 1: serial carrier detect (GPIP 1), vector $104"),
    bit(0, "BUSY", "Channel 0: printer busy (GPIP 0), vector $100"),
];

const VECTOR: &[Field] = &[
    bits(7, 4, "BASE", "The top four bits of every MFP vector number: TOS uses 4, so vectors $40-$4F, at $100-$13C"),
    bit(3, "S", "1: software end of interrupt: a handler must clear its in-service bit (TOS sets it). 0: the MFP clears it itself"),
];

const TIMER_AB: &[Field] = &[
    bit(4, "RESET", "Writing 1 resets the timer's output"),
    bits(3, 0, "MODE", "0 stopped; 1-7 delay mode, dividing by 4, 10, 16, 50, 64, 100 or 200; 8 event count (Timer B: one a display line); 9-15 pulse width"),
];

const TIMER_CD: &[Field] = &[
    bits(6, 4, "C", "Timer C: 0 stopped; 1-7 delay mode, dividing by 4, 10, 16, 50, 64, 100 or 200"),
    bits(2, 0, "D", "Timer D, the same way"),
];

const TIMER_DATA: &[Field] = &[bits(7, 0, "COUNT", "What the timer counts down from: 1-255, or 0 for 256. Reading gives where it is now")];

const USART_SCR: &[Field] = &[bits(7, 0, "SYNC", "The character a synchronous transfer looks for")];
const USART_UCR: &[Field] = &[
    bit(7, "/16", "1: the clock is divided by 16 (asynchronous)"),
    bits(6, 5, "WORD", "Bits a character: 0 eight, 1 seven, 2 six, 3 five"),
    bits(4, 3, "STOP", "0 synchronous; 1 one stop bit; 2 one and a half; 3 two"),
    bit(2, "PARITY", "Parity on"),
    bit(1, "EVEN", "Even parity (0 odd)"),
];
const USART_STATUS: &[Field] = &[
    bit(7, "FULL/EMPTY", "Receive: a byte is waiting. Transmit: the buffer is empty"),
    bit(6, "ERROR", "Receive: overrun. Transmit: underrun"),
    bit(0, "ENABLE", "The receiver or transmitter is on"),
];
const USART_DATA: &[Field] = &[bits(7, 0, "DATA", "The byte sent or received")];

// The YM2149.

const YM_SELECT: &[Field] = &[bits(3, 0, "REGISTER", "Which of the YM's 16 registers $FF8802 writes (and reading $FF8800 reads)")];
const YM_DATA: &[Field] = &[bits(7, 0, "VALUE", "Written to the register chosen at $FF8800")];

// The ACIAs.

const ACIA_CONTROL: &[Field] = &[
    bit(7, "RXINT", "An interrupt when a byte arrives"),
    bits(6, 5, "TX", "0: RTS low, no transmit interrupt; 1: RTS low, transmit interrupt; 2: RTS high; 3: RTS low and send a break"),
    bits(4, 2, "WORD", "Character format: 5 is 8 bits, no parity, 1 stop bit, as the keyboard and MIDI use"),
    bits(1, 0, "DIVIDE", "Clock divided by 1, 16 or 64; 3 resets the ACIA"),
];
const ACIA_STATUS: &[Field] = &[
    bit(7, "IRQ", "The ACIA is asking for an interrupt"),
    bit(6, "PE", "Parity error"),
    bit(5, "OVRN", "Overrun: a byte came before the last was read"),
    bit(4, "FE", "Framing error"),
    bit(3, "CTS", "Clear to send"),
    bit(2, "DCD", "Carrier lost"),
    bit(1, "TDRE", "The transmit register is empty: another byte can be sent"),
    bit(0, "RDRF", "A byte has arrived to be read"),
];
const ACIA_DATA: &[Field] = &[bits(7, 0, "DATA", "Keyboard: scan codes ($39 space; +$80 when released) and mouse and joystick packets. MIDI: bytes in and out")];

// The DMA chip and floppy controller.

const DISK_ACCESS: &[Field] = &[bits(7, 0, "DATA", "The floppy controller's register chosen at $FF8606, or the DMA sector count")];
const DMA_MODE: &[Field] = &[
    bit(8, "WRITE", "1: DMA writes to the disk; 0 reads from it (toggling it clears the DMA's buffers)"),
    bit(7, "FDC", "1: DMA with the floppy controller; 0 with the hard disk (ACSI)"),
    bit(6, "NODMA", "1: DMA off"),
    bit(4, "COUNT", "1: $FF8604 is the sector count; 0 the controller's register"),
    bit(3, "HDC", "1: $FF8604 talks to the hard disk; 0 the floppy controller"),
    bits(2, 1, "REG", "The floppy controller's register: 0 command or status, 1 track, 2 sector, 3 data"),
];
const DMA_ADDRESS: &[Field] = &[bits(7, 0, "ADDRESS", "This byte of the DMA's memory address (set low, then middle, then high)")];

// DMA sound.

const SOUND_CONTROL: &[Field] = &[bit(1, "LOOP", "Start again from the beginning at the end"), bit(0, "PLAY", "Play the sample (it stops by itself at the end unless LOOP is set)")];
const SOUND_ADDRESS: &[Field] = &[bits(7, 0, "ADDRESS", "This byte of the address (the low byte's bit 0 is always 0)")];
const SOUND_MODE: &[Field] = &[bit(7, "MONO", "1: mono, a byte a sample; 0: stereo, left then right"), bits(1, 0, "RATE", "0 6258 Hz, 1 12517 Hz, 2 25033 Hz, 3 50066 Hz")];
const MICROWIRE_DATA: &[Field] = &[
    bits(10, 9, "ADDRESS", "The device: 2 is the volume and tone chip (LMC1992)"),
    bits(8, 6, "COMMAND", "0 mix, 1 bass, 2 treble, 3 master volume, 4 right volume, 5 left volume"),
    bits(5, 0, "VALUE", "The setting (see the command)"),
];
const MICROWIRE_MASK: &[Field] = &[bits(15, 0, "MASK", "Which bits of the data are sent: $07FF for the 11 a command has")];

// The blitter.

const HALFTONE: &[Field] = &[bits(15, 0, "PATTERN", "A line of the 16-line pattern the halftone operations use")];
const INCREMENT: &[Field] = &[bits(15, 1, "BYTES", "Added to the address after each word, or at the end of a line: signed, even")];
const ADDRESS: &[Field] = &[bits(23, 1, "ADDRESS", "Where the blitter reads or writes next (even)")];
const ENDMASK: &[Field] = &[bits(15, 0, "MASK", "Which bits of the word are written: a 1 bit takes the result, a 0 keeps what was there")];
const COUNT: &[Field] = &[bits(15, 0, "COUNT", "1 to 65535, or 0 for 65536; counts down as the blitter works")];
const HOP: &[Field] = &[bits(1, 0, "HOP", "Where the source comes from: 0 all ones, 1 the halftone pattern, 2 the source, 3 the source AND the halftone")];
const OP: &[Field] = &[bits(3, 0, "OP", "How the source and destination are combined: one of 16 logic operations")];
const BLIT_CONTROL: &[Field] = &[
    bit(7, "BUSY", "Write 1 to start; reads 1 until it's done. Writing 0 pauses it"),
    bit(6, "HOG", "1: the blitter keeps the bus until it's finished; 0: it takes turns with the processor"),
    bit(5, "SMUDGE", "The halftone line comes from the source's lowest four bits, not the line number"),
    bits(3, 0, "LINE", "The halftone line it starts at; moves on by one each line"),
];
const SKEW: &[Field] = &[
    bit(7, "FXSR", "Force extra source read: read a word before each line starts"),
    bit(6, "NFSR", "No final source read: don't read for the last word of each line"),
    bits(3, 0, "SKEW", "How many bits the source is moved right"),
];

// The STE's joypads.

const PAD_BUTTONS: &[Field] = &[
    bits(15, 8, "DIP", "Mega STE and Falcon: the DIP switches"),
    bit(3, "FIRE_B", "Joypad B's fire button of the row chosen at $FF9202 (0 when pressed)"),
    bit(2, "PAUSE_B", "Joypad B's pause (0 when pressed)"),
    bit(1, "FIRE_A", "Joypad A's fire button of the row chosen (0 when pressed)"),
    bit(0, "PAUSE_A", "Joypad A's pause (0 when pressed)"),
];
const PAD_MULTI: &[Field] = &[
    bits(15, 12, "PAD_B", "Read: joypad B's row chosen (0 bits pressed: directions or keypad)"),
    bits(11, 8, "PAD_A", "Read: joypad A's row chosen"),
    bits(7, 4, "SELECT_B", "Write: which row of joypad B to read (a 0 bit chooses it)"),
    bits(3, 0, "SELECT_A", "Write: which row of joypad A to read (a 0 bit chooses it)"),
];
const PADDLE: &[Field] = &[bits(7, 0, "POSITION", "A paddle's position, 0 to 255")];

// The Falcon's DSP host port.

const DSP_ICR: &[Field] = &[
    bit(7, "INIT", "Starts the host interface"),
    bits(6, 5, "HM", "Host mode (DMA)"),
    bit(4, "HF1", "Flag 1 to the DSP"),
    bit(3, "HF0", "Flag 0 to the DSP"),
    bit(1, "TREQ", "Interrupt the host when the transmit register is empty"),
    bit(0, "RREQ", "Interrupt the host when the receive register is full"),
];
const DSP_CVR: &[Field] = &[bit(7, "HC", "Host command: set it to make the DSP run the vector below"), bits(4, 0, "HV", "Which vector the host command runs")];
const DSP_ISR: &[Field] = &[
    bit(7, "HREQ", "The DSP wants the host"),
    bit(4, "HF3", "Flag 3 from the DSP"),
    bit(3, "HF2", "Flag 2 from the DSP"),
    bit(2, "TRDY", "Transmit ready"),
    bit(1, "TXDE", "The transmit register is empty: write the next word"),
    bit(0, "RXDF", "The receive register is full: a word from the DSP is waiting"),
];
const DSP_DATA: &[Field] = &[bits(7, 0, "DATA", "A byte of the 24-bit word to or from the DSP (a long at $FFA204 moves the whole word)")];

fn build() -> Vec<Register> {
    use Access::*;
    use Chip::*;
    use Size::*;
    let mut b = Builder(Vec::new());
    // Memory and the system.
    b.add(0xFF8001, "MEMCONF", Byte, ReadWrite, ST | MEGA_ST | STE | MEGA_STE, System, "The memory controller's bank sizes, set by TOS at start-up", MEMCONF);
    b.add(0xFF8006, "MONITOR", Byte, Read, FALCON, System, "Which monitor is plugged in, and how memory is fitted", FALCON_MONITOR);
    b.add(0xFF8007, "BUSCTRL", Byte, ReadWrite, FALCON, System, "The Falcon's clocks and bus: 8 or 16 MHz, the blitter, STE compatibility", FALCON_BUS);
    b.add(0xFF8E21, "CACHECTRL", Byte, ReadWrite, MEGA_STE, System, "The Mega STE's processor speed and cache", MEGA_STE_CACHE);
    // Video.
    b.add(0xFF8201, "DBASEH", Byte, ReadWrite, EVERY, Video, "The screen's address, top byte: where the next frame starts (read at the start of each frame)", BASE_HIGH);
    b.add(0xFF8203, "DBASEM", Byte, ReadWrite, EVERY, Video, "The screen's address, middle byte. On the ST the address is a multiple of 256", BASE_MID);
    b.add(0xFF820D, "DBASEL", Byte, ReadWrite, STE_ON, Video, "The screen's address, low byte, so it can start on any word. Writing the top or middle byte clears it, so write it last", BASE_LOW);
    b.add(0xFF8205, "VCOUNTHI", Byte, ReadWrite, EVERY, Video, "The address the video is reading now, top byte (only read on the ST; the STE can write it)", COUNTER);
    b.add(0xFF8207, "VCOUNTMID", Byte, ReadWrite, EVERY, Video, "The address the video is reading now, middle byte", COUNTER);
    b.add(0xFF8209, "VCOUNTLO", Byte, ReadWrite, EVERY, Video, "The address the video is reading now, low byte: it changes as the beam moves, so code can wait for it to sync with the screen", COUNTER);
    b.add(0xFF820A, "SYNCMODE", Byte, ReadWrite, EVERY, Video, "50 or 60 Hz, and external sync", SYNC);
    b.add(0xFF820E, "LINEWID", Word, ReadWrite, STE_ON, Video, "Words added to the address at the end of each line (the STE has only the low byte, $FF820F): set it to see part of a wider picture", LINE_OFFSET);
    b.add(0xFF8210, "VWRAP", Word, ReadWrite, FALCON, Videl, "How many words each line of the picture has", LINE_WIDTH);
    b.add(0xFF8260, "SHIFTMD", Byte, ReadWrite, EVERY, Video, "The resolution: low, medium or high", SHIFT_MODE);
    b.add(0xFF8264, "HSCROLL", Byte, ReadWrite, STE | MEGA_STE | FALCON, Video, "Horizontal scroll without the extra read: moves the picture left 0 to 15 pixels, but doesn't fetch the word the moved pixels need", HSCROLL);
    b.add(0xFF8265, "HSCROLLP", Byte, ReadWrite, STE | MEGA_STE | FALCON, Video, "Horizontal scroll, the usual one: moves the picture left 0 to 15 pixels, reading an extra word a bitplane each line when it isn't 0 (so take 4 words off LINEWID in low resolution)", HSCROLL);
    for n in 0..16 {
        let about = format!("Colour {n}. ST: three bits of red, green and blue (512 colours). STE: a fourth bit each, the lowest, kept above the other three (4096 colours)");
        b.add(0xFF8240 + n * 2, &format!("COLOR{n}"), Word, ReadWrite, EVERY, Palette, &about, COLOUR);
    }
    // TT video.
    b.add(0xFF8262, "TTSHIFT", Word, ReadWrite, TT, TtVideo, "The TT's video mode, which 16 colours the ST modes use, hyper mono and sample-and-hold", TT_SHIFT);
    b.run(0xFF8400, 256, "TTPALETTE", Word, ReadWrite, TT, TtVideo, "The TT's 256 colours, each 4 bits of red, green and blue in the plain order", TT_COLOUR);
    // The Falcon's VIDEL.
    b.add(0xFF8266, "SPSHIFT", Word, ReadWrite, FALCON, Videl, "The Falcon's colour depth: 2, 256 or true colour (none of these bits: 16 colours or the ST modes)", FALCON_SHIFT);
    for (address, name, about) in [
        (0xFF8280, "HHC", "Horizontal hold counter: where the beam is across the line"),
        (0xFF8282, "HHT", "Horizontal hold timer: the length of a line, in half-cycles of the video clock"),
        (0xFF8284, "HBB", "Horizontal border begin: where the right border starts"),
        (0xFF8286, "HBE", "Horizontal border end: where the left border ends"),
        (0xFF8288, "HDB", "Horizontal display begin: where the picture starts"),
        (0xFF828A, "HDE", "Horizontal display end: where the picture stops"),
        (0xFF828C, "HSS", "Horizontal sync start"),
        (0xFF828E, "HFS", "Horizontal field sync (interlace)"),
        (0xFF8290, "HEE", "Horizontal equalisation end"),
        (0xFF82A0, "VFC", "Vertical frequency counter: which half-line the beam is on"),
        (0xFF82A2, "VFT", "Vertical frequency timer: the half-lines in a frame"),
        (0xFF82A4, "VBB", "Vertical border begin: where the bottom border starts (in half-lines)"),
        (0xFF82A6, "VBE", "Vertical border end: where the top border ends"),
        (0xFF82A8, "VDB", "Vertical display begin: where the picture starts"),
        (0xFF82AA, "VDE", "Vertical display end: where the picture stops"),
        (0xFF82AC, "VSS", "Vertical sync start"),
    ] {
        b.add(address, name, Word, if name == "HHC" || name == "VFC" { Read } else { ReadWrite }, FALCON, Videl, about, &[]);
    }
    b.add(0xFF82C0, "VCO", Word, ReadWrite, FALCON, Videl, "Video control: the clock and the monitor's timing", VIDEL_VCO);
    b.add(0xFF82C2, "VMD", Word, ReadWrite, FALCON, Videl, "Video mode: doubled lines, interlace, wide pixels", VIDEL_VMD);
    b.run(0xFF9800, 256, "FALCONPALETTE", Long, ReadWrite, FALCON, Videl, "The Falcon's 256 colours, a long each: 6 bits of red, green and blue (none in 16-bit true colour)", FALCON_COLOUR);
    // DMA and the floppy controller.
    b.add(0xFF8604, "DISKCTL", Word, ReadWrite, EVERY, Dma, "The floppy controller's or hard disk's registers, or the DMA sector count, as $FF8606 chooses", DISK_ACCESS);
    b.add(0xFF8606, "DMAMODE", Word, ReadWrite, EVERY, Dma, "Write: what $FF8604 reaches and which way DMA goes. Read: DMA status (bit 0 is 0 after an error, bit 1 the sector count isn't 0, bit 2 the controller wants data)", DMA_MODE);
    b.add(0xFF8609, "DMAHIGH", Byte, ReadWrite, EVERY, Dma, "The DMA's memory address, top byte", DMA_ADDRESS);
    b.add(0xFF860B, "DMAMID", Byte, ReadWrite, EVERY, Dma, "The DMA's memory address, middle byte", DMA_ADDRESS);
    b.add(0xFF860D, "DMALOW", Byte, ReadWrite, EVERY, Dma, "The DMA's memory address, low byte", DMA_ADDRESS);
    // The YM2149.
    b.add(0xFF8800, "YMSELECT", Byte, ReadWrite, EVERY, Ym, "Write: chooses which YM register to set. Read: that register's value. The YM also drives the floppy's drive and side select and the printer's strobe, through its port A (register 14)", YM_SELECT);
    b.add(0xFF8802, "YMWRITE", Byte, Write, EVERY, Ym, "Writes the YM register chosen at $FF8800. A long written to $FF8800 does both: the register in its top byte, the value in its third", YM_DATA);
    // DMA sound.
    b.add(0xFF8900, "SNDCTRL", Word, ReadWrite, STE_ON, DmaSound, "DMA sound on or off, once or looping (the Falcon adds recording and interrupts)", SOUND_CONTROL);
    b.add(0xFF8903, "SNDSTARTH", Byte, ReadWrite, STE_ON, DmaSound, "Where the sample starts, top byte", SOUND_ADDRESS);
    b.add(0xFF8905, "SNDSTARTM", Byte, ReadWrite, STE_ON, DmaSound, "Where the sample starts, middle byte", SOUND_ADDRESS);
    b.add(0xFF8907, "SNDSTARTL", Byte, ReadWrite, STE_ON, DmaSound, "Where the sample starts, low byte", SOUND_ADDRESS);
    b.add(0xFF8909, "SNDCOUNTH", Byte, Read, STE_ON, DmaSound, "Where the sound is playing now, top byte", SOUND_ADDRESS);
    b.add(0xFF890B, "SNDCOUNTM", Byte, Read, STE_ON, DmaSound, "Where the sound is playing now, middle byte", SOUND_ADDRESS);
    b.add(0xFF890D, "SNDCOUNTL", Byte, Read, STE_ON, DmaSound, "Where the sound is playing now, low byte", SOUND_ADDRESS);
    b.add(0xFF890F, "SNDENDH", Byte, ReadWrite, STE_ON, DmaSound, "Where the sample ends (the byte after its last), top byte", SOUND_ADDRESS);
    b.add(0xFF8911, "SNDENDM", Byte, ReadWrite, STE_ON, DmaSound, "Where the sample ends, middle byte", SOUND_ADDRESS);
    b.add(0xFF8913, "SNDENDL", Byte, ReadWrite, STE_ON, DmaSound, "Where the sample ends, low byte", SOUND_ADDRESS);
    b.add(0xFF8921, "SNDMODE", Byte, ReadWrite, STE_ON, DmaSound, "Mono or stereo and the sample rate (the Falcon adds 16-bit samples)", SOUND_MODE);
    b.add(0xFF8922, "MWDATA", Word, ReadWrite, STE | MEGA_STE | TT, DmaSound, "Microwire data: a command for the volume and tone chip, sent a bit at a time as the mask shifts (wait for MWMASK to come back before the next)", MICROWIRE_DATA);
    b.add(0xFF8924, "MWMASK", Word, ReadWrite, STE | MEGA_STE | TT | FALCON, DmaSound, "Microwire mask: which bits of MWDATA are sent", MICROWIRE_MASK);
    // The blitter.
    b.run(0xFF8A00, 16, "HALFTONE", Word, ReadWrite, BLITTER, Blitter, "The halftone RAM: 16 words of pattern, a line each", HALFTONE);
    b.add(0xFF8A20, "SRCXINC", Word, ReadWrite, BLITTER, Blitter, "Source X increment: added to the source address after each word of a line (2 for words side by side, 8 for one bitplane of low resolution)", INCREMENT);
    b.add(0xFF8A22, "SRCYINC", Word, ReadWrite, BLITTER, Blitter, "Source Y increment: added instead after a line's last word, to reach the next line", INCREMENT);
    b.add(0xFF8A24, "SRCADDR", Long, ReadWrite, BLITTER, Blitter, "Source address: where the next source word is read", ADDRESS);
    b.add(0xFF8A28, "ENDMASK1", Word, ReadWrite, BLITTER, Blitter, "The mask for each line's first word (and for every word of a one-word line)", ENDMASK);
    b.add(0xFF8A2A, "ENDMASK2", Word, ReadWrite, BLITTER, Blitter, "The mask for the words in the middle of a line, usually $FFFF", ENDMASK);
    b.add(0xFF8A2C, "ENDMASK3", Word, ReadWrite, BLITTER, Blitter, "The mask for each line's last word", ENDMASK);
    b.add(0xFF8A2E, "DSTXINC", Word, ReadWrite, BLITTER, Blitter, "Destination X increment: added after each word of a line", INCREMENT);
    b.add(0xFF8A30, "DSTYINC", Word, ReadWrite, BLITTER, Blitter, "Destination Y increment: added instead after a line's last word", INCREMENT);
    b.add(0xFF8A32, "DSTADDR", Long, ReadWrite, BLITTER, Blitter, "Destination address: where the next word is written", ADDRESS);
    b.add(0xFF8A36, "XCOUNT", Word, ReadWrite, BLITTER, Blitter, "Words in each line", COUNT);
    b.add(0xFF8A38, "YCOUNT", Word, ReadWrite, BLITTER, Blitter, "Lines to do: counts down to 0, when the blit is over", COUNT);
    b.add(0xFF8A3A, "HOP", Byte, ReadWrite, BLITTER, Blitter, "Halftone operation: what the source is, before the logic operation", HOP);
    b.add(0xFF8A3B, "OP", Byte, ReadWrite, BLITTER, Blitter, "The logic operation that makes the destination from the source and what's there", OP);
    b.add(0xFF8A3C, "LINENUM", Byte, ReadWrite, BLITTER, Blitter, "Control: start, hog mode, smudge and the halftone line", BLIT_CONTROL);
    b.add(0xFF8A3D, "SKEW", Byte, ReadWrite, BLITTER, Blitter, "How the source is shifted, and the extra or missing reads that shifting needs", SKEW);
    // STE joypads.
    b.add(0xFF9200, "JOYBUTTONS", Word, Read, JOYPADS | MEGA_STE, Joypads, "The joypads' fire and pause buttons, and on the Mega STE and Falcon the DIP switches (read it as a word)", PAD_BUTTONS);
    b.add(0xFF9202, "JOYPADS", Word, ReadWrite, JOYPADS, Joypads, "Write which row of each joypad to look at, then read its four bits: directions, or keys of the keypad", PAD_MULTI);
    b.add(0xFF9211, "PADDLE0X", Byte, Read, JOYPADS, Joypads, "Paddle 0's position", PADDLE);
    b.add(0xFF9213, "PADDLE0Y", Byte, Read, JOYPADS, Joypads, "Paddle 1's position", PADDLE);
    b.add(0xFF9215, "PADDLE1X", Byte, Read, JOYPADS, Joypads, "Paddle 2's position", PADDLE);
    b.add(0xFF9217, "PADDLE1Y", Byte, Read, JOYPADS, Joypads, "Paddle 3's position", PADDLE);
    b.add(0xFF9220, "LIGHTPENX", Word, Read, STE, Joypads, "Where a light pen was seen across the screen", &[]);
    b.add(0xFF9222, "LIGHTPENY", Word, Read, STE, Joypads, "Where a light pen was seen down the screen", &[]);
    // The Falcon's DSP.
    b.add(0xFFA200, "DSPICR", Byte, ReadWrite, FALCON, Dsp, "The DSP host port's interrupt control", DSP_ICR);
    b.add(0xFFA201, "DSPCVR", Byte, ReadWrite, FALCON, Dsp, "Command vector: makes the DSP run one of its interrupt routines", DSP_CVR);
    b.add(0xFFA202, "DSPISR", Byte, Read, FALCON, Dsp, "The host port's status: wait on bit 1 before writing a word and bit 0 before reading one", DSP_ISR);
    b.add(0xFFA203, "DSPIVR", Byte, ReadWrite, FALCON, Dsp, "The 68030's interrupt vector number for the DSP", &[]);
    b.add(0xFFA205, "DSPDATAH", Byte, ReadWrite, FALCON, Dsp, "The word to or from the DSP, top byte (a long at $FFA204 moves all three)", DSP_DATA);
    b.add(0xFFA206, "DSPDATAM", Byte, ReadWrite, FALCON, Dsp, "The word to or from the DSP, middle byte", DSP_DATA);
    b.add(0xFFA207, "DSPDATAL", Byte, ReadWrite, FALCON, Dsp, "The word to or from the DSP, low byte", DSP_DATA);
    // The MFP.
    b.add(0xFFFA01, "GPIP", Byte, ReadWrite, EVERY, Mfp, "The MFP's eight input lines: what's asking for attention (0 bits, mostly) and the monochrome monitor", GPIP);
    b.add(0xFFFA03, "AER", Byte, ReadWrite, EVERY, Mfp, "Active edge: for each input line, 1 to interrupt as it rises, 0 as it falls", GPIP);
    b.add(0xFFFA05, "DDR", Byte, ReadWrite, EVERY, Mfp, "Data direction: for each line, 1 output, 0 input (TOS leaves them all inputs)", GPIP);
    b.add(0xFFFA07, "IERA", Byte, ReadWrite, EVERY, Mfp, "Interrupt enable A: which of channels 8-15 may interrupt at all (clearing a bit also clears its pending bit)", INTERRUPTS_A);
    b.add(0xFFFA09, "IERB", Byte, ReadWrite, EVERY, Mfp, "Interrupt enable B: which of channels 0-7 may interrupt at all", INTERRUPTS_B);
    b.add(0xFFFA0B, "IPRA", Byte, ReadWrite, EVERY, Mfp, "Interrupt pending A: which have happened and are waiting. Write 0 bits to clear them (1 bits leave them)", INTERRUPTS_A);
    b.add(0xFFFA0D, "IPRB", Byte, ReadWrite, EVERY, Mfp, "Interrupt pending B, the same way", INTERRUPTS_B);
    b.add(0xFFFA0F, "ISRA", Byte, ReadWrite, EVERY, Mfp, "Interrupt in service A: which handlers are running. With the S bit set (as TOS has it), a handler clears its own bit when it's done, and lower channels wait until it has", INTERRUPTS_A);
    b.add(0xFFFA11, "ISRB", Byte, ReadWrite, EVERY, Mfp, "Interrupt in service B, the same way", INTERRUPTS_B);
    b.add(0xFFFA13, "IMRA", Byte, ReadWrite, EVERY, Mfp, "Interrupt mask A: an enabled channel whose bit here is 0 still becomes pending but doesn't interrupt", INTERRUPTS_A);
    b.add(0xFFFA15, "IMRB", Byte, ReadWrite, EVERY, Mfp, "Interrupt mask B, the same way", INTERRUPTS_B);
    b.add(0xFFFA17, "VR", Byte, ReadWrite, EVERY, Mfp, "Vector register: where the MFP's vectors are, and how in-service bits are cleared", VECTOR);
    b.add(0xFFFA19, "TACR", Byte, ReadWrite, EVERY, Mfp, "Timer A control: its mode and how its clock is divided (event mode counts the parallel port's busy line, used for sampled sound tricks)", TIMER_AB);
    b.add(0xFFFA1B, "TBCR", Byte, ReadWrite, EVERY, Mfp, "Timer B control: its mode and how its clock is divided. Event count mode (8) counts display lines: the raster interrupt", TIMER_AB);
    b.add(0xFFFA1D, "TCDCR", Byte, ReadWrite, EVERY, Mfp, "Timer C and D control: delay mode only, C in the top half, D in the bottom", TIMER_CD);
    b.add(0xFFFA1F, "TADR", Byte, ReadWrite, EVERY, Mfp, "Timer A data: the count it starts from", TIMER_DATA);
    b.add(0xFFFA21, "TBDR", Byte, ReadWrite, EVERY, Mfp, "Timer B data: the count it starts from (in event count mode, how many lines between interrupts)", TIMER_DATA);
    b.add(0xFFFA23, "TCDR", Byte, ReadWrite, EVERY, Mfp, "Timer C data: the count it starts from (TOS: 192 with divide-by-64, for 200 Hz)", TIMER_DATA);
    b.add(0xFFFA25, "TDDR", Byte, ReadWrite, EVERY, Mfp, "Timer D data: the count it starts from (it sets the serial port's speed)", TIMER_DATA);
    b.add(0xFFFA27, "SCR", Byte, ReadWrite, EVERY, Mfp, "Serial port: the synchronous character", USART_SCR);
    b.add(0xFFFA29, "UCR", Byte, ReadWrite, EVERY, Mfp, "Serial port: the character format", USART_UCR);
    b.add(0xFFFA2B, "RSR", Byte, ReadWrite, EVERY, Mfp, "Serial port: the receiver's status", USART_STATUS);
    b.add(0xFFFA2D, "TSR", Byte, ReadWrite, EVERY, Mfp, "Serial port: the transmitter's status", USART_STATUS);
    b.add(0xFFFA2F, "UDR", Byte, ReadWrite, EVERY, Mfp, "Serial port: the byte sent or received", USART_DATA);
    // The ACIAs.
    b.add(0xFFFC00, "KEYCTRL", Byte, ReadWrite, EVERY, Acia, "The keyboard ACIA. Write: its control (TOS writes $96). Read: its status", ACIA_CONTROL);
    b.add(0xFFFC02, "KEYDATA", Byte, ReadWrite, EVERY, Acia, "The keyboard ACIA's data: bytes from the keyboard processor (keys, mouse, joysticks), and commands to it", ACIA_DATA);
    b.add(0xFFFC04, "MIDICTRL", Byte, ReadWrite, EVERY, Acia, "The MIDI ACIA. Write: its control. Read: its status", ACIA_CONTROL);
    b.add(0xFFFC06, "MIDIDATA", Byte, ReadWrite, EVERY, Acia, "The MIDI ACIA's data", ACIA_DATA);
    b.0
}

pub fn registers() -> &'static [Register] {
    static ALL: OnceLock<Vec<Register>> = OnceLock::new();
    ALL.get_or_init(build)
}

/// The register at an address (24 bits; $FFFF8240 and $8240.W, sign-extended, are both $FF8240). An address in
/// a word or long register finds it; so does the STE's $FF820F, the line offset's byte.
pub fn at(address: u32) -> Option<&'static Register> {
    let address = address & 0xFF_FFFF;
    registers().iter().find(|r| r.holds(address))
}

/// The ACIA status registers share their address with the control registers: reading finds the status.
pub fn is_acia_control(register: &Register) -> bool {
    matches!(register.address, 0xFFFC00 | 0xFFFC04)
}

/// A register by name, in any case: COLOR0 (or COLOR00, PALETTE0), TBCR, the blitter's HOP.
pub fn named(name: &str) -> Option<&'static Register> {
    let upper = name.trim().to_ascii_uppercase().replace(['_', ' '], "");
    if upper.is_empty() {
        return None;
    }
    if let Some(found) = registers().iter().find(|r| r.name == upper) {
        return Some(found);
    }
    if let Some(n) = upper.strip_prefix("COLOR").or_else(|| upper.strip_prefix("COLOUR")).or_else(|| upper.strip_prefix("PALETTE")).and_then(|n| n.parse::<u32>().ok()).filter(|n| *n < 16) {
        return at(0xFF8240 + n * 2);
    }
    None
}

/// Whether an address is the ST's hardware: $FF8000 and above.
pub fn is_hardware(address: u32) -> bool {
    (0xFF8000..=0xFFFFFF).contains(&(address & 0xFF_FFFF))
}

// ---------- Colours ----------

/// A palette word as the ST shows it: three bits a gun, as 8-bit red, green and blue.
pub fn st_rgb(value: u16) -> [u8; 3] {
    let gun = |shift: u16| ((((value >> shift) & 7) as u32 * 255 + 3) / 7) as u8;
    [gun(8), gun(4), gun(0)]
}

/// A gun of the STE's palette, 0-15: its lowest bit is kept above the other three.
pub fn ste_gun(nibble: u16) -> u8 {
    (((nibble & 7) << 1) | ((nibble >> 3) & 1)) as u8
}

#[cfg(test)]
/// A gun 0-15 the STE way: the lowest bit moved to the top.
pub fn ste_nibble(level: u8) -> u16 {
    (((level as u16) >> 1) & 7) | (((level as u16) & 1) << 3)
}

/// A palette word as the STE shows it: four bits a gun.
pub fn ste_levels(value: u16) -> [u8; 3] {
    [ste_gun(value >> 8), ste_gun(value >> 4), ste_gun(value)]
}

pub fn ste_rgb(value: u16) -> [u8; 3] {
    ste_levels(value).map(|l| l * 17)
}

/// A colour as #RRGGBB, after a mark a hover shows as a swatch of it.
pub fn swatch(rgb: [u8; 3]) -> String {
    format!("\u{25A0}#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// A palette word explained for both machines.
pub fn explain_colour(n: u32, value: u16) -> String {
    let [r, g, b] = [(value >> 8) & 7, (value >> 4) & 7, value & 7];
    let [sr, sg, sb] = ste_levels(value);
    let mut out = format!("colour {n}\n  ST:  {} red {r}, green {g}, blue {b}", swatch(st_rgb(value)));
    out.push_str(&format!("\n  STE: {} red {sr}, green {sg}, blue {sb} (of 15)", swatch(ste_rgb(value))));
    if value & 0x888 != 0 {
        out.push_str("\n  The $888 bits are the STE's lowest: an ST ignores them");
    }
    out
}

// ---------- The MFP's timers ----------

/// A timer's control value read: its mode in words, and for delay mode, the divider.
pub fn timer_mode(control: u8) -> (String, Option<u32>) {
    match control & 15 {
        0 => ("stopped".into(), None),
        n @ 1..=7 => {
            let divider = PRESCALERS[n as usize - 1];
            (format!("delay mode, the 2.4576 MHz clock divided by {divider}"), Some(divider))
        }
        8 => ("event count mode (Timer B: one a display line; Timer A: the parallel port's busy line)".into(), None),
        n => {
            let divider = PRESCALERS[n as usize - 9];
            (format!("pulse width mode, divided by {divider}"), Some(divider))
        }
    }
}

/// How often a timer in delay mode interrupts, in Hz: the clock over the divider and the count (0 counts 256).
pub fn timer_rate(divider: u32, data: u8) -> f64 {
    let count = if data == 0 { 256 } else { data as u32 };
    MFP_CLOCK as f64 / (divider as f64 * count as f64)
}

/// The settings for a wanted rate: the prescaler's control value (1-7), the data byte (0 for 256), the rate it
/// gives and its error in percent; the best first, then others for the next best of each prescaler.
pub fn timer_settings(hz: f64) -> Vec<(u8, u8, f64, f64)> {
    let mut found: Vec<(u8, u8, f64, f64)> = Vec::new();
    if hz <= 0.0 || !hz.is_finite() {
        return found;
    }
    for (i, divider) in PRESCALERS.iter().enumerate() {
        let ideal = MFP_CLOCK as f64 / (*divider as f64 * hz);
        let mut best: Option<(u8, u8, f64, f64)> = None;
        for count in [ideal.floor(), ideal.ceil()] {
            if !(1.0..=256.0).contains(&count) {
                continue;
            }
            let data = if count as u32 == 256 { 0 } else { count as u8 };
            let rate = timer_rate(*divider, data);
            let error = (rate - hz) / hz * 100.0;
            if best.is_none_or(|b| error.abs() < b.3.abs()) {
                best = Some((i as u8 + 1, data, rate, error));
            }
        }
        found.extend(best);
    }
    // Most accurate first; of equals, the larger count (finer steps nearby), so the smaller divider.
    found.sort_by(|a, b| a.3.abs().partial_cmp(&b.3.abs()).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    found
}

// ---------- The YM2149 ----------

/// A YM2149 register: its number's name and what it does.
pub struct YmRegister {
    pub number: u8,
    pub name: &'static str,
    pub about: &'static str,
}

pub const YM: [YmRegister; 16] = [
    YmRegister { number: 0, name: "Channel A tone, fine", about: "The low 8 bits of channel A's tone period" },
    YmRegister { number: 1, name: "Channel A tone, coarse", about: "The top 4 bits of channel A's tone period" },
    YmRegister { number: 2, name: "Channel B tone, fine", about: "The low 8 bits of channel B's tone period" },
    YmRegister { number: 3, name: "Channel B tone, coarse", about: "The top 4 bits of channel B's tone period" },
    YmRegister { number: 4, name: "Channel C tone, fine", about: "The low 8 bits of channel C's tone period" },
    YmRegister { number: 5, name: "Channel C tone, coarse", about: "The top 4 bits of channel C's tone period" },
    YmRegister { number: 6, name: "Noise period", about: "The noise generator's period, 0 to 31: higher is lower" },
    YmRegister { number: 7, name: "Mixer", about: "Which channels play tone and noise (a 0 bit turns one on), and the two ports' directions" },
    YmRegister { number: 8, name: "Channel A volume", about: "Channel A's volume 0-15, or bit 4 to follow the envelope" },
    YmRegister { number: 9, name: "Channel B volume", about: "Channel B's volume 0-15, or bit 4 to follow the envelope" },
    YmRegister { number: 10, name: "Channel C volume", about: "Channel C's volume 0-15, or bit 4 to follow the envelope" },
    YmRegister { number: 11, name: "Envelope period, fine", about: "The low 8 bits of the envelope's period" },
    YmRegister { number: 12, name: "Envelope period, coarse", about: "The top 8 bits of the envelope's period" },
    YmRegister { number: 13, name: "Envelope shape", about: "The envelope's shape; writing it starts the envelope again" },
    YmRegister { number: 14, name: "Port A", about: "Outputs on the ST: floppy side and drive select, serial RTS and DTR, the printer's strobe" },
    YmRegister { number: 15, name: "Port B", about: "The printer port's eight data lines" },
];

/// A value written to a YM register, read.
pub fn explain_ym(register: u8, value: u8) -> String {
    let r = register & 15;
    let on = |n: u8| value & (1 << n) != 0;
    match r {
        0 | 2 | 4 => format!("the low 8 bits of channel {}'s period: ${value:02X} ({value}; with the top bits 0, {:.1} Hz)", (b'A' + r / 2) as char, tone_hz(value as u16)),
        1 | 3 | 5 => format!("the top 4 bits of channel {}'s period: {} (period ${:X}00 and up)", (b'A' + r / 2) as char, value & 15, value & 15),
        6 => {
            let period = (value & 31).max(1) as u32;
            format!("noise period {} (about {} Hz)", value & 31, YM_CLOCK / (16 * period))
        }
        7 => {
            let list = |base: u8| -> String {
                let on: Vec<String> = (0..3).filter(|c| value & (1 << (base + c)) == 0).map(|c| ((b'A' + c) as char).to_string()).collect();
                if on.is_empty() { "none".into() } else { on.join(", ") }
            };
            format!(
                "tone on {}; noise on {}; port A {}, port B {}",
                list(0),
                list(3),
                if on(6) { "output" } else { "input" },
                if on(7) { "output" } else { "input" }
            )
        }
        8..=10 => {
            let channel = (b'A' + r - 8) as char;
            if on(4) { format!("channel {channel} follows the envelope") } else { format!("channel {channel} at volume {} of 15", value & 15) }
        }
        11 => format!("the envelope period's low 8 bits: ${value:02X}"),
        12 => format!("the envelope period's top 8 bits: ${value:02X} (period ${value:02X}xx)"),
        13 => format!("envelope shape {}: {}", value & 15, envelope_shape(value)),
        14 => {
            let mut parts = vec![if on(0) { "side 0".to_string() } else { "side 1".to_string() }];
            parts.push(format!("drive A {}", if on(1) { "off" } else { "selected" }));
            parts.push(format!("drive B {}", if on(2) { "off" } else { "selected" }));
            parts.push(format!("RTS {}", u8::from(on(3))));
            parts.push(format!("DTR {}", u8::from(on(4))));
            parts.push(format!("printer strobe {}", u8::from(on(5))));
            parts.push(format!("bits 6 and 7 (general outputs) {}{}", u8::from(on(6)), u8::from(on(7))));
            parts.join(", ")
        }
        _ => format!("printer data ${value:02X}"),
    }
}

/// An envelope shape, 0-15, drawn in words.
pub fn envelope_shape(value: u8) -> &'static str {
    match value & 15 {
        0..=3 | 9 => "falls once, then silent  \\___",
        4..=7 | 15 => "rises once, then silent  /___",
        8 => "falls again and again  \\\\\\\\",
        10 => "falls and rises  \\/\\/",
        11 => "falls once, then loud  \\^^^",
        12 => "rises again and again  ////",
        13 => "rises once, then stays loud  /^^^",
        _ => "rises and falls  /\\/\\",
    }
}

/// A tone period's note: its frequency on the ST's 2 MHz YM.
pub fn tone_hz(period: u16) -> f64 {
    YM_CLOCK as f64 / (16.0 * period.max(1) as f64)
}

// ---------- The blitter ----------

/// The 16 logic operations, from Atari's numbering: what D becomes from the source S and the destination D.
pub const OPS: [&str; 16] = [
    "all zeros",
    "S AND D",
    "S AND NOT D",
    "S (copy)",
    "NOT S AND D",
    "D (unchanged)",
    "S XOR D",
    "S OR D",
    "NOT S AND NOT D",
    "NOT S XOR D",
    "NOT D",
    "S OR NOT D",
    "NOT S",
    "NOT S OR D",
    "NOT S OR NOT D",
    "all ones",
];

pub const HOPS: [&str; 4] = ["all ones", "the halftone pattern", "the source", "the source AND the halftone"];

// ---------- Values read ----------

/// What a value written to a register does, in a few words; for a run, `address` says which of it.
pub fn explain(register: &Register, address: u32, value: u32) -> String {
    let byte = value as u8;
    let word = value as u16;
    let set = |fields: &[Field]| -> String {
        let on: Vec<&str> = fields.iter().filter(|f| f.high == f.low && value & (1 << f.low) != 0).map(|f| f.name).collect();
        if on.is_empty() { "none".into() } else { on.join(", ") }
    };
    if let Some(n) = register.st_colour() {
        return explain_colour(n, word);
    }
    match register.address {
        0xFF8260 => match byte & 3 {
            0 => "low resolution: 320x200, 16 colours".into(),
            1 => "medium resolution: 640x200, 4 colours".into(),
            2 => "high resolution: 640x400, black and white".into(),
            _ => "3, which isn't a resolution".into(),
        },
        0xFF820A => format!("{} Hz{}", if byte & 2 != 0 { 50 } else { 60 }, if byte & 1 != 0 { ", external sync" } else { "" }),
        0xFF8264 | 0xFF8265 => format!("the picture moved {} pixel{} left", byte & 15, if byte & 15 == 1 { "" } else { "s" }),
        0xFF820E => format!("{} words skipped after each line", word & 0x1FF),
        0xFF8262 => {
            let mode = match (word >> 8) & 7 {
                0 => "ST low",
                1 => "ST medium",
                2 => "ST high",
                4 => "TT medium: 640x480, 16 colours",
                6 => "TT high: 1280x960, black and white",
                7 => "TT low: 320x480, 256 colours",
                _ => "not a mode",
            };
            format!("{mode}; ST colours from bank {}{}{}", word & 15, if word & 0x1000 != 0 { ", hyper mono" } else { "" }, if word & 0x8000 != 0 { ", sample-and-hold" } else { "" })
        }
        0xFF8400 => {
            let [r, g, b] = [(word >> 8) & 15, (word >> 4) & 15, word & 15];
            format!("TT colour {}: {} red {r}, green {g}, blue {b}", register.index(address), swatch([r as u8 * 17, g as u8 * 17, b as u8 * 17]))
        }
        0xFF9800 => {
            let [r, g, b] = [(value >> 26) & 63, (value >> 18) & 63, (value >> 2) & 63];
            let eight = |v: u32| ((v * 255 + 31) / 63) as u8;
            format!("Falcon colour {}: {} red {r}, green {g}, blue {b}", register.index(address), swatch([eight(r), eight(g), eight(b)]))
        }
        0xFF8266 => {
            if word & 0x400 != 0 {
                "2 colours".into()
            } else if word & 0x100 != 0 {
                "true colour, 16 bits a pixel".into()
            } else if word & 0x10 != 0 {
                "256 colours".into()
            } else {
                "16 colours (or an ST mode)".into()
            }
        }
        0xFF8E21 => format!("{} MHz, cache {}", if byte & 2 != 0 { 16 } else { 8 }, if byte & 1 != 0 { "on" } else { "off" }),
        0xFF8007 => format!("processor {} MHz, blitter {} MHz{}{}", if byte & 1 != 0 { 16 } else { 8 }, if byte & 4 != 0 { 16 } else { 8 }, if byte & 8 != 0 { ", blitter off" } else { "" }, if byte & 0x20 == 0 { ", STE bus timing" } else { "" }),
        // The MFP.
        0xFFFA07 | 0xFFFA0B | 0xFFFA0F | 0xFFFA13 => format!("channels on: {}", set(INTERRUPTS_A)),
        0xFFFA09 | 0xFFFA0D | 0xFFFA11 | 0xFFFA15 => format!("channels on: {}", set(INTERRUPTS_B)),
        0xFFFA01 | 0xFFFA03 | 0xFFFA05 => format!("bits set: {}", set(GPIP)),
        0xFFFA17 => format!("vectors ${:02X}-${:02X} (at ${:03X}-${:03X}), {} end of interrupt", byte & 0xF0, (byte & 0xF0) + 15, (byte as u32 & 0xF0) * 4, ((byte as u32 & 0xF0) + 15) * 4, if byte & 8 != 0 { "software" } else { "automatic" }),
        0xFFFA19 | 0xFFFA1B => {
            let timer = if register.address == 0xFFFA19 { "Timer A" } else { "Timer B" };
            let (mode, _) = timer_mode(byte);
            format!("{timer}: {mode}{}", if byte & 0x10 != 0 { ", output reset" } else { "" })
        }
        0xFFFA1D => format!("Timer C: {}; Timer D: {}", timer_mode((byte >> 4) & 7).0, timer_mode(byte & 7).0),
        0xFFFA1F | 0xFFFA21 | 0xFFFA23 | 0xFFFA25 => format!("counts {}", if byte == 0 { 256 } else { byte as u32 }),
        0xFFFA29 => {
            let bits = [8, 7, 6, 5][((byte >> 5) & 3) as usize];
            let stop = ["synchronous", "1 stop bit", "1.5 stop bits", "2 stop bits"][((byte >> 3) & 3) as usize];
            let parity = if byte & 4 == 0 { "no parity" } else if byte & 2 != 0 { "even parity" } else { "odd parity" };
            format!("{bits} bits, {parity}, {stop}{}", if byte & 0x80 != 0 { ", clock divided by 16" } else { "" })
        }
        // The YM.
        0xFF8800 => {
            let r = (byte & 15) as usize;
            format!("chooses register {r}: {}", YM[r].name)
        }
        // The ACIAs.
        0xFFFC00 | 0xFFFC04 => {
            if byte & 3 == 3 {
                return "master reset".into();
            }
            let divide = [1, 16, 64][(byte & 3) as usize];
            let format = ["7 bits, even parity, 2 stop", "7 bits, odd parity, 2 stop", "7 bits, even parity, 1 stop", "7 bits, odd parity, 1 stop", "8 bits, no parity, 2 stop", "8 bits, no parity, 1 stop", "8 bits, even parity, 1 stop", "8 bits, odd parity, 1 stop"][((byte >> 2) & 7) as usize];
            let tx = ["no transmit interrupt", "transmit interrupt", "RTS high, no transmit interrupt", "sends a break"][((byte >> 5) & 3) as usize];
            format!("clock divided by {divide}, {format}, {tx}, receive interrupt {}", if byte & 0x80 != 0 { "on" } else { "off" })
        }
        // DMA sound.
        0xFF8900 => match word & 3 {
            0 => "sound off".into(),
            1 => "plays the sample once".into(),
            _ => "plays the sample, looping".into(),
        },
        0xFF8921 => format!("{} Hz, {}", ["6258", "12517", "25033", "50066"][(byte & 3) as usize], if byte & 0x80 != 0 { "mono" } else { "stereo" }),
        0xFF8922 => microwire(word),
        // The blitter.
        0xFF8A3A => format!("HOP {}: the source is {}", byte & 3, HOPS[(byte & 3) as usize]),
        0xFF8A3B => format!("OP {}: D = {}", byte & 15, OPS[(byte & 15) as usize]),
        0xFF8A3C => {
            let mut parts = Vec::new();
            if byte & 0x80 != 0 {
                parts.push("starts the blit".to_string());
            }
            parts.push(if byte & 0x40 != 0 { "hog mode (the processor waits)".into() } else { "shares the bus with the processor".into() });
            if byte & 0x20 != 0 {
                parts.push("smudge".into());
            }
            parts.push(format!("halftone line {}", byte & 15));
            parts.join(", ")
        }
        0xFF8A3D => {
            let mut parts = vec![format!("source moved {} bit{} right", byte & 15, if byte & 15 == 1 { "" } else { "s" })];
            if byte & 0x80 != 0 {
                parts.push("FXSR: an extra read before each line".into());
            }
            if byte & 0x40 != 0 {
                parts.push("NFSR: no read for each line's last word".into());
            }
            parts.join(", ")
        }
        0xFF8A20 | 0xFF8A22 | 0xFF8A2E | 0xFF8A30 => format!("{} bytes", word as i16),
        0xFF8A28 | 0xFF8A2A | 0xFF8A2C => format!("%{word:016b}: {} bits written", word.count_ones()),
        0xFF8A36 | 0xFF8A38 => format!("{}", if word == 0 { 65536 } else { word as u32 }),
        0xFF8A24 | 0xFF8A32 => format!("${:06X}", value & 0xFF_FFFE),
        _ => match register.fields {
            [] => format!("${value:0w$X} ({value})", w = (register.size.bytes() * 2) as usize),
            fields if fields.iter().all(|f| f.high == f.low) => format!("bits set: {}", set(fields)),
            _ => format!("${value:0w$X}", w = (register.size.bytes() * 2) as usize),
        },
    }
}

/// A Microwire command for the STE's LMC1992 volume and tone chip, read.
pub fn microwire(word: u16) -> String {
    if (word >> 9) & 3 != 2 {
        return format!("${word:04X}: not for the volume and tone chip (its address is %10 in bits 10-9)");
    }
    let v = word & 0x3F;
    match (word >> 6) & 7 {
        0 => format!("mixing: {}", ["the YM at -12 dB", "the YM mixed in", "the YM left out", "not used"][(v & 3) as usize]),
        1 => format!("bass {:+} dB", (v.min(12) as i32 - 6) * 2),
        2 => format!("treble {:+} dB", (v.min(12) as i32 - 6) * 2),
        3 => format!("master volume {} dB", (v.min(40) as i32 - 40) * 2),
        4 => format!("right volume {} dB", ((v & 31).min(20) as i32 - 20) * 2),
        5 => format!("left volume {} dB", ((v & 31).min(20) as i32 - 20) * 2),
        n => format!("command {n}, which the chip doesn't have"),
    }
}

/// Each field's value, a line apiece.
pub fn field_lines(register: &Register, value: u32) -> Vec<String> {
    let width = register.fields.iter().map(|f| f.name.len()).max().unwrap_or(0);
    register
        .fields
        .iter()
        .map(|f| {
            let v = f.value(value);
            let shown = if f.high - f.low >= 4 { format!("${v:X}") } else { format!("{v}") };
            format!("{:>5}  {:<width$}  {shown:>4}  {}", f.bits(), f.name, f.about)
        })
        .collect()
}

/// A register described: its heading, what it's for, and its bits.
pub fn describe(register: &Register) -> String {
    let mut lines = vec![register.heading(), register.about.clone()];
    let width = register.fields.iter().chain(status_fields(register)).map(|f| f.name.len()).max().unwrap_or(0);
    for f in register.fields {
        lines.push(format!("{:>5}  {:<width$}  {}", f.bits(), f.name, f.about));
    }
    if !status_fields(register).is_empty() {
        lines.push("Read, it's the status:".into());
        for f in status_fields(register) {
            lines.push(format!("{:>5}  {:<width$}  {}", f.bits(), f.name, f.about));
        }
    }
    lines.join("\n")
}

/// The bits a register has when it's read, where they aren't the ones written: the ACIAs' status.
pub fn status_fields(register: &Register) -> &'static [Field] {
    if is_acia_control(register) { ACIA_STATUS } else { &[] }
}

/// A value written to a register described: the register, what the value does, and its bits.
pub fn describe_value(register: &Register, address: u32, value: u32) -> String {
    let digits = (register.size.bytes() * 2) as usize;
    let value = if register.size == Size::Long { value } else { value & ((1u32 << (digits * 4)) - 1) };
    let name = if register.count > 1 { format!("{} {}", register.name, register.index(address)) } else { register.name.clone() };
    let mut lines = vec![register.heading(), format!("{name} = ${value:0digits$X}: {}", explain(register, address, value))];
    if register.st_colour().is_none() {
        lines.extend(field_lines(register, value));
    }
    lines.join("\n")
}

/// What a bit of a register is, as `btst`, `bset` and the like name it.
pub fn describe_bit(register: &Register, n: u8) -> String {
    let field = register.fields.iter().find(|f| f.low <= n && n <= f.high);
    let what = match field {
        Some(f) if f.high == f.low => format!("bit {n}: {} - {}", f.name, f.about),
        Some(f) => format!("bit {n}, part of {} (bits {}): {}", f.name, f.bits(), f.about),
        None => format!("bit {n}: not used"),
    };
    format!("{}\n{what}", register.heading())
}

/// The registers as EQU lines, for a source to include: a chip's, or all of them.
pub fn equates(chip: Option<Chip>) -> String {
    let mut out = String::new();
    for r in registers().iter().filter(|r| chip.is_none_or(|c| r.chip == c)) {
        out.push_str(&format!("{:<16}equ     ${:08X}    ; {}\n", r.name, 0xFF00_0000 | r.address, r.about.split(['.', ':']).next().unwrap_or("")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_are_where_hatari_has_them() {
        // Every address here is one Hatari's tables give a handler, for at least one machine.
        let hatari = [
            0xFF8001, 0xFF8201, 0xFF8203, 0xFF8205, 0xFF8207, 0xFF8209, 0xFF820A, 0xFF820D, 0xFF8240, 0xFF825E, 0xFF8260, 0xFF8264, 0xFF8265, 0xFF8604, 0xFF8606, 0xFF8609, 0xFF860B, 0xFF860D, 0xFF8800, 0xFF8802, 0xFF8900, 0xFF8903,
            0xFF8921, 0xFF8922, 0xFF8924, 0xFF8A00, 0xFF8A1E, 0xFF8A20, 0xFF8A22, 0xFF8A24, 0xFF8A28, 0xFF8A2A, 0xFF8A2C, 0xFF8A2E, 0xFF8A30, 0xFF8A32, 0xFF8A36, 0xFF8A38, 0xFF8A3A, 0xFF8A3B, 0xFF8A3C, 0xFF8A3D, 0xFFFA01, 0xFFFA07,
            0xFFFA09, 0xFFFA17, 0xFFFA19, 0xFFFA1B, 0xFFFA1D, 0xFFFA1F, 0xFFFA21, 0xFFFA23, 0xFFFA25, 0xFFFA2F, 0xFFFC00, 0xFFFC02, 0xFFFC04, 0xFFFC06, 0xFF9200, 0xFF9202, 0xFF8262, 0xFF8400, 0xFF8266, 0xFF8282, 0xFF82C2, 0xFF9800,
            0xFFA202, 0xFF8E21, 0xFF8006, 0xFF8007,
        ];
        for address in hatari {
            assert!(at(address).is_some(), "${address:06X}");
        }
        assert_eq!(at(0xFFFF8240).unwrap().name, "COLOR0");
        assert_eq!(at(0xFF820F).unwrap().name, "LINEWID", "the STE's byte of it");
        assert_eq!(at(0xFF8A26).unwrap().name, "SRCADDR");
        assert_eq!(at(0xFF8A1E).unwrap().index(0xFF8A1E), 15);
        assert!(at(0xFF8A40).is_none());
        // No two overlap.
        for (i, a) in registers().iter().enumerate() {
            for b in &registers()[i + 1..] {
                assert!(a.address + a.span() <= b.address || b.address + b.span() <= a.address, "{} and {}", a.name, b.name);
            }
        }
        assert_eq!(named("tbcr").unwrap().address, 0xFFFA1B);
        assert_eq!(named("color15").unwrap().address, 0xFF825E);
        assert_eq!(named("COLOR07").unwrap().address, 0xFF824E);
    }

    #[test]
    fn colours_read_the_st_and_ste_ways() {
        // The STE's odd bit order, as Hatari converts STE colours for the TT: ((c & $777) << 1) | ((c & $888) >> 3).
        for word in 0..0x1000u16 {
            let hatari = ((word & 0x777) << 1) | ((word & 0x888) >> 3);
            let [r, g, b] = ste_levels(word);
            assert_eq!(((r as u16) << 8) | ((g as u16) << 4) | b as u16, hatari, "${word:03X}");
        }
        for level in 0..16 {
            assert_eq!(ste_gun(ste_nibble(level)), level);
        }
        assert_eq!(st_rgb(0x777), [255, 255, 255]);
        assert_eq!(ste_rgb(0x777), [238, 238, 238]);
        assert_eq!(ste_rgb(0xFFF), [255, 255, 255]);
        assert_eq!(st_rgb(0x700), [255, 0, 0]);
        let said = explain_colour(0, 0x777);
        assert!(said.contains("ST:  \u{25A0}#FFFFFF red 7") && said.contains("STE: \u{25A0}#EEEEEE red 14"), "{said}");
    }

    #[test]
    fn timers_are_worked_out() {
        assert_eq!(timer_mode(8).0, "event count mode (Timer B: one a display line; Timer A: the parallel port's busy line)");
        assert_eq!(timer_mode(7).1, Some(200));
        // TOS's 200 Hz clock: divide by 64, count 192.
        assert!((timer_rate(64, 192) - 200.0).abs() < 1e-9);
        let best = timer_settings(200.0)[0];
        assert!(best.3.abs() < 1e-9, "{best:?}");
        assert!((timer_rate(PRESCALERS[best.0 as usize - 1], best.1) - 200.0).abs() < 1e-9);
        // 50 Hz is exact with 200 and 245.76? Not whole: the nearest is within a percent.
        let fifty = timer_settings(50.0)[0];
        assert!(fifty.3.abs() < 0.5, "{fifty:?}");
        // Every setting found is one the MFP can do, and the rate is what it says.
        for hz in [1.0, 10.0, 60.0, 440.0, 1000.0, 8000.0, 12288.0, 50000.0, 600000.0] {
            for (control, data, rate, error) in timer_settings(hz) {
                assert!((1..=7).contains(&control));
                assert!((timer_rate(PRESCALERS[control as usize - 1], data) - rate).abs() < 1e-9);
                assert!(((rate - hz) / hz * 100.0 - error).abs() < 1e-9);
            }
        }
        assert!(timer_settings(0.0).is_empty());
    }

    #[test]
    fn values_are_explained() {
        let tbcr = at(0xFFFA1B).unwrap();
        assert!(explain(tbcr, 0xFFFA1B, 8).starts_with("Timer B: event count mode"));
        assert_eq!(explain(at(0xFFFA07).unwrap(), 0xFFFA07, 0x21), "channels on: TIMERA, TIMERB");
        assert_eq!(explain(at(0xFFFA17).unwrap(), 0xFFFA17, 0x48), "vectors $40-$4F (at $100-$13C), software end of interrupt");
        assert_eq!(explain(at(0xFF8800).unwrap(), 0xFF8800, 7), "chooses register 7: Mixer");
        assert_eq!(explain_ym(7, 0x3F), "tone on none; noise on none; port A input, port B input");
        assert_eq!(explain_ym(7, 0xF8), "tone on A, B, C; noise on none; port A output, port B output");
        assert_eq!(explain_ym(8, 0x10), "channel A follows the envelope");
        assert_eq!(explain(at(0xFF8A3B).unwrap(), 0xFF8A3B, 3), "OP 3: D = S (copy)");
        assert_eq!(explain(at(0xFF8A3A).unwrap(), 0xFF8A3A, 2), "HOP 2: the source is the source");
        assert_eq!(explain(at(0xFF8A3D).unwrap(), 0xFF8A3D, 0x84), "source moved 4 bits right, FXSR: an extra read before each line");
        assert_eq!(explain(at(0xFF8921).unwrap(), 0xFF8921, 0x81), "12517 Hz, mono");
        assert_eq!(microwire(0x4C0 | 40), "master volume 0 dB");
        assert_eq!(microwire(0x540 | 20), "left volume 0 dB");
        assert_eq!(microwire(0x401), "mixing: the YM mixed in");
        assert_eq!(explain(at(0xFFFC00).unwrap(), 0xFFFC00, 0x96), "clock divided by 64, 8 bits, no parity, 1 stop, no transmit interrupt, receive interrupt on");
        assert_eq!(explain(at(0xFF8260).unwrap(), 0xFF8260, 0), "low resolution: 320x200, 16 colours");
        let said = describe_value(at(0xFF9800).unwrap(), 0xFF9804, 0xFC00_00FC);
        assert!(said.contains("FALCONPALETTE 1 = $FC0000FC: Falcon colour 1: \u{25A0}#FF00FF red 63, green 0, blue 63"), "{said}");
        assert!(describe_bit(at(0xFFFA0F).unwrap(), 0).contains("TIMERB"));
        assert!(equates(Some(Chip::Mfp)).contains("TBCR            equ     $FFFFFA1B"));
    }
}
