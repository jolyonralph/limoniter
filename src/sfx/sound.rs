//! What an effect sounds like: the AY through the same emulation the built-in Spectrum uses, the SID and the TIA
//! modelled here as the chips behave, and the beeper by running the player itself on a Z80, so what's heard is
//! what the player does.

use super::{Bank, Chip, Effect, Machine};

use super::z80;

/// An effect's sound, at `rate` samples a second, mono, between -1 and 1.
pub fn render(machine: Machine, effect: &Effect, rate: u32) -> Vec<f32> {
    let rate = rate.max(8000);
    let samples = match machine.chip() {
        Chip::Ay => ay(machine, effect, rate),
        Chip::Sid => sid(machine, effect, rate),
        Chip::Tia => tia(machine, effect, rate),
        Chip::Beeper => beeper(machine, effect, rate),
    };
    dc_blocked(samples)
}

/// Each frame's samples: how many a frame has, with the remainders carried so the whole is the right length.
fn frame_lengths(machine: Machine, frames: usize, rate: u32) -> Vec<usize> {
    let per = f64::from(rate) / machine.frame_rate();
    (0..frames).map(|i| ((i + 1) as f64 * per).round() as usize - (i as f64 * per).round() as usize).collect()
}

/// The sound with its middle at 0, as a speaker's capacitor leaves it.
fn dc_blocked(samples: Vec<f32>) -> Vec<f32> {
    let mut last_in = samples.first().copied().unwrap_or(0.0);
    let mut last_out = 0.0f32;
    samples
        .into_iter()
        .map(|x| {
            last_out = x - last_in + 0.995 * last_out;
            last_in = x;
            last_out
        })
        .collect()
}

fn ay(machine: Machine, effect: &Effect, rate: u32) -> Vec<f32> {
    use aym::AymBackend;
    let mut chip = aym::AymPrecise::new(aym::SoundChip::AY, aym::AyMode::Mono, machine.clock() as usize, rate as usize);
    let registers = super::ay_registers(effect);
    let mut out = Vec::new();
    for (regs, length) in registers.iter().zip(frame_lengths(machine, registers.len(), rate)) {
        for (r, &value) in regs.iter().enumerate() {
            chip.write_register(r as u8, value);
        }
        // AY's volume at 15 is about a third of the way up, with three channels to come.
        out.extend((0..length).map(|_| (chip.next_sample().left * 1.6) as f32));
    }
    out
}

// The SID: a voice's oscillator, its waveforms, and its envelope, a cycle at a time.

/// Cycles for each step of the envelope at each of the 16 rates.
const RATE_PERIODS: [u32; 16] = [9, 32, 63, 95, 149, 220, 267, 313, 392, 977, 1954, 3126, 3907, 11720, 19532, 31251];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Attack,
    Decay,
    Release,
}

pub struct SidVoice {
    pub regs: [u8; 7],
    accumulator: u32,
    noise: u32,
    envelope: u8,
    stage: Stage,
    rate_counter: u32,
    exp_counter: u32,
    gate: bool,
}

impl Default for SidVoice {
    fn default() -> SidVoice {
        SidVoice { regs: [0; 7], accumulator: 0, noise: 0x7F_FFF8, envelope: 0, stage: Stage::Release, rate_counter: 0, exp_counter: 0, gate: false }
    }
}

impl SidVoice {
    /// A register written: 0 and 1 the frequency, 2 and 3 the pulse width, 4 control, 5 and 6 the envelope.
    pub fn write(&mut self, register: usize, value: u8) {
        self.regs[register] = value;
        if register == 4 {
            let gate = value & 1 != 0;
            if gate && !self.gate {
                self.stage = Stage::Attack;
            } else if !gate && self.gate {
                self.stage = Stage::Release;
            }
            self.gate = gate;
        }
    }

    #[cfg(test)]
    pub fn envelope(&self) -> u8 {
        self.envelope
    }

