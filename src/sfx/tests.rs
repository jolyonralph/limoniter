use super::*;
use crate::m6502_cpu::{Bus as Bus6502, Cpu};
use std::path::Path;

/// Every preset, for a machine.
fn presets(machine: Machine) -> Bank {
    Bank { machine, effects: Preset::ALL.iter().map(|&p| Effect::from_shape(p.name(), machine, &Shape::preset(p))).collect() }
}

fn assemble_z80(source: &str) -> crate::asm::Assembly {
    let assembly = crate::assemblers::assemble_own_at(crate::assemblers::Cpu::Z80, Path::new("t.asm"), source, 0);
    assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| (&p.message, source.lines().nth(p.line))));
    assembly
}

fn assemble_6502(source: &str) -> crate::asm::Assembly {
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("t.a"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| (&p.message, source.lines().nth(p.line))));
    assembly
}

fn symbol(assembly: &crate::asm::Assembly, name: &str) -> u16 {
    *assembly.symbols.get(name).unwrap_or_else(|| panic!("{name}")) as u16
}

/// Two effects of a bank as AYFX Editor saved it, for the example of a NextBASIC driver: their frames' bytes.
const EMPTY: &[u8] = &[0xEA, 0xAB, 0x00, 0x00, 0xAA, 0x8A, 0x00, 0x2A, 0xAF, 0x00, 0x2A, 0xC9, 0x00, 0x2A, 0xD5, 0x00, 0x2A, 0x46, 0x01, 0x2A, 0x5F, 0x01, 0xD0, 0x20];
const GOLD: &[u8] = &[0xEA, 0xAB, 0x00, 0x00, 0xAA, 0x81, 0x00, 0xAA, 0x61, 0x00, 0xAA, 0x49, 0x00, 0xAA, 0x37, 0x00, 0xAA, 0x2A, 0x00, 0xAA, 0x20, 0x00, 0xD0, 0x20];

fn editor_bank() -> Vec<u8> {
    let mut bank = vec![2u8];
    let first = 5usize;
    let second = first + EMPTY.len() + 6;
    bank.extend(((first - 2) as u16).to_le_bytes());
    bank.extend(((second - 4) as u16).to_le_bytes());
    bank.extend(EMPTY);
    bank.extend(b"empty\0");
    bank.extend(GOLD);
    bank.extend(b"gold\0");
    bank
}

#[test]
fn an_ayfx_editor_bank_reads_and_is_written_back_the_same() {
    let bytes = editor_bank();
    let effects = read_ayfx_file(&bytes, "example.afb").unwrap();
    assert_eq!(effects.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["empty", "gold"]);
    let empty = &effects[0].frames;
    assert_eq!(empty.len(), 7);
    assert_eq!(empty[0], Frame { tone: true, noise: false, period: 0xAB, noise_period: 0, volume: 10, wave: 0, width: 0 });
    assert_eq!((empty[1].period, empty[1].noise), (0x8A, false));
    assert_eq!((empty[2].period, empty[2].noise, empty[2].tone), (0xAF, true, true), "noise on, its period kept from before");
    assert_eq!(empty[6].period, 0x15F);
    assert_eq!(effects[1].frames.iter().map(|f| f.period).collect::<Vec<_>>(), [0xAB, 0x81, 0x61, 0x49, 0x37, 0x2A, 0x20]);
    assert_eq!(ayfx_bank(&effects).unwrap(), bytes, "written as the editor wrote it");
    // One effect on its own, as a .afx.
    let single = read_ayfx_file(GOLD, "coin.afx").unwrap();
    assert_eq!((single[0].name.as_str(), single[0].frames.len()), ("coin", 7));
    assert!(read_ayfx_file(&[0x2A, 0x10], "bad.afx").is_err());
}

