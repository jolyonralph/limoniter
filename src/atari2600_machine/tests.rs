//! The built-in 2600, tested with cartridges made by Limoniter's own assembler: pictures checked a pixel at a
//! time against what the hardware draws, the timer, the ports, bank switching and sound; and, where MAME is
//! installed, the same cartridges' frames compared with MAME's.

use super::*;
use std::path::Path;

const EQUATES: &str = "
VSYNC = $00
VBLANK = $01
WSYNC = $02
RSYNC = $03
NUSIZ0 = $04
NUSIZ1 = $05
COLUP0 = $06
COLUP1 = $07
COLUPF = $08
COLUBK = $09
CTRLPF = $0A
REFP0 = $0B
REFP1 = $0C
PF0 = $0D
PF1 = $0E
PF2 = $0F
RESP0 = $10
RESP1 = $11
RESM0 = $12
RESM1 = $13
RESBL = $14
AUDC0 = $15
AUDF0 = $17
AUDV0 = $19
GRP0 = $1B
GRP1 = $1C
ENAM0 = $1D
ENAM1 = $1E
ENABL = $1F
HMP0 = $20
HMP1 = $21
HMM0 = $22
HMM1 = $23
HMBL = $24
VDELP0 = $25
VDELP1 = $26
VDELBL = $27
RESMP0 = $28
HMOVE = $2A
HMCLR = $2B
CXCLR = $2C
CXM0P = $00
CXP0FB = $02
CXPPMM = $07
INPT4 = $0C
SWCHA = $0280
SWCHB = $0282
INTIM = $0284
TIMINT = $0285
TIM1T = $0294
TIM8T = $0295
TIM64T = $0296
T1024T = $0297
";

fn assemble(source: &str) -> Vec<u8> {
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("t.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| (&p.message, source.lines().nth(p.line))));
    assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap())
}

/// A 4K cartridge whose frames are 262 lines: three of vertical sync; `setup` (which may end lines of its own,
/// `vblank` being how many more make the 37 of the vertical blank); the picture on from line 37 for `kernel`,
/// which must end 192 lines; then 30 of overscan. The frame's line 0 is the line the vertical sync ends in.
fn program(setup: &str, vblank: u32, kernel: &str) -> Vec<u8> {
    let source = format!(
        "        processor 6502
{EQUATES}
        org $F000
Reset:  sei
        cld
        ldx #$FF
        txs
        lda #0
Clear:  sta $00,x
        dex
        bne Clear
Frame:  lda #2
        sta VBLANK
        sta VSYNC
        sta WSYNC
        sta WSYNC
        sta WSYNC
        lda #0
        sta VSYNC
{setup}
        ldx #{vblank}
VB:     sta WSYNC
        dex
        bne VB
        lda #0
        sta VBLANK
{kernel}
        lda #2
        sta VBLANK
        ldx #30
OS:     sta WSYNC
        dex
        bne OS
        jmp Frame
        org $FFFC
        .word Reset
        .word Reset
"
    );
    let rom = assemble(&source);
    assert_eq!(rom.len(), 4096);
    rom
}

/// A kernel that only waits out its 192 lines.
const PLAIN: &str = "        ldy #192
Line:   sta WSYNC
        dey
        bne Line";

/// Runs a cartridge for some frames; the last.
fn frame_after(rom: &[u8], frames: usize) -> (Atari2600, Frame) {
    let mut atari = Atari2600::from_rom(rom);
    for _ in 0..frames {
        atari.frame().unwrap();
    }
    let frame = atari.last_frame().clone();
    (atari, frame)
}

fn row(frame: &Frame, y: usize) -> &[u8] {
    &frame.pixels[y * WIDTH..(y + 1) * WIDTH]
}

/// A line drawn from a 20-bit playfield pattern (PF0's high half, PF1 backwards, PF2), repeated or reflected.
fn playfield_line(pf0: u8, pf1: u8, pf2: u8, reflect: bool, fg: u8, bg: u8) -> Vec<u8> {
    let pattern = u32::from(pf0 >> 4) | u32::from(pf1.reverse_bits()) << 4 | u32::from(pf2) << 12;
    (0..160)
        .map(|x| {
            let i = x / 4;
            let bit = if i < 20 { i } else if reflect { 39 - i } else { i - 20 };
            if pattern >> bit & 1 != 0 { fg } else { bg }
        })
        .collect()
}

#[test]
fn frames_have_the_lines_the_program_makes_and_the_picture_starts_where_vblank_ends() {
    let rom = program("        lda #$84\n        sta COLUBK", 37, PLAIN);
    let (atari, frame) = frame_after(&rom, 5);
    assert_eq!(frame.lines, 262);
    assert_eq!(atari.tv, Tv::Ntsc);
    for y in 0..262 {
        let expected = if (37..229).contains(&y) { 0x84 } else { 0 };
        assert!(row(&frame, y).iter().all(|&v| v == expected), "line {y}: {:?}", &row(&frame, y)[..8]);
    }
}