    /// One cycle; what the voice puts out, from -1 to 1.
    pub fn clock(&mut self) -> f32 {
        let control = self.regs[4];
        let frequency = u32::from(u16::from_le_bytes([self.regs[0], self.regs[1]]));
        if control & 8 != 0 {
            self.accumulator = 0;
            self.noise = 0x7F_FFF8;
        } else {
            let before = self.accumulator;
            self.accumulator = (self.accumulator + frequency) & 0xFF_FFFF;
            if before & 0x08_0000 == 0 && self.accumulator & 0x08_0000 != 0 {
                let bit = ((self.noise >> 22) ^ (self.noise >> 17)) & 1;
                self.noise = ((self.noise << 1) | bit) & 0x7F_FFFF;
            }
        }
        self.clock_envelope();
        self.waveform()
    }

    /// One cycle of the envelope.
    pub fn clock_envelope(&mut self) {
        let rate = usize::from(match self.stage {
            Stage::Attack => self.regs[5] >> 4,
            Stage::Decay => self.regs[5] & 15,
            Stage::Release => self.regs[6] & 15,
        });
        self.rate_counter += 1;
        if self.rate_counter >= RATE_PERIODS[rate] {
            self.rate_counter = 0;
            let exp_period = match self.envelope {
                94..=255 => 1,
                55..=93 => 2,
                27..=54 => 4,
                15..=26 => 8,
                7..=14 => 16,
                _ => 30,
            };
            self.exp_counter += 1;
            if self.stage == Stage::Attack || self.exp_counter >= exp_period {
                self.exp_counter = 0;
                match self.stage {
                    Stage::Attack => {
                        self.envelope = self.envelope.saturating_add(1);
                        if self.envelope == 255 {
                            self.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        if self.envelope > (self.regs[6] >> 4) * 17 {
                            self.envelope -= 1;
                        }
                    }
                    Stage::Release => self.envelope = self.envelope.saturating_sub(1),
                }
            }
        }
    }

    /// What the voice puts out now, from -1 to 1: its waveforms, ANDed where more than one is chosen, at the
    /// envelope's level.
    fn waveform(&self) -> f32 {
        let control = self.regs[4];
        let acc = self.accumulator;
        let mut wave = 0xFFFu32;
        let mut any = false;
        if control & 0x10 != 0 {
            let folded = if acc & 0x80_0000 != 0 { !acc } else { acc };
            wave &= (folded >> 11) & 0xFFF;
            any = true;
        }
        if control & 0x20 != 0 {
            wave &= acc >> 12;
            any = true;
        }
        if control & 0x40 != 0 {
            let width = u32::from(u16::from_le_bytes([self.regs[2], self.regs[3] & 15]));
            wave &= if (acc >> 12) >= width { 0xFFF } else { 0 };
            any = true;
        }
        if control & 0x80 != 0 {
            let n = self.noise;
            let bits = ((n >> 20) & 1) << 11 | ((n >> 18) & 1) << 10 | ((n >> 14) & 1) << 9 | ((n >> 11) & 1) << 8 | ((n >> 9) & 1) << 7 | ((n >> 5) & 1) << 6 | ((n >> 2) & 1) << 5 | (n & 1) << 4;
            wave &= bits;
            any = true;
        }
        if !any {
            return 0.0;
        }
        (wave as f32 - 2048.0) / 2048.0 * f32::from(self.envelope) / 255.0
    }
}

/// How long the SID's release takes, for a setting, in seconds.
fn release_seconds(release: u8) -> f64 {
    super::SID_ATTACK_MS[usize::from(release & 15)] * 3.0 / 1000.0
}

fn sid(machine: Machine, effect: &Effect, rate: u32) -> Vec<f32> {
    let mut voice = SidVoice::default();
    voice.write(4, 0x08);
    voice.write(5, effect.adsr[0]);
    voice.write(6, effect.adsr[1]);
    let registers = super::sid_registers(effect);
    let mut lengths = frame_lengths(machine, registers.len(), rate);
    // The release heard to its end, up to two seconds.
    if let Some(last) = lengths.last_mut() {
        *last = (f64::from(rate) * release_seconds(effect.adsr[1]).min(2.0)) as usize + 1;
    }
    let cycles_a_sample = machine.clock() / f64::from(rate);
    let mut owed = 0.0;
    let mut out = Vec::new();
    for (regs, length) in registers.iter().zip(lengths) {
        for (r, &value) in regs.iter().enumerate() {
            voice.write(r, value);
        }
        for _ in 0..length {
            owed += cycles_a_sample;
            let cycles = owed.floor();
            owed -= cycles;
            let mut sum = 0.0;
            for _ in 0..cycles as u32 {
                sum += voice.clock();
            }
            out.push(0.7 * sum / cycles.max(1.0) as f32);
        }
    }
    out
}

/// The SID's envelope at the end of each frame of an effect, from 0 to 1.
pub fn sid_levels(machine: Machine, effect: &Effect) -> Vec<f32> {
    let mut voice = SidVoice::default();
    voice.write(5, effect.adsr[0]);
    voice.write(6, effect.adsr[1]);
    let cycles = (machine.clock() / machine.frame_rate()) as u32;
    effect
        .frames
        .iter()
        .map(|f| {
            voice.write(4, u8::from(f.tone));
            for _ in 0..cycles {
                voice.clock_envelope();
            }
            f32::from(voice.envelope) / 255.0
        })
        .collect()
}

// The TIA's sound, a tick at a time (the colour clock over 114).

/// A shift register's bits, from all ones, as the TIA's 4-, 5- and 9-bit polynomial counters go round.
fn polynomial(size: u32, tap: u32) -> Vec<u8> {
    let mask = (1u32 << size) - 1;
    let mut x = mask;
    (0..mask)
        .map(|_| {
            let bit = x & 1;
            let feedback = (x ^ (x >> (size - tap))) & 1;
            x = (x >> 1) | (feedback << (size - 1));
            bit as u8
        })
        .collect()
}

pub struct TiaChannel {
    audc: u8,
    audf: u8,
    audv: u8,
    divide: u16,
    count: u16,
    p4: usize,
    p5: usize,
    p9: usize,
    div3: u8,
    pub level: u8,
    poly4: Vec<u8>,
    poly5: Vec<u8>,
    poly9: Vec<u8>,
}

/// Of every 31 of the 5-bit counter's steps, the two that clock the "divide by 31" sounds: 13 and 18 apart.
const DIV31: [u8; 31] = [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

impl Default for TiaChannel {
    fn default() -> TiaChannel {
        TiaChannel { audc: 0, audf: 0, audv: 0, divide: 0, count: 0, p4: 0, p5: 0, p9: 0, div3: 3, level: 0, poly4: polynomial(4, 3), poly5: polynomial(5, 3), poly9: polynomial(9, 5) }
    }
}

impl TiaChannel {
    pub fn set(&mut self, audc: u8, audf: u8, audv: u8) {
        self.audc = audc & 15;
        self.audf = audf & 31;
        self.audv = audv & 15;
        let divide = if self.audc == 0 || self.audc == 11 {
            self.level = self.audv;
            0
        } else {
            let n = u16::from(self.audf) + 1;
            if self.audc & 12 == 12 && self.audc != 15 { n * 3 } else { n }
        };
        if divide != self.divide {
            self.divide = divide;
            if self.count == 0 || divide == 0 {
                self.count = divide;
            }
        }
    }

    /// One tick.
    pub fn tick(&mut self) {
        if self.count > 1 {
            self.count -= 1;
            return;
        }
        if self.count == 0 {
            return;
        }
        self.count = self.divide;
        let before = self.poly5[self.p5];
        self.p5 = (self.p5 + 1) % 31;
        let now = self.poly5[self.p5];
        let c = self.audc;
        let clocked = c & 2 == 0 || (c & 1 == 0 && DIV31[self.p5] != 0) || (c & 1 == 1 && now != 0) || (c == 15 && now != before);
        if !clocked {
            return;
        }
        let on = |level: u8, audv: u8| if level != 0 { 0 } else { audv };
        if c & 4 != 0 {
            if c == 15 {
                if now != before {
                    self.div3 -= 1;
                    if self.div3 == 0 {
                        self.div3 = 3;
                        self.level = on(self.level, self.audv);
                    }
                }
            } else {
                self.level = on(self.level, self.audv);
            }
        } else if c & 8 != 0 {
            if c == 8 {
                self.p9 = (self.p9 + 1) % 511;
                self.level = if self.poly9[self.p9] != 0 { self.audv } else { 0 };
            } else if c & 2 != 0 {
                self.level = if self.level != 0 || c & 1 != 0 { 0 } else { self.audv };
            } else {
                self.level = if now != 0 { self.audv } else { 0 };
            }
        } else {
            self.p4 = (self.p4 + 1) % 15;
            self.level = if self.poly4[self.p4] != 0 { self.audv } else { 0 };
        }
    }
}

fn tia(machine: Machine, effect: &Effect, rate: u32) -> Vec<f32> {
    let mut channel = TiaChannel::default();
    let registers = super::tia_registers(effect);
    let ticks_a_sample = machine.clock() / f64::from(rate);
    let mut owed = 0.0;
    let mut out = Vec::new();
    for (regs, length) in registers.iter().zip(frame_lengths(machine, registers.len(), rate)) {
        channel.set(regs[0], regs[1], regs[2]);
        for _ in 0..length {
            owed += ticks_a_sample;
            while owed >= 1.0 {
                channel.tick();
                owed -= 1.0;
            }
            out.push(f32::from(channel.level) / 15.0 * 0.8);
        }
    }
    out
}

// The beeper: the player, assembled, run on a Z80, and its speaker's level followed.

/// The player and one effect, assembled at $8000, and where its SfxPlay is; or why not.
pub fn beeper_program(machine: Machine, effect: &Effect) -> Result<(Vec<u8>, u16), String> {
    let bank = Bank { machine, effects: vec![effect.clone()] };
    let source = format!("        org $8000\n{}", bank.source("sfx", false)?);
    let assembly = crate::assemblers::assemble_own_at(crate::assemblers::Cpu::Z80, std::path::Path::new("sfx.asm"), &source, 0);
    if let Some(problem) = assembly.problems.first() {
        return Err(crate::i18n::tf!("The beeper's player didn't assemble: {}", problem.message));
    }
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("The beeper's player didn't assemble.").into());
    };
    let play = *assembly.symbols.get("SfxPlay").ok_or(crate::i18n::tr("The beeper's player has no SfxPlay."))?;
    if low != 0x8000 {
        return Err(crate::i18n::tr("The beeper's player isn't at $8000.").into());
    }
    Ok((assembly.image(low, high), play as u16))
}

