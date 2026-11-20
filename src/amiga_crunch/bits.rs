//! The pieces the Amiga crunchers' decoders are built from: streams of bytes read forwards or backwards, bits taken
//! from them a few at a time (lowest first or highest first), output written forwards or backwards with copies from
//! what's already been written, and the fixed Huffman and variable-length codes the formats use. These follow the
//! helpers of Teemu Suutari's ancient library (BSD 2-clause licence; see data/ancient-license.txt), which the
//! decoders here are ported from, so that each decoder reads as its original does.

/// Something in the crunched data doesn't add up: it's damaged, cut short, or isn't what its header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bad;

pub type R<T> = Result<T, Bad>;

/// The lowest `count` bits of `value` in the opposite order.
pub fn reverse(value: u32, count: u32) -> u32 {
    if count == 0 { 0 } else { value.reverse_bits() >> (32 - count) }
}

/// A big-endian long, if there are four bytes there.
pub fn be32(bytes: &[u8], at: usize) -> R<u32> {
    let b = bytes.get(at..at.checked_add(4).ok_or(Bad)?).ok_or(Bad)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// A big-endian word, if there are two bytes there.
pub fn be16(bytes: &[u8], at: usize) -> R<u32> {
    let b = bytes.get(at..at.checked_add(2).ok_or(Bad)?).ok_or(Bad)?;
    Ok(u32::from(u16::from_be_bytes([b[0], b[1]])))
}

/// Bytes read from the end of a stretch towards its start, as most crunchers' decrunchers read.
pub struct Backward<'a> {
    bytes: &'a [u8],
    pub at: usize,
    floor: usize,
}

impl<'a> Backward<'a> {
    /// The stretch from `start` to `end`, read from `end`.
    pub fn new(bytes: &'a [u8], start: usize, end: usize) -> R<Self> {
        if start > end || end > bytes.len() {
            return Err(Bad);
        }
        Ok(Self { bytes, at: end, floor: start })
    }

    pub fn byte(&mut self) -> R<u8> {
        if self.at <= self.floor {
            return Err(Bad);
        }
        self.at -= 1;
        Ok(self.bytes[self.at])
    }

    /// The two bytes before here, as a big-endian word.
    pub fn be16(&mut self) -> R<u32> {
        let low = u32::from(self.byte()?);
        Ok(u32::from(self.byte()?) << 8 | low)
    }

    /// The four bytes before here, as a big-endian long.
    pub fn be32(&mut self) -> R<u32> {
        let mut value = 0;
        for shift in [0, 8, 16, 24] {
            value |= u32::from(self.byte()?) << shift;
        }
        Ok(value)
    }

    pub fn left(&self) -> usize {
        self.at - self.floor
    }
}

/// Bytes read from the start of a stretch towards its end.
pub struct Forward<'a> {
    bytes: &'a [u8],
    pub at: usize,
    end: usize,
}

impl<'a> Forward<'a> {
    pub fn new(bytes: &'a [u8], start: usize, end: usize) -> R<Self> {
        if start > end || end > bytes.len() {
            return Err(Bad);
        }
        Ok(Self { bytes, at: start, end })
    }

    pub fn byte(&mut self) -> R<u8> {
        if self.at >= self.end {
            return Err(Bad);
        }
        self.at += 1;
        Ok(self.bytes[self.at - 1])
    }

    pub fn be16(&mut self) -> R<u32> {
        Ok(u32::from(self.byte()?) << 8 | u32::from(self.byte()?))
    }

    pub fn be32(&mut self) -> R<u32> {
        Ok(self.be16()? << 16 | self.be16()?)
    }
}

fn mask(count: u32) -> u64 {
    (1u64 << count) - 1
}

/// Bits kept from the last word read, handed out lowest first: the first bit read becomes the lowest of the value.
#[derive(Default)]
pub struct Lsb {
    pub content: u32,
    pub length: u32,
}

impl Lsb {
    pub fn reset(&mut self, content: u32, length: u32) {
        self.content = content;
        self.length = length;
    }

