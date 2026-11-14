//! Atari ST packers, undone: Pack-Ice (its 2.1 files, marked Ice!, and its 2.3 and 2.4 files, marked ICE!) and
//! Atomik 3.5 (ATM5). Demo coders packed music, pictures and data with them, so a packed file opens as what it
//! holds: in a tab, in a disk image's browser, in the preview and for SNDH music. Written here from the formats'
//! own depackers' behaviour, and checked against Hans Wessels' public-domain C depackers and PSG play's on real
//! files. Nothing here packs: packing is left to the tools in Tools, Atari ST.

/// The packers recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packer {
    /// Pack-Ice 2.1, marked Ice!: its bits come 32 at a time.
    Ice21,
    /// Pack-Ice 2.3 and 2.4, marked ICE!: its bits come a byte at a time.
    Ice24,
    /// Atomik 3.5 by Altair, marked ATM5.
    Atomik35,
}

impl Packer {
    pub fn name(self) -> &'static str {
        match self {
            Packer::Ice21 => "Pack-Ice 2.1",
            Packer::Ice24 => "Pack-Ice 2.4",
            Packer::Atomik35 => "Atomik 3.5",
        }
    }
}

/// What a packed file held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unpacked {
    pub packer: Packer,
    pub bytes: Vec<u8>,
    /// How long the packed data was, from its header.
    pub packed_size: u64,
    /// Whether the packer's picture mode had rearranged a 32,000-byte screen to pack it better.
    pub picture: bool,
}

/// The most a packed file may unpack to here: far more than an ST ever had.
const MOST: u64 = 64 << 20;

fn long(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Which packer's mark the bytes start with, if any.
pub fn packer(bytes: &[u8]) -> Option<Packer> {
    if bytes.len() < 12 {
        return None;
    }
    match &bytes[..4] {
        b"ICE!" => Some(Packer::Ice24),
        b"Ice!" => Some(Packer::Ice21),
        b"ATM5" => Some(Packer::Atomik35),
        _ => None,
    }
}

/// The packer, the packed size and the unpacked size its header gives, without unpacking.
pub fn header(bytes: &[u8]) -> Option<(Packer, u64, u64)> {
    let packer = packer(bytes)?;
    let (packed, unpacked) = match packer {
        Packer::Ice21 | Packer::Ice24 => (u64::from(long(bytes, 4)), u64::from(long(bytes, 8))),
        Packer::Atomik35 => (u64::from(long(bytes, 8)) + 12, u64::from(long(bytes, 4))),
    };
    Some((packer, packed, unpacked))
}

/// Unpacks a packed file, or says why it can't be. `None` if it isn't packed with anything known.
pub fn unpack(bytes: &[u8]) -> Option<Result<Unpacked, String>> {
    let (packer, packed, unpacked) = header(bytes)?;
    Some(unpack_with(bytes, packer, packed, unpacked))
}

/// The bytes a file holds: unpacked if it's packed and unpacks cleanly, else as they are.
pub fn contents(bytes: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match unpack(bytes) {
        Some(Ok(unpacked)) => std::borrow::Cow::Owned(unpacked.bytes),
        _ => std::borrow::Cow::Borrowed(bytes),
    }
}

fn unpack_with(bytes: &[u8], packer: Packer, packed: u64, unpacked: u64) -> Result<Unpacked, String> {
    let name = packer.name();
    if packed < 12 || packed > bytes.len() as u64 {
        return Err(format!("This {name} file says it's {packed} bytes packed, but the file is {} bytes: it may be cut short.", bytes.len()));
    }
    if unpacked > MOST {
        return Err(format!("This {name} file says it unpacks to {unpacked} bytes, more than Limoniter unpacks."));
    }
    let data = &bytes[..packed as usize];
    let result = match packer {
        Packer::Ice21 => ice(data, unpacked as usize, 4),
        Packer::Ice24 => ice(data, unpacked as usize, 1),
        Packer::Atomik35 => atomik(data, unpacked as usize),
    };
    result.map(|(bytes, picture)| Unpacked { packer, bytes, packed_size: packed, picture }).map_err(|why| format!("This {name} file couldn't be unpacked: {why}."))
}

/// Reads a packed stream backwards from its end: the control bits, most significant first, from a word of
/// `width` bytes whose lowest set bit marks where it runs out, mixed in with whole bytes taken as they are.
struct Backwards<'a> {
    data: &'a [u8],
    at: usize,
    /// Where the stream starts: nothing before it is read.
    floor: usize,
    width: usize,
    bits: u32,
}

