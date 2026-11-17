//! Sound effects for the old machines' sound chips, made by ear: the AY-3-8910 (in AYFX's format, as AYFX
//! Editor saves a bank, for the Spectrum 128, the Amstrad CPC and the MSX), the Commodore 64's SID, the Atari
//! 2600's TIA and the Spectrum's beeper. An effect is a list of frames, one for each time the player is called
//! (once a TV frame), each saying what the chip does for that frame. They can be drawn, typed, or made from a
//! shape (a pitch that slides, a volume that rises and falls, noise) and its presets, and come out as source:
//! the effects as data, a player to call, and for most machines a program that plays each in turn.

use std::fmt::Write as _;

pub mod sound;
pub mod z80;

#[cfg(test)]
mod tests;

/// The sound chips.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Chip {
    #[default]
    Ay,
    Sid,
    Tia,
    Beeper,
}

impl Chip {
    pub const ALL: [Chip; 4] = [Chip::Ay, Chip::Sid, Chip::Tia, Chip::Beeper];

    pub fn name(self) -> &'static str {
        match self {
            Chip::Ay => "AY-3-8910 (AYFX)",
            Chip::Sid => "Commodore 64 SID",
            Chip::Tia => "Atari 2600 TIA",
            Chip::Beeper => "Spectrum beeper",
        }
    }

    pub fn machines(self) -> &'static [Machine] {
        match self {
            Chip::Ay => &[Machine::Spectrum128, Machine::Cpc, Machine::Msx],
            Chip::Sid => &[Machine::C64Pal, Machine::C64Ntsc],
            Chip::Tia => &[Machine::Atari2600Ntsc, Machine::Atari2600Pal],
            Chip::Beeper => &[Machine::Spectrum48Beeper, Machine::Spectrum128Beeper],
        }
    }
}

/// The machines, each with its chip's clock and frame rate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Machine {
    #[default]
    Spectrum128,
    Cpc,
    Msx,
    C64Pal,
    C64Ntsc,
    Atari2600Ntsc,
    Atari2600Pal,
    Spectrum48Beeper,
    Spectrum128Beeper,
}

impl Machine {
    pub const ALL: [Machine; 9] = [
        Machine::Spectrum128,
        Machine::Cpc,
        Machine::Msx,
        Machine::C64Pal,
        Machine::C64Ntsc,
        Machine::Atari2600Ntsc,
        Machine::Atari2600Pal,
        Machine::Spectrum48Beeper,
        Machine::Spectrum128Beeper,
    ];

    pub fn chip(self) -> Chip {
        match self {
            Machine::Spectrum128 | Machine::Cpc | Machine::Msx => Chip::Ay,
            Machine::C64Pal | Machine::C64Ntsc => Chip::Sid,
            Machine::Atari2600Ntsc | Machine::Atari2600Pal => Chip::Tia,
            Machine::Spectrum48Beeper | Machine::Spectrum128Beeper => Chip::Beeper,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Machine::Spectrum128 => "ZX Spectrum 128",
            Machine::Cpc => "Amstrad CPC",
            Machine::Msx => "MSX",
            Machine::C64Pal => "Commodore 64 (PAL)",
            Machine::C64Ntsc => "Commodore 64 (NTSC)",
            Machine::Atari2600Ntsc => "Atari 2600 (NTSC)",
            Machine::Atari2600Pal => "Atari 2600 (PAL)",
            Machine::Spectrum48Beeper => "ZX Spectrum 48K",
            Machine::Spectrum128Beeper => "ZX Spectrum 128K",
        }
    }

    /// The AY's clock; the SID's (the processor's); the TIA's sound, ticking at the colour clock over 114; the
    /// beeper's, the Z80's.
    pub fn clock(self) -> f64 {
        match self {
            Machine::Spectrum128 => 1_773_400.0,
            Machine::Cpc => 1_000_000.0,
            Machine::Msx => 1_789_772.5,
            Machine::C64Pal => 985_248.0,
            Machine::C64Ntsc => 1_022_727.0,
            Machine::Atari2600Ntsc => 3_579_545.0 / 114.0,
            Machine::Atari2600Pal => 3_546_894.0 / 114.0,
            Machine::Spectrum48Beeper => 3_500_000.0,
            Machine::Spectrum128Beeper => 3_546_900.0,
        }
    }

    /// How many times a second the player is called: the TV's frames.
    pub fn frame_rate(self) -> f64 {
        match self {
            Machine::Spectrum128 | Machine::Cpc | Machine::Msx => 50.0,
            Machine::C64Pal => 985_248.0 / (312.0 * 63.0),
            Machine::C64Ntsc => 1_022_727.0 / (263.0 * 65.0),
            Machine::Atari2600Ntsc => 3_579_545.0 / (262.0 * 228.0),
            Machine::Atari2600Pal => 3_546_894.0 / (312.0 * 228.0),
            Machine::Spectrum48Beeper => 3_500_000.0 / f64::from(BEEPER_FRAME_48),
            Machine::Spectrum128Beeper => 3_546_900.0 / f64::from(BEEPER_FRAME_128),
        }
    }

    /// A beeper frame's T-states.
    pub fn frame_t(self) -> u32 {
        if self == Machine::Spectrum128Beeper { BEEPER_FRAME_128 } else { BEEPER_FRAME_48 }
    }

    /// The processor the player is for.
    pub fn processor(self) -> &'static str {
        match self.chip() {
            Chip::Ay | Chip::Beeper => "Z80",
            Chip::Sid | Chip::Tia => "6502",
        }
    }
}

const BEEPER_FRAME_48: u32 = 69_888;
const BEEPER_FRAME_128: u32 = 70_908;

/// A beeper wave's T-states, beyond 26 for each count of its two waits: what the player's loop takes.
pub const BEEPER_WAVE_FIXED: u32 = 401;
/// Of those, the first (high) half's, the rest the second's.
pub const BEEPER_HIGH_FIXED: u32 = 192;
/// Each count of a wait.
pub const BEEPER_COUNT_T: u32 = 26;

/// One frame of an effect: what the chip does until the next. What each field means depends on the chip:
///
/// | | AY | SID | TIA | Beeper |
/// |---|---|---|---|---|
/// | `tone` | tone on | gate on | | sounding |
/// | `noise` | noise on | | | noise rather than a tone |
/// | `period` | tone period (12 bits) | frequency (16 bits) | AUDF (0-31) | wave length, in counts of 26 T-states |
/// | `noise_period` | noise period (0-31) | | | |
/// | `volume` | 0-15 | | AUDV (0-15) | how wide the wave's high half is (1-15; 0 quiet) |
/// | `wave` | | waveform, sync and ring ($10 triangle, $20 saw, $40 pulse, $80 noise) | AUDC (0-15) | |
/// | `width` | | pulse width (12 bits) | | |
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Frame {
    pub tone: bool,
    pub noise: bool,
    pub period: u16,
    pub noise_period: u8,
    pub volume: u8,
    pub wave: u8,
    pub width: u16,
}

impl Frame {
    /// A frame that makes no sound.
    pub fn quiet(chip: Chip) -> Frame {
        match chip {
            Chip::Ay => Frame { tone: false, noise: false, period: 0x100, noise_period: 0, volume: 0, wave: 0, width: 0 },
            Chip::Sid => Frame { tone: false, noise: false, period: 0x1000, noise_period: 0, volume: 0, wave: 0x40, width: 0x800 },
            Chip::Tia => Frame { tone: false, noise: false, period: 10, noise_period: 0, volume: 0, wave: 4, width: 0 },
            Chip::Beeper => Frame { tone: false, noise: false, period: 200, noise_period: 0, volume: 0, wave: 0, width: 0 },
        }
    }

    /// Kept within what the chip has.
    pub fn clamped(mut self, chip: Chip) -> Frame {
        match chip {
            Chip::Ay => {
                self.period &= 0xFFF;
                self.noise_period &= 0x1F;
                self.volume &= 0xF;
            }
            Chip::Sid => {
                self.wave &= 0xF6;
                self.width &= 0xFFF;
            }
            Chip::Tia => {
                self.period &= 0x1F;
                self.volume &= 0xF;
                self.wave &= 0xF;
            }
            Chip::Beeper => {
                self.period = self.period.clamp(2, 60_000);
                self.volume = self.volume.min(15);
            }
        }
        self
    }

    /// Whether it makes a sound at all (for the SID, whether the gate's on).
    pub fn sounding(&self, chip: Chip) -> bool {
        match chip {
            Chip::Ay => self.volume > 0 && (self.tone || self.noise),
            Chip::Sid => self.tone && self.wave & 0xF0 != 0,
            Chip::Tia => self.volume > 0,
            Chip::Beeper => self.volume > 0 && (self.tone || self.noise),
        }
    }
}

/// The TIA's sound: how many of its ticks each wave of AUDC's kind takes, for AUDF of 0. None where it doesn't
/// make a note: AUDC 0 and 11 hold the output at the volume.
pub fn tia_ticks(audc: u8) -> Option<f64> {
    Some(match audc & 15 {
        0 | 11 => return None,
        4 | 5 => 2.0,
        12 | 13 => 6.0,
        6 | 10 => 31.0,
        14 => 93.0,
        1 => 15.0,
        2 | 3 => 465.0,
        7 | 9 => 31.0,
        8 => 511.0,
        _ => 93.0,
    })
}

