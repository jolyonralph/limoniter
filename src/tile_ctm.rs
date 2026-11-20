//! CharPad's project files (.ctm), versions 5 to 9, read into the tile map editor and written from it. CharPad
//! (Subchrist Software) is the C64's best-known character, tile and map editor; its help documents each version.
//!
//! Every version starts "CTM" and its version byte. Numbers are little-endian; counts are kept less one.
//!
//! - Version 5 (CharPad 2.0): a header of 20 bytes ($D021 to $D023, the character colour, the colouring, flags for
//!   the tile system, an expanded character set and multicolour, the counts of characters and tiles, the tile
//!   size and the map size), then the characters (8 bytes each), an attribute byte for each (material in the high
//!   nibble, colour in the low), the tiles (a word a cell, left out when expanded), a colour for each tile when
//!   colouring is per tile, and the map, a word a cell.
//! - Versions 6 (CharPad 2.6) and 7 (2.8): a short header (7 adds $D024 and the display mode), then blocks, each
//!   starting $DA, $B0 + its number: the characters (with their count), their attributes, then with the tile
//!   system on the tiles (count, width, height, cells), their colours when per tile, their tags and their names
//!   (each ending in 0), and last the map (width, height, cells).
//! - Versions 8 (CharPad 3.0; the version byte may be 82) and 9 (3.5): the display mode, colouring and flags
//!   first, then $D021 to $D024 and the project's three colours (9 has CharPad's flexi-grid before them and the
//!   key map in the flags' high nibble). Materials are a block of their own, and colours take a byte (text), two
//!   (hires bitmap: the screen's low and high nibbles) or three (multicolour bitmap: colour RAM, then those two).
//!   An early build of CharPad 3 wrote version 8 with a fourth project colour and four bytes to each colour; that's
//!   read too.
//!
//! Display modes: 0 hires text, 1 multicolour text, 2 ECM text, 3 hires bitmap, 4 multicolour bitmap. Colouring: 0
//! the same everywhere, 1 per tile, 2 per character. With the tile system off, the map holds character numbers.

use crate::tile_map::{Char, Colouring, Colours, CtmKept, Layer, Mode, Project, Tile};

/// The versions written, newest first, and what reads each.
pub const VERSIONS: [(u8, &str); 5] = [
    (9, "Version 9: CharPad C64 Pro 3.5 and later"),
    (8, "Version 8: CharPad C64 3.0 and later"),
    (7, "Version 7: CharPad Free 2.8 and later"),
    (6, "Version 6: CharPad 2.6 and later"),
    (5, "Version 5: CharPad 2.0 and later"),
];

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    block: u8,
}

fn short() -> String {
    crate::i18n::tr("The CharPad file ends too soon.").into()
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len()).ok_or_else(short)?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn word(&mut self) -> Result<usize, String> {
        let b = self.take(2)?;
        Ok(usize::from(u16::from_le_bytes([b[0], b[1]])))
    }

    fn words(&mut self, n: usize) -> Result<Vec<u16>, String> {
        Ok(self.take(n * 2)?.chunks(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect())
    }

    /// A block's marker: $DA, then $B0 and the block's number.
    fn marker(&mut self) -> Result<(), String> {
        let at = self.at;
        let mark = self.take(2)?;
        if mark[0] != 0xDA || mark[1] != 0xB0 | (self.block & 15) {
            return Err(crate::i18n::tf!("The CharPad file's block {} should start at byte {at}, but doesn't.", self.block));
        }
        self.block += 1;
        Ok(())
    }

    fn name(&mut self) -> Result<String, String> {
        let mut name = String::new();
        loop {
            match self.byte()? {
                0 => return Ok(name),
                b => name.push(char::from(b)),
            }
        }
    }
}

