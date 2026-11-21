use crate::asm::{Assembly, Options, assemble, assemble_with, m68k};
use std::path::Path;

fn no_files(path: &Path) -> Result<Vec<u8>, String> {
    Err(crate::i18n::tf!("{} isn't there", path.display()))
}

fn on(source: &str) -> Assembly {
    assemble(&m68k::M68k, Path::new("t.s"), source, &no_files)
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

/// Every 68881 and 68882 instruction in its forms (each calculation in each size from each kind of address, FMOVE
/// out with its k-factors, the control registers, FMOVEM's lists, FMOVECR, every condition), as vasm 2.0f
/// assembles them with -no-opt.
#[test]
fn every_fpu_instruction_is_assembled_as_vasm_does() {
    let source = include_str!("../../testdata/m68k_fpu_all.asm");
    let assembly = on(source);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).map(|p| (p.line + 1, &p.message, source.lines().nth(p.line))).collect::<Vec<_>>());
    let expected = include_bytes!("../../testdata/m68k_fpu_all.bin");
    for placed in &assembly.lines {
        let at = (placed.address - 0x1000) as usize;
        assert_eq!(Some(&placed.bytes[..]), expected.get(at..at + placed.bytes.len()), "line {}: {}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""));
    }
    assert_eq!(assembly.image(0x1000, 0x1000 + expected.len() as i64), expected);
}

/// The floating-point data directives, as vasm 2.0f makes them; DS's room in each size.
#[test]
fn float_data_is_made_as_vasm_makes_it() {
    let source = "\torg $1000\n\tdc.s 1.5,-2.5e-3,$3fc00000,3,0.1\n\tdc.d 1.5,0.1,$3ff8000000000000,-1\n\tdc.x 1.5,-2.5,$3fff0000c000000000000000,10\n\tdc.p 1.5,3,-6.25e-2,123.456e100\n\tds.s 1\n\tds.d 1\n\tds.x 1\n\tds.p 1\n\tdc.b 1\n\tdc.s 1.5\n";
    let assembly = on(source);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let want = hex("3fc00000bb23d70a3fc00000404000003dcccccd3ff80000000000003fb999999999999a3ff8000000000000bff00000000000003fff0000c000000000000000c0000000a0000000000000003fff0000c00000000000000040020000a000000000000000000000015000000000000000000000030000000000000000c00200062500000000000000010200012345600000000000000000000000000000000000000000000000000000000000000000000000000000000000013fc00000");
    assert_eq!(assembly.image(0x1000, 0x1000 + want.len() as i64), want);
    // vasm refuses a floating-point fill for DCB.D, DCB.X and DCB.P, and makes DCB.S's a whole number (1.5 is 1),
    // though its manual says each takes one; Limoniter makes them.
    let assembly = on("\torg 0\n\tdcb.d 2,-0.5\n\tdcb.x 1,0.25\n\tdcb.p 1,1e10\n\tdcb.s 2,1.5\n");
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.image(0, 48), hex("bfe0000000000000bfe00000000000003ffd000080000000000000000010000100000000000000003fc000003fc00000"));
    // A label before them, and DS's sizes reckoned by labels after.
    let assembly = on("\torg 0\nvalue\tdc.x 1.5\nroom\tds.d 2\nafter\tdc.s 2\n");
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.image(28, 32), hex("40000000"));
}

/// A coprocessor other than number 1 (FPU 2, as vasm takes it) is written into each instruction's first word.
#[test]
fn the_coprocessor_number_is_the_one_fpu_chose() {
    let source = "\tmachine 68020\n\tfpu 2\n\torg $1000\nx:\n\tfnop\n\tfadd.x fp0,fp1\n\tfmove.s #1.5,fp2\n\tfbeq x\n\tfsave (a0)\n\tfrestore (a0)+\n\tfseq d0\n\tfdbne d1,x\n\tftrapeq\n\tfmovem.x fp0-fp3,-(a7)\n\tfpu 7\n\tfnop\n";
    let assembly = on(source);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let want = hex("f4800000f40000a2f43c45003fc00000f481ffeef510f558f4400001f449000effe0f47c0001f427e00ffe800000");
    assert_eq!(assembly.image(0x1000, 0x1000 + want.len() as i64), want);
    // The 68040's own is always number 1.
    assert!(!on("\tmachine 68040\n\tfpu 2\n").ok());
}

/// The first problem with a line, after a line choosing the processor and FPU.
fn problem(choose: &str, line: &str) -> Option<String> {
    let assembly = on(&format!("{choose}\n\torg $1000\nx:\t{line}\n"));
    assembly.problems.first().map(|p| p.message.clone())
}