#[test]
fn every_machine_s_effects_assemble_and_read_back() {
    for machine in Machine::ALL {
        let bank = presets(machine);
        for program in [false, true] {
            let source = bank.source("sfx", program).unwrap();
            let assembly = match machine.processor() {
                "Z80" => assemble_z80(&if program && bank.has_program() { source.clone() } else { format!("        org $8000\n{source}") }),
                _ => assemble_6502(&if program { source.clone() } else { format!("SfxAt = $80\nSfxLeft = $81\n* = $c000\n{}", source.replace("AUDF0+SFX_CHANNEL", "$17").replace("AUDV0+SFX_CHANNEL", "$19").replace("AUDC0+SFX_CHANNEL", "$15")) }),
            };
            if program && machine.chip() == Chip::Tia {
                assert_eq!((assembly.lowest(), assembly.highest()), (Some(0xF000), Some(0x10000)), "a 4K cartridge");
            }
            let back = from_source(&source, Machine::default()).unwrap_or_else(|e| panic!("{machine:?}: {e}"));
            assert_eq!(back.machine, machine);
            assert_eq!(back.bytes().unwrap(), bank.bytes().unwrap(), "{machine:?}");
            assert_eq!(back.effects.iter().map(|e| label_of(&e.name)).collect::<Vec<_>>(), bank.effects.iter().map(|e| label_of(&e.name)).collect::<Vec<_>>());
        }
    }
}

#[test]
fn the_presets_make_sense_on_every_machine() {
    for machine in Machine::ALL {
        let chip = machine.chip();
        for preset in Preset::ALL {
            let shape = Shape::preset(preset);
            let effect = Effect::from_shape("x", machine, &shape);
            assert_eq!(effect.frames.len(), shape.frames);
            assert!(effect.frames.iter().all(|f| f.clamped(chip) == *f), "{machine:?} {preset:?}");
            assert!(effect.frames.iter().any(|f| f.sounding(chip)), "{machine:?} {preset:?} makes a sound");
            // The first note is where the shape says, as near as the chip can play it.
            let sounds = shape.sounds();
            if !sounds[0].2 && chip != Chip::Tia {
                let got = hz(machine, &effect.frames[0]).unwrap();
                assert!((got / sounds[0].0).log2().abs() < 1.0 / 12.0, "{machine:?} {preset:?}: {got} for {}", sounds[0].0);
            }
        }
        // Chance makes something different each time, and the same from the same seed.
        assert_ne!(Shape::random(1), Shape::random(2));
        assert_eq!(Shape::random(7), Shape::random(7));
        assert_eq!(Shape::preset(Preset::Laser).vary(3, 0.0).frames, Shape::preset(Preset::Laser).frames);
    }
}

#[test]
fn notes_and_periods_go_both_ways() {
    for machine in Machine::ALL {
        let mut frame = Frame::quiet(machine.chip());
        frame.tone = true;
        frame.volume = 15;
        if machine.chip() == Chip::Tia {
            frame.wave = 4;
        }
        // The TIA's pure notes go from 490 Hz up, in steps of a semitone or so near the bottom.
        let (note, near) = if machine.chip() == Chip::Tia { (1000.0, 0.03) } else { (440.0, 0.01) };
        let period = period_for(machine, note, &frame);
        let back = hz(machine, &Frame { period, ..frame }).unwrap();
        assert!((back / note - 1.0).abs() < near, "{machine:?}: {back}");
    }
    assert_eq!(note_name(440.0), "A4");
    assert_eq!(note_name(261.63), "C4");
}

// ---------------------------------------------------------------------------------------------------------------
// The players, run.

/// The AY's registers as an AY player wrote them, on each machine's ports.
#[derive(Default)]
struct Ay {
    regs: [u8; 16],
    chosen: u8,
    latch: u8,
}

