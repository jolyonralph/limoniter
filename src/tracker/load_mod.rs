//! ProTracker modules to play: the header as `protracker` reads it, then the patterns and the samples.

use super::{Cell, Format, Loop, Pattern, Sample, Song, mod_fx, mod_shown, widen};

/// The note (from 1) whose Amiga period is nearest: C-4 here, ProTracker's C-2, is period 428.
fn note_of(period: u16) -> u8 {
    if period == 0 {
        return 0;
    }
    let semitones = 12.0 * (428.0 / f32::from(period)).log2();
    (49.0 + semitones).round().clamp(1.0, 120.0) as u8
}

pub fn load(bytes: &[u8], name: &str) -> Result<Song, String> {
    let module = crate::protracker::read(bytes, name)?;
    let flt8 = module.tag == "FLT8";
    let channels = module.channels;
    let mut song = Song::new(Format::Mod, channels);
    song.title = module.title.clone();
    song.tracker = module.format.clone();
    song.orders = module.order.iter().map(|&p| if flt8 { p / 2 } else { p }).collect();
    song.restart = Some(usize::from(module.restart)).filter(|&r| r < song.orders.len()).unwrap_or(0);
    // Paula's channels go left, right, right, left; panned in a little, as most players do, for headphones.
    song.pans = (0..channels).map(|c| if matches!(c % 4, 0 | 3) { 64 } else { 192 }).collect();
    song.amiga_limits = channels == 4;
    song.octave_shift = 2;
    let header = if module.samples.len() == 31 { 1084 } else { 600 };
    // A 15-sample module's restart byte is its tempo, which isn't used: it plays at 125.
    if module.samples.len() == 15 {
        song.restart = 0;
    }
    let read_pattern = |at: usize, width: usize| -> Vec<Cell> {
        let mut cells = Vec::with_capacity(64 * width);
        for i in 0..64 * width {
            let p = at + i * 4;
            let Some(b) = bytes.get(p..p + 4) else {
                cells.push(Cell::default());
                continue;
            };
            let instrument = (b[0] & 0xF0) | (b[2] >> 4);
            let period = (u16::from(b[0] & 0x0F) << 8) | u16::from(b[1]);
            let (effect, param) = (b[2] & 0x0F, b[3]);
            cells.push(Cell { note: note_of(period), instrument, vol: super::Vol::None, fx: mod_fx(effect, param, false), shown: mod_shown(effect, param) });
        }
        cells
    };
    if flt8 {
        // Each 8-channel pattern is two 4-channel ones, left half then right.
        for p in 0..module.patterns {
            let (left, right) = (read_pattern(header + p * 2 * 1024, 4), read_pattern(header + (p * 2 + 1) * 1024, 4));
            let mut cells = Vec::with_capacity(64 * 8);
            for row in 0..64 {
                cells.extend_from_slice(&left[row * 4..row * 4 + 4]);
                cells.extend_from_slice(&right[row * 4..row * 4 + 4]);
            }
            song.patterns.push(Pattern { rows: 64, cells });
        }
    } else {
        for p in 0..module.patterns {
            song.patterns.push(Pattern { rows: 64, cells: read_pattern(header + p * 64 * 4 * channels, channels) });
        }
    }
    let pattern_bytes = if flt8 { module.patterns * 2048 } else { module.patterns * 64 * 4 * channels };
    let mut at = header + pattern_bytes;
    for s in &module.samples {
        let length = s.length as usize;
        let data = bytes.get(at.min(bytes.len())..(at + length).min(bytes.len())).unwrap_or(&[]);
        at += length;
        let data = widen(data, true);
        let (start, end) = (s.loop_start as usize, (s.loop_start + s.loop_length) as usize);
        let loops = s.loops() && start < data.len();
        song.samples.push(Sample {
            name: s.name.clone(),
            loop_kind: if loops { Loop::Forward } else { Loop::None },
            loop_start: start.min(data.len()),
            loop_end: end.min(data.len()),
            volume: s.volume.min(64),
            global_volume: 64,
            base_rate: 8363.0,
            finetune: i16::from(s.finetune) * 16,
            bits: 8,
            data,
            ..Sample::default()
        });
    }
    Ok(song)
}