/// How a version's colour bytes are laid out: how many there are to each character or tile, and which is the colour
/// RAM's, the screen's low nibble and its high, if any.
fn colour_layout(mode: Mode, beta: bool) -> (usize, [Option<usize>; 3]) {
    match (mode, beta) {
        (Mode::C64BitmapHires, false) => (2, [None, Some(0), Some(1)]),
        (Mode::C64BitmapMulticolour, false) => (3, [Some(0), Some(1), Some(2)]),
        (_, false) => (1, [Some(0), None, None]),
        (Mode::C64BitmapHires, true) => (4, [Some(1), Some(3), Some(0)]),
        (Mode::C64BitmapMulticolour, true) => (4, [Some(3), Some(2), Some(1)]),
        (_, true) => (4, [Some(3), None, None]),
    }
}

fn colours_from(bytes: &[u8], layout: [Option<usize>; 3]) -> Colours {
    layout.map(|i| i.and_then(|i| bytes.get(i).copied()).unwrap_or(0))
}

/// A CharPad file read: the project, and the version it was (82 counted as 8).
pub fn read(bytes: &[u8]) -> Result<(Project, u8), String> {
    if bytes.len() < 4 || &bytes[..3] != b"CTM" {
        return Err(crate::i18n::tr("It isn't a CharPad file: those start \"CTM\".").into());
    }
    let version = bytes[3];
    let mut r = Reader { bytes, at: 4, block: 0 };
    let mut p = Project::new(Mode::C64Hires, (1, 1), (1, 1));
    p.ctm = CtmKept::default();
    let tile_system;
    let mut beta = false;
    match version {
        5 => return read_v5(&mut r, p).map(|p| (p, 5)),
        6 => {
            let h = r.take(6)?;
            p.backgrounds = [h[0], h[1], h[2], 0];
            p.colours = [h[3], 0, 0];
            p.colouring = Colouring::from_ctm(h[4]);
            p.mode = if h[5] & 1 != 0 { Mode::C64Multicolour } else { Mode::C64Hires };
            tile_system = h[5] & 2 != 0;
        }
        7 => {
            let h = r.take(8)?;
            p.backgrounds = [h[0], h[1], h[2], h[3]];
            p.colours = [h[4], 0, 0];
            p.colouring = Colouring::from_ctm(h[5]);
            p.mode = Mode::from_ctm_display(h[6].min(2)).unwrap_or(Mode::C64Hires);
            tile_system = h[7] & 1 != 0;
        }
        8 | 82 | 9 => {
            let h = r.take(3)?;
            p.mode = Mode::from_ctm_display(h[0].min(4)).unwrap_or(Mode::C64Hires);
            p.colouring = Colouring::from_ctm(h[1]);
            tile_system = h[2] & 1 != 0;
            if version == 9 {
                p.ctm.key_map = h[2] >> 4;
                p.ctm.flexigrid.copy_from_slice(r.take(5)?);
            }
            p.backgrounds.copy_from_slice(r.take(4)?);
            // The early build's header has a fourth colour, so its first block starts a byte later.
            beta = version == 8 && bytes.get(r.at + 3..r.at + 5) != Some(&[0xDA, 0xB0]) && bytes.get(r.at + 4..r.at + 6) == Some(&[0xDA, 0xB0]);
            let (_, layout) = colour_layout(p.mode, beta);
            p.colours = colours_from(r.take(if beta { 4 } else { 3 })?, if beta { layout } else { [Some(0), Some(1), Some(2)] });
        }
        _ => return Err(crate::i18n::tf!("It's a CharPad file of version {version}: Limoniter reads versions 5 to 9.")),
    }
    let newer = version >= 8;
    let (size, layout) = colour_layout(p.mode, beta);

    r.marker()?;
    let count = r.word()? + 1;
    let pixels = r.take(count * 8)?;
    p.chars = pixels.chunks(8).map(|b| Char { pixels: crate::tile_map::decode_char(b, Mode::C64Hires), colours: p.colours, material: 0 }).collect();
    r.marker()?;
    let attributes = r.take(count)?;
    for (c, a) in p.chars.iter_mut().zip(attributes) {
        if newer {
            c.material = a & 15;
        } else {
            c.material = a >> 4;
            c.colours = [a & 15, 0, 0];
        }
    }
    if newer && p.colouring == Colouring::Char {
        r.marker()?;
        let colours = r.take(count * size)?;
        for (c, bytes) in p.chars.iter_mut().zip(colours.chunks(size)) {
            c.colours = colours_from(bytes, layout);
        }
    }
    if tile_system {
        r.marker()?;
        let tiles = r.word()? + 1;
        let (tw, th) = (usize::from(r.byte()?), usize::from(r.byte()?));
        if tw == 0 || th == 0 {
            return Err(crate::i18n::tr("The CharPad file's tiles have no size.").into());
        }
        let cells = r.words(tiles * tw * th)?;
        p.tile_size = (tw, th);
        p.tiles = cells.chunks(tw * th).map(|c| Tile { cells: c.to_vec(), colours: p.colours, tag: 0, name: String::new() }).collect();
        if p.colouring == Colouring::Tile {
            r.marker()?;
            let colours = r.take(tiles * if newer { size } else { 1 })?;
            for (t, bytes) in p.tiles.iter_mut().zip(colours.chunks(if newer { size } else { 1 })) {
                t.colours = if newer { colours_from(bytes, layout) } else { [bytes[0], 0, 0] };
            }
        }
        r.marker()?;
        for (t, tag) in p.tiles.iter_mut().zip(r.take(tiles)?) {
            t.tag = *tag;
        }
        r.marker()?;
        for t in &mut p.tiles {
            t.name = r.name()?;
        }
    } else {
        plain_tiles(&mut p);
    }
    r.marker()?;
    read_map(&mut r, &mut p)?;
    Ok((p, if version == 82 { 8 } else { version }))
}

