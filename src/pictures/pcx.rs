//! ZSoft PC Paintbrush pictures (.pcx). A 128-byte header: 0x0A, the version, the packing (1, run-length), the bits
//! per pixel in a plane, the picture's corners, a 16-colour palette, the number of planes and the bytes in each
//! plane's row. Then each row, its planes one after another, packed: a byte with its top two bits set says the
//! next byte comes that many times (its low six bits), any other byte is itself. A 256-colour picture's palette is
//! the last 768 bytes, after a 12.
//!
//! The kinds: 1 bit in 1 plane (black and white), 1 bit in 2 to 4 planes (EGA's 16 colours), 2 or 4 bits in 1
//! plane, 8 bits in 1 plane (256 colours), and 8 bits in 3 or 4 planes (24-bit red, green and blue).

use super::Picture;

/// The EGA's colours, for a picture whose header has none.
const EGA: [[u8; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 0xAA],
    [0, 0xAA, 0],
    [0, 0xAA, 0xAA],
    [0xAA, 0, 0],
    [0xAA, 0, 0xAA],
    [0xAA, 0x55, 0],
    [0xAA, 0xAA, 0xAA],
    [0x55, 0x55, 0x55],
    [0x55, 0x55, 0xFF],
    [0x55, 0xFF, 0x55],
    [0x55, 0xFF, 0xFF],
    [0xFF, 0x55, 0x55],
    [0xFF, 0x55, 0xFF],
    [0xFF, 0xFF, 0x55],
    [0xFF, 0xFF, 0xFF],
];

fn le16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

struct Header {
    version: u8,
    packed: bool,
    bits: usize,
    width: usize,
    height: usize,
    planes: usize,
    row: usize,
}