impl<'a> Backwards<'a> {
    fn new(data: &'a [u8], floor: usize, width: usize) -> Self {
        Self { data, at: data.len(), floor, width, bits: 0 }
    }

    fn byte(&mut self) -> Result<u8, String> {
        if self.at <= self.floor {
            return Err("the packed data runs out too soon".into());
        }
        self.at -= 1;
        Ok(self.data[self.at])
    }

    fn word(&mut self) -> Result<u32, String> {
        let mut word = 0u32;
        let mut bytes = [0u8; 4];
        for i in (0..self.width).rev() {
            bytes[i] = self.byte()?;
        }
        for b in &bytes[..self.width] {
            word = word << 8 | u32::from(*b);
        }
        Ok(word)
    }

    fn top(&self) -> u32 {
        1 << (self.width * 8 - 1)
    }

    fn mask(&self) -> u32 {
        if self.width == 4 { u32::MAX } else { (1 << (self.width * 8)) - 1 }
    }

    /// The first word, already holding its marker bit, as the packers leave it.
    fn start(&mut self) -> Result<(), String> {
        self.bits = self.word()?;
        if self.bits == 0 {
            return Err("the first control bits are missing".into());
        }
        Ok(())
    }

    fn bit(&mut self) -> Result<u32, String> {
        let bit = u32::from(self.bits & self.top() != 0);
        self.bits = (self.bits << 1) & self.mask();
        if self.bits != 0 {
            return Ok(bit);
        }
        // The marker has gone out: a fresh word, with a new marker shifted in behind it.
        let word = self.word()?;
        self.bits = ((word << 1) | 1) & self.mask();
        Ok(u32::from(word & self.top() != 0))
    }

    fn bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for _ in 0..count {
            value = value << 1 | self.bit()?;
        }
        Ok(value)
    }
}

/// Copies `length` bytes from `distance` further on, downwards, as the packers' strings do.
fn copy_back(out: &mut [u8], at: &mut usize, length: usize, distance: usize) -> Result<(), String> {
    if length > *at {
        return Err("a repeat runs past the start".into());
    }
    for _ in 0..length {
        *at -= 1;
        let from = *at + distance;
        if from >= out.len() {
            return Err("a repeat reaches past the end".into());
        }
        out[*at] = out[from];
    }
    Ok(())
}

