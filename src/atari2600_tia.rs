//! The Atari 2600's TIA, a colour clock at a time: the playfield, both players (their copies, stretching,
//! reflection and vertical delay), both missiles, the ball, their collisions, HMOVE (with the black comb it leaves
//! at the left), WSYNC and RSYNC, VSYNC and VBLANK, the fire buttons, and the two sound channels.
//!
//! The objects follow how the chip works as Stella's TIA core models it (Stella is GPL, so this is our own code,
//! written from the hardware's behaviour): each has a position counter clocked once a visible colour clock, which
//! starts its drawing a few clocks after passing its decode point; HMOVE adds extra clocks during the horizontal
//! blank. Writes to some registers take effect a colour clock or more after the processor makes them.

use crate::sfx::sound::TiaChannel;

/// Colour clocks in a line, and those of them in the horizontal blank.
pub const LINE: u16 = 228;
const HBLANK: u16 = 68;
/// The visible pixels in a line.
pub const WIDTH: usize = 160;
/// At most this many lines make a frame when the program never starts the vertical sync.
const MOST_LINES: usize = 400;

/// Whether a position counter at this value starts a copy of a player or missile with this NUSIZ: the main copy
/// at 156 (as the counter goes round), the others 16, 32 or 64 pixels on.
fn decodes(nusiz: u8, counter: u8) -> bool {
    counter == 156
        || match nusiz & 7 {
            1 => counter == 12,
            2 => counter == 28,
            3 => counter == 12 || counter == 28,
            4 => counter == 60,
            6 => counter == 28 || counter == 60,
            _ => false,
        }
}

/// What a RESxx write sets the position counter to: later in the line, the counter is a step or two further on.
fn reset_counter(blank: bool, hctr: u16) -> u8 {
    match (blank, hctr >= 73) {
        (true, false) => 159,
        (true, true) => 158,
        _ => 157,
    }
}

#[derive(Clone, Debug)]
struct Player {
    counter: u8,
    nusiz: u8,
    /// How many clocks each of its pixels lasts (1, 2 or 4), and one waiting to take over mid-draw.
    divider: u8,
    divider_pending: u8,
    divider_change: i8,
    rendering: bool,
    render_counter: i8,
    sample: u8,
    new: u8,
    old: u8,
    vdel: bool,
    reflect: bool,
    /// HMOVE's extra clocks: it moves while the movement count hasn't reached this.
    motion: u8,
    moving: bool,
}

impl Default for Player {
    fn default() -> Player {
        Player { counter: 0, nusiz: 0, divider: 1, divider_pending: 1, divider_change: -1, rendering: false, render_counter: 0, sample: 0, new: 0, old: 0, vdel: false, reflect: false, motion: 8, moving: false }
    }
}

impl Player {
    fn pattern(&self) -> u8 {
        let graphics = if self.vdel { self.old } else { self.new };
        // The first pixel drawn is bit 7, unless it's reflected.
        if self.reflect { graphics } else { graphics.reverse_bits() }
    }

    fn tick(&mut self) {
        if decodes(self.nusiz, self.counter) {
            self.rendering = true;
            self.sample = 0;
            self.render_counter = -5;
        } else if self.rendering {
            self.render_counter += 1;
            if self.divider == 1 {
                if self.render_counter > 0 {
                    self.sample += 1;
                }
                if self.render_counter >= 0 && self.divider_change >= 0 {
                    if self.divider_change == 0 {
                        self.divider = self.divider_pending;
                    }
                    self.divider_change -= 1;
                }
            } else {
                if self.render_counter > 1 && (self.render_counter - 1) % self.divider as i8 == 0 {
                    self.sample += 1;
                }
                if self.render_counter > 0 && self.divider_change >= 0 {
                    if self.divider_change == 0 {
                        self.divider = self.divider_pending;
                    }
                    self.divider_change -= 1;
                }
            }
            if self.sample > 7 {
                self.rendering = false;
            }
        }
        self.counter = if self.counter >= 159 { 0 } else { self.counter + 1 };
    }

