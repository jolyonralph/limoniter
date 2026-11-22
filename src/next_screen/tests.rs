use super::*;

/// A screen with something in every corner, in colours of its own.
fn sample(mode: NextMode) -> NextScreen {
    let mut screen = NextScreen::new(mode);
    let (w, h) = mode.size();
    let count = mode.colours();
    for y in 0..h {
        for x in 0..w {
            screen.pixels[y * w + x] = ((x * 7 + y * 3 + x * y) % count) as u8;
        }
    }
    for (i, colour) in screen.palette.iter_mut().take(count).enumerate() {
        *colour = ((i * 37 + 5) % 512) as u16;
    }
    screen
}

#[test]
fn every_mode_reads_back_as_limoniters_next_reader_sees_it() {
    for mode in NextMode::ALL {
        let screen = sample(mode);
        let nxi = screen.nxi();
        assert_eq!(nxi.len(), 512 + mode.memory());
        assert_eq!(NextScreen::read("x.nxi", &nxi).unwrap(), screen, "{mode:?}");
        let picture = crate::pictures::next::decode("nxi", &nxi).unwrap();
        // The reader shows LoRes at twice its size.
        let (w, h) = mode.size();
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1), (w / 3, h / 2)] {
            let (px, py) = if mode == NextMode::LoRes { (x * 2, y * 2) } else { (x, y) };
            let at = (py * picture.width + px) * 4;
            assert_eq!(&picture.rgba[at..at + 3], &screen.colour_of(screen.pixels[y * w + x]), "{mode:?} {x},{y}");
        }
        // The BMP too.
        assert_eq!(NextScreen::read("x.bmp", &screen.bmp()).unwrap().pixels, screen.pixels, "{mode:?}");
    }
    // 320×256 goes down the columns: the second byte is the pixel under the first.
    let wide = sample(NextMode::Layer2Wide);
    assert_eq!(wide.memory()[1], wide.pixels[320]);
    let hi = sample(NextMode::Layer2Hi);
    assert_eq!(hi.memory()[0], hi.pixels[0] << 4 | hi.pixels[1]);
    assert_eq!(hi.memory()[1], hi.pixels[640] << 4 | hi.pixels[641]);
}

#[test]
fn colours_are_nine_bits() {
    assert_eq!(rgb(0x1FF), [255, 255, 255]);
    assert_eq!(nearest_value([255, 0, 0]), 0b111_000_000);
    assert_eq!(nearest_value([0, 0, 255]), 0b000_000_111);
    assert_eq!(split(0b111_000_111), [0xE3, 1]);
    assert_eq!(palette_from_bytes(&NextScreen::new(NextMode::Layer2).palette_bytes()), default_palette());
}

#[test]
fn modes_change_keeping_what_fits() {
    let mut screen = sample(NextMode::Layer2Wide);
    let corner = screen.pixels[10 * 320 + 20];
    screen.change_mode(NextMode::Layer2);
    assert_eq!(screen.pixels.len(), 256 * 192);
    assert_eq!(screen.pixels[10 * 256 + 20], corner);
    screen.change_mode(NextMode::Layer2Hi);
    assert!(screen.pixels.iter().all(|&p| p < 16));
}

/// What a .nex holds of a bank: its 16K, after the 512-byte header and the banks before it in the file's order.
fn bank<'a>(nex: &'a [u8], stored: &[usize], wanted: usize) -> &'a [u8] {
    let order = |b: usize| [5, 2, 0, 1, 3, 4, 6, 7].iter().position(|&o| o == b).unwrap_or(b);
    let mut banks = stored.to_vec();
    banks.sort_by_key(|&b| order(b));
    let at = 512 + banks.iter().position(|&b| b == wanted).unwrap() * 16384;
    &nex[at..at + 16384]
}

#[test]
fn the_viewer_assembles_into_a_nex_holding_the_picture() {
    for mode in NextMode::ALL {
        let screen = sample(mode);
        let nex = screen.nex("pic").unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        assert_eq!(&nex[..8], b"NextV1.2");
        let memory = screen.memory();
        if mode == NextMode::LoRes {
            let five = bank(&nex, &[5, 2], 5);
            assert_eq!(&five[..6144], &memory[..6144]);
            assert_eq!(&five[0x2000..0x2000 + 6144], &memory[6144..]);
        } else {
            let banks: Vec<usize> = std::iter::once(2).chain(9..9 + mode.memory() / 16384).collect();
            let stored: Vec<u8> = (9..9 + mode.memory() / 16384).flat_map(|b| bank(&nex, &banks, b).to_vec()).collect();
            assert_eq!(stored, memory, "{mode:?}");
        }
        // The code in bank 2 at $8000: DI, then NEXTREG $07,3.
        let two = bank(&nex, if mode == NextMode::LoRes { &[5, 2] } else { &[2, 9] }, 2);
        assert_eq!(&two[..5], &[0xF3, 0xED, 0x91, 0x07, 0x03]);
    }
}

/// sjasmplus itself, if it's been built beside the other assemblers, makes the same .nex from the same source.
#[test]
fn sjasmplus_makes_the_same_nex() {
    let sjasmplus = std::path::Path::new("/root/asmref/sjasmplus/sjasmplus");
    if !sjasmplus.exists() {
        return;
    }
    for mode in NextMode::ALL {
        let screen = sample(mode);
        let folder = std::env::temp_dir().join(format!("limoniter-next-{}-{mode:?}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in screen.everything("pic") {
            std::fs::write(folder.join(name), bytes).unwrap();
        }
        let ours = std::fs::read(folder.join("pic.nex")).unwrap();
        std::fs::remove_file(folder.join("pic.nex")).unwrap();
        let output = crate::platform::command(sjasmplus).arg("--nologo").arg("pic.asm").current_dir(&folder).output().unwrap();
        assert!(output.status.success(), "{mode:?}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let theirs = std::fs::read(folder.join("pic.nex")).unwrap();
        assert!(ours == theirs, "{mode:?}: the .nex differs from sjasmplus's ({} and {} bytes)", ours.len(), theirs.len());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
