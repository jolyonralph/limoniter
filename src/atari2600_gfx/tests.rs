use super::*;
use std::path::Path;

fn assemble(source: &str) -> Vec<u8> {
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("t.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| (&p.message, source.lines().nth(p.line))));
    assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap())
}

/// A playfield with something in every part of it: a different shape and colours on each row.
fn busy_playfield(halves: Halves) -> Playfield {
    let mut field = Playfield::new(halves, 24, 8);
    for y in 0..24 {
        for x in 0..40 {
            if (x * 7 + y * 3) % 5 < 2 || x == y || x == 39 - y {
                field.set(x, y, true);
            }
        }
        field.colours[y] = (0x10 * (y % 15) as u8 + 0x1A) & 0xFE;
        field.backgrounds[y] = (0x80 + 2 * y as u8) & 0xFE;
    }
    field
}

fn animated_sprites() -> Sprites {
    let mut sprites = Sprites::default();
    let mut second = sprites.frames[0].clone();
    second.rows.iter_mut().for_each(|r| *r = r.rotate_left(1));
    second.colours = vec![0x86; 8];
    sprites.frames.push(second);
    sprites
}

#[test]
fn blocks_go_into_the_registers_as_the_tia_reads_them() {
    let one = |i: usize| {
        let mut b = [false; 20];
        b[i] = true;
        registers(&b)
    };
    assert_eq!(one(0), [0x10, 0, 0], "the left block is PF0's bit 4");
    assert_eq!(one(3), [0x80, 0, 0]);
    assert_eq!(one(4), [0, 0x80, 0], "PF1 goes left to right");
    assert_eq!(one(11), [0, 0x01, 0]);
    assert_eq!(one(12), [0, 0, 0x01], "PF2 goes right to left");
    assert_eq!(one(19), [0, 0, 0x80]);
    for pattern in [[0xA0, 0x5A, 0xC3], [0xF0, 0xFF, 0xFF], [0x30, 0x01, 0x80]] {
        assert_eq!(registers(&blocks_of(pattern)), pattern);
    }
}

#[test]
fn the_halves_follow_the_left() {
    let mut field = Playfield::new(Halves::Mirrored, 2, 1);
    field.set(0, 0, true);
    assert!(field.pixel(39, 0));
    field.set(30, 1, true);
    assert!(field.pixel(9, 1) && field.pixel(30, 1));
    field.set_halves(Halves::Repeated);
    assert!(field.pixel(20, 0) && !field.pixel(39, 0) && field.pixel(29, 1));
    field.set_halves(Halves::Asymmetric);
    field.set(25, 0, true);
    assert!(!field.pixel(5, 0));
    assert_eq!(field.row_registers(0)[3..], registers(&field.pixels[20..40]));
}

#[test]
fn playfields_and_sprites_come_back_from_their_source() {
    for halves in Halves::ALL {
        let field = busy_playfield(halves);
        for order in [Order::TopFirst, Order::BottomFirst] {
            let source = field.source("title", order);
            assemble(&format!("\tprocessor 6502\n\torg $F000\n{source}"));
            assert_eq!(Playfield::from_source(&source).as_ref(), Some(&field), "{halves:?} {order:?}\n{source}");
            assert_eq!(field.bytes(order).len(), 24 * if halves == Halves::Asymmetric { 8 } else { 5 });
        }
    }
    let mut sprites = animated_sprites();
    for shared in [false, true] {
        if shared {
            sprites.frames[1].colours = sprites.frames[0].colours.clone();
        }
        for order in [Order::TopFirst, Order::BottomFirst] {
            let source = sprites.source("ship", order);
            assemble(&format!("\tprocessor 6502\n\torg $F000\n{source}"));
            assert_eq!(source.contains("ship_colours:"), shared);
            let back = Sprites::from_source(&source, 3).unwrap();
            assert_eq!(back.frames, sprites.frames, "{order:?}\n{source}");
        }
    }
    // Bytes from anywhere, cut into frames of the height asked.
    let back = Sprites::from_source("  .byte $18,$3C\n  dc.b %01111110, 255\n  byte 1,2\n", 3).unwrap();
    assert_eq!(back.frames.iter().map(|f| f.rows.clone()).collect::<Vec<_>>(), vec![vec![0x18, 0x3C, 0x7E], vec![0xFF, 1, 2]]);
}