#[test]
fn the_playfield_is_drawn_repeated_and_reflected() {
    for reflect in [false, true] {
        let setup = format!(
            "        lda #$90\n        sta PF0\n        lda #$A5\n        sta PF1\n        lda #$3C\n        sta PF2\n        lda #$1E\n        sta COLUPF\n        lda #$84\n        sta COLUBK\n        lda #{}\n        sta CTRLPF",
            u8::from(reflect)
        );
        let (_, frame) = frame_after(&program(&setup, 37, PLAIN), 3);
        let expected = playfield_line(0x90, 0xA5, 0x3C, reflect, 0x1E, 0x84);
        for y in 37..229 {
            assert_eq!(row(&frame, y), &expected[..], "line {y}, reflected {reflect}");
        }
    }
}

#[test]
fn score_mode_colours_each_half_as_its_player() {
    let setup = "        lda #$FF\n        sta PF0\n        sta PF1\n        sta PF2\n        lda #$1E\n        sta COLUPF\n        lda #$44\n        sta COLUP0\n        lda #$C6\n        sta COLUP1\n        lda #2\n        sta CTRLPF";
    let (_, frame) = frame_after(&program(setup, 37, PLAIN), 3);
    let line = row(&frame, 100);
    assert!(line[..80].iter().all(|&v| v == 0x44) && line[80..].iter().all(|&v| v == 0xC6), "{line:?}");
}

/// Positions player 0 on line 1 with a delay loop of `n` rounds: RESP0 is written at cycle 5n+4 of the line.
fn positioned(n: u8, graphics: u8, nusiz: u8, more: &str) -> Vec<u8> {
    let setup = format!(
        "        lda #{graphics}\n        sta GRP0\n        lda #$46\n        sta COLUP0\n        lda #$84\n        sta COLUBK\n        lda #{nusiz}\n        sta NUSIZ0\n{more}\n        sta WSYNC\n        ldx #{n}\nP:      dex\n        bne P\n        sta RESP0"
    );
    program(&setup, 36, PLAIN)
}

/// Where the pixels of a colour are in a line.
fn where_is(line: &[u8], colour: u8) -> Vec<usize> {
    line.iter().enumerate().filter(|(_, v)| **v == colour).map(|(x, _)| x).collect()
}

#[test]
fn a_player_is_drawn_five_clocks_after_resp0_is_written() {
    for n in [5u8, 8, 12] {
        let (_, frame) = frame_after(&positioned(n, 0xC3, 0, ""), 3);
        // RESP0 at cycle c = 5n + 4 is colour clock 3c, pixel 3c - 68; the player is drawn five clocks on from
        // there, from the next line (as MAME draws it too).
        let x = 3 * (5 * usize::from(n) + 4) - 68 + 5;
        assert_eq!(where_is(row(&frame, 100), 0x46), vec![x, x + 1, x + 6, x + 7], "{n} rounds");
    }
}

#[test]
fn nusiz_copies_and_stretches_the_player() {
    let x = 3 * (5 * 8 + 4) - 68 + 5;
    let cases: [(u8, Vec<usize>); 6] = [
        (1, vec![0, 16]),
        (2, vec![0, 32]),
        (3, vec![0, 16, 32]),
        (4, vec![0, 64]),
        (6, vec![0, 32, 64]),
        (0, vec![0]),
    ];
    for (nusiz, copies) in cases {
        let (_, frame) = frame_after(&positioned(8, 0x81, nusiz, ""), 3);
        let expected: Vec<usize> = copies.iter().flat_map(|c| [x + c, x + c + 7]).collect();
        assert_eq!(where_is(row(&frame, 100), 0x46), expected, "NUSIZ {nusiz}");
    }
    // Twice and four times as wide, a pixel further right than a player of one width.
    let (_, frame) = frame_after(&positioned(8, 0x81, 5, ""), 3);
    assert_eq!(where_is(row(&frame, 100), 0x46), vec![x + 1, x + 2, x + 15, x + 16]);
    let (_, frame) = frame_after(&positioned(8, 0x81, 7, ""), 3);
    assert_eq!(where_is(row(&frame, 100), 0x46), vec![x + 1, x + 2, x + 3, x + 4, x + 29, x + 30, x + 31, x + 32]);
}