/// Without an FPU chosen, its instructions are refused, saying how to choose one; the 68040 and 68060 have their
/// own, and every way vasm and Devpac choose one is taken.
#[test]
fn fpu_instructions_need_an_fpu() {
    for choose in ["\tmachine 68020", "", "\tmachine 68030", "\tmachine 68040\n\tfpu 0", "\tmachine 68040\n\tmachine 68020", "\tmachine 68020\n\tfpu 1\n\tfpu 0"] {
        let message = problem(choose, "fadd.x fp0,fp1").unwrap_or_else(|| panic!("FADD should need an FPU after {choose:?}"));
        assert!(message.contains("MACHINE 68881") && message.contains("FADD"), "{message}");
    }
    for choose in ["\tmachine 68020\n\tmachine 68881", "\tmachine 68882\n\tmachine 68030", "\tmachine 68020\n\tfpu 1", "\tmc68020\n\tmc68881", "\topt p=68020/68881", "\topt p=68030/68882", "\tmachine 68040", "\tmachine 68060", "\tmachine 68000\n\tmachine 68881"] {
        assert_eq!(problem(choose, "fadd.x fp0,fp1"), None, "{choose:?}");
    }
    // The settings' FPU comes in through the options, as vasm's -m68881.
    let options = Options { settings: vec![(m68k::SETTING, 68030), (m68k::FPU_SETTING, 68882)], ..Default::default() };
    let assembly = assemble_with(&m68k::M68k, Path::new("t.s"), "\tfmove.x fp0,fp1\n", &no_files, 0, &options);
    assert!(assembly.ok(), "{:?}", assembly.problems);
}

/// What the FPU can't do is refused, as vasm refuses it, and so is what vasm takes but makes no sense of.
#[test]
fn what_the_fpu_has_not_got_is_refused() {
    let fpu = "\tmachine 68020\n\tmachine 68881";
    for line in [
        "fmove.x a0,fp0",
        "fmove.d d0,fp0",
        "fmove.x fp0,x(pc)",
        "fmove.x fp0,#1",
        "fmove.p fp0,d0",
        "fmove.x fp0,(a0){#2}",
        "fmove.p fp0,(a0){#64}",
        "fmovem.l d0,fpcr/fpsr",
        "fmove.l a0,fpcr",
        "fmovem.l fpcr,x(pc)",
        "fmovem.x fp0-fp3,(a0)+",
        "fmovem.x -(a0),fp0-fp3",
        "fmovem.l fp0-fp3,-(a0)",
        "fmovem.x d0,(a0)+",
        "fseq a0",
        "fseq x(pc)",
        "fseq.w d0",
        "fsave (a0)+",
        "frestore -(a0)",
        "fsave x(pc)",
        "fmovecr #$80,fp0",
        "fmovecr.s #0,fp0",
        "fsincos fp0,fp1",
        "ftrapeq.b #1",
        "fbeq.b x",
        "fdbeq a0,x",
        "fabs.x fp0,fp1,fp2",
        "fadd fp0",
        "ftst a0",
        "fmove.l #2.7,fp0",
        "fmove.w #1.5,fp0",
        "fnop x",
        "fadd.q (a0),fp0",
        "fmove.s #1e39,fp0",
    ] {
        assert!(problem(fpu, line).is_some(), "{line} should be refused");
    }
}

/// Floats in immediates: decimal numbers (and expressions starting with one) in floating point, anything else as
/// the bits themselves; .X kept to all 64 bits where vasm keeps only a double's 53.
#[test]
fn immediates_are_made_in_their_format() {
    let image = |line: &str| {
        let assembly = on(&format!("\tmachine 68020\n\tmachine 68881\n\torg 0\nthree\tequ 3\n\t{line}\n"));
        assert!(assembly.ok(), "{line}: {:?}", assembly.problems);
        let end = assembly.lines.iter().map(|l| l.address + l.bytes.len() as i64).max().unwrap_or(0);
        assembly.image(0, end).iter().map(|b| format!("{b:02X}")).collect::<String>()
    };
    assert_eq!(image("fmove.s #three,fp0"), "F23C440000000003");
    assert_eq!(image("fmove.s #2*three,fp0"), "F23C440040C00000");
    assert_eq!(image("fmove.x #0.1,fp0"), "F23C48003FFB0000CCCCCCCCCCCCCCCD");
    assert_eq!(image("fmove.p #0.1,fp0"), "F23C4C00400100010000000000000000");
    assert_eq!(image("fmove.p #-0.0,fp0"), "F23C4C00800000000000000000000000");
    assert_eq!(image("fmove.l #three,fp0"), "F23C400000000003");
    assert_eq!(image("fmovem.l #-2,fpcr/fpsr/fpiar"), "F23C9C00FFFFFFFFFFFFFFFFFFFFFFFE");
}