#[test]
fn the_programs_are_cartridges_whose_lines_keep_to_their_cycles() {
    for tv in Tv::ALL {
        for halves in Halves::ALL {
            let rom = assemble(&busy_playfield(halves).program(tv, "test"));
            assert_eq!(rom.len(), 4096);
            let listing = crate::atari2600::disassemble(&crate::atari2600::recognise("t.a26", &rom).unwrap());
            assert!(!listing.contains("past the scanline"), "{listing}");
        }
        for sprites in [Sprites::default(), animated_sprites()] {
            let rom = assemble(&sprites.program(tv, "test"));
            assert_eq!(rom.len(), 4096);
        }
    }
}

#[test]
fn a_picture_of_a_playfield_becomes_that_playfield() {
    for halves in Halves::ALL {
        let field = busy_playfield(halves);
        let tv = Tv::Ntsc;
        let values = field.picture(tv);
        let top = field.top(tv);
        let tall = field.rows * field.lines;
        let rgba: Vec<u8> = values[top * 160..(top + tall) * 160].iter().flat_map(|&v| {
            let [r, g, b] = tv.rgb(v);
            [r, g, b, 255]
        }).collect();
        let image = Image { width: 160, height: tall, rgba };
        let back = Playfield::from_picture(&image, tv, halves, field.rows, field.lines, Fit::Stretch, false);
        assert_eq!(back.picture(tv), values, "{halves:?}");
    }
}

/// The cartridge run in MAME (when it's installed) for two seconds, and the last frame it showed.
fn mame_shows(name: &str, rom: &[u8], tv: Tv) -> Option<Image> {
    let program = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("mame")).find(|p| p.is_file()))?;
    let folder = std::env::temp_dir().join(format!("limoniter-2600gfx-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("cart.bin"), rom).unwrap();
    let output = std::process::Command::new(program)
        .args([if tv == Tv::Ntsc { "a2600" } else { "a2600p" }, "-cart", "cart.bin", "-video", "none", "-sound", "none", "-seconds_to_run", "2", "-nothrottle", "-skip_gameinfo", "-snapshot_directory", "snap", "-nvram_directory", "nv", "-cfg_directory", "cfg"])
        .current_dir(&folder)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let shot = std::fs::read(folder.join(if tv == Tv::Ntsc { "snap/a2600/0000.png" } else { "snap/a2600p/0000.png" })).unwrap();
    if let Some(out) = std::env::var_os("LIMONITER_OUT") {
        std::fs::write(Path::new(&out).join(format!("{name}.png")), &shot).unwrap();
    }
    let _ = std::fs::remove_dir_all(&folder);
    Some(crate::quantise::read_picture("shot.png", &shot).unwrap())
}

/// Whether the screenshot holds the picture expected (colour values, `width` across, None for "whatever's there")
/// somewhere in it, each value always the same colour in MAME's palette and no two values the same colour unless
/// the TV shows them alike (PAL's greys).
fn shows(shot: &Image, expected: &[Option<u8>], width: usize, tv: Tv) -> bool {
    let height = expected.len() / width;
    let at = |x: usize, y: usize| -> [u8; 3] {
        let i = (y * shot.width + x) * 4;
        [shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]]
    };
    if shot.width < width || shot.height < height {
        return false;
    }
    for top in 0..=shot.height - height {
        'place: for left in 0..=shot.width - width {
            let mut colour_of = std::collections::HashMap::new();
            let mut value_of = std::collections::HashMap::new();
            for y in 0..height {
                for x in 0..width {
                    let Some(value) = expected[y * width + x] else { continue };
                    let seen = at(left + x, top + y);
                    if *colour_of.entry(value).or_insert(seen) != seen || *value_of.entry(seen).or_insert(tv.rgb(value)) != tv.rgb(value) {
                        continue 'place;
                    }
                }
            }
            return true;
        }
    }
    false
}

