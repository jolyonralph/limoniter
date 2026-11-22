use super::*;
use std::path::Path;

fn assemble(source: &str) -> crate::asm::Assembly {
    let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, Path::new("t.s"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(assembly.ok(), "{:?}\n{source}", assembly.problems.first().map(|p| (&p.message, source.lines().nth(p.line))));
    assembly
}

/// A small 4K program as 2600 programmers write it: a frame of VSYNC, VBLANK with the timer, a kernel of
/// scanlines, and overscan; a sprite's graphics at the end, and the vectors.
const KERNEL: &str = "\tprocessor 6502
VSYNC = $00
VBLANK = $01
WSYNC = $02
COLUBK = $09
GRP0 = $1B
INTIM = $284
TIM64T = $296
\tseg.u vars
\torg $80
frame\tds 1
\tseg code
\torg $F000
Reset:\tsei
\tcld
\tldx #$FF
\ttxs
\tlda #0
Clear:\tsta $00,x
\tdex
\tbne Clear
Frame:\tlda #2
\tsta VSYNC
\tsta WSYNC
\tsta WSYNC
\tsta WSYNC
\tlda #0
\tsta VSYNC
\tlda #43
\tsta TIM64T
\tinc frame
Wait:\tlda INTIM
\tbne Wait
\tsta WSYNC
\tsta VBLANK
\tldy #192
Line:\tsta WSYNC
\tsty COLUBK
\tlda Ship,y
\tsta.w GRP0
\tdey
\tbne Line
\tlda #2
\tsta VBLANK
\tjmp Frame
Ship:\t.byte %00011000,%00111100,%01111110,%11011011,%11111111,%00100100,%01011010,%10100101
\torg $FFFC
\t.word Reset
\t.word Reset
";

#[test]
fn a_4k_cartridge_comes_back_the_same_with_its_registers_named() {
    let made = assemble(KERNEL);
    let rom = made.image(0xF000, 0x10000);
    let cart = recognise("game.a26", &rom).unwrap();
    assert_eq!(cart.scheme, Scheme::K4);
    let source = disassemble(&cart);
    for said in ["sta     VSYNC", "sta     WSYNC", "sta     TIM64T", "lda     INTIM", "sta.w   GRP0", "inc     ram_80", "WSYNC   = $02", "TIM64T  = $0296", "; ...##..."] {
        assert!(source.contains(said), "{said}:\n{source}");
    }
    let again = assemble(&source);
    assert_eq!(again.image(0xF000, 0x10000), rom, "{source}");
}

#[test]
fn code_running_past_a_scanline_is_pointed_out() {
    // 20 NOPs of 2 cycles each and a loop of 40 more after a WSYNC: 80 cycles before the next.
    let mut source = String::from("\tprocessor 6502\nWSYNC = $02\n\torg $F000\nStart:\tsta WSYNC\n");
    for _ in 0..40 {
        source += "\tnop\n";
    }
    source += "\tsta WSYNC\n\tjmp Start\n\torg $FFFC\n\t.word Start\n\t.word Start\n";
    let rom = assemble(&source).image(0xF000, 0x10000);
    let out = disassemble(&recognise("x.a26", &rom).unwrap());
    assert!(out.contains("past the scanline: 78 cycles since WSYNC"), "{out}");
}

/// An 8K F8 cartridge: each bank starts by switching to bank 1, whose code calls a routine in bank 0 through
/// the same trampoline in both.
#[test]
fn an_f8_cartridge_is_followed_from_bank_to_bank() {
    let bank = |n: usize| -> String {
        format!(
            "\torg ${:04X}\n\trorg $F000\nStart{n}:\tsta $1FF9\n\tjmp Main{n}\nToBank0_{n}:\tsta $1FF8\n\tnop\nBack{n}:\trts\nMain{n}:\t{}\n\torg ${:04X}\n\trorg $FFFC\n\t.word Start{n}\n\t.word Start{n}\n",
            n * 4096,
            if n == 1 { "jsr ToBank0_1\n\tlda #1\n\tjmp Main1" } else { "lda #0\n\tsta $1FF9\n\trts" },
            n * 4096 + 0xFFC
        )
    };
    let source = format!("\tprocessor 6502\n{}{}", bank(0), bank(1));
    let made = assemble(&source);
    let rom = made.image(0, 8192);
    let cart = recognise("f8.a26", &rom).unwrap();
    assert_eq!(cart.scheme, Scheme::F8);
    let out = disassemble(&cart);
    // Bank 0's code after the switch at $F006 is found from bank 1, and bank 1's from bank 0.
    assert!(out.contains("L1_F003:") && out.contains("rorg $F000"), "{out}");
    let again = assemble(&out);
    assert_eq!(again.image(0, 8192), rom, "{out}");
}

#[test]
fn bank_switching_is_told_apart() {
    let mut rom = vec![0u8; 8192];
    assert_eq!(detect(&rom), Some(Scheme::F8Sc), "the same RAM area at the start of each bank");
    rom[200] = 1;
    rom[0] = 9;
    assert_eq!(detect(&rom), Some(Scheme::F8));
    rom[300..303].copy_from_slice(&[0xAD, 0xE0, 0x1F]);
    assert_eq!(detect(&rom), Some(Scheme::E0));
    let mut big = vec![1u8; 16384];
    big[100..103].copy_from_slice(&[0xAD, 0xE7, 0x1F]);
    assert_eq!(detect(&big), Some(Scheme::E7));
    assert_eq!(detect(&vec![0u8; 2048]), Some(Scheme::K2));
    assert_eq!(detect(&vec![0u8; 12288]), Some(Scheme::Fa));
    assert_eq!(detect(&vec![0u8; 10495]), Some(Scheme::Dpc));
    assert_eq!(detect(&vec![0u8; 5000]), None);
    // A .bin of the wrong kind isn't taken for one: its reset vector isn't in the cartridge.
    assert!(recognise("data.bin", &vec![0u8; 4096]).is_none());
    assert_eq!(register(0x02, true), Some("WSYNC"));
    assert_eq!(register(0x0C, false), Some("INPT4"));
    assert_eq!(register(0x284, false), Some("INTIM"));
    assert_eq!(register(0x296, true), Some("TIM64T"));
    assert_eq!(register(0x296, false), None);
}

/// 2600 source is known for what it is, made into a cartridge, and MAME (where it's installed) runs it with the
/// arguments Limoniter gives it.
#[test]
fn source_becomes_a_cartridge_that_mame_runs() {
    let assembly = assemble(KERNEL);
    assert_eq!(crate::asm::output::machine_6502(&assembly), Some(crate::asm::output::Micro6502::Atari2600));
    assert_eq!(crate::asm::output::Micro6502::said_in(["; machine: Atari 2600"]), Some(crate::asm::output::Micro6502::Atari2600));
    let rom = assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap());
    assert_eq!(rom.len(), 4096);
    assert_eq!(crate::running::machine_for(crate::running::Format::A26, &rom), Some(crate::running::Machine::Atari2600));
    let mame = crate::emulators::by_id("mame").unwrap();
    assert!(mame.runs(crate::running::Machine::Atari2600, crate::running::Format::A26));
    let Some(program) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("mame")).find(|p| p.is_file())) else { return };
    let folder = std::env::temp_dir().join(format!("limoniter-2600-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("kernel.a26");
    std::fs::write(&file, &rom).unwrap();
    let args = mame.arguments(crate::running::Machine::Atari2600, crate::running::Format::A26, &file, None, "-video none -sound none -seconds_to_run 1 -nothrottle");
    let output = crate::platform::headless(program).args(&args).current_dir(&folder).output().unwrap();
    let _ = std::fs::remove_dir_all(&folder);
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_starter_is_a_cartridge() {
    let assembly = assemble(STARTER);
    assert_eq!(crate::asm::output::Micro6502::said_in(STARTER.lines()), Some(crate::asm::output::Micro6502::Atari2600));
    let rom = assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap());
    assert_eq!(detect(&rom), Some(Scheme::K4));
    let out = disassemble(&recognise("starter.a26", &rom).unwrap());
    assert!(!out.contains("past the scanline"), "{out}");
}