impl Ay {
    fn take(&mut self, machine: Machine, port: u16, value: u8) {
        match machine {
            Machine::Spectrum128 => {
                if port & 0xC002 == 0xC000 {
                    self.chosen = value & 15;
                } else if port & 0xC002 == 0x8000 {
                    self.regs[usize::from(self.chosen)] = value;
                }
            }
            Machine::Msx => match port & 0xFF {
                0xA0 => self.chosen = value & 15,
                0xA1 => self.regs[usize::from(self.chosen)] = value,
                _ => {}
            },
            _ => match port >> 8 {
                0xF4 => self.latch = value,
                0xF6 => match value & 0xC0 {
                    0xC0 => self.chosen = self.latch & 15,
                    0x80 => self.regs[usize::from(self.chosen)] = self.latch,
                    _ => {}
                },
                _ => {}
            },
        }
    }
}

/// The registers a channel's effect leaves, moved from channel A's to its own.
fn on_channel(regs: &[u8; 11], channel: usize) -> Vec<(usize, u8)> {
    let mixer_bits = |m: u8| (m >> channel & 1, m >> (channel + 3) & 1);
    let _ = mixer_bits;
    vec![(channel * 2, regs[0]), (channel * 2 + 1, regs[1]), (8 + channel, regs[8]), (16, (regs[7] & 1) << channel | (regs[7] >> 3 & 1) << (channel + 3))]
}

#[test]
fn the_ay_players_write_what_the_effects_say_on_every_machine() {
    for machine in Chip::Ay.machines().iter().copied() {
        let bank = presets(machine);
        let source = format!("        org $8000\n{}", bank.source("sfx", false).unwrap());
        let assembly = assemble_z80(&source);
        let image = assembly.image(0x8000, assembly.highest().unwrap());
        let mut z80 = z80::Machine::with(&image, 0x8000);
        let mut ay = Ay::default();
        let mut seen = 0;
        let mut frame_call = |z80: &mut z80::Machine, ay: &mut Ay, routine: &str, a: u8, hl: u16| {
            assert!(z80.call(symbol(&assembly, routine), a, hl, 1_000_000), "{routine} returns");
            for &(_, port, value) in &z80.bus.outs[seen..] {
                ay.take(machine, port, value);
            }
            seen = z80.bus.outs.len();
        };
        frame_call(&mut z80, &mut ay, "SfxInit", 0, symbol(&assembly, "sfx"));
        assert_eq!((ay.regs[7] & 0x3F, ay.regs[8], ay.regs[9], ay.regs[10]), (0x3F, 0, 0, 0), "{machine:?}: quiet to begin with");
        // Each effect alone, on channel A.
        for (n, effect) in bank.effects.iter().enumerate() {
            frame_call(&mut z80, &mut ay, "SfxPlay", n as u8, 0);
            for (i, expected) in ay_registers(effect).iter().enumerate() {
                frame_call(&mut z80, &mut ay, "SfxFrame", 0, 0);
                assert_eq!(&ay.regs[..11], &expected[..], "{machine:?} {} frame {i}", effect.name);
            }
        }
        // Two at once: the second on channel B, each as it is alone.
        let (first, second) = (&bank.effects[1], &bank.effects[3]);
        frame_call(&mut z80, &mut ay, "SfxPlay", 1, 0);
        frame_call(&mut z80, &mut ay, "SfxFrame", 0, 0);
        frame_call(&mut z80, &mut ay, "SfxPlay", 3, 0);
        let (a, b) = (ay_registers(first), ay_registers(second));
        for i in 0..b.len().min(a.len() - 1) {
            frame_call(&mut z80, &mut ay, "SfxFrame", 0, 0);
            for (channel, regs) in [(0, &a[i + 1]), (1, &b[i])] {
                for (r, value) in on_channel(regs, channel) {
                    if r == 16 {
                        let mask = (1 << channel) | (1 << (channel + 3));
                        assert_eq!(ay.regs[7] & mask, value, "{machine:?} mixer, channel {channel}, frame {i}");
                    } else {
                        assert_eq!(ay.regs[r], value, "{machine:?} register {r}, frame {i}");
                    }
                }
            }
        }
    }
}

