//! Impulse Tracker modules (.it), as ITTECH.TXT lays them out: a header, instruments with their note maps and
//! envelopes, samples (packed with IT 2.14's or 2.15's compression or not), and patterns packed with masks.

use super::{Cell, Envelope, Format, Instrument, Loop, NOTE_CUT, NOTE_FADE, NOTE_OFF, NewNote, Pattern, Sample, Song, Vol, st_fx, st_shown, text, u16_le, u32_le, widen, widen16};

fn volume_column(v: u8) -> Vol {
    const PORTA: [u8; 10] = [0, 1, 4, 8, 16, 32, 64, 96, 128, 255];
    match v {
        0..=64 => Vol::Set(v),
        65..=74 => Vol::FineUp(v - 65),
        75..=84 => Vol::FineDown(v - 75),
        85..=94 => Vol::SlideUp(v - 85),
        95..=104 => Vol::SlideDown(v - 95),
        105..=114 => Vol::PortaDown((v - 105) * 4),
        115..=124 => Vol::PortaUp((v - 115) * 4),
        128..=192 => Vol::Pan(((u16::from(v - 128) * 4).min(255)) as u8),
        193..=202 => Vol::TonePorta(PORTA[usize::from(v - 193)]),
        203..=212 => Vol::VibratoDepth(v - 203),
        _ => Vol::None,
    }
}

/// An envelope at `at`: its flags, point count, loop and sustain loop, then 25 points of a value and a tick.
/// Panning's values go from -32 to 32, and are moved up to 0 to 64.
fn envelope(h: &[u8], at: usize, signed: bool) -> Envelope {
    let Some(e) = h.get(at..at + 6 + 75) else { return Envelope::default() };
    let flags = e[0];
    let count = usize::from(e[1].min(25));
    let points: Vec<(u16, u8)> = (0..count).map(|i| {
        let value = e[6 + i * 3];
        let value = if signed { (i16::from(value as i8) + 32).clamp(0, 64) as u8 } else { value.min(64) };
        (u16_le(e, 7 + i * 3), value)
    }).collect();
    let pair = |a: u8, b: u8| (usize::from(a) < points.len() && usize::from(b) < points.len() && a <= b).then_some((usize::from(a), usize::from(b)));
    Envelope { on: flags & 1 != 0 && !points.is_empty(), looped: if flags & 2 != 0 { pair(e[2], e[3]) } else { None }, sustain: if flags & 4 != 0 { pair(e[4], e[5]) } else { None }, points }
}

/// IT 2.14's sample compression, and 2.15's (which adds the differences once more): blocks of 32K bytes'
/// worth of samples, each with its own length, holding numbers of a width that changes as it goes.
pub fn decompress(bytes: &[u8], samples: usize, wide: bool, it215: bool) -> Vec<i16> {
    let mut out = Vec::with_capacity(samples);
    let mut at = 0;
    let block = if wide { 0x4000 } else { 0x8000 };
    while out.len() < samples && at + 2 <= bytes.len() {
        let size = usize::from(u16_le(bytes, at));
        at += 2;
        let data = &bytes[at..(at + size).min(bytes.len())];
        at += size;
        let mut bit = 0usize;
        let mut read = |n: u32| -> Option<u32> {
            let mut value = 0u32;
            for i in 0..n {
                let byte = *data.get(bit / 8)?;
                value |= u32::from((byte >> (bit % 8)) & 1) << i;
                bit += 1;
            }
            Some(value)
        };
        let count = block.min(samples - out.len());
        let top: u32 = if wide { 17 } else { 9 };
        let mut width = top;
        let (mut d1, mut d2) = (0i32, 0i32);
        let mut made = 0;
        while made < count {
            let Some(mut value) = read(width) else { break };
            if width < 7 {
                if value == 1 << (width - 1) {
                    let Some(new) = read(if wide { 4 } else { 3 }) else { break };
                    let new = new + 1;
                    width = if new < width { new } else { new + 1 };
                    continue;
                }
            } else if width < top {
                let border = ((if wide { 0xFFFF } else { 0xFF }) >> (top - width)) - if wide { 8 } else { 4 };
                if value > border && value <= border + if wide { 16 } else { 8 } {
                    value -= border;
                    width = if value < width { value } else { value + 1 };
                    continue;
                }
            } else if width == top {
                if value & (1 << (top - 1)) != 0 {
                    width = (value + 1) & 0xFF;
                    continue;
                }
            } else {
                break;
            }
            let bits = if wide { 16 } else { 8 };
            let v = if width < bits {
                let shift = 32 - width;
                ((value << shift) as i32) >> shift
            } else if wide {
                i32::from(value as u16 as i16)
            } else {
                i32::from(value as u8 as i8)
            };
            d1 = d1.wrapping_add(v);
            d1 = if wide { i32::from(d1 as i16) } else { i32::from(d1 as i8) };
            d2 = d2.wrapping_add(d1);
            d2 = if wide { i32::from(d2 as i16) } else { i32::from(d2 as i8) };
            let sample = if it215 { d2 } else { d1 };
            out.push(if wide { sample as i16 } else { (sample as i16) << 8 });
            made += 1;
        }
        // A block that ran short is made up with silence, so what follows stays in its place.
        while made < count {
            out.push(0);
            made += 1;
        }
    }
    out.resize(samples, 0);
    out
}