/// The note a frame plays, in hertz, if it plays one.
pub fn hz(machine: Machine, frame: &Frame) -> Option<f64> {
    let clock = machine.clock();
    match machine.chip() {
        Chip::Ay => frame.tone.then(|| clock / (16.0 * f64::from(frame.period.max(1)))),
        Chip::Sid => Some(f64::from(frame.period) * clock / 16_777_216.0).filter(|&h| h > 0.0 && frame.wave & 0x80 == 0),
        Chip::Tia => tia_ticks(frame.wave).map(|ticks| clock / (ticks * f64::from(frame.period + 1))),
        Chip::Beeper => (frame.tone && !frame.noise).then(|| clock / f64::from(BEEPER_WAVE_FIXED + BEEPER_COUNT_T * u32::from(frame.period))),
    }
}

/// The period (or frequency, or AUDF) that comes nearest a note, for a frame as it is.
pub fn period_for(machine: Machine, hz: f64, frame: &Frame) -> u16 {
    let clock = machine.clock();
    let hz = hz.max(1.0);
    match machine.chip() {
        Chip::Ay => (clock / (16.0 * hz)).round().clamp(1.0, 4095.0) as u16,
        Chip::Sid => (hz * 16_777_216.0 / clock).round().clamp(1.0, 65535.0) as u16,
        Chip::Tia => {
            let ticks = tia_ticks(frame.wave).unwrap_or(2.0);
            (clock / (ticks * hz) - 1.0).round().clamp(0.0, 31.0) as u16
        }
        Chip::Beeper => ((clock / hz - f64::from(BEEPER_WAVE_FIXED)) / f64::from(BEEPER_COUNT_T)).round().clamp(2.0, 60_000.0) as u16,
    }
}

/// The note's name nearest a pitch, as C#4 (A4 is 440 Hz).
pub fn note_name(hz: f64) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let semitones = (12.0 * (hz / 440.0).log2()).round() as i64 + 57;
    format!("{}{}", NAMES[semitones.rem_euclid(12) as usize], semitones.div_euclid(12))
}

/// One sound effect.
#[derive(Clone, PartialEq, Debug)]
pub struct Effect {
    pub name: String,
    pub frames: Vec<Frame>,
    /// The SID's attack and decay, and sustain and release, as its registers take them.
    pub adsr: [u8; 2],
}

/// The effects for one machine.
#[derive(Clone, PartialEq, Debug)]
pub struct Bank {
    pub machine: Machine,
    pub effects: Vec<Effect>,
}

impl Default for Bank {
    fn default() -> Bank {
        let machine = Machine::default();
        Bank { machine, effects: vec![Effect::from_shape("laser", machine, &Shape::preset(Preset::Laser))] }
    }
}

/// The longest an effect can be.
pub const MOST_FRAMES: usize = 1000;

/// A label made from a name: letters, digits and underlines, not starting with a digit.
pub fn label_of(name: &str) -> String {
    let mut out: String = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "effect".into()
    } else if out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("e{out}")
    } else {
        out
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Shapes: an effect described as a sound, made into any chip's frames.

/// What a shape's noise does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Noise {
    #[default]
    Off,
    /// Noise rather than a note, all the way through.
    All,
    /// Noise for the first few frames, then the note: the crack of a hit.
    Start,
    /// Noise with the note (on the AY; the others choose the noise).
    Mixed,
}

impl Noise {
    pub const ALL: [Noise; 4] = [Noise::Off, Noise::All, Noise::Start, Noise::Mixed];

    pub fn name(self) -> &'static str {
        match self {
            Noise::Off => "None",
            Noise::All => "Noise only",
            Noise::Start => "At the start",
            Noise::Mixed => "With the note",
        }
    }
}

/// The note's sound, where the chip has a choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wave {
    #[default]
    Square,
    Triangle,
    Saw,
    /// A rough, buzzing note: the TIA's and SID's harsher sounds.
    Buzz,
}

impl Wave {
    pub const ALL: [Wave; 4] = [Wave::Square, Wave::Triangle, Wave::Saw, Wave::Buzz];

    pub fn name(self) -> &'static str {
        match self {
            Wave::Square => "Square",
            Wave::Triangle => "Triangle",
            Wave::Saw => "Sawtooth",
            Wave::Buzz => "Buzz",
        }
    }
}

/// An effect described as a sound: a pitch that slides and wobbles, a volume that rises, holds and falls, and noise.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shape {
    pub frames: usize,
    /// The note at the start, in hertz.
    pub pitch: f64,
    /// Semitones a frame the note slides (up when more than 0).
    pub slide: f64,
    /// Semitones a frame added to the slide each frame: a slide that speeds up or slows down.
    pub curve: f64,
    /// Semitones the note wobbles either way, and frames for one wobble.
    pub vibrato: f64,
    pub vibrato_frames: f64,
    /// Semitones the note jumps, and the frame it jumps at (0 for never): a coin's two notes.
    pub jump: f64,
    pub jump_at: usize,
    /// Frames after which the note starts its slide again (0 for never): an arpeggio, a siren.
    pub repeat: usize,
    /// Frames rising to full volume, then held there; the rest fade away.
    pub attack: usize,
    pub hold: usize,
    pub noise: Noise,
    /// How high the noise is, from 0 (a rumble) to 1 (a hiss), and how much it changes a frame.
    pub noise_pitch: f64,
    pub noise_slide: f64,
    pub wave: Wave,
    /// The pulse's width, for the SID's pulse and the beeper (0.5 is square).
    pub width: f64,
}

/// The ready-made shapes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    Pickup,
    Laser,
    Explosion,
    PowerUp,
    Hit,
    Jump,
    Blip,
    Siren,
}

impl Preset {
    pub const ALL: [Preset; 8] = [Preset::Pickup, Preset::Laser, Preset::Explosion, Preset::PowerUp, Preset::Hit, Preset::Jump, Preset::Blip, Preset::Siren];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Pickup => "Pickup",
            Preset::Laser => "Laser",
            Preset::Explosion => "Explosion",
            Preset::PowerUp => "Power-up",
            Preset::Hit => "Hit",
            Preset::Jump => "Jump",
            Preset::Blip => "Blip",
            Preset::Siren => "Siren",
        }
    }
}

impl Default for Shape {
    fn default() -> Shape {
        Shape::preset(Preset::Blip)
    }
}

/// A small random number generator, so the same seed makes the same sound.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }

    fn chance(&mut self, odds: f64) -> bool {
        self.next() < odds
    }
}

impl Shape {
    pub fn preset(preset: Preset) -> Shape {
        let plain = Shape {
            frames: 10,
            pitch: 880.0,
            slide: 0.0,
            curve: 0.0,
            vibrato: 0.0,
            vibrato_frames: 4.0,
            jump: 0.0,
            jump_at: 0,
            repeat: 0,
            attack: 0,
            hold: 2,
            noise: Noise::Off,
            noise_pitch: 0.5,
            noise_slide: 0.0,
            wave: Wave::Square,
            width: 0.5,
        };
        match preset {
            Preset::Pickup => Shape { frames: 12, pitch: 988.0, jump: 5.0, jump_at: 3, hold: 4, ..plain },
            Preset::Laser => Shape { frames: 12, pitch: 1760.0, slide: -2.0, hold: 3, wave: Wave::Saw, width: 0.25, ..plain },
            Preset::Explosion => Shape { frames: 40, pitch: 110.0, hold: 4, noise: Noise::All, noise_pitch: 0.35, noise_slide: -0.012, ..plain },
            Preset::PowerUp => Shape { frames: 36, pitch: 330.0, slide: 0.9, repeat: 6, hold: 24, wave: Wave::Triangle, ..plain },
            Preset::Hit => Shape { frames: 10, pitch: 220.0, slide: -1.5, hold: 1, noise: Noise::Start, noise_pitch: 0.7, ..plain },
            Preset::Jump => Shape { frames: 14, pitch: 260.0, slide: 0.9, curve: 0.05, hold: 6, ..plain },
            Preset::Blip => Shape { frames: 4, pitch: 1320.0, hold: 2, ..plain },
            Preset::Siren => Shape { frames: 50, pitch: 600.0, vibrato: 4.0, vibrato_frames: 16.0, hold: 44, wave: Wave::Triangle, ..plain },
        }
    }

    /// A shape made by chance from the seed: something like one of the presets, but its own.
    pub fn random(seed: u64) -> Shape {
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..4 {
            dice.next();
        }
        let base = Preset::ALL[(dice.next() * Preset::ALL.len() as f64) as usize % Preset::ALL.len()];
        Shape::preset(base).varied(&mut dice, 1.0)
    }

