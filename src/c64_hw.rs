//! The Commodore 64's hardware, for demo coders: the VIC-II's registers at $D000-$D02E, the SID's at
//! $D400-$D41C, the two CIAs' at $DC00 and $DD00, the 6510's own port at $00 and $01, and the KERNAL's entry
//! points and vectors; with what a value written to one means (a colour, the screen mode, where the screen,
//! characters and bitmap are, a note, an envelope's times, which ROMs are switched in, which keys are read).
//!
//! Names, addresses and bit positions are the hardware's, checked against VICE's sources (vicii, sid, cia and
//! c64mem) and the KERNAL's jump table; the words describing them are Limoniter's own.

use std::sync::OnceLock;

/// The PAL machine's clock: 985248 Hz.
pub const PAL_CLOCK: f64 = 985_248.0;
/// The NTSC machine's clock: 1022727 Hz.
pub const NTSC_CLOCK: f64 = 1_022_727.0;

/// The sixteen colours' names.
pub const COLOUR_NAMES: [&str; 16] = ["black", "white", "red", "cyan", "purple", "green", "blue", "yellow", "orange", "brown", "light red", "dark grey", "grey", "light green", "light blue", "light grey"];

/// The colours as Philip Timmermann ("Pepto") measured them, as VICE's pepto-pal palette has them.
pub const PEPTO: [u32; 16] = [0x000000, 0xFFFFFF, 0x68372B, 0x70A4B2, 0x6F3D86, 0x588D43, 0x352879, 0xB8C76F, 0x6F4F25, 0x433900, 0x9A6759, 0x444444, 0x6C6C6C, 0x9AD284, 0x6C5EB5, 0x959595];

/// The colours as the Colodore palette has them, as VICE's colodore palette has them.
pub const COLODORE: [u32; 16] = [0x000000, 0xFFFFFF, 0x96282E, 0x5BD6CE, 0x9F2DAD, 0x41B936, 0x2724C4, 0xEFF347, 0x9F4815, 0x5E3500, 0xDA5F66, 0x474747, 0x787878, 0x91FF84, 0x6864FF, 0xAEAEAE];

pub fn rgb(colour: u32) -> [u8; 3] {
    [(colour >> 16) as u8, (colour >> 8) as u8, colour as u8]
}

/// The chips the guide groups registers by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Chip {
    Vic,
    Sid,
    Cia1,
    Cia2,
    CpuPort,
    Kernal,
}

impl Chip {
    pub const ALL: [Chip; 6] = [Chip::Vic, Chip::Sid, Chip::Cia1, Chip::Cia2, Chip::CpuPort, Chip::Kernal];

    pub fn name(self) -> &'static str {
        match self {
            Chip::Vic => "VIC-II (video)",
            Chip::Sid => "SID (sound)",
            Chip::Cia1 => "CIA 1 (keyboard, joysticks, IRQ)",
            Chip::Cia2 => "CIA 2 (VIC bank, serial bus, NMI)",
            Chip::CpuPort => "6510 port (memory setup)",
            Chip::Kernal => "KERNAL and vectors",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
    /// A KERNAL routine, called with JSR.
    Call,
    /// An address holding an address: a vector.
    Vector,
}

impl Access {
    pub fn name(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::ReadWrite => "read/write",
            Access::Call => "JSR",
            Access::Vector => "vector (two bytes, low first)",
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

    pub fn value(&self, v: u8) -> u8 {
        let width = self.high - self.low + 1;
        let mask = if width >= 8 { 0xFF } else { (1u8 << width) - 1 };
        (v >> self.low) & mask
    }
}

const fn bit(n: u8, name: &'static str, about: &'static str) -> Field {
    Field { high: n, low: n, name, about }
}

const fn bits(high: u8, low: u8, name: &'static str, about: &'static str) -> Field {
    Field { high, low, name, about }
}

/// A register, or a KERNAL routine or vector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Register {
    pub address: u16,
    /// The usual label for it in source (EXTCOL, CHROUT).
    pub name: String,
    /// What it is, in a few words.
    pub title: String,
    pub chip: Chip,
    pub access: Access,
    pub about: String,
    pub fields: &'static [Field],
}

impl Register {
    /// The bytes it covers: two for a vector.
    pub fn span(&self) -> u16 {
        if self.access == Access::Vector { 2 } else { 1 }
    }

    pub fn heading(&self) -> String {
        format!("{}  ${:04X}  {}  ({})", self.name, self.address, self.title, self.access.name())
    }

    /// Which of the SID's voices it's for (1-3), if it's one of theirs.
    pub fn voice(&self) -> Option<u16> {
        (self.chip == Chip::Sid && self.address < 0xD415).then(|| (self.address - 0xD400) / 7 + 1)
    }

