use super::*;
use std::path::Path;

/// Source assembled by Limoniter's ARM assembler, from its lowest address to its highest.
fn assemble(source: &str) -> Vec<u8> {
    let assembly = crate::asm::assemble(&crate::asm::arm::Arm, Path::new("back.s"), source, &|p| Err(format!("no {}", p.display())));
    assert!(assembly.ok(), "{:?}\n{}", assembly.problems.iter().take(5).collect::<Vec<_>>(), source.lines().take(40).collect::<Vec<_>>().join("\n"));
    assembly.image(assembly.lowest().unwrap_or(0), assembly.highest().unwrap_or(0))
}

/// Bytes disassembled and assembled again: the same bytes, wherever they differ said.
fn round_trip(bytes: &[u8], origin: u32) {
    let source = disassemble(bytes, origin);
    let back = assemble(&source);
    if back != bytes {
        let at = back.iter().zip(bytes).position(|(a, b)| a != b).unwrap_or(back.len().min(bytes.len()));
        let line = source.lines().find(|l| l.contains(&format!("; {:08X}", origin as usize + at / 4 * 4))).unwrap_or("");
        panic!("byte {at} differs ({} of {} bytes): {line}", back.len(), bytes.len());
    }
}

/// Pseudo-random words, the same each run.
fn words(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .flat_map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.to_le_bytes()
        })
        .collect()
}

#[test]
fn every_word_comes_back_as_itself_instruction_or_not() {
    // 200,000 words of every kind: each an instruction, or DCD, that assembles back to itself.
    round_trip(&words(200_000, 7), 0x8000);
    // With conditions always AL, where most real code is.
    let always: Vec<u8> = words(100_000, 11).chunks(4).flat_map(|c| (u32::from_le_bytes(c.try_into().unwrap()) | 0xE000_0000).to_le_bytes()).collect();
    round_trip(&always, 0x8000);
}

#[test]
fn instructions_read_as_acorn_writes_them() {
    let label = |a: u32| format!("&{a:X}");
    for (word, text) in [
        (0xE3A0_0001u32, "mov r0,#1"),
        (0xE92D_400F, "stmfd sp!,{r0-r3,lr}"),
        (0xE8FD_800F, "ldmfd sp!,{r0-r3,pc}^"),
        (0x0092_1103, "addeqs r1,r2,r3,lsl #2"),
        (0xE33F_F000, "teqp pc,#0"),
        (0xEF00_0002, "swi \"OS_Write0\""),
        (0xEF02_0141, "swi \"XOS_WriteI+65\""),
        (0xE59F_1008, "ldr r1,&8010"),
        (0xEB00_0000, "bl &8008"),
        (0xE102_0091, "swp r0,r1,[r2]"),
    ] {
        assert_eq!(instruction(word, 0x8000, &label).as_deref(), Some(text), "{word:08X}");
    }
}

#[test]
fn real_archimedes_programs_come_back_byte_for_byte() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testmedia");
    let mut checked = 0;
    for disc in ["ace2-tcu1-ge72-crimbo-demos-archimedes-1990/ACE2, TCU1, Ge72, Crimbo Demos.adf", "zeridajh-video-digitiser-demo-disk-2-archimedes-1992/Zeridajh Video Digitiser Demo Disk 2.adf", "acorn-user-coverdisk-february-1994/Acorn User (February 1994).adf"] {
        let Ok(bytes) = std::fs::read(folder.join(disc)) else { continue };
        let source: std::sync::Arc<dyn crate::media::Source> = std::sync::Arc::new(bytes);
        let Ok(volume) = crate::media::open(source, disc) else { continue };
        let mut stack = vec![None];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = volume.list(dir.as_ref()) else { continue };
            for entry in entries {
                if entry.is_dir {
                    stack.push(Some(entry));
                    continue;
                }
                let Ok(file) = volume.read(&entry) else { continue };
                if file.len() < 256 || file.len() > 400_000 {
                    continue;
                }
                let whole = &file[..file.len() / 4 * 4];
                round_trip(whole, 0x8000);
                checked += 1;
            }
        }
    }
    if checked > 0 {
        eprintln!("{checked} files from Archimedes discs came back byte for byte");
        assert!(checked >= 10);
    }
}
