//! The Amiga's custom chips and CIAs, for demo coders: every register at $DFF000-$DFF1FE and in the two CIAs, with
//! its name, whether it's read, written or a strobe, which chipsets have it (the original chips, ECS or AGA) and
//! what its bits mean; how to read a value written to one; and the copper's instructions, MOVE, WAIT and SKIP,
//! decoded from their two words.
//!
//! Names, addresses and bit positions are the hardware's; the words describing them are Limoniter's own.

use std::sync::OnceLock;

/// Where the custom chips' registers start.
pub const CUSTOM: u32 = 0xDF_F000;
/// The CIAs: CIA-A on the odd bytes from $BFE001, CIA-B on the even bytes from $BFD000, a register every 256 bytes.
pub const CIAA: u32 = 0xBF_E001;
pub const CIAB: u32 = 0xBF_D000;
/// The PAL colour clock, which audio periods and serial rates count in (NTSC's is 3579545).
pub const PAL_CLOCK: u32 = 3_546_895;

/// The chipsets, each with everything the one before had.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Chipset {
    /// The original chips: Agnus, Denise and Paula.
    Ocs,
    /// Enhanced: Fat Agnus and Super Denise (A500+, A600, A3000).
    Ecs,
    /// Advanced Graphics: Alice and Lisa (A1200, A4000, CD32).
    Aga,
}

impl Chipset {
    pub const ALL: [Chipset; 3] = [Chipset::Ocs, Chipset::Ecs, Chipset::Aga];

    pub fn name(self) -> &'static str {
        match self {
            Chipset::Ocs => "OCS",
            Chipset::Ecs => "ECS",
            Chipset::Aga => "AGA",
        }
    }

    /// The chipsets that have something that came in with this one.
    pub fn onwards(self) -> &'static str {
        match self {
            Chipset::Ocs => "OCS, ECS, AGA",
            Chipset::Ecs => "ECS, AGA",
            Chipset::Aga => "AGA only",
        }
    }
}

/// How the processor (or copper) gets at a register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    /// Writing it does something; what's written doesn't matter.
    Strobe,
    /// Only the chips' own DMA uses it.
    Dma,
    /// A CIA register, read and written.
    ReadWrite,
}

impl Access {
    pub fn name(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::Strobe => "strobe",
            Access::Dma => "DMA only",
            Access::ReadWrite => "read/write",
        }
    }

    pub fn writable(self) -> bool {
        matches!(self, Access::Write | Access::Strobe | Access::ReadWrite)
    }
}

/// The groups the guide shows registers in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    Bitplanes,
    Colours,
    Sprites,
    Copper,
    Blitter,
    Audio,
    DmaInterrupts,
    Display,
    Beam,
    Disk,
    Ports,
    AgaExtras,
    CiaA,
    CiaB,
}

impl Area {
    pub const ALL: [Area; 14] = [
        Area::Bitplanes,
        Area::Colours,
        Area::Sprites,
        Area::Copper,
        Area::Blitter,
        Area::Audio,
        Area::DmaInterrupts,
        Area::Display,
        Area::Beam,
        Area::Disk,
        Area::Ports,
        Area::AgaExtras,
        Area::CiaA,
        Area::CiaB,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Area::Bitplanes => "Bitplanes",
            Area::Colours => "Colours",
            Area::Sprites => "Sprites and collisions",
            Area::Copper => "Copper",
            Area::Blitter => "Blitter",
            Area::Audio => "Audio",
            Area::DmaInterrupts => "DMA and interrupts",
            Area::Display => "Display window and beam position",
            Area::Beam => "Programmable beam (ECS and AGA)",
            Area::Disk => "Disk",
            Area::Ports => "Joysticks, mice, pots and serial",
            Area::AgaExtras => "AGA extras",
            Area::CiaA => "CIA-A ($BFE001)",
            Area::CiaB => "CIA-B ($BFD000)",
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
    pub since: Chipset,
}

impl Field {
    pub fn bits(&self) -> String {
        if self.high == self.low { format!("{}", self.high) } else { format!("{}-{}", self.high, self.low) }
    }

    pub fn value(&self, word: u16) -> u16 {
        let width = self.high - self.low + 1;
        let mask = if width >= 16 { 0xFFFF } else { (1u16 << width) - 1 };
        (word >> self.low) & mask
    }
}

const fn bit(n: u8, name: &'static str, about: &'static str) -> Field {
    Field { high: n, low: n, name, about, since: Chipset::Ocs }
}

const fn bits(high: u8, low: u8, name: &'static str, about: &'static str) -> Field {
    Field { high, low, name, about, since: Chipset::Ocs }
}

const fn ecs(f: Field) -> Field {
    Field { since: Chipset::Ecs, ..f }
}

const fn aga(f: Field) -> Field {
    Field { since: Chipset::Aga, ..f }
}

/// A register of the custom chips or a CIA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Register {
    pub address: u32,
    pub name: String,
    pub access: Access,
    pub since: Chipset,
    pub area: Area,
    pub about: String,
    pub fields: &'static [Field],
}

impl Register {
    pub fn is_cia(&self) -> bool {
        matches!(self.area, Area::CiaA | Area::CiaB)
    }

    /// Its offset from the custom chips' base, or for a CIA, from that CIA's base.
    pub fn offset(&self) -> u32 {
        match self.area {
            Area::CiaA => self.address - CIAA,
            Area::CiaB => self.address - CIAB,
            _ => self.address - CUSTOM,
        }
    }

    /// The colour register this is (0-31), if it is one.
    pub fn colour(&self) -> Option<u16> {
        let offset = self.offset();
        (!self.is_cia() && (0x180..0x1C0).contains(&offset)).then(|| ((offset - 0x180) / 2) as u16)
    }

    /// The name and where it is, as the guide heads it.
    pub fn heading(&self) -> String {
        if self.is_cia() {
            format!("{}  ${:06X}  {}", self.name, self.address, self.access.name())
        } else {
            format!("{}  ${:06X} (+${:03X})  {}  {}", self.name, self.address, self.offset(), self.access.name(), self.since.onwards())
        }
    }
}

// ---------- The bits ----------

const BPLCON0: &[Field] = &[
    bit(15, "HIRES", "High resolution: 640 pixels across a normal screen"),
    bits(14, 12, "BPU", "How many bitplanes are shown, 0 to 6 (with BPU3, 7 or 8)"),
    bit(11, "HAM", "Hold-and-modify: each pixel can change one colour component of the one to its left"),
    bit(10, "DPF", "Dual playfield: odd planes make one playfield and even planes another, with its own scroll and priority"),
    bit(9, "COLOR", "Colour burst on the composite output (off gives a monochrome picture there)"),
    bit(8, "GAUD", "Genlock audio enable"),
    ecs(bit(7, "UHRES", "Ultra-high resolution, from external video RAM")),
    ecs(bit(6, "SHRES", "Super-high resolution: 1280 pixels across")),
    aga(bit(5, "BYPASS", "Bitplane bits go straight out, past the colour table (for external hardware)")),
    aga(bit(4, "BPU3", "Fourth bit of the bitplane count, for 8 planes")),
    bit(3, "LPEN", "Light pen enable"),
    bit(2, "LACE", "Interlace: alternate frames are drawn on alternate lines"),
    bit(1, "ERSY", "External resync: the beam follows sync from outside (a genlock)"),
    ecs(bit(0, "ECSENA", "Turns on what BPLCON3 adds (border blanking and the like)")),
];

const BPLCON1: &[Field] = &[
    aga(bits(15, 14, "PF2H7-6", "Playfield 2 delay, top two bits (in 35ns steps)")),
    aga(bits(13, 12, "PF2H1-0", "Playfield 2 delay, finest two bits")),
    aga(bits(11, 10, "PF1H7-6", "Playfield 1 delay, top two bits")),
    aga(bits(9, 8, "PF1H1-0", "Playfield 1 delay, finest two bits")),
    bits(7, 4, "PF2H", "Delay of playfield 2 (even planes), 0-15 lowres pixels: smooth scrolling"),
    bits(3, 0, "PF1H", "Delay of playfield 1 (odd planes), 0-15 lowres pixels"),
];

const BPLCON2: &[Field] = &[
    ecs(bits(14, 12, "ZDBPSEL", "Which bitplane makes genlock transparency, when ZDBPEN is on")),
    ecs(bit(11, "ZDBPEN", "A bitplane decides genlock transparency")),
    ecs(bit(10, "ZDCTEN", "Bit 15 of each colour register decides genlock transparency")),
    aga(bit(9, "KILLEHB", "No extra-half-brite: six planes pick from 64 colours instead")),
    aga(bit(8, "RDRAM", "Reading a colour register gives back the colour table")),
    aga(bit(7, "SOGEN", "Sync on green")),
    bit(6, "PF2PRI", "Playfield 2 is in front of playfield 1"),
    bits(5, 3, "PF2P", "Where playfield 2 sits among the sprite pairs: 0 in front of them all, 4 behind them all"),
    bits(2, 0, "PF1P", "Where playfield 1 (or the only playfield) sits among the sprite pairs"),
];

const BPLCON3: &[Field] = &[
    aga(bits(15, 13, "BANK", "Which bank of 32 colours COLOR00-COLOR31 reach (0-7, so 256 colours)")),
    aga(bits(12, 10, "PF2OF", "Colour offset for playfield 2 in dual playfield: 0, 2, 4, 8 (the usual), 16, 32, 64 or 128")),
    aga(bit(9, "LOCT", "Colour writes set the low four bits of red, green and blue, for 24-bit colour")),
    aga(bits(7, 6, "SPRES", "Sprite resolution: 0 as ECS, 1 lowres, 2 hires, 3 super-hires")),
    ecs(bit(5, "BRDRBLNK", "The border is blanked to black instead of showing COLOR00")),
    ecs(bit(4, "BRDNTRAN", "The border isn't transparent to a genlock")),
    ecs(bit(2, "ZDCLKEN", "The ZD pin gives out a 14MHz clock")),
    aga(bit(1, "BRDSPRT", "Sprites show in the border too")),
    ecs(bit(0, "EXTBLKEN", "External blank enable")),
];

const BPLCON4: &[Field] = &[
    bits(15, 8, "BPLAM", "Exclusive-ORed with every bitplane pixel's colour number: swaps palettes without rewriting them"),
    bits(7, 4, "ESPRM", "Colour bank of the even sprites (top four bits of their colour numbers)"),
    bits(3, 0, "OSPRM", "Colour bank of the odd sprites"),
];

const CLXCON2: &[Field] = &[
    bit(7, "ENBP8", "Bitplane 8 takes part in collisions"),
    bit(6, "ENBP7", "Bitplane 7 takes part in collisions"),
    bit(1, "MVBP8", "Value bitplane 8 must have to collide"),
    bit(0, "MVBP7", "Value bitplane 7 must have to collide"),
];

const DMACON: &[Field] = &[
    bit(15, "SET/CLR", "Writing: 1 sets each bit written as 1, 0 clears each bit written as 1; others are left alone"),
    bit(14, "BBUSY", "The blitter is busy (read only)"),
    bit(13, "BZERO", "Everything the last blit made was zero (read only)"),
    bit(10, "BLTPRI", "Blitter priority: the blitter doesn't give the processor a turn (\"blitter nasty\")"),
    bit(9, "DMAEN", "Master switch: no DMA below happens without it"),
    bit(8, "BPLEN", "Bitplane DMA"),
    bit(7, "COPEN", "Copper DMA"),
    bit(6, "BLTEN", "Blitter DMA"),
    bit(5, "SPREN", "Sprite DMA"),
    bit(4, "DSKEN", "Disk DMA"),
    bit(3, "AUD3EN", "Audio channel 3 DMA"),
    bit(2, "AUD2EN", "Audio channel 2 DMA"),
    bit(1, "AUD1EN", "Audio channel 1 DMA"),
    bit(0, "AUD0EN", "Audio channel 0 DMA"),
];