    /// The VIC-II's colour register this is, if it's one.
    pub fn is_colour(&self) -> bool {
        self.chip == Chip::Vic && (0xD020..=0xD02E).contains(&self.address)
    }
}

// ---------- The bits ----------

const SPRITE_X: &[Field] = &[bits(7, 0, "X", "Bits 0-7 of the sprite's position across; bit 8 is in $D010")];
const SPRITE_Y: &[Field] = &[bits(7, 0, "Y", "The sprite's line down; the screen's 25 rows are lines 50-249")];
const EIGHT_SPRITES: &[Field] = &[
    bit(7, "S7", "Sprite 7"),
    bit(6, "S6", "Sprite 6"),
    bit(5, "S5", "Sprite 5"),
    bit(4, "S4", "Sprite 4"),
    bit(3, "S3", "Sprite 3"),
    bit(2, "S2", "Sprite 2"),
    bit(1, "S1", "Sprite 1"),
    bit(0, "S0", "Sprite 0"),
];
const CONTROL1: &[Field] = &[
    bit(7, "RST8", "Bit 8 of the raster line: read, of the line being drawn; written, of the line to interrupt on"),
    bit(6, "ECM", "Extended background colour text: four background colours, from the top two bits of each character"),
    bit(5, "BMM", "Bitmap mode"),
    bit(4, "DEN", "Display on (0 blanks the screen to the border colour, and frees the bus cycles the VIC was taking)"),
    bit(3, "RSEL", "25 rows (1) or 24 (0), for scrolling up and down"),
    bits(2, 0, "YSCROLL", "Scroll down by 0-7 lines; 3 is where the screen normally sits"),
];
const RASTER: &[Field] = &[bits(7, 0, "RASTER", "Read: the low 8 bits of the line being drawn. Written: the line to interrupt on, with bit 8 in $D011")];
const LIGHT_PEN: &[Field] = &[bits(7, 0, "LP", "Where the light pen was: across in 2-pixel steps, or the line down")];
const CONTROL2: &[Field] = &[
    bit(4, "MCM", "Multicolour: in text, characters with colour 8-15 are 4 wide pixels in 4 colours; in a bitmap, every cell is"),
    bit(3, "CSEL", "40 columns (1) or 38 (0), for scrolling sideways"),
    bits(2, 0, "XSCROLL", "Scroll right by 0-7 pixels"),
];
const MEMORY: &[Field] = &[
    bits(7, 4, "VM", "Where the screen (the character codes) is: this times $0400 in the VIC's bank; the sprite pointers are its last 8 bytes"),
    bits(3, 1, "CB", "Where the characters are: this times $0800 in the bank. In bitmap mode only bit 3 counts: the bitmap is at $0000 or $2000"),
];
const INTERRUPTS: &[Field] = &[
    bit(7, "IRQ", "Read: set when any of the enabled ones below has happened"),
    bit(3, "ILP", "The light pen fired"),
    bit(2, "IMMC", "Two sprites touched"),
    bit(1, "IMBC", "A sprite touched the background"),
    bit(0, "IRST", "The raster line in $D012 and $D011 bit 7 was reached"),
];
const INTERRUPT_ENABLE: &[Field] = &[
    bit(3, "ELP", "Interrupt when the light pen fires"),
    bit(2, "EMMC", "Interrupt when sprites touch each other"),
    bit(1, "EMBC", "Interrupt when a sprite touches the background"),
    bit(0, "ERST", "Interrupt on the raster line set in $D012"),
];
const COLOUR: &[Field] = &[bits(3, 0, "COLOUR", "One of the 16 colours (the top four bits aren't there: they read as 1s)")];

const FREQ_LO: &[Field] = &[bits(7, 0, "FREQ", "The low byte of the voice's 16-bit frequency")];
const FREQ_HI: &[Field] = &[bits(7, 0, "FREQ", "The high byte of the voice's 16-bit frequency: Hz = value x clock / 16777216")];
const PW_LO: &[Field] = &[bits(7, 0, "PW", "The low byte of the 12-bit pulse width")];
const PW_HI: &[Field] = &[bits(3, 0, "PW", "The top four bits of the 12-bit pulse width: $800 is a square wave")];
const VOICE_CONTROL: &[Field] = &[
    bit(7, "NOISE", "Noise"),
    bit(6, "PULSE", "Pulse, as wide as the pulse width says"),
    bit(5, "SAW", "Sawtooth"),
    bit(4, "TRI", "Triangle"),
    bit(3, "TEST", "Holds the oscillator at zero (and the noise generator, to unstick it)"),
    bit(2, "RING", "Ring modulation of the triangle by the voice before (voice 3 for voice 1)"),
    bit(1, "SYNC", "Restart this voice's oscillator with the voice before's"),
    bit(0, "GATE", "1 starts the attack, decay and sustain; 0 starts the release"),
];
const ATTACK_DECAY: &[Field] = &[bits(7, 4, "ATTACK", "How long to rise to full: 2 ms (0) to 8 s (15)"), bits(3, 0, "DECAY", "How long to fall to the sustain level: 6 ms (0) to 24 s (15)")];
const SUSTAIN_RELEASE: &[Field] = &[bits(7, 4, "SUSTAIN", "The level held while the gate is on: 0 silent to 15 full"), bits(3, 0, "RELEASE", "How long to fall to silence once the gate is off: 6 ms (0) to 24 s (15)")];
const CUTOFF_LO: &[Field] = &[bits(2, 0, "FC", "The low 3 bits of the 11-bit filter cutoff")];
const CUTOFF_HI: &[Field] = &[bits(7, 0, "FC", "The top 8 bits of the 11-bit filter cutoff")];
const RESONANCE: &[Field] = &[
    bits(7, 4, "RES", "Resonance: 0 none to 15 most, a peak at the cutoff"),
    bit(3, "FILTEX", "Filter the external input"),
    bit(2, "FILT3", "Filter voice 3"),
    bit(1, "FILT2", "Filter voice 2"),
    bit(0, "FILT1", "Filter voice 1"),
];
const MODE_VOLUME: &[Field] = &[
    bit(7, "3OFF", "Voice 3 silent (unless it's filtered): for using it only to read OSC3 and ENV3"),
    bit(6, "HP", "High-pass filter"),
    bit(5, "BP", "Band-pass filter"),
    bit(4, "LP", "Low-pass filter"),
    bits(3, 0, "VOL", "The volume of everything, 0-15 (writing it clicks: the old way to play samples)"),
];
const POT: &[Field] = &[bits(7, 0, "POT", "A paddle's (or the mouse's) position, 0-255, from the control port CIA 1 chooses")];
const OSC3: &[Field] = &[bits(7, 0, "OSC3", "The top 8 bits of voice 3's waveform: random numbers when it plays noise")];
const ENV3: &[Field] = &[bits(7, 0, "ENV3", "Voice 3's envelope level now")];

const KEYBOARD_COLUMNS: &[Field] = &[
    bits(7, 0, "COLUMNS", "Written: a 0 bit chooses each keyboard column to read through $DC01"),
    bit(4, "FIRE2", "Read: joystick 2's fire button (0 pressed)"),
    bit(3, "RIGHT2", "Read: joystick 2 right"),
    bit(2, "LEFT2", "Read: joystick 2 left"),
    bit(1, "DOWN2", "Read: joystick 2 down"),
    bit(0, "UP2", "Read: joystick 2 up"),
];
const KEYBOARD_ROWS: &[Field] = &[
    bits(7, 0, "ROWS", "Read: a 0 bit for each row with a key down in the columns chosen at $DC00"),
    bit(4, "FIRE1", "Joystick 1's fire button (0 pressed)"),
    bit(3, "RIGHT1", "Joystick 1 right"),
    bit(2, "LEFT1", "Joystick 1 left"),
    bit(1, "DOWN1", "Joystick 1 down"),
    bit(0, "UP1", "Joystick 1 up"),
];
const DIRECTION: &[Field] = &[bits(7, 0, "DDR", "Each bit: 1 makes that bit of the port an output, 0 an input")];
const TIMER_LO: &[Field] = &[bits(7, 0, "LO", "Read: the low byte of the count now. Written: the low byte it starts from")];
const TIMER_HI: &[Field] = &[bits(7, 0, "HI", "Read: the high byte of the count now. Written: the high byte it starts from (loaded when it's stopped)")];
const TOD_TENTHS: &[Field] = &[bits(3, 0, "TENTHS", "Tenths of a second, 0-9 (writing it starts the clock; reading it frees the latch)")];
const TOD_BCD: &[Field] = &[bits(6, 4, "TENS", "Tens, in BCD"), bits(3, 0, "UNITS", "Units, in BCD")];
const TOD_HOURS: &[Field] = &[bit(7, "PM", "PM"), bit(4, "TEN", "Ten hours"), bits(3, 0, "HOURS", "Hours, in BCD (writing it stops the clock until the tenths are written; reading it latches all four)")];
const SERIAL: &[Field] = &[bits(7, 0, "SDR", "The byte the serial port shifts in or out")];
const ICR: &[Field] = &[
    bit(7, "SET", "Written: 1 sets, 0 clears the bits written as 1. Read: set when any enabled one has happened"),
    bit(4, "FLAG", "The FLAG pin (the cassette's read line on CIA 1; the user port on CIA 2)"),
    bit(3, "SP", "The serial port has sent or received a byte"),
    bit(2, "ALARM", "The clock reached the alarm time"),
    bit(1, "TB", "Timer B ran out"),
    bit(0, "TA", "Timer A ran out"),
];
const CRA: &[Field] = &[
    bit(7, "TODIN", "The time-of-day clock is counted from 50 Hz (1) or 60 Hz (0) mains"),
    bit(6, "SPMODE", "The serial port sends (1) or receives (0)"),
    bit(5, "INMODE", "Count the CNT pin (1) or the clock (0)"),
    bit(4, "LOAD", "Load the start value now (it reads back as 0)"),
    bit(3, "RUNMODE", "Stop when it runs out (1), or start again (0)"),
    bit(2, "OUTMODE", "On port B bit 6: toggle (1) or a pulse (0)"),
    bit(1, "PBON", "Show it running out on port B bit 6"),
    bit(0, "START", "Run the timer"),
];
const CRB: &[Field] = &[
    bit(7, "ALARM", "Writes to the time-of-day clock set the alarm (1) or the time (0)"),
    bits(6, 5, "INMODE", "Count: 0 the clock, 1 the CNT pin, 2 timer A running out, 3 timer A running out while CNT is high"),
    bit(4, "LOAD", "Load the start value now"),
    bit(3, "RUNMODE", "Stop when it runs out (1), or start again (0)"),
    bit(2, "OUTMODE", "On port B bit 7: toggle (1) or a pulse (0)"),
    bit(1, "PBON", "Show it running out on port B bit 7"),
    bit(0, "START", "Run the timer"),
];
const CIA2_PORT_A: &[Field] = &[
    bit(7, "DATAIN", "The serial bus's data line, in"),
    bit(6, "CLKIN", "The serial bus's clock line, in"),
    bit(5, "DATAOUT", "The serial bus's data line, out"),
    bit(4, "CLKOUT", "The serial bus's clock line, out"),
    bit(3, "ATNOUT", "The serial bus's ATN line, out"),
    bit(2, "PA2", "The user port's PA2 (RS-232 data out)"),
    bits(1, 0, "VA15-14", "The VIC's 16K bank, inverted: 3 $0000, 2 $4000, 1 $8000, 0 $C000"),
];
const CIA2_PORT_B: &[Field] = &[bits(7, 0, "PB", "The user port's eight data lines (RS-232's control lines)")];
const CPU_DDR: &[Field] = &[bits(7, 0, "DDR", "Each bit: 1 makes that bit of $01 an output. The KERNAL sets $2F")];
const CPU_PORT: &[Field] = &[
    bit(5, "MOTOR", "The cassette motor: 0 on"),
    bit(4, "SENSE", "Read: 0 when a cassette button is down"),
    bit(3, "WRITE", "The cassette's write line"),
    bit(2, "CHAREN", "With either of the others set: I/O at $D000 (1) or the character ROM (0)"),
    bit(1, "HIRAM", "The KERNAL ROM at $E000 (with LORAM, BASIC too)"),
    bit(0, "LORAM", "With HIRAM: the BASIC ROM at $A000"),
];

struct Builder(Vec<Register>);

impl Builder {
    fn add(&mut self, address: u16, name: &str, title: &str, chip: Chip, access: Access, about: &str, fields: &'static [Field]) {
        self.0.push(Register { address, name: name.into(), title: title.into(), chip, access, about: about.into(), fields });
    }
}

fn build() -> Vec<Register> {
    use Access::*;
    use Chip::*;
    let mut b = Builder(Vec::new());
    // The VIC-II.
    for n in 0..8u16 {
        b.add(0xD000 + n * 2, &format!("SP{n}X"), &format!("Sprite {n} X"), Vic, ReadWrite, &crate::i18n::tf!("Where sprite {n} is across: the low 8 bits, with the ninth in $D010. 24 is the screen's left edge."), SPRITE_X);
        b.add(0xD001 + n * 2, &format!("SP{n}Y"), &format!("Sprite {n} Y"), Vic, ReadWrite, &crate::i18n::tf!("Where sprite {n} is down, by raster line. 50 is the screen's top row."), SPRITE_Y);
    }
    b.add(0xD010, "MSIGX", "Sprites' X bit 8", Vic, ReadWrite, "The ninth bit of each sprite's X position, for the right of the screen (X 256-343).", EIGHT_SPRITES);
    b.add(0xD011, "SCROLY", "Control register 1", Vic, ReadWrite, "Vertical scroll, 24 or 25 rows, the display on or off, bitmap and extended colour modes, and bit 8 of the raster line.", CONTROL1);
    b.add(0xD012, "RASTER", "Raster line", Vic, ReadWrite, "The line being drawn (read), or the line to interrupt on (written). A PAL machine has 312 lines, NTSC 263.", RASTER);
    b.add(0xD013, "LPENX", "Light pen X", Vic, Read, "Where the light pen saw the beam, across.", LIGHT_PEN);
    b.add(0xD014, "LPENY", "Light pen Y", Vic, Read, "Where the light pen saw the beam, down.", LIGHT_PEN);
    b.add(0xD015, "SPENA", "Sprites on", Vic, ReadWrite, "A bit for each sprite to show it.", EIGHT_SPRITES);
    b.add(0xD016, "SCROLX", "Control register 2", Vic, ReadWrite, "Horizontal scroll, 38 or 40 columns, and multicolour mode.", CONTROL2);
    b.add(0xD017, "YXPAND", "Sprites twice as tall", Vic, ReadWrite, "A bit for each sprite to stretch it to twice its height.", EIGHT_SPRITES);
    b.add(0xD018, "VMCSB", "Memory setup", Vic, ReadWrite, "Where in the VIC's 16K bank (set by CIA 2's $DD00) the screen and the characters or bitmap are.", MEMORY);
    b.add(0xD019, "VICIRQ", "Interrupts happened", Vic, ReadWrite, "Which of the VIC's interrupts have happened. Write a 1 to a bit to say it's been dealt with (a raster interrupt routine writes $01 or $FF, or does ASL $D019).", INTERRUPTS);
    b.add(0xD01A, "IRQMSK", "Interrupts on", Vic, ReadWrite, "Which of the VIC's interrupts reach the processor's IRQ.", INTERRUPT_ENABLE);
    b.add(0xD01B, "SPBGPR", "Sprites behind", Vic, ReadWrite, "A bit for each sprite to put it behind the foreground of the characters or bitmap.", EIGHT_SPRITES);
    b.add(0xD01C, "SPMC", "Sprites multicolour", Vic, ReadWrite, "A bit for each sprite to make it 12 wide pixels in 3 colours ($D025, $D026 and its own).", EIGHT_SPRITES);
    b.add(0xD01D, "XXPAND", "Sprites twice as wide", Vic, ReadWrite, "A bit for each sprite to stretch it to twice its width.", EIGHT_SPRITES);
    b.add(0xD01E, "SPSPCL", "Sprite-sprite touches", Vic, Read, "A bit for each sprite that has touched another. Reading it clears it.", EIGHT_SPRITES);
    b.add(0xD01F, "SPBGCL", "Sprite-background touches", Vic, Read, "A bit for each sprite that has touched the foreground. Reading it clears it.", EIGHT_SPRITES);
    b.add(0xD020, "EXTCOL", "Border colour", Vic, ReadWrite, "The border's colour.", COLOUR);
    b.add(0xD021, "BGCOL0", "Background colour", Vic, ReadWrite, "The background's colour (in bitmap mode, the multicolour bitmap's background).", COLOUR);
    b.add(0xD022, "BGCOL1", "Background colour 1", Vic, ReadWrite, "Multicolour text's colour for bit pair %01, and extended colour text's second background.", COLOUR);
    b.add(0xD023, "BGCOL2", "Background colour 2", Vic, ReadWrite, "Multicolour text's colour for bit pair %10, and extended colour text's third background.", COLOUR);
    b.add(0xD024, "BGCOL3", "Background colour 3", Vic, ReadWrite, "Extended colour text's fourth background.", COLOUR);
    b.add(0xD025, "SPMC0", "Sprite multicolour 0", Vic, ReadWrite, "The colour multicolour sprites show for bit pair %01.", COLOUR);
    b.add(0xD026, "SPMC1", "Sprite multicolour 1", Vic, ReadWrite, "The colour multicolour sprites show for bit pair %11.", COLOUR);
    for n in 0..8u16 {
        b.add(0xD027 + n, &format!("SP{n}COL"), &crate::i18n::tf!("Sprite {n} colour"), Vic, ReadWrite, &crate::i18n::tf!("Sprite {n}'s own colour (bit pair %10 in multicolour)."), COLOUR);
    }
    // The SID.
    for voice in 0..3u16 {
        let base = 0xD400 + voice * 7;
        let v = voice + 1;
        b.add(base, &format!("FRELO{v}"), &crate::i18n::tf!("Voice {v} frequency, low"), Sid, Write, &crate::i18n::tf!("The low byte of voice {v}'s pitch."), FREQ_LO);
        b.add(base + 1, &format!("FREHI{v}"), &crate::i18n::tf!("Voice {v} frequency, high"), Sid, Write, &crate::i18n::tf!("The high byte of voice {v}'s pitch."), FREQ_HI);
        b.add(base + 2, &format!("PWLO{v}"), &crate::i18n::tf!("Voice {v} pulse width, low"), Sid, Write, &crate::i18n::tf!("The low byte of voice {v}'s pulse width, for the pulse wave."), PW_LO);
        b.add(base + 3, &format!("PWHI{v}"), &crate::i18n::tf!("Voice {v} pulse width, high"), Sid, Write, &crate::i18n::tf!("The top four bits of voice {v}'s pulse width."), PW_HI);
        b.add(base + 4, &format!("VCREG{v}"), &crate::i18n::tf!("Voice {v} control"), Sid, Write, &crate::i18n::tf!("Voice {v}'s waveform, its gate and how it's tied to the voice before."), VOICE_CONTROL);
        b.add(base + 5, &format!("ATDCY{v}"), &crate::i18n::tf!("Voice {v} attack and decay"), Sid, Write, &crate::i18n::tf!("How quickly voice {v} rises when gated, and falls to its sustain level."), ATTACK_DECAY);
        b.add(base + 6, &format!("SUREL{v}"), &crate::i18n::tf!("Voice {v} sustain and release"), Sid, Write, &crate::i18n::tf!("The level voice {v} holds, and how quickly it fades once the gate is off."), SUSTAIN_RELEASE);
    }
    b.add(0xD415, "CUTLO", "Filter cutoff, low", Sid, Write, "The low 3 bits of the filter's cutoff frequency.", CUTOFF_LO);
    b.add(0xD416, "CUTHI", "Filter cutoff, high", Sid, Write, "The top 8 bits of the filter's cutoff frequency.", CUTOFF_HI);
    b.add(0xD417, "RESON", "Filter resonance and routing", Sid, Write, "How much the filter rings, and which voices go through it.", RESONANCE);
    b.add(0xD418, "SIGVOL", "Filter mode and volume", Sid, Write, "Which filters are on, voice 3 off, and the volume.", MODE_VOLUME);
    b.add(0xD419, "POTX", "Paddle X", Sid, Read, "The first paddle's position (or a 1351 mouse's X).", POT);
    b.add(0xD41A, "POTY", "Paddle Y", Sid, Read, "The second paddle's position (or a 1351 mouse's Y).", POT);
    b.add(0xD41B, "RANDOM", "Voice 3 oscillator", Sid, Read, "Voice 3's waveform, read: with noise on and a high frequency, a random number every read.", OSC3);
    b.add(0xD41C, "ENV3", "Voice 3 envelope", Sid, Read, "Voice 3's envelope, read: for effects that follow it.", ENV3);
    // The CIAs.
    for (base, chip, n) in [(0xDC00u16, Cia1, 1), (0xDD00, Cia2, 2)] {
        let (port_a, port_b, a_about, b_about): (&'static [Field], &'static [Field], &str, &str) = if n == 1 {
            (KEYBOARD_COLUMNS, KEYBOARD_ROWS, "Port A: which keyboard columns are read (written), and joystick 2 (read).", "Port B: the keyboard rows (read), and joystick 1.")
        } else {
            (CIA2_PORT_A, CIA2_PORT_B, "Port A: the VIC's 16K bank (bits 0 and 1), and the serial bus to the disk drive.", "Port B: the user port.")
        };
        let irq = if n == 1 { "IRQ" } else { "NMI" };
        let prefix = if n == 1 { "CIA1" } else { "CIA2" };
        b.add(base, &format!("{prefix}PRA"), "Port A", chip, ReadWrite, a_about, port_a);
        b.add(base + 1, &format!("{prefix}PRB"), "Port B", chip, ReadWrite, b_about, port_b);
        b.add(base + 2, &format!("{prefix}DDRA"), "Port A direction", chip, ReadWrite, "Which bits of port A are outputs.", DIRECTION);
        b.add(base + 3, &format!("{prefix}DDRB"), "Port B direction", chip, ReadWrite, "Which bits of port B are outputs.", DIRECTION);
        b.add(base + 4, &format!("{prefix}TALO"), "Timer A, low", chip, ReadWrite, "Timer A counts down from this, once a clock cycle unless CRA says otherwise.", TIMER_LO);
        b.add(base + 5, &format!("{prefix}TAHI"), "Timer A, high", chip, ReadWrite, "The high byte of timer A.", TIMER_HI);
        b.add(base + 6, &format!("{prefix}TBLO"), "Timer B, low", chip, ReadWrite, "Timer B counts down from this.", TIMER_LO);
        b.add(base + 7, &format!("{prefix}TBHI"), "Timer B, high", chip, ReadWrite, "The high byte of timer B.", TIMER_HI);
        b.add(base + 8, &format!("{prefix}TOD10"), "Time of day, tenths", chip, ReadWrite, "The time-of-day clock's tenths of a second.", TOD_TENTHS);
        b.add(base + 9, &format!("{prefix}TODSEC"), "Time of day, seconds", chip, ReadWrite, "The time-of-day clock's seconds, in BCD.", TOD_BCD);
        b.add(base + 10, &format!("{prefix}TODMIN"), "Time of day, minutes", chip, ReadWrite, "The time-of-day clock's minutes, in BCD.", TOD_BCD);
        b.add(base + 11, &format!("{prefix}TODHR"), "Time of day, hours", chip, ReadWrite, "The time-of-day clock's hours, in BCD, with AM or PM.", TOD_HOURS);
        b.add(base + 12, &format!("{prefix}SDR"), "Serial data", chip, ReadWrite, "The serial port's shift register.", SERIAL);
        b.add(base + 13, &format!("{prefix}ICR"), "Interrupt control", chip, ReadWrite, &crate::i18n::tf!("Which of this CIA's interrupts reach the processor's {irq} (written), and which have happened (read; reading clears them, so an interrupt routine reads it to say it's been seen)."), ICR);
        b.add(base + 14, &format!("{prefix}CRA"), "Timer A control", chip, ReadWrite, "Starts and stops timer A, and how it counts.", CRA);
        b.add(base + 15, &format!("{prefix}CRB"), "Timer B control", chip, ReadWrite, "Starts and stops timer B, and how it counts.", CRB);
    }
    // The 6510's own port.
    b.add(0x0000, "D6510", "Processor port direction", CpuPort, ReadWrite, "Which bits of $01 are outputs.", CPU_DDR);
    b.add(0x0001, "R6510", "Processor port", CpuPort, ReadWrite, "Which ROMs and I/O are seen in place of the RAM under them, and the cassette's lines. Writes always go to the RAM.", CPU_PORT);
    // RAM vectors the KERNAL jumps through.
    let vectors: [(u16, &str, &str, &str); 9] = [
        (0x0314, "CINV", "IRQ vector", "Where the KERNAL's IRQ handler jumps once it has saved the registers: $EA31 normally. Point it at a raster routine, which ends with JMP $EA31 (or $EA81 to skip the keyboard)."),
        (0x0316, "CBINV", "BRK vector", "Where BRK goes: $FE66 normally."),
        (0x0318, "NMINV", "NMI vector", "Where the KERNAL's NMI handler jumps: $FE47 normally. RESTORE and CIA 2 come here."),
        (0x031A, "IOPEN", "OPEN vector", "The KERNAL's OPEN routine: $F34A."),
        (0x031C, "ICLOSE", "CLOSE vector", "The KERNAL's CLOSE routine: $F291."),
        (0x0324, "IBASIN", "CHRIN vector", "The KERNAL's CHRIN routine: $F157."),
        (0x0326, "IBSOUT", "CHROUT vector", "The KERNAL's CHROUT routine: $F1CA."),
        (0x032A, "IGETIN", "GETIN vector", "The KERNAL's GETIN routine: $F13E."),
        (0x0330, "ILOAD", "LOAD vector", "The KERNAL's LOAD routine: $F4A5."),
    ];
    for (address, name, title, about) in vectors {
        b.add(address, name, title, Kernal, Vector, about, &[]);
    }
    for (address, name, title, about) in [
        (0xFFFA, "NMI", "Hardware NMI vector", "Where the processor goes on an NMI, when the KERNAL is switched out (a program with $01 = $35 puts its own handler's address here)."),
        (0xFFFC, "RESET", "Hardware reset vector", "Where the processor starts: $FCE2."),
        (0xFFFE, "IRQ", "Hardware IRQ and BRK vector", "Where the processor goes on an IRQ or BRK, when the KERNAL is switched out ($01 = $35): the routine must save and restore the registers itself and end with RTI."),
    ] {
        b.add(address, name, title, Kernal, Vector, about, &[]);
    }
    // The KERNAL's jump table, and some routines inside it that programs call.
    let calls: &[(u16, &str, &str, &str)] = &[
        (0xFF81, "CINIT", "Set up the screen", "Sets up the VIC-II and the screen editor, and clears the screen."),
        (0xFF84, "IOINIT", "Set up I/O", "Sets up the CIAs, the SID's volume and the processor port."),
        (0xFF87, "RAMTAS", "Test RAM", "Clears the zero page and pages 2 and 3, finds the top of RAM and sets the cassette buffer."),
        (0xFF8A, "RESTOR", "Default vectors", "Puts the KERNAL's own addresses back in the vectors at $0314-$0333."),
        (0xFF8D, "VECTOR", "Read or set the vectors", "Carry set: copies the vectors to the table X (low) and Y (high) point to. Clear: copies them from it."),
        (0xFF90, "SETMSG", "KERNAL messages", "A: bit 7 for the KERNAL's own messages (SEARCHING), bit 6 for its error messages."),
        (0xFF93, "SECOND", "Secondary address after LISTEN", "Sends A as the secondary address after LISTEN."),
        (0xFF96, "TKSA", "Secondary address after TALK", "Sends A as the secondary address after TALK."),
        (0xFF99, "MEMTOP", "Top of memory", "Carry set: reads the top of BASIC's memory into X and Y. Clear: sets it from them."),
        (0xFF9C, "MEMBOT", "Bottom of memory", "Carry set: reads the bottom of BASIC's memory into X and Y. Clear: sets it."),
        (0xFF9F, "SCNKEY", "Scan the keyboard", "Reads the keyboard into the key buffer (the IRQ handler does it normally)."),
        (0xFFA2, "SETTMO", "Serial timeout", "Sets the IEEE timeout flag from A."),
        (0xFFA5, "ACPTR", "Read a byte from the serial bus", "Reads a byte from the serial bus into A."),
        (0xFFA8, "CIOUT", "Write a byte to the serial bus", "Sends A on the serial bus."),
        (0xFFAB, "UNTLK", "UNTALK", "Tells the serial bus's devices to stop talking."),
        (0xFFAE, "UNLSN", "UNLISTEN", "Tells the serial bus's devices to stop listening."),
        (0xFFB1, "LISTEN", "LISTEN", "Tells device A on the serial bus to listen."),
        (0xFFB4, "TALK", "TALK", "Tells device A on the serial bus to talk."),
        (0xFFB7, "READST", "Read the status", "The status of the last input or output, in A (bit 6: end of file)."),
        (0xFFBA, "SETLFS", "Set the file numbers", "A the logical file, X the device (8 the first disk drive), Y the secondary address (1 to load to the file's own address)."),
        (0xFFBD, "SETNAM", "Set the file name", "A the name's length, X and Y its address (low, high)."),
        (0xFFC0, "OPEN", "Open a file", "Opens the file SETLFS and SETNAM set up. Carry set on an error, with the error in A."),
        (0xFFC3, "CLOSE", "Close a file", "Closes logical file A."),
        (0xFFC6, "CHKIN", "Input from a file", "Makes logical file X the input for CHRIN."),
        (0xFFC9, "CHKOUT", "Output to a file", "Makes logical file X the output for CHROUT."),
        (0xFFCC, "CLRCHN", "Back to the keyboard and screen", "Puts input and output back to the keyboard and the screen."),
        (0xFFCF, "CHRIN", "Read a character", "Reads a character from the input into A (from the keyboard, a line at a time)."),
        (0xFFD2, "CHROUT", "Write a character", "Writes the PETSCII character in A to the output, normally the screen ($0D a new line, $93 clear the screen)."),
        (0xFFD5, "LOAD", "Load", "Loads the file SETLFS and SETNAM set up: A 0 to load (1 to verify), X and Y where to (with secondary address 0). Ends with X and Y the end."),
        (0xFFD8, "SAVE", "Save", "Saves memory from the address in the zero page that A points to, up to X and Y, to the file set up."),
        (0xFFDB, "SETTIM", "Set the clock", "Sets the jiffy clock (60ths of a second) from A, X and Y (high to low)."),
        (0xFFDE, "RDTIM", "Read the clock", "Reads the jiffy clock into A, X and Y (high to low)."),
        (0xFFE1, "STOP", "RUN/STOP pressed?", "Zero set if RUN/STOP is down."),
        (0xFFE4, "GETIN", "Get a character", "A key from the keyboard buffer in A, or 0 if none has been pressed."),
        (0xFFE7, "CLALL", "Close every file", "Closes every file and puts input and output back to the keyboard and screen."),
        (0xFFEA, "UDTIM", "Count the clock", "Adds one to the jiffy clock and reads RUN/STOP (the IRQ handler calls it)."),
        (0xFFED, "SCREEN", "Screen size", "The screen's size: X columns (40), Y rows (25)."),
        (0xFFF0, "PLOT", "Cursor position", "Carry set: reads the cursor's row into X and column into Y. Clear: moves the cursor there."),
        (0xFFF3, "IOBASE", "Where I/O is", "X and Y: the CIAs' address ($DC00)."),
        (0xEA31, "IRQ_DEFAULT", "The KERNAL's IRQ handler", "The KERNAL's own IRQ routine: clock, cursor, keyboard. A raster routine ends with JMP $EA31 to do all that, once a frame."),
        (0xEA81, "IRQ_END", "End of an IRQ", "Restores Y, X and A from the stack and returns from the interrupt: JMP $EA81 to end a raster routine without the keyboard and clock."),
        (0xE544, "CLEAR_SCREEN", "Clear the screen", "Clears the screen and homes the cursor (the same as printing $93)."),
        (0xFCE2, "RESET_ROUTINE", "Start again", "The KERNAL's reset routine, as at switching on."),
    ];
    for (address, name, title, about) in calls {
        b.add(*address, name, title, Kernal, Call, about, &[]);
    }
    b.0.sort_by_key(|r| r.address);
    b.0
}

/// Every register, in address order.
pub fn registers() -> &'static [Register] {
    static ALL: OnceLock<Vec<Register>> = OnceLock::new();
    ALL.get_or_init(build)
}

