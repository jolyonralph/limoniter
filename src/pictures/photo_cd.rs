//! Kodak Photo CD image packs (.pcd, as IMG0001.PCD in a Photo CD's PHOTO_CD/IMAGES folder), at their highest
//! resolution.
//!
//! An image pack is 2048-byte sectors. Sector 1 starts "PCD_IPI"; byte 0xE02 says which way up the photo goes.
//! Base/16 (192x128), Base/4 (384x256) and Base (768x512) are kept whole, at sectors 4, 23 and 96: Kodak's
//! PhotoYCC, two rows of luma then a row each of the two chroma channels at half the size. The larger ones are
//! kept as corrections: 4Base (1536x1024) is Base enlarged and then its luma corrected, and 16Base (3072x2048) is
//! 4Base enlarged and then all three corrected. Each correction is a Huffman code, from tables at the start of its
//! part, between sync marks that say which row and channel follow.
//!
//! The layout, the enlarging and the decoding follow ImageMagick's PCD reader (coders/pcd.c), which itself follows
//! Hadmut Danisch's hpcdtoppm, so the results can be checked against it pixel for pixel. The colours are made from
//! PhotoYCC with the equations hpcdtoppm uses, clipped to what a screen shows.

use super::Picture;

const SECTOR: usize = 0x800;
const BASE_WIDTH: usize = 768;
const BASE_HEIGHT: usize = 512;

pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= 3 * SECTOR && &bytes[SECTOR..SECTOR + 7] == b"PCD_IPI"
}

/// The photo as PhotoYCC, all three channels at full size.
pub struct Ycc {
    pub width: usize,
    pub height: usize,
    pub luma: Vec<u8>,
    pub c1: Vec<u8>,
    pub c2: Vec<u8>,
    /// Which of Base, 4Base and 16Base it is.
    pub resolution: &'static str,
    /// What stopped it going larger, if something did.
    pub stopped: Option<String>,
}

/// A channel twice the size each way, by ImageMagick's and hpcdtoppm's interpolation: the old pixels where the
/// even rows and columns are, averages of the two or four around them between.
fn upsample(plane: &[u8], width: usize, height: usize) -> Vec<u8> {
    let wide = width * 2;
    let mut out = vec![0u8; wide * height * 2];
    let avg2 = |a: u8, b: u8| ((u16::from(a) + u16::from(b) + 1) >> 1) as u8;
    for y in 0..height {
        let from = &plane[y * width..(y + 1) * width];
        let to = &mut out[y * 2 * wide..y * 2 * wide + wide];
        for x in 0..width {
            to[x * 2] = from[x];
            to[x * 2 + 1] = if x + 1 < width { avg2(from[x], from[x + 1]) } else { from[x] };
        }
    }
    for y in 0..height - 1 {
        let (above, rest) = out[y * 2 * wide..].split_at_mut(wide);
        let (between, below) = rest.split_at_mut(wide);
        for x in 0..width - 1 {
            let at = x * 2;
            between[at] = avg2(above[at], below[at]);
            between[at + 1] = ((u16::from(above[at]) + u16::from(above[at + 2]) + u16::from(below[at]) + u16::from(below[at + 2]) + 2) >> 2) as u8;
        }
        let at = (width - 1) * 2;
        between[at] = avg2(above[at], below[at]);
        between[at + 1] = avg2(above[at + 1], below[at + 1]);
    }
    let last = (height * 2 - 1) * wide;
    out.copy_within(last - wide..last, last);
    out
}