    /// This shape, changed a little (by `amount`, from 0 to 1) at random.
    pub fn vary(&self, seed: u64, amount: f64) -> Shape {
        let mut dice = Dice(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        dice.next();
        self.varied(&mut dice, amount)
    }

    fn varied(&self, dice: &mut Dice, amount: f64) -> Shape {
        let mut s = *self;
        let spread = |dice: &mut Dice, value: f64, by: f64| value + by * amount * dice.between(-1.0, 1.0);
        s.frames = (spread(dice, s.frames as f64, s.frames as f64 * 0.5).round() as usize).clamp(2, 120);
        s.pitch = (s.pitch * 2f64.powf(spread(dice, 0.0, 1.0))).clamp(60.0, 4000.0);
        s.slide = spread(dice, s.slide, 1.5);
        s.curve = spread(dice, s.curve, 0.06);
        if dice.chance(0.3 * amount) {
            s.vibrato = dice.between(0.0, 3.0);
            s.vibrato_frames = dice.between(3.0, 12.0);
        }
        if dice.chance(0.3 * amount) {
            s.jump = [-12.0, -5.0, 4.0, 5.0, 7.0, 12.0][(dice.next() * 6.0) as usize % 6];
            s.jump_at = (dice.between(1.0, s.frames as f64 / 2.0)) as usize;
        }
        if dice.chance(0.15 * amount) {
            s.repeat = (dice.between(3.0, 10.0)) as usize;
        }
        s.attack = (spread(dice, s.attack as f64, 2.0).max(0.0)) as usize;
        s.hold = (spread(dice, s.hold as f64, s.frames as f64 * 0.3).max(0.0)) as usize;
        if dice.chance(0.2 * amount) {
            s.noise = Noise::ALL[(dice.next() * 4.0) as usize % 4];
        }
        s.noise_pitch = spread(dice, s.noise_pitch, 0.3).clamp(0.0, 1.0);
        s.noise_slide = spread(dice, s.noise_slide, 0.01);
        if dice.chance(0.2 * amount) {
            s.wave = Wave::ALL[(dice.next() * 4.0) as usize % 4];
        }
        s.width = spread(dice, s.width, 0.2).clamp(0.05, 0.5);
        s
    }

    /// Each frame as a sound: its note in hertz, its volume from 0 to 1, whether it's noise, and how high the
    /// noise is.
    pub fn sounds(&self) -> Vec<(f64, f64, bool, f64)> {
        let frames = self.frames.clamp(1, MOST_FRAMES);
        let fade = frames.saturating_sub(self.attack + self.hold).max(1);
        (0..frames)
            .map(|i| {
                let along = if self.repeat > 0 { i % self.repeat } else { i } as f64;
                let mut semitones = self.slide * along + self.curve * along * along / 2.0;
                if self.jump_at > 0 && i >= self.jump_at {
                    semitones += self.jump;
                }
                if self.vibrato > 0.0 {
                    semitones += self.vibrato * (std::f64::consts::TAU * i as f64 / self.vibrato_frames.max(1.0)).sin();
                }
                let hz = (self.pitch * 2f64.powf(semitones / 12.0)).clamp(20.0, 15_000.0);
                let volume = if i < self.attack {
                    (i + 1) as f64 / (self.attack + 1) as f64
                } else if i < self.attack + self.hold {
                    1.0
                } else {
                    1.0 - (i - self.attack - self.hold + 1) as f64 / (fade + 1) as f64
                };
                let noisy = match self.noise {
                    Noise::Off => false,
                    Noise::All | Noise::Mixed => true,
                    Noise::Start => i < 3,
                };
                let noise_pitch = (self.noise_pitch + self.noise_slide * i as f64).clamp(0.0, 1.0);
                (hz, volume.clamp(0.0, 1.0), noisy, noise_pitch)
            })
            .collect()
    }
}

/// The SID's attack times in milliseconds, for each of its 16 settings; decay and release take three times as long.
pub const SID_ATTACK_MS: [f64; 16] = [2.0, 8.0, 16.0, 24.0, 38.0, 56.0, 68.0, 80.0, 100.0, 250.0, 500.0, 800.0, 1000.0, 3000.0, 5000.0, 8000.0];

fn nearest_sid_rate(ms: f64, scale: f64) -> u8 {
    (0..16u8).min_by(|&a, &b| ((SID_ATTACK_MS[usize::from(a)] * scale) - ms).abs().total_cmp(&((SID_ATTACK_MS[usize::from(b)] * scale) - ms).abs())).unwrap_or(0)
}

impl Effect {
    /// An effect made from a shape, for a machine.
    pub fn from_shape(name: &str, machine: Machine, shape: &Shape) -> Effect {
        let chip = machine.chip();
        let sounds = shape.sounds();
        let ms_a_frame = 1000.0 / machine.frame_rate();
        let mut adsr = [0x00, 0xF0];
        let frames = sounds
            .iter()
            .enumerate()
            .map(|(i, &(hz, volume, noisy, noise_pitch))| {
                let mut frame = Frame::quiet(chip);
                let level = (volume * 15.0).round() as u8;
                match chip {
                    Chip::Ay => {
                        frame.tone = !(noisy && shape.noise != Noise::Mixed);
                        frame.noise = noisy;
                        frame.period = period_for(machine, hz, &frame);
                        frame.noise_period = ((1.0 - noise_pitch) * 31.0).round() as u8;
                        frame.volume = level;
                    }
                    Chip::Sid => {
                        frame.tone = i < shape.attack + shape.hold.max(1);
                        if noisy {
                            frame.wave = 0x80;
                            frame.period = (2f64.powf(9.0 + noise_pitch * 7.0)).round().min(65535.0) as u16;
                        } else {
                            frame.wave = match shape.wave {
                                Wave::Square => 0x40,
                                Wave::Triangle => 0x10,
                                Wave::Saw => 0x20,
                                Wave::Buzz => 0x60,
                            };
                            frame.period = period_for(machine, hz, &frame);
                        }
                        frame.width = (shape.width.clamp(0.0, 1.0) * 4095.0).round() as u16;
                    }
                    Chip::Tia => {
                        if noisy {
                            frame.wave = 8;
                            frame.period = ((1.0 - noise_pitch) * 31.0).round() as u16;
                        } else {
                            // The kind of wave that comes nearest the note, of those that sound like the shape's.
                            let kinds: &[u8] = if shape.wave == Wave::Buzz { &[1, 15, 2] } else { &[4, 12, 6] };
                            let mut best = (f64::MAX, 4u8, 0u16);
                            for &audc in kinds {
                                let trial = Frame { wave: audc, ..frame };
                                let audf = period_for(machine, hz, &trial);
                                let got = hz_of(machine, Frame { period: audf, ..trial });
                                let miss = (got / hz).log2().abs();
                                if miss < best.0 - 1e-9 {
                                    best = (miss, audc, audf);
                                }
                            }
                            frame.wave = best.1;
                            frame.period = best.2;
                        }
                        frame.volume = level;
                    }
                    Chip::Beeper => {
                        frame.tone = true;
                        frame.noise = noisy;
                        frame.period = if noisy { (40.0 + (1.0 - noise_pitch) * 400.0).round() as u16 } else { period_for(machine, hz, &frame) };
                        frame.volume = (volume * 30.0 * shape.width.clamp(0.0, 0.5)).round().clamp(0.0, 15.0) as u8;
                    }
                }
                frame.clamped(chip)
            })
            .collect();
        if chip == Chip::Sid {
            let attack = nearest_sid_rate(shape.attack as f64 * ms_a_frame, 1.0);
            let release = nearest_sid_rate(shape.frames.saturating_sub(shape.attack + shape.hold).max(1) as f64 * ms_a_frame, 3.0);
            adsr = [attack << 4, 0xF0 | release];
        }
        Effect { name: name.to_string(), frames, adsr }
    }

