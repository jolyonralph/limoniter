use super::*;
use std::path::Path;

/// Names that are always numbers, for instructions on their own.
struct Plain;

impl Names for Plain {
    fn long(&self, value: u32, _at: u32) -> String {
        format!("${value:X}")
    }
    fn place(&self, address: u32) -> String {
        format!("${address:X}")
    }
    fn short(&self, address: u32) -> String {
        format!("${address:X}.w")
    }
}

fn assemble(source: &str) -> crate::asm::Assembly {
    crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("t.s"), source, &|_| Err("no".into()))
}

/// Every instruction vasm assembled in m68k_all (each form of each instruction), decoded one after another and
/// written again, gives back vasm's bytes.
#[test]
fn every_form_vasm_makes_is_read_back() {
    let bytes = include_bytes!("../asm/testdata/m68k_all.bin");
    let source = include_str!("../asm/testdata/m68k_all.asm");
    let _ = source;
    let word = |a: u32| be16(bytes, (a - 0x1000) as usize);
    let mut pc = 0x1000u32;
    let mut lines = vec!["\torg $1000".to_string()];
    let mut count = 0;
    while ((pc - 0x1000) as usize) < bytes.len() {
        let insn = decode(&word, pc).unwrap_or_else(|| panic!("${pc:X}: {:04X} isn't decoded", word(pc).unwrap()));
        lines.push(format!("\t{}", text(&insn, &Plain)));
        pc += insn.length;
        count += 1;
    }
    assert!(count > 700, "{count}");
    let again = assemble(&(lines.join("\n") + "\n"));
    assert!(again.ok(), "{:?}", again.problems.first().map(|p| (&p.message, &lines[p.line])));
    let made = again.image(0x1000, 0x1000 + bytes.len() as i64);
    if made != bytes {
        let at = made.iter().zip(bytes.iter()).position(|(a, b)| a != b).unwrap_or(0) & !1;
        let line = again.lines.iter().find(|l| l.address as usize <= 0x1000 + at && l.address as usize + l.bytes.len() > 0x1000 + at).map(|l| lines[l.line].clone());
        panic!("differs at ${:X}: {line:?}", 0x1000 + at);
    }
}

/// Every opcode word there is, with the same extension words after it: whatever is decoded is written so that
/// Limoniter's assembler gives back exactly those bytes.
#[test]
fn every_opcode_decoded_is_written_so_it_assembles_back() {
    const AFTER: [u16; 5] = [0x0012, 0x0034, 0x8056, 0x0078, 0x009A];
    let mut decoded = 0;
    for chunk in 0..16u32 {
        let mut lines = Vec::new();
        let mut wanted: Vec<(u32, Vec<u8>, String)> = Vec::new();
        for low in 0..0x1000u32 {
            let op = (chunk << 12 | low) as u16;
            let pc = 0x10_0000 + low * 16;
            let word = |a: u32| match (a - pc) / 2 {
                0 => Some(op),
                n => AFTER.get(n as usize - 1).copied(),
            };
            let Some(insn) = decode(&word, pc) else { continue };
            let text = text(&insn, &Plain);
            let bytes: Vec<u8> = (0..insn.length / 2).flat_map(|i| word(pc + i * 2).unwrap().to_be_bytes()).collect();
            lines.push(format!("\torg ${pc:X}\n\t{text}"));
            wanted.push((pc, bytes, text));
            decoded += 1;
        }
        let assembly = assemble(&(lines.join("\n") + "\n"));
        if let Some(p) = assembly.problems.first() {
            panic!("{} on line {}: {}", p.message, p.line, lines.get(p.line / 2).cloned().unwrap_or_default());
        }
        for (pc, bytes, text) in wanted {
            let made = assembly.image(i64::from(pc), i64::from(pc) + bytes.len() as i64);
            assert_eq!(made, bytes, "{text} at ${pc:X}");
        }
    }
    // The 68000 has about 45,000 opcode words that mean something.
    assert!(decoded > 40_000, "{decoded}");
}

fn reassembled_tos(source: &str) -> Vec<u8> {
    let normal = assemble(source);
    assert!(normal.ok(), "{:?}", normal.problems.first());
    let moved = crate::asm::assemble_with(&crate::asm::m68k::M68k, Path::new("t.s"), source, &|_| Err("no".into()), 0x10_0000, &crate::asm::Options { folders: Vec::new(), values: None });
    crate::asm::output::atari_program(&normal, &moved, 0x10_0000).unwrap()
}