/// Pack-Ice. Version 2.1 takes its control bits a long at a time; 2.3 and 2.4 a byte at a time, and they also
/// mark a repeat of the byte just written specially. Both read from the end, writing the file from its end.
fn ice(data: &[u8], size: usize, width: usize) -> Result<(Vec<u8>, bool), String> {
    let mut out = vec![0u8; size];
    let mut at = size;
    let mut input = Backwards::new(data, 12, width);
    input.start()?;
    let new = width == 1;
    loop {
        // A run of bytes as they are, if the next bit says so.
        if input.bit()? == 1 {
            const BITS: [u32; 5] = [1, 2, 2, 3, 8];
            const ALL: [u32; 5] = [1, 3, 3, 7, 255];
            const FROM: [usize; 6] = [1, 2, 5, 8, 15, 270];
            let mut level = 0;
            let mut count = input.bits(BITS[0])?;
            while level < 5 && count == ALL[level] {
                level += 1;
                count = input.bits(if level == 5 { 15 } else { BITS[level] })?;
            }
            let count = (count as usize + FROM[level]).min(at);
            for _ in 0..count {
                at -= 1;
                out[at] = input.byte()?;
            }
        }
        if at == 0 {
            break;
        }
        // Then a string: its length...
        const EXTRA: [u32; 5] = [0, 0, 1, 2, 10];
        const LENGTH: [usize; 5] = [0, 1, 2, 4, 8];
        let mut step = 0;
        while step < 4 && input.bit()? == 1 {
            step += 1;
        }
        let less_two = LENGTH[step] + input.bits(EXTRA[step])? as usize;
        // ...and how far back it is.
        let distance = if less_two == 0 {
            let far = input.bit()? == 1;
            let near = if far { 64 + input.bits(9)? as usize } else { input.bits(6)? as usize };
            if new { near + 1 } else { near + 2 }
        } else {
            const FAR_BITS: [u32; 3] = [8, 5, 12];
            const FAR: [usize; 3] = [32, 0, 288];
            let mut step = 0;
            while step < 2 && input.bit()? == 1 {
                step += 1;
            }
            let far = FAR[step] + input.bits(FAR_BITS[step])? as usize;
            match (new, far) {
                // In 2.3 and 2.4, nothing further means the byte just written, repeated.
                (true, 0) => 1,
                (true, far) => far + less_two + 1,
                (false, far) => far + less_two + 2,
            }
        };
        let length = (less_two + 2).min(at);
        copy_back(&mut out, &mut at, length, distance)?;
        if at == 0 && !new {
            break;
        }
        if at == 0 {
            // 2.3 and 2.4 still read one more bit, saying there's no run, before their end.
            if input.bit()? == 1 {
                return Err("there's more after the end".into());
            }
            break;
        }
    }
    // Pack-Ice 2.4 can rearrange a screen's bitplanes into nibbles to pack it better; a last bit says so.
    let mut picture = false;
    if new && size >= 32_000 && input.bit().unwrap_or(0) == 1 {
        let groups = if input.bit()? == 1 { input.bits(16)? as usize + 1 } else { 4000 };
        unscramble_ice_picture(&mut out, groups)?;
        picture = true;
    }
    Ok((out, picture))
}

/// Undoes Pack-Ice's picture mode, from the end: each 8 bytes held four words of pixels a nibble each, and become
/// four bitplane words again.
fn unscramble_ice_picture(out: &mut [u8], groups: usize) -> Result<(), String> {
    let mut at = out.len();
    for _ in 0..groups {
        if at < 8 {
            return Err("the picture runs past the start".into());
        }
        let mut planes = [0u16; 4];
        for _ in 0..4 {
            at -= 2;
            let mut word = u16::from_be_bytes([out[at], out[at + 1]]);
            for _ in 0..4 {
                for plane in &mut planes {
                    *plane = *plane << 1 | word >> 15;
                    word <<= 1;
                }
            }
        }
        for (i, plane) in planes.iter().enumerate() {
            out[at + i * 2..at + i * 2 + 2].copy_from_slice(&plane.to_be_bytes());
        }
    }
    Ok(())
}