/// 1×1 tiles, one for each character, numbered as they are: the tile system off.
fn plain_tiles(p: &mut Project) {
    p.tile_size = (1, 1);
    p.tiles = (0..p.chars.len()).map(|i| Tile { cells: vec![i as u16], colours: p.colours, tag: 0, name: String::new() }).collect();
}

fn read_map(r: &mut Reader, p: &mut Project) -> Result<(), String> {
    let (w, h) = (r.word()?, r.word()?);
    let cells = r.words(w * h)?;
    p.map_size = (w.max(1), h.max(1));
    let mut cells = cells;
    cells.resize(p.map_size.0 * p.map_size.1, 0);
    p.layers = vec![Layer { name: crate::i18n::tr("Map").into(), cells, shown: true }];
    Ok(())
}

fn read_v5(r: &mut Reader, mut p: Project) -> Result<Project, String> {
    let h = r.take(6)?;
    p.backgrounds = [h[0], h[1], h[2], 0];
    p.colours = [h[3], 0, 0];
    p.colouring = Colouring::from_ctm(h[4]);
    let flags = h[5];
    p.mode = if flags & 4 != 0 { Mode::C64Multicolour } else { Mode::C64Hires };
    let chars = r.word()? + 1;
    let tiles = r.word()? + 1;
    let (tw, th) = (usize::from(r.byte()?), usize::from(r.byte()?));
    let (mw, mh) = (r.word()?, r.word()?);
    let pixels = r.take(chars * 8)?;
    let attributes = r.take(chars)?;
    p.chars = pixels.chunks(8).zip(attributes).map(|(b, a)| Char { pixels: crate::tile_map::decode_char(b, Mode::C64Hires), colours: [a & 15, 0, 0], material: a >> 4 }).collect();
    if flags & 1 != 0 {
        if tw == 0 || th == 0 {
            return Err(crate::i18n::tr("The CharPad file's tiles have no size.").into());
        }
        let n = tw * th;
        // An expanded set gives each tile cell a character of its own, in order, so the tiles aren't kept.
        let cells: Vec<u16> = if flags & 2 != 0 { (0..tiles * n).map(|i| i as u16).collect() } else { r.words(tiles * n)? };
        p.tile_size = (tw, th);
        p.tiles = cells.chunks(n).map(|c| Tile { cells: c.to_vec(), colours: p.colours, tag: 0, name: String::new() }).collect();
        if p.colouring == Colouring::Tile {
            for (t, c) in p.tiles.iter_mut().zip(r.take(tiles)?) {
                t.colours = [*c, 0, 0];
            }
        }
    } else {
        plain_tiles(&mut p);
    }
    let cells = r.words(mw * mh)?;
    p.map_size = (mw.max(1), mh.max(1));
    let mut cells = cells;
    cells.resize(p.map_size.0 * p.map_size.1, 0);
    p.layers = vec![Layer { name: crate::i18n::tr("Map").into(), cells, shown: true }];
    Ok(p)
}