#[test]
fn mame_shows_the_playfield_as_drawn() {
    for (halves, tv) in Halves::ALL.into_iter().map(|h| (h, Tv::Ntsc)).chain([(Halves::Asymmetric, Tv::Pal)]) {
        let mut field = busy_playfield(halves);
        if tv == Tv::Pal {
            field.set_rows(28);
        }
        let rom = assemble(&field.program(tv, "test"));
        let Some(shot) = mame_shows(&format!("playfield-{halves:?}-{tv:?}"), &rom, tv) else { return };
        let top = field.top(tv);
        let tall = field.rows * field.lines;
        let expected: Vec<Option<u8>> = field.picture(tv)[top * 160..(top + tall) * 160].iter().map(|&v| Some(v)).collect();
        assert!(shows(&shot, &expected, 160, tv), "{halves:?} {tv:?}: {}\u{D7}{}", shot.width, shot.height);
    }
}

#[test]
fn mame_shows_the_sprite_as_drawn() {
    let mut sprites = Sprites { background: 0x84, ..Sprites::default() };
    for size in Size::ALL {
        sprites.size = size;
        let rom = assemble(&sprites.program(Tv::Ntsc, "test"));
        let Some(shot) = mame_shows(&format!("sprite-{size:?}"), &rom, Tv::Ntsc) else { return };
        // The sprite with a pixel of background round it.
        let width = 8 * size.clocks() + 2;
        let inner = sprites.picture(0, 8 * size.clocks());
        let tall = sprites.height * sprites.lines + 2;
        let mut expected = vec![Some(sprites.background); width * tall];
        for y in 0..tall - 2 {
            for x in 0..width - 2 {
                expected[(y + 1) * width + x + 1] = Some(inner[y * (width - 2) + x].unwrap_or(sprites.background));
            }
        }
        assert!(shows(&shot, &expected, width, Tv::Ntsc), "{size:?}");
    }
}

