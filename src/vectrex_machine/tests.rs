use super::*;
use crate::vectrex_screen::{Segment, to_pixel};

/// A cartridge that needs no BIOS: it sets the VIA up itself, puts a tone on the AY, and each frame reads the
/// buttons and the first joystick, zeroes the beam and draws a square, waiting for timer 2 between frames.
pub const SQUARE: &str = include_str!("square.asm");

fn cartridge(source: &str) -> Vec<u8> {
    let assembly = crate::asm::assemble(&crate::asm::m6809::M6809, std::path::Path::new("v.asm"), source, &|_| Err("no".into()));
    assert!(assembly.problems.is_empty(), "{:?}", assembly.problems);
    assembly.image(0, assembly.highest().unwrap())
}

/// The lit lines that go somewhere (not the dots where the beam waited).
fn lines(segments: &[Segment]) -> Vec<Segment> {
    segments.iter().filter(|s| s.from != s.to).copied().collect()
}

fn near(a: (i32, i32), b: (i32, i32)) -> bool {
    (a.0 - b.0).abs() < 400 && (a.1 - b.1).abs() < 400
}

#[test]
fn a_cartridge_draws_a_square_without_the_bios() {
    let mut vectrex = Vectrex::with_cartridge(cartridge(SQUARE), None, None);
    for _ in 0..5 {
        vectrex.frame().unwrap();
    }
    let drawn = lines(&vectrex.last_segments);
    // Four sides of 64 at a scale of 127, from (-32, -32) times 127: up, across, down and back.
    let corner = 32 * 127;
    let corners = [(-corner, -corner), (-corner, corner), (corner, corner), (corner, -corner)];
    assert_eq!(drawn.len(), 4, "{drawn:?}");
    for (side, line) in drawn.iter().enumerate() {
        let (from, to) = (corners[side], corners[(side + 1) % 4]);
        assert!(near(line.from, from) && near(line.to, to), "side {side}: {line:?}, wanted {from:?} to {to:?}");
        assert_eq!(line.z, 127);
    }
    // On the screen: the sides lit, the middle dark.
    let picture = vectrex.picture().to_vec();
    let brightness = |x: i32, y: i32| {
        let (px, py) = to_pixel(x, y);
        let at = (py as usize * crate::vectrex_screen::WIDTH + px as usize) * 4;
        picture[at..at + 3].iter().map(|c| u32::from(*c)).sum::<u32>()
    };
    for (x, y) in [(-corner, 0), (0, corner), (corner, 0), (0, -corner)] {
        assert!(brightness(x, y) > 300, "a side at ({x}, {y}): {}", brightness(x, y));
    }
    assert_eq!(brightness(0, 0), 0, "the middle is dark");
    assert!(vectrex.frames >= 5);
}

#[test]
fn the_ay_plays_what_the_cartridge_writes() {
    let mut vectrex = Vectrex::with_cartridge(cartridge(SQUARE), None, None);
    for _ in 0..10 {
        vectrex.frame().unwrap();
    }
    let regs = vectrex.ay_registers();
    assert_eq!((regs[0], regs[1], regs[7], regs[8]), (0x00, 0x01, 0x3E, 0x0C));
    // A tone of 1.5 MHz over 16 over 256, about 366 Hz: the sound goes up through 0 that often.
    let heard = &vectrex.heard[TEST_RATE as usize / 10..];
    let crossings = heard.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    let hz = crossings as f64 / (heard.len() as f64 / f64::from(TEST_RATE));
    assert!((hz - 366.0).abs() < 30.0, "{hz} Hz");
}

