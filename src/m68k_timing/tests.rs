use super::*;
use std::path::Path;

/// One instruction's bytes, assembled for the 68000 at $1000.
fn bytes(source: &str) -> Vec<u8> {
    let assembly = crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("t.s"), &format!("\torg $1000\nhere:\n\t{source}\n"), &|_| Err("no".into()));
    assert!(assembly.ok(), "{source}: {:?}", assembly.problems);
    assembly.lines.iter().flat_map(|l| l.bytes.clone()).collect()
}

fn on(cpu: Processor, source: &str) -> String {
    cycles(&bytes(source), cpu).unwrap_or_else(|| panic!("{source} has no time")).text()
}

/// Motorola's figures for the 68000 (the MC68000 User's Manual, section 8), each with its address's time added.
#[test]
fn the_68000_takes_what_motorola_gives() {
    for (source, want) in [
        ("nop", "4"),
        ("move.w d0,d1", "4"),
        ("move.b (a0),d1", "8"),
        ("move.l (a0),d1", "12"),
        ("move.w (a0)+,-(a1)", "12"),
        ("move.b d0,$12345678", "16"),
        ("move.l $12345678,$12345678", "36"),
        ("move.l 8(a0),-4(a1,d0.w)", "30"),
        ("movea.l #$12345678,a0", "12"),
        ("moveq #1,d0", "4"),
        ("add.l d0,d1", "8"),
        ("add.l (a0),d1", "14"),
        ("add.w d0,(a0)", "12"),
        ("add.l d0,(a0)", "20"),
        ("adda.w d0,a0", "8"),
        ("adda.l (a0),a1", "14"),
        ("addi.w #1,d0", "8"),
        ("addi.l #1,d0", "16"),
        ("addi.l #1,(a0)", "28"),
        ("cmpi.l #1,d0", "14"),
        ("addq.w #1,d0", "4"),
        ("addq.l #1,d0", "8"),
        ("addq.w #1,a0", "8"),
        ("addq.l #1,(a0)", "20"),
        ("clr.l d0", "6"),
        ("clr.w (a0)", "12"),
        ("tst.l d0", "4"),
        ("tst.w 8(a0)", "12"),
        ("lea 8(a0),a1", "8"),
        ("lea (a0,d0.w),a1", "12"),
        ("pea (a0)", "12"),
        ("jmp $12345678", "12"),
        ("jsr (a0)", "16"),
        ("jsr here(pc)", "18"),
        ("movem.l d0-d7/a0-a6,-(sp)", "128"),
        ("movem.l (sp)+,d0-d7/a0-a6", "132"),
        ("movem.w (a0),d0/d1", "20"),
        ("bra.s here", "10"),
        ("bsr.w here", "18"),
        ("beq.s here", "8/10"),
        ("beq.w here", "10/12"),
        ("dbra d0,here", "10/14"),
        ("rts", "16"),
        ("rte", "20"),
        ("lsl.w #1,d0", "8"),
        ("lsl.l #8,d0", "24"),
        ("asl.w d1,d0", "6/132"),
        ("ror.w (a0)", "12"),
        ("btst #3,d0", "10"),
        ("btst d1,(a0)", "8"),
        ("bset d1,(a0)", "12"),
        ("bclr #1,d0", "14"),
        ("mulu.w #0,d0", "42"),
        ("mulu.w #$ffff,d0", "74"),
        ("muls.w #$5555,d0", "74"),
        ("mulu.w d1,d0", "38/70"),
        ("divu.w d1,d0", "\u{2264}140"),
        ("divs.w (a0),d0", "\u{2264}162"),
        ("movep.w 0(a0),d0", "16"),
        ("movep.l d0,0(a0)", "24"),
        ("exg d0,d1", "6"),
        ("swap d0", "4"),
        ("link a6,#-8", "16"),
        ("unlk a6", "12"),
        ("abcd -(a0),-(a1)", "18"),
        ("addx.l -(a0),-(a1)", "30"),
        ("cmpm.l (a0)+,(a1)+", "20"),
        ("seq d0", "4/6"),
        ("st (a0)", "12"),
        ("tas (a0)", "14"),
        ("move.w sr,d0", "6"),
        ("move.w d0,sr", "12"),
        ("ori.w #$700,sr", "20"),
        ("trap #0", "34"),
    ] {
        assert_eq!(on(Processor::M68000, source), want, "{source}");
    }
}

