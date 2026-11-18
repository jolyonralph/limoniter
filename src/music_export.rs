//! Music saved as sound: any module or sound file that plays, rendered by the same player and written as a WAV;
//! and a module's samples taken out, each as an Amiga IFF 8SVX sound with its loop.

use crate::tracker::{Loop, Sample, play::Player};
use std::sync::Arc;

/// The rate music is saved at.
pub const RATE: u32 = 44_100;

/// The longest that's saved, for a song that never ends.
const LONGEST: f64 = 60.0 * 60.0;

/// Whether bytes play as a module (rather than as a sound file).
pub fn is_module(bytes: &[u8], name: &str) -> bool {
    crate::tracker::pc_format(bytes).is_some() || (!crate::sound::recognised(bytes, name) && (crate::protracker::has_module_tag(bytes) || crate::protracker::named_as_module(name)))
}

/// A module or sound file played through once, left and right one after the other, at `RATE`. Channels marked
/// in `silenced` are left out, as they're switched off in the player.
pub fn render(bytes: &[u8], name: &str, silenced: &[bool]) -> Result<Vec<f32>, String> {
    let mut out = Vec::new();
    let mut block = vec![0f32; 2 * 4096];
    // How long it played, to cut the last block's silence after its end.
    let seconds = if is_module(bytes, name) {
        let song = Arc::new(crate::tracker::load(bytes, name)?);
        let mut player = Player::new(song, RATE);
        for (c, &off) in silenced.iter().enumerate() {
            if let Some(s) = player.silenced.get_mut(c) {
                *s = off;
            }
        }
        while !player.ended && player.time < LONGEST {
            player.render(&mut block);
            out.extend_from_slice(&block);
        }
        player.time
    } else {
        let mut player = crate::sound::SoundPlayer::new(bytes.to_vec(), name, RATE)?;
        while !player.ended() && player.time() < LONGEST {
            player.render(&mut block);
            out.extend_from_slice(&block);
        }
        player.time()
    };
    out.truncate(((seconds * f64::from(RATE)).round() as usize * 2).min(out.len()));
    Ok(out)
}

/// Stereo samples as a 16-bit WAV file.
pub fn wav(frames: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (frames.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for &s in frames {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    b
}

/// A module's sample as an IFF 8SVX sound: 8-bit, at the rate it plays its middle note at, its loop kept as the
/// part repeated after the one-shot part, and its name and volume.
pub fn eight_svx(sample: &Sample) -> Vec<u8> {
    let data: Vec<u8> = sample.data.iter().map(|&s| (s >> 8) as i8 as u8).collect();
    let (one_shot, repeat) = match sample.loop_kind {
        Loop::None => (data.len(), 0),
        _ => (sample.loop_start.min(data.len()), sample.loop_end.min(data.len()).saturating_sub(sample.loop_start)),
    };
    let rate = (sample.base_rate.round() as u32).clamp(1, 65_535) as u16;
    let mut vhdr = Vec::new();
    vhdr.extend_from_slice(&(one_shot as u32).to_be_bytes());
    vhdr.extend_from_slice(&(repeat as u32).to_be_bytes());
    vhdr.extend_from_slice(&0u32.to_be_bytes());
    vhdr.extend_from_slice(&rate.to_be_bytes());
    vhdr.push(1);
    vhdr.push(0);
    // Its volume as a fixed-point fraction: 0x10000 is full.
    vhdr.extend_from_slice(&(u32::from(sample.volume.min(64)) * 0x10000 / 64).to_be_bytes());
    let mut body = Vec::new();
    let chunk = |body: &mut Vec<u8>, id: &[u8], bytes: &[u8]| {
        body.extend_from_slice(id);
        body.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        body.extend_from_slice(bytes);
        if bytes.len() % 2 == 1 {
            body.push(0);
        }
    };
    chunk(&mut body, b"VHDR", &vhdr);
    if !sample.name.trim().is_empty() {
        chunk(&mut body, b"NAME", sample.name.trim().as_bytes());
    }
    chunk(&mut body, b"BODY", &data);
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
    out.extend_from_slice(b"8SVX");
    out.extend(body);
    out
}

/// Each of a module's samples that has any sound, as a file name and its 8SVX bytes, numbered as the module
/// numbers them.
pub fn samples(bytes: &[u8], name: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let song = crate::tracker::load(bytes, name)?;
    Ok(song
        .samples
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.data.is_empty())
        .map(|(i, s)| {
            let base: String = s.name.trim().chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
            let base = base.trim().trim_matches('.');
            let file = if base.is_empty() { format!("{:02}.8svx", i + 1) } else { format!("{:02} {base}.8svx", i + 1) };
            (file, eight_svx(s))
        })
        .collect())
}

#[cfg(test)]
mod tests;