    /// The effect made for another machine: its notes kept as near as that chip can play them.
    pub fn for_machine(&self, from: Machine, to: Machine) -> Effect {
        if from.chip() == to.chip() {
            let frames = self
                .frames
                .iter()
                .map(|f| match hz(from, f) {
                    Some(h) if from.chip() != Chip::Sid || f.wave & 0x80 == 0 => Frame { period: period_for(to, h, f), ..*f }.clamped(to.chip()),
                    _ => *f,
                })
                .collect();
            return Effect { frames, ..self.clone() };
        }
        let chip = to.chip();
        let frames = self
            .frames
            .iter()
            .map(|f| {
                let note = hz(from, f);
                let volume = loudness(from.chip(), f, self.adsr);
                let mut out = Frame::quiet(chip);
                if let Some(h) = note {
                    out.tone = true;
                    out.period = period_for(to, h, &out);
                }
                match chip {
                    Chip::Ay | Chip::Tia => out.volume = (volume * 15.0).round() as u8,
                    Chip::Beeper => out.volume = (volume * 15.0).round() as u8,
                    Chip::Sid => out.tone = volume > 0.0,
                }
                out.clamped(chip)
            })
            .collect();
        Effect { frames, ..self.clone() }
    }
}

fn hz_of(machine: Machine, frame: Frame) -> f64 {
    hz(machine, &frame).unwrap_or(1.0)
}

/// How loud a frame is, from 0 to 1, for drawing: the volume, or for the SID whether its gate's on.
pub fn loudness(chip: Chip, frame: &Frame, adsr: [u8; 2]) -> f64 {
    match chip {
        Chip::Sid => {
            if frame.tone {
                f64::from(adsr[1] >> 4).max(1.0) / 15.0
            } else {
                0.0
            }
        }
        _ if !frame.sounding(chip) => 0.0,
        _ => f64::from(frame.volume) / 15.0,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// AYFX: each frame a byte (the volume in bits 0-3; bit 4 tone off, bit 5 a tone period follows, bit 6 a noise
// period follows, bit 7 noise off), the tone period (two bytes, low first) and the noise period after it when
// they're there, and $D0 $20 at the end: a noise period of $20 the chip can't have. A bank (.afb) is how many
// effects, an offset to each (from the offset's own second byte), and the effects, each followed by its name and
// a zero.

/// An effect's frames in AYFX's form, with its end.
pub fn ayfx(frames: &[Frame]) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut tone, mut noise): (Option<u16>, Option<u8>) = (None, None);
    for frame in frames {
        let f = frame.clamped(Chip::Ay);
        let mut flags = f.volume | if f.tone { 0 } else { 0x10 } | if f.noise { 0 } else { 0x80 };
        let new_tone = tone != Some(f.period);
        // As AYFX Editor writes them: a period whenever it changes, heard or not.
        let new_noise = noise != Some(f.noise_period);
        if new_tone {
            flags |= 0x20;
        }
        if new_noise {
            flags |= 0x40;
        }
        out.push(flags);
        if new_tone {
            out.extend(f.period.to_le_bytes());
            tone = Some(f.period);
        }
        if new_noise {
            out.push(f.noise_period);
            noise = Some(f.noise_period);
        }
    }
    out.extend([0xD0, 0x20]);
    out
}

/// An AYFX effect's frames, from its start, and how many bytes it took (its end included).
pub fn read_ayfx(bytes: &[u8]) -> Result<(Vec<Frame>, usize), String> {
    let mut frames = Vec::new();
    let (mut tone, mut noise) = (0u16, 0u8);
    let mut at = 0;
    loop {
        let &flags = bytes.get(at).ok_or(crate::i18n::tr("The effect stops before its end ($D0 $20)."))?;
        at += 1;
        if flags & 0x20 != 0 {
            let pair = bytes.get(at..at + 2).ok_or(crate::i18n::tr("The effect stops partway through a frame."))?;
            tone = u16::from_le_bytes([pair[0], pair[1]]) & 0xFFF;
            at += 2;
        }
        if flags & 0x40 != 0 {
            let &n = bytes.get(at).ok_or(crate::i18n::tr("The effect stops partway through a frame."))?;
            at += 1;
            if n >= 0x20 {
                return Ok((frames, at));
            }
            noise = n;
        }
        frames.push(Frame { tone: flags & 0x10 == 0, noise: flags & 0x80 == 0, period: tone, noise_period: noise, volume: flags & 15, wave: 0, width: 0 });
        if frames.len() > 10_000 {
            return Err(crate::i18n::tr("The effect goes on for more than 10,000 frames without ending.").into());
        }
    }
}

/// A bank as AYFX Editor saves it.
pub fn ayfx_bank(effects: &[Effect]) -> Result<Vec<u8>, String> {
    if effects.len() > 255 {
        return Err(crate::i18n::tr("An AYFX bank holds at most 255 effects.").into());
    }
    let mut out = vec![effects.len() as u8];
    let mut body = Vec::new();
    let table = 1 + 2 * effects.len();
    for (i, effect) in effects.iter().enumerate() {
        let offset = table + body.len() - (1 + 2 * i + 1);
        out.extend(u16::try_from(offset).map_err(|_| "The bank is more than 64K.".to_string())?.to_le_bytes());
        body.extend(ayfx(&effect.frames));
        body.extend(effect.name.bytes().filter(|b| (32..127).contains(b)));
        body.push(0);
    }
    out.extend(body);
    Ok(out)
}

/// The effects of an AYFX bank (.afb), or of one effect on its own (.afx).
pub fn read_ayfx_file(bytes: &[u8], name: &str) -> Result<Vec<Effect>, String> {
    let single = name.to_ascii_lowercase().ends_with(".afx");
    if !single && let Ok(effects) = read_ayfx_bank(bytes) {
        return Ok(effects);
    }
    let (frames, _) = read_ayfx(bytes)?;
    let stem = std::path::Path::new(name).file_stem().map_or("effect".into(), |s| s.to_string_lossy().into_owned());
    Ok(vec![Effect { name: stem, frames, adsr: [0, 0xF0] }])
}

fn read_ayfx_bank(bytes: &[u8]) -> Result<Vec<Effect>, String> {
    let &count = bytes.first().ok_or(crate::i18n::tr("The file is empty."))?;
    if count == 0 {
        return Err(crate::i18n::tr("The bank has no effects.").into());
    }
    let mut effects = Vec::new();
    for i in 0..usize::from(count) {
        let entry = 1 + 2 * i;
        let pair = bytes.get(entry..entry + 2).ok_or(crate::i18n::tr("The bank's table of effects is cut short."))?;
        let start = entry + 1 + usize::from(u16::from_le_bytes([pair[0], pair[1]]));
        let (frames, length) = read_ayfx(bytes.get(start..).ok_or(crate::i18n::tr("An effect is beyond the end of the bank."))?)?;
        // AYFX Editor keeps each effect's name after it.
        let rest = &bytes[start + length..];
        let named: String = rest.iter().take_while(|&&b| b != 0).take(64).map(|&b| b as char).collect();
        let name = if rest.contains(&0) && !named.is_empty() && named.chars().all(|c| (' '..='~').contains(&c)) { named } else { format!("effect {i}") };
        effects.push(Effect { name, frames, adsr: [0, 0xF0] });
    }
    Ok(effects)
}

// ---------------------------------------------------------------------------------------------------------------
// The other chips' data.

/// A SID effect: its attack and decay, sustain and release, then each frame a byte saying what follows (bit 0 the
/// frequency, low byte first; bit 1 the control register: the waveform and the gate; bit 2 the pulse width, low
/// byte first), and $FF at the end.
pub fn sid_data(effect: &Effect) -> Vec<u8> {
    let mut out = vec![effect.adsr[0], effect.adsr[1]];
    let mut last: Option<Frame> = None;
    for frame in &effect.frames {
        let f = frame.clamped(Chip::Sid);
        let control = f.wave | u8::from(f.tone);
        let mut flags = 0u8;
        let mut follows = Vec::new();
        if last.is_none_or(|l| l.period != f.period) {
            flags |= 1;
            follows.extend(f.period.to_le_bytes());
        }
        if last.is_none_or(|l| l.wave | u8::from(l.tone) != control) {
            flags |= 2;
            follows.push(control);
        }
        if last.is_none_or(|l| l.width != f.width) {
            flags |= 4;
            follows.extend(f.width.to_le_bytes());
        }
        out.push(flags);
        out.extend(follows);
        last = Some(f);
    }
    out.push(0xFF);
    out
}

/// The SID's voice registers after each frame of an effect, as the player leaves them: frequency (2), pulse (2),
/// control, attack and decay, sustain and release.
pub fn sid_registers(effect: &Effect) -> Vec<[u8; 7]> {
    let mut regs = [0u8, 0, 0, 0, 0x08, effect.adsr[0], effect.adsr[1]];
    let mut out = Vec::new();
    for frame in &effect.frames {
        let f = frame.clamped(Chip::Sid);
        regs[0..2].copy_from_slice(&f.period.to_le_bytes());
        regs[2..4].copy_from_slice(&f.width.to_le_bytes());
        regs[4] = f.wave | u8::from(f.tone);
        out.push(regs);
    }
    // After the end, the gate's let go of.
    regs[4] &= 0xFE;
    out.push(regs);
    out
}

/// The TIA's AUDC, AUDF and AUDV after each frame, as the player leaves them; then quiet.
pub fn tia_registers(effect: &Effect) -> Vec<[u8; 3]> {
    let mut out: Vec<[u8; 3]> = effect.frames.iter().map(|f| f.clamped(Chip::Tia)).map(|f| [f.wave, f.period as u8, f.volume]).collect();
    let last = out.last().copied().unwrap_or([0, 0, 0]);
    out.push([last[0], last[1], 0]);
    out
}

/// A beeper frame's waits: the high half's count and the low half's, and how many waves make up the frame.
pub fn beeper_counts(machine: Machine, frame: &Frame) -> (u16, u16, u16) {
    let f = frame.clamped(Chip::Beeper);
    let total = u32::from(f.period).max(2);
    let wave = BEEPER_WAVE_FIXED + BEEPER_COUNT_T * total;
    // The high half's share of the wave: 1/30 of it for each step of the width, up to a half at 15.
    let width = f64::from(f.volume.max(1)) / 30.0;
    let high_t = f64::from(wave) * width;
    let high = (((high_t - f64::from(BEEPER_HIGH_FIXED)) / f64::from(BEEPER_COUNT_T)).round() as i64).clamp(1, i64::from(total) - 1) as u32;
    let low = total - high;
    let waves = ((f64::from(machine.frame_t()) / f64::from(wave)).round() as u32).clamp(1, 65535);
    (high as u16, low as u16, waves as u16)
}

/// A beeper effect: each frame what it is (0 quiet, $10 a note, $20 noise), the high half's wait, the low half's,
/// and how many waves (each two bytes, low first); $FF at the end.
pub fn beeper_data(machine: Machine, effect: &Effect) -> Vec<u8> {
    let mut out = Vec::new();
    for frame in &effect.frames {
        let f = frame.clamped(Chip::Beeper);
        let kind = if !f.sounding(Chip::Beeper) {
            0
        } else if f.noise {
            0x20
        } else {
            0x10
        };
        let (high, low, waves) = beeper_counts(machine, &f);
        out.push(kind);
        out.extend(high.to_le_bytes());
        out.extend(low.to_le_bytes());
        out.extend(waves.to_le_bytes());
    }
    out.push(0xFF);
    out
}

/// The AY's registers after each frame of one effect played on channel A on its own, as the player leaves them:
/// R0 to R10 (tone periods, noise, mixer, volumes).
pub fn ay_registers(effect: &Effect) -> Vec<[u8; 11]> {
    let mut regs = [0u8; 11];
    regs[7] = 0x3F;
    let mut out = Vec::new();
    for frame in read_ayfx(&ayfx(&effect.frames)).map(|r| r.0).unwrap_or_default() {
        regs[0..2].copy_from_slice(&frame.period.to_le_bytes());
        regs[6] = frame.noise_period;
        regs[7] = (regs[7] & !0x09) | if frame.tone { 0 } else { 0x01 } | if frame.noise { 0 } else { 0x08 };
        regs[8] = frame.volume;
        out.push(regs);
    }
    // The end's frame: quiet, tone and noise off.
    regs[7] |= 0x09;
    regs[8] = 0;
    out.push(regs);
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Source: the data, a player, and a program to hear them.

impl Bank {
    /// Each effect's label, unique within the bank.
    pub fn labels(&self, label: &str) -> Vec<String> {
        // Not the names the players use, nor the data's end.
        let mut seen: std::collections::HashSet<String> = RESERVED.iter().map(|r| r.to_string()).collect();
        seen.insert(format!("{label}_end"));
        self.effects
            .iter()
            .map(|e| {
                let base = format!("{label}_{}", label_of(&e.name));
                let mut name = base.clone();
                let mut n = 2;
                while !seen.insert(name.clone()) {
                    name = format!("{base}{n}");
                    n += 1;
                }
                name
            })
            .collect()
    }

    /// Each effect's number's name, as an equate: SFX_LASER.
    fn numbers(&self, label: &str) -> Vec<String> {
        self.labels(label).iter().map(|l| l.to_ascii_uppercase()).collect()
    }

    /// Whether the bank can be made into source, and why not.
    pub fn check(&self) -> Result<(), String> {
        if self.effects.is_empty() {
            return Err(crate::i18n::tr("There are no effects.").into());
        }
        if let Some(e) = self.effects.iter().find(|e| e.frames.is_empty()) {
            return Err(crate::i18n::tf!("\u{201C}{}\u{201D} has no frames.", e.name));
        }
        match self.machine.chip() {
            Chip::Tia if self.effects.iter().map(|e| e.frames.len()).sum::<usize>() > 255 => Err(crate::i18n::tr("The 2600's player counts frames in a byte: all the effects together can have at most 255 frames.").into()),
            Chip::Tia | Chip::Sid | Chip::Beeper if self.effects.len() > 128 => Err(crate::i18n::tr("At most 128 effects.").into()),
            Chip::Ay if self.effects.len() > 255 => Err(crate::i18n::tr("An AYFX bank holds at most 255 effects.").into()),
            _ => Ok(()),
        }
    }

    /// The data and player as source, for the machine's usual assembler (sjasmplus for the Z80, ACME for the C64,
    /// DASM for the 2600); with `program`, a program round it that plays each effect in turn, where the machine
    /// has one that runs with F9.
    pub fn source(&self, label: &str, program: bool) -> Result<String, String> {
        self.check()?;
        let label = label_of(label);
        Ok(match self.machine.chip() {
            Chip::Ay => self.ay_source(&label, program),
            Chip::Sid => self.sid_source(&label, program),
            Chip::Tia => self.tia_source(&label, program),
            Chip::Beeper => self.beeper_source(&label, program),
        })
    }

    /// Whether there's a program to run for this machine.
    pub fn has_program(&self) -> bool {
        !matches!(self.machine, Machine::Cpc | Machine::Msx)
    }

    /// The bytes of the data alone: an AYFX bank, or each effect's data one after another.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        self.check()?;
        Ok(match self.machine.chip() {
            Chip::Ay => ayfx_bank(&self.effects)?,
            Chip::Sid => self.effects.iter().flat_map(sid_data).collect(),
            Chip::Tia => {
                let regs: Vec<Frame> = self.effects.iter().flat_map(|e| e.frames.iter().map(|f| f.clamped(Chip::Tia))).collect();
                regs.iter().map(|f| f.wave << 4 | f.volume).chain(regs.iter().map(|f| f.period as u8)).collect()
            }
            Chip::Beeper => self.effects.iter().flat_map(|e| beeper_data(self.machine, e)).collect(),
        })
    }

    fn header(&self, comment: &str, lines: &[&str]) -> String {
        let mut out = format!("{comment} Limoniter sound effects for the {} on the {}: {} effect{}.\n", self.machine.chip().name(), self.machine.name(), self.effects.len(), if self.effects.len() == 1 { "" } else { "s" });
        for line in lines {
            out.push_str(comment);
            if !line.is_empty() {
                out.push(' ');
                out.push_str(line);
            }
            out.push('\n');
        }
        out
    }

    fn ay_source(&self, label: &str, program: bool) -> String {
        let machine = self.machine;
        let spectrum = machine == Machine::Spectrum128 && program;
        let mut out = self.header(
            ";",
            &[
                "Made by Limoniter's sound effects designer. The effects are in AYFX's form, as AYFX Editor saves a bank,",
                "and play with any AYFX player; this one is Limoniter's own:",
                "  SfxInit   HL = the bank. Quietens the AY's three channels, ready for effects.",
                "  SfxPlay   A = the effect's number. Plays it on a free channel, or else on the one that's played longest.",
                "  SfxFrame  plays the next frame of each effect playing: call it once a frame, as from the interrupt.",
                "They use AF, BC, DE, HL and IX.",
            ],
        );
        if spectrum {
            out.push_str("; F9 plays each effect in turn on the built-in Spectrum 128, the border showing which.\n\n        DEVICE ZXSPECTRUM128\n");
        }
        out.push('\n');
        for (i, name) in self.numbers(label).iter().enumerate() {
            let _ = writeln!(out, "{name:<15} equ {i}");
        }
        out.push('\n');
        if spectrum {
            let lengths: Vec<String> = self.effects.iter().map(|e| (e.frames.len() + 25).min(255).to_string()).collect();
            let _ = write!(
                out,
                "        org $8000

start:  ld hl,{label}
        call SfxInit
        ei
        xor a
.effect:
        ld (demo_at),a
        push af
        and 7
        out ($FE),a             ; the border shows which
        pop af
        call SfxPlay
        ld a,(demo_at)
        ld l,a
        ld h,0
        ld de,demo_frames
        add hl,de
        ld b,(hl)               ; how long to let it play
.frame: halt
        push bc
        call SfxFrame
        pop bc
        djnz .frame
        ld a,(demo_at)
        inc a
        cp {count}
        jr c,.effect
        xor a
        jr .effect

demo_at:
        db 0
demo_frames:
        db {lengths}

",
                count = self.effects.len(),
                lengths = lengths.join(",")
            );
        }
        out.push_str(AY_PLAYER);
        out.push_str(match machine {
            Machine::Cpc => AY_OUT_CPC,
            Machine::Msx => AY_OUT_MSX,
            _ => AY_OUT_SPECTRUM,
        });
        let _ = write!(out, "\n; The bank, as AYFX Editor saves it.\n{label}:\n        db {}\n", self.effects.len());
        let labels = self.labels(label);
        for l in &labels {
            let _ = writeln!(out, "        dw {l}-$-1");
        }
        for (effect, l) in self.effects.iter().zip(&labels) {
            let _ = writeln!(out, "{l}:");
            push_bytes(&mut out, "        db ", &ayfx(&effect.frames), 16);
            let name: String = effect.name.chars().filter(|c| (' '..='~').contains(c) && *c != '"').collect();
            let _ = writeln!(out, "        db \"{name}\",0");
        }
        let _ = writeln!(out, "{label}_end:");
        out
    }

    fn sid_source(&self, label: &str, program: bool) -> String {
        let pal = self.machine == Machine::C64Pal;
        let mut out = String::new();
        if program {
            out.push_str("; machine: c64\n");
        }
        out.push_str(&self.header(
            ";",
            &[
                "Made by Limoniter's sound effects designer, in ACME's syntax, with Limoniter's own player:",
                "  sfx_init   turns the SID's volume up full.",
                "  sfx_play   A = the effect's number. Starts it on voice 3 (SFX_VOICE), taking it from whatever played there.",
                "  sfx_frame  plays the next frame: call it once a frame, after any music player, which should leave voice 3",
                "             alone while SFX_ZP+1 isn't 0.",
                "They use A and Y, and SFX_ZP and SFX_ZP+1 in zero page.",
            ],
        ));
        if program {
            let _ = writeln!(out, "; F9 plays each effect in turn in VICE, the border showing which ({}).", if pal { "PAL" } else { "NTSC" });
        }
        out.push_str("\n!cpu 6510\n\nSFX_VOICE   = $d40e             ; voice 3's registers\nSFX_ZP      = $fb               ; and $fc: where the effect's got to\n");
        for (i, name) in self.numbers(label).iter().enumerate() {
            let _ = writeln!(out, "{name:<11} = {i}");
        }
        out.push('\n');
        if program {
            let lengths: Vec<String> = self.effects.iter().map(|e| (e.frames.len() + 25).min(255).to_string()).collect();
            let _ = write!(
                out,
                "; The BASIC line: 10 SYS 2061.
        * = $0801
        !word demo_basic_end
        !word 10
        !byte $9e
        !text \"2061\"
        !byte 0
demo_basic_end
        !word 0

demo_start
        sei
        jsr sfx_init
        lda #0
        sta demo_at
demo_effect
        lda demo_at
        sta $d020               ; the border shows which
        jsr sfx_play
        ldx demo_at
        lda demo_frames,x
        sta demo_left
demo_frame
        lda $d012               ; once a frame: line 250 comes round once
        cmp #250
        bne demo_frame
demo_wait
        lda $d012
        cmp #250
        beq demo_wait
        jsr sfx_frame
        dec demo_left
        bne demo_frame
        inc demo_at
        lda demo_at
        cmp #{count}
        bcc demo_effect
        lda #0
        sta demo_at
        jmp demo_effect

demo_at     !byte 0
demo_left   !byte 0
demo_frames !byte {lengths}

",
                count = self.effects.len(),
                lengths = lengths.join(",")
            );
        }
        let _ = write!(out, "{}", SID_PLAYER.replace("LABEL", label));
        let labels = self.labels(label);
        let _ = writeln!(out, "\n; Each effect: its attack and decay, sustain and release, then each frame a byte saying what follows (bit 0 the\n; frequency, bit 1 the waveform and gate, bit 2 the pulse width), and $ff at the end.\n{label}");
        for l in &labels {
            let _ = writeln!(out, "        !word {l}");
        }
        for (effect, l) in self.effects.iter().zip(&labels) {
            let _ = writeln!(out, "{l}");
            push_bytes(&mut out, "        !byte ", &sid_data(effect), 16);
        }
        let _ = writeln!(out, "{label}_end");
        out
    }

    fn tia_source(&self, label: &str, program: bool) -> String {
        let pal = self.machine == Machine::Atari2600Pal;
        let mut out = String::new();
        if program {
            out.push_str("; machine: atari 2600\n");
        }
        out.push_str(&self.header(
            ";",
            &[
                "Made by Limoniter's sound effects designer, in DASM's syntax, with Limoniter's own player:",
                "  SfxPlay   X = the effect's number: starts it on channel SFX_CHANNEL.",
                "  SfxFrame  plays the next frame: call it once a frame, as in the overscan.",
                "They use A and Y, and two bytes of RAM, SfxAt and SfxLeft.",
            ],
        ));
        if program {
            let _ = writeln!(out, "; F9 builds the 4K cartridge and plays each effect in turn in Stella or MAME ({}), the colour showing which.", if pal { "PAL" } else { "NTSC" });
        }
        let names = self.numbers(label);
        let mut equates = String::new();
        for (i, name) in names.iter().enumerate() {
            let _ = writeln!(equates, "{name:<15} = {i}");
        }
        let (vblank, picture, overscan) = if pal { (45, 228, 36) } else { (37, 192, 30) };
        if program {
            let lengths: Vec<String> = self.effects.iter().map(|e| (e.frames.len() + 25).min(255).to_string()).collect();
            let _ = write!(
                out,
                "
        processor 6502

VSYNC   = $00
VBLANK  = $01
WSYNC   = $02
COLUBK  = $09
AUDC0   = $15
AUDF0   = $17
AUDV0   = $19
SFX_CHANNEL = 0

{equates}
        seg.u variables
        org $80
SfxAt   ds 1                ; the frame playing
SfxLeft ds 1                ; frames still to play
demoAt  ds 1                ; the effect
demoLeft ds 1               ; frames until the next

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
        ldx #0              ; the first effect
        jsr SfxPlay
        lda demoFrames
        sta demoLeft

; {total} lines a frame: 3 of sync, {vblank} of vertical blank, {picture} of picture and {overscan} of overscan. The
; work is done in the vertical blank, a line for each part, each ended with WSYNC.
Frame:  lda #2
        sta WSYNC
        sta VSYNC
        sta WSYNC
        sta WSYNC
        sta WSYNC
        lda #0
        sta VSYNC
        jsr SfxFrame        ; the effect's next frame
        sta WSYNC
        dec demoLeft        ; and after a while, the next effect
        bne Playing
        inc demoAt
        lda demoAt
        cmp #{count}
        bcc Chosen
        lda #0
        sta demoAt
Chosen: tax
        jsr SfxPlay
        ldx demoAt
        lda demoFrames,x
        sta demoLeft
Playing:
        sta WSYNC
        ldx #{vblank_less}
Blank:  sta WSYNC
        dex
        bne Blank
        lda #0
        sta VBLANK
        lda demoAt          ; the colour shows which effect
        asl
        asl
        asl
        asl
        ora #$06
        sta COLUBK
        ldx #{picture}
Picture: sta WSYNC
        dex
        bne Picture
        lda #2
        sta VBLANK
        lda #0
        sta COLUBK
        ldx #{overscan_less}
Over:   sta WSYNC
        dex
        bne Over
        jmp Frame

demoFrames:
        .byte {lengths}
",
                total = vblank + picture + overscan + 3,
                overscan_less = overscan - 1,
                vblank_less = vblank - 2,
                count = self.effects.len(),
                lengths = lengths.join(",")
            );
        } else {
            let _ = write!(out, "\nSFX_CHANNEL = 0             ; or 1\n\n{equates}\n; Two bytes of RAM, for instance:\n;         seg.u variables\n; SfxAt   ds 1\n; SfxLeft ds 1\n");
        }
        // The frames of all the effects, one after another; an effect is where it starts and how long it is.
        let mut starts = Vec::new();
        let mut at = 0usize;
        for effect in &self.effects {
            starts.push(at);
            at += effect.frames.len();
        }
        let frames: Vec<Frame> = self.effects.iter().flat_map(|e| e.frames.iter().map(|f| f.clamped(Chip::Tia))).collect();
        let _ = write!(
            out,
            "
SfxPlay:
        lda {label}Start,x
        sta SfxAt
        lda {label}Length,x
        sta SfxLeft
        rts

SfxFrame:
        lda SfxLeft
        beq SfxQuiet
        dec SfxLeft
        ldy SfxAt
        inc SfxAt
        lda {label}F,y
        sta AUDF0+SFX_CHANNEL
        lda {label}CV,y         ; AUDC in the top four bits, AUDV in the bottom
        sta AUDV0+SFX_CHANNEL
        lsr
        lsr
        lsr
        lsr
        sta AUDC0+SFX_CHANNEL
        rts
SfxQuiet:
        sta AUDV0+SFX_CHANNEL   ; A is 0
        rts

; Where each effect starts among the frames, and how many it has.
{label}:
{label}Start:
"
        );
        push_bytes(&mut out, "        .byte ", &starts.iter().map(|&s| s as u8).collect::<Vec<_>>(), 16);
        let _ = writeln!(out, "{label}Length:");
        push_bytes(&mut out, "        .byte ", &self.effects.iter().map(|e| e.frames.len() as u8).collect::<Vec<_>>(), 16);
        let _ = writeln!(out, "; Each frame's AUDC and AUDV.\n{label}CV:");
        for (effect, start) in self.effects.iter().zip(&starts) {
            let _ = writeln!(out, "; {}", effect.name);
            push_bytes(&mut out, "        .byte ", &frames[*start..start + effect.frames.len()].iter().map(|f| f.wave << 4 | f.volume).collect::<Vec<_>>(), 16);
        }
        let _ = writeln!(out, "; Each frame's AUDF.\n{label}F:");
        for (effect, start) in self.effects.iter().zip(&starts) {
            let _ = writeln!(out, "; {}", effect.name);
            push_bytes(&mut out, "        .byte ", &frames[*start..start + effect.frames.len()].iter().map(|f| f.period as u8).collect::<Vec<_>>(), 16);
        }
        let _ = writeln!(out, "{label}_end:");
        if program {
            out.push_str("\n        org $FFFC\n        .word Reset\n        .word Reset\n");
        }
        out
    }

    fn beeper_source(&self, label: &str, program: bool) -> String {
        let mut out = self.header(
            ";",
            &[
                "Made by Limoniter's sound effects designer, with Limoniter's own player:",
                "  SfxPlay   A = the effect's number. Plays it, and returns when it's over: nothing else runs while it plays,",
                "            and interrupts are off until it's done. It uses AF, BC, DE and HL.",
                "  SfxBeep   the same, for the effect HL points to.",
            ],
        );
        if program {
            let _ = writeln!(out, "; F9 plays each effect in turn on the built-in Spectrum, the border showing which.");
            if self.machine == Machine::Spectrum128Beeper {
                out.push_str("\n        DEVICE ZXSPECTRUM128\n");
            }
        }
        out.push_str("\nSFX_BORDER      equ 0           ; the border's colour, which the same port sets\n");
        for (i, name) in self.numbers(label).iter().enumerate() {
            let _ = writeln!(out, "{name:<15} equ {i}");
        }
        out.push('\n');
        if program {
            let _ = write!(
                out,
                "        org $8000

start:  xor a
.effect:
        ld (demo_at),a
        call SfxPlay
        ei
        ld b,25                 ; a moment between them
.pause: halt
        djnz .pause
        ld a,(demo_at)
        inc a
        cp {count}
        jr c,.effect
        xor a
        jr .effect

demo_at:
        db 0

",
                count = self.effects.len()
            );
        }
        out.push_str(&BEEPER_PLAYER.replace("LABEL", label));
        let labels = self.labels(label);
        let _ = writeln!(
            out,
            "\n; Each effect: each frame what it is (0 quiet, $10 a note, $20 noise), the high half's wait and the low half's, in\n; counts of 26 T-states, and how many waves make the frame; $FF at the end.\n{label}:"
        );
        for l in &labels {
            let _ = writeln!(out, "        dw {l}");
        }
        for (effect, l) in self.effects.iter().zip(&labels) {
            let _ = writeln!(out, "{l}:");
            push_bytes(&mut out, "        db ", &beeper_data(self.machine, effect), 14);
        }
        let _ = writeln!(out, "{label}_end:");
        out
    }
}

/// Names the players use, which an effect's label mustn't be (in any case).
const RESERVED: [&str; 14] = ["sfx_init", "sfx_play", "sfx_frame", "sfx_flags", "sfx_control", "sfx_same_frequency", "sfx_same_control", "sfx_moved", "sfx_done", "sfx_over", "sfx_voice", "sfx_zp", "sfx_border", "sfx_channel"];

/// Bytes as lines of a directive, so many a line.
fn push_bytes(out: &mut String, directive: &str, bytes: &[u8], per: usize) {
    for chunk in bytes.chunks(per.max(1)) {
        out.push_str(directive);
        let parts: Vec<String> = chunk.iter().map(|b| format!("${b:02X}")).collect();
        out.push_str(&parts.join(","));
        out.push('\n');
    }
}

const AY_PLAYER: &str = "\
SfxInit:
        inc hl
        ld (SfxBank),hl         ; the bank's table of offsets
        xor a
        ld hl,SfxChannels
        ld b,12
.clear: ld (hl),a               ; every channel free
        inc hl
        djnz .clear
        ld a,$3F
        ld (SfxMixer),a
        ld e,a
        ld a,7
        call SfxOut             ; tone and noise off on all three
        ld e,0
        ld a,8
        call SfxOut             ; and their volumes down
        inc a
        call SfxOut
        inc a
        jp SfxOut

SfxPlay:
        ld l,a
        ld h,0
        add hl,hl
        ld de,(SfxBank)
        add hl,de               ; the effect's offset
        ld e,(hl)
        inc hl
        ld d,(hl)
        add hl,de               ; its address: the offset counts from the offset's own second byte
        ld (SfxNew),hl
        ld ix,SfxChannels       ; a free channel, or else the one that's played longest
        ld hl,SfxChannels
        ld (SfxTake),hl
        ld de,0                 ; the longest time yet
        ld b,3
.look:  ld a,(ix+0)
        or (ix+1)
        jr z,.take              ; free
        ld a,(ix+3)
        cp d
        jr c,.short
        jr nz,.longer
        ld a,(ix+2)
        cp e
        jr c,.short
.longer:
        ld e,(ix+2)
        ld d,(ix+3)
        ld (SfxTake),ix
.short: inc ix
        inc ix
        inc ix
        inc ix
        djnz .look
        ld ix,(SfxTake)
.take:  ld hl,(SfxNew)
        ld (ix+0),l
        ld (ix+1),h
        xor a
        ld (ix+2),a
        ld (ix+3),a
        ret

SfxFrame:
        ld ix,SfxChannels
        ld c,0                  ; the channel: 0, 1, 2
.channel:
        ld l,(ix+0)
        ld h,(ix+1)
        ld a,h
        or l
        jp z,.next              ; nothing playing on it
        inc (ix+2)
        jr nz,.timed
        inc (ix+3)
.timed: ld b,(hl)               ; the frame's flags
        inc hl
        ld a,b
        and 15
        ld e,a
        ld a,c
        add a,8
        call SfxOut             ; the volume
        ld a,b
        and $90                 ; tone off (bit 4) and noise off (bit 7)
        rrca
        rrca
        rrca
        rrca                    ; to bits 0 and 3
        ld d,a
        ld e,%00001001          ; the channel's two bits in the mixer
        ld a,c
        or a
        jr z,.placed
.shift: sla d
        sla e
        dec a
        jr nz,.shift
.placed:
        ld a,e
        cpl
        push hl
        ld hl,SfxMixer
        and (hl)
        or d
        ld (hl),a
        pop hl
        ld e,a
        ld a,7
        call SfxOut             ; the mixer
        bit 5,b
        jr z,.same_tone
        ld e,(hl)
        inc hl
        ld a,c
        add a,a
        call SfxOut             ; the tone period's low byte
        ld e,(hl)
        inc hl
        inc a
        call SfxOut             ; and high
.same_tone:
        bit 6,b
        jr z,.same_noise
        ld a,(hl)
        inc hl
        cp $20
        jr nc,.over             ; a noise period of $20 or more: the end
        ld e,a
        ld a,6
        call SfxOut
.same_noise:
        ld (ix+0),l
        ld (ix+1),h
        jr .next
.over:  xor a
        ld (ix+0),a
        ld (ix+1),a
.next:  ld de,4
        add ix,de
        inc c
        ld a,c
        cp 3
        jp nz,.channel
        ret

SfxBank:
        dw 0
SfxNew: dw 0
SfxTake:
        dw 0
SfxMixer:
        db $3F
SfxChannels:
        ds 12                   ; each channel: where its effect has got to (0 when free), and how long it's played
";

const AY_OUT_SPECTRUM: &str = "
; Register A of the AY gets E, on the Spectrum 128.
SfxOut: push bc
        ld bc,$FFFD
        out (c),a
        ld b,$BF
        out (c),e
        pop bc
        ret
";

const AY_OUT_MSX: &str = "
; Register A of the AY gets E, on the MSX.
SfxOut: out ($A0),a
        push af
        ld a,e
        out ($A1),a
        pop af
        ret
";

const AY_OUT_CPC: &str = "
; Register A of the AY gets E, on the CPC, through the PPI.
SfxOut: push bc
        ld b,$F4
        out (c),a               ; the register's number
        ld bc,$F6C0
        out (c),c               ; chosen
        ld bc,$F600
        out (c),c
        ld b,$F4
        out (c),e               ; the value
        ld bc,$F680
        out (c),c               ; written
        ld bc,$F600
        out (c),c
        pop bc
        ret
";

const SID_PLAYER: &str = "\
sfx_init
        lda #$0f
        sta $d418
        lda #0
        sta SFX_ZP+1            ; nothing playing
        rts

sfx_play
        asl
        tay
        lda LABEL,y
        sta SFX_ZP
        lda LABEL+1,y
        sta SFX_ZP+1
        lda #$08                ; the test bit: the oscillator from the start, and the gate off
        sta SFX_VOICE+4
        ldy #0
        lda (SFX_ZP),y          ; attack and decay
        sta SFX_VOICE+5
        iny
        lda (SFX_ZP),y          ; sustain and release
        sta SFX_VOICE+6
        iny
        jmp sfx_moved

sfx_frame
        lda SFX_ZP+1
        beq sfx_done            ; nothing playing
        ldy #0
        lda (SFX_ZP),y          ; what this frame changes
        cmp #$ff
        beq sfx_over
        sta sfx_flags
        iny
        lsr sfx_flags
        bcc sfx_same_frequency
        lda (SFX_ZP),y
        sta SFX_VOICE
        iny
        lda (SFX_ZP),y
        sta SFX_VOICE+1
        iny
sfx_same_frequency
        lsr sfx_flags
        bcc sfx_same_control
        lda (SFX_ZP),y
        sta SFX_VOICE+4
        sta sfx_control
        iny
sfx_same_control
        lsr sfx_flags
        bcc sfx_moved
        lda (SFX_ZP),y
        sta SFX_VOICE+2
        iny
        lda (SFX_ZP),y
        sta SFX_VOICE+3
        iny
sfx_moved
        tya
        clc
        adc SFX_ZP
        sta SFX_ZP
        bcc sfx_done
        inc SFX_ZP+1
sfx_done
        rts
sfx_over
        lda sfx_control
        and #$fe                ; the gate let go of: the sound fades as it releases
        sta SFX_VOICE+4
        lda #0
        sta SFX_ZP+1
        rts

sfx_flags   !byte 0
sfx_control !byte 0
";

const BEEPER_PLAYER: &str = "\
SfxPlay:
        add a,a
        ld l,a
        ld h,0
        ld de,LABEL
        add hl,de
        ld a,(hl)
        inc hl
        ld h,(hl)
        ld l,a
SfxBeep:
        di
        push ix
        push hl
        pop ix
        ld hl,(SfxSeed)         ; the noise's shift register
.frame: ld a,(ix+0)             ; what the frame is: 0 quiet, $10 a note, $20 noise, $FF the end
        cp $FF
        jr z,.done
        ld c,(ix+5)
        ld b,(ix+6)             ; how many waves
.wave:  ld a,(ix+0)             ; the first half: high for a note
        call .half
        ld e,(ix+1)
        ld d,(ix+2)
        call .wait
        ld a,(ix+0)             ; the second half: low for a note
        and $20
        call .half
        ld e,(ix+3)
        ld d,(ix+4)
        call .wait
        dec bc
        ld a,b
        or c
        jp nz,.wave
        ld de,7
        add ix,de
        jr .frame
.done:  ld (SfxSeed),hl
        pop ix
        ei
        ret

; The speaker for half a wave: A is $10 for high, $20 for a random level, 0 for low. Always 89 T-states.
.half:  ld e,a
        add hl,hl               ; the noise's next bit, into carry
        sbc a,a
        ld d,a
        and $2D
        xor l
        ld l,a
        ld a,e
        rrca                    ; noise's $20 to $10
        and d                   ; kept when the bit was set
        or e
        and $10
        or SFX_BORDER
        out ($FE),a
        ret

; 26 T-states for each count in DE, and 5 more.
.wait:  dec de
        ld a,d
        or e
        jr nz,.wait
        ret

SfxSeed:
        dw $ACE1
";

/// Reads effects back from source this wrote (the data's label to its end label, assembled), or from the bytes of
/// an AYFX bank. The machine is read from the source's first line.
pub fn from_source(text: &str, fallback: Machine) -> Result<Bank, String> {
    let machine = Machine::ALL.into_iter().find(|m| text.lines().take(5).any(|l| l.contains("Limoniter sound effects") && l.contains(&format!("on the {}:", m.name())))).unwrap_or(fallback);
    let chip = machine.chip();
    let cpu = if chip == Chip::Ay || chip == Chip::Beeper { crate::assemblers::Cpu::Z80 } else { crate::assemblers::Cpu::M6502 };
    // The data alone, which assembles without the rest.
    let assembly = crate::assemblers::assemble_own_at(cpu, std::path::Path::new("effects.asm"), text, 0);
    let ends: Vec<(&String, &i64)> = assembly.symbols.iter().filter(|(name, _)| name.ends_with("_end")).collect();
    for (end_name, &end) in ends {
        let label = &end_name[..end_name.len() - 4];
        let Some(&start) = assembly.symbols.get(label) else { continue };
        if start >= end || end - start > 0x10000 {
            continue;
        }
        let bytes = assembly.image(start, end);
        let names = effect_names(text, label);
        let effects = match chip {
            Chip::Ay => read_ayfx_bank(&bytes)?,
            Chip::Sid | Chip::Beeper => {
                let mut effects = Vec::new();
                for name in &names {
                    let Some(&at) = assembly.symbols.get(&format!("{label}_{}", label_of(name))) else { break };
                    let from = usize::try_from(at - start).map_err(|_| "An effect is before the data's label.".to_string())?;
                    let data = bytes.get(from..).ok_or(crate::i18n::tr("An effect is past the data's end."))?;
                    effects.push(if chip == Chip::Sid { read_sid(name, data)? } else { read_beeper(name, data)? });
                }
                effects
            }
            Chip::Tia => {
                let at = |suffix: &str| assembly.symbols.get(&format!("{label}{suffix}")).copied();
                let (Some(starts), Some(lengths), Some(cv), Some(f)) = (at("Start"), at("Length"), at("CV"), at("F")) else { continue };
                let read = |a: i64, n: usize| assembly.image(a, a + n as i64);
                let count = names.len();
                let starts = read(starts, count);
                let lengths = read(lengths, count);
                let total = lengths.iter().map(|&l| usize::from(l)).sum::<usize>().max(1);
                let cv = read(cv, total);
                let f = read(f, total);
                names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let from = usize::from(starts[i]);
                        let frames = (from..from + usize::from(lengths[i])).filter(|&j| j < cv.len()).map(|j| Frame { tone: false, noise: false, period: u16::from(f[j] & 31), noise_period: 0, volume: cv[j] & 15, wave: cv[j] >> 4, width: 0 }).collect();
                        Effect { name: name.clone(), frames, adsr: [0, 0xF0] }
                    })
                    .collect()
            }
        };
        if !effects.is_empty() {
            return Ok(Bank { machine, effects });
        }
    }
    Err(crate::i18n::tr("There aren't sound effects here that Limoniter can read: it reads back the source its sound effects designer writes.").into())
}

