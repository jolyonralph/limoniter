//! The Amiga programs File, New as starts with: a demo (taking the machine over, one bitplane, a raster bar, and
//! giving it all back) and a smaller copper-only one. Both are in Devpac/vasm syntax and assemble with
//! Limoniter's own assembler as with vasm.

/// The demo starter: system takeover, a bitplane, a moving raster bar, left mouse button to leave.
/// No longer on File, New as (How To Code v8's startup is, as one file), but kept: vasm's assembly of it checks
/// Limoniter's 68000 assembler.
#[cfg(test)]
pub const DEMO: &str = include_str!("starters/amiga_demo.s");

/// The copper-only starter: system takeover and a colour gradient made by the copper.
pub const COPPER: &str = include_str!("starters/amiga_copper.s");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::{assemble, m68k};
    use std::path::Path;

    fn long(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    /// An AmigaDOS executable's hunks, each loaded at the base given and its 32-bit relocations made: what the
    /// Amiga's loader would put in memory. Each hunk's size in longs, its memory type and its contents.
    fn load_hunks(file: &[u8], bases: &[u32]) -> Vec<(u32, u32, Vec<u8>)> {
        let at = std::cell::Cell::new(0);
        let next = || {
            at.set(at.get() + 4);
            long(file, at.get() - 4)
        };
        assert_eq!(next(), 0x3F3, "HUNK_HEADER");
        assert_eq!(next(), 0, "no resident libraries");
        let count = next() as usize;
        let (first, last) = (next(), next());
        assert_eq!((first, last as usize), (0, count - 1));
        let sizes: Vec<u32> = (0..count).map(|_| next()).collect();
        let mut hunks = Vec::new();
        for size in sizes {
            let mut memory = vec![0u8; (size & 0x3FFF_FFFF) as usize * 4];
            loop {
                match next() & 0x3FFF_FFFF {
                    0x3E9 | 0x3EA => {
                        let longs = next() as usize;
                        memory[..longs * 4].copy_from_slice(&file[at.get()..at.get() + longs * 4]);
                        at.set(at.get() + longs * 4);
                    }
                    0x3EB => {
                        next();
                    }
                    0x3EC => loop {
                        let offsets = next() as usize;
                        if offsets == 0 {
                            break;
                        }
                        let target = next() as usize;
                        for _ in 0..offsets {
                            let offset = next() as usize;
                            let value = long(&memory, offset).wrapping_add(bases[target]);
                            memory[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
                        }
                    },
                    0x3F2 => break,
                    other => panic!("hunk type ${other:X} at {}", at.get()),
                }
            }
            hunks.push((size & 0x3FFF_FFFF, size >> 30, memory));
        }
        hunks
    }

    /// A starter assembled here, against what vasm 2.0f made of it (vasmm68k_mot -Fhunkexe -kick1hunks -nosym):
    /// each section's bytes, with the program's own addresses in them, must be the same as vasm's hunk once it's
    /// loaded at the same place.
    fn matches_vasm(source: &str, vasm: &[u8], sections: &[&str], chip: &[bool]) {
        let assembly = assemble(&m68k::M68k, Path::new("demo.s"), source, &|p: &Path| Err(crate::i18n::tf!("{} isn't there", p.display())));
        assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).collect::<Vec<_>>());
        let low = assembly.lowest().unwrap();
        let image = assembly.image(low, assembly.highest().unwrap());
        // Where each section's first label landed here.
        let bases: Vec<u32> = sections.iter().map(|label| (assembly.symbols[*label] - low) as u32).collect();
        let hunks = load_hunks(vasm, &bases);
        assert_eq!(hunks.len(), sections.len());
        for (i, (_, memory_type, memory)) in hunks.iter().enumerate() {
            assert_eq!(*memory_type == 1, chip[i], "section {} in chip memory", sections[i]);
            let start = bases[i] as usize;
            // vasm rounds each hunk up to whole longs; ours is the bytes up to where the next section starts.
            let end = bases.get(i + 1).map(|b| *b as usize).unwrap_or(image.len());
            let ours: Vec<u8> = image.get(start..end).map(<[u8]>::to_vec).unwrap_or_default();
            let theirs = &memory[..memory.len().min(end - start)];
            if !ours.is_empty() {
                assert_eq!(&ours[..], &theirs[..ours.len()], "section {}", sections[i]);
            } else {
                // Space (DS) at the end isn't in the image: vasm's hunk must be all zero.
                assert!(theirs.iter().all(|b| *b == 0), "section {}", sections[i]);
            }
        }
    }

    #[test]
    fn the_demo_starter_assembles_as_vasm_assembles_it() {
        matches_vasm(DEMO, include_bytes!("starters/amiga_demo.vasm"), &["start", "gfxbase", "copper", "screen"], &[false, false, true, true]);
    }

    #[test]
    fn the_copper_starter_assembles_as_vasm_assembles_it() {
        matches_vasm(COPPER, include_bytes!("starters/amiga_copper.vasm"), &["start", "gfxbase", "copper"], &[false, false, true]);
    }

    #[test]
    fn the_starters_are_whole_programs_with_plain_line_ends() {
        for text in [DEMO, COPPER] {
            assert!(text.contains("section code,code"));
            assert!(text.contains("LoadView"));
            assert!(!text.contains('\r'));
        }
    }
}
