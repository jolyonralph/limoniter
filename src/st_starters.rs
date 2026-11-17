//! The Atari ST programs File, New as starts with: an ST demo (supervisor mode, its own screen, a VBL handler and
//! Timer B raster bars, everything given back at the end) and an STE one (the same, with the STE's 4096 colours
//! and hardware scrolling). Both are in Devpac/vasm syntax and assemble with Limoniter's own assembler, to the same
//! program vasm makes.

/// The ST demo starter.
pub const ST_DEMO: &str = include_str!("starters/st_demo.s");

/// The STE demo starter.
pub const STE_DEMO: &str = include_str!("starters/ste_demo.s");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::{assemble, assemble_at, m68k, output};
    use std::path::Path;

    fn no_files(path: &Path) -> Result<Vec<u8>, String> {
        Err(crate::i18n::tf!("{} isn't there", path.display()))
    }

    /// A starter made into a TOS program here, against what vasm 2.0f made of it (vasmm68k_mot -Ftos -nosym).
    fn matches_vasm(source: &str, vasm: &[u8]) {
        let here = assemble(&m68k::M68k, Path::new("demo.s"), source, &no_files);
        assert!(here.ok() && !here.absolute, "{:?}", here.problems.iter().take(5).collect::<Vec<_>>());
        assert!(!crate::asm::hunk::is_amiga_source(source), "it's made into an ST program, not an Amiga one");
        let moved = assemble_at(&m68k::M68k, Path::new("demo.s"), source, &no_files, 0x10000);
        let ours = output::atari_program(&here, &moved, 0x10000).unwrap();
        assert_eq!(ours.len(), vasm.len());
        assert_eq!(ours, vasm);
    }

    #[test]
    fn the_st_starter_assembles_as_vasm_assembles_it() {
        matches_vasm(ST_DEMO, include_bytes!("starters/st_demo.vasm"));
    }

    #[test]
    fn the_ste_starter_assembles_as_vasm_assembles_it() {
        matches_vasm(STE_DEMO, include_bytes!("starters/ste_demo.vasm"));
    }

    #[test]
    fn the_starters_explain_themselves_through_hover() {
        for text in [ST_DEMO, STE_DEMO] {
            assert!(!text.contains('\r'));
            let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, Path::new("demo.s"), text).assembly;
            let source = crate::st_source::Source::scan(text, Some(&assembly));
            assert!(source.atari);
            let amiga = crate::amiga_source::Source::scan(text, None);
            assert!(!amiga.amiga && amiga.lists.is_empty(), "nothing of the Amiga's");
            let (n, line) = text.lines().enumerate().find(|(_, l)| l.contains("move.b  #8,TBCR.w")).unwrap();
            assert!(source.describe(n, line, line.find("TBCR").unwrap() + 1).unwrap().contains("event count mode"));
            let (n, line) = text.lines().enumerate().find(|(_, l)| l.contains("Setscreen")).unwrap();
            let trap = n + 1;
            assert!(source.describe(trap, text.lines().nth(trap).unwrap(), 9).unwrap().starts_with("XBIOS Setscreen"), "{line}");
        }
    }
}