/// A 6507 running a cartridge just far enough to count its frames: the instructions these programs use, with
/// their cycles (a cycle more for a page crossed, or a branch taken), WSYNC holding the processor to the end of
/// the line, and the RIOT's timer (read as N-1 the cycle after N is written, then a count less every 64). Each
/// frame's lines from VSYNC on to VSYNC on, the lines VSYNC was on, the lines the picture was on, and the first
/// byte of RAM as the frame started.
fn frames_of(rom: &[u8], count: usize) -> Vec<(usize, usize, usize, u8)> {
    let mut ram = [0u8; 128];
    let (mut a, mut x, mut y) = (0u8, 0u8, 0u8);
    let (mut zero, mut negative, mut carry) = (false, false, false);
    let read16 = |rom: &[u8], at: u16| u16::from(rom[usize::from(at) & 0xFFF]) | u16::from(rom[(usize::from(at) + 1) & 0xFFF]) << 8;
    let mut pc = read16(rom, 0xFFFC);
    let mut t: u64 = 0;
    let mut timer: (u64, u8) = (0, 0);
    let (mut vsync_on, mut vblank_on) = (false, true);
    let mut frame_start: Option<u64> = None;
    let (mut vsync_from, mut vsync_lines, mut picture_from, mut picture_lines) = (0u64, 0u64, 0u64, 0u64);
    let mut out = Vec::new();
    while out.len() < count && t < 80_000_000 {
        let op = rom[usize::from(pc) & 0xFFF];
        let byte = |i: u16| rom[(usize::from(pc) + usize::from(i)) & 0xFFF];
        let word = u16::from(byte(1)) | u16::from(byte(2)) << 8;
        let read = |address: u16, ram: &[u8; 128], now: u64| -> u8 {
            let address = address & 0x1FFF;
            if address & 0x1000 != 0 {
                rom[usize::from(address) & 0xFFF]
            } else if address & 0x280 == 0x280 {
                let (written, n) = timer;
                let ticks = (now - written - 1) / 64;
                u8::try_from(i64::from(n) - 1 - ticks as i64).unwrap_or(0xFF)
            } else if address & 0x80 != 0 {
                ram[usize::from(address & 0x7F)]
            } else {
                0
            }
        };
        // Each instruction: its length, cycles, and what it does; writes happen on its last cycle.
        let (length, mut cycles): (u16, u64) = match op {
            0xA9 | 0xA2 | 0xA0 | 0xC9 | 0xC0 | 0xE0 => (2, 2),
            0xA5 | 0xA6 | 0xA4 | 0x85 | 0x86 | 0x84 => (2, 3),
            0x95 => (2, 4),
            0xC6 | 0xE6 => (2, 5),
            0xB1 => (2, 5),
            0xAD | 0x8D | 0x2C | 0xBD | 0xB9 | 0xBE => (3, 4),
            0x4C => (3, 3),
            0xCA | 0x88 | 0xC8 | 0xE8 | 0x78 | 0xD8 | 0x9A | 0xEA => (1, 2),
            0xD0 | 0x10 | 0x90 | 0xF0 | 0x30 | 0xB0 => (2, 2),
            _ => panic!("${op:02X} at ${pc:04X} isn't modelled"),
        };
        let mut next = pc.wrapping_add(length);
        let mut store: Option<(u16, u8)> = None;
        let crossed = |base: u16, index: u8| (base & 0xFF00) != (base.wrapping_add(u16::from(index)) & 0xFF00);
        let set = |v: u8, zero: &mut bool, negative: &mut bool| {
            *zero = v == 0;
            *negative = v & 0x80 != 0;
            v
        };
        match op {
            0xA9 => a = set(byte(1), &mut zero, &mut negative),
            0xA2 => x = set(byte(1), &mut zero, &mut negative),
            0xA0 => y = set(byte(1), &mut zero, &mut negative),
            0xA5 => a = set(read(u16::from(byte(1)), &ram, t + 2), &mut zero, &mut negative),
            0xA6 => x = set(read(u16::from(byte(1)), &ram, t + 2), &mut zero, &mut negative),
            0xA4 => y = set(read(u16::from(byte(1)), &ram, t + 2), &mut zero, &mut negative),
            0xAD => a = set(read(word, &ram, t + 3), &mut zero, &mut negative),
            0x2C => zero = a & read(word, &ram, t + 3) == 0,
            0xBD | 0xB9 | 0xBE => {
                let index = if op == 0xBD { x } else { y };
                if crossed(word, index) {
                    cycles += 1;
                }
                let v = read(word.wrapping_add(u16::from(index)), &ram, t + cycles - 1);
                if op == 0xBE {
                    x = set(v, &mut zero, &mut negative);
                } else {
                    a = set(v, &mut zero, &mut negative);
                }
            }
            0xB1 => {
                let base = u16::from(ram[usize::from(byte(1) & 0x7F)]) | u16::from(ram[usize::from(byte(1).wrapping_add(1) & 0x7F)]) << 8;
                if crossed(base, y) {
                    cycles += 1;
                }
                a = set(read(base.wrapping_add(u16::from(y)), &ram, t + cycles - 1), &mut zero, &mut negative);
            }
            0x85 => store = Some((u16::from(byte(1)), a)),
            0x86 => store = Some((u16::from(byte(1)), x)),
            0x84 => store = Some((u16::from(byte(1)), y)),
            0x95 => store = Some((u16::from(byte(1).wrapping_add(x)), a)),
            0x8D => store = Some((word, a)),
            0xC6 | 0xE6 => {
                let at = usize::from(byte(1) & 0x7F);
                ram[at] = set(if op == 0xC6 { ram[at].wrapping_sub(1) } else { ram[at].wrapping_add(1) }, &mut zero, &mut negative);
            }
            0xC9 | 0xC0 | 0xE0 => {
                let register = match op {
                    0xC9 => a,
                    0xC0 => y,
                    _ => x,
                };
                carry = register >= byte(1);
                set(register.wrapping_sub(byte(1)), &mut zero, &mut negative);
            }
            0xCA => x = set(x.wrapping_sub(1), &mut zero, &mut negative),
            0x88 => y = set(y.wrapping_sub(1), &mut zero, &mut negative),
            0xE8 => x = set(x.wrapping_add(1), &mut zero, &mut negative),
            0xC8 => y = set(y.wrapping_add(1), &mut zero, &mut negative),
            0x4C => next = word,
            0xD0 | 0x10 | 0x90 | 0xF0 | 0x30 | 0xB0 => {
                let taken = match op {
                    0xD0 => !zero,
                    0xF0 => zero,
                    0x10 => !negative,
                    0x30 => negative,
                    0x90 => !carry,
                    _ => carry,
                };
                if taken {
                    let to = next.wrapping_add(i16::from(byte(1) as i8) as u16);
                    cycles += if (to & 0xFF00) != (next & 0xFF00) { 2 } else { 1 };
                    next = to;
                }
            }
            _ => {}
        }
        let written = t + cycles - 1;
        t += cycles;
        if let Some((address, value)) = store {
            let address = address & 0x1FFF;
            if address & 0x280 == 0x280 {
                timer = (written, value);
            } else if address & 0x80 != 0 && address & 0x200 == 0 {
                ram[usize::from(address & 0x7F)] = value;
            } else {
                match address & 0x3F {
                    0x00 => {
                        let on = value & 2 != 0;
                        if on && !vsync_on {
                            if let Some(start) = frame_start {
                                out.push((((written - start) / 76) as usize, vsync_lines as usize, picture_lines as usize, ram[0]));
                            }
                            frame_start = Some(written);
                            vsync_from = written;
                        } else if !on && vsync_on {
                            vsync_lines = (written - vsync_from + 38) / 76;
                        }
                        vsync_on = on;
                    }
                    0x01 => {
                        let on = value & 2 != 0;
                        if !on && vblank_on {
                            picture_from = written;
                        } else if on && !vblank_on {
                            picture_lines = (written - picture_from).div_ceil(76);
                        }
                        vblank_on = on;
                    }
                    0x02 => t = (written / 76 + 1) * 76,
                    _ => {}
                }
            }
        }
        pc = next;
    }
    out
}