    fn pixel(&self) -> bool {
        let trip = if self.divider == 1 { 0 } else { 1 };
        self.rendering && self.render_counter >= trip && self.pattern() >> self.sample & 1 != 0
    }

    fn reset(&mut self, counter: u8) {
        self.counter = counter;
        if self.rendering && self.render_counter < -2 {
            self.render_counter = -5 + (counter as i16 - 157) as i8;
        }
    }

    /// NUSIZ: the copies at once, and the stretching at once unless a copy is being drawn, when it changes as the
    /// chip's does part way.
    fn set_nusiz(&mut self, value: u8, blank: bool) {
        self.nusiz = value & 7;
        self.divider_pending = match value & 7 {
            5 => 2,
            7 => 4,
            _ => 1,
        };
        if self.divider_pending == self.divider {
            return;
        }
        if !self.rendering {
            self.divider = self.divider_pending;
            return;
        }
        let delta = self.render_counter + 5;
        match (self.divider, self.divider_pending) {
            (1, _) => {
                if delta < if blank { 4 } else { 3 } {
                    self.divider = self.divider_pending;
                } else {
                    self.divider_change = if blank { i8::from(delta < 5) } else { 1 };
                }
            }
            (_, 1) => {
                if delta < if blank { 4 } else { 3 } {
                    self.divider = 1;
                } else if delta < if blank { 6 } else { 5 } {
                    self.divider = 1;
                    self.render_counter -= 1;
                } else {
                    self.divider_change = if blank { 0 } else { 1 };
                }
            }
            _ => {
                let d = self.divider as i8;
                if self.render_counter < 1 || (blank && self.render_counter % d == 1) {
                    self.divider = self.divider_pending;
                } else {
                    self.divider_change = d - (self.render_counter - 1) % d;
                }
            }
        }
    }

    /// Where its missile goes when RESMP lets go of it: the middle of the player.
    fn missile_counter(&self) -> u8 {
        let back = match self.divider {
            1 => 5,
            2 => 9,
            _ => 12,
        };
        ((u16::from(self.counter) + 160 - back) % 160) as u8
    }
}

#[derive(Clone, Debug)]
struct Missile {
    counter: u8,
    nusiz: u8,
    width: u8,
    enabled: bool,
    locked: bool,
    rendering: bool,
    render_counter: i8,
    motion: u8,
    moving: bool,
}

impl Default for Missile {
    fn default() -> Missile {
        Missile { counter: 0, nusiz: 0, width: 1, enabled: false, locked: false, rendering: false, render_counter: 0, motion: 8, moving: false }
    }
}

impl Missile {
    fn tick(&mut self) {
        if !self.locked && decodes(self.nusiz, self.counter) {
            self.rendering = true;
            self.render_counter = -4;
        } else if self.rendering {
            self.render_counter += 1;
            if self.render_counter >= self.width as i8 {
                self.rendering = false;
            }
        }
        self.counter = if self.counter >= 159 { 0 } else { self.counter + 1 };
    }

    fn pixel(&self) -> bool {
        self.rendering && self.render_counter >= 0 && self.enabled && !self.locked
    }