/// The effects' names, from the equates that number them (`SFX_LASER equ 0`), in order; the comment after a 2600's
/// table names them too, but the equates are the same.
fn effect_names(text: &str, label: &str) -> Vec<String> {
    let prefix = format!("{}_", label.to_ascii_uppercase());
    let mut named: Vec<(usize, String)> = text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let rest: Vec<&str> = words.collect();
            let value = match rest.as_slice() {
                [op, value, ..] if op.eq_ignore_ascii_case("equ") || *op == "=" => *value,
                _ => return None,
            };
            let number = value.parse::<usize>().ok()?;
            // Not the players' own equates, SFX_CHANNEL and the rest, which the effects' never are.
            name.strip_prefix(&prefix).filter(|_| !RESERVED.contains(&name.to_ascii_lowercase().as_str())).map(|n| (number, n.to_ascii_lowercase()))
        })
        .collect();
    named.sort();
    named.dedup_by_key(|n| n.0);
    named.into_iter().map(|(_, n)| n).collect()
}

fn read_sid(name: &str, data: &[u8]) -> Result<Effect, String> {
    let adsr = [*data.first().ok_or(crate::i18n::tr("An effect is cut short."))?, *data.get(1).ok_or(crate::i18n::tr("An effect is cut short."))?];
    let mut at = 2;
    let mut frame = Frame::quiet(Chip::Sid);
    let mut frames = Vec::new();
    loop {
        let &flags = data.get(at).ok_or(crate::i18n::tr("An effect has no end."))?;
        at += 1;
        if flags == 0xFF {
            break;
        }
        let mut take = |n: usize| -> Result<&[u8], String> {
            let part = data.get(at..at + n).ok_or(crate::i18n::tr("An effect is cut short."))?;
            at += n;
            Ok(part)
        };
        if flags & 1 != 0 {
            let p = take(2)?;
            frame.period = u16::from_le_bytes([p[0], p[1]]);
        }
        if flags & 2 != 0 {
            let c = take(1)?[0];
            frame.wave = c & 0xFE;
            frame.tone = c & 1 != 0;
        }
        if flags & 4 != 0 {
            let p = take(2)?;
            frame.width = u16::from_le_bytes([p[0], p[1]]) & 0xFFF;
        }
        frames.push(frame);
        if frames.len() > MOST_FRAMES {
            return Err(crate::i18n::tr("An effect goes on too long.").into());
        }
    }
    Ok(Effect { name: name.to_string(), frames, adsr })
}

