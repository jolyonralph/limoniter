use super::*;

/// A picture of a few tiles' worth of pattern repeated about, some mirrored and turned, for sharing to find.
fn patterned(wide: bool) -> Vec<[f32; 3]> {
    let width = if wide { 640 } else { 320 };
    let shape = |x: usize, y: usize| -> [f32; 3] {
        // A tile with nothing symmetrical about it: a corner block and a diagonal.
        if (x < 3 && y < 2) || x == y { [250.0, 40.0, 40.0] } else if y == 7 { [40.0, 40.0, 220.0] } else { [10.0, 10.0, 10.0] }
    };
    (0..width * 256)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let (cx, cy, px, py) = (x / 8, y / 8, x % 8, y % 8);
            match (cx + cy) % 4 {
                0 => shape(px, py),
                1 => shape(7 - px, py),
                2 => shape(py, 7 - px),
                _ => [200.0, 200.0, 60.0],
            }
        })
        .collect()
}

/// What the Next shows from the tilemap's own bytes, worked out again from them.
fn shown_from_bytes(t: &Tilemap) -> Vec<u8> {
    let (tiles, map, palette) = (t.tile_bytes(), t.map_bytes(), t.palette_bytes());
    let columns = t.columns();
    let width = columns * 8;
    let mut out = Vec::new();
    for y in 0..256 {
        for x in 0..width {
            let place = (y / 8) * columns + x / 8;
            let (tile, attribute) = (usize::from(map[place * 2]), map[place * 2 + 1]);
            // The attribute's way of showing it, undone: mirrored, then turned back.
            let (mut sx, mut sy) = (x % 8, y % 8);
            if attribute & 8 != 0 {
                sx = 7 - sx;
            }
            if attribute & 4 != 0 {
                sy = 7 - sy;
            }
            let (dx, dy) = if attribute & 2 != 0 { (sy, 7 - sx) } else { (sx, sy) };
            let byte = tiles[tile * 32 + dy * 4 + dx / 2];
            let value = if dx % 2 == 0 { byte >> 4 } else { byte & 15 };
            let entry = usize::from(attribute >> 4) * 16 + usize::from(value);
            let [r, g, b] = crate::pictures::next::colour(palette[entry * 2], palette[entry * 2 + 1]);
            out.extend([r, g, b, 255]);
        }
    }
    out
}

#[test]
fn turned_and_mirrored_tiles_share_a_definition() {
    for wide in [false, true] {
        let pixels = patterned(wide);
        let s = TileSettings { wide, ..TileSettings::default() };
        let t = tilemap(&pixels, &s, Dither::None, 0.0);
        assert_eq!(t.tiles.len(), 2, "the shape and the plain tile, the shape's mirror and turn shared");
        assert_eq!(t.different, 2);
        assert_eq!(shown_from_bytes(&t), t.rgba());
        // The red is red, where the shape's diagonal is, in each way it's shown.
        assert_eq!(t.shown(0, 0), [255, 36, 36]);
        assert_eq!(t.shown(15, 0), [255, 36, 36], "mirrored: the corner block on the right");
        assert_eq!(t.shown(16, 3), [36, 36, 218], "turned: the bottom row is the left column");
        // Every pixel shown is the picture's own, in the Next's nearest colour (by Oklab, as chosen): what was shared is shown the
        // way each place had it.
        let width = if wide { 640 } else { 320 };
        let gamut = Gamut::new();
        for (i, p) in pixels.iter().enumerate() {
            let near = rgb(gamut.nearest(*p));
            assert_eq!(t.shown(i % width, i / width), near, "{},{}", i % width, i / width);
        }
        // Not allowed to turn, three.
        let t = tilemap(&pixels, &TileSettings { turns: false, ..s }, Dither::None, 0.0);
        assert_eq!(t.tiles.len(), 4);
    }
}

#[test]
fn a_photograph_is_made_to_fit_the_tiles_and_palettes_allowed() {
    let image = crate::quantise::Image {
        width: 320,
        height: 256,
        rgba: (0..320 * 256).flat_map(|i| {
            let (x, y) = (i % 320, i / 320);
            [(x * 255 / 320) as u8, (y * 255 / 256) as u8, ((x * y) % 256) as u8, 255]
        }).collect(),
    };
    let pixels = crate::quantise::scale(&image, 320, 256, 1.0, crate::quantise::Fit::Fill, crate::quantise::Filter::Lanczos);
    for how in [Dither::None, Dither::Ordered] {
        let s = TileSettings { most: 64, palettes: 4, ..TileSettings::default() };
        let t = tilemap(&pixels, &s, how, 0.5);
        assert!(t.tiles.len() <= 64 && t.different > 64, "{} tiles of {} different", t.tiles.len(), t.different);
        assert!(t.map.iter().all(|&(tile, a)| usize::from(tile) < t.tiles.len() && a >> 4 < 4));
        assert!(t.tiles.iter().flatten().all(|&v| v < CLEAR));
        assert_eq!(shown_from_bytes(&t), t.rgba());
        // Made to fit, it's still near the picture: on average within a few levels.
        let error = crate::quantise::average_error(&pixels, &t.rgba());
        assert!(error < 30.0, "{how:?}: {error}");
    }
}