/// The VIC-II's, SID's and CIAs' registers are repeated through their pages: the VIC-II's every 64 bytes to
/// $D3FF, the SID's every 32 to $D7FF, and each CIA's every 16 through its page. The register an address
/// reaches, as its first copy.
pub fn mirror(address: u16) -> u16 {
    match address {
        0xD000..=0xD3FF => 0xD000 + (address & 0x3F),
        0xD400..=0xD7FF => 0xD400 + (address & 0x1F),
        0xDC00..=0xDCFF => 0xDC00 + (address & 0x0F),
        0xDD00..=0xDDFF => 0xDD00 + (address & 0x0F),
        _ => address,
    }
}

/// The register, vector or routine at an address (a copy of a register's own included).
pub fn at(address: u16) -> Option<&'static Register> {
    let address = mirror(address);
    registers().iter().find(|r| address >= r.address && address < r.address + r.span())
}

/// Whether an address is the chips' (rather than the KERNAL's or the processor port's).
pub fn is_chip(address: u16) -> bool {
    matches!(address, 0xD000..=0xD7FF | 0xDC00..=0xDDFF)
}

/// A register by its usual name, in any case.
pub fn named(name: &str) -> Option<&'static Register> {
    registers().iter().find(|r| r.name.eq_ignore_ascii_case(name))
}