/// A Commodore 64 with nothing but memory, where the SID's registers are kept as written.
struct C64(Vec<u8>);

impl Bus6502 for C64 {
    fn read(&mut self, address: u16) -> u8 {
        self.0[usize::from(address)]
    }
    fn write(&mut self, address: u16, value: u8) {
        self.0[usize::from(address)] = value;
    }
}

/// Calls a routine with A set, until it returns.
fn call_6502(cpu: &mut Cpu, bus: &mut impl Bus6502, routine: u16, a: u8) -> u64 {
    cpu.a = a;
    cpu.s = 0xFD;
    bus.write(0x1FE, 0xFF);
    bus.write(0x1FF, 0xFF);
    cpu.pc = routine;
    let mut cycles = 0u64;
    while cpu.pc != 0 {
        cycles += u64::from(cpu.step(bus));
        assert!(cycles < 1_000_000, "it returns");
    }
    cycles
}

#[test]
fn the_sid_player_writes_what_the_effects_say() {
    let bank = presets(Machine::C64Pal);
    let assembly = assemble_6502(&format!("* = $c000\n{}", bank.source("sfx", false).unwrap()));
    let mut c64 = C64(vec![0; 0x10000]);
    let image = assembly.image(0xC000, assembly.highest().unwrap());
    c64.0[0xC000..0xC000 + image.len()].copy_from_slice(&image);
    let mut cpu = Cpu::default();
    call_6502(&mut cpu, &mut c64, symbol(&assembly, "sfx_init"), 0);
    assert_eq!(c64.0[0xD418], 15);
    for (n, effect) in bank.effects.iter().enumerate() {
        call_6502(&mut cpu, &mut c64, symbol(&assembly, "sfx_play"), n as u8);
        assert_eq!(&c64.0[0xD412..0xD415], &[0x08, effect.adsr[0], effect.adsr[1]], "{}: the test bit, then its envelope", effect.name);
        for (i, expected) in sid_registers(effect).iter().enumerate() {
            let cycles = call_6502(&mut cpu, &mut c64, symbol(&assembly, "sfx_frame"), 0);
            assert!(cycles < 200, "a frame is quick: {cycles} cycles");
            assert_eq!(&c64.0[0xD40E..0xD415], &expected[..], "{} frame {i}", effect.name);
        }
        assert_eq!(c64.0[0xFC], 0, "{}: over", effect.name);
    }
}

/// An Atari 2600 for counting frames and hearing channel 0: the cartridge, RAM, WSYNC, VSYNC and the sound's
/// registers, each frame's as VSYNC starts.
fn atari_frames(rom: &[u8], count: usize) -> Vec<(u64, [u8; 3])> {
    struct Atari<'a> {
        rom: &'a [u8],
        ram: [u8; 128],
        wsync: bool,
        vsync: Option<bool>,
        sound: [u8; 3],
    }
    impl Bus6502 for Atari<'_> {
        fn read(&mut self, address: u16) -> u8 {
            let address = address & 0x1FFF;
            if address & 0x1000 != 0 {
                self.rom[usize::from(address & 0xFFF)]
            } else if address & 0x280 == 0x80 {
                self.ram[usize::from(address & 0x7F)]
            } else {
                0
            }
        }
        fn write(&mut self, address: u16, value: u8) {
            let address = address & 0x1FFF;
            if address & 0x1000 != 0 {
            } else if address & 0x280 == 0x80 {
                self.ram[usize::from(address & 0x7F)] = value;
            } else if address & 0x80 == 0 {
                match address & 0x3F {
                    0x00 => self.vsync = Some(value & 2 != 0),
                    0x02 => self.wsync = true,
                    0x15 => self.sound[0] = value & 15,
                    0x17 => self.sound[1] = value & 31,
                    0x19 => self.sound[2] = value & 15,
                    _ => {}
                }
            }
        }
    }
    let mut atari = Atari { rom, ram: [0; 128], wsync: false, vsync: None, sound: [0; 3] };
    let mut cpu = Cpu { pc: u16::from_le_bytes([rom[0xFFC], rom[0xFFD]]), ..Cpu::default() };
    let mut t = 0u64;
    let mut on = false;
    let mut started: Option<u64> = None;
    let mut out = Vec::new();
    while out.len() < count && t < 500_000_000 {
        t += u64::from(cpu.step(&mut atari));
        if std::mem::take(&mut atari.wsync) {
            t = (t / 76 + 1) * 76;
        }
        if let Some(now) = atari.vsync.take() {
            if now && !on {
                if let Some(from) = started {
                    out.push(((t - from + 38) / 76, atari.sound));
                }
                started = Some(t);
            }
            on = now;
        }
    }
    out
}