#[test]
fn refp0_reflects_the_player() {
    let x = 3 * (5 * 8 + 4) - 68 + 5;
    let (_, frame) = frame_after(&positioned(8, 0xE0, 0, "        lda #8\n        sta REFP0"), 3);
    assert_eq!(where_is(row(&frame, 100), 0x46), vec![x + 5, x + 6, x + 7]);
    let (_, frame) = frame_after(&positioned(8, 0xE0, 0, ""), 3);
    assert_eq!(where_is(row(&frame, 100), 0x46), vec![x, x + 1, x + 2]);
}

#[test]
fn hmove_moves_by_hmp0_and_blanks_the_left_eight_pixels() {
    let x = 3 * (5 * 8 + 4) - 68 + 5;
    // HMOVE once in the vertical blank, after the player's been put in place.
    for (hm, moved) in [(0x00u8, 0i32), (0x10, -1), (0x70, -7), (0x80, 8), (0xF0, 1)] {
        let more = format!("        lda #{hm}\n        sta HMP0");
        let setup = format!(
            "        lda #$C3\n        sta GRP0\n        lda #$46\n        sta COLUP0\n        lda #$84\n        sta COLUBK\n{more}\n        sta WSYNC\n        ldx #8\nP:      dex\n        bne P\n        sta RESP0\n        sta WSYNC\n        sta HMOVE"
        );
        let (_, frame) = frame_after(&program(&setup, 35, PLAIN), 3);
        let at = (x + moved) as usize;
        assert_eq!(where_is(row(&frame, 100), 0x46), vec![at, at + 1, at + 6, at + 7], "HMP0 {hm:02X}");
    }
    // HMOVE on every line: the first eight pixels black.
    let kernel = "        ldy #192\nLine:   sta WSYNC\n        sta HMOVE\n        dey\n        bne Line";
    let (_, frame) = frame_after(&program("        lda #$84\n        sta COLUBK", 37, kernel), 3);
    let line = row(&frame, 100);
    assert!(line[..8].iter().all(|&v| v == 0) && line[8..].iter().all(|&v| v == 0x84), "{line:?}");
}

#[test]
fn missiles_and_the_ball_are_a_pixel_left_of_a_player_and_as_wide_as_asked() {
    let x = 3 * (5 * 8 + 4) - 68 + 5;
    for (width, nusiz) in [(1usize, 0x00u8), (2, 0x10), (4, 0x20), (8, 0x30)] {
        let setup = format!(
            "        lda #2\n        sta ENAM0\n        lda #$46\n        sta COLUP0\n        lda #{nusiz}\n        sta NUSIZ0\n        sta WSYNC\n        ldx #8\nP:      dex\n        bne P\n        sta RESM0"
        );
        let (_, frame) = frame_after(&program(&setup, 36, PLAIN), 3);
        assert_eq!(where_is(row(&frame, 100), 0x46), (x - 1..x - 1 + width).collect::<Vec<_>>(), "missile {width} wide");
        let setup = format!(
            "        lda #2\n        sta ENABL\n        lda #$1E\n        sta COLUPF\n        lda #{nusiz}\n        sta CTRLPF\n        sta WSYNC\n        ldx #8\nP:      dex\n        bne P\n        sta RESBL"
        );
        let (_, frame) = frame_after(&program(&setup, 36, PLAIN), 3);
        assert_eq!(where_is(row(&frame, 100), 0x1E), (x - 1..x - 1 + width).collect::<Vec<_>>(), "ball {width} wide");
    }
}

#[test]
fn vdelp0_shows_the_graphics_from_before_grp1_was_last_written() {
    // GRP0 is $F0 and then $0F; GRP1 written after: with VDELP0, player 0 shows the $0F copied when GRP1 was
    // written; written again first, it would show $F0.
    let x = 3 * (5 * 8 + 4) - 68 + 5;
    for (vdel, order, expected) in [
        (0u8, "        lda #$F0\n        sta GRP0\n        sta GRP1\n        lda #$0F\n        sta GRP0", vec![x + 4, x + 5, x + 6, x + 7]),
        (1, "        lda #$F0\n        sta GRP0\n        sta GRP1\n        lda #$0F\n        sta GRP0", vec![x, x + 1, x + 2, x + 3]),
        (1, "        lda #$0F\n        sta GRP0\n        sta GRP1\n        lda #$F0\n        sta GRP0", vec![x + 4, x + 5, x + 6, x + 7]),
    ] {
        let more = format!("        lda #{vdel}\n        sta VDELP0\n        lda #$00\n        sta COLUP1\n{order}");
        let (_, frame) = frame_after(&positioned(8, 0, 0, &more), 3);
        assert_eq!(where_is(row(&frame, 100), 0x46), expected, "VDELP0 {vdel}: {order}");
    }
}