/// A colour as a swatch for hover text: a block, its RGB in Pepto's palette, and its name.
pub fn colour_words(n: u8) -> String {
    let n = (n & 15) as usize;
    format!("\u{25A0}#{:06X} {n} {}", PEPTO[n], COLOUR_NAMES[n])
}

// ---------- Values explained ----------

/// What the screen shows for the mode bits of $D011 and $D016.
pub fn screen_mode(d011: u8, d016: u8) -> &'static str {
    let ecm = d011 & 0x40 != 0;
    let bmm = d011 & 0x20 != 0;
    let mcm = d016 & 0x10 != 0;
    match (ecm, bmm, mcm) {
        (false, false, false) => "standard text",
        (false, false, true) => "multicolour text",
        (false, true, false) => "hires bitmap",
        (false, true, true) => "multicolour bitmap",
        (true, false, false) => "extended background colour text",
        _ => "a mode that isn't one (the screen shows black)",
    }
}

/// Where the VIC-II finds things, from $D018 and the bank $DD00 chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VicMemory {
    pub bank: u16,
    pub screen: u16,
    pub charset: u16,
    pub bitmap: u16,
    pub sprite_pointers: u16,
}

/// The VIC's 16K bank from what's in CIA 2's port A ($DD00): its bits 0 and 1, inverted.
pub fn bank_of(dd00: u8) -> u16 {
    (3 - (dd00 & 3) as u16) * 0x4000
}

