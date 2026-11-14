//! SuperCard Pro images (`.scp`), as Jim Drew's specification lays them out, and as the Greaseweazle and other flux
//! readers write them too. A 16-byte header ("SCP", version, disk type, revolutions, first and last track, flags,
//! the width of each flux value, which sides, the timing resolution, a checksum) is followed by a table of 168 track
//! offsets. Each track is "TRK" and its number, then for each revolution its length in time, how many flux values it
//! has and where they start, then the values: big-endian 16-bit counts of 25 ns ticks between changes, a 0 adding
//! 65536 to the next.

use super::super::Source;
use super::Flux;
use std::sync::Arc;

pub fn looks_like(head: &[u8]) -> bool {
    head.len() >= 0x10 && head.starts_with(b"SCP") && head[5] >= 1 && matches!(head[9], 0 | 16) && head[10] <= 2
}

pub struct Scp {
    source: Arc<dyn Source>,
    /// Each track's place on the disk and where its header is.
    tracks: Vec<((u8, u8), u64)>,
    revolutions: usize,
    tick: f64,
    pub disk_type: u8,
    pub version: u8,
}

impl Scp {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let header = source.read_vec(0, (0x10 + 168 * 4).min(source.len() as usize))?;
        if !looks_like(&header) {
            return Err("This isn't a SuperCard Pro image.".into());
        }
        let revolutions = usize::from(header[5]);
        let tick = 25.0 * f64::from(header[11]) + 25.0;
        let mut offsets: Vec<(usize, u64)> = header[0x10..].as_chunks::<4>().0.iter().map(|c| u64::from(u32::from_le_bytes(*c))).enumerate().filter(|(_, o)| *o != 0).collect();
        // Some writers make the table shorter and start the first track straight after it: an entry inside the
        // table is that track's data, not an offset.
        if let Some(first) = offsets.iter().map(|(_, o)| *o).filter(|o| *o >= 0x10).min()
            && first < 0x10 + 168 * 4
        {
            let entries = (first as usize - 0x10) / 4;
            offsets.retain(|(n, _)| *n < entries);
        }
        offsets.retain(|(_, o)| *o + 4 <= source.len());
        if offsets.is_empty() {
            return Err("This SuperCard Pro image has no tracks in it.".into());
        }
        // Each track number is cylinder × 2 + side, unless the header says one side only and the image numbers its
        // tracks one after another, as some older writers did.
        let even = offsets.iter().any(|(n, _)| n % 2 == 0);
        let odd = offsets.iter().any(|(n, _)| n % 2 == 1);
        let consecutive = header[10] != 0 && even && odd;
        let tracks = offsets
            .into_iter()
            .filter_map(|(n, offset)| {
                let place = if consecutive { (u8::try_from(n).ok()?, header[10] - 1) } else { ((n / 2) as u8, (n % 2) as u8) };
                Some((place, offset))
            })
            .collect();
        Ok(Self { source, tracks, revolutions, tick, disk_type: header[4], version: header[3] })
    }

    pub fn places(&self) -> Vec<(u8, u8)> {
        self.tracks.iter().map(|(p, _)| *p).collect()
    }

    pub fn revolutions(&self) -> usize {
        self.revolutions
    }

    pub fn tick(&self) -> f64 {
        self.tick
    }

    /// One track's flux, every revolution one after another.
    pub fn flux(&self, place: (u8, u8)) -> Result<Flux, String> {
        let offset = self.tracks.iter().find(|(p, _)| *p == place).map(|(_, o)| *o).ok_or("That track isn't in the image.")?;
        let table = self.source.read_vec(offset, (4 + 12 * self.revolutions).min((self.source.len() - offset) as usize))?;
        if !table.starts_with(b"TRK") {
            return Err(format!("Track {} side {}'s header is missing, so the image is damaged.", place.0, place.1));
        }
        let mut gaps = Vec::new();
        let mut revolutions = 0;
        for entry in table[4..].as_chunks::<12>().0 {
            let count = u64::from(u32::from_le_bytes(entry[4..8].try_into().unwrap_or_default()));
            let start = offset + u64::from(u32::from_le_bytes(entry[8..12].try_into().unwrap_or_default()));
            if count == 0 {
                continue;
            }
            // A revolution is at most a few hundred thousand changes; anything past the file's end is damage.
            if count > 4_000_000 || start + count * 2 > self.source.len() {
                return Err(format!("Track {} side {}'s flux runs past the end of the image, so it's damaged.", place.0, place.1));
            }
            if gaps.len() as u64 + count > 16_000_000 {
                return Err(format!("Track {} side {} has more flux than any disk holds, so the image is damaged.", place.0, place.1));
            }
            let values = self.source.read_vec(start, count as usize * 2)?;
            let mut carry = 0u64;
            for pair in values.as_chunks::<2>().0 {
                let value = u64::from(u16::from_be_bytes(*pair));
                if value == 0 {
                    carry += 65536;
                    continue;
                }
                gaps.push(((carry + value) as f64 * self.tick) as f32);
                carry = 0;
            }
            revolutions += 1;
        }
        Ok(Flux { gaps, revolutions })
    }
}

#[cfg(test)]
pub(super) mod tests {
    /// A SuperCard Pro image of tracks given as gaps in nanoseconds, each the same flux for every revolution.
    pub fn scp_of(tracks: &[((u8, u8), Vec<f32>)], revolutions: u8) -> Vec<u8> {
        let mut out = vec![0u8; 0x10 + 168 * 4];
        out[..3].copy_from_slice(b"SCP");
        out[3] = 0x22;
        out[4] = 0x80;
        out[5] = revolutions;
        out[8] = 1;
        for ((cylinder, head), gaps) in tracks {
            let number = usize::from(*cylinder) * 2 + usize::from(*head);
            let at = out.len();
            out[0x10 + number * 4..0x14 + number * 4].copy_from_slice(&(at as u32).to_le_bytes());
            let mut values = Vec::new();
            for gap in gaps {
                let mut ticks = (f64::from(*gap) / 25.0).round() as u64;
                while ticks >= 65536 {
                    values.extend([0, 0]);
                    ticks -= 65536;
                }
                values.extend((ticks.max(1) as u16).to_be_bytes());
            }
            out.extend(b"TRK");
            out.push(number as u8);
            let table = 4 + 12 * usize::from(revolutions);
            for r in 0..usize::from(revolutions) {
                let time: f64 = gaps.iter().map(|g| f64::from(*g) / 25.0).sum();
                out.extend((time as u32).to_le_bytes());
                out.extend(((values.len() / 2) as u32).to_le_bytes());
                out.extend(((table + r * values.len()) as u32).to_le_bytes());
            }
            for _ in 0..revolutions {
                out.extend(&values);
            }
        }
        out
    }
}