#[test]
fn an_atari_st_program_comes_back_the_same_with_its_calls_named() {
    let original = include_bytes!("../asm/testdata/st_hello.tos");
    let program = recognise("hello.tos", original).unwrap();
    assert_eq!((program.layout, program.machine), (Layout::Tos, Machine::AtariSt));
    let source = disassemble(&program);
    assert!(source.contains("; GEMDOS Cconws"), "{source}");
    assert!(!source.contains("; dc.") && !source.contains("?\n"), "every instruction is written as one:\n{source}");
    assert_eq!(reassembled_tos(&source), original.to_vec(), "{source}");
}

fn reassembled_amiga(source: &str) -> Vec<u8> {
    let (assembly, file) = crate::asm::hunk::assemble_executable(Path::new("t.s"), source, &|_| Err("no".into()), false, &crate::asm::Options { folders: Vec::new(), values: None });
    assert!(assembly.ok(), "{:?}\n{source}", assembly.problems.first().map(|p| (&p.message, source.lines().nth(p.line))));
    file.unwrap()
}

/// vasm's Amiga programs, disassembled and assembled again: loaded at the same addresses, they're the same.
#[test]
fn amiga_programs_come_back_the_same_wherever_they_load() {
    for (name, file) in [
        ("amiga_demo", &include_bytes!("../asm/testdata/amiga_demo.exe")[..]),
        ("amiga_hello", &include_bytes!("../asm/testdata/amiga_hello.exe")[..]),
        ("amiga_merge", &include_bytes!("../asm/testdata/amiga_merge.exe")[..]),
        ("amiga_plain", &include_bytes!("../asm/testdata/amiga_plain.exe")[..]),
        ("amiga_all", &include_bytes!("../asm/testdata/amiga_all.exe")[..]),
    ] {
        let program = recognise(name, file).unwrap_or_else(|| panic!("{name} isn't recognised"));
        assert_eq!(program.layout, Layout::Hunks);
        let source = disassemble(&program);
        let again = reassembled_amiga(&source);
        let (ours, theirs) = (hunk::read_executable(&again).unwrap(), hunk::read_executable(file).unwrap());
        assert_eq!(ours.len(), theirs.len(), "{name}");
        let at: Vec<u32> = (0..ours.len() as u32).map(|i| 0x20_0000 + i * 0x10_0000).collect();
        assert_eq!(hunk::load(&ours, &at), hunk::load(&theirs, &at), "{name}:\n{source}");
        for (a, b) in ours.iter().zip(&theirs) {
            assert_eq!((a.kind, a.memory), (b.kind, b.memory), "{name}");
        }
    }
}

#[test]
fn the_amigas_hardware_and_library_calls_are_named() {
    let source = "\tsection code,code\nstart:\tmove.l 4.w,a6\n\tlea dosname,a1\n\tmoveq #0,d0\n\tjsr -552(a6)\n\tmove.l d0,dosbase\n\tmove.l dosbase,a6\n\tjsr -60(a6)\n\tmove.l 4.w,a6\n\tjsr -132(a6)\n\tlea $dff000,a5\n\tmove.w #$0f00,$180(a5)\n\tmove.w #$7fff,$dff09a\n\tmove.l #handler,$6c.w\n\trts\nhandler:\tmove.w #$20,$dff09c\n\trte\n\tsection data,data\ndosname:\tdc.b \"dos.library\",0\n\teven\ndosbase:\tdc.l 0\n";
    let file = reassembled_amiga(source);
    let program = recognise("x", &file).unwrap();
    let out = disassemble(&program);
    for said in ["; exec OpenLibrary", "; dos Output", "; exec Forbid", "; COLOR00", "; INTENA", "; INTREQ"] {
        assert!(out.contains(said), "{said}:\n{out}");
    }
    // The handler is found through the address put in the vector, and its long is a label.
    assert!(out.contains("move.l  #L0_000046,$6C.w") && out.contains("rte"), "{out}");
    assert!(out.contains("jsr     -552(a6)") && out.contains("; the custom chips"), "{out}");
    assert!(out.contains("dc.b    \"dos.library\""), "{out}");
    let at: Vec<u32> = vec![0x40_0000, 0x50_0000];
    assert_eq!(hunk::load(&hunk::read_executable(&reassembled_amiga(&out)).unwrap(), &at), hunk::load(&hunk::read_executable(&file).unwrap(), &at));
}