pub fn vic_memory(d018: u8, dd00: u8) -> VicMemory {
    let bank = bank_of(dd00);
    let screen = bank + ((d018 >> 4) as u16) * 0x400;
    VicMemory { bank, screen, charset: bank + (((d018 >> 1) & 7) as u16) * 0x800, bitmap: bank + (((d018 >> 3) & 1) as u16) * 0x2000, sprite_pointers: screen + 0x3F8 }
}

/// What the VIC sees at an address: in banks 0 and 2, $1000-$1FFF is the character ROM, not RAM.
pub fn sees_rom(address: u16) -> Option<&'static str> {
    match address & 0x7FFF {
        0x1000..=0x17FF => Some("the character ROM's capitals and graphics"),
        0x1800..=0x1FFF => Some("the character ROM's lower case"),
        _ => None,
    }
}

impl VicMemory {
    /// In words, as hover says it.
    pub fn words(&self, bitmap: bool) -> String {
        let rom = |a: u16| sees_rom(a).map(|r| format!(" ({r})")).unwrap_or_default();
        let mut out = format!("screen ${:04X}-${:04X}{}, ", self.screen, self.screen + 0x3E7, rom(self.screen));
        if bitmap {
            out.push_str(&format!("bitmap ${:04X}-${:04X}", self.bitmap, self.bitmap + 0x1F3F));
        } else {
            out.push_str(&format!("characters ${:04X}-${:04X}{}", self.charset, self.charset + 0x7FF, rom(self.charset)));
        }
        out.push_str(&format!(", sprite pointers ${:04X}-${:04X}", self.sprite_pointers, self.sprite_pointers + 7));
        out
    }