    /// `count` bits (at most 32), a fresh word fetched whenever these run out.
    pub fn read(&mut self, mut count: u32, mut fetch: impl FnMut() -> R<(u32, u32)>) -> R<u32> {
        if count > 32 {
            return Err(Bad);
        }
        let (mut value, mut position) = (0u64, 0);
        while count > 0 {
            if self.length == 0 {
                (self.content, self.length) = fetch()?;
            }
            let n = count.min(self.length);
            value |= (u64::from(self.content) & mask(n)) << position;
            self.content = (u64::from(self.content) >> n) as u32;
            self.length -= n;
            count -= n;
            position += n;
        }
        Ok(value as u32)
    }
}

/// Bits kept from the last word read, handed out highest first: the first bit read becomes the highest of the value.
#[derive(Default)]
pub struct Msb {
    pub content: u32,
    pub length: u32,
}

impl Msb {
    pub fn reset(&mut self, content: u32, length: u32) {
        self.content = content;
        self.length = length;
    }

    pub fn read(&mut self, mut count: u32, mut fetch: impl FnMut() -> R<(u32, u32)>) -> R<u32> {
        if count > 32 {
            return Err(Bad);
        }
        let mut value = 0u64;
        while count > 0 {
            if self.length == 0 {
                (self.content, self.length) = fetch()?;
            }
            let n = count.min(self.length);
            self.length -= n;
            value = value << n | (u64::from(self.content) >> self.length) & mask(n);
            count -= n;
        }
        Ok(value as u32)
    }
}

/// Output written from its end towards its start, as the decrunchers write it, with copies of what's already been
/// written (which, written backwards, lies after the place being written).
pub struct BackOut {
    pub bytes: Vec<u8>,
    at: usize,
}

impl BackOut {
    pub fn new(size: usize) -> Self {
        Self { bytes: vec![0; size], at: size }
    }

    pub fn done(&self) -> bool {
        self.at == 0
    }

    pub fn byte(&mut self, value: u32) -> R<()> {
        if self.at == 0 {
            return Err(Bad);
        }
        self.at -= 1;
        self.bytes[self.at] = value as u8;
        Ok(())
    }

    pub fn copy(&mut self, distance: usize, count: usize) -> R<()> {
        if distance == 0 || count > self.at || self.at + distance > self.bytes.len() {
            return Err(Bad);
        }
        for _ in 0..count {
            self.bytes[self.at - 1] = self.bytes[self.at + distance - 1];
            self.at -= 1;
        }
        Ok(())
    }
}

/// Output written from its start, with copies from what's already been written.
pub struct ForwardOut {
    pub bytes: Vec<u8>,
    pub at: usize,
}

impl ForwardOut {
    pub fn new(size: usize) -> Self {
        Self { bytes: vec![0; size], at: 0 }
    }

    pub fn done(&self) -> bool {
        self.at == self.bytes.len()
    }

    pub fn byte(&mut self, value: u32) -> R<()> {
        let slot = self.bytes.get_mut(self.at).ok_or(Bad)?;
        *slot = value as u8;
        self.at += 1;
        Ok(())
    }

    pub fn copy(&mut self, distance: usize, count: usize) -> R<()> {
        if distance == 0 || distance > self.at || self.at + count > self.bytes.len() {
            return Err(Bad);
        }
        for _ in 0..count {
            self.bytes[self.at] = self.bytes[self.at - distance];
            self.at += 1;
        }
        Ok(())
    }
}

/// A prefix code, as a tree: each code a number of bits read one at a time, highest first, to a value.
pub struct Huffman<T: Copy + Default> {
    /// Each node's children by the next bit (0 for none), or its value at a leaf.
    nodes: Vec<(u32, u32, T)>,
}

impl<T: Copy + Default> Huffman<T> {
    pub fn empty() -> Self {
        Self { nodes: Vec::new() }
    }

    /// A code from its (length in bits, bits, value) entries; Bad if they don't make a prefix code.
    pub fn new(codes: &[(u32, u32, T)]) -> R<Self> {
        let mut tree = Self::empty();
        for &(length, code, value) in codes {
            tree.insert(length, code, value)?;
        }
        Ok(tree)
    }

    pub fn insert(&mut self, length: u32, code: u32, value: T) -> R<()> {
        let mut i = 0usize;
        let mut count = self.nodes.len();
        for bit in (0..=length).rev() {
            let one = bit > 0 && (code >> (bit - 1)) & 1 == 1;
            if i != count {
                let (left, right, _) = self.nodes[i];
                if bit == 0 || (left == 0 && right == 0) {
                    return Err(Bad);
                }
                let next = if one { right } else { left };
                if next == 0 {
                    if one {
                        self.nodes[i].1 = count as u32;
                    } else {
                        self.nodes[i].0 = count as u32;
                    }
                    i = count;
                } else {
                    i = next as usize;
                }
            } else {
                let child = count as u32 + 1;
                self.nodes.push((if bit > 0 && !one { child } else { 0 }, if bit > 0 && one { child } else { 0 }, if bit > 0 { T::default() } else { value }));
                count += 1;
                i += 1;
            }
        }
        Ok(())
    }

