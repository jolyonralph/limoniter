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
    crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("t.s"), source, &|_| Err(crate::i18n::tr("no").into()))
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
    let moved = crate::asm::assemble_with(&crate::asm::m68k::M68k, Path::new("t.s"), source, &|_| Err(crate::i18n::tr("no").into()), 0x10_0000, &crate::asm::Options { folders: Vec::new(), values: None, ..Default::default() });
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
    let (assembly, file) = crate::asm::hunk::assemble_executable(Path::new("t.s"), source, &|_| Err(crate::i18n::tr("no").into()), false, &crate::asm::Options { folders: Vec::new(), values: None, ..Default::default() });
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

/// Decodes every instruction in `bytes` (at $1000) for a processor, writes it, and assembles what's written for
/// that processor: it has to give back the same bytes.
fn read_back(bytes: &[u8], cpu: Processor) -> usize {
    let word = |a: u32| be16(bytes, (a - 0x1000) as usize);
    let mut pc = 0x1000u32;
    let mut lines = vec![format!("\tmachine {}", cpu.name()), "\torg $1000".to_string()];
    let mut count = 0;
    while ((pc - 0x1000) as usize) < bytes.len() {
        let insn = decode_for(&word, pc, cpu).unwrap_or_else(|| panic!("${pc:X}: {:04X} {:04X} isn't decoded", word(pc).unwrap(), word(pc + 2).unwrap_or(0)));
        lines.push(format!("\t{}", text(&insn, &Plain)));
        pc += insn.length;
        count += 1;
    }
    let again = assemble(&(lines.join("\n") + "\n"));
    assert!(again.ok(), "{:?}", again.problems.first().map(|p| (&p.message, &lines[p.line])));
    let made = again.image(0x1000, 0x1000 + bytes.len() as i64);
    if made != bytes {
        let at = made.iter().zip(bytes.iter()).position(|(a, b)| a != b).unwrap_or(0) & !1;
        let line = again.lines.iter().find(|l| l.address as usize <= 0x1000 + at && l.address as usize + l.bytes.len() > 0x1000 + at).map(|l| lines[l.line].clone());
        panic!("differs at ${:X}: {line:?}", 0x1000 + at);
    }
    count
}

/// Every 68010, 68020 and 68030 instruction vasm assembled in m68k_020_all is read back for the 68020 (which has
/// them all, CALLM and RTM too) and written so that it assembles to vasm's bytes again.
#[test]
fn every_68020_form_vasm_makes_is_read_back() {
    let count = read_back(include_bytes!("../asm/testdata/m68k_020_all.bin"), Processor::M68020);
    assert!(count > 1400, "{count}");
    // The 68000's own come back the same when read as a 68020's, though a 68020's assembler takes them.
    read_back(include_bytes!("../asm/testdata/m68k_all.bin"), Processor::M68020);
}