/// The 68008's, worked out from the 68000's: a word read or written takes it eight cycles, not four; a byte, four.
#[test]
fn the_68008_takes_four_cycles_more_for_each_word() {
    for (source, want) in [
        ("nop", "8"),
        ("move.w d0,d1", "8"),
        ("move.l d0,d1", "8"),
        ("move.b (a0),d1", "12"),
        ("move.w (a0),d1", "16"),
        ("move.l (a0),d1", "24"),
        ("move.b d0,(a0)", "12"),
        ("move.l d0,(a0)", "24"),
        ("rts", "32"),
        ("bra.s here", "18"),
        ("beq.s here", "12/18"),
        ("dbra d0,here", "18/26"),
        ("movem.l (sp)+,d0-d7/a0-a6", "264"),
        ("addq.b #1,(a0)", "16"),
        ("movep.w 0(a0),d0", "24"),
    ] {
        assert_eq!(on(Processor::M68008, source), want, "{source}");
    }
}

/// Every 68000 instruction in every mode has a time on the 68000 and the 68008; the 68010, 68020 and later have
/// none, rather than wrong ones.
#[test]
fn every_68000_form_has_a_time_and_later_processors_none() {
    let all = include_bytes!("../asm/testdata/m68k_all.bin");
    let word = |a: u32| all.get(a as usize..a as usize + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let mut at = 0u32;
    let mut counted = 0;
    while (at as usize) < all.len() {
        let insn = crate::m68k_disasm::decode(&word, at).unwrap();
        let these = &all[at as usize..(at + insn.length) as usize];
        let plain = cycles(these, Processor::M68000).unwrap_or_else(|| panic!("{} has no time", insn.mnemonic));
        let slow = cycles(these, Processor::M68008).unwrap();
        // STOP's four cycles are all Motorola gives, its word read before it stops.
        assert!(slow.least >= plain.least + 4 || insn.mnemonic == "stop", "{}: the 68008 fetches at least its opcode word in two", insn.mnemonic);
        for later in [Processor::M68010, Processor::M68020, Processor::M68030, Processor::M68040, Processor::M68060, Processor::Cpu32] {
            assert_eq!(cycles(these, later), None);
        }
        at += insn.length;
        counted += 1;
    }
    assert!(counted > 700);
}

/// Which processor each line is for follows MACHINE, and the settings' choice before it.
#[test]
fn each_line_is_timed_for_the_processor_it_is_for() {
    let source = "\torg $1000\n\tnop\n\tmachine 68008\n\tnop\n\tmachine 68020\n\tnop\n\tmc68000\n\tnop\n";
    let assembly = crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("t.s"), source, &|_| Err("no".into()));
    let timed = |n: usize| line(&assembly, 0, n).map(|(_, cpu, t)| words(t, cpu));
    assert_eq!(timed(1).as_deref(), Some("4 cycles"));
    assert_eq!(timed(3).as_deref(), Some("8 cycles on the 68008"));
    assert_eq!(timed(5), None, "the 68020's aren't given");
    assert_eq!(timed(7).as_deref(), Some("4 cycles"));
    let options = crate::asm::Options { settings: vec![(crate::asm::m68k::SETTING, 68008)], ..Default::default() };
    let chosen = crate::asm::assemble_with(&crate::asm::m68k::M68k, Path::new("t.s"), "\tnop\n\tmachine 68000\n\tnop\n", &|_| Err("no".into()), 0, &options);
    assert_eq!(line(&chosen, 0, 0).map(|(_, cpu, t)| words(t, cpu)).as_deref(), Some("8 cycles on the 68008"));
    assert_eq!(line(&chosen, 0, 2).map(|(_, cpu, t)| words(t, cpu)).as_deref(), Some("4 cycles"), "MACHINE wins over the settings");
}