/// The project as a CharPad file of a version, and what had to be left out of it. Only the C64's modes go into
/// CharPad files, and versions 5 and 6 only hold its text modes without ECM, 7 not its bitmaps.
pub fn write(p: &Project, version: u8) -> Result<(Vec<u8>, Vec<String>), String> {
    let Some(display) = p.mode.ctm_display() else {
        return Err(crate::i18n::tr("CharPad files are for the C64's modes: choose one of those to save one.").into());
    };
    let most = match version {
        5 | 6 => 1,
        7 => 2,
        8 | 9 => 4,
        _ => return Err(crate::i18n::tf!("Limoniter writes CharPad's versions 5 to 9, not {version}.")),
    };
    if display > most {
        return Err(crate::i18n::tf!("CharPad's version {version} doesn't have the {} mode: save it as version {}.", crate::i18n::tr(p.mode.name()), if display > 2 { 8 } else { 7 }));
    }
    if p.chars.len() > 65536 || p.tiles.len() > 65536 {
        return Err(crate::i18n::tr("A CharPad file holds at most 65536 characters and 65536 tiles.").into());
    }
    let (mw, mh) = p.map_size;
    if mw > 65535 || mh > 65535 {
        return Err(crate::i18n::tr("A CharPad map is at most 65535 tiles across and down.").into());
    }
    let mut notes = Vec::new();
    if p.layers.len() > 1 {
        notes.push(crate::i18n::tr("A CharPad file has one map: only the first layer went into it.").to_string());
    }
    if version < 6 && p.tiles.iter().any(|t| t.tag != 0 || !t.name.is_empty()) {
        notes.push(crate::i18n::tr("Version 5 has no tile tags or names: they were left out.").to_string());
    }
    let tile_system = !(p.plain_char_map() && p.colouring != Colouring::Tile);
    let (tw, th) = p.tile_size;
    let multicolour = p.mode == Mode::C64Multicolour;
    let word = |out: &mut Vec<u8>, n: usize| out.extend((n as u16).to_le_bytes());
    let mut out = b"CTM".to_vec();
    out.push(version);
    let b = p.backgrounds;
    let attribute = |c: &Char| (c.material << 4) | (c.colours[0] & 15);
    let map: &[u16] = p.layers.first().map_or(&[], |l| &l.cells);
    if version == 5 {
        let flags = u8::from(tile_system) | if multicolour { 4 } else { 0 };
        out.extend([b[0], b[1], b[2], p.colours[0], p.colouring.ctm(), flags]);
        word(&mut out, p.chars.len() - 1);
        if tile_system {
            word(&mut out, p.tiles.len() - 1);
            out.extend([tw as u8, th as u8]);
        } else {
            out.extend([0, 0, 1, 1]);
        }
        word(&mut out, mw);
        word(&mut out, mh);
        for c in &p.chars {
            out.extend(crate::tile_map::encode_char(&c.pixels, Mode::C64Hires));
        }
        out.extend(p.chars.iter().map(attribute));
        if tile_system {
            for t in &p.tiles {
                t.cells.iter().for_each(|c| word(&mut out, usize::from(*c)));
            }
            if p.colouring == Colouring::Tile {
                out.extend(p.tiles.iter().map(|t| t.colours[0]));
            }
        }
        map.iter().for_each(|c| word(&mut out, usize::from(*c)));
        return Ok((out, notes));
    }
    match version {
        6 => out.extend([b[0], b[1], b[2], p.colours[0], p.colouring.ctm(), u8::from(multicolour) | if tile_system { 2 } else { 0 }]),
        7 => out.extend([b[0], b[1], b[2], b[3], p.colours[0], p.colouring.ctm(), display, u8::from(tile_system)]),
        _ => {
            let flags = u8::from(tile_system) | if version == 9 { p.ctm.key_map << 4 } else { 0 };
            out.extend([display, p.colouring.ctm(), flags]);
            if version == 9 {
                out.extend(p.ctm.flexigrid);
            }
            out.extend(b);
            out.extend(p.colours);
        }
    }
    let newer = version >= 8;
    let (size, layout) = colour_layout(p.mode, false);
    let colour_bytes = |c: &Colours| -> Vec<u8> {
        let mut bytes = vec![0u8; size];
        for (from, to) in layout.iter().enumerate() {
            if let Some(to) = to {
                bytes[*to] = c[from];
            }
        }
        bytes
    };
    let mut block = 0u8;
    let mut marker = |out: &mut Vec<u8>| {
        out.extend([0xDA, 0xB0 | (block & 15)]);
        block += 1;
    };
    marker(&mut out);
    word(&mut out, p.chars.len() - 1);
    for c in &p.chars {
        out.extend(crate::tile_map::encode_char(&c.pixels, Mode::C64Hires));
    }
    marker(&mut out);
    if newer {
        out.extend(p.chars.iter().map(|c| c.material & 15));
        if p.colouring == Colouring::Char {
            marker(&mut out);
            out.extend(p.chars.iter().flat_map(|c| colour_bytes(&c.colours)));
        }
    } else {
        out.extend(p.chars.iter().map(attribute));
    }
    if tile_system {
        if tw > 255 || th > 255 {
            return Err(crate::i18n::tr("A CharPad tile is at most 255 characters across and down.").into());
        }
        marker(&mut out);
        word(&mut out, p.tiles.len() - 1);
        out.extend([tw as u8, th as u8]);
        for t in &p.tiles {
            t.cells.iter().for_each(|c| word(&mut out, usize::from(*c)));
        }
        if p.colouring == Colouring::Tile {
            marker(&mut out);
            if newer {
                out.extend(p.tiles.iter().flat_map(|t| colour_bytes(&t.colours)));
            } else {
                out.extend(p.tiles.iter().map(|t| t.colours[0]));
            }
        }
        marker(&mut out);
        out.extend(p.tiles.iter().map(|t| t.tag));
        marker(&mut out);
        for t in &p.tiles {
            // Names are plain ASCII, at most 32 letters.
            out.extend(t.name.chars().filter(|c| c.is_ascii() && *c != '\0').take(32).map(|c| c as u8));
            out.push(0);
        }
    }
    marker(&mut out);
    word(&mut out, mw);
    word(&mut out, mh);
    map.iter().for_each(|c| word(&mut out, usize::from(*c)));
    Ok((out, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Files saved by CharPad itself, from c64lib's Gradle Retro Assembler Plugin (MIT licence: see the folder's
    /// LICENSE.md): one small project, hires or multicolour, coloured per tile or per character, with tiles or
    /// without, saved in each version.
    fn sample(name: &str) -> &'static [u8] {
        match name {
            "hi-per-tile-5" => include_bytes!("tile_testdata/text-hi-per-tile-ctm5.ctm"),
            "hi-per-tile-6" => include_bytes!("tile_testdata/text-hi-per-tile-ctm6.ctm"),
            "hi-per-tile-7" => include_bytes!("tile_testdata/text-hi-per-tile-ctm7.ctm"),
            "hi-per-tile-8" => include_bytes!("tile_testdata/text-hi-per-tile-ctm8.ctm"),
            "hi-per-tile-82" => include_bytes!("tile_testdata/text-hi-per-tile-ctm82.ctm"),
            "hi-per-tile-8-beta" => include_bytes!("tile_testdata/text-hi-per-tile-ctm8-beta.ctm"),
            "hi-per-char-notiles-5" => include_bytes!("tile_testdata/text-hi-per-char-notiles-ctm5.ctm"),
            "hi-per-char-notiles-7" => include_bytes!("tile_testdata/text-hi-per-char-notiles-ctm7.ctm"),
            "hi-per-char-notiles-8" => include_bytes!("tile_testdata/text-hi-per-char-notiles-ctm8.ctm"),
            "mc-per-char-tiles-5" => include_bytes!("tile_testdata/text-mc-per-char-tiles-ctm5.ctm"),
            "mc-per-char-tiles-6" => include_bytes!("tile_testdata/text-mc-per-char-tiles-ctm6.ctm"),
            "mc-per-char-tiles-7" => include_bytes!("tile_testdata/text-mc-per-char-tiles-ctm7.ctm"),
            "mc-per-char-tiles-8-beta" => include_bytes!("tile_testdata/text-mc-per-char-tiles-ctm8.ctm"),
            _ => panic!("no sample {name}"),
        }
    }

    /// What a project shows, whatever version held it: characters, tiles, map, mode and colours.
    fn shown(p: &Project) -> (Mode, Colouring, Vec<[u8; 64]>, Vec<Vec<u16>>, (usize, usize), Vec<u16>, Vec<u8>) {
        let colours = match p.colouring {
            Colouring::Project => vec![p.colours[0]],
            Colouring::Tile => p.tiles.iter().map(|t| t.colours[0] & 15).collect(),
            Colouring::Char => p.chars.iter().map(|c| c.colours[0] & 15).collect(),
        };
        (p.mode, p.colouring, p.chars.iter().map(|c| c.pixels).collect(), p.tiles.iter().map(|t| t.cells.clone()).collect(), p.map_size, p.layers[0].cells.clone(), colours)
    }

    #[test]
    fn charpads_own_files_read_the_same_in_every_version() {
        let (first, version) = read(sample("hi-per-tile-5")).unwrap();
        assert_eq!(version, 5);
        assert_eq!((first.mode, first.colouring, first.chars.len(), first.tiles.len(), first.tile_size, first.map_size), (Mode::C64Hires, Colouring::Tile, 4, 3, (2, 2), (20, 12)));
        assert_eq!(first.backgrounds[..3], [14, 0, 1]);
        assert_eq!(first.chars[0].pixels[8..24], [0, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 1, 0], "the first character's second and third rows, $7E and $42");
        for name in ["hi-per-tile-6", "hi-per-tile-7", "hi-per-tile-8", "hi-per-tile-82", "hi-per-tile-8-beta"] {
            let (p, version) = read(sample(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(shown(&p), shown(&first), "{name}");
            assert_eq!(version, if name.contains("6") { 6 } else if name.contains("7") { 7 } else { 8 });
        }
        let (plain, _) = read(sample("hi-per-char-notiles-5")).unwrap();
        assert!(plain.plain_char_map(), "with the tile system off, the map is of characters");
        assert_eq!(plain.map_size, (40, 25));
        for name in ["hi-per-char-notiles-7", "hi-per-char-notiles-8"] {
            assert_eq!(shown(&read(sample(name)).unwrap().0), shown(&plain), "{name}");
        }
        let (multi, _) = read(sample("mc-per-char-tiles-5")).unwrap();
        assert_eq!((multi.mode, multi.colouring), (Mode::C64Multicolour, Colouring::Char));
        for name in ["mc-per-char-tiles-6", "mc-per-char-tiles-7", "mc-per-char-tiles-8-beta"] {
            assert_eq!(shown(&read(sample(name)).unwrap().0), shown(&multi), "{name}");
        }
    }

    #[test]
    fn charpads_own_files_are_written_back_byte_for_byte() {
        for name in ["hi-per-tile-5", "hi-per-tile-6", "hi-per-tile-7", "hi-per-tile-8", "hi-per-tile-82", "hi-per-char-notiles-5", "hi-per-char-notiles-7", "hi-per-char-notiles-8", "mc-per-char-tiles-5", "mc-per-char-tiles-6", "mc-per-char-tiles-7"] {
            let bytes = sample(name);
            let (p, version) = read(bytes).unwrap();
            let (written, notes) = write(&p, version).unwrap();
            let mut expected = bytes.to_vec();
            // Version 82 is written as 8, as CharPad itself does.
            expected[3] = version;
            assert_eq!(written, expected, "{name}");
            assert!(notes.is_empty(), "{name}: {notes:?}");
        }
    }

    #[test]
    fn every_version_reads_back_what_it_writes() {
        let mut p = Project::new(Mode::C64Multicolour, (2, 2), (5, 3));
        p.chars.truncate(5);
        p.chars[1].pixels = std::array::from_fn(|i| (i % 3 == 0) as u8);
        p.chars[2].material = 9;
        p.tiles.truncate(4);
        p.tiles[1].cells = vec![1, 2, 3, 4];
        p.tiles[3].cells = vec![4, 3, 2, 1];
        p.tiles[3].tag = 200;
        p.tiles[3].name = "ladder".into();
        p.backgrounds = [0, 9, 10, 11];
        p.set(0, 4, 2, 3);
        p.set(0, 1, 0, 1);
        p.ctm.key_map = 9;
        p.ctm.flexigrid = [3, 0, 2, 0, 0x81];
        for colouring in Colouring::ALL {
            p.colouring = colouring;
            for (i, t) in p.tiles.iter_mut().enumerate() {
                t.colours = [8 + i as u8, 0, 0];
            }
            for (i, c) in p.chars.iter_mut().enumerate() {
                c.colours = [9 + i as u8, 0, 0];
            }
            for (version, _) in VERSIONS {
                let (bytes, notes) = write(&p, version).unwrap();
                let (back, read_as) = read(&bytes).unwrap_or_else(|e| panic!("version {version}: {e}"));
                assert_eq!(read_as, version);
                assert_eq!(shown(&back), shown(&p), "version {version}, {colouring:?}");
                assert_eq!(back.chars[2].material, 9);
                assert_eq!(back.backgrounds[..3], p.backgrounds[..3]);
                if version >= 6 {
                    assert_eq!((back.tiles[3].tag, back.tiles[3].name.as_str()), (200, "ladder"), "version {version}");
                    assert!(notes.is_empty());
                } else {
                    assert_eq!(notes.len(), 1, "version 5 says it has no tags");
                }
                if version >= 7 {
                    assert_eq!(back.backgrounds, p.backgrounds);
                }
                if version == 9 {
                    assert_eq!(back.ctm, p.ctm);
                }
                // And written again, the same bytes.
                assert_eq!(write(&back, version).unwrap().0, bytes, "version {version}, {colouring:?}");
            }
        }
    }

    #[test]
    fn bitmaps_and_ecm_keep_their_colours_in_versions_8_and_9() {
        for mode in [Mode::C64BitmapHires, Mode::C64BitmapMulticolour, Mode::C64Ecm] {
            let mut p = Project::new(mode, (1, 1), (4, 2));
            p.chars.truncate(3);
            p.tiles.truncate(3);
            p.colouring = Colouring::Char;
            p.chars[1].colours = [7, 3, 4];
            p.colours = [1, 2, 5];
            for version in [8, 9] {
                let (bytes, _) = write(&p, version).unwrap();
                let (back, _) = read(&bytes).unwrap();
                assert_eq!(back.mode, mode);
                let expected = match mode {
                    Mode::C64BitmapHires => [0, 3, 4],
                    Mode::C64BitmapMulticolour => [7, 3, 4],
                    _ => [7, 0, 0],
                };
                assert_eq!(back.chars[1].colours, expected, "{mode:?}");
                assert_eq!(back.colours, p.colours);
            }
            // Older versions refuse what they can't hold, and say which will.
            assert!(write(&p, 6).unwrap_err().contains(if mode == Mode::C64Ecm { '7' } else { '8' }));
        }
        assert!(write(&Project::new(Mode::Spectrum, (1, 1), (1, 1)), 9).is_err());
        assert!(read(b"CTM\x04nothing").unwrap_err().contains('4'));
        assert!(read(b"PNG").is_err());
        // A file cut short says so, rather than going wrong.
        let (bytes, _) = write(&Project::default(), 9).unwrap();
        for cut in [5, 30, bytes.len() - 1] {
            assert!(read(&bytes[..cut]).is_err(), "{cut}");
        }
    }

    #[test]
    fn layers_after_the_first_are_left_out_and_said_to_be() {
        let mut p = Project::default();
        p.add_layer();
        let (bytes, notes) = write(&p, 9).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(read(&bytes).unwrap().0.layers.len(), 1);
    }
}
