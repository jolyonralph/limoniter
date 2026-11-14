//! HxC Floppy Emulator images (`.hfe`), Jean-François Del Nero's: not flux timings but the bit cells themselves, at
//! one rate for the whole disk. A 512-byte header ("HXCPICFE", or "HXCHFEV3" for version 3; tracks, sides, the
//! track encoding, the bit rate in kbit/s, the interface mode, where the track list is) is followed by a list of
//! each track's place and length, in 512-byte blocks and bytes. A track's two sides are interleaved 256 bytes at a
//! time, and each byte's cells are kept lowest bit first.
//!
//! Version 3 adds opcodes among the cells, bytes from &F0 up as read: a change of rate (in 36 MHz ticks a cell),
//! a shortened byte, random ("weak") cells, the index. FM and Apple GCR disks are kept at twice their rate, each cell
//! written as two; giving the cells to the same loop as real flux, as gaps between 1s, undoes that by itself.

use super::super::Source;
use super::Flux;
use std::sync::Arc;

pub fn looks_like(head: &[u8]) -> bool {
    head.len() >= 20 && (head.starts_with(b"HXCPICFE") || head.starts_with(b"HXCHFEV3")) && head[9] > 0 && (1..=2).contains(&head[10])
}

pub struct Hfe {
    source: Arc<dyn Source>,
    version: u8,
    sides: u8,
    /// Each cylinder's first block and its length in bytes, both sides together.
    cylinders: Vec<(u64, usize)>,
    /// Nanoseconds a cell, from the header's rate.
    cell: f64,
    pub bit_rate: u16,
    pub encoding: u8,
    pub interface: u8,
}

impl Hfe {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let header = source.read_vec(0, 32.min(source.len() as usize))?;
        if !looks_like(&header) {
            return Err("This isn't an HxC (HFE) image.".into());
        }
        let version = if header.starts_with(b"HXCHFEV3") { 3 } else { 1 };
        let (count, sides, encoding) = (usize::from(header[9]), header[10], header[11]);
        let bit_rate = u16::from_le_bytes([header[12], header[13]]);
        let interface = header[16];
        let list = u64::from(u16::from_le_bytes([header[18], header[19]])) * 512;
        if bit_rate == 0 || bit_rate > 2000 {
            return Err(format!("This HFE image gives a bit rate of {bit_rate} kbit/s, which no floppy disk has."));
        }
        if list + count as u64 * 4 > source.len() {
            return Err("This HFE image's track list is past its end, so it's damaged.".into());
        }
        let cylinders = source
            .read_vec(list, count * 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| (u64::from(u16::from_le_bytes([c[0], c[1]])) * 512, usize::from(u16::from_le_bytes([c[2], c[3]]))))
            .collect();
        // The rate is of data bits; MFM has two cells a bit.
        let cell = 500_000.0 / f64::from(bit_rate);
        Ok(Self { source, version, sides, cylinders, cell, bit_rate, encoding, interface })
    }

    pub fn places(&self) -> Vec<(u8, u8)> {
        (0..self.cylinders.len()).flat_map(|c| (0..self.sides).map(move |s| (c as u8, s))).collect()
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    /// One side of a cylinder as gaps between its 1 cells, the cells' timing taken from the rate.
    pub fn flux(&self, place: (u8, u8)) -> Result<Flux, String> {
        let (start, length) = *self.cylinders.get(usize::from(place.0)).ok_or("That track isn't in the image.")?;
        let blocks = length.div_ceil(512);
        if start + blocks as u64 * 512 > self.source.len() {
            return Err(format!("Track {} runs past the end of the HFE image, so it's damaged.", place.0));
        }
        let raw = self.source.read_vec(start, blocks * 512)?;
        let mut bytes: Vec<u8> = raw.chunks(512).flat_map(|block| block[usize::from(place.1) * 256..][..256].iter().map(|b| b.reverse_bits())).collect();
        bytes.truncate(length / 2);
        // The track is a loop: it's given twice, so a sector written across the index still reads whole.
        let bytes = [bytes.clone(), bytes].concat();
        let mut gaps = Vec::new();
        let mut since = 0.0f64;
        let mut cell = self.cell;
        let mut push_cells = |byte: u8, count: u32, cell: f64, gaps: &mut Vec<f32>| {
            for i in (8 - count..8).rev() {
                since += cell;
                if (byte >> i) & 1 == 1 {
                    gaps.push(since as f32);
                    since = 0.0;
                }
            }
        };
        if self.version == 3 {
            // A track that sets its rate after its start was meant to begin at the rate it ends with.
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    0xF2 if i + 1 < bytes.len() => {
                        if bytes[i + 1] > 0 {
                            cell = f64::from(bytes[i + 1]) * 1000.0 / 36.0;
                            break;
                        }
                        i += 2;
                    }
                    0xF3 => i += 3,
                    _ => i += 1,
                }
            }
            let mut i = 0;
            while i < bytes.len() {
                let op = bytes[i];
                i += 1;
                match op {
                    0xF0 | 0xF1 => {}
                    0xF2 => {
                        if let Some(&rate) = bytes.get(i).filter(|r| **r > 0) {
                            cell = f64::from(rate) * 1000.0 / 36.0;
                        }
                        i += 1;
                    }
                    // A shortened byte: the count of cells to drop, then the byte, its cells from the top.
                    0xF3 => {
                        let (Some(&drop), Some(&byte)) = (bytes.get(i), bytes.get(i + 1)) else {
                            break;
                        };
                        i += 2;
                        if (1..8).contains(&drop) {
                            let byte = if byte == 0xF4 { 0 } else { byte };
                            push_cells(byte << drop, 8 - u32::from(drop), cell, &mut gaps);
                        }
                    }
                    // Weak cells: random on a real disk, taken as no changes here.
                    0xF4 => push_cells(0, 8, cell, &mut gaps),
                    byte => push_cells(byte, 8, cell, &mut gaps),
                }
            }
        } else {
            for &byte in &bytes {
                push_cells(byte, 8, cell, &mut gaps);
            }
        }
        Ok(Flux { gaps, revolutions: 1 })
    }
}