const INTERRUPTS: &[Field] = &[
    bit(15, "SET/CLR", "Writing: 1 sets each bit written as 1, 0 clears each bit written as 1"),
    bit(14, "INTEN", "Master interrupt enable (INTENA only)"),
    bit(13, "EXTER", "Level 6: CIA-B and the external INT6 line"),
    bit(12, "DSKSYNC", "Level 5: the disk sync word was found"),
    bit(11, "RBF", "Level 5: a serial byte has come in"),
    bit(10, "AUD3", "Level 4: audio channel 3 has started its block (ready for the next)"),
    bit(9, "AUD2", "Level 4: audio channel 2 block"),
    bit(8, "AUD1", "Level 4: audio channel 1 block"),
    bit(7, "AUD0", "Level 4: audio channel 0 block"),
    bit(6, "BLIT", "Level 3: the blitter has finished"),
    bit(5, "VERTB", "Level 3: vertical blank has started"),
    bit(4, "COPER", "Level 3: the copper asked (by writing INTREQ)"),
    bit(3, "PORTS", "Level 2: CIA-A and the external INT2 line"),
    bit(2, "SOFT", "Level 1: software interrupt"),
    bit(1, "DSKBLK", "Level 1: a disk DMA block has finished"),
    bit(0, "TBE", "Level 1: the serial port's transmit buffer is empty"),
];

const ADKCON: &[Field] = &[
    bit(15, "SET/CLR", "Writing: 1 sets each bit written as 1, 0 clears each bit written as 1"),
    bits(14, 13, "PRECOMP", "Disk write precompensation: 0 none, 1 140ns, 2 280ns, 3 560ns"),
    bit(12, "MFMPREC", "Precompensation the MFM way (0: the GCR way)"),
    bit(11, "UARTBRK", "Holds the serial output low: a break"),
    bit(10, "WORDSYNC", "Disk reads wait for the word in DSKSYNC before starting"),
    bit(9, "MSBSYNC", "GCR disk reads sync on the top bit"),
    bit(8, "FAST", "Disk data rate: 2µs a bit (MFM, the Amiga's own); 0 is 4µs (GCR)"),
    bit(7, "USE3PN", "Channel 3 modulates nothing (it's the last)"),
    bit(6, "USE2P3", "Channel 2 modulates channel 3's period"),
    bit(5, "USE1P2", "Channel 1 modulates channel 2's period"),
    bit(4, "USE0P1", "Channel 0 modulates channel 1's period"),
    bit(3, "USE3VN", "Channel 3 modulates nothing (it's the last)"),
    bit(2, "USE2V3", "Channel 2 modulates channel 3's volume"),
    bit(1, "USE1V2", "Channel 1 modulates channel 2's volume"),
    bit(0, "USE0V1", "Channel 0 modulates channel 1's volume"),
];

const BLTCON0: &[Field] = &[
    bits(15, 12, "ASH", "Source A is shifted right this many bits (line mode: the first pixel's bit in its word)"),
    bit(11, "USEA", "Source A is read"),
    bit(10, "USEB", "Source B is read"),
    bit(9, "USEC", "Source C is read"),
    bit(8, "USED", "Destination D is written"),
    bits(7, 0, "LF", "Logic function (minterms): for each mix of A, B and C bits, whether D gets a 1"),
];

const BLTCON0L: &[Field] = &[bits(7, 0, "LF", "Logic function (minterms) alone, leaving the rest of BLTCON0")];

const BLTCON1: &[Field] = &[
    bits(15, 12, "BSH", "Source B is shifted right this many bits (line mode: where the texture pattern starts)"),
    ecs(bit(7, "DOFF", "Nothing is written to D (ECS)")),
    bit(6, "SIGN", "Line mode: the starting sign of the error term"),
    bit(4, "EFE/SUD", "Area mode: exclusive fill. Line mode: octant bit SUD"),
    bit(3, "IFE/SUL", "Area mode: inclusive fill. Line mode: octant bit SUL"),
    bit(2, "FCI/AUL", "Area mode: fill carry in, so filling starts on. Line mode: octant bit AUL"),
    bit(1, "DESC/SING", "Area mode: descending, working back from the last word. Line mode: one dot per line, for filling"),
    bit(0, "LINE", "Line-drawing mode"),
];

const BLTSIZE: &[Field] = &[
    bits(15, 6, "H", "Height in lines (0 means 1024)"),
    bits(5, 0, "W", "Width in words (0 means 64). Writing this starts the blit"),
];

const BLTSIZV: &[Field] = &[bits(14, 0, "H", "Height in lines, up to 32768 (write before BLTSIZH)")];
const BLTSIZH: &[Field] = &[bits(10, 0, "W", "Width in words, up to 2048. Writing this starts the blit")];

const FMODE: &[Field] = &[
    bit(15, "SSCAN2", "Sprites are scan-doubled: each line shown twice"),
    bit(14, "BSCAN2", "Bitplanes are scan-doubled, alternate lines using BPL1MOD and BPL2MOD"),
    bit(3, "SPAGEM", "Sprite fetches use page mode (double CAS)"),
    bit(2, "SPR32", "Sprites are fetched 32 bits at a time"),
    bit(1, "BPAGEM", "Bitplane fetches use page mode (double CAS)"),
    bit(0, "BPL32", "Bitplanes are fetched 32 bits at a time"),
];

const BEAMCON0: &[Field] = &[
    bit(14, "HARDDIS", "Turns off the fixed limits on blanking and the display"),
    bit(13, "LPENDIS", "The light pen doesn't latch the beam counters"),
    bit(12, "VARVBEN", "Vertical blank comes from VBSTRT and VBSTOP"),
    bit(11, "LOLDIS", "Every line is the same length (no long and short lines in NTSC)"),
    bit(10, "CSCBEN", "The composite sync pin gives out blanking instead"),
    bit(9, "VARVSYEN", "Vertical sync comes from VSSTRT and VSSTOP"),
    bit(8, "VARHSYEN", "Horizontal sync comes from HSSTRT and HSSTOP"),
    bit(7, "VARBEAMEN", "The beam counters wrap at HTOTAL and VTOTAL: a programmed display mode"),
    bit(6, "DUAL", "Dual display: the UHRES pointers as well as the usual ones"),
    bit(5, "PAL", "PAL timing (0: NTSC)"),
    bit(4, "VARCSYEN", "Composite sync from the programmed horizontal and vertical sync"),
    bit(3, "BLANKEN", "Composite blank goes out on the CSY pin"),
    bit(2, "CSYTRUE", "Composite sync is active high"),
    bit(1, "VSYTRUE", "Vertical sync is active high"),
    bit(0, "HSYTRUE", "Horizontal sync is active high"),
];

const DIWHIGH: &[Field] = &[
    bit(13, "HSTOP8", "Top bit of the window's right edge"),
    aga(bits(12, 11, "HSTOPF", "The right edge's finest bits (super-hires steps)")),
    bits(10, 8, "VSTOP10-8", "Top bits of the line the window stops on"),
    bit(5, "HSTART8", "Top bit of the window's left edge"),
    aga(bits(4, 3, "HSTARTF", "The left edge's finest bits (super-hires steps)")),
    bits(2, 0, "VSTART10-8", "Top bits of the line the window starts on"),
];

const DIWSTRT: &[Field] = &[
    bits(15, 8, "VSTART", "Line the display window starts on (bits 7-0; bit 8 is 0)"),
    bits(7, 0, "HSTART", "Left edge in lowres pixels (bits 7-0; bit 8 is 0)"),
];

const DIWSTOP: &[Field] = &[
    bits(15, 8, "VSTOP", "Line the display window stops on (bits 7-0; bit 8 is the opposite of bit 7)"),
    bits(7, 0, "HSTOP", "Right edge in lowres pixels (bits 7-0; bit 8 is 1)"),
];

const DDF: &[Field] = &[bits(7, 1, "H", "Horizontal position in colour clocks (OCS ignores bit 1)")];

const CLXCON: &[Field] = &[
    bit(15, "ENSP7", "Sprite 7 counts with sprite 6 in collisions"),
    bit(14, "ENSP5", "Sprite 5 counts with sprite 4"),
    bit(13, "ENSP3", "Sprite 3 counts with sprite 2"),
    bit(12, "ENSP1", "Sprite 1 counts with sprite 0"),
    bits(11, 6, "ENBP6-1", "Which bitplanes (6 down to 1) take part in collisions"),
    bits(5, 0, "MVBP6-1", "The value each taking part must have to collide"),
];

const CLXDAT: &[Field] = &[
    bit(14, "S45-S67", "Sprite pair 4/5 hit pair 6/7"),
    bit(13, "S23-S67", "Sprite pair 2/3 hit pair 6/7"),
    bit(12, "S23-S45", "Sprite pair 2/3 hit pair 4/5"),
    bit(11, "S01-S67", "Sprite pair 0/1 hit pair 6/7"),
    bit(10, "S01-S45", "Sprite pair 0/1 hit pair 4/5"),
    bit(9, "S01-S23", "Sprite pair 0/1 hit pair 2/3"),
    bit(8, "EVEN-S67", "Even bitplanes hit sprite pair 6/7"),
    bit(7, "EVEN-S45", "Even bitplanes hit sprite pair 4/5"),
    bit(6, "EVEN-S23", "Even bitplanes hit sprite pair 2/3"),
    bit(5, "EVEN-S01", "Even bitplanes hit sprite pair 0/1"),
    bit(4, "ODD-S67", "Odd bitplanes hit sprite pair 6/7"),
    bit(3, "ODD-S45", "Odd bitplanes hit sprite pair 4/5"),
    bit(2, "ODD-S23", "Odd bitplanes hit sprite pair 2/3"),
    bit(1, "ODD-S01", "Odd bitplanes hit sprite pair 0/1"),
    bit(0, "EVEN-ODD", "Even bitplanes hit odd bitplanes"),
];

const VPOSR: &[Field] = &[
    bit(15, "LOF", "Long frame: in interlace, which of the two fields this is"),
    bits(14, 8, "ID", "Agnus or Alice's identity: 0 or $10 original, $20 or $30 ECS, $22 or $32 AGA ($10 set means NTSC)"),
    ecs(bit(7, "LOL", "Long line (NTSC lines alternate in length)")),
    bits(2, 0, "V10-8", "Top bits of the beam's line (the original chips have only bit 8)"),
];

const VPOSW: &[Field] = &[bit(15, "LOF", "Long frame: which interlace field comes next"), bits(2, 0, "V10-8", "Top bits of the beam's line")];

const VHPOS: &[Field] = &[bits(15, 8, "V7-0", "The beam's line, low eight bits"), bits(7, 0, "H8-1", "The beam's horizontal position in colour clocks")];

const JOYDAT: &[Field] = &[bits(15, 8, "Y", "Vertical counter (mouse movement, or joystick up and down)"), bits(7, 0, "X", "Horizontal counter (mouse movement, or joystick left and right)")];

const JOYTEST: &[Field] = &[bits(15, 10, "Y7-2", "Written into both ports' vertical counters"), bits(7, 2, "X7-2", "Written into both ports' horizontal counters")];

const POTDAT: &[Field] = &[bits(15, 8, "Y", "Count for the vertical (pin 9) potentiometer"), bits(7, 0, "X", "Count for the horizontal (pin 5) potentiometer")];