#[test]
fn the_2600_program_keeps_its_frames_and_plays_each_effect() {
    for machine in Chip::Tia.machines().iter().copied() {
        let bank = presets(machine);
        let assembly = assemble_6502(&bank.source("sfx", true).unwrap());
        let rom = assembly.image(0xF000, 0x10000);
        let total: usize = bank.effects.iter().map(|e| e.frames.len() + 25).sum();
        let frames = atari_frames(&rom, total + 5);
        let lines = if machine == Machine::Atari2600Ntsc { 262 } else { 312 };
        assert!(frames.iter().all(|f| f.0 == lines), "{machine:?}: {:?}", frames.iter().map(|f| f.0).collect::<std::collections::BTreeSet<_>>());
        // Each effect, frame by frame, then quiet until the next.
        let mut at = 0;
        for effect in &bank.effects {
            let expected = tia_registers(effect);
            let heard: Vec<[u8; 3]> = frames[at..at + effect.frames.len() + 25].iter().map(|f| f.1).collect();
            assert_eq!(&heard[..effect.frames.len()], &expected[..effect.frames.len()], "{machine:?} {}", effect.name);
            assert!(heard[effect.frames.len()..].iter().all(|r| r[2] == 0), "{machine:?} {}: quiet after", effect.name);
            at += effect.frames.len() + 25;
        }
    }
}