    /// Where sprite data with a pointer value is.
    pub fn sprite(&self, pointer: u8) -> u16 {
        self.bank + pointer as u16 * 64
    }
}

/// What the processor sees at $A000, $D000 and $E000 for the low three bits of $01 (with no cartridge).
pub fn memory_setup(port: u8) -> [&'static str; 3] {
    let loram = port & 1 != 0;
    let hiram = port & 2 != 0;
    let charen = port & 4 != 0;
    let basic = if loram && hiram { "BASIC ROM" } else { "RAM" };
    let kernal = if hiram { "KERNAL ROM" } else { "RAM" };
    let io = if !loram && !hiram {
        "RAM"
    } else if charen {
        "I/O"
    } else {
        "character ROM"
    };
    [basic, io, kernal]
}

pub fn memory_words(port: u8) -> String {
    let [basic, io, kernal] = memory_setup(port);
    let usual = match port & 7 {
        7 => " (as at switching on)",
        5 => " (the usual for demos: all RAM but the chips)",
        4 | 0 => " (all RAM)",
        _ => "",
    };
    crate::i18n::tf!("$A000-$BFFF {basic}, $D000-$DFFF {io}, $E000-$FFFF {kernal}{usual}")
}

/// The keyboard's matrix: for each column (CIA 1's port A bit, chosen by writing it 0), the key on each row
/// (port B bit 0 to 7).
pub const KEYBOARD: [[&str; 8]; 8] = [
    ["INST/DEL", "RETURN", "CRSR right", "F7", "F1", "F3", "F5", "CRSR down"],
    ["3", "W", "A", "4", "Z", "S", "E", "left SHIFT"],
    ["5", "R", "D", "6", "C", "F", "T", "X"],
    ["7", "Y", "G", "8", "B", "H", "U", "V"],
    ["9", "I", "J", "0", "M", "K", "O", "N"],
    ["+", "P", "L", "-", ".", ":", "@", ","],
    ["\u{A3}", "*", ";", "CLR/HOME", "right SHIFT", "=", "\u{2191}", "/"],
    ["1", "\u{2190}", "CTRL", "2", "SPACE", "C=", "Q", "RUN/STOP"],
];

/// The keys a value written to $DC00 chooses to read.
pub fn keyboard_columns(value: u8) -> String {
    let chosen: Vec<usize> = (0..8).filter(|c| value & (1 << c) == 0).collect();
    match chosen.len() {
        0 => crate::i18n::tr("no keyboard column chosen").into(),
        8 => crate::i18n::tr("every keyboard column: $DC01 then shows whether any key is down").into(),
        _ => chosen.iter().map(|c| format!("column {c} ({})", KEYBOARD[*c].join(" "))).collect::<Vec<_>>().join("; "),
    }
}

/// A SID frequency register's value in Hz, on a PAL or an NTSC machine.
pub fn sid_hz(value: u16, pal: bool) -> f64 {
    value as f64 * if pal { PAL_CLOCK } else { NTSC_CLOCK } / 16_777_216.0
}

/// The value for a frequency.
pub fn sid_value(hz: f64, pal: bool) -> u16 {
    (hz * 16_777_216.0 / if pal { PAL_CLOCK } else { NTSC_CLOCK }).round().clamp(0.0, 65535.0) as u16
}

/// The note nearest a frequency, and how many cents it's off: A4 is 440 Hz.
pub fn note(hz: f64) -> Option<(String, f64)> {
    if hz < 8.0 {
        return None;
    }
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
    let nearest = midi.round();
    let n = nearest as i64;
    Some((format!("{}{}", NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1), (midi - nearest) * 100.0))
}

/// The attack's time for 0-15, in milliseconds.
pub const ATTACK_MS: [u32; 16] = [2, 8, 16, 24, 38, 56, 68, 80, 100, 250, 500, 800, 1000, 3000, 5000, 8000];
/// The decay's and the release's, three times as long.
pub const DECAY_MS: [u32; 16] = [6, 24, 48, 72, 114, 168, 204, 240, 300, 750, 1500, 2400, 3000, 9000, 15000, 24000];

fn time(ms: u32) -> String {
    if ms >= 1000 { format!("{} s", ms as f64 / 1000.0) } else { format!("{ms} ms") }
}

/// A frequency as "440.0 Hz, A4 +0 cents".
fn pitch_words(value: u16) -> String {
    let words = |pal: bool| {
        let hz = sid_hz(value, pal);
        let note = note(hz).map(|(n, c)| format!(", {n} {c:+.0} cents")).unwrap_or_default();
        format!("{hz:.1} Hz{note}")
    };
    crate::i18n::tf!("PAL {}; NTSC {}", words(true), words(false))
}

/// What a value written to a register means, in words; empty when it says nothing more than its bits.
pub fn explain(register: &Register, value: u8) -> String {
    let a = register.address;
    match a {
        0xD000..=0xD00F if a.is_multiple_of(2) => crate::i18n::tf!("sprite {} at X {value} (+256 if its bit in $D010 is set)", (a - 0xD000) / 2),
        0xD000..=0xD00F => crate::i18n::tf!("sprite {} at line {value}", (a - 0xD001) / 2),
        0xD011 => {
            let rows = if value & 8 != 0 { 25 } else { 24 };
            let on = if value & 0x10 != 0 { "display on" } else { "display off (blank)" };
            crate::i18n::tf!("{}, {on}, {rows} rows, scrolled down {}, raster bit 8 = {}", screen_mode(value, 0), value & 7, value >> 7)
        }
        0xD012 => crate::i18n::tf!("raster line {value} (or {} with $D011 bit 7)", value as u16 + 256),
        0xD016 => {
            let columns = if value & 8 != 0 { 40 } else { 38 };
            crate::i18n::tf!("{}, {columns} columns, scrolled right {}", if value & 0x10 != 0 { "multicolour" } else { "not multicolour" }, value & 7)
        }
        0xD018 => {
            let memory = vic_memory(value, 3);
            crate::i18n::tf!("in bank 0 ($0000, as at switching on): {}; or a bitmap at ${:04X}. Screen offset ${:04X}, characters offset ${:04X}", memory.words(false), memory.bitmap, (value >> 4) as u16 * 0x400, ((value >> 1) & 7) as u16 * 0x800)
        }
        0xD019 => {
            let names: Vec<&str> = INTERRUPTS.iter().rev().filter(|f| f.low < 4 && value & (1 << f.low) != 0).map(|f| f.name).collect();
            if names.is_empty() { "nothing acknowledged".into() } else { format!("acknowledges {}", names.join(", ")) }
        }
        0xD01A => {
            let names: Vec<&str> = INTERRUPT_ENABLE.iter().rev().filter(|f| value & (1 << f.low) != 0).map(|f| f.name).collect();
            if names.is_empty() { crate::i18n::tr("no VIC interrupts").into() } else { format!("interrupts on: {}", names.join(", ")) }
        }
        0xD010 | 0xD015 | 0xD017 | 0xD01B..=0xD01F => {
            let on: Vec<String> = (0..8).filter(|s| value & (1 << s) != 0).map(|s| s.to_string()).collect();
            if on.is_empty() { "no sprites".into() } else { format!("sprites {}", on.join(", ")) }
        }
        0xD020..=0xD02E => colour_words(value),
        _ if register.chip == Chip::Sid => explain_sid(register, value),
        0xDC00 => keyboard_columns(value),
        0xDD00 => {
            let bank = bank_of(value);
            crate::i18n::tf!("VIC bank {} (${bank:04X}-${:04X})", bank / 0x4000, bank + 0x3FFF)
        }
        0xDC0D | 0xDD0D => {
            let names: Vec<&str> = ICR.iter().rev().filter(|f| f.low < 5 && value & (1 << f.low) != 0).map(|f| f.name).collect();
            let what = if names.is_empty() { "none".to_string() } else { names.join(", ") };
            let line = if a == 0xDC0D { "IRQ" } else { "NMI" };
            if value & 0x80 != 0 { crate::i18n::tf!("turns on the {line} for {what}") } else { crate::i18n::tf!("turns off the {line} for {what}") }
        }
        0xDC0E | 0xDD0E | 0xDC0F | 0xDD0F => {
            let timer = if a & 1 == 0 { "A" } else { "B" };
            let running = if value & 1 != 0 { "running" } else { "stopped" };
            let once = if value & 8 != 0 { ", once" } else { ", over and over" };
            let load = if value & 0x10 != 0 { ", loaded now" } else { "" };
            let counts = if a & 1 == 0 {
                if value & 0x20 != 0 { ", counting CNT" } else { ", counting clock cycles" }
            } else {
                match (value >> 5) & 3 {
                    0 => ", counting clock cycles",
                    1 => ", counting CNT",
                    2 => ", counting timer A running out",
                    _ => ", counting timer A running out while CNT is high",
                }
            };
            format!("timer {timer} {running}{once}{load}{counts}")
        }
        0x0001 => memory_words(value),
        _ => String::new(),
    }
}