/// What a processor hasn't got isn't read as an instruction for it: the 68020's on a 68000 or 68010, memory
/// indirect addressing on a CPU32, CALLM after the 68020.
#[test]
fn only_a_processors_own_instructions_are_read_for_it() {
    let read = |words: &[u16], cpu: Processor| {
        let word = |a: u32| words.get(((a - 0x1000) / 2) as usize).copied();
        decode_for(&word, 0x1000, cpu).map(|i| text(&i, &Plain))
    };
    // MOVEC VBR,D0; BFEXTU D1{4:8},D0; MOVE.L ([A0]),D0; CALLM #5,(A0); MULU.L D1,D2; a scaled index.
    let movec = [0x4E7A, 0x0801];
    let bfextu = [0xE9C1, 0x0108];
    let indirect = [0x2030, 0x0151];
    let callm = [0x06D0, 0x0005];
    let mulu = [0x4C01, 0x2002];
    let scaled = [0x2030, 0x1A00];
    assert_eq!(read(&movec, Processor::M68000), None);
    assert_eq!(read(&movec, Processor::M68010).as_deref(), Some("movec   vbr,d0"));
    assert_eq!(read(&bfextu, Processor::M68010), None);
    assert_eq!(read(&bfextu, Processor::M68030).as_deref(), Some("bfextu  d1{4:8},d0"));
    assert_eq!(read(&bfextu, Processor::Cpu32), None, "the CPU32 has no bit fields");
    assert_eq!(read(&indirect, Processor::Cpu32), None, "nor memory indirect addressing");
    assert_eq!(read(&indirect, Processor::M68020).as_deref(), Some("move.l  ([a0]),d0"));
    assert_eq!(read(&callm, Processor::M68020).as_deref(), Some("callm   #5,(a0)"));
    assert_eq!(read(&callm, Processor::M68030), None, "CALLM is only the 68020's");
    assert_eq!(read(&mulu, Processor::M68000), None);
    assert_eq!(read(&mulu, Processor::Cpu32).as_deref(), Some("mulu.l  d1,d2"));
    assert_eq!(read(&scaled, Processor::M68000), None);
    assert_eq!(read(&scaled, Processor::M68020).as_deref(), Some("move.l  0(a0,d1.l*2),d0"));
}

/// Every opcode word there is, with 68020 extension words after it (brief ones with scales, and full ones with
/// each displacement and indirection): whatever a 68020 reads is written so that it assembles back to those words.
#[test]
fn every_68020_opcode_decoded_is_written_so_it_assembles_back() {
    const AFTERS: [[u16; 6]; 4] = [
        [0x1A12, 0x0034, 0x8056, 0x0078, 0x009A, 0x00BC],
        [0x1D26, 0x0034, 0x8056, 0x0078, 0x009A, 0x00BC],
        [0x91B3, 0x0012, 0x3456, 0x0078, 0x9ABC, 0x00DE],
        [0x0161, 0xFFF0, 0x8056, 0x0178, 0x009A, 0x00BC],
    ];
    let mut decoded = 0;
    for after in AFTERS {
        for chunk in 0..16u32 {
            let mut lines = vec!["\tmachine 68020".to_string()];
            let mut wanted: Vec<(u32, Vec<u8>, String)> = Vec::new();
            for low in 0..0x1000u32 {
                let op = (chunk << 12 | low) as u16;
                let pc = 0x10_0000 + low * 32;
                let word = |a: u32| match (a - pc) / 2 {
                    0 => Some(op),
                    n => after.get(n as usize - 1).copied(),
                };
                let Some(insn) = decode_for(&word, pc, Processor::M68020) else { continue };
                let text = text(&insn, &Plain);
                let bytes: Vec<u8> = (0..insn.length / 2).flat_map(|i| word(pc + i * 2).unwrap().to_be_bytes()).collect();
                lines.push(format!("\torg ${pc:X}\n\t{text}"));
                wanted.push((pc, bytes, text));
                decoded += 1;
            }
            let assembly = assemble(&(lines.join("\n") + "\n"));
            if let Some(p) = assembly.problems.first() {
                let at = p.line.saturating_sub(1) / 2 + 1;
                panic!("{} on line {}: {}", p.message, p.line, lines.get(at).cloned().unwrap_or_default());
            }
            for (pc, bytes, text) in wanted {
                let made = assembly.image(i64::from(pc), i64::from(pc) + bytes.len() as i64);
                assert_eq!(made, bytes, "{text} at ${pc:X}");
            }
        }
    }
    assert!(decoded > 170_000, "{decoded}");
}

