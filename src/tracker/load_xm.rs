//! FastTracker 2 modules (.xm), as Mr.H's XM.TXT describes them: a header, packed patterns, then each
//! instrument with its envelopes and its samples, stored as the differences from one sample to the next.

use super::{Envelope, Format, Instrument, Loop, NOTE_OFF, Pattern, Sample, Song, Vol, mod_fx, mod_shown, text, u16_le, u32_le};

fn volume_column(v: u8) -> Vol {
    let (x, y) = (v >> 4, v & 15);
    match x {
        1..=4 => Vol::Set(v - 0x10),
        5 if y == 0 => Vol::Set(64),
        6 => Vol::SlideDown(y),
        7 => Vol::SlideUp(y),
        8 => Vol::FineDown(y),
        9 => Vol::FineUp(y),
        10 => Vol::VibratoSpeed(y),
        11 => Vol::VibratoDepth(y),
        12 => Vol::Pan(y * 17),
        13 => Vol::PanLeft(y),
        14 => Vol::PanRight(y),
        15 => Vol::TonePorta(y << 4),
        _ => Vol::None,
    }
}

fn envelope(h: &[u8], points_at: usize, count: u8, sustain: u8, loop_start: u8, loop_end: u8, kind: u8) -> Envelope {
    let count = usize::from(count.min(12));
    let points: Vec<(u16, u8)> = (0..count).map(|i| (u16_le(h, points_at + i * 4), u16_le(h, points_at + i * 4 + 2).min(64) as u8)).collect();
    let within = |p: u8| usize::from(p) < points.len();
    Envelope {
        on: kind & 1 != 0 && !points.is_empty(),
        sustain: (kind & 2 != 0 && within(sustain)).then_some((usize::from(sustain), usize::from(sustain))),
        looped: (kind & 4 != 0 && within(loop_start) && within(loop_end) && loop_start <= loop_end).then_some((usize::from(loop_start), usize::from(loop_end))),
        points,
    }
}