fn explain_sid(register: &Register, value: u8) -> String {
    let a = register.address;
    match a {
        0xD400..=0xD414 => match (a - 0xD400) % 7 {
            1 => crate::i18n::tf!("frequency ${value:02X}xx: about {}", pitch_words((value as u16) << 8)),
            3 => crate::i18n::tf!("pulse width ${:X}xx of $FFF: about {:.0}%", value & 15, ((value & 15) as f64 * 256.0) / 40.95),
            4 => {
                let waves: Vec<&str> = VOICE_CONTROL[..4].iter().filter(|f| value & (1 << f.low) != 0).map(|f| f.name).collect();
                let extras: Vec<&str> = VOICE_CONTROL[4..7].iter().filter(|f| value & (1 << f.low) != 0).map(|f| f.name).collect();
                let wave = if waves.is_empty() { "no waveform".to_string() } else { waves.join("+") };
                let extras = if extras.is_empty() { String::new() } else { format!(", {}", extras.join(", ")) };
                format!("{wave}{extras}, gate {}", if value & 1 != 0 { "on (attack)" } else { "off (release)" })
            }
            5 => format!("attack {}, decay {}", time(ATTACK_MS[(value >> 4) as usize]), time(DECAY_MS[(value & 15) as usize])),
            6 => crate::i18n::tf!("sustain at {}/15, release {}", value >> 4, time(DECAY_MS[(value & 15) as usize])),
            _ => String::new(),
        },
        0xD416 => crate::i18n::tf!("cutoff ${:03X} (with the low bits 0): about {:.0} Hz on an 8580; a 6581's varies from chip to chip", (value as u16) << 3, cutoff_8580((value as u16) << 3)),
        0xD417 => {
            let routed: Vec<&str> = RESONANCE[1..].iter().filter(|f| value & (1 << f.low) != 0).map(|f| f.name).collect();
            format!("resonance {}, filtering {}", value >> 4, if routed.is_empty() { "nothing".to_string() } else { routed.join(", ") })
        }
        0xD418 => {
            let modes: Vec<&str> = MODE_VOLUME[1..4].iter().filter(|f| value & (1 << f.low) != 0).map(|f| f.name).collect();
            let off = if value & 0x80 != 0 { ", voice 3 off" } else { "" };
            format!("volume {}, filter {}{off}", value & 15, if modes.is_empty() { "off".to_string() } else { modes.join("+") })
        }
        _ => String::new(),
    }
}

/// An 8580's filter cutoff, roughly: from about 30 Hz at 0 to about 12 kHz at $7FF, in a straight line.
pub fn cutoff_8580(cutoff: u16) -> f64 {
    30.0 + (cutoff.min(0x7FF) as f64) * (12_000.0 - 30.0) / 2047.0
}

/// A 16-bit SID frequency or timer from two writes, as hover says it.
pub fn explain_frequency(value: u16) -> String {
    format!("frequency ${value:04X}: {}", pitch_words(value))
}

/// A CIA timer's count, in time and rate, on PAL and NTSC.
pub fn explain_timer(count: u16) -> String {
    let cycles = count as f64 + 1.0;
    let pal = PAL_CLOCK / cycles;
    let ntsc = NTSC_CLOCK / cycles;
    crate::i18n::tf!("${count:04X} ({count}): runs out {pal:.2} times a second on PAL, {ntsc:.2} on NTSC")
}

/// Each bit of a value, by the register's fields: "4 DEN = 1".
pub fn field_lines(register: &Register, value: u8) -> Vec<String> {
    register.fields.iter().filter(|f| !(f.high == 7 && f.low == 0 && register.fields.len() > 1)).map(|f| format!("  {:>3} {:<8} = {}", f.bits(), f.name, f.value(value))).collect()
}

/// A register described for hover: its heading, what it is, and its bits.
pub fn describe(register: &Register) -> String {
    let mut out = format!("{}\n{}", register.heading(), register.about);
    for f in register.fields {
        out.push_str(&format!("\n  {:>3} {:<8} {}", f.bits(), f.name, f.about));
    }
    out
}

/// A register and a value written to it, for hover.
pub fn describe_value(register: &Register, value: u8) -> String {
    let mut out = format!("{}\n${value:02X} (%{value:08b}) written", register.heading());
    let said = explain(register, value);
    if !said.is_empty() {
        out.push_str(&format!(": {said}"));
    }
    if register.fields.len() > 1 && !register.is_colour() {
        for line in field_lines(register, value) {
            out.push('\n');
            out.push_str(&line);
        }
    }
    out
}

