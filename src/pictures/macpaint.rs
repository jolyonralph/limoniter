//! MacPaint pictures (file type PNTG): a 512-byte header (a version number and the 38 fill patterns), then the
//! 576x720 page, 72 bytes a row, each row packed with PackBits. Set bits are black. A copy that travelled away
//! from a Mac may have a 128-byte MacBinary header in front, which names its type.

use super::Picture;

const WIDTH: usize = 576;
const HEIGHT: usize = 720;
const ROW: usize = WIDTH / 8;

/// The MacPaint file itself, from inside MacBinary if it's wrapped.
fn body(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() > 128 + 512 && bytes[0] == 0 && &bytes[65..69] == b"PNTG" {
        let length = u32::from_be_bytes([bytes[83], bytes[84], bytes[85], bytes[86]]) as usize;
        return bytes.get(128..128 + length.min(bytes.len() - 128));
    }
    Some(bytes)
}

pub fn looks_like(ending: &str, bytes: &[u8]) -> bool {
    let wrapped = bytes.len() > 128 + 512 && bytes[0] == 0 && bytes.get(65..69) == Some(b"PNTG");
    // An unwrapped file says nothing of itself but its version, 0, 2 or 3.
    let named = matches!(ending, "pntg" | "pnt" | "mac" | "macp") && bytes.len() > 512 && bytes[..4] == [0, 0, 0, bytes[3]] && bytes[3] <= 3;
    wrapped || named
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let data = body(bytes).filter(|d| d.len() > 512).ok_or("This is too short to be a MacPaint picture.")?;
    let mut page = Vec::with_capacity(ROW * HEIGHT);
    let mut at = 512;
    while page.len() < ROW * HEIGHT && at < data.len() {
        let n = data[at] as i8;
        at += 1;
        match n {
            0..=127 => {
                let end = (at + n as usize + 1).min(data.len());
                page.extend_from_slice(&data[at..end]);
                at = end;
            }
            -127..=-1 => {
                if let Some(&b) = data.get(at) {
                    page.extend(std::iter::repeat_n(b, (1 - i16::from(n)) as usize));
                }
                at += 1;
            }
            -128 => {}
        }
    }
    let short = page.len() < ROW * HEIGHT;
    page.resize(ROW * HEIGHT, 0);
    let mut picture = Picture::new(WIDTH, HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let black = page[y * ROW + x / 8] & (0x80 >> (x % 8)) != 0;
            picture.set(x, y, if black { [0, 0, 0] } else { [255, 255, 255] });
        }
    }
    picture.note("Size", "576\u{D7}720 (a whole page)");
    picture.note("Colours", "Black and white");
    if data.len() != bytes.len() {
        picture.note("Header", "MacBinary");
    }
    if short {
        picture.note("Damaged", "The page ends early; the rest is shown white");
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_unpack_and_set_bits_are_black() {
        let mut file = vec![0u8; 512];
        file[3] = 2;
        // Row 0: one literal 0x80, then 71 zero bytes as a run.
        file.extend([0, 0x80, (-70i8) as u8, 0]);
        let picture = decode(&file).unwrap();
        assert_eq!(&picture.rgba[..3], &[0, 0, 0]);
        assert_eq!(&picture.rgba[4..7], &[255, 255, 255]);
        assert!(picture.info.iter().any(|(label, _)| label == "Damaged"), "only one row is there");
        assert!(looks_like("pntg", &file));
        assert!(!looks_like("txt", &file));
    }

    #[test]
    fn a_macbinary_copy_is_unwrapped() {
        let mut inner = vec![0u8; 512];
        inner.extend(std::iter::repeat_n([(-71i8) as u8, 0xFF], HEIGHT).flatten());
        let mut file = vec![0u8; 128];
        file[65..69].copy_from_slice(b"PNTG");
        file[83..87].copy_from_slice(&(inner.len() as u32).to_be_bytes());
        file.extend(&inner);
        assert!(looks_like("", &file));
        let picture = decode(&file).unwrap();
        assert_eq!(&picture.rgba[..3], &[0, 0, 0], "every row is a run of black");
        assert!(!picture.info.iter().any(|(label, _)| label == "Damaged"));
    }
}