#[test]
fn collisions_are_seen_where_objects_meet_and_cleared_by_cxclr() {
    // Player 0 over a full playfield: the collision's read on the last line and kept in RAM.
    let setup = "        lda #$FF\n        sta PF2\n        lda #$81\n        sta GRP0\n        sta CXCLR\n        sta WSYNC\n        ldx #8\nP:      dex\n        bne P\n        sta RESP0";
    let kernel = "        ldy #192\nLine:   sta WSYNC\n        dey\n        bne Line\n        lda CXP0FB\n        sta $80\n        lda CXPPMM\n        sta $81\n        sta CXCLR";
    let (atari, _) = frame_after(&program(setup, 36, kernel), 3);
    assert_eq!(atari.peek(0x80) & 0xC0, 0x80, "player 0 and the playfield");
    assert_eq!(atari.peek(0x81) & 0xC0, 0x00, "no player 1");
}

#[test]
fn vblank_blacks_the_picture_out() {
    let kernel = "        ldy #96\nA1:     sta WSYNC\n        dey\n        bne A1\n        lda #2\n        sta VBLANK\n        ldy #96\nA2:     sta WSYNC\n        dey\n        bne A2";
    let (_, frame) = frame_after(&program("        lda #$84\n        sta COLUBK", 37, kernel), 3);
    assert!(row(&frame, 100).iter().all(|&v| v == 0x84));
    assert!(row(&frame, 140).iter().all(|&v| v == 0));
}

#[test]
fn the_timer_counts_down_by_its_interval_and_then_every_cycle() {
    let mut riot = Riot::default();
    riot.write(0x296, 3); // TIM64T
    riot.tick();
    assert_eq!(riot.read(0x284, 0, 0), 2, "a step the cycle after it's written");
    for _ in 0..63 {
        riot.tick();
    }
    assert_eq!(riot.read(0x284, 0, 0), 2);
    riot.tick();
    assert_eq!(riot.read(0x284, 0, 0), 1, "then one every 64");
    for _ in 0..128 {
        riot.tick();
    }
    assert_eq!(riot.timer, 0xFF, "past 0 it goes to $FF");
    assert_eq!(riot.read(0x285, 0, 0) & 0x80, 0x80, "and TIMINT says so");
    riot.tick();
    riot.tick();
    assert_eq!(riot.timer, 0xFD, "and counts every cycle");
    // Reading INTIM puts it back to its interval.
    riot.read(0x284, 0, 0);
    riot.tick();
    let after = riot.timer;
    riot.tick();
    riot.tick();
    assert_eq!(riot.timer, after, "64 cycles a step again");
    for (address, interval) in [(0x294u16, 1u32), (0x295, 8), (0x296, 64), (0x297, 1024)] {
        let mut riot = Riot::default();
        riot.write(address, 10);
        for _ in 0..1 + 4 * interval {
            riot.tick();
        }
        assert_eq!(riot.timer, 5, "interval {interval}");
    }
}

#[test]
fn frames_timed_by_the_riot_are_as_long_as_the_timer_makes_them() {
    // The starter's frame: the vertical blank and overscan timed by TIM64T. Every frame the same length, and its
    // bands move up a line a frame: the colour at a line now was the next line's a frame ago.
    let rom = assemble(crate::atari2600::STARTER);
    let mut atari = Atari2600::from_rom(&rom);
    let mut lengths = Vec::new();
    for _ in 0..10 {
        atari.frame().unwrap();
        lengths.push(atari.last_frame().lines);
    }
    assert!(lengths[2..].iter().all(|&n| n == lengths[2]) && (259..=263).contains(&lengths[2]), "{lengths:?}");
    let before = atari.last_frame().clone();
    atari.frame().unwrap();
    let after = atari.last_frame().clone();
    assert_eq!(row(&after, 101)[0], row(&before, 101)[0].wrapping_add(2) & 0xFE);
}

/// The programs the sprite and playfield editor writes (whose timing the editor's own tests count as 262 and 312
/// lines a frame) run here as long, and show exactly the picture the editor drew: the two-halves playfield's
/// registers are rewritten as each line is drawn, so it's a test of when each write takes hold.
#[test]
fn the_sprite_and_playfield_editor_s_programs_show_what_it_drew() {
    use crate::atari2600_gfx::{Halves, Playfield};
    for (halves, tv) in Halves::ALL.into_iter().map(|h| (h, Tv::Ntsc)).chain([(Halves::Asymmetric, Tv::Pal)]) {
        let mut field = Playfield::new(halves, if tv == Tv::Pal { 28 } else { 24 }, 8);
        for y in 0..field.rows {
            for x in 0..40 {
                if (x * 7 + y * 3) % 5 < 2 || x == y || x == 39 - y {
                    field.set(x, y, true);
                }
            }
            field.colours[y] = (0x10 * (y % 15) as u8 + 0x1A) & 0xFE;
            field.backgrounds[y] = (0x80 + 2 * y as u8) & 0xFE;
        }
        let rom = assemble(&field.program(tv, "test"));
        let (atari, frame) = frame_after(&rom, 4);
        assert_eq!((frame.lines, atari.tv), (if tv == Tv::Pal { 312 } else { 262 }, tv), "{halves:?}");
        let top = field.top(tv);
        let tall = field.rows * field.lines;
        let wanted = &field.picture(tv)[top * WIDTH..(top + tall) * WIDTH];
        let found = (0..frame.lines - tall).any(|y| &frame.pixels[y * WIDTH..(y + tall) * WIDTH] == wanted);
        assert!(found, "{halves:?} {tv:?}");
    }
}