const POTGO: &[Field] = &[
    bit(15, "OUTRY", "Port 2 pin 9 is an output"),
    bit(14, "DATRY", "Port 2 pin 9's level (reading: 0 when port 2's right/middle button is pressed)"),
    bit(13, "OUTRX", "Port 2 pin 5 is an output"),
    bit(12, "DATRX", "Port 2 pin 5's level"),
    bit(11, "OUTLY", "Port 1 pin 9 is an output"),
    bit(10, "DATLY", "Port 1 pin 9's level (reading: 0 when the mouse's right button is pressed)"),
    bit(9, "OUTLX", "Port 1 pin 5 is an output"),
    bit(8, "DATLX", "Port 1 pin 5's level (middle button)"),
    bit(0, "START", "Starts the potentiometer counters (write only)"),
];

const SERDATR: &[Field] = &[
    bit(15, "OVRUN", "A byte came in before the last was read"),
    bit(14, "RBF", "A byte has come in"),
    bit(13, "TBE", "The transmit buffer is empty"),
    bit(12, "TSRE", "The transmit shift register is empty: everything's gone"),
    bit(11, "RXD", "The level on the receive line now"),
    bit(9, "STP", "Stop bit"),
    bit(8, "STP/DB8", "Stop bit, or data bit 8 in 9-bit mode"),
    bits(7, 0, "DB", "The byte received"),
];

const SERDAT: &[Field] = &[bits(9, 0, "DB", "Data to send, 8 or 9 bits, with a 1 above it for each stop bit")];
const SERPER: &[Field] = &[bit(15, "LONG", "Nine data bits"), bits(14, 0, "RATE", "Bit time in colour clocks, less one (PAL 9600 baud: 368)")];

const DSKLEN: &[Field] = &[
    bit(15, "DMAEN", "Disk DMA on (it must be written twice to start)"),
    bit(14, "WRITE", "Writing to the disk (0: reading)"),
    bits(13, 0, "LENGTH", "How many words to transfer"),
];

const DSKBYTR: &[Field] = &[
    bit(15, "DSKBYT", "A byte is ready"),
    bit(14, "DMAON", "Disk DMA is on"),
    bit(13, "DISKWRITE", "The disk is being written"),
    bit(12, "WORDEQUAL", "The disk data matches DSKSYNC right now"),
    bits(7, 0, "DATA", "The byte read"),
];

const COPCON: &[Field] = &[bit(1, "CDANG", "Copper danger: the copper may write registers below $80 (the blitter's, from $40; ECS and AGA, all of them)")];

const SPRPOS: &[Field] = &[bits(15, 8, "SV7-0", "Line the sprite starts on, low eight bits"), bits(7, 0, "SH8-1", "Left edge in lowres pixels, top eight bits of nine")];

const SPRCTL: &[Field] = &[
    bits(15, 8, "EV7-0", "Line the sprite stops on (the first it isn't shown), low eight bits"),
    bit(7, "ATT", "Attached: this odd sprite joins the even one before it, for 15 colours"),
    ecs(bit(6, "SV9", "Start line bit 9")),
    ecs(bit(5, "EV9", "Stop line bit 9")),
    ecs(bit(4, "SH1", "Half a lowres pixel further right (hires steps)")),
    aga(bit(3, "SH0", "A quarter of a lowres pixel further right (super-hires steps)")),
    bit(2, "SV8", "Start line bit 8"),
    bit(1, "EV8", "Stop line bit 8"),
    bit(0, "SH2", "Left edge bit 0 in lowres pixels"),
];

const COLOUR: &[Field] = &[
    ecs(bit(15, "T", "Genlock transparency, when BPLCON2's ZDCTEN is on")),
    bits(11, 8, "R", "Red, 0-15"),
    bits(7, 4, "G", "Green, 0-15"),
    bits(3, 0, "B", "Blue, 0-15"),
];

const AUDVOL: &[Field] = &[bits(6, 0, "VOL", "Volume, 0 to 64 (64 is full)")];
const AUDPER: &[Field] = &[bits(15, 0, "PER", "Colour clocks for each sample: the lower, the higher the note (DMA can't go below 124)")];
const AUDLEN: &[Field] = &[bits(15, 0, "LEN", "Length of the sample in words")];
const AUDDAT: &[Field] = &[bits(15, 8, "S1", "First sample, signed"), bits(7, 0, "S2", "Second sample, signed")];
const MODULO: &[Field] = &[bits(15, 0, "MOD", "Bytes added to the pointer at the end of each line (signed; even)")];
const PTH: &[Field] = &[bits(4, 0, "A20-16", "Top bits of the chip memory address (original chips use fewer)")];
const PTL: &[Field] = &[bits(15, 1, "A15-1", "Low bits of the chip memory address (always even)")];
const DENISEID: &[Field] = &[bits(7, 0, "ID", "$FC ECS Denise, $F8 Lisa (AGA); the original Denise has no such register")];
const MASKS: &[Field] = &[bits(15, 0, "MASK", "ANDed with source A's first (or last) word of each line")];
const DSKSYNC: &[Field] = &[bits(15, 0, "SYNC", "The word disk reads wait for ($4489 for the Amiga's own format)")];

const CIAA_PRA: &[Field] = &[
    bit(7, "/FIR1", "Port 2 fire button (0 when pressed)"),
    bit(6, "/FIR0", "Port 1 fire button, or the mouse's left button (0 when pressed)"),
    bit(5, "/RDY", "The selected drive is ready (0)"),
    bit(4, "/TK0", "The selected drive's heads are on track 0 (0)"),
    bit(3, "/WPRO", "The disk is write-protected (0)"),
    bit(2, "/CHNG", "The disk was taken out (0 until the heads step with a disk in)"),
    bit(1, "/LED", "Power light and audio filter: 0 bright and filtered, 1 dim and unfiltered"),
    bit(0, "OVL", "Overlay: ROM shows at address 0 (only right after a reset)"),
];
const CIAA_PRB: &[Field] = &[bits(7, 0, "DATA", "The parallel port's eight data lines")];
const CIAB_PRA: &[Field] = &[
    bit(7, "/DTR", "Serial: data terminal ready"),
    bit(6, "/RTS", "Serial: request to send"),
    bit(5, "/CD", "Serial: carrier detect"),
    bit(4, "/CTS", "Serial: clear to send"),
    bit(3, "/DSR", "Serial: data set ready"),
    bit(2, "SEL", "Parallel: printer selected"),
    bit(1, "POUT", "Parallel: paper out"),
    bit(0, "BUSY", "Parallel: printer busy"),
];
const CIAB_PRB: &[Field] = &[
    bit(7, "/MTR", "Motor on, latched into the drive when it's selected (0 on)"),
    bit(6, "/SEL3", "Selects drive 3 (0)"),
    bit(5, "/SEL2", "Selects drive 2 (0)"),
    bit(4, "/SEL1", "Selects drive 1 (0)"),
    bit(3, "/SEL0", "Selects drive 0, the internal one (0)"),
    bit(2, "/SIDE", "Which side of the disk: 0 the upper head, 1 the lower"),
    bit(1, "DIR", "Which way the heads step: 0 inwards, 1 outwards towards track 0"),
    bit(0, "/STEP", "Steps the heads one track as it goes from 0 to 1"),
];
const CIA_DDR: &[Field] = &[bits(7, 0, "DDR", "Each bit 1 makes that port line an output, 0 an input")];
const CIA_ICR: &[Field] = &[
    bit(7, "IR/SETCLR", "Reading: an enabled interrupt happened. Writing: 1 enables, 0 disables, each bit written as 1"),
    bit(4, "FLG", "The FLAG line went low (CIA-A: parallel acknowledge; CIA-B: disk index)"),
    bit(3, "SP", "The serial port has a full byte (CIA-A: a key from the keyboard)"),
    bit(2, "ALRM", "The event counter reached the alarm"),
    bit(1, "TB", "Timer B ran out"),
    bit(0, "TA", "Timer A ran out"),
];
const CIA_CRA: &[Field] = &[
    bit(7, "TODIN", "Event counter input is 50Hz rather than 60Hz (unused on the Amiga)"),
    bit(6, "SPMODE", "Serial port sends (1) or receives (0); CIA-A sets it to answer the keyboard"),
    bit(5, "INMODE", "Timer A counts CNT pulses (1) rather than E clock ticks (0)"),
    bit(4, "LOAD", "Loads the latch into timer A now (write only)"),
    bit(3, "RUNMODE", "One-shot: the timer stops when it runs out (0: it reloads and goes on)"),
    bit(2, "OUTMODE", "Port B bit 6 toggles (1) or pulses (0) when timer A runs out"),
    bit(1, "PBON", "Timer A shows on port B bit 6"),
    bit(0, "START", "Timer A is running"),
];
const CIA_CRB: &[Field] = &[
    bit(7, "ALARM", "Writes to the event counter set the alarm (0: the counter itself)"),
    bits(6, 5, "INMODE", "Timer B counts: 0 E clock ticks, 1 CNT pulses, 2 timer A running out, 3 timer A with CNT high"),
    bit(4, "LOAD", "Loads the latch into timer B now (write only)"),
    bit(3, "RUNMODE", "One-shot: the timer stops when it runs out"),
    bit(2, "OUTMODE", "Port B bit 7 toggles (1) or pulses (0) when timer B runs out"),
    bit(1, "PBON", "Timer B shows on port B bit 7"),
    bit(0, "START", "Timer B is running"),
];

// ---------- The registers ----------

struct Builder(Vec<Register>);

impl Builder {
    fn add(&mut self, offset: u32, name: &str, access: Access, since: Chipset, area: Area, about: &str, fields: &'static [Field]) {
        self.0.push(Register { address: CUSTOM + offset, name: name.to_string(), access, since, area, about: about.to_string(), fields });
    }

    /// A pointer in chip memory, as its high and low words.
    fn pointer(&mut self, offset: u32, name: &str, since: Chipset, area: Area, about: &str) {
        self.add(offset, &format!("{name}H"), Access::Write, since, area, &crate::i18n::tf!("{about} (high word; write both with one MOVE.L to {name}H)"), PTH);
        self.add(offset + 2, &format!("{name}L"), Access::Write, since, area, &format!("{about} (low word)"), PTL);
    }
}

