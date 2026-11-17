use super::*;

/// A picture with something of everything: a colour sweep across, light to dark down, and a white box.
fn sample(width: usize, height: usize) -> Image {
    let rgba = (0..width * height)
        .flat_map(|i| {
            let (x, y) = (i % width, i / width);
            if (width / 3..width / 2).contains(&x) && (height / 3..height / 2).contains(&y) {
                return [255, 255, 255, 255];
            }
            let hue = x as f32 / width as f32 * 6.0;
            let level = 1.0 - y as f32 / height as f32;
            let part = |offset: f32| (((hue + offset) % 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0) * level * 255.0;
            [part(0.0) as u8, part(4.0) as u8, part(2.0) as u8, 255]
        })
        .collect();
    Image { width, height, rgba }
}

fn settings(mode: Mode, dither: Dither) -> Settings {
    Settings { mode, dither, ..Settings::default() }
}

#[test]
fn every_mode_converts_at_its_size() {
    let image = sample(300, 220);
    for machine in Machine::ALL {
        for &mode in machine.modes() {
            assert_eq!(mode.machine(), machine);
            for dither in Dither::ALL {
                let screen = convert(&image, &settings(mode, dither));
                let (w, h) = if mode == Mode::Sprites { (64, 64) } else { mode.size() };
                assert_eq!((screen.width, screen.height), (w, h), "{mode:?}");
                assert!(screen.values.len() == w * h || screen.tilemap.is_some() || screen.sprites.is_some(), "{mode:?}");
                assert_eq!(screen.rgba().len(), w * h * 4);
                let files = screen.everything("test");
                assert!(files.iter().any(|(n, _)| n == "test.png"), "{mode:?}");
                assert!(files.len() >= 2, "{mode:?}");
            }
        }
    }
}

#[test]
fn a_spectrum_picture_keeps_two_colours_a_cell_both_bright_or_neither() {
    let screen = convert(&sample(256, 192), &settings(Mode::Spectrum, Dither::FloydSteinberg));
    assert_eq!(screen.cells.len(), 768);
    for &(ink, paper) in &screen.cells {
        assert_eq!(ink >= 8, paper >= 8, "{ink} {paper}");
    }
    // The .scr shows what the converter says it does, as Limoniter's own Spectrum screen reader sees it.
    let scr = screen.spectrum_screen().unwrap().to_scr();
    let shown = crate::screen_designer::Screen::from_scr(&scr).unwrap();
    for y in (0..192).step_by(3) {
        for x in (0..256).step_by(5) {
            let c = shown.shown(x, y);
            assert_eq!([c.r(), c.g(), c.b()], screen.rgb(x, y), "{x},{y}");
        }
    }
    // The white box stays white.
    assert_eq!(screen.rgb(110, 75), [255, 255, 255]);
    // Never BRIGHT, then none is.
    let plain = convert(&sample(256, 192), &Settings { bright: Bright::Never, ..settings(Mode::Spectrum, Dither::None) });
    assert!(plain.cells.iter().all(|&(i, p)| i < 8 && p < 8));
}

#[test]
fn cells_of_the_machines_own_colours_come_back_as_they_were() {
    // Each cell two Spectrum colours from one group, in a pattern: nothing to lose.
    let mut rgba = Vec::new();
    for y in 0..192 {
        for x in 0..256 {
            let cell = (y / 8) * 32 + x / 8;
            let bright = cell % 3 == 0;
            let (a, b) = ((cell % 8) as usize, ((cell / 8 + 3) % 8) as usize);
            let (a, b) = if bright { (a + 8, b + 8) } else { (a, b) };
            let c = if (x + y) % 3 == 0 { SPECTRUM[a] } else { SPECTRUM[b] };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    let image = Image { width: 256, height: 192, rgba: rgba.clone() };
    let screen = convert(&image, &Settings { filter: Filter::Nearest, ..settings(Mode::Spectrum, Dither::None) });
    assert_eq!(screen.rgba(), rgba);
}

#[test]
fn timex_hi_colour_has_a_colour_for_each_byte() {
    let screen = convert(&sample(256, 192), &settings(Mode::Timex, Dither::Ordered));
    assert_eq!(screen.cells.len(), 32 * 192);
    let scr = &screen.everything("t")[0].1;
    assert_eq!(scr.len(), 12288);
    // Pixel row 1 is 256 bytes on, its colours 6144 on from it.
    let (ink, paper) = screen.cells[32 + 4];
    let attribute = scr[6144 + 256 + 4];
    assert_eq!((attribute & 7, (attribute >> 3) & 7), (ink & 7, paper & 7));
}

#[test]
fn a_next_layer2_picture_is_an_nxi_limoniter_reads_back() {
    let screen = convert(&sample(256, 192), &settings(Mode::Layer2, Dither::None));
    assert!(screen.palette.len() <= 256);
    let mut seen = screen.palette.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), screen.palette.len(), "no colour twice");
    assert_eq!(screen.palette[0], 0, "black first");
    let (name, nxi) = &screen.everything("n")[0];
    assert_eq!(name, "n.nxi");
    let picture = crate::pictures::next::decode("nxi", nxi).unwrap();
    assert_eq!(picture.rgba, screen.rgba());
}

/// The CPC's screen memory read back, as the gate array shows it.
fn cpc_pixel(memory: &[u8], mode: Mode, x: usize, y: usize) -> u8 {
    let line = (y / 8) * 80 + (y % 8) * 0x800;
    match mode {
        Mode::Cpc0 => {
            let byte = memory[line + x / 2];
            let s = 1 - x % 2;
            (byte >> (7 - (1 - s)) & 1) | (byte >> (3 - (1 - s)) & 1) << 1 | (byte >> (5 - (1 - s)) & 1) << 2 | (byte >> (1 - (1 - s)) & 1) << 3
        }
        Mode::Cpc1 => {
            let byte = memory[line + x / 4];
            (byte >> (7 - x % 4) & 1) | (byte >> (3 - x % 4) & 1) << 1
        }
        _ => memory[line + x / 8] >> (7 - x % 8) & 1,
    }
}

#[test]
fn cpc_screens_are_laid_out_as_the_cpc_keeps_them() {
    for mode in [Mode::Cpc0, Mode::Cpc1, Mode::Cpc2] {
        let screen = convert(&sample(320, 200), &settings(mode, Dither::Ordered));
        let memory = screen.cpc_memory();
        for y in [0, 1, 7, 8, 99, 199] {
            for x in (0..screen.width).step_by(3) {
                assert_eq!(cpc_pixel(&memory, mode, x, y), screen.values[y * screen.width + x], "{mode:?} {x},{y}");
            }
        }
        let files = screen.everything("pic");
        let (name, scr) = &files[0];
        assert_eq!(name, "pic.scr");
        assert_eq!(scr.len(), 128 + 16384);
        assert_eq!(&scr[1..12], b"PIC     SCR");
        let checksum: u16 = scr[..67].iter().map(|&b| u16::from(b)).sum();
        assert_eq!(u16::from_le_bytes([scr[67], scr[68]]), checksum);
        let loader = String::from_utf8(files[1].1.clone()).unwrap();
        assert!(loader.contains("LOAD\"PIC.SCR\",&C000"), "{loader}");
        assert!(screen.palette.iter().all(|&c| c < 27));
    }
    // Mode 0: 16 inks.
    assert_eq!(convert(&sample(320, 200), &settings(Mode::Cpc0, Dither::None)).palette.len(), 16);
}

#[test]
fn msx_screen_2_is_a_bsaved_vram_image() {
    let screen = convert(&sample(256, 192), &settings(Mode::Msx2, Dither::FloydSteinberg));
    assert!(screen.cells.iter().all(|&(f, b)| f != 0 && b != 0), "colour 0 is see-through");
    let sc2 = &screen.everything("m")[0].1;
    assert_eq!(&sc2[..7], &[0xFE, 0, 0, 0xFF, 0x37, 0, 0]);
    let vram = &sc2[7..];
    assert_eq!(vram.len(), 0x3800);
    for y in (0..192).step_by(5) {
        for x in (0..256).step_by(7) {
            let character = usize::from(vram[0x1800 + (y / 8) * 32 + x / 8]) + (y / 64) * 256;
            let pattern = vram[character * 8 + y % 8];
            let colours = vram[0x2000 + character * 8 + y % 8];
            let c = if pattern >> (7 - x % 8) & 1 == 1 { colours >> 4 } else { colours & 15 };
            assert_eq!(TMS9918[usize::from(c)], screen.rgb(x, y), "{x},{y}");
        }
    }
}

#[test]
fn bbc_screens_are_character_cells_of_eight_bytes() {
    for mode in [Mode::Bbc0, Mode::Bbc1, Mode::Bbc2, Mode::Bbc4, Mode::Bbc5] {
        let screen = convert(&sample(320, 256), &settings(mode, Dither::Ordered));
        let memory = screen.bbc_memory();
        let (per_byte, row) = match mode {
            Mode::Bbc0 => (8, 640),
            Mode::Bbc1 => (4, 640),
            Mode::Bbc2 => (2, 640),
            Mode::Bbc4 => (8, 320),
            _ => (4, 320),
        };
        assert_eq!(memory.len(), row * 32);
        for y in [0, 7, 8, 100, 255] {
            for x in (0..screen.width).step_by(3) {
                let byte = memory[(y / 8) * row + (x / per_byte) * 8 + y % 8];
                let p = x % per_byte;
                let value = match per_byte {
                    2 => (byte >> (7 - p) & 1) << 3 | (byte >> (5 - p) & 1) << 2 | (byte >> (3 - p) & 1) << 1 | (byte >> (1 - p) & 1),
                    4 => (byte >> (7 - p) & 1) << 1 | (byte >> (3 - p) & 1),
                    _ => byte >> (7 - p) & 1,
                };
                assert_eq!(value, screen.values[y * screen.width + x], "{mode:?} {x},{y}");
            }
        }
    }
}

#[test]
fn the_spectrum_tape_loads_the_screen_after_its_loader() {
    let screen = convert(&sample(256, 192), &settings(Mode::Spectrum, Dither::Ordered));
    let files = screen.everything("pic");
    let tap = &files.iter().find(|(n, _)| n == "pic.tap").unwrap().1;
    // Four blocks: the program's header and body, the screen's header and the screen.
    let mut blocks = Vec::new();
    let mut at = 0;
    while at + 2 <= tap.len() {
        let length = usize::from(u16::from_le_bytes([tap[at], tap[at + 1]]));
        blocks.push(&tap[at + 2..at + 2 + length]);
        at += 2 + length;
    }
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[2][1], 3, "a CODE header");
    assert_eq!(u16::from_le_bytes([blocks[2][14], blocks[2][15]]), 16384);
    assert_eq!(&blocks[3][1..6913], &files[0].1[..]);
    let listed = crate::basic::list_tap(tap).lines.join("\n");
    assert!(listed.contains("SCREEN$"), "{listed}");
}

