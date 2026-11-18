//! Scream Tracker 3 modules (.s3m), as ST3's TECH.DOC lays them out: a header with the order and the places
//! (in 16-byte paragraphs) of each instrument and pattern, packed patterns of 64 rows, and 8- or 16-bit samples.

use super::{Cell, Format, Loop, NOTE_CUT, Pattern, Sample, Song, Vol, st_fx, st_shown, text, u16_le, u32_le, widen, widen16};

pub fn load(bytes: &[u8]) -> Result<Song, String> {
    if bytes.len() < 0x60 || bytes.get(44..48) != Some(b"SCRM") {
        return Err(crate::i18n::tr("This isn't a Scream Tracker 3 module: there's no SCRM at byte 44.").into());
    }
    let orders = usize::from(u16_le(bytes, 0x20));
    let instruments = usize::from(u16_le(bytes, 0x22));
    let patterns = usize::from(u16_le(bytes, 0x24));
    let flags = u16_le(bytes, 0x26);
    let made_with = u16_le(bytes, 0x28);
    let signed = u16_le(bytes, 0x2A) == 1;
    let settings = &bytes[0x40..0x60];
    // Channels 16 and up are AdLib's, which aren't played; the rest are kept in their places.
    let used = |c: usize| settings[c] < 16;
    let channels = (0..32).rfind(|&c| used(c)).map_or(1, |c| c + 1);
    let mut song = Song::new(Format::S3m, channels);
    song.title = text(&bytes[..28]);
    song.tracker = match made_with >> 12 {
        1 => crate::i18n::tf!("Scream Tracker {}.{:02X}", (made_with >> 8) & 0xF, made_with & 0xFF),
        2 => crate::i18n::tr("Imago Orpheus").into(),
        3 => crate::i18n::tr("Impulse Tracker").into(),
        4 => crate::i18n::tr("Schism Tracker").into(),
        5 => "OpenMPT".into(),
        _ => String::new(),
    };
    song.global_volume = bytes[0x30].min(64) * 2;
    song.speed = if bytes[0x31] == 0 || bytes[0x31] == 255 { 6 } else { bytes[0x31] };
    song.tempo = if bytes[0x32] < 33 { 125 } else { bytes[0x32] };
    let stereo = bytes[0x33] & 0x80 != 0;
    song.fast_slides = made_with == 0x1300 || flags & 0x40 != 0;
    song.shared_memory = true;
    song.orders = bytes.get(0x60..0x60 + orders).ok_or(crate::i18n::tr("The module's order is cut short."))?.to_vec();
    while song.orders.last() == Some(&255) {
        song.orders.pop();
    }
    let pointers = 0x60 + orders;
    let paragraph = |at: usize| usize::from(u16_le(bytes, at)) * 16;
    let pan_table = (bytes[0x35] == 252).then(|| bytes.get(pointers + (instruments + patterns) * 2..pointers + (instruments + patterns) * 2 + 32)).flatten();
    for c in 0..channels {
        song.muted[c] = !used(c);
        let default = if settings[c] & 0x7F < 8 { 0x3 } else { 0xC };
        let pan = match pan_table {
            Some(table) if table[c] & 0x20 != 0 => table[c] & 0x0F,
            _ => default,
        };
        song.pans[c] = if stereo { pan * 17 } else { 128 };
    }
    for i in 0..instruments {
        let at = paragraph(pointers + i * 2);
        let Some(h) = bytes.get(at..at + 0x50) else {
            song.samples.push(Sample::default());
            continue;
        };
        let mut sample = Sample { name: text(&h[0x30..0x4C]), volume: h[0x1C].min(64), global_volume: 64, bits: 8, base_rate: 8363.0, ..Sample::default() };
        if h[0] == 1 {
            let data_at = ((usize::from(h[0x0D]) << 16) | usize::from(u16_le(h, 0x0E))) * 16;
            let length = u32_le(h, 0x10) as usize;
            let (loop_start, loop_end) = (u32_le(h, 0x14) as usize, u32_le(h, 0x18) as usize);
            let sample_flags = h[0x1F];
            let wide = sample_flags & 4 != 0;
            let width = if wide { 2 } else { 1 };
            let raw = bytes.get(data_at.min(bytes.len())..(data_at + length * width).min(bytes.len())).unwrap_or(&[]);
            sample.data = if wide { widen16(raw, signed) } else { widen(raw, signed) };
            sample.bits = if wide { 16 } else { 8 };
            let rate = u32_le(h, 0x20);
            sample.base_rate = if rate == 0 { 8363.0 } else { rate as f32 };
            if sample_flags & 1 != 0 && loop_end > loop_start && loop_start < sample.data.len() {
                sample.loop_kind = Loop::Forward;
                sample.loop_start = loop_start;
                sample.loop_end = loop_end.min(sample.data.len());
            }
        }
        song.samples.push(sample);
    }
    for p in 0..patterns {
        let at = paragraph(pointers + instruments * 2 + p * 2);
        let mut pattern = Pattern::empty(64, channels);
        if at > 0 && at + 2 <= bytes.len() {
            let end = (at + usize::from(u16_le(bytes, at))).min(bytes.len());
            let mut i = at + 2;
            let mut row = 0;
            while row < 64 && i < end {
                let what = bytes[i];
                i += 1;
                if what == 0 {
                    row += 1;
                    continue;
                }
                let channel = usize::from(what & 31);
                let mut cell = Cell::default();
                if what & 0x20 != 0 && i + 2 <= end {
                    cell.note = match bytes[i] {
                        255 => 0,
                        254 => NOTE_CUT,
                        n => ((n >> 4) * 12 + (n & 15) + 1).min(120),
                    };
                    cell.instrument = bytes[i + 1];
                    i += 2;
                }
                if what & 0x40 != 0 && i < end {
                    if bytes[i] <= 64 {
                        cell.vol = Vol::Set(bytes[i]);
                    }
                    i += 1;
                }
                if what & 0x80 != 0 && i + 2 <= end {
                    cell.fx = st_fx(bytes[i], bytes[i + 1], false);
                    cell.shown = st_shown(bytes[i], bytes[i + 1]);
                    i += 2;
                }
                if channel < channels {
                    pattern.cells[row * channels + channel] = cell;
                }
            }
        }
        song.patterns.push(pattern);
    }
    Ok(song)
}