fn build() -> Vec<Register> {
    use Access::*;
    use Area::*;
    use Chipset::*;
    let mut b = Builder(Vec::new());
    b.add(0x000, "BLTDDAT", Dma, Ocs, Blitter, "The blitter's destination data, which only its DMA uses", &[]);
    b.add(0x002, "DMACONR", Read, Ocs, DmaInterrupts, "Which DMA channels are on, and whether the blitter is busy (DMACON read back)", DMACON);
    b.add(0x004, "VPOSR", Read, Ocs, Display, "The beam's line (top bits), the frame type and the chip's identity", VPOSR);
    b.add(0x006, "VHPOSR", Read, Ocs, Display, "Where the beam is: its line (low bits) and horizontal position", VHPOS);
    b.add(0x008, "DSKDATR", Dma, Ocs, Disk, "Disk data being read, which only the disk DMA uses", &[]);
    b.add(0x00A, "JOY0DAT", Read, Ocs, Ports, "Port 1's mouse or joystick counters", JOYDAT);
    b.add(0x00C, "JOY1DAT", Read, Ocs, Ports, "Port 2's mouse or joystick counters", JOYDAT);
    b.add(0x00E, "CLXDAT", Read, Ocs, Sprites, "What has collided since it was last read (reading clears it)", CLXDAT);
    b.add(0x010, "ADKCONR", Read, Ocs, Audio, "Audio, disk and serial control (ADKCON read back)", ADKCON);
    b.add(0x012, "POT0DAT", Read, Ocs, Ports, "Port 1's potentiometer counters", POTDAT);
    b.add(0x014, "POT1DAT", Read, Ocs, Ports, "Port 2's potentiometer counters", POTDAT);
    b.add(0x016, "POTGOR", Read, Ocs, Ports, "The levels on the ports' pot pins: the second and third mouse buttons (also called POTINP)", POTGO);
    b.add(0x018, "SERDATR", Read, Ocs, Ports, "The serial port's received byte and status", SERDATR);
    b.add(0x01A, "DSKBYTR", Read, Ocs, Disk, "A byte read from the disk, and the disk's status", DSKBYTR);
    b.add(0x01C, "INTENAR", Read, Ocs, DmaInterrupts, "Which interrupts are enabled (INTENA read back)", INTERRUPTS);
    b.add(0x01E, "INTREQR", Read, Ocs, DmaInterrupts, "Which interrupts are asking (INTREQ read back)", INTERRUPTS);
    b.pointer(0x020, "DSKPT", Ocs, Disk, "Where disk DMA reads or writes");
    b.add(0x024, "DSKLEN", Write, Ocs, Disk, "Starts disk DMA: on, direction and length", DSKLEN);
    b.add(0x026, "DSKDAT", Dma, Ocs, Disk, "Disk data being written, which only the disk DMA uses", &[]);
    b.add(0x028, "REFPTR", Write, Ocs, Display, "The memory refresh counter (for testing the chips)", &[]);
    b.add(0x02A, "VPOSW", Write, Ocs, Display, "Sets the beam's line (top bits) and the frame type", VPOSW);
    b.add(0x02C, "VHPOSW", Write, Ocs, Display, "Sets where the beam is", VHPOS);
    b.add(0x02E, "COPCON", Write, Ocs, Copper, "Copper control: whether it may write the lower registers", COPCON);
    b.add(0x030, "SERDAT", Write, Ocs, Ports, "Sends a byte from the serial port", SERDAT);
    b.add(0x032, "SERPER", Write, Ocs, Ports, "The serial port's speed and word length", SERPER);
    b.add(0x034, "POTGO", Write, Ocs, Ports, "Sets the pot pins as inputs or outputs, and starts the pot counters", POTGO);
    b.add(0x036, "JOYTEST", Write, Ocs, Ports, "Writes both ports' mouse counters at once", JOYTEST);
    b.add(0x038, "STREQU", Strobe, Ocs, Display, "Horizontal sync with vertical blank and equalisation (the chips' own use)", &[]);
    b.add(0x03A, "STRVBL", Strobe, Ocs, Display, "Horizontal sync with vertical blank (the chips' own use)", &[]);
    b.add(0x03C, "STRHOR", Strobe, Ocs, Display, "Horizontal sync (the chips' own use)", &[]);
    b.add(0x03E, "STRLONG", Strobe, Ocs, Display, "Marks a long line (the chips' own use)", &[]);
    b.add(0x040, "BLTCON0", Write, Ocs, Blitter, "Blitter control: source A's shift, which channels are used and the logic function", BLTCON0);
    b.add(0x042, "BLTCON1", Write, Ocs, Blitter, "Blitter control: source B's shift, fill, direction and line mode", BLTCON1);
    b.add(0x044, "BLTAFWM", Write, Ocs, Blitter, "Mask for source A's first word of each line", MASKS);
    b.add(0x046, "BLTALWM", Write, Ocs, Blitter, "Mask for source A's last word of each line", MASKS);
    b.pointer(0x048, "BLTCPT", Ocs, Blitter, "Where the blitter reads source C");
    b.pointer(0x04C, "BLTBPT", Ocs, Blitter, "Where the blitter reads source B");
    b.pointer(0x050, "BLTAPT", Ocs, Blitter, "Where the blitter reads source A (line mode: the error term)");
    b.pointer(0x054, "BLTDPT", Ocs, Blitter, "Where the blitter writes D");
    b.add(0x058, "BLTSIZE", Write, Ocs, Blitter, "Starts the blit: height and width", BLTSIZE);
    b.add(0x05A, "BLTCON0L", Write, Ecs, Blitter, "BLTCON0's logic function by itself (a byte write to $05B)", BLTCON0L);
    b.add(0x05C, "BLTSIZV", Write, Ecs, Blitter, "The height of a big blit", BLTSIZV);
    b.add(0x05E, "BLTSIZH", Write, Ecs, Blitter, "The width of a big blit, and starts it", BLTSIZH);
    b.add(0x060, "BLTCMOD", Write, Ocs, Blitter, "Source C's modulo: bytes skipped after each line", MODULO);
    b.add(0x062, "BLTBMOD", Write, Ocs, Blitter, "Source B's modulo (line mode: 4 x the shorter side)", MODULO);
    b.add(0x064, "BLTAMOD", Write, Ocs, Blitter, "Source A's modulo (line mode: 4 x (shorter - longer side))", MODULO);
    b.add(0x066, "BLTDMOD", Write, Ocs, Blitter, "Destination D's modulo", MODULO);
    b.add(0x070, "BLTCDAT", Write, Ocs, Blitter, "Source C's data, for a blit that doesn't read C", &[]);
    b.add(0x072, "BLTBDAT", Write, Ocs, Blitter, "Source B's data (line mode: the texture pattern)", &[]);
    b.add(0x074, "BLTADAT", Write, Ocs, Blitter, "Source A's data (line mode: $8000, the pixel)", &[]);
    b.add(0x078, "SPRHDAT", Strobe, Ecs, Beam, "Marks UHRES sprite data for outside hardware", &[]);
    b.add(0x07A, "BPLHDAT", Strobe, Ecs, Beam, "Marks UHRES bitplane data for outside hardware", &[]);
    b.add(0x07C, "DENISEID", Read, Ecs, AgaExtras, "Which Denise or Lisa this is: the low byte is $F8 on AGA", DENISEID);
    b.add(0x07E, "DSKSYNC", Write, Ocs, Disk, "The sync word disk reads wait for", DSKSYNC);
    b.pointer(0x080, "COP1LC", Ocs, Copper, "The copper's first list: where it starts each frame");
    b.pointer(0x084, "COP2LC", Ocs, Copper, "The copper's second list");
    b.add(0x088, "COPJMP1", Strobe, Ocs, Copper, "Restarts the copper at COP1LC", &[]);
    b.add(0x08A, "COPJMP2", Strobe, Ocs, Copper, "Sends the copper to COP2LC (a jump, from inside a list)", &[]);
    b.add(0x08C, "COPINS", Dma, Ocs, Copper, "The copper instruction being fetched, which only the copper uses", &[]);
    b.add(0x08E, "DIWSTRT", Write, Ocs, Display, "Display window's top left corner (usually $2C81)", DIWSTRT);
    b.add(0x090, "DIWSTOP", Write, Ocs, Display, "Display window's bottom right corner (usually $2CC1 for PAL)", DIWSTOP);
    b.add(0x092, "DDFSTRT", Write, Ocs, Bitplanes, "Where bitplane fetching starts on each line (usually $38 lowres, $3C hires)", DDF);
    b.add(0x094, "DDFSTOP", Write, Ocs, Bitplanes, "Where bitplane fetching stops (usually $D0 lowres, $D4 hires)", DDF);
    b.add(0x096, "DMACON", Write, Ocs, DmaInterrupts, "Turns DMA channels on and off", DMACON);
    b.add(0x098, "CLXCON", Write, Ocs, Sprites, "What counts in collisions (writing it clears CLXCON2)", CLXCON);
    b.add(0x09A, "INTENA", Write, Ocs, DmaInterrupts, "Turns interrupts on and off", INTERRUPTS);
    b.add(0x09C, "INTREQ", Write, Ocs, DmaInterrupts, "Asks for or acknowledges interrupts (clear a bit to say it's dealt with)", INTERRUPTS);
    b.add(0x09E, "ADKCON", Write, Ocs, Audio, "Audio modulation, disk and serial control", ADKCON);
    for n in 0..4u32 {
        let base = 0x0A0 + n * 0x10;
        b.pointer(base, &format!("AUD{n}LC"), Ocs, Audio, &crate::i18n::tf!("Audio channel {n}'s sample"));
        b.add(base + 4, &format!("AUD{n}LEN"), Write, Ocs, Audio, &crate::i18n::tf!("Audio channel {n}'s sample length in words"), AUDLEN);
        b.add(base + 6, &format!("AUD{n}PER"), Write, Ocs, Audio, &crate::i18n::tf!("Audio channel {n}'s period: how fast it plays"), AUDPER);
        b.add(base + 8, &format!("AUD{n}VOL"), Write, Ocs, Audio, &crate::i18n::tf!("Audio channel {n}'s volume"), AUDVOL);
        b.add(base + 10, &format!("AUD{n}DAT"), Write, Ocs, Audio, &crate::i18n::tf!("Audio channel {n}'s next two samples, written by DMA or by hand"), AUDDAT);
    }
    for n in 1..=8u32 {
        let since = if n > 6 { Aga } else { Ocs };
        b.pointer(0x0E0 + (n - 1) * 4, &format!("BPL{n}PT"), since, Bitplanes, &crate::i18n::tf!("Bitplane {n}'s data"));
    }
    b.add(0x100, "BPLCON0", Write, Ocs, Bitplanes, "Bitplane control: resolution, how many planes and the display modes", BPLCON0);
    b.add(0x102, "BPLCON1", Write, Ocs, Bitplanes, "Horizontal scroll of each playfield", BPLCON1);
    b.add(0x104, "BPLCON2", Write, Ocs, Bitplanes, "Priorities of playfields and sprites, and genlock", BPLCON2);
    b.add(0x106, "BPLCON3", Write, Ecs, Bitplanes, "Enhanced features: colour bank, 24-bit colour, border and sprite resolution", BPLCON3);
    b.add(0x108, "BPL1MOD", Write, Ocs, Bitplanes, "Modulo of the odd bitplanes: bytes skipped after each line", MODULO);
    b.add(0x10A, "BPL2MOD", Write, Ocs, Bitplanes, "Modulo of the even bitplanes", MODULO);
    b.add(0x10C, "BPLCON4", Write, Aga, AgaExtras, "Bitplane colour XOR and sprite colour banks", BPLCON4);
    b.add(0x10E, "CLXCON2", Write, Aga, AgaExtras, "Collisions for bitplanes 7 and 8", CLXCON2);
    for n in 1..=8u32 {
        let since = if n > 6 { Aga } else { Ocs };
        b.add(0x110 + (n - 1) * 2, &format!("BPL{n}DAT"), Write, since, Bitplanes, &crate::i18n::tf!("Bitplane {n}'s next word, written by DMA (writing plane 1 shows them all)"), &[]);
    }
    for n in 0..8u32 {
        b.pointer(0x120 + n * 4, &format!("SPR{n}PT"), Ocs, Sprites, &crate::i18n::tf!("Sprite {n}'s data, for sprite DMA"));
    }
    for n in 0..8u32 {
        let base = 0x140 + n * 8;
        b.add(base, &format!("SPR{n}POS"), Write, Ocs, Sprites, &crate::i18n::tf!("Sprite {n}'s start line and left edge"), SPRPOS);
        b.add(base + 2, &format!("SPR{n}CTL"), Write, Ocs, Sprites, &crate::i18n::tf!("Sprite {n}'s stop line, attachment and position's other bits"), SPRCTL);
        b.add(base + 4, &format!("SPR{n}DATA"), Write, Ocs, Sprites, &crate::i18n::tf!("Sprite {n}'s first plane of data (writing it shows the sprite)"), &[]);
        b.add(base + 6, &format!("SPR{n}DATB"), Write, Ocs, Sprites, &crate::i18n::tf!("Sprite {n}'s second plane of data"), &[]);
    }
    for n in 0..32u32 {
        b.add(0x180 + n * 2, &format!("COLOR{n:02}"), Write, Ocs, Colours, &crate::i18n::tf!("Colour {n}: $0RGB (AGA: in the bank BPLCON3 picks; with LOCT, the low bits)"), COLOUR);
    }
    b.add(0x1C0, "HTOTAL", Write, Ecs, Beam, "The last colour clock of a line, with VARBEAMEN", &[]);
    b.add(0x1C2, "HSSTOP", Write, Ecs, Beam, "Where horizontal sync stops", &[]);
    b.add(0x1C4, "HBSTRT", Write, Ecs, Beam, "Where horizontal blank starts", &[]);
    b.add(0x1C6, "HBSTOP", Write, Ecs, Beam, "Where horizontal blank stops", &[]);
    b.add(0x1C8, "VTOTAL", Write, Ecs, Beam, "The last line of a frame, with VARBEAMEN", &[]);
    b.add(0x1CA, "VSSTOP", Write, Ecs, Beam, "The line vertical sync stops on", &[]);
    b.add(0x1CC, "VBSTRT", Write, Ecs, Beam, "The line vertical blank starts on", &[]);
    b.add(0x1CE, "VBSTOP", Write, Ecs, Beam, "The line vertical blank stops on", &[]);
    b.add(0x1D0, "SPRHSTRT", Write, Ecs, Beam, "The line UHRES sprites start on", &[]);
    b.add(0x1D2, "SPRHSTOP", Write, Ecs, Beam, "The line UHRES sprites stop on", &[]);
    b.add(0x1D4, "BPLHSTRT", Write, Ecs, Beam, "The line UHRES bitplanes start on", &[]);
    b.add(0x1D6, "BPLHSTOP", Write, Ecs, Beam, "The line UHRES bitplanes stop on", &[]);
    b.add(0x1D8, "HHPOSW", Write, Ecs, Beam, "Sets the second horizontal beam counter, for dual display", &[]);
    b.add(0x1DA, "HHPOSR", Read, Ecs, Beam, "The second horizontal beam counter", &[]);
    b.add(0x1DC, "BEAMCON0", Write, Ecs, Beam, "Beam control: PAL or NTSC, and which programmed timings are used", BEAMCON0);
    b.add(0x1DE, "HSSTRT", Write, Ecs, Beam, "Where horizontal sync starts", &[]);
    b.add(0x1E0, "VSSTRT", Write, Ecs, Beam, "The line vertical sync starts on", &[]);
    b.add(0x1E2, "HCENTER", Write, Ecs, Beam, "Where vertical sync goes on lines of an interlaced frame", &[]);
    b.add(0x1E4, "DIWHIGH", Write, Ecs, Display, "Display window's top bits; write it after DIWSTRT and DIWSTOP, which undo it", DIWHIGH);
    b.add(0x1E6, "BPLHMOD", Write, Ecs, Beam, "Modulo of the UHRES bitplanes", MODULO);
    b.pointer(0x1E8, "SPRHPT", Ecs, Beam, "UHRES sprite data, in video RAM");
    b.pointer(0x1EC, "BPLHPT", Ecs, Beam, "UHRES bitplane data, in video RAM");
    b.add(0x1FC, "FMODE", Write, Aga, AgaExtras, "Fetch mode: how wide bitplane and sprite fetches are, and scan doubling", FMODE);
    b.add(0x1FE, "NOOP", Strobe, Ocs, Copper, "Does nothing: a harmless place for the copper to write", &[]);

    let cia: [(&str, &str, &'static [Field], &'static [Field]); 16] = [
        ("PRA", "Port A", CIAA_PRA, CIAB_PRA),
        ("PRB", "Port B", CIAA_PRB, CIAB_PRB),
        ("DDRA", "Port A's directions ($03 for CIA-A, $C0 for CIA-B)", CIA_DDR, CIA_DDR),
        ("DDRB", "Port B's directions", CIA_DDR, CIA_DDR),
        ("TALO", "Timer A, low byte (writing sets the latch)", &[], &[]),
        ("TAHI", "Timer A, high byte (writing sets the latch, and starts a one-shot timer)", &[], &[]),
        ("TBLO", "Timer B, low byte", &[], &[]),
        ("TBHI", "Timer B, high byte", &[], &[]),
        ("TODLO", "Event counter, low byte (CIA-A counts frames, CIA-B lines)", &[], &[]),
        ("TODMID", "Event counter, middle byte", &[], &[]),
        ("TODHI", "Event counter, high byte (reading it holds the count until the low byte is read)", &[], &[]),
        ("", "", &[], &[]),
        ("SDR", "Serial data (CIA-A: the keyboard's)", &[], &[]),
        ("ICR", "Interrupt control: reading says what happened and clears it; writing enables or disables", CIA_ICR, CIA_ICR),
        ("CRA", "Control of timer A and the serial port", CIA_CRA, CIA_CRA),
        ("CRB", "Control of timer B and the event counter's alarm", CIA_CRB, CIA_CRB),
    ];
    for (n, (name, about, a, bf)) in cia.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let at = n as u32 * 0x100;
        let pra = |which: &str| match (*name, which) {
            ("PRA", "A") => " (fire buttons, disk status, power light, overlay)",
            ("PRB", "A") => " (the parallel port's data)",
            ("PRA", "B") => " (serial and parallel control lines)",
            ("PRB", "B") => " (the disk drives: motor, select, side, step)",
            _ => "",
        };
        b.0.push(Register { address: CIAA + at, name: format!("CIAA {name}"), access: ReadWrite, since: Ocs, area: CiaA, about: format!("{about}{}", pra("A")), fields: a });
        b.0.push(Register { address: CIAB + at, name: format!("CIAB {name}"), access: ReadWrite, since: Ocs, area: CiaB, about: format!("{about}{}", pra("B")), fields: bf });
    }
    b.0
}