#[test]
fn the_buttons_and_the_joystick_reach_the_cartridge() {
    let mut vectrex = Vectrex::with_cartridge(cartridge(SQUARE), None, None);
    vectrex.frame().unwrap();
    vectrex.frame().unwrap();
    assert_eq!((vectrex.sys.peek(0xC880), vectrex.sys.peek(0xC881)), (0xFF, 0), "nothing pressed");
    vectrex.controls().buttons[0][0] = true;
    vectrex.controls().buttons[1][3] = true;
    vectrex.controls().sticks[0][0] = 127;
    vectrex.frame().unwrap();
    vectrex.frame().unwrap();
    assert_eq!(vectrex.sys.peek(0xC880), 0b0111_1110, "the first controller's button 1, the second's button 4");
    assert_eq!(vectrex.sys.peek(0xC881), 0x20, "pushed right: the joystick is further than the DAC's 0");
    vectrex.release_all();
    vectrex.frame().unwrap();
    vectrex.frame().unwrap();
    assert_eq!(vectrex.sys.peek(0xC880), 0xFF);
}

#[test]
fn without_the_bios_a_call_to_it_says_so() {
    let mut vectrex = Vectrex::with_cartridge(cartridge(crate::vectrex::STARTER), None, None);
    let problem = vectrex.frame().unwrap_err();
    assert!(problem.contains("$F192 (Wait_Recal)") && problem.contains("BIOS"), "{problem}");
    assert!(vectrex.paused);
}

#[test]
fn an_opcode_the_6809_lacks_stops_it() {
    let mut cart = cartridge("        org 0\n        fcc \"g GCE 2026\"\n        fcb $80\n        fdb 0\n        fcb $F8,$50,$20,-$40\n        fcc \"X\"\n        fcb $80\n        fcb 0\n        nop\n        fcb $87\n");
    cart.resize(32, 0);
    let mut vectrex = Vectrex::with_cartridge(cart, None, None);
    let problem = vectrex.frame().unwrap_err();
    assert!(problem.contains("$87"), "{problem}");
}

#[test]
fn breakpoints_and_steps_stop_where_they_should() {
    let assembly = crate::asm::assemble(&crate::asm::m6809::M6809, std::path::Path::new("v.asm"), SQUARE, &|_| Err("no".into()));
    let symbol = |name: &str| assembly.symbols[name] as u16;
    let mut vectrex = Vectrex::with_cartridge(cartridge(SQUARE), None, None);
    vectrex.set_breakpoints([symbol("side")]);
    vectrex.frame().unwrap();
    assert_eq!((vectrex.paused, vectrex.stopped, vectrex.cpu.pc), (true, Some(symbol("side")), symbol("side")));
    // Step: LDD ,X++.
    vectrex.step().unwrap();
    assert_eq!(vectrex.cpu.d(), 0x4000);
    // Step over the BSR to move: it comes back to the DEC after it.
    vectrex.step_over().unwrap();
    assert_eq!(vectrex.cpu.pc, symbol("side") + 4);
    assert!(vectrex.stop_words().words.iter().any(|w| w.starts_with("PC ")));
    // A condition: stopped only on the last side.
    vectrex.set_conditions(BTreeMap::from([(symbol("side"), "[count] = 1".to_string())]), Some(std::sync::Arc::new(assembly.symbols.clone())));
    vectrex.carry_on();
    vectrex.frame().unwrap();
    assert_eq!((vectrex.cpu.pc, vectrex.sys.peek(0xC882)), (symbol("side"), 1));
}

/// With the real BIOS, if it's here: the starter cartridge runs, and after the BIOS's title it draws the square and
/// its text.
#[test]
fn with_the_bios_the_starter_runs() {
    let Some(Ok(bios)) = crate::micro_roms::rom(crate::micro_roms::Needs::Vectrex) else {
        eprintln!("No Vectrex BIOS (LIMONITER_VECTREX_ROM): skipped.");
        return;
    };
    let mut vectrex = Vectrex::with_cartridge(cartridge(crate::vectrex::STARTER), Some(bios), None);
    for _ in 0..50 * 6 {
        vectrex.frame().unwrap();
    }
    assert!(lines(&vectrex.last_segments).len() > 20, "the square and HELLO VECTREX: {}", vectrex.last_segments.len());
}