#[test]
fn every_frame_is_as_long_as_the_tv_wants() {
    for tv in Tv::ALL {
        let total = if tv == Tv::Ntsc { 262 } else { 312 };
        let mut programs: Vec<(String, String)> = Halves::ALL.into_iter().map(|h| (format!("{h:?}"), busy_playfield(h).program(tv, "t"))).collect();
        programs.push(("sprite".into(), Sprites::default().program(tv, "t")));
        programs.push(("frames".into(), animated_sprites().program(tv, "t")));
        for (name, source) in programs {
            let frames = frames_of(&assemble(&source), 6);
            assert_eq!(frames.len(), 6, "{name} {tv:?}");
            for &(lines, vsync, _, _) in &frames[1..] {
                assert_eq!((lines, vsync), (total, 3), "{name} {tv:?}: {frames:?}");
            }
        }
    }
    // The sprite's frames take turns, each for its 8 TV frames.
    let shown: Vec<u8> = frames_of(&assemble(&animated_sprites().program(Tv::Ntsc, "t")), 40).iter().map(|f| f.3).collect();
    let changes: Vec<usize> = (1..shown.len()).filter(|&i| shown[i] != shown[i - 1]).collect();
    assert!(changes.len() >= 3 && changes.windows(2).all(|w| w[1] - w[0] == 8), "{shown:?}");
    assert!(shown.iter().all(|&f| f < 2));
}