/// Every register, custom chips first in address order, then the CIAs.
pub fn registers() -> &'static [Register] {
    static ALL: OnceLock<Vec<Register>> = OnceLock::new();
    ALL.get_or_init(build)
}

/// The custom register at an offset from $DFF000 (an odd offset finds the word it's the low byte of).
pub fn custom(offset: u32) -> Option<&'static Register> {
    let offset = offset & !1;
    registers().iter().find(|r| !r.is_cia() && r.offset() == offset)
}

/// The register at an address: a custom chip's, or a CIA's.
pub fn at(address: u32) -> Option<&'static Register> {
    let address = address & 0xFF_FFFF;
    if (CUSTOM..CUSTOM + 0x200).contains(&address) {
        return custom(address - CUSTOM);
    }
    registers().iter().find(|r| r.is_cia() && r.address == address)
}

/// A register by name, in any case: COLOR00 (or COLOR0), BPLCON0, a pointer pair's own name (COP1LC is COP1LCH,
/// as the NDK names it), POTINP for POTGOR, or a CIA's (CIAAPRA, CIAA PRA).
pub fn named(name: &str) -> Option<&'static Register> {
    let upper = name.trim().to_ascii_uppercase();
    let squashed = upper.replace([' ', '_'], "");
    let all = registers();
    if let Some(found) = all.iter().find(|r| r.name.replace(' ', "") == squashed) {
        return Some(found);
    }
    // COLOR0 as well as COLOR00.
    if let Some(n) = squashed.strip_prefix("COLOR").and_then(|n| n.parse::<u32>().ok()).filter(|n| *n < 32) {
        return custom(0x180 + n * 2);
    }
    // A pointer pair by its own name (DSKPT, BPL1PT, COP1LC, AUD0LC): its high word.
    if let Some(found) = all.iter().find(|r| r.fields == PTH && r.name == format!("{squashed}H")) {
        return Some(found);
    }
    match squashed.as_str() {
        "POTINP" => custom(0x016),
        "LISAID" => custom(0x07C),
        "NULL" => custom(0x1FE),
        _ => None,
    }
}

/// Built-in names a source may use without defining them: the NDK's hardware/custom.i group names and bases.
pub fn builtin_value(name: &str) -> Option<i64> {
    let lower = name.to_ascii_lowercase();
    let value = match lower.as_str() {
        "_custom" | "custom" => CUSTOM as i64,
        "_ciaa" | "ciaa" => CIAA as i64,
        "_ciab" | "ciab" => CIAB as i64,
        "bplpt" => 0x0E0,
        "sprpt" => 0x120,
        "spr" => 0x140,
        "color" => 0x180,
        "aud" | "aud0" => 0x0A0,
        "aud1" => 0x0B0,
        "aud2" => 0x0C0,
        "aud3" => 0x0D0,
        "ac_ptr" | "sd_pos" => 0,
        "ac_len" => 4,
        "ac_per" => 6,
        "ac_vol" => 8,
        "ac_dat" => 10,
        "ac_sizeof" => 16,
        "sd_ctl" => 2,
        "sd_dataa" => 4,
        "sd_datab" => 6,
        "sd_sizeof" => 8,
        "bltcon0l" => 0x05B,
        "ciapra" => 0x000,
        "ciaprb" => 0x100,
        "ciaddra" => 0x200,
        "ciaddrb" => 0x300,
        "ciatalo" => 0x400,
        "ciatahi" => 0x500,
        "ciatblo" => 0x600,
        "ciatbhi" => 0x700,
        "ciatodlow" => 0x800,
        "ciatodmid" => 0x900,
        "ciatodhi" => 0xA00,
        "ciasdr" => 0xC00,
        "ciaicr" => 0xD00,
        "ciacra" => 0xE00,
        "ciacrb" => 0xF00,
        _ => {
            // A custom register's own name stands for its offset, as custom.i has them.
            let found = named(&lower).filter(|r| !r.is_cia())?;
            return Some(found.offset() as i64);
        }
    };
    Some(value)
}

/// Whether a built-in name is a CIA register's offset (ciapra and the like), which needs a CIA's base.
pub fn is_cia_offset_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("cia") && !matches!(lower.as_str(), "ciaa" | "ciab")
}

// ---------- Reading values ----------

fn on_off(word: u16, n: u8) -> bool {
    word & (1 << n) != 0
}

/// A colour register's value as words: "red 15, green 0, blue 0".
pub fn rgb12(value: u16) -> (u8, u8, u8) {
    (((value >> 8) & 15) as u8, ((value >> 4) & 15) as u8, (value & 15) as u8)
}