/// Bits read the way hpcdtoppm reads them: a 32-bit window, the next bits at its top, topped up a byte at a time.
struct Bits<'a> {
    data: &'a [u8],
    /// The next byte to take into the window.
    at: usize,
    sum: u32,
    bits: i32,
    ended: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], at: usize) -> Self {
        Self { data, at, sum: 0, bits: 32, ended: false }
    }

    fn skip(&mut self, n: u32) {
        self.sum = if n >= 32 { 0 } else { self.sum << n };
        self.bits -= n as i32;
        while self.bits <= 24 {
            let byte = match self.data.get(self.at) {
                Some(&b) => b,
                None => {
                    self.ended = true;
                    0
                }
            };
            self.sum |= u32::from(byte) << (24 - self.bits);
            self.bits += 8;
            self.at += 1;
        }
    }

    fn is_sync(&self) -> bool {
        self.sum & 0xFFFF_FF00 == 0xFFFF_FE00
    }

    /// On to the next sync mark.
    fn find_sync(&mut self) {
        while self.sum & 0x00FF_F000 != 0x00FF_F000 && !self.ended {
            self.skip(8);
        }
        while !self.is_sync() && !self.ended {
            self.skip(1);
        }
    }

    /// Where reading has got to, as whole sectors from where it started reading them, as ImageMagick counts it.
    fn sector_after(&self, start: usize) -> usize {
        start + (self.at - start).div_ceil(SECTOR) * SECTOR
    }
}

struct Code {
    length: u32,
    sequence: u32,
    mask: u32,
    key: u8,
}

/// The codes in a Huffman table, and for every 16 bits that could come next, which of them they start with.
struct Table {
    codes: Vec<Code>,
    first: Vec<Option<u16>>,
}

impl Table {
    fn read(bits: &mut Bits) -> Result<Self, String> {
        bits.skip(8);
        let count = (bits.sum & 0xFF) as usize + 1;
        let mut codes = Vec::with_capacity(count);
        for _ in 0..count {
            bits.skip(8);
            let length = (bits.sum & 0xFF) + 1;
            if length > 16 {
                return Err("a Huffman table has a code longer than 16 bits".into());
            }
            bits.skip(16);
            let sequence = (bits.sum & 0xFFFF) << 16;
            bits.skip(8);
            let key = (bits.sum & 0xFF) as u8;
            codes.push(Code { length, sequence, mask: !((1u32 << (32 - length)) - 1), key });
        }
        // The first code in the table's order that matches, as hpcdtoppm searches, worked out once for each prefix.
        let first = (0..=0xFFFFu32).map(|prefix| codes.iter().position(|c| (prefix << 16) & c.mask == c.sequence).map(|i| i as u16)).collect();
        Ok(Self { codes, first })
    }
}

/// Corrects the channels from the Huffman-coded deltas starting at `start`, as far as the sync mark for the row past
/// the last. Returns where reading got to. Channels: luma at `width`, the chroma at half the size each way.
fn correct(data: &[u8], start: usize, tables: usize, width: usize, height: usize, channels: [&mut Vec<u8>; 3]) -> Result<usize, String> {
    let [luma, c1, c2] = channels;
    let mut bits = Bits::new(data, start);
    let tables: Vec<Table> = (0..tables).map(|_| Table::read(&mut bits)).collect::<Result<_, _>>()?;
    bits.skip(16);
    bits.skip(16);
    bits.find_sync();
    let (mut plane, mut row, mut column, mut count) = (0usize, 0usize, 0usize, 0usize);
    loop {
        if bits.ended {
            return Err("the corrections stop before the last row".into());
        }
        if bits.is_sync() {
            bits.skip(16);
            row = ((bits.sum >> 9) & 0x1FFF) as usize;
            if row == height {
                break;
            }
            bits.skip(8);
            let kind = (bits.sum >> 30) as usize;
            bits.skip(16);
            (plane, count) = match kind {
                0 => (0, width),
                2 | 3 => (kind - 1, width / 2),
                _ => return Err("a sync mark names a channel that isn't one".into()),
            };
            column = 0;
            continue;
        }
        let found = tables.get(plane).and_then(|t| t.first[(bits.sum >> 16) as usize].map(|i| &t.codes[usize::from(i)]));
        let Some(code) = found.filter(|_| row <= height) else {
            // A code that isn't in the table: on to the next row, as hpcdtoppm does.
            bits.find_sync();
            continue;
        };
        if count > 0 {
            let (target, stride) = match plane {
                0 => (&mut *luma, width),
                1 => (&mut *c1, width / 2),
                _ => (&mut *c2, width / 2),
            };
            let line = if plane == 0 { row } else { row >> 1 };
            if let Some(value) = target.get_mut(line * stride + column) {
                *value = (i16::from(*value) + i16::from(code.key as i8)).clamp(0, 255) as u8;
            }
            column += 1;
            count -= 1;
        }
        bits.skip(code.length);
    }
    Ok(bits.sector_after(start))
}

