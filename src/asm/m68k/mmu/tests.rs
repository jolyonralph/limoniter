use crate::asm::{Assembly, Options, assemble, assemble_with, m68k};
use std::path::Path;

fn no_files(path: &Path) -> Result<Vec<u8>, String> {
    Err(crate::i18n::tf!("{} isn't there", path.display()))
}

fn on(source: &str) -> Assembly {
    assemble(&m68k::M68k, Path::new("t.s"), source, &no_files)
}

/// Every MMU instruction of the 68030, the 68851, the 68040 and the 68060 in its forms (PMOVE of each register to
/// and from each kind of address, PFLUSH, PLOAD and PTEST with each kind of function code, every condition), the
/// 68040's, 68060's and CPU32's own instructions and the rounding FPU instructions, as vasm 2.0f assembles them
/// with -no-opt, choosing the processor with MACHINE as the source goes.
#[test]
fn every_mmu_instruction_is_assembled_as_vasm_does() {
    let source = include_str!("../../testdata/m68k_mmu_all.asm");
    let assembly = on(source);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).map(|p| (p.line + 1, &p.message, source.lines().nth(p.line))).collect::<Vec<_>>());
    let expected = include_bytes!("../../testdata/m68k_mmu_all.bin");
    for placed in &assembly.lines {
        let at = (placed.address - 0x1000) as usize;
        assert_eq!(Some(&placed.bytes[..]), expected.get(at..at + placed.bytes.len()), "line {}: {}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""));
    }
    assert_eq!(assembly.image(0x1000, 0x1000 + expected.len() as i64), expected);
}

/// The first problem with a line, after lines choosing the processor and MMU.
fn problem(choose: &str, line: &str) -> Option<String> {
    let assembly = on(&format!("{choose}\n\torg $1000\nx:\t{line}\n"));
    assembly.problems.first().map(|p| p.message.clone())
}

/// Without an MMU, its instructions are refused, saying how to choose one; the 68030, 68040 and 68060 have their
/// own, and every way vasm, Devpac and PhxAss choose the 68851 is taken.
#[test]
fn mmu_instructions_need_an_mmu() {
    for choose in ["\tmachine 68020", "", "\tmachine 68010", "\tmachine cpu32"] {
        let message = problem(choose, "pmove tc,(a0)").unwrap_or_else(|| panic!("PMOVE should need an MMU after {choose:?}"));
        assert!(message.contains("MACHINE 68851") && message.contains("PMOVE"), "{message}");
    }
    for choose in ["\tmachine 68020\n\tmachine 68851", "\tmachine 68851\n\tmachine 68020", "\tmc68020\n\tmc68851", "\tmachine 68020\n\tpmmu", "\topt p=68020/68851", "\tmachine 68030", "\tmachine mc68030"] {
        assert_eq!(problem(choose, "pmove tc,(a0)"), None, "{choose:?}");
    }
    // The settings' MMU comes in through the options, as vasm's -m68851.
    let options = Options { settings: vec![(m68k::SETTING, 68020), (m68k::MMU_SETTING, 68851)], ..Default::default() };
    let assembly = assemble_with(&m68k::M68k, Path::new("t.s"), "\tpmove (a0),cal\n", &no_files, 0, &options);
    assert!(assembly.ok(), "{:?}", assembly.problems);
}

/// Each MMU's own forms are refused on the others, naming the MMU that has them.
#[test]
fn each_mmu_refuses_the_others_forms_naming_the_one_that_has() {
    let pmmu = "\tmachine 68020\n\tmachine 68851";
    for (choose, line, needs) in [
        ("\tmachine 68030", "pmove (a0),cal", "68851"),
        ("\tmachine 68030\n\tmachine 68851", "pmove (a0),drp", "68851"),
        ("\tmachine 68030", "pvalid val,(a0)", "68851"),
        ("\tmachine 68030", "pflushs #1,#2", "68851"),
        ("\tmachine 68030", "pbbs x", "68851"),
        ("\tmachine 68030", "psave (a0)", "68851"),
        ("\tmachine 68030", "pflush (a0)", "68040"),
        ("\tmachine 68030", "plpar (a0)", "68060"),
        (pmmu, "pmove (a0),tt0", "68030"),
        (pmmu, "pmovefd (a0),tc", "68030"),
        (pmmu, "pflushn (a0)", "68040"),
        ("\tmachine 68040", "pmove tc,(a0)", "68030"),
        ("\tmachine 68040", "ptestr #1,(a0),#7", "68030"),
        ("\tmachine 68040", "plpaw (a0)", "68060"),
        ("\tmachine 68060", "ptestr (a0)", "68040"),
        ("\tmachine 68060", "pflush #1,#2", "68030"),
        ("\tmachine 68020", "pflush (a0)", "68040"),
        ("\tmachine 68030", "move16 (a0)+,(a1)+", "68040"),
        ("\tmachine 68020", "cinva bc", "68040"),
        ("\tmachine 68030", "cpushl dc,(a0)", "68040"),
        ("\tmachine 68040", "lpstop #$2700", "68060"),
        ("\tmachine 68060", "tblu.b (a0),d0", "CPU32"),
        ("\tmachine 68060", "bgnd", "CPU32"),
        ("\tmachine 68030\n\tmachine 68882", "fsadd fp0,fp1", "68040"),
        ("\tmachine 68040", "movec buscr,d0", "68060"),
        ("\tmachine 68060", "movec mmusr,d0", "68040"),
        ("\tmachine 68030", "movec urp,d0", "68040"),
        ("\tmachine 68020", "movec tc,d0", "68040"),
    ] {
        let message = problem(choose, line).unwrap_or_else(|| panic!("{line} should be refused after {choose:?}"));
        assert!(message.contains(needs), "{line} after {choose:?}: {message}");
    }
}