/// What a value written to a register does, in a few words (the bits one by one come from `field_lines`).
pub fn explain(register: &Register, value: u16) -> String {
    let set = |fields: &[Field]| -> Vec<&'static str> { fields.iter().filter(|f| f.high == f.low && on_off(value, f.low)).map(|f| f.name).collect() };
    if let Some(n) = register.colour() {
        let (r, g, b) = rgb12(value);
        return crate::i18n::tf!("colour {n}: red {r}, green {g}, blue {b}");
    }
    let offset = register.offset();
    if register.is_cia() {
        return match register.fields {
            [] => format!("${value:02X} ({value})"),
            fields => {
                let on = set(fields);
                if on.is_empty() { crate::i18n::tr("no bits set").into() } else { on.join(", ") }
            }
        };
    }
    match offset {
        0x100 => {
            let bpu = (value >> 12) & 7;
            let planes = if on_off(value, 4) { if bpu == 0 { 8 } else { bpu + 8 } } else { bpu };
            let resolution = if on_off(value, 6) {
                "super-hires"
            } else if on_off(value, 15) {
                "hires"
            } else {
                "lowres"
            };
            let mut parts = vec![format!("{planes} bitplane{}", if planes == 1 { "" } else { "s" }), resolution.to_string()];
            if on_off(value, 11) {
                parts.push(if planes >= 7 { "HAM8".into() } else { "hold-and-modify".into() });
            }
            if on_off(value, 10) {
                parts.push("dual playfield".into());
            }
            if planes == 6 && !on_off(value, 11) && !on_off(value, 10) {
                parts.push(crate::i18n::tr("extra-half-brite (unless BPLCON2 KILLEHB)").into());
            }
            if on_off(value, 2) {
                parts.push("interlaced".into());
            }
            parts.push(if on_off(value, 9) { "colour on".into() } else { crate::i18n::tr("colour burst off").into() });
            for (n, what) in [(8, "genlock audio"), (7, "UHRES"), (5, "colour table bypassed"), (3, "light pen"), (1, "external sync"), (0, "ECS features on")] {
                if on_off(value, n) {
                    parts.push(what.into());
                }
            }
            parts.join(", ")
        }
        0x102 => {
            let (pf1, pf2) = (value & 15, (value >> 4) & 15);
            if value & 0xFF00 == 0 {
                if pf1 == 1 { crate::i18n::tf!("playfield 1 delayed 1 pixel, playfield 2 {pf2}") } else { crate::i18n::tf!("playfield 1 delayed {pf1} pixels, playfield 2 {pf2}") }
            } else {
                let fine1 = (((value >> 10) & 3) << 6) | (pf1 << 2) | ((value >> 8) & 3);
                let fine2 = (((value >> 14) & 3) << 6) | (pf2 << 2) | ((value >> 12) & 3);
                crate::i18n::tf!("playfield 1 delayed {fine1}, playfield 2 {fine2} (in super-hires steps, 4 to a lowres pixel)")
            }
        }
        0x104 => {
            let mut parts = vec![crate::i18n::tf!("playfield 1 at priority {}, playfield 2 at {}", value & 7, (value >> 3) & 7)];
            if on_off(value, 6) {
                parts.push(crate::i18n::tr("playfield 2 in front").into());
            }
            parts.extend(set(&BPLCON2[..6]).iter().map(|s| s.to_string()));
            parts.join(", ")
        }
        0x106 => {
            let offsets = [0, 2, 4, 8, 16, 32, 64, 128];
            let mut parts = vec![crate::i18n::tf!("colour bank {} (colours {}-{})", value >> 13, (value >> 13) * 32, (value >> 13) * 32 + 31)];
            parts.push(if on_off(value, 9) { crate::i18n::tr("colour writes set the low bits").into() } else { crate::i18n::tr("colour writes set the high bits").into() });
            parts.push(crate::i18n::tf!("playfield 2 colour offset {}", offsets[((value >> 10) & 7) as usize]));
            let spres = (value >> 6) & 3;
            if spres != 0 {
                parts.push(["", "lowres sprites", "hires sprites", "super-hires sprites"][spres as usize].into());
            }
            parts.extend(set(&BPLCON3[4..]).iter().map(|s| s.to_string()));
            parts.join(", ")
        }
        0x10C => crate::i18n::tf!("bitplane colours XOR ${:02X}, even sprites use colours from {}, odd from {}", value >> 8, ((value >> 4) & 15) * 16, (value & 15) * 16),
        0x002 | 0x096 => {
            let mut names = set(&DMACON[3..]);
            if offset == 0x002 {
                let mut busy = set(&DMACON[1..3]);
                busy.append(&mut names);
                return if busy.is_empty() { "nothing on".into() } else { busy.join(", ") };
            }
            if value & 0x7FFF == 0x7FFF || (!on_off(value, 15) && value & 0x07FF == 0x07FF) {
                names = vec!["everything"];
            }
            let what = if names.is_empty() { "nothing".to_string() } else { names.join(", ") };
            if on_off(value, 15) { format!("turns on {what}") } else { format!("turns off {what}") }
        }
        0x01C | 0x01E | 0x09A | 0x09C => {
            let mut names = set(&INTERRUPTS[1..]);
            if offset < 0x020 {
                return if names.is_empty() { "none".into() } else { names.join(", ") };
            }
            if value & 0x7FFF == 0x7FFF {
                names = vec!["every interrupt"];
            }
            let what = if names.is_empty() { "nothing".to_string() } else { names.join(", ") };
            match (offset, on_off(value, 15)) {
                (0x09A, true) => format!("enables {what}"),
                (0x09A, false) => format!("disables {what}"),
                (_, true) => format!("requests {what}"),
                (_, false) => format!("acknowledges {what}"),
            }
        }
        0x010 | 0x09E => {
            let names = set(&ADKCON[3..]);
            let what = if names.is_empty() { "nothing".to_string() } else { names.join(", ") };
            match (offset, on_off(value, 15)) {
                (0x010, _) => what,
                (_, true) => format!("sets {what}"),
                (_, false) => format!("clears {what}"),
            }
        }
        0x040 => {
            let channels: String = ["A", "B", "C", "D"].iter().enumerate().filter(|(i, _)| on_off(value, 11 - *i as u8)).map(|(_, c)| *c).collect();
            let lf = value & 0xFF;
            let what = minterm_name(lf as u8).map(|w| format!(" ({w})")).unwrap_or_default();
            crate::i18n::tf!("A shifted {}, channels {}, minterm ${lf:02X}{what}", value >> 12, if channels.is_empty() { "none".into() } else { channels })
        }
        0x05A => {
            let lf = (value & 0xFF) as u8;
            format!("minterm ${lf:02X}{}", minterm_name(lf).map(|w| format!(" ({w})")).unwrap_or_default())
        }
        0x042 => {
            if on_off(value, 0) {
                let octant = match value & 0x1C {
                    0x10 => 1,
                    0x00 => 2,
                    0x08 => 3,
                    0x14 => 4,
                    0x1C => 5,
                    0x0C => 6,
                    0x04 => 7,
                    _ => 8,
                };
                let mut parts = vec![crate::i18n::tf!("line mode, octant {octant}, texture from bit {}", value >> 12)];
                if on_off(value, 6) {
                    parts.push(crate::i18n::tr("starting sign negative").into());
                }
                if on_off(value, 1) {
                    parts.push(crate::i18n::tr("one dot per line").into());
                }
                parts.join(", ")
            } else {
                let mut parts = vec![format!("B shifted {}", value >> 12)];
                for (n, what) in [(4, "exclusive fill"), (3, "inclusive fill"), (2, "fill carry in"), (1, "descending"), (7, "D not written")] {
                    if on_off(value, n) {
                        parts.push(what.into());
                    }
                }
                parts.join(", ")
            }
        }
        0x058 => {
            let h = match value >> 6 {
                0 => 1024,
                h => h,
            };
            let w = match value & 63 {
                0 => 64,
                w => w,
            };
            {
                let pixels = w * 16;
                let across = if w == 1 { crate::i18n::tf!("1 word ({pixels} pixels)") } else { crate::i18n::tf!("{w} words ({pixels} pixels)") };
                if h == 1 { crate::i18n::tf!("{across} by 1 line") } else { crate::i18n::tf!("{across} by {h} lines") }
            }
        }
        0x08E => crate::i18n::tf!("starts on line ${:02X} ({}), x ${:02X} ({})", value >> 8, value >> 8, value & 0xFF, value & 0xFF),
        0x090 => {
            let v = (value >> 8) | if value & 0x8000 == 0 { 0x100 } else { 0 };
            let h = (value & 0xFF) | 0x100;
            crate::i18n::tf!("stops at line ${v:03X} ({v}), x ${h:03X} ({h})")
        }
        0x092 | 0x094 => format!("colour clock ${:02X} ({})", value & 0xFE, value & 0xFE),
        0x098 => {
            let planes: Vec<String> = (0..6).filter(|n| on_off(value, 6 + n)).map(|n| format!("{}", n + 1)).collect();
            let sprites: Vec<String> = [(12, "1"), (13, "3"), (14, "5"), (15, "7")].iter().filter(|(n, _)| on_off(value, *n)).map(|(_, s)| s.to_string()).collect();
            let mut parts = vec![if planes.is_empty() { "no bitplanes".to_string() } else { format!("bitplanes {} (match ${:02X})", planes.join(","), value & 63) }];
            if !sprites.is_empty() {
                parts.push(crate::i18n::tf!("odd sprites {} too", sprites.join(",")));
            }
            parts.join(", ")
        }
        0x1DC => {
            let mut parts = vec![if on_off(value, 5) { "PAL".to_string() } else { "NTSC".to_string() }];
            parts.extend(set(BEAMCON0).iter().filter(|n| **n != "PAL").map(|s| s.to_string()));
            parts.join(", ")
        }
        0x1E4 => {
            let vstart = value & 7;
            let vstop = (value >> 8) & 7;
            crate::i18n::tf!("start line bits 10-8 = {vstart}, horizontal bit 8 {}; stop line bits 10-8 = {vstop}, horizontal bit 8 {}", (value >> 5) & 1, (value >> 13) & 1)
        }
        0x1FC => {
            let width = |two: u16| match two {
                0 => 16,
                3 => 64,
                _ => 32,
            };
            let mut parts = vec![crate::i18n::tf!("bitplanes fetched {} bits at a time", width(value & 3)), crate::i18n::tf!("sprites {} pixels wide", width((value >> 2) & 3))];
            if on_off(value, 14) {
                parts.push(crate::i18n::tr("bitplane scan doubling").into());
            }
            if on_off(value, 15) {
                parts.push(crate::i18n::tr("sprite scan doubling").into());
            }
            parts.join(", ")
        }
        0x02E => if on_off(value, 1) { crate::i18n::tr("copper may write the blitter's registers").into() } else { crate::i18n::tr("copper kept to $80 and above").into() },
        0x024 => format!("{} {} words{}", if on_off(value, 14) { "write" } else { "read" }, value & 0x3FFF, if on_off(value, 15) { ", DMA on" } else { ", DMA off" }),
        0x032 => {
            let rate = value & 0x7FFF;
            format!("{} baud (PAL){}", PAL_CLOCK / (rate as u32 + 1), if on_off(value, 15) { ", 9 data bits" } else { "" })
        }
        0x034 | 0x016 => {
            let on = set(POTGO);
            if on.is_empty() { crate::i18n::tr("all bits clear").into() } else { on.join(", ") }
        }
        _ if (0x0A6..=0x0D6).contains(&offset) && offset & 0xF == 6 => {
            if value == 0 { "period 0 (silent)".into() } else { crate::i18n::tf!("{} Hz a sample (PAL)", PAL_CLOCK / value as u32) }
        }
        _ if (0x0A8..=0x0D8).contains(&offset) && offset & 0xF == 8 => format!("volume {}", (value & 0x7F).min(64)),
        _ if (0x0A4..=0x0D4).contains(&offset) && offset & 0xF == 4 => format!("{} words ({} bytes)", value, value as u32 * 2),
        0x108 | 0x10A | 0x060..=0x066 | 0x1E6 => format!("{} bytes", value as i16),
        _ if (0x140..0x180).contains(&offset) && offset & 7 == 0 => crate::i18n::tf!("start line ${:02X}, left edge ${:03X} (lowres, without its lowest bit)", value >> 8, (value & 0xFF) << 1),
        _ if (0x140..0x180).contains(&offset) && offset & 7 == 2 => {
            let attached = if on_off(value, 7) { ", attached" } else { "" };
            format!("stop line ${:03X}{attached}", (value >> 8) | ((value & 2) << 7))
        }
        _ => {
            let on = set(register.fields);
            if !on.is_empty() {
                on.join(", ")
            } else {
                format!("${value:04X} ({value})")
            }
        }
    }
}