/// Luma, then the two chroma channels.
type Channels = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Reads one of the three sizes kept whole.
fn read_whole(data: &[u8], sector: usize, width: usize, height: usize) -> Result<Channels, String> {
    let size = width * height * 3 / 2;
    let start = sector * SECTOR;
    let whole = data.get(start..start + size).ok_or("The image pack is too short to hold its photo.")?;
    let (mut luma, mut c1, mut c2) = (Vec::with_capacity(width * height), Vec::with_capacity(size / 6), Vec::with_capacity(size / 6));
    for pair in whole.chunks_exact(width * 3) {
        luma.extend_from_slice(&pair[..width * 2]);
        c1.extend_from_slice(&pair[width * 2..width * 5 / 2]);
        c2.extend_from_slice(&pair[width * 5 / 2..]);
    }
    Ok((luma, c1, c2))
}

/// The photo as PhotoYCC at the largest size the pack holds, the right way up.
pub fn decode_ycc(data: &[u8]) -> Result<Ycc, String> {
    if !looks_like(data) {
        return Err("This isn't a Photo CD image pack.".into());
    }
    let (mut luma, mut c1, mut c2) = read_whole(data, 96, BASE_WIDTH, BASE_HEIGHT)?;
    let (mut width, mut height) = (BASE_WIDTH, BASE_HEIGHT);
    let mut resolution = "Base";
    let mut stopped = None;
    let after_base = (96 + BASE_WIDTH * BASE_HEIGHT * 3 / 2 / SECTOR + 4) * SECTOR;
    if data.len() > after_base {
        // 4Base: luma corrections only.
        let mut big = (upsample(&luma, width, height), upsample(&c1, width / 2, height / 2), upsample(&c2, width / 2, height / 2));
        match correct(data, after_base, 1, width * 2, height * 2, [&mut big.0, &mut big.1, &mut big.2]) {
            Ok(end) => {
                (luma, c1, c2) = big;
                (width, height) = (width * 2, height * 2);
                resolution = "4Base";
                let sixteen = end + 12 * SECTOR;
                if data.len() > sixteen {
                    // 16Base: all three corrected.
                    let mut bigger = (upsample(&luma, width, height), upsample(&c1, width / 2, height / 2), upsample(&c2, width / 2, height / 2));
                    match correct(data, sixteen, 3, width * 2, height * 2, [&mut bigger.0, &mut bigger.1, &mut bigger.2]) {
                        Ok(_) => {
                            (luma, c1, c2) = bigger;
                            (width, height) = (width * 2, height * 2);
                            resolution = "16Base";
                        }
                        Err(problem) => stopped = Some(format!("16Base couldn't be read ({problem}), so 4Base is shown")),
                    }
                }
            }
            Err(problem) => stopped = Some(format!("4Base couldn't be read ({problem}), so Base is shown")),
        }
    }
    let c1 = upsample(&c1, width / 2, height / 2);
    let c2 = upsample(&c2, width / 2, height / 2);
    let mut ycc = Ycc { width, height, luma, c1, c2, resolution, stopped };
    match data[0xE02] & 3 {
        1 => ycc.turn(3),
        2 => ycc.turn(2),
        3 => ycc.turn(1),
        _ => {}
    }
    Ok(ycc)
}