/// The beeper effect played on a Z80: each change of the speaker, as (T-state, high), and the T-states it took.
pub fn beeper_run(machine: Machine, effect: &Effect) -> Result<(Vec<(u64, bool)>, u64), String> {
    let (code, play) = beeper_program(machine, effect)?;
    let mut z80 = z80::Machine::with(&code, 0x8000);
    let most = (effect.frames.len() as u64 + 4) * u64::from(machine.frame_t()) * 3;
    if !z80.call(play, 0, 0, most) {
        return Err(crate::i18n::tr("The beeper's player didn't finish.").into());
    }
    let changes = z80.bus.outs.iter().filter(|(_, port, _)| port & 1 == 0).map(|&(t, _, value)| (t, value & 0x10 != 0)).collect();
    Ok((changes, z80.bus.t))
}

fn beeper(machine: Machine, effect: &Effect, rate: u32) -> Vec<f32> {
    let Ok((changes, length)) = beeper_run(machine, effect) else {
        return Vec::new();
    };
    let t_a_sample = machine.clock() / f64::from(rate);
    let samples = (length as f64 / t_a_sample).ceil() as usize;
    let mut out = vec![0.0f32; samples];
    // The time high in each sample's span, as a share of it.
    let mut high_from: Option<f64> = None;
    let add = |from: f64, to: f64, out: &mut Vec<f32>| {
        let (mut at, end) = (from / t_a_sample, to / t_a_sample);
        while at < end {
            let index = at.floor() as usize;
            let next = (index as f64 + 1.0).min(end);
            if let Some(sample) = out.get_mut(index) {
                *sample += (next - at) as f32;
            }
            at = next;
        }
    };
    for &(t, high) in &changes {
        match (high, high_from) {
            (true, None) => high_from = Some(t as f64),
            (false, Some(from)) => {
                add(from, t as f64, &mut out);
                high_from = None;
            }
            _ => {}
        }
    }
    if let Some(from) = high_from {
        add(from, length as f64, &mut out);
    }
    out.iter().map(|&x| x * 0.6).collect()
}