#[test]
fn brightness_and_contrast_change_the_picture() {
    let mut pixels = vec![[100.0, 120.0, 140.0]; 4];
    adjust(&mut pixels, &Settings { brightness: 0.5, ..Settings::default() });
    assert!(pixels[0][0] > 150.0);
    let mut grey = vec![[200.0, 50.0, 50.0]];
    adjust(&mut grey, &Settings { saturation: -1.0, ..Settings::default() });
    assert!((grey[0][0] - grey[0][1]).abs() < 0.5);
}

/// Every mode's picture of `LIMONITER_PICTURE`, as PNGs in `LIMONITER_OUT`, to look at.
#[test]
#[ignore]
fn show_every_mode() {
    let (Ok(input), Ok(out)) = (std::env::var("LIMONITER_PICTURE"), std::env::var("LIMONITER_OUT")) else { return };
    let image = crate::quantise::read_picture(&input, &std::fs::read(&input).unwrap()).unwrap();
    for machine in Machine::ALL {
        for &mode in machine.modes() {
            for dither in [Dither::None, Dither::Ordered, Dither::FloydSteinberg] {
                let started = std::time::Instant::now();
                let screen = convert(&image, &settings(mode, dither));
                let png = screen.everything("x").into_iter().find(|(n, _)| n == "x.png").unwrap().1;
                std::fs::write(format!("{out}/{mode:?}-{dither:?}.png"), png).unwrap();
                println!("{mode:?} {dither:?}: {:?}", started.elapsed());
            }
        }
    }
}