fn read_beeper(name: &str, data: &[u8]) -> Result<Effect, String> {
    let mut frames = Vec::new();
    let mut at = 0;
    loop {
        let &kind = data.get(at).ok_or(crate::i18n::tr("An effect has no end."))?;
        if kind == 0xFF {
            break;
        }
        let part = data.get(at + 1..at + 7).ok_or(crate::i18n::tr("An effect is cut short."))?;
        let high = u16::from_le_bytes([part[0], part[1]]);
        let low = u16::from_le_bytes([part[2], part[3]]);
        at += 7;
        let period = high.saturating_add(low);
        let wave = f64::from(BEEPER_WAVE_FIXED + BEEPER_COUNT_T * u32::from(period));
        let width = ((f64::from(BEEPER_HIGH_FIXED + BEEPER_COUNT_T * u32::from(high)) / wave) * 30.0).round().clamp(1.0, 15.0) as u8;
        frames.push(Frame { tone: kind != 0, noise: kind == 0x20, period, noise_period: 0, volume: if kind == 0 { 0 } else { width }, wave: 0, width: 0 });
        if frames.len() > MOST_FRAMES {
            return Err(crate::i18n::tr("An effect goes on too long.").into());
        }
    }
    Ok(Effect { name: name.to_string(), frames, adsr: [0, 0xF0] })
}