fn header(bytes: &[u8]) -> Option<Header> {
    if bytes.len() < 129 || bytes[0] != 0x0A || !matches!(bytes[1], 0 | 2..=5) || bytes[2] > 1 {
        return None;
    }
    let (left, top, right, bottom) = (le16(bytes, 4), le16(bytes, 6), le16(bytes, 8), le16(bytes, 10));
    let header = Header { version: bytes[1], packed: bytes[2] == 1, bits: usize::from(bytes[3]), width: right.checked_sub(left)? + 1, height: bottom.checked_sub(top)? + 1, planes: usize::from(bytes[65]), row: le16(bytes, 66) };
    let kind_ok = matches!((header.bits, header.planes), (1, 1..=4) | (2 | 4, 1) | (8, 1 | 3 | 4));
    (kind_ok && header.row * 8 >= header.width * header.bits && header.width * header.height <= 64 * 1024 * 1024).then_some(header)
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    // With another ending, the header's reserved byte must be 0 as well, as ZSoft's own programs leave it.
    header(bytes).is_some() && (matches!(ending, "pcx" | "pcc") || bytes[64] == 0)
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let h = header(bytes).ok_or("This isn't a PCX picture of a kind Limoniter knows.")?;
    let line = h.row * h.planes;
    let size = line * h.height;
    let mut data = Vec::with_capacity(size);
    let mut at = 128;
    while data.len() < size && at < bytes.len() {
        let b = bytes[at];
        at += 1;
        if h.packed && b >= 0xC0 {
            if let Some(&value) = bytes.get(at) {
                data.extend(std::iter::repeat_n(value, usize::from(b & 0x3F)));
            }
            at += 1;
        } else {
            data.push(b);
        }
    }
    let short = data.len() < size;
    data.resize(size, 0);

    // Version 3 has no palette of its own.
    let header_palette: Vec<[u8; 3]> = if h.version == 3 { EGA.to_vec() } else { bytes[16..64].as_chunks::<3>().0.to_vec() };
    let end_palette = (bytes.len() >= 128 + 769 && bytes[bytes.len() - 769] == 0x0C).then(|| bytes[bytes.len() - 768..].as_chunks::<3>().0.to_vec());
    let mut picture = Picture::new(h.width, h.height);
    let mut kind = String::new();
    for y in 0..h.height {
        let row = &data[y * line..(y + 1) * line];
        for x in 0..h.width {
            let colour = match (h.bits, h.planes) {
                (8, 3 | 4) => [row[x], row[h.row + x], row[2 * h.row + x]],
                (8, _) => match &end_palette {
                    Some(palette) => palette[usize::from(row[x])],
                    None => [row[x]; 3],
                },
                // Black and white: 0 black, 1 white, whatever the header's palette says; real files' headers often
                // say otherwise, or nothing (as version 3 doesn't), and were shown this way.
                (1, 1) => {
                    if row[x / 8] & (0x80 >> (x % 8)) != 0 { [255; 3] } else { [0; 3] }
                }
                (1, planes) => {
                    let index = (0..planes).filter(|p| row[p * h.row + x / 8] & (0x80 >> (x % 8)) != 0).fold(0, |v, p| v | 1 << p);
                    header_palette[index]
                }
                (bits, _) => {
                    let per = 8 / bits;
                    let value = (row[x / per] >> (8 - bits - (x % per) * bits)) & ((1 << bits) - 1);
                    header_palette[usize::from(value)]
                }
            };
            picture.set(x, y, colour);
        }
    }
    kind.push_str(match (h.bits, h.planes) {
        (8, 3 | 4) => "24-bit colour",
        (8, _) if end_palette.is_some() => "256 colours",
        (8, _) => "256 shades of grey (no palette)",
        (1, 1) => "Black and white",
        (1, 2) => "4 colours (2 planes)",
        (1, 3) => "8 colours (3 planes)",
        (1, _) => "16 colours (EGA, 4 planes)",
        (2, _) => "4 colours (CGA)",
        _ => "16 colours",
    });
    picture.note("Size", format!("{}\u{D7}{}", h.width, h.height));
    picture.note("Colours", kind);
    picture.note("Version", format!("{} ({})", h.version, if h.packed { "run-length packed" } else { "not packed" }));
    if (h.bits, h.planes) == (2, 1) {
        picture.note("Palette", "A CGA picture's palette is shown as the header gives it, which programs didn't always fill in");
    }
    if short {
        picture.note("Damaged", "The picture data ends early; the rest is shown in colour 0");
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_for(bits: u8, planes: u8, width: u16, height: u16, row: u16) -> Vec<u8> {
        let mut h = vec![0u8; 128];
        h[0] = 0x0A;
        h[1] = 5;
        h[2] = 1;
        h[3] = bits;
        h[8..10].copy_from_slice(&(width - 1).to_le_bytes());
        h[10..12].copy_from_slice(&(height - 1).to_le_bytes());
        h[65] = planes;
        h[66..68].copy_from_slice(&row.to_le_bytes());
        h
    }

    #[test]
    fn a_256_colour_picture_runs_and_finds_its_palette() {
        let mut file = header_for(8, 1, 4, 1, 4);
        // A run of three 1s, then a literal 2.
        file.extend([0xC3, 1, 2]);
        file.push(0x0C);
        let mut palette = vec![0u8; 768];
        palette[3..9].copy_from_slice(&[10, 20, 30, 40, 50, 60]);
        file.extend(palette);
        let picture = decode(&file).unwrap();
        assert_eq!(&picture.rgba[..3], &[10, 20, 30]);
        assert_eq!(&picture.rgba[12..15], &[40, 50, 60]);
    }

    #[test]
    fn ega_planes_and_24_bit_planes() {
        let mut file = header_for(1, 4, 8, 1, 2);
        file[16..64].copy_from_slice(&EGA.concat());
        // Pixel 0 has planes 0 and 2: colour 5.
        file.extend([0x80, 0, 0, 0, 0x80, 0, 0, 0]);
        assert_eq!(&decode(&file).unwrap().rgba[..3], &EGA[5]);
        let mut rgb = header_for(8, 3, 2, 1, 2);
        rgb.extend([1, 2, 3, 4, 5, 6]);
        let picture = decode(&rgb).unwrap();
        assert_eq!(&picture.rgba[..3], &[1, 3, 5]);
        assert_eq!(&picture.rgba[4..7], &[2, 4, 6]);
        assert!(looks_like("pcx", &rgb));
        assert!(!looks_like("pcx", b"\x0A\x05\x01"));
    }
}