#[test]
fn sprites_are_cut_with_what_is_see_through() {
    // 40×20: red where x < 20, see-through after; a blue corner pixel at (0, 0).
    let mut rgba = Vec::new();
    for y in 0..20 {
        for x in 0..40 {
            let c = if (x, y) == (0, 0) { [0, 0, 255, 255] } else if x < 20 { [255, 0, 0, 255] } else { [0, 255, 0, 0] };
            rgba.extend(c);
        }
    }
    let image = crate::quantise::Image { width: 40, height: 20, rgba };
    let s = SpriteSettings { across: 3, down: 2, ..SpriteSettings::default() };
    let sprites = cut_sprites(&image, &s, Dither::None, 0.0);
    assert_eq!(sprites.patterns.len(), 6);
    // The Next's own palette: red is $E0, see-through $E3.
    assert_eq!(sprites.patterns[0][1], 0xE0);
    assert_eq!(sprites.patterns[0][0], 0x03, "blue");
    assert_eq!(sprites.patterns[1][3], 0xE0, "x 19 red, x 20 see-through");
    assert_eq!(sprites.patterns[1][4], 0xE3);
    assert_eq!(sprites.patterns[2][0], 0xE3, "beyond the picture");
    assert_eq!(sprites.shown(30, 3), None);
    assert_eq!(sprites.pattern_bytes().len(), 6 * 256);
    // The corner's colour see-through instead: blue goes, the see-through green stays see-through.
    let corner = cut_sprites(&image, &SpriteSettings { see_through: SeeThrough::Corner, ..s.clone() }, Dither::None, 0.0);
    assert_eq!(corner.patterns[0][0], 0xE3);
    // 4 bits: 16 colours, 3 see-through, two pixels a byte.
    let four = cut_sprites(&image, &SpriteSettings { four_bit: true, ..s }, Dither::None, 0.0);
    assert!(four.patterns.iter().flatten().all(|&v| v < 16));
    assert_eq!(four.patterns[1][4], 3);
    let bytes = four.pattern_bytes();
    assert_eq!(bytes.len(), 768, "six of 128 bytes, in three 256-byte slots");
    assert_eq!(bytes[128 + 1], four.patterns[1][2] << 4 | four.patterns[1][3]);
    assert_eq!(rgb(four.palette[usize::from(four.patterns[0][1])]), [255, 0, 0]);
}

#[test]
fn as_many_sprites_as_the_next_holds() {
    let image = crate::quantise::Image { width: 16, height: 16, rgba: vec![255; 16 * 16 * 4] };
    let eight = cut_sprites(&image, &SpriteSettings { across: 16, down: 16, ..SpriteSettings::default() }, Dither::None, 0.0);
    assert_eq!(eight.patterns.len(), 64);
    let four = cut_sprites(&image, &SpriteSettings { across: 16, down: 16, four_bit: true, ..SpriteSettings::default() }, Dither::None, 0.0);
    assert_eq!(four.patterns.len(), 128);
}

/// The .nex holds the tiles and map in bank 5, and sjasmplus makes the same one, if it's here.
#[test]
fn the_viewers_assemble_and_sjasmplus_agrees() {
    let t = tilemap(&patterned(false), &TileSettings::default(), Dither::None, 0.0);
    let image = crate::quantise::Image { width: 64, height: 64, rgba: (0..64 * 64).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 99, 255]).collect() };
    let sprites8 = cut_sprites(&image, &SpriteSettings { across: 16, down: 4, own_palette: true, ..SpriteSettings::default() }, Dither::Ordered, 0.5);
    let sprites4 = cut_sprites(&image, &SpriteSettings { across: 16, down: 8, four_bit: true, ..SpriteSettings::default() }, Dither::Ordered, 0.5);
    let sets = [("tiles", t.everything("tiles")), ("eight", sprites8.everything("eight")), ("four", sprites4.everything("four"))];
    for (name, files) in &sets {
        let nex = &files.iter().find(|(f, _)| f.ends_with(".nex")).unwrap_or_else(|| panic!("{name}: no .nex")).1;
        assert_eq!(&nex[..8], b"NextV1.2");
    }
    // The tilemap's bank 5 (first in the file): the tiles at $4000, the map at $6000.
    let nex = &sets[0].1.iter().find(|(f, _)| f.ends_with(".nex")).unwrap().1;
    assert_eq!(&nex[512..512 + 64], &t.tile_bytes()[..64]);
    assert_eq!(&nex[512 + 0x2000..512 + 0x2000 + 2560], &t.map_bytes()[..]);
    let sjasmplus = std::path::Path::new("/root/asmref/sjasmplus/sjasmplus");
    if !sjasmplus.exists() {
        return;
    }
    for (name, files) in &sets {
        let folder = std::env::temp_dir().join(format!("limoniter-tiles-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (file, bytes) in files.iter().filter(|(f, _)| !f.ends_with(".nex")) {
            std::fs::write(folder.join(file), bytes).unwrap();
        }
        let output = crate::platform::command(sjasmplus).arg("--nologo").arg(format!("{name}.asm")).current_dir(&folder).output().unwrap();
        assert!(output.status.success(), "{name}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let theirs = std::fs::read(folder.join(format!("{name}.nex"))).unwrap();
        let ours = &files.iter().find(|(f, _)| f.ends_with(".nex")).unwrap().1;
        assert!(*ours == theirs, "{name}: the .nex differs from sjasmplus's ({} and {} bytes)", ours.len(), theirs.len());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