    fn reset(&mut self, counter: u8, blank: bool) {
        self.counter = counter;
        if !self.rendering {
            return;
        }
        let shift = (counter as i16 - 157) as i8;
        if self.render_counter < 0 {
            self.render_counter = -4 + shift;
            return;
        }
        match self.width {
            8 => self.render_counter = shift + if self.render_counter >= 4 { 4 } else { 0 },
            4 => self.render_counter = shift,
            2 => {
                if blank {
                    self.rendering = self.render_counter > 1;
                } else if self.render_counter == 0 {
                    self.render_counter += 1;
                }
            }
            _ => {
                if blank {
                    self.rendering = self.render_counter > 0;
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Ball {
    counter: u8,
    width: u8,
    new: bool,
    old: bool,
    vdel: bool,
    rendering: bool,
    render_counter: i8,
    motion: u8,
    moving: bool,
}

impl Default for Ball {
    fn default() -> Ball {
        Ball { counter: 0, width: 1, new: false, old: false, vdel: false, rendering: false, render_counter: 0, motion: 8, moving: false }
    }
}

impl Ball {
    fn tick(&mut self) {
        if self.counter == 156 {
            self.rendering = true;
            self.render_counter = -4;
        } else if self.rendering {
            self.render_counter += 1;
            if self.render_counter >= self.width as i8 {
                self.rendering = false;
            }
        }
        self.counter = if self.counter >= 159 { 0 } else { self.counter + 1 };
    }

    fn pixel(&self) -> bool {
        self.rendering && self.render_counter >= 0 && if self.vdel { self.old } else { self.new }
    }

    /// RESBL starts the ball drawing at once, on this line, unlike the players and missiles.
    fn reset(&mut self, counter: u8) {
        self.counter = counter;
        self.rendering = true;
        self.render_counter = -4 + (counter as i16 - 157) as i8;
    }
}

/// A finished frame: each line's 160 colour values, from the line the vertical sync ended in.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub pixels: Vec<u8>,
    pub lines: usize,
}

/// Registers written with a delay, by where the delay queue keeps them: the TIA's own addresses, and three more
/// for the copying from new to old that GRP0, GRP1 and ENABL set off.
const SHUFFLE_P0: u8 = 0x40;
const SHUFFLE_P1: u8 = 0x41;
const SHUFFLE_BL: u8 = 0x42;

pub struct Tia {
    /// The colour clock in the line, 0 to 227.
    pub hctr: u16,
    blank: bool,
    extended: bool,
    moving: bool,
    movement_clock: u8,
    queue: Vec<(u8, u8, u8)>,
    players: [Player; 2],
    missiles: [Missile; 2],
    ball: Ball,
    pf: u32,
    ctrlpf: u8,
    reflect_now: bool,
    pf_pixel: bool,
    /// COLUP0, COLUP1, COLUPF and COLUBK.
    colours: [u8; 4],
    /// The eight collision registers' two bits each, as they're read.
    collisions: [u8; 8],
    vsync: bool,
    vblank: u8,
    /// The fire buttons held, and their latches (true while not pressed since VBLANK's bit 6 was set).
    pub fire: [bool; 2],
    latches: [bool; 2],
    /// WSYNC has been written: the processor waits for the line to end.
    pub wsync: bool,
    line: [u8; WIDTH],
    building: Vec<u8>,
    /// A frame finished and not yet taken.
    finished: Option<Frame>,
    /// Lines begun since the frame started.
    lines: usize,
    audio: [TiaChannel; 2],
    /// AUDC, AUDF and AUDV of each channel.
    sound: [[u8; 3]; 2],
    /// The sound's level at each of its ticks (two a line), from 0 to 1, since last taken.
    pub samples: Vec<f32>,
}

impl Default for Tia {
    fn default() -> Tia {
        Tia {
            hctr: 0,
            blank: true,
            extended: false,
            moving: false,
            movement_clock: 0,
            queue: Vec::new(),
            players: Default::default(),
            missiles: Default::default(),
            ball: Ball::default(),
            pf: 0,
            ctrlpf: 0,
            reflect_now: false,
            pf_pixel: false,
            colours: [0; 4],
            collisions: [0; 8],
            vsync: false,
            vblank: 0,
            fire: [false; 2],
            latches: [true; 2],
            wsync: false,
            line: [0; WIDTH],
            building: Vec::with_capacity(WIDTH * 320),
            finished: None,
            lines: 0,
            audio: Default::default(),
            sound: [[0; 3]; 2],
            samples: Vec::with_capacity(2048),
        }
    }
}

impl Tia {
    /// One colour clock.
    pub fn clock(&mut self) {
        if !self.queue.is_empty() {
            self.run_queue();
        }
        if self.moving && self.hctr & 3 == 0 {
            self.tick_movement();
        }
        if self.blank {
            self.tick_blank();
        } else {
            self.tick_visible();
        }
        // The sound ticks twice a line.
        if self.hctr == 9 || self.hctr == 123 {
            for channel in &mut self.audio {
                channel.tick();
            }
            self.samples.push(f32::from(self.audio[0].level + self.audio[1].level) / 30.0);
        }
        self.hctr += 1;
        if self.hctr >= LINE {
            self.next_line();
        }
    }

    fn run_queue(&mut self) {
        let mut i = 0;
        while i < self.queue.len() {
            if self.queue[i].0 == 0 {
                let (_, register, value) = self.queue.remove(i);
                self.apply(register, value);
            } else {
                self.queue[i].0 -= 1;
                i += 1;
            }
        }
    }

    fn tick_movement(&mut self) {
        let count = if self.movement_clock > 15 { 0 } else { self.movement_clock };
        let blank = self.blank;
        let mut any = false;
        macro_rules! movement {
            ($object:expr) => {
                if count == $object.motion {
                    $object.moving = false;
                }
                if $object.moving {
                    if blank {
                        $object.tick();
                    }
                    any = true;
                }
            };
        }
        movement!(self.missiles[0]);
        movement!(self.missiles[1]);
        movement!(self.players[0]);
        movement!(self.players[1]);
        movement!(self.ball);
        self.moving = any;
        self.movement_clock = self.movement_clock.saturating_add(1);
    }

    fn tick_blank(&mut self) {
        match self.hctr {
            0 => self.extended = false,
            67 if !self.extended => self.blank = false,
            75 if self.extended => self.blank = false,
            _ => {}
        }
        // The comb: HMOVE's eight clocks more of blank, black where the line would start.
        if self.extended && self.hctr >= HBLANK {
            let x = (self.hctr - HBLANK) as u8;
            self.tick_playfield(x);
            self.line[usize::from(x)] = 0;
        }
    }

    fn tick_playfield(&mut self, x: u8) {
        if x == 0 || x == 79 {
            self.reflect_now = self.ctrlpf & 1 != 0;
        }
        if x & 3 != 0 {
            return;
        }
        let i = u32::from(x >> 2);
        let bit = if i < 20 {
            i
        } else if self.reflect_now {
            39 - i
        } else {
            i - 20
        };
        self.pf_pixel = self.pf >> bit & 1 != 0;
    }

    fn tick_visible(&mut self) {
        let x = (self.hctr - HBLANK) as u8;
        self.tick_playfield(x);
        self.missiles[0].tick();
        self.missiles[1].tick();
        self.players[0].tick();
        self.players[1].tick();
        self.ball.tick();
        let (p0, p1, m0, m1, bl, pf) = (self.players[0].pixel(), self.players[1].pixel(), self.missiles[0].pixel(), self.missiles[1].pixel(), self.ball.pixel(), self.pf_pixel);
        let blanked = self.vblank & 2 != 0;
        if !blanked {
            let c = &mut self.collisions;
            c[0] |= u8::from(m0 && p1) << 7 | u8::from(m0 && p0) << 6;
            c[1] |= u8::from(m1 && p0) << 7 | u8::from(m1 && p1) << 6;
            c[2] |= u8::from(p0 && pf) << 7 | u8::from(p0 && bl) << 6;
            c[3] |= u8::from(p1 && pf) << 7 | u8::from(p1 && bl) << 6;
            c[4] |= u8::from(m0 && pf) << 7 | u8::from(m0 && bl) << 6;
            c[5] |= u8::from(m1 && pf) << 7 | u8::from(m1 && bl) << 6;
            c[6] |= u8::from(bl && pf) << 7;
            c[7] |= u8::from(p0 && p1) << 7 | u8::from(m0 && m1) << 6;
        }
        self.line[usize::from(x)] = if blanked { 0 } else { self.colour(x, p0 || m0, p1 || m1, bl, pf) };
    }

    /// The colour at a pixel, by the priorities CTRLPF sets.
    fn colour(&self, x: u8, p0: bool, p1: bool, bl: bool, pf: bool) -> u8 {
        let [c0, c1, cpf, cbk] = self.colours;
        let score = self.ctrlpf & 6 == 2;
        let pf_colour = if score {
            if x < 80 { c0 } else { c1 }
        } else {
            cpf
        };
        if self.ctrlpf & 4 != 0 {
            // The playfield and ball in front of the players.
            if bl {
                cpf
            } else if pf {
                pf_colour
            } else if p0 {
                c0
            } else if p1 {
                c1
            } else {
                cbk
            }
        } else if score {
            if p0 {
                c0
            } else if pf {
                pf_colour
            } else if p1 {
                c1
            } else if bl {
                cpf
            } else {
                cbk
            }
        } else if p0 {
            c0
        } else if p1 {
            c1
        } else if bl {
            cpf
        } else if pf {
            pf_colour
        } else {
            cbk
        }
    }

    fn next_line(&mut self) {
        self.hctr = 0;
        self.blank = true;
        self.wsync = false;
        self.building.extend_from_slice(&self.line);
        self.lines += 1;
        if self.lines >= MOST_LINES {
            self.finish_frame();
        }
    }

    fn finish_frame(&mut self) {
        let pixels = std::mem::replace(&mut self.building, Vec::with_capacity(WIDTH * 320));
        self.finished = Some(Frame { lines: pixels.len() / WIDTH, pixels });
        self.lines = 0;
    }

    /// The frame finished since last asked, if one has.
    pub fn take_frame(&mut self) -> Option<Frame> {
        self.finished.take()
    }

    /// The lines of the frame being drawn so far, and the line it's on.
    pub fn frame_so_far(&self) -> (&[u8], &[u8; WIDTH]) {
        (&self.building, &self.line)
    }

    fn push(&mut self, register: u8, value: u8, delay: u8) {
        self.queue.retain(|(_, r, _)| *r != register);
        self.queue.push((delay, register, value));
    }

    /// The processor writes a register.
    pub fn write(&mut self, address: u16, value: u8) {
        let register = (address & 0x3F) as u8;
        let blank = self.blank;
        match register {
            0x00 => {
                let on = value & 2 != 0;
                // The frame ends as the vertical sync does.
                if self.vsync && !on {
                    self.finish_frame();
                }
                self.vsync = on;
            }
            0x01 => self.push(register, value, 1),
            0x02 => self.wsync = true,
            0x03 => {
                // RSYNC: the line ends three clocks from now.
                for x in self.hctr.max(HBLANK)..LINE {
                    self.line[usize::from(x - HBLANK)] = 0;
                }
                self.hctr = LINE - 3;
            }
            0x04 | 0x05 => {
                let n = usize::from(register - 4);
                self.players[n].set_nusiz(value, blank);
                self.missiles[n].nusiz = value & 7;
                self.missiles[n].width = 1 << ((value >> 4) & 3);
            }
            0x06..=0x09 => self.colours[usize::from(register - 6)] = value & 0xFE,
            0x0A => {
                self.ctrlpf = value;
                self.ball.width = 1 << ((value >> 4) & 3);
            }
            0x0B..=0x0F => self.push(register, value, if register >= 0x0D { 2 } else { 1 }),
            // The resets take hold a colour clock after the write, as MAME (and the chip) have them.
            0x10..=0x14 => self.push(register, value, 1),
            0x15..=0x1A => {
                let channel = usize::from(register - 0x15) & 1;
                let which = usize::from(register - 0x15) / 2;
                self.sound[channel][which] = value;
                let [c, f, v] = self.sound[channel];
                self.audio[channel].set(c, f, v);
            }
            0x1B => {
                self.push(register, value, 1);
                self.push(SHUFFLE_P1, 0, 1);
            }
            0x1C => {
                self.push(register, value, 1);
                self.push(SHUFFLE_P0, 0, 1);
                self.push(SHUFFLE_BL, 0, 1);
            }
            0x1D..=0x1F => self.push(register, value, 1),
            0x20..=0x24 | 0x2B => self.push(register, value, 2),
            0x25 => self.players[0].vdel = value & 1 != 0,
            0x26 => self.players[1].vdel = value & 1 != 0,
            0x27 => self.ball.vdel = value & 1 != 0,
            0x28 | 0x29 => {
                let n = usize::from(register - 0x28);
                let locked = value & 2 != 0;
                if self.missiles[n].locked && !locked {
                    self.missiles[n].counter = self.players[n].missile_counter();
                }
                self.missiles[n].locked = locked;
            }
            0x2A => self.push(register, value, 6),
            0x2C => self.collisions = [0; 8],
            _ => {}
        }
    }

    /// A delayed write, as it takes effect.
    fn apply(&mut self, register: u8, value: u8) {
        let motion = (value >> 4) ^ 8;
        match register {
            0x01 => {
                // VBLANK's bit 6 holds the fire buttons' latches; setting it starts them afresh.
                if value & 0x40 != 0 && self.vblank & 0x40 == 0 {
                    self.latches = [true; 2];
                }
                self.vblank = value;
            }
            0x10 | 0x11 => self.players[usize::from(register - 0x10)].reset(reset_counter(self.blank, self.hctr)),
            0x12 | 0x13 => self.missiles[usize::from(register - 0x12)].reset(reset_counter(self.blank, self.hctr), self.blank),
            0x14 => self.ball.reset(reset_counter(self.blank, self.hctr)),
            0x0B => self.players[0].reflect = value & 8 != 0,
            0x0C => self.players[1].reflect = value & 8 != 0,
            0x0D => self.pf = (self.pf & !0x0F) | u32::from(value >> 4),
            0x0E => self.pf = (self.pf & !0xFF0) | u32::from(value.reverse_bits()) << 4,
            0x0F => self.pf = (self.pf & !0xFF000) | u32::from(value) << 12,
            0x1B => self.players[0].new = value,
            0x1C => self.players[1].new = value,
            0x1D => self.missiles[0].enabled = value & 2 != 0,
            0x1E => self.missiles[1].enabled = value & 2 != 0,
            0x1F => self.ball.new = value & 2 != 0,
            SHUFFLE_P0 => self.players[0].old = self.players[0].new,
            SHUFFLE_P1 => self.players[1].old = self.players[1].new,
            SHUFFLE_BL => self.ball.old = self.ball.new,
            0x20 => self.players[0].motion = motion,
            0x21 => self.players[1].motion = motion,
            0x22 => self.missiles[0].motion = motion,
            0x23 => self.missiles[1].motion = motion,
            0x24 => self.ball.motion = motion,
            0x2B => {
                self.players[0].motion = 8;
                self.players[1].motion = 8;
                self.missiles[0].motion = 8;
                self.missiles[1].motion = 8;
                self.ball.motion = 8;
            }
            0x2A => {
                self.movement_clock = 0;
                self.moving = true;
                if !self.extended {
                    self.extended = true;
                }
                self.players[0].moving = true;
                self.players[1].moving = true;
                self.missiles[0].moving = true;
                self.missiles[1].moving = true;
                self.ball.moving = true;
            }
            _ => {}
        }
    }

    /// The processor reads a register: the collisions and inputs give bits 7 and 6, and the rest is whatever was
    /// last on the data bus.
    pub fn read(&mut self, address: u16, bus: u8) -> u8 {
        let value = match address & 0x0F {
            r @ 0..=7 => self.collisions[usize::from(r)],
            // The paddles' inputs: nothing plugged in charges them.
            8..=0x0B => 0,
            r @ (0x0C | 0x0D) => {
                let n = usize::from(r - 0x0C);
                let up = if self.vblank & 0x40 != 0 {
                    self.latches[n] &= !self.fire[n];
                    self.latches[n]
                } else {
                    !self.fire[n]
                };
                u8::from(up) << 7
            }
            _ => bus & 0xC0,
        };
        (value & 0xC0) | (bus & 0x3F)
    }

    /// A fire button pressed or let go: a latch, when on, keeps the press.
    pub fn set_fire(&mut self, n: usize, pressed: bool) {
        self.fire[n] = pressed;
        if pressed && self.vblank & 0x40 != 0 {
            self.latches[n] = false;
        }
    }

    /// Whether the processor must wait: WSYNC written and the line not yet over.
    pub fn holding(&self) -> bool {
        self.wsync
    }
}