#[test]
fn the_joysticks_and_switches_are_read_through_swcha_swchb_and_inpt4() {
    let kernel = "        ldy #192\nLine:   sta WSYNC\n        dey\n        bne Line\n        lda SWCHA\n        sta $80\n        lda SWCHB\n        sta $81\n        lda INPT4\n        sta $82";
    let rom = program("", 37, kernel);
    let mut atari = Atari2600::from_rom(&rom);
    atari.frame().unwrap();
    atari.frame().unwrap();
    assert_eq!((atari.peek(0x80), atari.peek(0x81) & 0xCB, atari.peek(0x82) & 0x80), (0xFF, 0x0B, 0x80));
    atari.controls().sticks[0] = [true, false, false, true];
    atari.controls().reset = true;
    atari.controls().difficulty_a[1] = true;
    atari.set_fire(0, true);
    atari.frame().unwrap();
    atari.frame().unwrap();
    assert_eq!(atari.peek(0x80), 0b0110_1111, "up and right on the left joystick");
    assert_eq!(atari.peek(0x81) & 0xCB, 0x8A, "Reset pressed, the right difficulty at A");
    assert_eq!(atari.peek(0x82) & 0x80, 0, "the fire button");
}

/// An 8K F8 cartridge whose two banks each colour the background their own way and then switch to the other: the
/// frames take turns.
#[test]
fn f8_cartridges_switch_banks_at_their_hotspots() {
    let bank = |n: u8| {
        format!(
            "        org ${:X}000\n        rorg $F000\nStart{n}: sta $1FF9\n        jmp Frame{n}\nFrame{n}: lda #2\n        sta VSYNC\n        sta WSYNC\n        sta WSYNC\n        sta WSYNC\n        lda #0\n        sta VSYNC\n        lda #${}\n        sta COLUBK\n        ldx #255\nL{n}:     sta WSYNC\n        dex\n        bne L{n}\n        sta $1FF{}\n        jmp Frame{n}\n        org ${:X}FFC\n        rorg $FFFC\n        .word Start{n}\n        .word Start{n}\n",
            n,
            if n == 0 { "42" } else { "84" },
            if n == 0 { 9 } else { 8 },
            n
        )
    };
    let source = format!("        processor 6502\n{EQUATES}\n{}{}", bank(0), bank(1));
    let rom = assemble(&source);
    assert_eq!(rom.len(), 8192);
    assert_eq!(crate::atari2600::detect(&rom), Some(Scheme::F8));
    let mut atari = Atari2600::from_rom(&rom);
    // The first frame is only the lines before the first vertical sync.
    atari.frame().unwrap();
    let mut seen = Vec::new();
    for _ in 0..6 {
        atari.frame().unwrap();
        seen.push(row(atari.last_frame(), 100)[0]);
    }
    assert!(seen.windows(2).skip(1).all(|w| w[0] != w[1] && [0x42, 0x84].contains(&w[0])), "{seen:?}");
}