/// The tape loads in the built-in Spectrum: the loader runs, LOAD "" SCREEN$ reads the screen, and the Spectrum's
/// screen memory ends up the .scr byte for byte.
#[test]
fn the_spectrum_tape_shows_the_picture_on_a_spectrum() {
    let screen = convert(&sample(256, 192), &settings(Mode::Spectrum, Dither::Ordered));
    let files = screen.everything("pic");
    let scr = files[0].1.clone();
    let tap = files.iter().find(|(n, _)| n == "pic.tap").unwrap().1.clone();
    let run = crate::running::Runnable { machine: crate::running::Machine::Spectrum48, format: crate::running::Format::Tap, name: "pic.tap".into(), bytes: std::sync::Arc::new(tap) };
    let mut spectrum = crate::zx_machine::BuiltIn::start(&run).unwrap();
    let shown = |m: &crate::zx_machine::BuiltIn| (0..6912u16).all(|i| m.peek(0x4000 + i) == scr[usize::from(i)]);
    let mut frames = 0;
    while !shown(&spectrum) && frames < 4000 {
        spectrum.frame().unwrap();
        frames += 1;
    }
    let wrong: Vec<usize> = (0..6912usize).filter(|&i| spectrum.peek(0x4000 + i as u16) != scr[i]).collect();
    assert!(shown(&spectrum), "the screen wasn't loaded after {frames} frames: {} bytes differ, first {:?}; screen says {:?}", wrong.len(), &wrong[..wrong.len().min(10)], spectrum.screen_text());
}