/// What some well-known minterms do.
pub fn minterm_name(lf: u8) -> Option<&'static str> {
    Some(match lf {
        0x00 => "clear D",
        0xFF => "fill D with ones",
        0xF0 => "copy A",
        0xCC => "copy B",
        0xAA => "copy C",
        0x0F => "NOT A",
        0x33 => "NOT B",
        0x55 => "NOT C",
        0xCA => "cookie-cut: B where A is set, C elsewhere",
        0xFA => "A OR C",
        0xEE => "B OR C",
        0x5A => "A XOR C",
        0x66 => "B XOR C",
        0x3C => "A XOR B",
        0x0A => "C where A is clear",
        0x4A => "line: XOR",
        0xA0 => "A AND C",
        0x88 => "B AND C",
        0xC0 => "A AND B",
        0xFC => "A OR B",
        _ => return None,
    })
}

/// Each field's value, a line apiece, for a value written to (or read from) a register.
pub fn field_lines(register: &Register, value: u16) -> Vec<String> {
    let width = register.fields.iter().map(|f| f.name.len()).max().unwrap_or(0);
    register
        .fields
        .iter()
        .map(|f| {
            let v = f.value(value);
            let shown = if f.high == f.low { format!("{v}") } else if f.high - f.low >= 4 { format!("${v:X}") } else { format!("{v}") };
            let since = if f.since > register.since { format!(" ({})", f.since.name()) } else { String::new() };
            format!("{:>5}  {:<width$}  {shown:>4}  {}{since}", f.bits(), f.name, f.about)
        })
        .collect()
}

/// A register described: its heading, what it's for, and its bits.
pub fn describe(register: &Register) -> String {
    let mut lines = vec![register.heading(), register.about.clone()];
    let width = register.fields.iter().map(|f| f.name.len()).max().unwrap_or(0);
    for f in register.fields {
        let since = if f.since > register.since { format!(" ({})", f.since.name()) } else { String::new() };
        lines.push(format!("{:>5}  {:<width$}  {}{since}", f.bits(), f.name, f.about));
    }
    lines.join("\n")
}

/// A value written to a register described: the register, what the value does, and its bits.
pub fn describe_value(register: &Register, value: u16) -> String {
    let shown = if register.is_cia() { format!("${:02X}", value & 0xFF) } else { format!("${value:04X}") };
    let mut lines = vec![register.heading(), format!("{} = {shown}: {}", register.name, explain(register, value))];
    lines.extend(field_lines(register, value));
    lines.join("\n")
}

/// What a bit of a register is, as `btst`, `bset` and the like name it.
pub fn describe_bit(register: &Register, n: u8) -> String {
    let field = register.fields.iter().find(|f| f.low <= n && n <= f.high);
    let what = match field {
        Some(f) if f.high == f.low => format!("bit {n}: {} - {}", f.name, f.about),
        Some(f) => crate::i18n::tf!("bit {n}, part of {} (bits {}): {}", f.name, f.bits(), f.about),
        None => format!("bit {n}: unused"),
    };
    format!("{}\n{}", register.heading(), what)
}

// ---------- The copper ----------

/// One copper instruction, from its two words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copper {
    /// Writes a value to a register (its offset from $DFF000).
    Move { register: u16, value: u16 },
    /// Waits for the beam to reach a place, or skips the next instruction if it has.
    Wait { line: u8, x: u8, line_mask: u8, x_mask: u8, blitter: bool, skip: bool },
    /// $FFFF,$FFFE: waits for a place the beam never reaches, the end of the list.
    End,
}

impl Copper {
    pub fn decode(first: u16, second: u16) -> Copper {
        if first & 1 == 0 {
            return Copper::Move { register: first & 0x1FE, value: second };
        }
        if first == 0xFFFF && second == 0xFFFE {
            return Copper::End;
        }
        Copper::Wait {
            line: (first >> 8) as u8,
            x: (first & 0xFE) as u8,
            line_mask: ((second >> 8) & 0x7F) as u8 | 0x80,
            x_mask: (second & 0xFE) as u8,
            blitter: second & 0x8000 == 0,
            skip: second & 1 == 1,
        }
    }

    /// The two words back.
    pub fn encode(self) -> (u16, u16) {
        match self {
            Copper::Move { register, value } => (register & 0x1FE, value),
            Copper::End => (0xFFFF, 0xFFFE),
            Copper::Wait { line, x, line_mask, x_mask, blitter, skip } => {
                (((line as u16) << 8) | (x as u16 & 0xFE) | 1, if blitter { 0 } else { 0x8000 } | (((line_mask & 0x7F) as u16) << 8) | (x_mask as u16 & 0xFE) | skip as u16)
            }
        }
    }
}

/// What the copper list has done before an instruction, which changes what it means: BPLCON3's colour bank and
/// LOCT (AGA), and whether a wait has taken it past line 255 (waits after count from 256).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopperState {
    pub bplcon3: u16,
    pub past_255: bool,
}

impl CopperState {
    /// Says what an instruction does, and moves on past it.
    pub fn step(&mut self, op: Copper) -> String {
        let said = self.say(op);
        match op {
            Copper::Move { register: 0x106, value } => self.bplcon3 = value,
            Copper::Wait { line: 0xFF, x, skip: false, .. } if x >= 0xD0 => self.past_255 = true,
            _ => {}
        }
        said
    }

    fn say(&self, op: Copper) -> String {
        match op {
            Copper::End => crate::i18n::tr("END (waits for a place the beam never reaches)").into(),
            Copper::Move { register, value } => {
                let Some(found) = custom(register as u32) else {
                    return crate::i18n::tf!("MOVE ${value:04X} to ${register:03X} (no register there)");
                };
                let danger = match register {
                    0..0x40 => " (the copper can only write this on ECS or AGA, with CDANG)",
                    0x40..0x80 => " (needs COPCON's CDANG)",
                    _ => "",
                };
                let mut said = crate::i18n::tf!("MOVE ${value:04X} to {}{danger}", found.name);
                if !found.access.writable() {
                    said.push_str(" (it can't be written)");
                }
                if let Some(n) = found.colour() {
                    let bank = self.bplcon3 >> 13;
                    let colour = bank * 32 + n;
                    let (r, g, b) = rgb12(value);
                    if self.bplcon3 & 0x200 != 0 {
                        said.push_str(&crate::i18n::tf!(": colour {colour}, low bits of red {r}, green {g}, blue {b}"));
                    } else {
                        said.push_str(&crate::i18n::tf!(": colour {colour}, red {r}, green {g}, blue {b}"));
                    }
                } else if !found.fields.is_empty() || matches!(register, 0x058 | 0x08E | 0x090 | 0x092 | 0x094 | 0x108 | 0x10A) {
                    said.push_str(&format!(": {}", explain(found, value)));
                }
                said
            }
            Copper::Wait { line, x, line_mask, x_mask, blitter, skip } => {
                let full_line = if self.past_255 { line as u16 + 0x100 } else { line as u16 };
                let line_text = if self.past_255 { format!("${full_line:03X} ({full_line})") } else { format!("${line:02X} ({line})") };
                let mut said = if skip { crate::i18n::tf!("SKIP the next instruction if past line {line_text}, x ${x:02X}") } else { crate::i18n::tf!("WAIT for line {line_text}, x ${x:02X}") };
                if line == 0xFF && x >= 0xD0 && !skip {
                    said = crate::i18n::tf!("WAIT for line $FF, x ${x:02X}: past line 255, so later waits count from 256");
                }
                if line_mask != 0xFF {
                    said.push_str(&crate::i18n::tf!(", comparing only the line bits in ${line_mask:02X}"));
                }
                if x_mask == 0 {
                    said.push_str(", any x");
                } else if x_mask != 0xFE {
                    said.push_str(&crate::i18n::tf!(", comparing only the x bits in ${x_mask:02X}"));
                }
                if blitter {
                    said.push_str(", and for the blitter to finish");
                }
                said
            }
        }
    }

    /// A colour MOVE's colour number and its 24-bit colour as the screen shows it (with LOCT's low bits, the high
    /// bits taken from what was there, which a list can't know: they're shown as a copy of the low bits).
    pub fn colour_of(&self, op: Copper) -> Option<(u16, [u8; 3])> {
        let Copper::Move { register, value } = op else {
            return None;
        };
        let n = custom(register as u32)?.colour()?;
        let (r, g, b) = rgb12(value);
        Some(((self.bplcon3 >> 13) * 32 + n, [r * 17, g * 17, b * 17]))
    }
}

/// Decodes a run of copper words in order, saying what each instruction does.
#[cfg(test)]
pub fn decode_list(words: &[u16]) -> Vec<String> {
    let mut state = CopperState::default();
    words.as_chunks::<2>().0.iter().map(|[a, b]| state.step(Copper::decode(*a, *b))).collect()
}