#[test]
fn plain_code_follows_its_branches_and_leaves_data_alone() {
    let source = "\torg $40000\nstart:\tlea table(pc),a0\n\tmoveq #3,d0\nloop:\tmove.w (a0)+,d1\n\tbsr.s show\n\tdbra d0,loop\n\tbra.w start\nshow:\tmove.w d1,$ffff8240.w\n\trts\ntable:\tdc.w $777,$700,$070,$007\n\tdc.b \"text that isn't code\",0\n\teven\n";
    let assembly = assemble(source);
    let bytes = assembly.image(0x40000, assembly.highest().unwrap());
    let program = raw(&bytes, 0x40000, &[0x40000], Machine::AtariSt);
    let out = disassemble(&program);
    assert!(out.contains("dbra    d0,L040006") && out.contains("bsr.s   L040012"), "{out}");
    assert!(out.contains("; VIDEO_PAL0") || out.contains("; COLOR0") || out.contains("8240"), "{out}");
    assert!(out.contains("dc.b    \"text that isn"), "{out}");
    let again = assemble(&out);
    assert_eq!(again.image(0x40000, 0x40000 + bytes.len() as i64), bytes, "{out}");
}

#[test]
fn boot_blocks_cartridges_and_roms_are_recognised() {
    let mut boot = vec![0u8; 1024];
    boot[..4].copy_from_slice(b"DOS\0");
    boot[12..14].copy_from_slice(&[0x4E, 0x75]);
    let p = recognise("boot.bin", &boot).unwrap();
    assert_eq!((p.machine, p.starts.as_slice()), (Machine::Amiga, &[12][..]));
    let mut md = vec![0u8; 0x400];
    md[0x100..0x104].copy_from_slice(b"SEGA");
    md[4..8].copy_from_slice(&0x200u32.to_be_bytes());
    md[0x78..0x7C].copy_from_slice(&0x300u32.to_be_bytes());
    md[0x200..0x202].copy_from_slice(&[0x60, 0xFE]);
    md[0x300..0x302].copy_from_slice(&[0x4E, 0x73]);
    let p = recognise("game.md", &md).unwrap();
    assert_eq!(p.machine, Machine::MegaDrive);
    assert!(p.starts.contains(&0x200) && p.starts.contains(&0x300));
    let out = disassemble(&p);
    assert!(out.contains("dc.l    L000200") && out.contains("rte"), "{out}");
    let again = assemble(&out);
    assert_eq!(again.image(0, md.len() as i64), md);
}

/// The same with vasm, where it's been built: what Limoniter writes is what vasm takes too.
#[test]
fn vasm_assembles_it_the_same() {
    let vasm = Path::new("/root/asmref/vasm/vasmm68k_mot");
    if !vasm.exists() {
        return;
    }
    let bytes = include_bytes!("../asm/testdata/m68k_all.bin");
    let program = raw(bytes, 0x1000, &[0x1000], Machine::Plain);
    // Every instruction one after another, as data runs into code here; written from the decoder directly.
    let word = |a: u32| be16(bytes, (a - 0x1000) as usize);
    let mut pc = 0x1000;
    let mut lines = vec!["\torg $1000".to_string()];
    while ((pc - 0x1000) as usize) < bytes.len() {
        let insn = decode(&word, pc).unwrap();
        lines.push(format!("\t{}", text(&insn, &Plain)));
        pc += insn.length;
    }
    let _ = program;
    let folder = std::env::temp_dir().join(format!("limoniter-vasm-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("t.s"), lines.join("\n") + "\n").unwrap();
    let status = std::process::Command::new(vasm).args(["-quiet", "-no-opt", "-Fbin", "-o"]).arg(folder.join("t.bin")).arg(folder.join("t.s")).status().unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read(folder.join("t.bin")).unwrap(), bytes.to_vec());
    let _ = std::fs::remove_dir_all(folder);
}