#[cfg(test)]
pub(super) mod tests {
    /// A version 1 HFE image of cylinders' cells (one entry a side), at a rate in kbit/s.
    pub fn hfe_of(cylinders: &[Vec<Vec<u8>>], rate: u16) -> Vec<u8> {
        let sides = cylinders.iter().map(Vec::len).max().unwrap_or(1);
        let mut out = vec![0xFFu8; 1024];
        out[..8].copy_from_slice(b"HXCPICFE");
        out[8] = 0;
        out[9] = cylinders.len() as u8;
        out[10] = sides as u8;
        out[11] = 0;
        out[12..14].copy_from_slice(&rate.to_le_bytes());
        out[14..16].copy_from_slice(&300u16.to_le_bytes());
        out[16] = 7;
        out[18..20].copy_from_slice(&1u16.to_le_bytes());
        for (c, cylinder) in cylinders.iter().enumerate() {
            let packed: Vec<Vec<u8>> = cylinder.iter().map(|cells| cells.chunks(8).map(|eight| eight.iter().enumerate().fold(0u8, |b, (i, c)| b | ((c & 1) << i))).collect()).collect();
            let longest = packed.iter().map(Vec::len).max().unwrap_or(0);
            let blocks = longest.div_ceil(256);
            let start = out.len() / 512;
            out[512 + c * 4..514 + c * 4].copy_from_slice(&(start as u16).to_le_bytes());
            out[514 + c * 4..516 + c * 4].copy_from_slice(&((longest * 2) as u16).to_le_bytes());
            for block in 0..blocks {
                for side in 0..2 {
                    let part = packed.get(side).map(|p| p.get(block * 256..((block + 1) * 256).min(p.len())).unwrap_or_default()).unwrap_or_default();
                    out.extend(part);
                    out.extend(std::iter::repeat_n(0x88u8, 256 - part.len()));
                }
            }
        }
        out
    }
}