/// Atomik 3.5: blocks, each with its own tables of near distances and of bytes that come up often, read from the
/// end as Pack-Ice is. Pictures are kept as nibbles too, with where each starts listed at the end of the file.
fn atomik(data: &[u8], size: usize) -> Result<(Vec<u8>, bool), String> {
    let mut out = vec![0u8; size];
    let mut at = size;
    let mut input = Backwards::new(data, 12, 1);
    let pictures = input.byte()?;
    input.start()?;
    while at > 0 {
        let special = if input.bit()? == 1 {
            let skip = usize::from(input.byte()?);
            let mut table = [0usize; 7];
            let mut offset = 1;
            for slot in &mut table {
                if offset == skip {
                    offset += 2;
                }
                *slot = offset;
                offset += 2;
            }
            Some((skip, table))
        } else {
            None
        };
        let common = if input.bit()? == 1 {
            let mut table = [0u8; 16];
            for slot in &mut table {
                *slot = input.byte()?;
            }
            Some(table)
        } else {
            None
        };
        let length_bits = usize::from(input.byte()?);
        let block = (usize::from(input.byte()?) << 8 | usize::from(input.byte()?)).min(at);
        let end = at - block;
        while at > end {
            if input.bit()? == 0 {
                at -= 1;
                out[at] = input.byte()?;
                continue;
            }
            let mut length = 0usize;
            while input.bit()? == 0 {
                if length == length_bits {
                    let mut more = input.bits(4)? as usize;
                    if more == 0 {
                        more = input.bits(8)? as usize;
                        if more == 0 {
                            more = input.bits(14)? as usize + 255;
                        }
                        more += 15;
                    }
                    length = more + length_bits;
                    break;
                }
                length += 1;
            }
            let selector_bits = if length == 0 {
                if input.bit()? == 1 {
                    // A byte from the table, or the byte after it changed by a little.
                    let n = input.bits(4)? as usize;
                    let byte = match &common {
                        Some(table) => table[n],
                        None => {
                            let next = *out.get(at).ok_or("a change refers past the end")?;
                            next.wrapping_sub(if n > 7 { (n as u8).wrapping_add(0xF0) } else { n as u8 })
                        }
                    };
                    at -= 1;
                    out[at] = byte;
                    continue;
                }
                length = 2;
                2
            } else {
                length += 2;
                3
            };
            const BITS: [u32; 8] = [1, 2, 4, 5, 6, 7, 8, 9];
            const FROM: [usize; 8] = [0, 32, 96, 352, 864, 1888, 3936, 8032];
            let selector = input.bits(selector_bits)? as usize;
            let offset = FROM[selector]
                + match &special {
                    Some((skip, table)) => {
                        let high = (input.bits(BITS[selector])? as usize) << 4;
                        let index = input.bits(3)? as usize;
                        let low = if index < 7 {
                            table[index]
                        } else if input.bit()? == 1 {
                            input.bits(3)? as usize * 2
                        } else {
                            *skip
                        };
                        high | low
                    }
                    None => input.bits(BITS[selector] + 4)? as usize,
                };
            let length = length.min(at - end);
            copy_back(&mut out, &mut at, length, offset + 1)?;
        }
    }
    // Pictures: the offset of each is a long at the end of what was unpacked, last first.
    let mut kept = size;
    let mut count = pictures;
    while count > 0 && kept > 32_000 {
        kept -= 4;
        let start = (long(&out, kept) as usize).min(kept - 32_000);
        for group in out[start..start + 32_000].as_chunks_mut::<8>().0 {
            let mut planes = [0u16; 4];
            for &byte in group.iter() {
                for (plane, value) in planes.iter_mut().enumerate() {
                    // Each byte is two pixels; the colour's top bit is the first plane's.
                    let shift = 3 - plane;
                    *value = *value << 2 | u16::from((byte >> (shift + 4)) & 1) << 1 | u16::from((byte >> shift) & 1);
                }
            }
            for (i, plane) in planes.iter().enumerate() {
                group[i * 2..i * 2 + 2].copy_from_slice(&plane.to_be_bytes());
            }
        }
        count -= 1;
    }
    out.truncate(kept);
    Ok((out, pictures > 0 && kept < size))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Writes Pack-Ice streams for tests: control bits and whole bytes in the order a depacker reads them, made
    /// into a file read from its end.
    pub struct IceWriter {
        width: usize,
        /// Bytes in reading order, with the control words in place.
        stream: Vec<u8>,
        /// Where the control word being filled is, and how many bits it has.
        word_at: usize,
        word: u32,
        used: usize,
        first: bool,
    }

    impl IceWriter {
        pub fn new(width: usize) -> Self {
            Self { width, stream: vec![0; width], word_at: 0, word: 0, used: 0, first: true }
        }

        fn room(&self) -> usize {
            self.width * 8 - usize::from(self.first)
        }

        fn close(&mut self) {
            let bits = self.width * 8;
            let value = if self.first {
                // The first word carries its own marker just below its bits.
                (self.word << (bits - self.used)) | (1 << (bits - self.used - 1))
            } else {
                self.word << (bits - self.used)
            };
            let value = if bits == 32 { value } else { value & ((1 << bits) - 1) };
            let bytes = value.to_be_bytes();
            // A word is read as one, from its high byte; in the stream (reversed later) it sits low byte first.
            for (i, b) in bytes[4 - self.width..].iter().rev().enumerate() {
                self.stream[self.word_at + i] = *b;
            }
        }

        pub fn bit(&mut self, bit: u32) {
            if self.used == self.room() {
                self.close();
                self.first = false;
                self.word_at = self.stream.len();
                self.stream.extend(std::iter::repeat_n(0, self.width));
                self.word = 0;
                self.used = 0;
            }
            self.word = self.word << 1 | bit;
            self.used += 1;
        }

        pub fn bits(&mut self, value: u32, count: u32) {
            for i in (0..count).rev() {
                self.bit(value >> i & 1);
            }
        }

        pub fn byte(&mut self, byte: u8) {
            self.stream.push(byte);
        }

        /// The stream as it sits in a file, read from its end.
        pub fn into_file_order(mut self) -> Vec<u8> {
            self.close();
            self.stream.iter().rev().copied().collect()
        }

        pub fn finish(mut self, magic: &[u8; 4], size: usize) -> Vec<u8> {
            self.close();
            let mut file = magic.to_vec();
            file.extend(((self.stream.len() + 12) as u32).to_be_bytes());
            file.extend((size as u32).to_be_bytes());
            file.extend(self.stream.iter().rev());
            file
        }
    }

    /// Packs bytes as Pack-Ice 2.4 (`width` 1) or 2.1 (`width` 4) would, simply: runs of bytes as they are and the
    /// longest string found close by. Only for tests; it never packs well.
    pub fn ice_pack(data: &[u8], width: usize) -> Vec<u8> {
        let new = width == 1;
        let mut w = IceWriter::new(width);
        let mut at = data.len();
        let mut literals: Vec<u8> = Vec::new();
        // The far value a string of `length` from `distance` further on is written with, if it can be written.
        let far = move |length: usize, distance: usize| -> Option<usize> {
            let far = match (new, length) {
                (_, 2) => distance.checked_sub(if new { 1 } else { 2 }).filter(|n| *n < 576),
                (true, _) if distance == 1 => Some(0),
                (true, _) => (distance + 1).checked_sub(length).filter(|n| *n > 0),
                (false, _) => distance.checked_sub(length),
            };
            far.filter(|n| *n < 4384)
        };
        // The longest string that ends at `at`, copied from `distance` further on.
        let find = |at: usize| -> Option<(usize, usize)> {
            let mut best: Option<(usize, usize)> = None;
            for distance in 1..=600usize.min(data.len() - at) {
                let mut length = 0;
                while length < 1033 && length < at && data[at - length - 1] == data[at + distance - length - 1] {
                    length += 1;
                }
                let length = (2..=length).rev().find(|&l| far(l, distance).is_some()).unwrap_or(0);
                if length >= 2 && best.is_none_or(|(l, _)| length > l) {
                    best = Some((length, distance));
                }
            }
            best
        };
        let flush = |w: &mut IceWriter, literals: &mut Vec<u8>| {
            if literals.is_empty() {
                w.bit(0);
                return;
            }
            w.bit(1);
            let n = literals.len() as u32;
            if n == 1 {
                w.bit(0);
            } else {
                w.bit(1);
                // Each level full of ones goes on to the next.
                let levels: [(u32, u32); 5] = [(2, 2), (2, 5), (3, 8), (8, 15), (15, 270)];
                for (i, (bits, from)) in levels.iter().enumerate() {
                    let room = (1u32 << bits) - 1;
                    if n - from < room || i == 4 {
                        w.bits(n - from, *bits);
                        break;
                    }
                    w.bits(room, *bits);
                }
            }
            for b in literals.drain(..) {
                w.byte(b);
            }
        };
        while at > 0 {
            match find(at) {
                Some((length, distance)) => {
                    flush(&mut w, &mut literals);
                    let l = length - 2;
                    match l {
                        0 => w.bit(0),
                        1 => w.bits(0b10, 2),
                        2..=3 => {
                            w.bits(0b110, 3);
                            w.bits(l as u32 - 2, 1);
                        }
                        4..=7 => {
                            w.bits(0b1110, 4);
                            w.bits(l as u32 - 4, 2);
                        }
                        _ => {
                            w.bits(0b1111, 4);
                            w.bits(l as u32 - 8, 10);
                        }
                    }
                    if l == 0 {
                        let near = far(2, distance).expect("found as one that can be written");
                        if near < 64 {
                            w.bit(0);
                            w.bits(near as u32, 6);
                        } else {
                            w.bit(1);
                            w.bits((near - 64) as u32, 9);
                        }
                    } else {
                        let far = far(length, distance).expect("found as one that can be written");
                        if far < 32 {
                            w.bits(0b10, 2);
                            w.bits(far as u32, 5);
                        } else if far < 288 {
                            w.bit(0);
                            w.bits((far - 32) as u32, 8);
                        } else {
                            w.bits(0b11, 2);
                            w.bits((far - 288) as u32, 12);
                        }
                    }
                    at -= length;
                    if at == 0 && new {
                        // 2.4 reads one more run bit before it ends.
                        w.bit(0);
                    }
                }
                None => {
                    at -= 1;
                    literals.push(data[at]);
                }
            }
        }
        if !literals.is_empty() {
            flush(&mut w, &mut literals);
        }
        if new {
            // No picture.
            w.bit(0);
        }
        w.finish(if new { b"ICE!" } else { b"Ice!" }, data.len())
    }

    fn sample(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        let words = ["move.l", "d0", "a0", "dc.b", "SNDH", "TITL", "  ", "\n", "$ff8800"];
        let mut out = Vec::new();
        while out.len() < n {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            if x >> 28 < 3 {
                out.push((x >> 8) as u8);
            } else {
                out.extend_from_slice(words[(x >> 16) as usize % words.len()].as_bytes());
            }
        }
        out.truncate(n);
        out
    }

    #[test]
    fn pack_ice_files_unpack_to_what_was_packed() {
        for width in [1, 4] {
            for (n, seed) in [(1, 1), (2, 2), (17, 3), (300, 4), (5000, 5), (40_000, 6)] {
                let data = sample(n, seed);
                let packed = ice_pack(&data, width);
                let unpacked = unpack(&packed).unwrap().unwrap_or_else(|e| panic!("{width} {n}: {e}"));
                assert_eq!(unpacked.packer, if width == 1 { Packer::Ice24 } else { Packer::Ice21 });
                assert!(unpacked.bytes == data, "{width} {n}");
                assert!(!unpacked.picture);
            }
        }
        // Runs of one byte use the repeat that 2.4 writes specially.
        let zeros = vec![0u8; 3000];
        let packed = ice_pack(&zeros, 1);
        assert!(packed.len() < 100, "{}", packed.len());
        assert_eq!(contents(&packed).as_ref(), zeros.as_slice());
    }

    #[test]
    fn damaged_packed_files_say_so_rather_than_crash() {
        let data = sample(4000, 9);
        let packed = ice_pack(&data, 1);
        assert!(unpack(&packed[..packed.len() / 2]).unwrap().unwrap_err().contains("cut short"));
        for cut in [1usize, 7, 100] {
            let mut broken = packed.clone();
            let at = broken.len() - cut;
            broken[at] ^= 0x5A;
            // Either an error or something, but never a panic.
            let _ = unpack(&broken);
        }
        let mut huge = packed.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(unpack(&huge).unwrap().is_err());
        assert!(unpack(b"not packed at all").is_none());
        assert_eq!(contents(b"plain").as_ref(), b"plain");
        let mut atomik = b"ATM5".to_vec();
        atomik.extend(100u32.to_be_bytes());
        atomik.extend(3u32.to_be_bytes());
        atomik.extend([0x80, 0x01, 0]);
        assert!(unpack(&atomik).unwrap().is_err());
    }

    #[test]
    fn pack_ice_pictures_are_put_back_into_bitplanes() {
        // The packer keeps a group's sixteen pixels a nibble each, the last four in the first word, as its
        // depacker takes the words from the end; these count 0 to 15 left to right.
        let mut out = vec![0u8; 8];
        for (i, w) in [0xCDEFu16, 0x89AB, 0x4567, 0x0123].iter().enumerate() {
            out[i * 2..i * 2 + 2].copy_from_slice(&w.to_be_bytes());
        }
        unscramble_ice_picture(&mut out, 1).unwrap();
        let planes: Vec<u16> = out.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        // Pixel n has colour n: plane 0 is its top bit (8 to 15), plane 3 its lowest (odd pixels).
        assert_eq!(planes, [0x00FF, 0x0F0F, 0x3333, 0x5555]);
    }

    /// Random streams given to both this depacker and a reference one, named by LIMONITER_REF_DEPACK (a program
    /// taking the packed file and the file to write): whenever both unpack, they must agree.
    #[test]
    fn random_streams_unpack_as_the_reference_depacker_does() {
        let Some(reference) = std::env::var_os("LIMONITER_REF_DEPACK") else {
            eprintln!("LIMONITER_REF_DEPACK isn't set, so nothing was compared.");
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-depack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut x = 7u32;
        let mut next = || {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            x
        };
        let (mut compared, mut agreed) = (std::collections::HashMap::new(), 0);
        let rounds: usize = std::env::var("LIMONITER_REF_ROUNDS").ok().and_then(|n| n.parse().ok()).unwrap_or(30_000);
        for round in 0..rounds {
            let magic: &[u8; 4] = [b"ICE!", b"Ice!", b"ATM5"][round % 3];
            let body: Vec<u8> = (0..(next() % 3000 + 50)).map(|_| (next() >> 24) as u8).collect();
            let size = next() % 300 + 1;
            let mut file = magic.to_vec();
            if magic == b"ATM5" {
                file.extend(size.to_be_bytes());
                file.extend((body.len() as u32).to_be_bytes());
            } else {
                file.extend((body.len() as u32 + 12).to_be_bytes());
                file.extend(size.to_be_bytes());
            }
            file.extend(&body);
            let Some(Ok(ours)) = unpack(&file) else {
                continue;
            };
            let input = dir.join("in");
            let output = dir.join("out");
            std::fs::write(&input, &file).unwrap();
            let _ = std::fs::remove_file(&output);
            let ok = std::process::Command::new(&reference).arg(&input).arg(&output).status().is_ok_and(|s| s.success());
            if !ok {
                continue;
            }
            let theirs = std::fs::read(&output).unwrap();
            *compared.entry(ours.packer.name()).or_insert(0) += 1;
            assert!(theirs == ours.bytes, "{} round {round}: {} bytes against {}", ours.packer.name(), ours.bytes.len(), theirs.len());
            agreed += 1;
        }
        eprintln!("{agreed} random streams unpacked the same as the reference: {compared:?}");
        // An Atomik picture: one block of bytes as they are, the last four the picture's offset.
        let mut picture: Vec<u8> = (0..32_000u32).map(|_| (next() >> 24) as u8).collect();
        picture.extend(0u32.to_be_bytes());
        let mut w = IceWriter::new(1);
        w.bits(0, 2);
        w.byte(8);
        w.byte((picture.len() >> 8) as u8);
        w.byte(picture.len() as u8);
        for &b in picture.iter().rev() {
            w.bit(0);
            w.byte(b);
        }
        let mut stream = w.into_file_order();
        stream.push(1);
        let mut file = b"ATM5".to_vec();
        file.extend((picture.len() as u32).to_be_bytes());
        file.extend((stream.len() as u32).to_be_bytes());
        file.extend(&stream);
        let ours = unpack(&file).unwrap().unwrap();
        assert!(ours.picture && ours.bytes.len() == 32_000);
        let (input, output) = (dir.join("in"), dir.join("out"));
        std::fs::write(&input, &file).unwrap();
        assert!(std::process::Command::new(&reference).arg(&input).arg(&output).status().unwrap().success());
        assert!(std::fs::read(&output).unwrap() == ours.bytes, "the Atomik picture differs");
        eprintln!("An Atomik picture unpacked the same as the reference.");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Real packed files from a folder named by LIMONITER_TEST_PACKED, with the reference depacker's output beside
    /// each as name.ref: they must be the same.
    #[test]
    fn real_packed_files_unpack_as_the_reference_does() {
        let Some(folder) = std::env::var_os("LIMONITER_TEST_PACKED") else {
            eprintln!("LIMONITER_TEST_PACKED isn't set, so no real packed files were unpacked.");
            return;
        };
        let mut counts = std::collections::HashMap::new();
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ref") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let Some(result) = unpack(&bytes) else {
                continue;
            };
            let unpacked = result.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let mut reference = path.clone().into_os_string();
            reference.push(".ref");
            if let Ok(theirs) = std::fs::read(&reference) {
                assert!(theirs == unpacked.bytes, "{}", path.display());
                *counts.entry(unpacked.packer.name()).or_insert(0) += 1;
            }
        }
        eprintln!("Real packed files the same as the reference: {counts:?}");
    }
}