pub fn load(bytes: &[u8]) -> Result<Song, String> {
    if bytes.len() < 0xC0 || !bytes.starts_with(b"IMPM") {
        return Err(crate::i18n::tr("This isn't an Impulse Tracker module: it doesn't start IMPM.").into());
    }
    let orders = usize::from(u16_le(bytes, 0x20));
    let instrument_count = usize::from(u16_le(bytes, 0x22));
    let sample_count = usize::from(u16_le(bytes, 0x24));
    let pattern_count = usize::from(u16_le(bytes, 0x26));
    let made_with = u16_le(bytes, 0x28);
    let compatible = u16_le(bytes, 0x2A);
    let flags = u16_le(bytes, 0x2C);
    let special = u16_le(bytes, 0x2E);
    let mut song = Song::new(Format::It, 64);
    song.title = text(&bytes[4..30]);
    song.tracker = match made_with >> 12 {
        0 if made_with > 0 => crate::i18n::tf!("Impulse Tracker {}.{:02X}", (made_with >> 8) & 0xF, made_with & 0xFF),
        1 => crate::i18n::tr("Schism Tracker").into(),
        5 => "OpenMPT".into(),
        _ => String::new(),
    };
    let stereo = flags & 1 != 0;
    let use_instruments = flags & 4 != 0 && compatible >= 0x200;
    song.linear = flags & 8 != 0;
    song.global_volume = bytes[0x30].min(128);
    song.speed = bytes[0x32].max(1);
    song.tempo = bytes[0x33].max(32);
    song.middle = 60;
    if special & 1 != 0 {
        let (length, at) = (usize::from(u16_le(bytes, 0x36)), u32_le(bytes, 0x38) as usize);
        if let Some(message) = bytes.get(at..at + length) {
            song.message = text(message).replace('\r', "\n");
        }
    }
    for c in 0..64 {
        let pan = bytes[0x40 + c];
        song.muted[c] = pan & 0x80 != 0;
        song.pans[c] = if !stereo || pan & 0x7F == 100 { 128 } else { ((u16::from(pan & 0x7F).min(64)) * 4).min(255) as u8 };
        song.surround[c] = stereo && pan & 0x7F == 100;
        song.channel_volumes[c] = bytes[0x80 + c].min(64);
    }
    song.orders = bytes.get(0xC0..0xC0 + orders).ok_or(crate::i18n::tr("The module's order is cut short."))?.to_vec();
    while song.orders.last() == Some(&255) {
        song.orders.pop();
    }
    let pointers = 0xC0 + orders;
    let pointer = |table: usize, i: usize| u32_le(bytes, pointers + table * 4 + i * 4) as usize;
    for i in 0..instrument_count {
        let at = pointer(0, i);
        let Some(h) = bytes.get(at..at + 0x130).filter(|h| h.starts_with(b"IMPI")) else {
            song.instruments.push(Instrument { keymap: vec![(0, 0); 120], ..Instrument::default() });
            continue;
        };
        let keymap = (0..120).map(|n| (h[0x40 + n * 2].min(119) + 1, u16::from(h[0x41 + n * 2]))).collect();
        song.instruments.push(Instrument {
            name: text(&h[0x20..0x3A]),
            keymap,
            new_note: match h[0x11] {
                1 => NewNote::Continue,
                2 => NewNote::Off,
                3 => NewNote::Fade,
                _ => NewNote::Cut,
            },
            fadeout: f32::from(u16_le(h, 0x14)) / 1024.0,
            global_volume: h[0x18].min(128),
            pan: (h[0x19] & 0x80 == 0).then(|| ((u16::from(h[0x19]).min(64)) * 4).min(255) as u8),
            volume: envelope(bytes, at + 0x130, false),
            panning: envelope(bytes, at + 0x182, true),
        });
    }
    for i in 0..sample_count {
        let at = pointer(instrument_count, i);
        let Some(h) = bytes.get(at..at + 0x50).filter(|h| h.starts_with(b"IMPS")) else {
            song.samples.push(Sample::default());
            continue;
        };
        let sample_flags = h[0x12];
        let convert = h[0x2E];
        let wide = sample_flags & 2 != 0;
        let length = u32_le(h, 0x30) as usize;
        let data_at = u32_le(h, 0x48) as usize;
        let data = if sample_flags & 1 == 0 || length == 0 {
            Vec::new()
        } else if sample_flags & 8 != 0 {
            decompress(bytes.get(data_at..).unwrap_or(&[]), length, wide, convert & 4 != 0)
        } else {
            let raw = bytes.get(data_at.min(bytes.len())..(data_at + length * if wide { 2 } else { 1 }).min(bytes.len())).unwrap_or(&[]);
            if wide { widen16(raw, convert & 1 != 0) } else { widen(raw, convert & 1 != 0) }
        };
        let fits = |start: usize, end: usize| start < end && start < data.len();
        let (loop_start, loop_end) = (u32_le(h, 0x34) as usize, u32_le(h, 0x38) as usize);
        let (sustain_start, sustain_end) = (u32_le(h, 0x40) as usize, u32_le(h, 0x44) as usize);
        let rate = u32_le(h, 0x3C);
        song.samples.push(Sample {
            name: text(&h[0x14..0x2E]),
            loop_kind: if sample_flags & 0x10 != 0 && fits(loop_start, loop_end) {
                if sample_flags & 0x40 != 0 { Loop::PingPong } else { Loop::Forward }
            } else {
                Loop::None
            },
            loop_start,
            loop_end: loop_end.min(data.len()),
            sustain: (sample_flags & 0x20 != 0 && fits(sustain_start, sustain_end)).then(|| (sustain_start, sustain_end.min(data.len()), if sample_flags & 0x80 != 0 { Loop::PingPong } else { Loop::Forward })),
            volume: h[0x13].min(64),
            global_volume: h[0x11].min(64),
            pan: (h[0x2F] & 0x80 != 0).then(|| ((u16::from(h[0x2F] & 0x7F).min(64)) * 4).min(255) as u8),
            base_rate: if rate == 0 { 8363.0 } else { rate as f32 },
            bits: if wide { 16 } else { 8 },
            data,
            ..Sample::default()
        });
    }
    let mut highest = 0;
    for i in 0..pattern_count {
        let at = pointer(instrument_count + sample_count, i);
        let rows = if at == 0 { 64 } else { usize::from(u16_le(bytes, at + 2)).clamp(1, 200) };
        let mut pattern = Pattern::empty(rows, 64);
        if at != 0 {
            let end = (at + 8 + usize::from(u16_le(bytes, at))).min(bytes.len());
            let mut i = at + 8;
            let (mut masks, mut last) = ([0u8; 64], [Cell::default(); 64]);
            let mut row = 0;
            let take = |i: &mut usize| {
                let b = bytes.get(*i).copied().unwrap_or(0);
                *i += 1;
                b
            };
            while row < rows && i < end {
                let what = take(&mut i);
                if what == 0 {
                    row += 1;
                    continue;
                }
                let channel = usize::from((what - 1) & 63);
                if what & 0x80 != 0 {
                    masks[channel] = take(&mut i);
                }
                let mask = masks[channel];
                let mut cell = Cell::default();
                let previous = &mut last[channel];
                if mask & 1 != 0 {
                    previous.note = match take(&mut i) {
                        n @ 0..=119 => n + 1,
                        255 => NOTE_OFF,
                        254 => NOTE_CUT,
                        _ => NOTE_FADE,
                    };
                }
                if mask & 2 != 0 {
                    previous.instrument = take(&mut i);
                }
                if mask & 4 != 0 {
                    previous.vol = volume_column(take(&mut i));
                }
                if mask & 8 != 0 {
                    let (command, param) = (take(&mut i), take(&mut i));
                    previous.fx = st_fx(command, param, true);
                    previous.shown = st_shown(command, param);
                }
                if mask & 0x11 != 0 {
                    cell.note = previous.note;
                }
                if mask & 0x22 != 0 {
                    cell.instrument = previous.instrument;
                }
                if mask & 0x44 != 0 {
                    cell.vol = previous.vol;
                }
                if mask & 0x88 != 0 {
                    cell.fx = previous.fx;
                    cell.shown = previous.shown;
                }
                highest = highest.max(channel);
                pattern.cells[row * 64 + channel] = cell;
            }
        }
        song.patterns.push(pattern);
    }
    // Only as many channels as the patterns use are kept, so the pattern view isn't 64 wide.
    let channels = highest + 1;
    for pattern in &mut song.patterns {
        let cells = std::mem::take(&mut pattern.cells);
        pattern.cells = cells.chunks(64).flat_map(|row| row[..channels].to_vec()).collect();
    }
    song.channels = channels;
    song.pans.truncate(channels);
    song.channel_volumes.truncate(channels);
    song.muted.truncate(channels);
    song.surround.truncate(channels);
    if !use_instruments {
        song.instruments.clear();
    }
    Ok(song)
}
