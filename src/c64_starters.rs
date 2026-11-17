//! The Commodore 64 programs File, New as starts with: a demo (a BASIC line to start it, the KERNAL out, a chain
//! of raster interrupts with stable raster bars in the border, a sprite, and back to BASIC on space) and an intro
//! with hooks for a SID tune at $1000. Both are in ACME's syntax, for PAL machines, and assemble with Limoniter's
//! own assembler to the same program ACME makes.

/// The demo starter.
pub const DEMO: &str = include_str!("starters/c64_demo.a");

/// The intro starter, with music.
pub const INTRO: &str = include_str!("starters/c64_intro.a");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::{assemble, m6502, output};
    use std::path::Path;

    fn no_files(path: &Path) -> Result<Vec<u8>, String> {
        Err(crate::i18n::tf!("{} isn't there", path.display()))
    }

    /// A starter made into a .prg here, against what ACME 0.97 made of it (acme -f cbm).
    fn matches_acme(source: &str, acme: &[u8]) -> crate::asm::Assembly {
        let here = assemble(&m6502::M6502, Path::new("demo.a"), source, &no_files);
        assert!(here.ok(), "{:?}", here.problems.iter().take(5).collect::<Vec<_>>());
        let ours = output::commodore_program(&here).unwrap();
        assert_eq!(ours.len(), acme.len());
        assert_eq!(ours, acme);
        here
    }

    /// Where a label is.
    fn at(assembly: &crate::asm::Assembly, name: &str) -> i64 {
        *assembly.symbols.get(name).unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn the_demo_assembles_as_acme_assembles_it() {
        let a = matches_acme(DEMO, include_bytes!("starters/c64_demo.prg"));
        assert_eq!(at(&a, "start"), 2061);
        // The bar loop is a line long, 63 cycles, with its branches taken and in one page.
        let bar = at(&a, "bar_line");
        let source_line = DEMO.lines().position(|l| l.starts_with("bar_line")).unwrap();
        let (sum, count) = crate::m6502_timing::block(&a, 0, source_line, source_line + 10);
        assert_eq!(count, 10);
        // DEY and BNE run 8 times (the BNE taken 7 of them), the loop's own BNE is taken.
        assert_eq!(sum.least, 4 + 4 + 2 + 2 + 2 + 2 + 2 + 2 + 5 + 2);
        assert_eq!(sum.least + 7 * 5 + 1, 63);
        assert_eq!(bar >> 8, (bar + 20) >> 8, "the loop doesn't cross a page");
        assert_eq!(at(&a, "bars_irq") >> 8, at(&a, "bar_start") >> 8, "nor does any timed code");
    }

    #[test]
    fn the_intro_assembles_as_acme_assembles_it() {
        let a = matches_acme(INTRO, include_bytes!("starters/c64_intro.prg"));
        assert_eq!(at(&a, "start"), 2061);
        assert_eq!(at(&a, "music_init"), 0x1000);
        assert_eq!(at(&a, "music_play"), 0x1003);
        assert_eq!(at(&a, "bars_irq") >> 8, at(&a, "bar_start") >> 8, "the timed code is in one page");
    }

    #[test]
    fn a_test_copy_stops_by_itself() {
        // TEST_FRAMES set makes a copy that goes back to BASIC after that many frames, as the checks in VICE use.
        for text in [DEMO, INTRO] {
            let test = format!("TEST_FRAMES = 100\n{text}");
            let a = assemble(&m6502::M6502, Path::new("demo.a"), &test, &no_files);
            assert!(a.ok(), "{:?}", a.problems);
            let plain = assemble(&m6502::M6502, Path::new("demo.a"), text, &no_files);
            assert_eq!(at(&a, "start"), at(&plain, "start"));
            assert!(at(&a, "exit") > at(&plain, "exit"));
            assert!(!text.contains('\r') && !text.contains('\t'));
        }
    }
}