/// What the MMUs can't do is refused, as vasm refuses it, and so is what vasm takes but the chips wouldn't do.
#[test]
fn what_the_mmus_have_not_got_is_refused() {
    let m030 = "\tmachine 68030";
    let pmmu = "\tmachine 68020\n\tmachine 68851";
    for (choose, line) in [
        (m030, "pmove d0,tc"),
        (m030, "pmove (a0)+,tc"),
        (m030, "pmove tc,-(a0)"),
        (m030, "pmove #1,tc"),
        (m030, "pmove x(pc),tc"),
        (m030, "pmove.w (a0),tc"),
        (m030, "pmove.l (a0),srp"),
        (m030, "pmovefd crp,(a0)"),
        (m030, "pmovefd (a0),mmusr"),
        (m030, "pflush #8,#2"),
        (m030, "pflush #1,#8"),
        (m030, "pflush #1,#1,(a0)+"),
        (m030, "pflush a0,#1"),
        (m030, "ploadr #1,d0"),
        (m030, "ptestr #1,(a0),#8"),
        (m030, "ptestr #1,(a0)+,#1"),
        (m030, "ptestr #1,(a0),#1,d0"),
        // Level 0 searches only the ATC: there's no descriptor whose address could be given (vasm takes it).
        (m030, "ptestr #1,(a0),#0,a1"),
        (m030, "pflusha.l"),
        (m030, "pflusha #1"),
        (pmmu, "pmove d0,srp"),
        (pmmu, "pmove srp,-(a0)"),
        (pmmu, "pmove d0,pcsr"),
        (pmmu, "pmove #$12345,ac"),
        (pmmu, "pmove #$123,cal"),
        (pmmu, "pmove.b (a0),tc"),
        (pmmu, "pmove tc,#1"),
        (pmmu, "pflush #16,#1"),
        (pmmu, "pflushr d0"),
        (pmmu, "pflushr.l (a0)"),
        (pmmu, "psave (a0)+"),
        (pmmu, "prestore -(a0)"),
        (pmmu, "psbs a0"),
        (pmmu, "psbs.w d0"),
        (pmmu, "pbbs.b x"),
        (pmmu, "pdbbs.l d0,x"),
        (pmmu, "pdbbs a0,x"),
        (pmmu, "ptrapbs.b #1"),
        (pmmu, "pvalid d0,(a0)"),
        ("\tmachine 68040", "pflush (a0)+"),
        ("\tmachine 68040", "pflushan (a0)"),
        ("\tmachine 68040", "move16 (a0),(a1)"),
        ("\tmachine 68040", "move16 (a0)+,(a1)"),
        ("\tmachine 68040", "move16 (a0)+,$1234.w"),
        ("\tmachine 68040", "move16 (a0)+,(8,a1)"),
        ("\tmachine 68040", "cpusha dc/ic"),
        ("\tmachine 68040", "cinvl dc,8(a0)"),
        ("\tmachine 68040", "cinva xc"),
        ("\tmachine 68040", "fsmove fp1"),
        ("\tmachine 68040", "fsadd fp1"),
        ("\tmachine 68040", "fsmove fp0,(a0)"),
        ("\tmachine 68040\n\tfpu 0", "fsadd fp0,fp1"),
        ("\tmachine 68060", "lpstop #$12345"),
        ("\tmachine 68060", "lpstop d0"),
        ("\tmachine cpu32", "tblu.b (a0)+,d0"),
        ("\tmachine cpu32", "tblu.b d0,d1"),
        ("\tmachine cpu32", "tblu.b (a0),a1"),
        ("\tmachine cpu32", "tblu.b ([a0]),d0"),
        ("\tmachine cpu32", "bgnd #1"),
    ] {
        assert!(problem(choose, line).is_some(), "{line} should be refused after {choose:?}");
    }
}

/// vasm's other names are taken: PSR for MMUSR, .D for a root pointer's .Q, IACR0 and the rest (the 68EC040's
/// names) for ITT0 and the others.
#[test]
fn other_names_are_taken_as_vasm_takes_them() {
    let image = |source: &str| {
        let assembly = on(&format!("{source}\n\torg 0\nx:\n"));
        assert!(assembly.ok(), "{source}: {:?}", assembly.problems);
        let end = assembly.lines.iter().map(|l| l.address + l.bytes.len() as i64).max().unwrap_or(0);
        assembly.image(0, end)
    };
    assert_eq!(image("\tmachine 68030\n\tpmove psr,(a0)"), image("\tmachine 68030\n\tpmove mmusr,(a0)"));
    assert_eq!(image("\tmachine 68030\n\tpmove.d (a0),srp"), image("\tmachine 68030\n\tpmove.q (a0),srp"));
    assert_eq!(image("\tmachine 68040\n\tmovec iacr1,d0"), image("\tmachine 68040\n\tmovec itt1,d0"));
    assert_eq!(image("\tmachine 68040\n\tmovec d0,dacr0"), image("\tmachine 68040\n\tmovec d0,dtt0"));
}