pub fn load(bytes: &[u8]) -> Result<Song, String> {
    if bytes.len() < 80 || !bytes.starts_with(b"Extended Module: ") {
        return Err("This isn't a FastTracker 2 module: it doesn't start \u{201C}Extended Module:\u{201D}.".into());
    }
    let header_size = u32_le(bytes, 60) as usize;
    let length = usize::from(u16_le(bytes, 64));
    let channels = usize::from(u16_le(bytes, 68)).clamp(1, 64);
    let pattern_count = usize::from(u16_le(bytes, 70));
    let instrument_count = usize::from(u16_le(bytes, 72));
    let mut song = Song::new(Format::Xm, channels);
    song.title = text(&bytes[17..37]);
    song.tracker = text(&bytes[38..58]);
    song.restart = usize::from(u16_le(bytes, 66));
    song.linear = u16_le(bytes, 74) & 1 != 0;
    song.speed = u16_le(bytes, 76).clamp(1, 31) as u8;
    song.tempo = u16_le(bytes, 78).clamp(32, 255) as u8;
    song.orders = bytes.get(80..80 + length.min(256)).unwrap_or(&[]).to_vec();
    if song.restart >= song.orders.len() {
        song.restart = 0;
    }
    let mut at = 60 + header_size;
    for _ in 0..pattern_count {
        let header = u32_le(bytes, at) as usize;
        let rows = usize::from(u16_le(bytes, at + 5)).clamp(1, 256);
        let packed = usize::from(u16_le(bytes, at + 7));
        let start = at + header;
        let end = (start + packed).min(bytes.len());
        let mut pattern = Pattern::empty(rows, channels);
        let mut i = start;
        if packed > 0 {
            'cells: for cell in pattern.cells.iter_mut() {
                if i >= end {
                    break 'cells;
                }
                let first = bytes[i];
                let (mut note, mut instrument, mut volume, mut effect, mut param) = (0, 0, 0, 0, 0);
                let mut take = || {
                    let b = bytes.get(i).copied().unwrap_or(0);
                    i += 1;
                    b
                };
                if first & 0x80 != 0 {
                    take();
                    if first & 1 != 0 {
                        note = take();
                    }
                    if first & 2 != 0 {
                        instrument = take();
                    }
                    if first & 4 != 0 {
                        volume = take();
                    }
                    if first & 8 != 0 {
                        effect = take();
                    }
                    if first & 16 != 0 {
                        param = take();
                    }
                } else {
                    (note, instrument, volume, effect, param) = (take(), take(), take(), take(), take());
                }
                cell.note = match note {
                    97 => NOTE_OFF,
                    1..=96 => note,
                    _ => 0,
                };
                cell.instrument = instrument;
                cell.vol = volume_column(volume);
                cell.fx = mod_fx(effect, param, true);
                cell.shown = mod_shown(effect, param);
            }
        }
        song.patterns.push(pattern);
        at = start + packed;
    }
    for _ in 0..instrument_count {
        let size = (u32_le(bytes, at) as usize).max(29);
        let Some(h) = bytes.get(at..) else { break };
        let mut instrument = Instrument { name: text(h.get(4..26).unwrap_or(&[])), keymap: vec![(0, 0); 120], global_volume: 128, ..Instrument::default() };
        let samples = usize::from(u16_le(h, 27));
        if samples == 0 {
            song.instruments.push(instrument);
            at += size;
            continue;
        }
        let sample_header = (u32_le(h, 29) as usize).max(40);
        let first = song.samples.len();
        if h.len() >= 241 {
            for (note, entry) in instrument.keymap.iter_mut().enumerate().take(96) {
                let s = h[33 + note];
                *entry = (note as u8 + 1, if usize::from(s) < samples { (first + usize::from(s) + 1) as u16 } else { 0 });
            }
            instrument.volume = envelope(h, 129, h[225], h[227], h[228], h[229], h[233]);
            instrument.panning = envelope(h, 177, h[226], h[230], h[231], h[232], h[234]);
            instrument.fadeout = f32::from(u16_le(h, 239)) / 32768.0;
        }
        at += size;
        let mut headers = Vec::new();
        for _ in 0..samples {
            headers.push(bytes.get(at..at + 40).unwrap_or(&[0; 40]).to_vec());
            at += sample_header;
        }
        for s in &headers {
            let length = u32_le(s, 0) as usize;
            let kind = s[14];
            let wide = kind & 16 != 0;
            let raw = bytes.get(at.min(bytes.len())..(at + length).min(bytes.len())).unwrap_or(&[]);
            at += length;
            let data: Vec<i16> = if wide {
                let mut old = 0i16;
                raw.as_chunks::<2>().0.iter().map(|p| {
                    old = old.wrapping_add(i16::from_le_bytes([p[0], p[1]]));
                    old
                }).collect()
            } else {
                let mut old = 0i8;
                raw.iter().map(|&b| {
                    old = old.wrapping_add(b as i8);
                    i16::from(old) << 8
                }).collect()
            };
            let unit = if wide { 2 } else { 1 };
            let (loop_start, loop_length) = (u32_le(s, 4) as usize / unit, u32_le(s, 8) as usize / unit);
            let loop_kind = match kind & 3 {
                _ if loop_length == 0 || loop_start >= data.len() => Loop::None,
                1 => Loop::Forward,
                2 | 3 => Loop::PingPong,
                _ => Loop::None,
            };
            song.samples.push(Sample {
                name: text(&s[18..40]),
                loop_kind,
                loop_start: loop_start.min(data.len()),
                loop_end: (loop_start + loop_length).min(data.len()),
                volume: s[12].min(64),
                global_volume: 64,
                pan: Some(s[15]),
                base_rate: 8363.0,
                relative: s[16] as i8,
                finetune: i16::from(s[13] as i8),
                bits: if wide { 16 } else { 8 },
                data,
                ..Sample::default()
            });
        }
        song.instruments.push(instrument);
    }
    Ok(song)
}