#[test]
fn the_beeper_plays_the_notes_and_lengths_it_is_given() {
    for machine in Chip::Beeper.machines().iter().copied() {
        let bank = presets(machine);
        for effect in &bank.effects {
            let (changes, length) = sound::beeper_run(machine, effect).unwrap();
            // Every frame's waves are as long as its period says, to the T-state.
            let ups: Vec<u64> = changes.windows(2).filter(|w| !w[0].1 && w[1].1).map(|w| w[1].0).collect();
            let waves: std::collections::BTreeSet<u64> = ups.windows(2).map(|w| w[1] - w[0]).collect();
            let notes: std::collections::BTreeSet<u64> = effect.frames.iter().filter(|f| f.sounding(Chip::Beeper) && !f.noise).map(|f| u64::from(BEEPER_WAVE_FIXED + BEEPER_COUNT_T * u32::from(f.period))).collect();
            if !notes.is_empty() {
                assert!(notes.iter().all(|n| waves.contains(n)), "{machine:?} {}: {notes:?} in {waves:?}", effect.name);
            }
            // And the whole lasts its frames, within a wave a frame.
            let wanted = effect.frames.len() as f64 / machine.frame_rate() * machine.clock();
            assert!((length as f64 / wanted - 1.0).abs() < 0.12, "{machine:?} {}: {length} T-states for {wanted}", effect.name);
            // A frame's high half is as wide as its width says.
            if let Some(f) = effect.frames.iter().find(|f| f.sounding(Chip::Beeper) && !f.noise) {
                let (high, low, _) = beeper_counts(machine, f);
                assert_eq!(u32::from(high + low), u32::from(f.period));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound.

/// How often a sound repeats, in hertz, from where it crosses the middle going up.
fn pitch(samples: &[f32], rate: u32) -> f64 {
    let ups: Vec<usize> = samples.windows(2).enumerate().filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0).map(|(i, _)| i).collect();
    let (first, last) = (ups[1], ups[ups.len() - 2]);
    f64::from(rate) * (ups.len() - 3) as f64 / (last - first) as f64
}

fn held(chip: Chip, frame: Frame, frames: usize) -> Effect {
    Effect { name: "held".into(), frames: vec![frame.clamped(chip); frames], adsr: [0x00, 0xF0] }
}

#[test]
fn each_chip_sounds_the_note_its_registers_say() {
    let rate = 48_000;
    let ay = Frame { tone: true, noise: false, period: 252, noise_period: 0, volume: 15, wave: 0, width: 0 };
    let tia = Frame { volume: 15, wave: 4, period: 20, ..Frame::quiet(Chip::Tia) };
    let tia3 = Frame { wave: 12, period: 5, ..tia };
    let sid = Frame { tone: true, wave: 0x20, period: 7493, ..Frame::quiet(Chip::Sid) };
    for (machine, frame) in [(Machine::Spectrum128, ay), (Machine::Cpc, ay), (Machine::Atari2600Ntsc, tia), (Machine::Atari2600Pal, tia3), (Machine::C64Pal, sid)] {
        let effect = held(machine.chip(), frame, 50);
        let samples = sound::render(machine, &effect, rate);
        let expected = hz(machine, &frame).unwrap();
        let heard = pitch(&samples[samples.len() / 10..samples.len() * 8 / 10], rate);
        assert!((heard / expected - 1.0).abs() < 0.01, "{machine:?}: heard {heard} Hz for {expected}");
    }
    // The beeper's note, from the player itself.
    let beep = Frame { tone: true, volume: 15, period: 300, ..Frame::quiet(Chip::Beeper) };
    let effect = held(Chip::Beeper, beep, 30);
    let samples = sound::render(Machine::Spectrum48Beeper, &effect, rate);
    let heard = pitch(&samples[2400..samples.len() - 2400], rate);
    let expected = hz(Machine::Spectrum48Beeper, &beep).unwrap();
    assert!((heard / expected - 1.0).abs() < 0.01, "beeper: {heard} for {expected}");
}

#[test]
fn quiet_frames_are_quiet_and_volume_is_louder() {
    let rate = 48_000;
    let loudest = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    for machine in [Machine::Spectrum128, Machine::Atari2600Ntsc] {
        let chip = machine.chip();
        let mut frame = Frame { tone: true, volume: 15, period: if chip == Chip::Tia { 10 } else { 200 }, ..Frame::quiet(chip) };
        let loud = loudest(&sound::render(machine, &held(chip, frame, 20), rate)[2000..]);
        frame.volume = 6;
        let softer = loudest(&sound::render(machine, &held(chip, frame, 20), rate)[2000..]);
        frame.volume = 0;
        let none = loudest(&sound::render(machine, &held(chip, frame, 20), rate)[2000..]);
        assert!(loud > softer * 1.5 && softer > none && none < 0.01, "{machine:?}: {loud} {softer} {none}");
    }
}

#[test]
fn the_sid_s_envelope_rises_and_falls_in_its_time() {
    // Attack 8 (100 ms), sustain full, release 9 (750 ms).
    let mut voice = sound::SidVoice::default();
    voice.write(5, 0x80);
    voice.write(6, 0xF9);
    voice.write(4, 0x21);
    let clock = Machine::C64Pal.clock();
    let ms = |n: f64| (clock * n / 1000.0) as usize;
    for _ in 0..ms(50.0) {
        voice.clock();
    }
    let halfway = voice.envelope();
    assert!((110..=145).contains(&halfway), "halfway up after half the attack: {halfway}");
    for _ in 0..ms(60.0) {
        voice.clock();
    }
    assert_eq!(voice.envelope(), 255);
    voice.write(4, 0x20);
    for _ in 0..ms(750.0) {
        voice.clock();
    }
    assert!(voice.envelope() < 8, "released: {}", voice.envelope());
}

// ---------------------------------------------------------------------------------------------------------------
// In real emulators, when they're here.

/// The cartridge run in MAME for some seconds with its sound written to a WAV file: the samples, and their rate.
fn mame_hears(rom: &[u8], pal: bool, seconds: u32) -> Option<(Vec<f32>, u32)> {
    let program = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("mame")).find(|p| p.is_file()))?;
    let folder = std::env::temp_dir().join(format!("limoniter-sfx-{}-{pal}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("cart.bin"), rom).unwrap();
    let output = crate::platform::headless(program)
        .args([if pal { "a2600p" } else { "a2600" }, "-cart", "cart.bin", "-video", "none", "-sound", "none", "-wavwrite", "out.wav", "-seconds_to_run", &seconds.to_string(), "-nothrottle", "-skip_gameinfo", "-nvram_directory", "nv", "-cfg_directory", "cfg"])
        .current_dir(&folder)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let wav = std::fs::read(folder.join("out.wav")).unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    Some(read_wav(&wav))
}

/// A 16-bit PCM WAV's first channel.
fn read_wav(wav: &[u8]) -> (Vec<f32>, u32) {
    let mut at = 12;
    let (mut channels, mut rate, mut samples) = (1usize, 48_000u32, Vec::new());
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size = u32::from_le_bytes(wav[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &wav[at + 8..(at + 8 + size).min(wav.len())];
        if id == b"fmt " {
            channels = usize::from(u16::from_le_bytes([body[2], body[3]]));
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
        } else if id == b"data" {
            samples = body.chunks_exact(2 * channels).map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0).collect();
        }
        at += 8 + size + (size & 1);
    }
    (samples, rate)
}

/// The spans of a sound that aren't quiet, as (start, end) samples.
fn sounding_spans(samples: &[f32]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut last_loud = 0;
    for (i, x) in samples.iter().enumerate() {
        if x.abs() > 0.002 {
            if start.is_none() {
                start = Some(i);
            }
            last_loud = i;
        } else if let Some(s) = start
            && i - last_loud > 2000
        {
            spans.push((s, last_loud));
            start = None;
        }
    }
    if let Some(s) = start {
        spans.push((s, last_loud));
    }
    spans
}

/// A sound's period in samples, by where it best matches itself moved along: for notes that aren't squares.
fn period_of(samples: &[f32], least: usize, most: usize) -> usize {
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    let centred: Vec<f32> = samples.iter().map(|x| x - mean).collect();
    let n = centred.len() - most;
    (least..most).max_by(|&a, &b| {
        let score = |lag: usize| (0..n).map(|i| centred[i] * centred[i + lag]).sum::<f32>();
        score(a).total_cmp(&score(b))
    }).unwrap()
}

#[test]
fn mame_plays_the_2600_s_effects_as_they_sound_here() {
    // Notes of several kinds, each held a second: pure, divided by 3, divided by 31, and the 4-bit polynomial.
    let kinds = [(4u8, 9u8), (12, 3), (6, 1), (1, 3)];
    let effects: Vec<Effect> = kinds.iter().map(|&(audc, audf)| Effect { name: format!("c{audc}"), frames: vec![Frame { wave: audc, period: u16::from(audf), volume: 12, ..Frame::quiet(Chip::Tia) }; 60], adsr: [0, 0xF0] }).collect();
    let bank = Bank { machine: Machine::Atari2600Ntsc, effects };
    let rom = assemble_6502(&bank.source("sfx", true).unwrap()).image(0xF000, 0x10000);
    let Some((heard, rate)) = mame_hears(&rom, false, 8) else { return };
    let spans = sounding_spans(&heard);
    assert!(spans.len() >= kinds.len(), "{} notes heard: {spans:?}", spans.len());
    for (effect, &(start, end)) in bank.effects.iter().zip(&spans) {
        let ours = sound::render(Machine::Atari2600Ntsc, effect, rate);
        let theirs = &heard[start + (end - start) / 4..start + (end - start) * 3 / 4];
        let ticks = tia_ticks(effect.frames[0].wave).unwrap() * f64::from(effect.frames[0].period + 1);
        let expected = f64::from(rate) * ticks / Machine::Atari2600Ntsc.clock();
        let (least, most) = ((expected * 0.6) as usize, (expected * 1.6) as usize + 2);
        let mame = period_of(theirs, least, most);
        let here = period_of(&ours[ours.len() / 4..ours.len() * 3 / 4], least, most);
        assert!((mame as f64 - here as f64).abs() <= 1.0 + expected * 0.01, "{}: MAME repeats every {mame} samples, here every {here} (expected {expected:.1})", effect.name);
        // And lasts as long: 60 frames.
        let seconds = (end - start) as f64 / f64::from(rate);
        assert!((seconds - 60.0 / Machine::Atari2600Ntsc.frame_rate()).abs() < 0.05, "{}: {seconds} s", effect.name);
    }
}

/// With LIMONITER_VICE naming VICE's x64sc (and LIMONITER_VICE_OPTIONS where its ROMs are), the C64 program run
/// with its sound recorded: each effect's note as the model says. It opens VICE's window: run it under xvfb-run.
#[test]
fn vice_plays_the_sid_s_effects_as_they_sound_here() {
    let Some(vice) = std::env::var_os("LIMONITER_VICE") else {
        return;
    };
    let options = std::env::var("LIMONITER_VICE_OPTIONS").unwrap_or_default();
    let notes = [(0x20u8, 7493u16), (0x40, 3000), (0x10, 12000)];
    let effects: Vec<Effect> = notes.iter().map(|&(wave, period)| Effect { name: format!("w{wave:x}"), frames: vec![Frame { tone: true, wave, period, ..Frame::quiet(Chip::Sid) }; 50], adsr: [0x00, 0xF0] }).collect();
    let bank = Bank { machine: Machine::C64Pal, effects };
    let assembly = assemble_6502(&bank.source("sfx", true).unwrap());
    let dir = std::env::temp_dir().join(format!("limoniter-sid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let prg = dir.join("sfx.prg");
    std::fs::write(&prg, crate::asm::output::commodore_program(&assembly).unwrap()).unwrap();
    let wav = dir.join("out.wav");
    let status = crate::platform::command(&vice)
        .arg("-default")
        .args(options.split_whitespace())
        // Its sound written as it's made: only when the WAV file is the sound device, and not in warp mode.
        .args(["-sound", "-sounddev", "wav", "-soundarg"])
        .arg(&wav)
        .args(["-soundrate", "48000", "-autostartprgmode", "1", "-limitcycles", "12000000", "-autostart"])
        .arg(&prg)
        .output()
        .unwrap();
    let bytes = std::fs::read(&wav).unwrap_or_else(|_| panic!("no recording: {}", String::from_utf8_lossy(&status.stdout)));
    let (heard, rate) = read_wav(&bytes);
    let spans = sounding_spans(&heard);
    assert!(spans.len() >= notes.len(), "{spans:?}");
    for (effect, &(start, end)) in bank.effects.iter().zip(&spans) {
        let expected = hz(Machine::C64Pal, &effect.frames[0]).unwrap();
        let samples = f64::from(rate) / expected;
        let middle = &heard[start + (end - start) / 4..start + (end - start) * 3 / 4];
        let theirs = period_of(middle, (samples * 0.7) as usize, (samples * 1.4) as usize + 2);
        assert!((theirs as f64 - samples).abs() <= 1.0 + samples * 0.02, "{}: VICE repeats every {theirs} samples, for {samples:.1}", effect.name);
    }
}