/// The 68020's forms as Limoniter writes them, assembled by vasm where it's been built (LIMONITER_VASM names it):
/// vasm takes them to the same bytes too.
#[test]
fn vasm_assembles_the_68020s_the_same() {
    let vasm = std::env::var_os("LIMONITER_VASM").map(std::path::PathBuf::from).unwrap_or_else(|| "/root/asmref/vasm/vasmm68k_mot".into());
    if !vasm.exists() {
        return;
    }
    let bytes = include_bytes!("../asm/testdata/m68k_020_all.bin");
    let word = |a: u32| be16(bytes, (a - 0x1000) as usize);
    let mut pc = 0x1000;
    let mut lines = vec!["\tmachine 68020".to_string(), "\torg $1000".to_string()];
    while ((pc - 0x1000) as usize) < bytes.len() {
        let insn = decode_for(&word, pc, Processor::M68020).unwrap();
        lines.push(format!("\t{}", text(&insn, &Plain)));
        pc += insn.length;
    }
    let folder = std::env::temp_dir().join(format!("limoniter-vasm020-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("t.s"), lines.join("\n") + "\n").unwrap();
    let status = std::process::Command::new(vasm).args(["-quiet", "-no-opt", "-Fbin", "-o"]).arg(folder.join("t.bin")).arg(folder.join("t.s")).status().unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read(folder.join("t.bin")).unwrap(), bytes.to_vec());
    let _ = std::fs::remove_dir_all(folder);
}

/// A 68020's code disassembled as a whole, followed from its start: it says MACHINE 68020, and every line comes
/// back as the bytes it came from, none of them made data for not doing so.
#[test]
fn a_68020_program_is_disassembled_for_the_68020() {
    let bytes = [
        0x20, 0x30, 0x01, 0x51, // move.l ([a0]),d0
        0xE9, 0xC1, 0x01, 0x08, // bfextu d1{4:8},d0
        0x4C, 0x01, 0x24, 0x03, // mulu.l d1,d3:d2
        0x67, 0xFF, 0x00, 0x00, 0x00, 0x08, // beq.l to the RTS
        0x4E, 0x7A, 0x08, 0x01, // movec vbr,d0
        0x4E, 0x75, // rts
    ];
    let mut program = raw(&bytes, 0x1000, &[0x1000], Machine::Plain);
    program.cpu = Processor::M68020;
    let out = disassemble(&program);
    assert!(out.contains("machine 68020") && out.contains("bfextu  d1{4:8},d0") && out.contains("beq.l") && !out.contains('?'), "{out}");
    assert_eq!(assemble(&out).image(0x1000, 0x1000 + bytes.len() as i64), bytes);
    // Read for a 68000, the same bytes are data.
    let plain = disassemble(&raw(&bytes, 0x1000, &[0x1000], Machine::Plain));
    assert!(!plain.contains("bfextu") && !plain.contains("machine"), "{plain}");
}

/// An Amiga program assembled by Limoniter's own assembler, from source.
fn amiga(source: &str) -> (Program, Vec<u8>) {
    let file = reassembled_amiga(source);
    (recognise("x", &file).unwrap(), file)
}

/// What a source assembles to, hunk by hunk: the bytes and the sorted relocations.
fn hunks_of(file: &[u8]) -> Vec<(Vec<u8>, Vec<(u32, u32)>)> {
    hunk::read_executable(file)
        .unwrap()
        .into_iter()
        .map(|h| {
            let mut r = h.relocations.clone();
            r.sort();
            (h.bytes, r)
        })
        .collect()
}

/// The source written for a program, checked from a source that's been changed, as if lines hadn't come back.
fn checked_after(program: &Program, change: impl Fn(&mut Vec<Line>)) -> String {
    let work = analyse(program);
    let mut lines = write(&work);
    change(&mut lines);
    let lines = check(&work, lines);
    lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n") + "\n"
}

/// A line made data by check() keeps its relocated long as a label, even one inside an instruction (label+2),
/// so the program still has every relocation: none is written as a number with the section's start added.
#[test]
fn a_relocated_long_is_never_made_a_number() {
    let source = "\tsection code,code\nstart:\tmove.l #$12345678,d0\n\tlea start+2,a0\n\tmove.l (a0),d1\n\trts\n\tsection data,data\nptr:\tdc.l start+2\n\tdc.l ptr\n";
    let (program, file) = amiga(source);
    let out = checked_after(&program, |lines| {
        // The LEA written as if it gave back other bytes: it's mended as data.
        let n = lines.iter().position(|l| l.text.contains("lea ")).unwrap();
        lines[n].text = lines[n].text.replace(",a0", ",a1");
    });
    assert!(out.contains("; lea     L0_000000+2,a1?") && out.contains("dc.l    L0_000000+2"), "{out}");
    assert!(!out.contains("dc.l    $"), "{out}");
    assert_eq!(hunks_of(&reassembled_amiga(&out)), hunks_of(&file), "{out}");
}

/// When a line doesn't assemble, or comes to another length, only it is mended, and the lines after it, wrong
/// only because their labels moved, are left as instructions.
#[test]
fn only_the_lines_that_went_wrong_are_made_data() {
    let source = "\tsection code,code\nstart:\tmoveq #0,d0\n\tbsr.w sub\n\tlea table,a0\n\tmove.w (a0),d1\n\tbsr.w sub\n\tjsr sub\n\trts\nsub:\taddq.w #1,d0\n\trts\n\tsection data,data\ntable:\tdc.w 1,2,3\n\tdcb.b 20,0\n";
    let (program, file) = amiga(source);
    for (wrong, by) in [("bsr.w", "bsr.s"), ("moveq ", "bogus ")] {
        let out = checked_after(&program, |lines| {
            let n = lines.iter().position(|l| l.text.contains(wrong)).unwrap();
            lines[n].text = lines[n].text.replace(wrong, by);
        });
        assert_eq!(out.matches("?\n").count(), 1, "{wrong}: {out}");
        assert_eq!(hunks_of(&reassembled_amiga(&out)), hunks_of(&file), "{out}");
    }
    // A run of zeros that didn't come back is data, written byte by byte, not an instruction gone wrong.
    let out = checked_after(&program, |lines| {
        let n = lines.iter().position(|l| l.text.contains("dcb.b")).unwrap();
        lines[n].text = lines[n].text.replace(",0", ",1");
    });
    assert!(!out.contains('?') && !out.contains("dcb.b") && out.contains("dc.b    $00,$00"), "{out}");
}

/// A place jumped to inside an instruction (code that overlaps itself) is named from the instruction's start,
/// as label+2, so the label is one the source defines.
#[test]
fn a_place_inside_an_instruction_is_written_from_its_label() {
    let source = "\tsection code,code\nstart:\tmove.w #$4E75,d1\n\tjsr start+2\n\tmove.l #start+2,a1\n\tjmp (a1)\n";
    let (program, file) = amiga(source);
    let out = disassemble(&program);
    assert!(out.contains("jsr     L000000+2") && out.contains("movea.l #L000000+2,a1"), "{out}");
    assert!(!out.contains("L000002") && !out.contains('?'), "{out}");
    assert_eq!(hunks_of(&reassembled_amiga(&out)), hunks_of(&file), "{out}");
}

/// An immediate address is only followed as code where it's put in an exception vector or jumped to through a
/// register. Put in a custom chip register, a data register or (a6), it's data, with a label.
#[test]
fn immediates_are_only_followed_as_code_where_they_are_run() {
    let source = "\tsection code,code\nstart:\tmove.l #handler,$6c.w\n\tmove.l #buffer,$dff054\n\tmove.l #table,d0\n\tmove.l #more,(a6)\n\tlea routine,a0\n\tjsr (a0)\n\trts\nhandler:\tmove.w #$20,$dff09c\n\trte\nroutine:\tmoveq #1,d0\n\trts\ntable:\tdc.w $3001,$3001,$4E75\nmore:\tdc.w $7001,$4E75\nbuffer:\tdcb.b 64,0\n";
    let (program, file) = amiga(source);
    let out = disassemble(&program);
    assert!(out.contains("rte") && out.contains("moveq   #1,d0"), "the handler and the routine are code:\n{out}");
    assert!(out.contains("; the level 3 interrupt vector"), "{out}");
    assert_eq!(out.matches("moveq   #1,d0").count(), 1, "{out}");
    assert!(out.contains("dc.b    $30,$01,$30,$01,$4E,$75") && out.contains("dc.b    $70,$01,$4E,$75") && out.contains("dcb.b   64,0"), "{out}");
    assert!(!out.contains("ori.b"), "{out}");
    assert_eq!(hunks_of(&reassembled_amiga(&out)), hunks_of(&file), "{out}");
}

/// Text is only written as text when it reads as text; a table of symbols is numbers.
#[test]
fn only_text_that_reads_as_text_is_a_string() {
    let source = "\tsection data,data\nwords:\tdc.b \"Copyright (c) 1989 Scoopex.\",0\ntable:\tdc.b \"!&)**+,/11/*%!!#\",0\nname:\tdc.b \"dos.library\",0\n";
    let (program, file) = amiga(source);
    let out = disassemble(&program);
    assert!(out.contains("dc.b    \"Copyright (c) 1989 Scoopex.\"") && out.contains("dc.b    \"dos.library\""), "{out}");
    assert!(!out.contains("\"!&)") && out.contains("$21,$26,$29,$2A,$2A,$2B"), "{out}");
    assert_eq!(hunks_of(&reassembled_amiga(&out)), hunks_of(&file), "{out}");
}

/// Copper lists, put in COP1LC, in graphics.library's LOFlist, or found by their end, are dc.w pairs with their
/// registers named (and defined), and their WAITs and end said; code writing into one names its place from the
/// pair's start.
#[test]
fn copper_lists_are_written_as_copper_instructions() {
    let source = "\tsection code,code\nstart:\tlea $dff000,a5\n\tmove.l #list,$80(a5)\n\tmove.w d0,list+6\n\tlea third,a0\n\tmove.l 4.w,a6\n\tlea gfxname,a1\n\tjsr -552(a6)\n\tmove.l d0,gfxbase\n\tmove.l gfxbase,a6\n\tadda.l #$32,a6\n\tmove.l #second,(a6)\n\trts\n\
\tsection data,data_c\nlist:\tdc.w $0100,$0200,$0180,$0000,$2c07,$fffe,$2c07,$ffff,$ffff,$fffe\nsecond:\tdc.w $0182,$0fff,$4001,$ff00\n\tdc.w 0,0\nthird:\tdc.w $0180,$0f00,$ffff,$fffe\nnotalist:\tdc.w $0180,$0f00,$1234,$5678\ngfxname:\tdc.b \"graphics.library\",0\n\teven\ngfxbase:\tdc.l 0\n";
    let (program, file) = amiga(source);
    let out = disassemble(&program);
    for said in ["dc.w    BPLCON0,$0200", "dc.w    COLOR00,$0000", "BPLCON0         equ     $100", "COLOR01         equ     $182", "; wait for line $2C, x $06", "; skip the next if the beam is past line $2C, x $06", "; the end of the copper list", "move.w  d0,L1_000004+2", "dc.w    COLOR01,$0FFF", "dc.w    COLOR00,$0F00"] {
        assert!(out.contains(said), "{said}:\n{out}");
    }
    // The list given only by its LOFlist ends where its copper instructions do, before the zeros.
    assert!(out.contains("dc.w    $4001,$FF00") && !out.contains("dc.w    $0000,$0000"), "{out}");
    // What only looks a little like one, with no end, is data.
    assert_eq!(out.matches("dc.w    COLOR00,$0F00").count(), 1, "{out}");
    assert_eq!(hunks_of(&reassembled_amiga(&out)), hunks_of(&file), "{out}");
}

/// Where vasm has been built, plain vasm (nothing asked of it but the format) makes the same program from what's
/// written: the source says OPT O- so that vasm doesn't make instructions shorter.
#[test]
fn plain_vasm_makes_the_same_program() {
    let vasm = std::env::var_os("LIMONITER_VASM").map(std::path::PathBuf::from).unwrap_or_else(|| "/root/asmref/vasm/vasmm68k_mot".into());
    if !vasm.exists() {
        return;
    }
    let folder = std::env::temp_dir().join(format!("limoniter-vasm-hunks-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let ours = "\tsection code,code\nstart:\tjsr sub\n\tlea table,a0\n\tmove.l #0,d0\n\tmove.l table,d1\n\tbra.w sub\nsub:\tadd.l #1,d0\n\trts\n\tsection data,data\ntable:\tdc.l start\n";
    for (name, file) in [("amiga_demo", include_bytes!("../asm/testdata/amiga_demo.exe").to_vec()), ("amiga_hello", include_bytes!("../asm/testdata/amiga_hello.exe").to_vec()), ("ours", reassembled_amiga(ours))] {
        let program = recognise(name, &file).unwrap();
        let source = disassemble(&program);
        assert!(source.contains("        opt     o-"), "{source}");
        std::fs::write(folder.join("t.s"), &source).unwrap();
        let status = std::process::Command::new(&vasm).args(["-quiet", "-Fhunkexe", "-o"]).arg(folder.join("t.exe")).arg(folder.join("t.s")).status().unwrap();
        assert!(status.success(), "{source}");
        assert_eq!(hunks_of(&std::fs::read(folder.join("t.exe")).unwrap()), hunks_of(&file), "{name}:\n{source}");
    }
    let _ = std::fs::remove_dir_all(folder);
}

/// SAE-MULater (Scoopex, 1989), a PowerPacked demo whose one Chip hunk holds code, copper lists, tables, music
/// and bitplanes together, from the test media beside the source where it's there (LIMONITER_TESTMEDIA names
/// another folder). Decrunched and disassembled, it assembles with Limoniter's own assembler, and with plain
/// vasm where it's been built, to the same hunk and all 363 of its relocations: no line has been made data for
/// not coming back, no label is undefined, and the zeroed bitplanes and buffers aren't read as ORI.B #0,D0.
#[test]
fn a_real_demo_comes_back_the_same() {
    let folder = std::env::var_os("LIMONITER_TESTMEDIA").map(std::path::PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../testmedia"));
    let Ok(crunched) = std::fs::read(folder.join("amiga-demos/SAE-MULater")) else { return };
    let file = crate::amiga_crunch::decrunch(&crunched).expect("PowerPacker").expect("decrunched").bytes;
    let original = hunks_of(&file);
    assert_eq!(original[0].1.len(), 363);
    for cpu in [Processor::M68000, Processor::M68020] {
        let mut program = recognise("SAE-MULater", &file).unwrap();
        program.cpu = cpu;
        let source = disassemble(&program);
        let flattened = source.lines().filter(|l| l.ends_with('?')).count();
        let zeros = source.matches("ori.b   #0,d0").count();
        assert_eq!(flattened, 0, "{cpu:?}: lines made data");
        assert!(zeros < 10, "{cpu:?}: {zeros} ORI.B #0,D0");
        assert!(source.contains("dc.w    $FFFF,$FFFE") && source.contains("dc.w    BPLCON0,"), "{cpu:?}: copper lists");
        assert_eq!(hunks_of(&reassembled_amiga(&source)), original, "{cpu:?}");
        let vasm = std::env::var_os("LIMONITER_VASM").map(std::path::PathBuf::from).unwrap_or_else(|| "/root/asmref/vasm/vasmm68k_mot".into());
        if vasm.exists() {
            let out = std::env::temp_dir().join(format!("limoniter-mulater-{}", std::process::id()));
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join("t.s"), &source).unwrap();
            let status = std::process::Command::new(&vasm).args(["-quiet", "-Fhunkexe", "-o"]).arg(out.join("t.exe")).arg(out.join("t.s")).status().unwrap();
            assert!(status.success(), "{cpu:?}: vasm");
            assert_eq!(hunks_of(&std::fs::read(out.join("t.exe")).unwrap()), original, "{cpu:?}: vasm");
            let _ = std::fs::remove_dir_all(out);
        }
    }
}