#[test]
fn bank_switching_schemes_map_the_cartridge_as_each_does() {
    // F6: the bank chosen by touching $1FF6-$1FF9; the Superchip's RAM written at $1000 and read at $1080.
    let mut rom = vec![0u8; 16384];
    for b in 0..4 {
        rom[b * 4096 + 0x200] = 0xB0 + b as u8;
    }
    let mut cart = Cartridge::new(rom.clone(), Scheme::F6);
    cart.read(0x1FF8, 0);
    assert_eq!(cart.read(0x1200, 0), 0xB2);
    cart.write(0x1FF6, 0);
    assert_eq!(cart.read(0x1200, 0), 0xB0);
    let mut cart = Cartridge::new(rom, Scheme::F6Sc);
    cart.write(0x1005, 0x5A);
    assert_eq!(cart.read(0x1085, 0), 0x5A);
    // E0: 1K slices, the last fixed.
    let rom: Vec<u8> = (0..8192).map(|i| (i / 1024) as u8).collect();
    let mut cart = Cartridge::new(rom, Scheme::E0);
    cart.read(0x1FE2, 0);
    cart.read(0x1FF5, 0);
    assert_eq!([cart.read(0x1000, 0), cart.read(0x1400, 0), cart.read(0x1800, 0), cart.read(0x1C00, 0)], [2, 5, 5, 7]);
    // 3F: the lower 2K chosen by writing to $3F.
    let rom: Vec<u8> = (0..8192).map(|i| (i / 2048) as u8).collect();
    let mut cart = Cartridge::new(rom, Scheme::ThreeF);
    cart.saw_tia_write(0x3F, 2);
    assert_eq!([cart.read(0x1000, 0), cart.read(0x1800, 0)], [2, 3]);
    // E7: 2K banks, its 1K of RAM, and 256 bytes of its other RAM.
    let rom: Vec<u8> = (0..16384).map(|i| (i / 2048) as u8).collect();
    let mut cart = Cartridge::new(rom, Scheme::E7);
    cart.read(0x1FE3, 0);
    assert_eq!([cart.read(0x1000, 0), cart.read(0x1A00, 0)], [3, 7]);
    cart.read(0x1FE7, 0);
    cart.write(0x1010, 9);
    assert_eq!(cart.read(0x1410, 0), 9);
    cart.read(0x1FE9, 0);
    cart.write(0x1820, 4);
    assert_eq!(cart.read(0x1920, 0), 4);
}

#[test]
fn a_pal_frame_is_shown_in_pal_colours() {
    // 312 lines: 3 of sync, 45 of blank, 228 shown, 36 of overscan.
    let source = format!(
        "        processor 6502\n{EQUATES}\n        org $F000\nReset:  ldx #$FF\n        txs\nFrame:  lda #2\n        sta VSYNC\n        sta WSYNC\n        sta WSYNC\n        sta WSYNC\n        lda #0\n        sta VSYNC\n        lda #$2A\n        sta COLUBK\n        ldx #45\nV:      sta WSYNC\n        dex\n        bne V\n        lda #0\n        sta VBLANK\n        ldx #228\nK:      sta WSYNC\n        dex\n        bne K\n        lda #2\n        sta VBLANK\n        ldx #36\nO:      sta WSYNC\n        dex\n        bne O\n        jmp Frame\n        org $FFFC\n        .word Reset\n        .word Reset\n"
    );
    let (mut atari, frame) = frame_after(&assemble(&source), 4);
    assert_eq!(frame.lines, 312);
    assert_eq!(atari.tv, Tv::Pal);
    assert_eq!(atari.size(), [160, 274]);
    let picture = atari.picture().to_vec();
    let (top, _) = window(Tv::Pal);
    let at = ((100 - top) * 160 + 10) * 4;
    assert_eq!(picture[at..at + 3], Tv::Pal.rgb(0x2A));
}

#[test]
fn breakpoints_stop_before_the_instruction_and_step_over_runs_a_subroutine_whole() {
    let source = format!(
        "        processor 6502\n{EQUATES}\n        org $F000\nReset:  ldx #$FF\n        txs\nLoop:   lda #1\n        jsr Sub\nAfter:  inc $80\n        jmp Loop\nSub:    inc $81\n        rts\n        org $FFFC\n        .word Reset\n        .word Reset\n"
    );
    let rom = assemble(&source);
    let mut atari = Atari2600::from_rom(&rom);
    atari.set_breakpoints([0xF005]);
    atari.frame().unwrap();
    assert!(atari.paused);
    assert_eq!(atari.cpu().pc, 0xF005, "at the JSR, not yet run");
    atari.step_over().unwrap();
    assert_eq!(atari.cpu().pc, 0xF008, "past the subroutine");
    assert_eq!(atari.peek(0x81), 1);
    atari.step().unwrap();
    assert_eq!((atari.cpu().pc, atari.peek(0x80)), (0xF00A, 1));
    atari.carry_on();
    atari.frame().unwrap();
    assert_eq!(atari.cpu().pc, 0xF005, "round again to the breakpoint");
    // Stopped, the picture shows the frame so far over the last.
    assert_eq!(atari.values().len(), 160 * 228);
    // A condition on a register: only when A is 2, which it never is.
    atari.set_conditions([(0xF005, "A = 2".to_string())].into(), None);
    atari.carry_on();
    atari.frame().unwrap();
    assert!(!atari.paused);
    // Watching a write.
    atari.set_conditions(BTreeMap::new(), None);
    atari.set_breakpoints([]);
    atari.set_write_watches([0x81]);
    atari.frame().unwrap();
    assert!(atari.paused);
    assert_eq!(atari.cpu().pc, 0xF00F, "after the INC");
    assert!(atari.stop_reason.as_deref().is_some_and(|r| r.contains("$0081")), "{:?}", atari.stop_reason);
}