    pub fn decode(&self, mut bit: impl FnMut() -> R<u32>) -> R<T> {
        if self.nodes.is_empty() {
            return Err(Bad);
        }
        let mut i = 0usize;
        while self.nodes[i].0 != 0 || self.nodes[i].1 != 0 {
            let next = if bit()? != 0 { self.nodes[i].1 } else { self.nodes[i].0 };
            if next == 0 {
                return Err(Bad);
            }
            i = next as usize;
        }
        Ok(self.nodes[i].2)
    }
}

/// Numbers in ranges, each range a number of bits read after its index is known: the first range from 0, each
/// next one following on from the last (or, given as a negative width, starting again from 0 after one that ends
/// at 2 to the power of it).
pub struct Ranges {
    widths: Vec<u32>,
    offsets: Vec<u32>,
}

impl Ranges {
    pub fn new(widths: &[i32]) -> Self {
        let (mut offsets, mut length) = (Vec::new(), 0u32);
        for &w in widths {
            if w < 0 {
                offsets.push(0);
                length = 1 << -w;
            } else {
                offsets.push(length);
                length += 1 << w;
            }
        }
        Self { widths: widths.iter().map(|w| w.unsigned_abs()).collect(), offsets }
    }

    pub fn decode(&self, mut bits: impl FnMut(u32) -> R<u32>, index: u32) -> R<u32> {
        let i = index as usize;
        let width = *self.widths.get(i).ok_or(Bad)?;
        Ok(self.offsets[i] + bits(width)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_come_lowest_or_highest_first() {
        let mut lsb = Lsb::default();
        let mut words = vec![(0b1011_0110u32, 8u32)].into_iter();
        assert_eq!(lsb.read(3, || words.next().ok_or(Bad)), Ok(0b110));
        assert_eq!(lsb.read(5, || words.next().ok_or(Bad)), Ok(0b10110));
        let mut msb = Msb::default();
        let mut words = vec![(0b1011_0110u32, 8u32), (0xFF, 8)].into_iter();
        assert_eq!(msb.read(3, || words.next().ok_or(Bad)), Ok(0b101));
        assert_eq!(msb.read(7, || words.next().ok_or(Bad)), Ok(0b101_1011));
        assert_eq!(reverse(0b0011, 4), 0b1100);
        assert_eq!(reverse(0b011, 3), 0b110);
        assert_eq!(reverse(1, 32), 0x8000_0000);
    }

    #[test]
    fn a_prefix_code_is_read_bit_by_bit() {
        let code = Huffman::new(&[(1, 0b0, 0u8), (2, 0b10, 1), (2, 0b11, 2)]).unwrap();
        let mut bits = [1, 1, 0, 1, 0].into_iter();
        let mut next = || bits.next().ok_or(Bad);
        assert_eq!(code.decode(&mut next), Ok(2));
        assert_eq!(code.decode(&mut next), Ok(0));
        assert_eq!(code.decode(&mut next), Ok(1));
        // A code that is a prefix of another isn't one.
        assert!(Huffman::new(&[(1, 0b0, 0u8), (2, 0b01, 1)]).is_err());
    }

    #[test]
    fn copies_reach_back_into_what_is_written() {
        let mut out = BackOut::new(4);
        out.byte(7).unwrap();
        out.byte(9).unwrap();
        out.copy(2, 2).unwrap();
        assert!(out.done());
        assert_eq!(out.bytes, [9, 7, 9, 7]);
        let mut out = ForwardOut::new(5);
        out.byte(1).unwrap();
        out.copy(1, 4).unwrap();
        assert_eq!(out.bytes, [1; 5]);
        assert!(ForwardOut::new(2).copy(1, 1).is_err());
        let ranges = Ranges::new(&[1, 2, 4, 8]);
        assert_eq!(ranges.decode(|_| Ok(1), 2), Ok(7));
    }
}