impl Ycc {
    /// Turns it a quarter turn clockwise, `quarters` times.
    fn turn(&mut self, quarters: u8) {
        for _ in 0..quarters {
            let (w, h) = (self.width, self.height);
            let turn = |plane: &[u8]| {
                let mut out = vec![0u8; w * h];
                for y in 0..h {
                    for x in 0..w {
                        out[x * h + (h - 1 - y)] = plane[y * w + x];
                    }
                }
                out
            };
            self.luma = turn(&self.luma);
            self.c1 = turn(&self.c1);
            self.c2 = turn(&self.c2);
            (self.width, self.height) = (h, w);
        }
    }
}

/// PhotoYCC to a screen's RGB: luma scaled by 1.3584, the chroma centred on 156 and 137, as hpcdtoppm does it.
fn rgb(y: u8, c1: u8, c2: u8) -> [u8; 3] {
    let l = 1.3584 * f32::from(y);
    let c1 = 2.2179 * (f32::from(c1) - 156.0);
    let c2 = 1.8215 * (f32::from(c2) - 137.0);
    let clip = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    [clip(l + c2), clip(l - 0.194 * c1 - 0.509 * c2), clip(l + c1)]
}

pub fn decode(data: &[u8]) -> Result<Picture, String> {
    let ycc = decode_ycc(data)?;
    let mut picture = Picture::new(ycc.width, ycc.height);
    for (i, pixel) in picture.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let [r, g, b] = rgb(ycc.luma[i], ycc.c1[i], ycc.c2[i]);
        pixel.copy_from_slice(&[r, g, b, 255]);
    }
    picture.note("Size", format!("{}\u{D7}{}", ycc.width, ycc.height));
    picture.note("Colours", "PhotoYCC, shown as RGB");
    picture.note("Resolution", format!("{} of Base/16 to 16Base", ycc.resolution));
    let turned = match data[0xE02] & 3 {
        1 => "a quarter turn anticlockwise",
        2 => "upside down",
        3 => "a quarter turn clockwise",
        _ => "",
    };
    if !turned.is_empty() {
        picture.note("Turned", format!("Scanned on its side; turned {turned}"));
    }
    if let Some(stopped) = ycc.stopped {
        picture.note("Damaged", stopped);
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsampling_keeps_the_old_pixels_and_averages_between() {
        let plane = [0u8, 100, 200, 50];
        let big = upsample(&plane, 2, 2);
        assert_eq!(&big[0..4], &[0, 50, 100, 100], "first row: the old ones, their average, the last repeated");
        assert_eq!(&big[4..8], &[100, 88, 75, 75], "between rows: averages of two, and of four");
        assert_eq!(&big[8..12], &[200, 125, 50, 50]);
        assert_eq!(&big[12..16], &big[8..12].to_vec()[..], "the last row repeated");
    }

    #[test]
    fn grey_stays_grey_and_white_is_white() {
        assert_eq!(rgb(0, 156, 137), [0, 0, 0]);
        let [r, g, b] = rgb(100, 156, 137);
        assert!(r == g && g == b);
        assert_eq!(rgb(255, 156, 137), [255, 255, 255]);
    }

    #[test]
    fn a_base_only_pack_is_read_and_turned() {
        let mut data = vec![0u8; 96 * SECTOR + BASE_WIDTH * BASE_HEIGHT * 3 / 2];
        data[SECTOR..SECTOR + 7].copy_from_slice(b"PCD_IPI");
        // The top-left pixel bright, on its side (a quarter turn anticlockwise to put right).
        data[96 * SECTOR] = 200;
        data[96 * SECTOR + BASE_WIDTH * 2..96 * SECTOR + BASE_WIDTH * 3].fill(156);
        data[0xE02] = 1;
        let ycc = decode_ycc(&data).unwrap();
        assert_eq!((ycc.width, ycc.height, ycc.resolution), (512, 768, "Base"));
        assert_eq!(ycc.luma[(767) * 512], 200, "the top-left corner turns to the bottom-left");
    }
}