#[test]
fn wsync_holds_the_processor_to_the_end_of_the_line() {
    let source = format!("        processor 6502\n{EQUATES}\n        org $F000\nReset:  sta WSYNC\n        nop\nHere:   jmp Here\n        org $FFFC\n        .word Reset\n        .word Reset\n");
    let mut atari = Atari2600::from_rom(&assemble(&source));
    assert!(atari.run_to(0xF003, 1000));
    // The reset's 7 cycles, then STA WSYNC to cycle 10 and the wait to the line's end at 76, then 2 for the NOP.
    assert_eq!(atari.cycles(), 76 + 2);
}

/// The TIA's sound through the player of the sound effects designer, against the model of it the designer plays.
#[test]
fn the_sound_is_what_the_sound_effects_designer_s_model_makes() {
    use crate::sfx::{Bank, Chip, Effect, Frame as SfxFrame, Machine as SfxMachine};
    let kinds = [(4u8, 9u8), (12, 3), (6, 1), (1, 3)];
    for (audc, audf) in kinds {
        let effect = Effect { name: format!("c{audc}"), frames: vec![SfxFrame { wave: audc, period: u16::from(audf), volume: 12, ..SfxFrame::quiet(Chip::Tia) }; 60], adsr: [0, 0xF0] };
        let bank = Bank { machine: SfxMachine::Atari2600Ntsc, effects: vec![effect.clone()] };
        let rom = assemble(&bank.source("sfx", true).unwrap());
        let mut atari = Atari2600::from_rom(&rom);
        for _ in 0..90 {
            atari.frame().unwrap();
        }
        let heard = &atari.heard;
        let loud: Vec<usize> = heard.iter().enumerate().filter(|(_, v)| **v > 0.0).map(|(i, _)| i).collect();
        assert!(!loud.is_empty(), "AUDC {audc}: nothing heard");
        // The first effect's span: from the first sound to the first long quiet after it.
        let start = loud[0];
        let end = loud.windows(2).find(|w| w[1] - w[0] > 2000).map_or(*loud.last().unwrap(), |w| w[0]);
        // As long as 60 frames.
        let lines = (end - start) / 2;
        assert!((lines as i64 - 60 * 262).abs() < 262, "AUDC {audc}: {lines} lines");
        // The same wave, tick for tick, as the model's.
        let mut model = crate::sfx::sound::TiaChannel::default();
        model.set(audc, audf, 12);
        let ours: Vec<u8> = heard[start + 100..start + 4000].iter().map(|v| (v * 30.0).round() as u8).collect();
        let theirs: Vec<u8> = (0..20_000).map(|_| {
            model.tick();
            model.level
        }).collect();
        // Which way up a pure tone starts depends on where it was left, so either is the same wave.
        let flipped: Vec<u8> = ours.iter().map(|&v| if v == 0 { 12 } else { 0 }).collect();
        let found = (0..15_000).any(|shift| theirs[shift..shift + ours.len()] == ours[..] || (audc & 3 != 1 && theirs[shift..shift + ours.len()] == flipped[..]));
        let runs = |v: &[u8]| v.chunk_by(|a, b| a == b).take(24).map(|c| (c[0], c.len())).collect::<Vec<_>>();
        assert!(found, "AUDC {audc}: the wave isn't the model's: {:?}\nthe model's: {:?}", runs(&ours), runs(&theirs));
    }
}

// ---------- MAME as the reference ----------