#[test]
fn next_pictures_read_back_in_limoniter_and_keep_to_their_palette() {
    let image = sample(320, 256);
    for (mode, size) in [(Mode::Layer2, 256), (Mode::Layer2Wide, 40), (Mode::Layer2Hi, 256), (Mode::LoRes, 256)] {
        let screen = convert(&image, &Settings { palette_size: size, ..settings(mode, Dither::Ordered) });
        let next = screen.next_screen().unwrap();
        let colours = if mode == Mode::Layer2Hi { 16 } else { size };
        assert!(screen.palette.len() <= colours, "{mode:?}: {} colours", screen.palette.len());
        assert!(screen.values.iter().all(|&v| usize::from(v) < colours), "{mode:?}");
        // Never the see-through colour.
        assert!(screen.palette.iter().all(|&c| c >> 1 != 0xE3), "{mode:?}");
        assert_eq!(next.rgba(), screen.rgba(), "{mode:?}");
        let files = screen.everything("n");
        let nxi = &files.iter().find(|(f, _)| f == "n.nxi").unwrap().1;
        let picture = crate::pictures::next::decode("nxi", nxi).unwrap();
        let (w, _) = mode.size();
        let step = if mode == Mode::LoRes { 2 } else { 1 };
        for (x, y) in [(0, 0), (w / 2, 40), (w - 1, 90)] {
            let at = ((y * step) * picture.width + x * step) * 4;
            assert_eq!(&picture.rgba[at..at + 3], &screen.rgb(x, y), "{mode:?} {x},{y}");
        }
        assert!(files.iter().any(|(f, _)| f == "n.nex"), "{mode:?}: a .nex");
        // The places it didn't take are the Next's at reset, for sprites.
        if mode == Mode::Layer2Wide {
            assert_eq!(next.palette[200], crate::next_screen::default_palette()[200]);
        }
    }
}

#[test]
fn timex_hi_res_is_one_pair_its_bytes_alternating_between_two_screens() {
    let screen = convert(&sample(512, 192), &settings(Mode::TimexHires, Dither::FloydSteinberg));
    let (ink, paper) = screen.cells[0];
    assert!(ink >= 8 && paper == 15 - (ink - 8), "a BRIGHT pair, PAPER INK's opposite: {ink} {paper}");
    let scr = &screen.everything("h")[0].1;
    assert_eq!(scr.len(), 12289);
    assert_eq!(scr[12288], 6 | (ink & 7) << 3);
    for y in [0, 7, 8, 100, 191] {
        for x in (0..512).step_by(5) {
            let column = x / 8;
            let byte = scr[if column % 2 == 0 { 0 } else { 6144 } + crate::screen_designer::row_offset(y) + column / 2];
            assert_eq!(byte >> (7 - x % 8) & 1, screen.values[y * 512 + x], "{x},{y}");
        }
    }
}