/// The registers of a chip as assembler lines, NAME = $D020, to paste in.
pub fn equates(chip: Option<Chip>) -> String {
    registers().iter().filter(|r| chip.is_none_or(|c| r.chip == c)).map(|r| format!("{:<12}= ${:04X}    ; {}\n", r.name, r.address, r.title)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_register_is_where_vice_has_it() {
        // VICE's VIC-II has 47 registers ($D000-$D02E); its SID 29 ($D400-$D41C); each CIA 16.
        assert_eq!(registers().iter().filter(|r| r.chip == Chip::Vic).count(), 47);
        assert_eq!(registers().iter().filter(|r| r.chip == Chip::Sid).count(), 29);
        assert_eq!(registers().iter().filter(|r| r.chip == Chip::Cia1).count(), 16);
        assert_eq!(registers().iter().filter(|r| r.chip == Chip::Cia2).count(), 16);
        for address in 0xD000..=0xD02E {
            assert!(at(address).is_some_and(|r| r.address == address), "${address:04X}");
        }
        for address in 0xD400..=0xD41C {
            assert!(at(address).is_some_and(|r| r.address == address), "${address:04X}");
        }
        assert!(at(0xD02F).is_none(), "unused on the C64");
        assert_eq!(at(0xD060).unwrap().name, "EXTCOL", "the VIC's registers repeat every 64 bytes");
        assert_eq!(at(0xD438).unwrap().name, "SIGVOL", "the SID's every 32");
        assert_eq!(at(0xDC1D).unwrap().name, "CIA1ICR");
        assert_eq!(at(0x0315).unwrap().name, "CINV", "a vector's high byte");
        assert_eq!(named("chrout").unwrap().address, 0xFFD2);
        assert_eq!(named("vmcsb").unwrap().address, 0xD018);
        // No two overlap.
        for (i, a) in registers().iter().enumerate() {
            for b in &registers()[i + 1..] {
                assert!(a.address + a.span() <= b.address, "{} and {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn the_palettes_are_vice_s() {
        assert_eq!(PEPTO.map(crate::screen::rgb), crate::retro_screens::C64);
        assert_eq!(colour_words(2), "\u{25A0}#68372B 2 red");
        assert_eq!(rgb(COLODORE[14]), [0x68, 0x64, 0xFF]);
    }

    #[test]
    fn the_vic_s_memory_is_worked_out() {
        // $15 is how the C64 starts: screen $0400, characters $1000 (the ROM's capitals).
        let start = vic_memory(0x15, 0x97);
        assert_eq!((start.bank, start.screen, start.charset, start.sprite_pointers), (0, 0x0400, 0x1000, 0x07F8));
        assert!(start.words(false).contains("characters $1000-$17FF (the character ROM's capitals and graphics)"));
        // $DD00 = %10 is bank 1; $18 is a screen at $0400 and a bitmap at $2000 in it.
        let bitmap = vic_memory(0x18, 0x02);
        assert_eq!((bitmap.bank, bitmap.screen, bitmap.bitmap), (0x4000, 0x4400, 0x6000));
        assert_eq!(vic_memory(0x18, 0x00).screen, 0xC400);
        assert_eq!(start.sprite(0x0D), 0x0340);
        assert_eq!(sees_rom(0x9800), Some("the character ROM's lower case"));
        assert_eq!(sees_rom(0x5000), None);
    }

    #[test]
    fn values_are_explained() {
        let d011 = at(0xD011).unwrap();
        assert_eq!(explain(d011, 0x1B), "standard text, display on, 25 rows, scrolled down 3, raster bit 8 = 0");
        assert!(explain(d011, 0x3B).starts_with("hires bitmap"));
        assert!(explain(at(0xD016).unwrap(), 0xD8).starts_with("multicolour, 40 columns"));
        assert_eq!(explain(at(0xD418).unwrap(), 0x1F), "volume 15, filter LP");
        assert_eq!(explain(at(0xD405).unwrap(), 0x09), "attack 2 ms, decay 750 ms");
        assert_eq!(explain(at(0xD406).unwrap(), 0xF0), "sustain at 15/15, release 6 ms");
        assert_eq!(explain(at(0xD404).unwrap(), 0x41), "PULSE, gate on (attack)");
        assert_eq!(explain(at(0xD40B).unwrap(), 0x21), "SAW, gate on (attack)");
        assert_eq!(explain(at(0x0001).unwrap(), 0x35), "$A000-$BFFF RAM, $D000-$DFFF I/O, $E000-$FFFF RAM (the usual for demos: all RAM but the chips)");
        assert_eq!(explain(at(0x0001).unwrap(), 0x37), "$A000-$BFFF BASIC ROM, $D000-$DFFF I/O, $E000-$FFFF KERNAL ROM (as at switching on)");
        assert_eq!(memory_setup(0x33), ["BASIC ROM", "character ROM", "KERNAL ROM"]);
        assert_eq!(memory_setup(0x34), ["RAM", "RAM", "RAM"]);
        assert_eq!(explain(at(0xDD00).unwrap(), 0x97), "VIC bank 0 ($0000-$3FFF)");
        assert_eq!(explain(at(0xDC0D).unwrap(), 0x7F), "turns off the IRQ for TA, TB, ALARM, SP, FLAG");
        assert_eq!(explain(at(0xD01A).unwrap(), 0x01), "interrupts on: ERST");
        assert!(explain(at(0xD020).unwrap(), 0).contains("black"));
        assert_eq!(explain(at(0xDC00).unwrap(), 0x7F), "column 7 (1 \u{2190} CTRL 2 SPACE C= Q RUN/STOP)");
        assert!(explain(at(0xDC0E).unwrap(), 0x11).starts_with("timer A running, over and over, loaded now"));
    }

    #[test]
    fn the_cias_bits_are_vice_s() {
        // VICE's cia.h: CIA_CR_START 0x01, PBON 0x02, OUTMODE 0x04, RUNMODE 0x08, LOAD 0x10, CRA INMODE 0x20,
        // SPMODE 0x40, TODIN 0x80; CRB INMODE 0x60, ALARM 0x80; ICR TA 1, TB 2, TOD 4, SDR 8, FLAG 16, SET 0x80.
        let mask = |register: u16, name: &str| {
            let f = at(register).unwrap().fields.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("{name}"));
            (f.low..=f.high).map(|b| 1u8 << b).sum::<u8>()
        };
        for (name, bits) in [("START", 0x01), ("PBON", 0x02), ("OUTMODE", 0x04), ("RUNMODE", 0x08), ("LOAD", 0x10), ("INMODE", 0x20), ("SPMODE", 0x40), ("TODIN", 0x80)] {
            assert_eq!(mask(0xDC0E, name), bits, "CRA {name}");
        }
        assert_eq!((mask(0xDD0F, "INMODE"), mask(0xDD0F, "ALARM")), (0x60, 0x80));
        for (name, bits) in [("TA", 1), ("TB", 2), ("ALARM", 4), ("SP", 8), ("FLAG", 16), ("SET", 0x80)] {
            assert_eq!(mask(0xDC0D, name), bits, "ICR {name}");
        }
        // vicii-mem.c's: $D011 bit 7 the raster's ninth bit, $D016 bit 4 multicolour, $D018's two fields.
        assert_eq!((mask(0xD011, "RST8"), mask(0xD011, "BMM"), mask(0xD011, "ECM"), mask(0xD011, "DEN"), mask(0xD011, "RSEL"), mask(0xD011, "YSCROLL")), (0x80, 0x20, 0x40, 0x10, 0x08, 0x07));
        assert_eq!((mask(0xD016, "MCM"), mask(0xD016, "CSEL"), mask(0xD016, "XSCROLL")), (0x10, 0x08, 0x07));
        assert_eq!((mask(0xD018, "VM"), mask(0xD018, "CB")), (0xF0, 0x0E));
    }

    /// With LIMONITER_C64_KERNAL naming the KERNAL ROM (901227-03): every routine of the jump table is a JMP there,
    /// the hardware vectors and the RAM vectors' defaults are what the guide says, and the routines inside it are
    /// what they're said to be.
    #[test]
    fn the_kernal_is_as_the_guide_says() {
        let Some(rom) = std::env::var_os("LIMONITER_C64_KERNAL").and_then(|p| std::fs::read(p).ok()) else {
            return;
        };
        assert_eq!(rom.len(), 8192);
        let byte = |a: u16| rom[(a - 0xE000) as usize];
        let word = |a: u16| u16::from_le_bytes([byte(a), byte(a + 1)]);
        for r in registers().iter().filter(|r| r.access == Access::Call && r.address >= 0xFF81) {
            assert!(matches!(byte(r.address), 0x4C | 0x6C), "{} at ${:04X}", r.name, r.address);
        }
        assert_eq!((word(0xFFFA), word(0xFFFC), word(0xFFFE)), (0xFE43, 0xFCE2, 0xFF48));
        // RESTOR's table at $FD30 holds the RAM vectors' defaults, $0314 on.
        for r in registers().iter().filter(|r| r.access == Access::Vector && r.address < 0x0400) {
            let default = word(0xFD30 + (r.address - 0x0314));
            assert!(r.about.contains(&format!("${default:04X}")), "{}: ${default:04X} in {}", r.name, r.about);
        }
        assert_eq!(rom[(0xEA81 - 0xE000) as usize..][..6], [0x68, 0xA8, 0x68, 0xAA, 0x68, 0x40], "PLA TAY PLA TAX PLA RTI");
        assert_eq!(rom[(0xFCE2 - 0xE000) as usize..][..4], [0xA2, 0xFF, 0x78, 0x9A], "LDX #$FF, SEI, TXS");
        assert_eq!(byte(0xFFD2), 0x6C, "CHROUT goes through its vector");
        assert_eq!(word(0xFFD3), 0x0326);
    }

    #[test]
    fn sid_pitches_and_notes() {
        // A4: 440 Hz is $1D45 on PAL, as the usual note tables have it.
        assert_eq!(sid_value(440.0, true), 0x1D45);
        assert_eq!(sid_value(440.0, false), 0x1C32);
        let (name, cents) = note(sid_hz(0x1D45, true)).unwrap();
        assert_eq!(name, "A4");
        assert!(cents.abs() < 2.0);
        assert_eq!(note(261.63).unwrap().0, "C4");
        assert!(explain_frequency(0x1D45).contains("PAL 440.0 Hz, A4"));
        // The KERNAL's 1/60 second IRQ on PAL: $4025.
        assert!(explain_timer(0x4025).contains("60.0"));
    }
}