/// The cartridge run in MAME (when it's installed) for some seconds, and the last frame it showed.
fn mame_shows(name: &str, rom: &[u8]) -> Option<crate::quantise::Image> {
    let program = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("mame")).find(|p| p.is_file()))?;
    let folder = std::env::temp_dir().join(format!("limoniter-2600run-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("cart.bin"), rom).unwrap();
    let output = std::process::Command::new(program)
        .args(["a2600", "-cart", "cart.bin", "-video", "none", "-sound", "none", "-seconds_to_run", "2", "-nothrottle", "-skip_gameinfo", "-snapshot_directory", "snap", "-nvram_directory", "nv", "-cfg_directory", "cfg"])
        .current_dir(&folder)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let shot = std::fs::read(folder.join("snap/a2600/0000.png")).unwrap();
    if let Some(out) = std::env::var_os("LIMONITER_OUT") {
        std::fs::write(Path::new(&out).join(format!("{name}.png")), &shot).unwrap();
    }
    let _ = std::fs::remove_dir_all(&folder);
    Some(crate::quantise::read_picture("shot.png", &shot).unwrap())
}

/// How MAME's picture lines up with a frame of ours: where our line 37's pixel 0 is in it, if a place makes every
/// pixel of our lines 37 to 228 agree (a colour value always the same colour in MAME's palette, and two values
/// never the same colour). MAME shows each frame mixed half and half with the one before (against flicker), so
/// it's the pair of values, this frame's and the last's, that must agree. With the number of pixels that
/// disagree at the best place, if none agrees.
fn agrees_with_mame(shot: &crate::quantise::Image, before: &Frame, frame: &Frame) -> Result<(usize, usize), usize> {
    let at = |x: usize, y: usize| -> [u8; 3] {
        let i = (y * shot.width + x) * 4;
        [shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]]
    };
    let (first, last) = (37, 229);
    let height = last - first;
    let mut best = usize::MAX;
    for top in 0..=shot.height.saturating_sub(height) {
        for left in 0..=shot.width.saturating_sub(WIDTH) {
            let mut colour_of = std::collections::HashMap::new();
            let mut value_of = std::collections::HashMap::new();
            let mut wrong = 0;
            'rows: for y in 0..height {
                for x in 0..WIDTH {
                    let (a, b) = (frame.pixels[(first + y) * WIDTH + x], before.pixels.get((first + y) * WIDTH + x).copied().unwrap_or(0));
                    let value = (a.min(b), a.max(b));
                    let seen = at(left + x, top + y);
                    if *colour_of.entry(value).or_insert(seen) != seen || *value_of.entry(seen).or_insert(value) != value {
                        wrong += 1;
                        if wrong > best.min(50_000) {
                            break 'rows;
                        }
                    }
                }
            }
            if wrong == 0 {
                return Ok((left, top));
            }
            best = best.min(wrong);
        }
    }
    Err(best)
}

/// The test cartridges, each drawn here and in MAME: the two agree pixel for pixel.
#[test]
fn mame_draws_the_test_cartridges_as_they_are_drawn_here() {
    let mut carts: Vec<(String, Vec<u8>)> = Vec::new();
    let pf = "        lda #$90\n        sta PF0\n        lda #$A5\n        sta PF1\n        lda #$3C\n        sta PF2\n        lda #$1E\n        sta COLUPF\n        lda #$84\n        sta COLUBK\n        lda #1\n        sta CTRLPF";
    carts.push(("playfield".into(), program(pf, 37, PLAIN)));
    for n in [5u8, 9] {
        carts.push((format!("sprite-{n}"), positioned(n, 0xC3, 0, "")));
    }
    for nusiz in [1u8, 3, 5, 7] {
        carts.push((format!("nusiz-{nusiz}"), positioned(8, 0x81, nusiz, "")));
    }
    let objects = "        lda #2\n        sta ENAM0\n        sta ENABL\n        lda #$46\n        sta COLUP0\n        lda #$1E\n        sta COLUPF\n        lda #$84\n        sta COLUBK\n        lda #$20\n        sta NUSIZ0\n        lda #$10\n        sta CTRLPF\n        sta WSYNC\n        ldx #6\nP:      dex\n        bne P\n        sta RESM0\n        nop\n        nop\n        sta RESBL";
    carts.push(("missile-ball".into(), program(objects, 36, PLAIN)));
    let hmove = "        lda #$C3\n        sta GRP0\n        lda #$46\n        sta COLUP0\n        lda #$84\n        sta COLUBK\n        lda #$70\n        sta HMP0\n        sta WSYNC\n        ldx #8\nP:      dex\n        bne P\n        sta RESP0";
    let kernel = "        ldy #192\nLine:   sta WSYNC\n        sta HMOVE\n        dey\n        bne Line";
    carts.push(("hmove".into(), program(hmove, 36, kernel)));
    carts.push(("starter".into(), assemble(crate::atari2600::STARTER)));
    for (name, rom) in carts {
        let Some(shot) = mame_shows(&name, &rom) else { return };
        let mut atari = Atari2600::from_rom(&rom);
        // MAME runs two seconds, about 120 frames; the starter's colours go round every 128 frames, so it's
        // compared with each of the first 260 (one of them is MAME's).
        let mut best = usize::MAX;
        for n in 0..260 {
            let before = atari.last_frame().clone();
            atari.frame().unwrap();
            if n >= 3 {
                match agrees_with_mame(&shot, &before, atari.last_frame()) {
                    Ok(_) => {
                        best = 0;
                        break;
                    }
                    Err(wrong) => best = best.min(wrong),
                }
            }
        }
        assert_eq!(best, 0, "{name}: {best} pixels disagree at best ({}\u{D7}{})", shot.width, shot.height);
    }
}