/// The two words of a copper line for the source: `dc.w $0180,$0FFF`.
pub fn copper_line(op: Copper) -> String {
    let (a, b) = op.encode();
    let note = match op {
        Copper::Move { register, .. } => custom(register as u32).map(|r| r.name.clone()).unwrap_or_default(),
        Copper::Wait { line, x, skip, .. } => format!("{} line ${line:02X}, x ${x:02X}", if skip { "skip if past" } else { "wait for" }),
        Copper::End => crate::i18n::tr("end of the list").into(),
    };
    format!("dc.w ${a:04X},${b:04X}\t; {note}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_register_is_known_once() {
        let all = registers();
        let custom_count = all.iter().filter(|r| !r.is_cia()).count();
        let cia_count = all.iter().filter(|r| r.is_cia()).count();
        assert_eq!(cia_count, 30, "15 registers in each CIA");
        let mut addresses = HashSet::new();
        let mut names = HashSet::new();
        for r in all {
            assert!(addresses.insert(r.address), "{} shares ${:06X}", r.name, r.address);
            assert!(names.insert(r.name.clone()), "{} twice", r.name);
            assert!(!r.about.is_empty(), "{} says what it is", r.name);
            if !r.is_cia() {
                assert!(r.offset() < 0x200 && r.offset() % 2 == 0, "{}", r.name);
            }
            for f in r.fields {
                assert!(f.high >= f.low && f.high < 16, "{} {}", r.name, f.name);
                if r.is_cia() {
                    assert!(f.high < 8, "CIA registers are bytes: {} {}", r.name, f.name);
                }
            }
            // Fields don't overlap.
            let mut used = 0u16;
            for f in r.fields {
                let mask = ((1u32 << (f.high + 1)) - (1u32 << f.low)) as u16;
                assert_eq!(used & mask, 0, "{} {} overlaps", r.name, f.name);
                used |= mask;
            }
        }
        // Every name from the NDK's hardware/custom.i, and every word from $000 to $1FE that has one.
        for (name, offset) in [
            ("bltddat", 0x000), ("dmaconr", 0x002), ("vposr", 0x004), ("vhposr", 0x006), ("dskdatr", 0x008), ("joy0dat", 0x00A), ("joy1dat", 0x00C), ("clxdat", 0x00E),
            ("adkconr", 0x010), ("pot0dat", 0x012), ("pot1dat", 0x014), ("potinp", 0x016), ("serdatr", 0x018), ("dskbytr", 0x01A), ("intenar", 0x01C), ("intreqr", 0x01E),
            ("dskpt", 0x020), ("dsklen", 0x024), ("dskdat", 0x026), ("refptr", 0x028), ("vposw", 0x02A), ("vhposw", 0x02C), ("copcon", 0x02E), ("serdat", 0x030),
            ("serper", 0x032), ("potgo", 0x034), ("joytest", 0x036), ("strequ", 0x038), ("strvbl", 0x03A), ("strhor", 0x03C), ("strlong", 0x03E), ("bltcon0", 0x040),
            ("bltcon1", 0x042), ("bltafwm", 0x044), ("bltalwm", 0x046), ("bltcpt", 0x048), ("bltbpt", 0x04C), ("bltapt", 0x050), ("bltdpt", 0x054), ("bltsize", 0x058),
            ("bltsizv", 0x05C), ("bltsizh", 0x05E), ("bltcmod", 0x060), ("bltbmod", 0x062), ("bltamod", 0x064), ("bltdmod", 0x066), ("bltcdat", 0x070), ("bltbdat", 0x072),
            ("bltadat", 0x074), ("deniseid", 0x07C), ("dsksync", 0x07E), ("cop1lc", 0x080), ("cop2lc", 0x084), ("copjmp1", 0x088), ("copjmp2", 0x08A), ("copins", 0x08C),
            ("diwstrt", 0x08E), ("diwstop", 0x090), ("ddfstrt", 0x092), ("ddfstop", 0x094), ("dmacon", 0x096), ("clxcon", 0x098), ("intena", 0x09A), ("intreq", 0x09C),
            ("adkcon", 0x09E), ("bplcon0", 0x100), ("bplcon1", 0x102), ("bplcon2", 0x104), ("bplcon3", 0x106), ("bpl1mod", 0x108), ("bpl2mod", 0x10A), ("bplcon4", 0x10C),
            ("clxcon2", 0x10E), ("htotal", 0x1C0), ("hsstop", 0x1C2), ("hbstrt", 0x1C4), ("hbstop", 0x1C6), ("vtotal", 0x1C8), ("vsstop", 0x1CA), ("vbstrt", 0x1CC),
            ("vbstop", 0x1CE), ("sprhstrt", 0x1D0), ("sprhstop", 0x1D2), ("bplhstrt", 0x1D4), ("bplhstop", 0x1D6), ("hhposw", 0x1D8), ("hhposr", 0x1DA), ("beamcon0", 0x1DC),
            ("hsstrt", 0x1DE), ("vsstrt", 0x1E0), ("hcenter", 0x1E2), ("diwhigh", 0x1E4), ("fmode", 0x1FC),
        ] {
            assert_eq!(named(name).map(|r| r.offset()), Some(offset), "{name}");
            assert_eq!(builtin_value(name), Some(offset as i64), "{name}");
        }
        let unused: Vec<u32> = (0..0x200).step_by(2).filter(|o| custom(*o).is_none()).collect();
        assert_eq!(unused, [0x068, 0x06A, 0x06C, 0x06E, 0x076, 0x0AC, 0x0AE, 0x0BC, 0x0BE, 0x0CC, 0x0CE, 0x0DC, 0x0DE, 0x1F0, 0x1F2, 0x1F4, 0x1F6, 0x1F8, 0x1FA]);
        assert_eq!(custom_count, 256 - unused.len());
        for n in 0..32 {
            assert_eq!(named(&format!("COLOR{n:02}")).and_then(|r| r.colour()), Some(n as u16));
        }
        assert_eq!(named("color0").map(|r| r.offset()), Some(0x180));
        assert_eq!(named("AUD2VOL").map(|r| r.offset()), Some(0x0C8));
        assert_eq!(named("SPR7DATB").map(|r| r.offset()), Some(0x17E));
        assert_eq!(named("BPL8PTL").map(|r| (r.offset(), r.since)), Some((0x0FE, Chipset::Aga)));
        assert_eq!(named("ciaa pra").map(|r| r.address), Some(0xBFE001));
        assert_eq!(named("CIABPRB").map(|r| r.address), Some(0xBFD100));
        assert_eq!(at(0xBFEC01).map(|r| r.name.as_str()), Some("CIAA SDR"));
        assert_eq!(at(0xBFDE00).map(|r| r.name.as_str()), Some("CIAB CRA"));
        assert_eq!(at(0xDFF003).map(|r| r.name.as_str()), Some("DMACONR"), "an odd address is the word's low byte");
        assert_eq!(builtin_value("bplpt"), Some(0xE0));
        assert_eq!(builtin_value("_custom"), Some(0xDFF000));
    }

    #[test]
    fn chipsets_are_right() {
        let since = |name: &str| named(name).unwrap().since;
        assert_eq!(since("BPLCON0"), Chipset::Ocs);
        assert_eq!(since("BPLCON3"), Chipset::Ecs);
        assert_eq!(since("BEAMCON0"), Chipset::Ecs);
        assert_eq!(since("DIWHIGH"), Chipset::Ecs);
        assert_eq!(since("FMODE"), Chipset::Aga);
        assert_eq!(since("BPLCON4"), Chipset::Aga);
        assert_eq!(since("BPL7DAT"), Chipset::Aga);
        assert_eq!(named("COP1LCH").unwrap().access, Access::Write);
        assert_eq!(named("VHPOSR").unwrap().access, Access::Read);
        assert_eq!(named("COPJMP1").unwrap().access, Access::Strobe);
        let bplcon0 = named("BPLCON0").unwrap();
        assert_eq!(bplcon0.fields.iter().find(|f| f.name == "BPU3").map(|f| f.since), Some(Chipset::Aga));
        assert_eq!(bplcon0.fields.iter().find(|f| f.name == "SHRES").map(|f| f.since), Some(Chipset::Ecs));
    }

    #[test]
    fn values_are_read_into_their_bits() {
        let r = |name: &str| named(name).unwrap();
        assert_eq!(explain(r("BPLCON0"), 0x5200), "5 bitplanes, lowres, colour on");
        assert_eq!(explain(r("BPLCON0"), 0x9204), "1 bitplane, hires, interlaced, colour on");
        assert_eq!(explain(r("BPLCON0"), 0x0211), "8 bitplanes, lowres, colour on, ECS features on");
        assert_eq!(explain(r("BPLCON0"), 0x6200), "6 bitplanes, lowres, extra-half-brite (unless BPLCON2 KILLEHB), colour on");
        assert_eq!(explain(r("DMACON"), 0x83C0), "turns on DMAEN, BPLEN, COPEN, BLTEN");
        assert_eq!(explain(r("DMACON"), 0x7FFF), "turns off everything");
        assert_eq!(explain(r("INTENA"), 0xC020), "enables INTEN, VERTB");
        assert_eq!(explain(r("INTREQ"), 0x0020), "acknowledges VERTB");
        assert_eq!(explain(r("COLOR00"), 0x0F80), "colour 0: red 15, green 8, blue 0");
        assert_eq!(explain(r("BLTCON0"), 0x09F0), "A shifted 0, channels AD, minterm $F0 (copy A)");
        assert_eq!(explain(r("BLTCON1"), 0x0001 | 0x10), "line mode, octant 1, texture from bit 0");
        assert_eq!(explain(r("BLTSIZE"), (200 << 6) | 20), "20 words (320 pixels) by 200 lines");
        assert_eq!(explain(r("DIWSTRT"), 0x2C81), "starts on line $2C (44), x $81 (129)");
        assert_eq!(explain(r("DIWSTOP"), 0x2CC1), "stops at line $12C (300), x $1C1 (449)");
        assert_eq!(explain(r("FMODE"), 3), "bitplanes fetched 64 bits at a time, sprites 16 pixels wide");
        assert_eq!(explain(r("BPLCON3"), 0x2C00).split(", ").next(), Some("colour bank 1 (colours 32-63)"));
        assert_eq!(explain(r("AUD0VOL"), 64), "volume 64");
        assert_eq!(explain(r("BPL1MOD"), 0xFFD8), "-40 bytes");
        assert_eq!(explain(r("CIAA PRA"), 0x03), "/LED, OVL");
        let lines = field_lines(r("BPLCON0"), 0x5200);
        assert!(lines.iter().any(|l| l.contains("BPU") && l.contains(" 5 ")), "{lines:?}");
        assert!(describe_bit(r("DMACONR"), 14).contains("BBUSY"));
        assert!(describe(r("BPLCON0")).contains("ECSENA"));
    }

    #[test]
    fn copper_words_are_decoded() {
        assert_eq!(Copper::decode(0x0180, 0x0FFF), Copper::Move { register: 0x180, value: 0x0FFF });
        assert_eq!(Copper::decode(0xFFFF, 0xFFFE), Copper::End);
        let wait = Copper::decode(0x2C07, 0xFFFE);
        assert_eq!(wait, Copper::Wait { line: 0x2C, x: 0x06, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false });
        assert_eq!(wait.encode(), (0x2C07, 0xFFFE));
        assert_eq!(Copper::decode(0x2C07, 0xFFFF), Copper::Wait { line: 0x2C, x: 0x06, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: true });
        let said = decode_list(&[0x0180, 0x0FFF, 0x2C07, 0xFFFE, 0x0100, 0x5200, 0xFFDF, 0xFFFE, 0x2C07, 0xFFFE, 0x0040, 0x09F0, 0xFFFF, 0xFFFE]);
        assert_eq!(said[0], "MOVE $0FFF to COLOR00: colour 0, red 15, green 15, blue 15");
        assert_eq!(said[1], "WAIT for line $2C (44), x $06");
        assert_eq!(said[2], "MOVE $5200 to BPLCON0: 5 bitplanes, lowres, colour on");
        assert!(said[3].starts_with("WAIT for line $FF, x $DE: past line 255"), "{}", said[3]);
        assert_eq!(said[4], "WAIT for line $12C (300), x $06", "after $FFDF, lines count from 256");
        assert!(said[5].contains("needs COPCON's CDANG"));
        assert_eq!(said[6], "END (waits for a place the beam never reaches)");
        assert_eq!(decode_list(&[0x0001, 0x8000])[0], "WAIT for line $00 (0), x $00, comparing only the line bits in $80, any x");
        assert!(decode_list(&[0x2C01, 0x7F00])[0].ends_with("and for the blitter to finish"));
    }

    #[test]
    fn aga_colour_writes_follow_bplcon3() {
        // Bank 1, then LOCT: colour 33, high bits then low bits.
        let said = decode_list(&[0x0106, 0x2000, 0x0182, 0x0F00, 0x0106, 0x2200, 0x0182, 0x0080]);
        assert_eq!(said[1], "MOVE $0F00 to COLOR01: colour 33, red 15, green 0, blue 0");
        assert_eq!(said[3], "MOVE $0080 to COLOR01: colour 33, low bits of red 0, green 8, blue 0");
        let mut state = CopperState { bplcon3: 0xE000, past_255: false };
        assert_eq!(state.colour_of(Copper::Move { register: 0x1BE, value: 0x0FFF }), Some((255, [255, 255, 255])));
        state.step(Copper::Move { register: 0x106, value: 0 });
        assert_eq!(state.bplcon3, 0);
    }

    #[test]
    fn copper_lines_are_written_for_the_source() {
        assert_eq!(copper_line(Copper::Move { register: 0x180, value: 0xFFF }), "dc.w $0180,$0FFF\t; COLOR00");
        assert_eq!(copper_line(Copper::Wait { line: 0x2C, x: 6, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false }), "dc.w $2C07,$FFFE\t; wait for line $2C, x $06");
        assert_eq!(copper_line(Copper::End), "dc.w $FFFF,$FFFE\t; end of the list");
    }
}
