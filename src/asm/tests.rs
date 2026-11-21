use super::*;
use std::path::Path;

fn no_files(path: &Path) -> Result<Vec<u8>, String> {
    Err(crate::i18n::tf!("{} isn't there", path.display()))
}

fn z80(text: &str) -> Assembly {
    assemble(&z80::Z80, Path::new("test.asm"), text, &no_files)
}

fn bytes(text: &str) -> Vec<u8> {
    let assembly = z80(text);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assembly.chunks.iter().flat_map(|c| c.bytes.clone()).collect()
}

/// Every Z80 instruction, documented and undocumented, assembled as sjasmplus 1.24 assembles it.
fn matches_sjasmplus(source: &str, expected: &[u8]) {
    let assembly = z80(source);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).collect::<Vec<_>>());
    let ours = assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap());
    if ours != expected {
        // Say which line differs first.
        for placed in &assembly.lines {
            let at = (placed.address - assembly.lowest().unwrap()) as usize;
            let theirs = expected.get(at..at + placed.bytes.len());
            if theirs != Some(&placed.bytes[..]) {
                panic!("line {}: \"{}\" gave {:02X?}, sjasmplus {:02X?}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""), placed.bytes, theirs);
            }
        }
        panic!("{} bytes, sjasmplus {}", ours.len(), expected.len());
    }
}

#[test]
fn every_z80_instruction_is_assembled_as_sjasmplus_does() {
    matches_sjasmplus(include_str!("testdata/z80_all.asm"), include_bytes!("testdata/z80_all.bin"));
}

#[test]
fn every_next_instruction_is_assembled_as_sjasmplus_does() {
    matches_sjasmplus(include_str!("testdata/z80n_all.asm"), include_bytes!("testdata/z80n_all.bin"));
}

#[test]
fn labels_can_be_used_before_they_are_defined() {
    assert_eq!(bytes("  org $8000\n  jp later\n  nop\nlater: ret\n"), vec![0xC3, 0x04, 0x80, 0x00, 0xC9]);
    let assembly = z80("  org 100\nstart\n  ld a,1\n.loop\n  djnz .loop\nother:\n.loop: jr .loop\n");
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.symbols.get("start.loop"), Some(&102));
    assert_eq!(assembly.symbols.get("other.loop"), Some(&104));
}

#[test]
fn problems_say_which_line_and_what() {
    let assembly = z80("  org $8000\n  ld a,missing\n  jr far\n  ds 200\nfar: ld q,1\n  lb a,b\n  ld a,300\n");
    let lines: Vec<(usize, &str)> = assembly.problems.iter().map(|p| (p.line, p.message.as_str())).collect();
    assert!(lines.iter().any(|(l, m)| *l == 1 && m.contains("missing isn't defined")), "{lines:?}");
    assert!(lines.iter().any(|(l, m)| *l == 2 && m.contains("out of a relative jump's reach")), "{lines:?}");
    assert!(lines.iter().any(|(l, m)| *l == 4 && m.contains("isn't a Z80 instruction")), "{lines:?}");
    assert!(lines.iter().any(|(l, m)| *l == 5 && m.contains("lb isn't a Z80 instruction")), "{lines:?}");
    // A value too big for a byte is cut to fit, with a warning, as sjasmplus does.
    assert!(assembly.warnings.iter().any(|w| w.line == 6 && w.message.contains("doesn't fit in a byte")), "{:?}", assembly.warnings);
    let twice = z80("a: nop\na: nop\n");
    assert!(twice.problems.iter().any(|p| p.line == 1 && p.message.contains("defined twice")), "{:?}", twice.problems);
}

#[test]
fn macros_repeats_and_conditions() {
    let source = "  macro border colour\n  ld a,colour\n  out ($fe),a\n  endm\n  org $8000\n  border 2\n  border 5\n  rept 3\n  nop\n  endr\n  if 1 > 2\n  halt\n  else\n  ret\n  endif\n";
    assert_eq!(bytes(source), vec![0x3E, 2, 0xD3, 0xFE, 0x3E, 5, 0xD3, 0xFE, 0, 0, 0, 0xC9]);
    // sjasmplus's way round, MACRO first.
    assert_eq!(bytes("  macro twice x\n  db x,x\n  endm\n  twice 7\n"), vec![7, 7]);
    assert_eq!(bytes("  ifdef nothing\n  nop\n  endif\n  ifndef nothing\n  ret\n  endif\n"), vec![0xC9]);
}

#[test]
fn several_statements_on_a_line_and_comments() {
    assert_eq!(bytes("  ld a,1 : ld b,2 ; a comment\n  // another\n  ld c,';' ; ex af,af'\n  ex af,af' : nop\n"), vec![0x3E, 1, 0x06, 2, 0x0E, b';', 0x08, 0x00]);
}

#[test]
fn sjasmplus_ways_of_writing() {
    // Block comments, DISP to assemble code for where it will be copied to, and a routine called PRINT.
    let source = "  org $8000\n/* a comment /* nested */\n  over lines */ nop\n  ld a,1 /* here */ : ret\n  disp $c000\nthere: jp there\n  ent\nPRINT\n  call PRINT\nTEXT dw TEXT\nSL1 ret\n";
    let assembly = z80(source);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.image(0x8000, 0x800C), vec![0x00, 0x3E, 1, 0xC9, 0xC3, 0x00, 0xC0, 0xCD, 0x07, 0x80, 0x0A, 0x80]);
    assert_eq!(assembly.symbols.get("there"), Some(&0xC000));
    assert_eq!(assembly.line_at(0xC000), Some((0, 5)), "the debugger finds code by where it runs");
}

#[test]
fn includes_are_read_beside_the_file() {
    let read = |path: &Path| -> Result<Vec<u8>, String> {
        match path.file_name().and_then(|n| n.to_str()) {
            Some("defs.asm") => Ok(b"colour equ 4\n".to_vec()),
            Some("pic.bin") => Ok(vec![9, 8, 7, 6]),
            _ => Err(crate::i18n::tr("no").into()),
        }
    };
    let assembly = assemble(&z80::Z80, Path::new("/src/main.asm"), "  include \"defs.asm\"\n  ld a,colour\n  incbin \"pic.bin\",1,2\n", &read);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.image(0, 4), vec![0x3E, 4, 8, 7]);
    assert_eq!(assembly.files[1], Path::new("/src/defs.asm"));
}

#[test]
fn source_lines_and_addresses_map_both_ways() {
    let assembly = z80("  org $8000\nstart: ld a,1\n  ; nothing\n  ret\n  end start\n");
    assert_eq!(assembly.line_at(0x8001), Some((0, 1)));
    assert_eq!(assembly.line_at(0x8002), Some((0, 3)));
    assert_eq!(assembly.address_of(0, 3), Some(0x8002));
    assert_eq!(assembly.address_of(0, 2), None);
    assert_eq!(assembly.start(), Some(0x8000));
}

#[test]
fn instructions_say_how_long_they_take() {
    let t = |text: &str| z80::cycles(&bytes(text)).unwrap();
    assert_eq!(t("nop"), "4");
    assert_eq!(t("ld a,(hl)"), "7");
    assert_eq!(t("ld (ix+1),a"), "19");
    assert_eq!(t("jr nz,$"), "7/12");
    assert_eq!(t("ldir"), "16/21");
    assert_eq!(t("call $1234"), "17");
    assert_eq!(t("bit 0,(ix+1)"), "20");
    assert_eq!(t("push ix"), "15");
}

#[test]
fn a_tape_loads_and_calls_the_code() {
    let assembly = z80("  org 32768\nstart: ld a,2\n  out ($fe),a\n  ret\n");
    let tape = output::spectrum_tape(&assembly, "border").unwrap();
    // The loader as a BASIC program, then the code's header and the code.
    let code_header = &tape[tape.len() - 9 - 21..];
    assert_eq!(code_header[3], 3, "a CODE block");
    assert_eq!(u16::from_le_bytes([code_header[16], code_header[17]]), 32768);
    assert_eq!(&tape[tape.len() - 6..tape.len() - 1], &[0x3E, 2, 0xD3, 0xFE, 0xC9]);
    let low = z80("  org 16384\n  nop\n");
    assert!(output::spectrum_tape(&low, "x").unwrap_err().contains("below 24000"));
}

#[test]
fn a_next_file_matches_sjasmplus() {
    let source = include_str!("testdata/nex_hello.asm");
    let assembly = z80(source);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let ours = output::next_file(&assembly, Some(0xFFFE)).unwrap();
    let theirs = include_bytes!("testdata/nex_hello.nex");
    assert_eq!(ours.len(), theirs.len());
    let differ: Vec<usize> = (0..ours.len()).filter(|i| ours[*i] != theirs[*i]).collect();
    assert!(differ.is_empty(), "differs at {:?}: ours {:02X?} theirs {:02X?}", &differ[..differ.len().min(8)], differ.iter().take(8).map(|i| ours[*i]).collect::<Vec<_>>(), differ.iter().take(8).map(|i| theirs[*i]).collect::<Vec<_>>());
}

fn m6502(text: &str) -> Assembly {
    assemble(&m6502::M6502, Path::new("test.asm"), text, &no_files)
}

/// Every 6502 instruction in every mode, the undocumented ones too, as ACME assembles them.
#[test]
fn every_6502_instruction_is_assembled_as_acme_does() {
    let source = include_str!("testdata/m6502_all.asm");
    let assembly = m6502(source);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).collect::<Vec<_>>());
    let expected = include_bytes!("testdata/m6502_all.bin");
    let ours = assembly.image(0x1000, 0x1000 + expected.len() as i64);
    for placed in &assembly.lines {
        let at = (placed.address - 0x1000) as usize;
        assert_eq!(Some(&placed.bytes[..]), expected.get(at..at + placed.bytes.len()), "line {}: {}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""));
    }
    assert_eq!(ours, expected);
}

#[test]
fn the_6502_says_how_long_instructions_take() {
    let t = |text: &str| {
        let a = m6502(text);
        assert!(a.ok(), "{:?}", a.problems);
        Isa::cycles(&m6502::M6502, &a.chunks[0].bytes).unwrap()
    };
    assert_eq!(t("  lda #1"), "2");
    assert_eq!(t("  lda $1234,x"), "4/5");
    assert_eq!(t("  sta $1234,x"), "5");
    assert_eq!(t("  inc $10"), "5");
    assert_eq!(t("  jsr $1000"), "6");
    // A branch takes one more taken, and another into a different page, which its address tells.
    assert_eq!(t("- bne -"), "2/4");
    let a = m6502("*=$1000\n- bne -\n");
    assert_eq!(Isa::cycles_at(&m6502::M6502, &a.chunks[0].bytes, 0x1000).unwrap(), "2/3");
}

#[test]
fn a_commodore_program_starts_with_run() {
    let at_basic = m6502("  *=$0801\n  !byte 1,2\n");
    assert_eq!(output::commodore_program(&at_basic).unwrap(), vec![0x01, 0x08, 1, 2]);
    let high = m6502("  *=$c000\nstart lda #1\n  rts\n");
    let prg = output::commodore_program(&high).unwrap();
    assert_eq!(&prg[..2], &[0x01, 0x08]);
    // 10 SYS 49152, as the C64's BASIC lists it, then the code at $C000.
    let listed = crate::basic::list(&prg, "prg", crate::charset::utf8());
    let text = listed.lines.join("\n");
    assert!(text.contains("10 SYS 49152") || text.contains("10 SYS  49152"), "{text}");
    assert_eq!(&prg[2 + 0xC000 - 0x0801..], &[0xA9, 1, 0x60]);
    assert!(output::commodore_program(&m6502("  *=$0805\n  nop\n")).is_err());
}

#[test]
fn bbc_code_is_told_from_commodore_code_by_the_calls_it_makes() {
    use output::Micro6502::{Bbc, C64};
    let said = |text: &str| output::machine_6502(&m6502(text));
    // OSWRCH and OSBYTE are the BBC's; CHROUT is the C64's.
    assert_eq!(said("  *=$2000\n  lda #65\n  jsr $ffee\n  rts\n"), Some(Bbc));
    assert_eq!(said("  *=$2000\n  lda #200\n  jmp $fff4\n"), Some(Bbc));
    assert_eq!(said("  *=$c000\n  lda #65\n  jsr $ffd2\n  rts\n"), Some(C64));
    // OSNEWL is at the C64's CLALL, so it doesn't say which; and a call to both says neither.
    assert_eq!(said("  *=$2000\n  jsr $ffe7\n  rts\n"), None);
    assert_eq!(said("  *=$c000\n  jsr $ffee\n  jsr $ffd2\n  rts\n"), None);
    assert_eq!(said("  *=$2000\n  jsr $ffee\n  jsr $ffe7\n  rts\n"), Some(Bbc), "OSWRCH, then OSNEWL");
    // Bytes that only look like a call, in data, don't count unless they're a whole line's three bytes.
    assert_eq!(said("  *=$c000\n  !byte 1,$20,$ee,$ff\n"), None);
}

#[test]
fn the_machine_6502_code_is_for_is_told_from_its_calls_its_chips_and_where_it_starts() {
    use output::Micro6502::{Bbc, C64};
    let said = |text: &str| output::machine_6502(&m6502(text));
    assert_eq!(said("  *=$2000\n  lda #65\n  jsr $ffee\n  rts\n"), Some(Bbc), "OSWRCH");
    assert_eq!(said("  *=$2000\n  lda #1\n  sta $fe40\n  rts\n"), Some(Bbc), "the system VIA, in SHEILA");
    assert_eq!(said("  *=$2000\n  lda #65\n  jsr $ffd2\n  rts\n"), Some(C64), "CHROUT");
    assert_eq!(said("  *=$2000\n  ldx #0\n  sta $d800,x\n  rts\n"), Some(C64), "colour memory");
    assert_eq!(said("  *=$c000\n  lda #1\n  sta $70\n  rts\n"), None, "nothing says");
    assert_eq!(said("  *=$2000\n  jsr $ffee\n  sta $d020\n  rts\n"), None, "both say");
    assert_eq!(said("  *=$1000\n  lda #1\n  sta $70\n  rts\n"), Some(C64), "below &1900, where a BBC disc can't load it");
    assert_eq!(said("  *=$1000\n  jsr $ffee\n  rts\n"), Some(Bbc), "a BBC's call says more than where it starts");
    // An immediate operand isn't an address.
    assert_eq!(said("  *=$2000\n  lda #$d0\n  cmp #$20\n  rts\n"), None);
}

#[test]
fn source_can_say_which_machine_its_for() {
    use output::Micro6502::{self, Bbc, C64};
    assert_eq!(Micro6502::said_in(["; machine: bbc", "  *=$2000"]), Some(Bbc));
    assert_eq!(Micro6502::said_in(["  ; Machine: Commodore 64"]), Some(C64));
    assert_eq!(Micro6502::said_in(["// machine: c64"]), Some(C64));
    assert_eq!(Micro6502::said_in([";; machine:BBC Micro"]), Some(Bbc));
    assert_eq!(Micro6502::said_in(["*=$2000", "; machine: atari"]), None);
    assert_eq!(Micro6502::said_in(["  lda #1 ; machine: bbc"]), None, "only a line of its own");
}

#[test]
fn a_bbc_program_loads_where_it_was_put_and_starts_at_its_start() {
    let (load, exec, code) = output::bbc_program(&m6502("  *=$2000\n  !byte 0\nstart jsr $ffee\n  rts\n")).unwrap();
    assert_eq!((load, exec, code), (0x2000, 0x2000, vec![0, 0x20, 0xEE, 0xFF, 0x60]));
    assert!(output::bbc_program(&m6502("  *=$1100\n  rts\n")).unwrap_err().contains("&1900"));
    assert!(output::bbc_program(&m6502("  *=$7fff\n  nop\n  nop\n")).unwrap_err().contains("&7FFF"));
}

/// Every 6809 instruction, with every indexed mode, as lwasm assembles them.
#[test]
fn every_6809_instruction_is_assembled_as_lwasm_does() {
    let source = include_str!("testdata/m6809_all.asm");
    let assembly = assemble(&m6809::M6809, Path::new("test.asm"), source, &no_files);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).collect::<Vec<_>>());
    let expected = include_bytes!("testdata/m6809_all.bin");
    for placed in &assembly.lines {
        let at = (placed.address - 0x1000) as usize;
        assert_eq!(Some(&placed.bytes[..]), expected.get(at..at + placed.bytes.len()), "line {}: {}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""));
    }
    assert_eq!(assembly.image(0x1000, 0x1000 + expected.len() as i64), expected);
}

/// Every 68000 instruction in its addressing modes, as vasm assembles them with -no-opt.
#[test]
fn every_68000_instruction_is_assembled_as_vasm_does() {
    let source = include_str!("testdata/m68k_all.asm");
    let assembly = assemble(&m68k::M68k, Path::new("test.asm"), source, &no_files);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).collect::<Vec<_>>());
    let expected = include_bytes!("testdata/m68k_all.bin");
    for placed in &assembly.lines {
        let at = (placed.address - 0x1000) as usize;
        assert_eq!(Some(&placed.bytes[..]), expected.get(at..at + placed.bytes.len()), "line {}: {}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""));
    }
    assert_eq!(assembly.image(0x1000, 0x1000 + expected.len() as i64), expected);
}

/// The 68020's forms are refused, not quietly assembled as different 68000 instructions: MULU.L as MULU.W, or a
/// TST of a PC-relative address, which the 68000 can't do and an A500 crashes on.
#[test]
fn the_68020s_forms_are_refused_in_68000_code() {
    for wrong in ["mulu.l d0,d1", "muls.l d0,d1", "divu.l d0,d1", "divs.l #3,d1", "mulu.l #3,d1", "chk.l d0,d1", "tst.b x(pc)", "tst.w x(pc,d0.w)", "cmpi.w #1,x(pc)", "cmpi.b #1,x(pc,d1.w)", "clr.w x(pc)", "addq.w #1,x(pc)", "add.w d0,x(pc)", "bset #1,x(pc)", "st x(pc)", "lsl x(pc)"] {
        let source = format!("\t{wrong}\nx:\tdc.w 0\n");
        assert!(!assemble(&m68k::M68k, Path::new("t.s"), &source, &no_files).ok(), "{wrong} should be refused");
    }
    for right in ["mulu.w d0,d1", "mulu d0,d1", "divs.w #3,d1", "btst #1,x(pc)", "tst.b x", "cmpi.w #1,x", "move.w x(pc),d0", "add.w x(pc),d0", "cmp.w x(pc,d0.w),d1", "lsl.w #2,d0"] {
        let source = format!("\t{right}\nx:\tdc.w 0\n");
        let assembly = assemble(&m68k::M68k, Path::new("t.s"), &source, &no_files);
        assert!(assembly.ok(), "{right}: {:?}", assembly.problems);
    }
}

/// An Atari ST program, its addresses found by assembling it twice, as vasm writes it (-Ftos -nosym).
#[test]
fn an_atari_st_program_matches_vasm() {
    let source = include_str!("testdata/st_hello.s");
    let here = assemble(&m68k::M68k, Path::new("hello.s"), source, &no_files);
    assert!(here.ok() && !here.absolute, "{:?}", here.problems);
    let moved = assemble_at(&m68k::M68k, Path::new("hello.s"), source, &no_files, 0x10000);
    let ours = output::atari_program(&here, &moved, 0x10000).unwrap();
    assert_eq!(ours, include_bytes!("testdata/st_hello.tos"));
    // With ORG, it's at an address of its own.
    assert!(assemble(&m68k::M68k, Path::new("t.s"), "\torg $1000\n\tnop\n", &no_files).absolute);
}

/// A program written as ca65 has it (.proc, @ labels, := and : anonymous labels), as ca65 and ld65 make it.
#[test]
fn a_ca65_program_assembles_as_ca65_does() {
    let assembly = m6502(include_str!("testdata/ca65_prog.s"));
    assert!(assembly.ok(), "{:?}", assembly.problems);
    let expected = include_bytes!("testdata/ca65_prog.bin");
    assert_eq!(assembly.image(0xC000, 0xC000 + expected.len() as i64), expected);
}

#[test]
fn labels_are_known_where_defined_and_used() {
    let source = "  org 32768\nmain: ld b,8\n.loop: djnz .loop\n  jp main\ncount equ 3\n  ld a,count+count\n  macro border c\n  ld a,c\n  endm\n";
    let a = assemble(&crate::asm::z80::Z80, Path::new("t.asm"), source, &|_| Err(crate::i18n::tr("no").into()));
    assert!(a.ok(), "{:?}", a.problems);
    let def = |name: &str| a.definitions.iter().find(|d| d.name == name).map(|d| (d.line, d.written.as_str()));
    assert_eq!(def("main"), Some((1, "main")));
    assert_eq!(def("main.loop"), Some((2, ".loop")), "a local label is known in full, and written as it is");
    assert_eq!(def("count"), Some((4, "count")));
    let uses = |name: &str| a.uses.iter().filter(|u| u.name == name).map(|u| (u.line, u.written.as_str())).collect::<Vec<_>>();
    assert_eq!(uses("main.loop"), [(2, ".loop")]);
    assert_eq!(uses("main"), [(3, "main")]);
    assert_eq!(uses("count"), [(5, "count")], "used twice on one line, listed once");
    assert_eq!(a.macros.iter().map(|m| (m.name.as_str(), m.line)).collect::<Vec<_>>(), [("border", 6)]);
}

/// Real Next and Spectrum projects, assembled here and by sjasmplus: every file sjasmplus wrote (but its maps) has
/// to come out the same. The corpus and sjasmplus's files are made by tools outside the repository (see the 0.7
/// notes); without them this does nothing. Run with --ignored --nocapture to see each project.
#[test]
#[ignore]
fn real_projects_assemble_as_sjasmplus_assembles_them() {
    let results = std::path::Path::new("/root/nextcorpus-out/results.json");
    let Ok(text) = std::fs::read_to_string(results) else {
        return;
    };
    let results: serde_json::Value = serde_json::from_str(&text).unwrap();
    let read = |path: &Path| std::fs::read(path).map_err(|e| e.to_string());
    let (mut same, mut total) = (0, 0);
    let mut report = String::new();
    for result in results.as_array().unwrap() {
        if result["code"].as_i64() != Some(0) {
            continue;
        }
        let made: Vec<String> = result["made"].as_array().unwrap().iter().map(|m| m.as_str().unwrap().to_string()).filter(|m| !m.ends_with(".map") && !m.ends_with(".sld") && !m.ends_with(".lst") && !m.ends_with(".sym")).collect();
        if made.is_empty() {
            continue;
        }
        total += 1;
        let path = std::path::PathBuf::from(result["path"].as_str().unwrap());
        let n = result["n"].as_i64().unwrap();
        let source = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
        let assembly = assemble(&z80::Z80, &path, &source, &read);
        let short = path.strip_prefix("/root/nextcorpus").unwrap_or(&path).display().to_string();
        if !assembly.ok() {
            report.push_str(&format!("PROBLEMS {short}: {} problem(s)\n", assembly.problems.len()));
            let mut seen = std::collections::HashSet::new();
            for problem in assembly.problems.iter().filter(|p| seen.insert(p.message.split_whitespace().next().unwrap_or("").to_string())).take(6) {
                let file = assembly.files[problem.file].file_name().unwrap_or_default().to_string_lossy();
                report.push_str(&format!("    {file}:{} {}\n", problem.line + 1, problem.message));
            }
            continue;
        }
        let mut all = true;
        for name in &made {
            let theirs = std::fs::read(format!("/root/nextcorpus-out/{n}/{}", name.replace("../", "__/"))).unwrap();
            let ours = assembly.outputs.iter().find(|o| Path::new(&o.name).file_name() == Path::new(name).file_name());
            match ours {
                None => {
                    all = false;
                    report.push_str(&format!("MISSING {short}: {name} (we made {:?})\n", assembly.outputs.iter().map(|o| &o.name).collect::<Vec<_>>()));
                }
                Some(ours) if ours.bytes != theirs => {
                    all = false;
                    let at = ours.bytes.iter().zip(&theirs).position(|(a, b)| a != b).unwrap_or(ours.bytes.len().min(theirs.len()));
                    report.push_str(&format!("DIFFERS {short}: {name}: {} vs {} bytes, first at {at:#X}\n", ours.bytes.len(), theirs.len()));
                }
                Some(_) => {}
            }
        }
        if all {
            same += 1;
        }
    }
    println!("{report}{same} of {total} projects match sjasmplus");
}

/// sjasmplus's own test of every fake instruction (tests/z80/all_fake.asm), checked against the binary it makes.
#[test]
fn sjasmplus_fake_instructions_assemble_as_it_does() {
    let source = include_str!("testdata/all_fake.asm");
    let expected = include_bytes!("testdata/all_fake.bin");
    // Its last four lines check errors without --zxnext=cspect; they're left out here.
    let source: String = source.lines().take_while(|l| !l.contains("OPT --zxnext ")).collect::<Vec<_>>().join("\n");
    let read = |path: &Path| std::fs::read(path).map_err(|e| e.to_string());
    let assembly = assemble(&z80::Z80, Path::new("all_fake.asm"), &source, &read);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).collect::<Vec<_>>());
    let ours = assembly.image(0x8000, assembly.highest().unwrap());
    let theirs = &expected[..ours.len()];
    if let Some(at) = ours.iter().zip(theirs).position(|(a, b)| a != b) {
        let line = assembly.line_at(0x8000 + at as i64).map(|(_, l)| l + 1).unwrap_or(0);
        panic!("differs at ${:04X}, line {line}: {}", 0x8000 + at, source.lines().nth(line.saturating_sub(1)).unwrap_or(""));
    }
}

/// A small types.i of the Amiga's kind, written for the test: macros using \1 and \2, \@, IFC, MEXIT and SET.
const TYPES_I: &str = "\tIFND EXEC_TYPES_I\nEXEC_TYPES_I SET 1\n* Structure building\nSTRUCTURE   MACRO\t\t; structure name, initial offset\n\\1\t    EQU     0\nSOFFSET     SET     \\2\n\t    ENDM\nAPTR\t    MACRO\n\\1\t    EQU     SOFFSET\nSOFFSET     SET     SOFFSET+4\n\t    ENDM\nUBYTE\t    MACRO\n\\1\t    EQU     SOFFSET\nSOFFSET     SET     SOFFSET+1\n\t    ENDM\nBYTE\t    MACRO\n\\1\t    EQU     SOFFSET\nSOFFSET     SET     SOFFSET+1\n\t    ENDM\nLABEL\t    MACRO\n\\1\t    EQU     SOFFSET\n\t    ENDM\nSTRUCT\t    MACRO\n\\1\t    EQU     SOFFSET\nSOFFSET     SET     SOFFSET+\\2\n\t    ENDM\nBITDEF\t    MACRO   ; prefix,&name,&bitnum\n\t    BITDEF0 \\1,\\2,B_,\\3\n\\@BITDEF    SET     1<<\\3\n\t    BITDEF0 \\1,\\2,F_,\\@BITDEF\n\t    ENDM\nBITDEF0     MACRO   ; prefix,&name,&type,&value\n\\1\\3\\2\t    EQU     \\4\n\t    ENDM\n\tENDC\t; EXEC_TYPES_I\n";
const NODES_I: &str = "\tIFND EXEC_NODES_I\nEXEC_NODES_I\tSET 1\n    IFND EXEC_TYPES_I\n    INCLUDE \"ndk/exec/types.i\"\n    ENDC ; EXEC_TYPES_I\n   STRUCTURE\tLN,0\t; List Node\n\tAPTR\tLN_SUCC\t; next\n\tAPTR\tLN_PRED\n\tUBYTE\tLN_TYPE\n\tBYTE\tLN_PRI\n\tAPTR\tLN_NAME\n\tLABEL\tLN_SIZE\t; Note: word aligned\n\tSTRUCT\tLN_PAD,<(2+6)/4>\nNT_TASK\tEQU 1\t* Exec task\n\tBITDEF\tMEM,CHIP,1\n\tBITDEF\tMEM,FAST,2\nMEMF_BOTH EQU MEMF_CHIP!MEMF_FAST\n\tENDC\n";

fn amiga_headers(path: &Path) -> Result<Vec<u8>, String> {
    let name = path.to_string_lossy().replace('\\', "/");
    if name.ends_with("ndk/exec/types.i") {
        Ok(TYPES_I.as_bytes().to_vec())
    } else if name.ends_with("ndk/exec/nodes.i") {
        Ok(NODES_I.as_bytes().to_vec())
    } else {
        no_files(path)
    }
}

#[test]
fn motorola_macros_build_the_amiga_s_structures() {
    let source = "\tinclude \"ndk/exec/nodes.i\"\n\tinclude \"ndk/exec/nodes.i\"\n\tmove.l\tLN_NAME(a1),a0\n\tdc.w\tLN_SIZE,LN_PAD,NT_TASK,MEMB_CHIP,MEMF_CHIP,MEMF_BOTH\n";
    let assembly = assemble(&m68k::M68k, Path::new("t.s"), source, &amiga_headers);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.chunks.iter().flat_map(|c| c.bytes.clone()).collect::<Vec<u8>>(), [0x20, 0x69, 0x00, 0x0A, 0, 14, 0, 14, 0, 1, 0, 1, 0, 2, 0, 6]);
}

#[test]
fn mexit_ifc_and_angle_brackets_work_in_motorola_macros() {
    let source = "fetch MACRO\n\tIFC '\\1',''\n\tmoveq #1,d0\n\tMEXIT\n\tENDC\n\tmoveq #\\1,d0\n\tENDM\n\tfetch\n\tfetch <2+1>\n\tIFGT 3-2\n\tmoveq #4,d0\n\tENDC\n\tIFLE 3-2\n\tmoveq #5,d0\n\tENDC\n\tIFND nothing\n\tnop\n\tENDC\n";
    let assembly = assemble(&m68k::M68k, Path::new("t.s"), source, &no_files);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.chunks.iter().flat_map(|c| c.bytes.clone()).collect::<Vec<u8>>(), [0x70, 1, 0x70, 3, 0x70, 4, 0x4E, 0x71]);
}

#[test]
fn values_given_from_outside_are_used_only_when_the_source_has_none() {
    let lvo = |name: &str| match name {
        "_LVOAllocMem" => Some(-198),
        "_LVOFreeMem" => Some(-210),
        _ => None,
    };
    let options = Options { folders: Vec::new(), values: Some(&lvo), ..Default::default() };
    let source = "\tjsr _LVOAllocMem(a6)\n\tjsr _LVOFreeMem(a6)\n_LVOFreeMem equ -6\n";
    let assembly = assemble_with(&m68k::M68k, Path::new("t.s"), source, &no_files, 0, &options);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.chunks.iter().flat_map(|c| c.bytes.clone()).collect::<Vec<u8>>(), [0x4E, 0xAE, 0xFF, 0x3A, 0x4E, 0xAE, 0xFF, 0xFA], "the source's own wins");
    assert!(!assemble(&m68k::M68k, Path::new("t.s"), source, &no_files).ok(), "without them, it isn't defined");
}

#[test]
fn include_looks_in_the_folders_given_after_the_source_s_own() {
    let folder = std::env::temp_dir().join(format!("limoniter-include-{}", std::process::id()));
    std::fs::create_dir_all(folder.join("exec")).unwrap();
    std::fs::write(folder.join("exec/answer.i"), "ANSWER equ 42\n").unwrap();
    let read = |p: &Path| std::fs::read(p).map_err(|e| e.to_string());
    let options = Options { folders: vec![folder.clone()], values: None, ..Default::default() };
    let assembly = assemble_with(&m68k::M68k, Path::new("/nowhere/t.s"), "\tinclude \"exec/answer.i\"\n\tdc.b ANSWER\n", &read, 0, &options);
    let _ = std::fs::remove_dir_all(&folder);
    assert!(assembly.ok(), "{:?}", assembly.problems);
    assert_eq!(assembly.chunks.iter().flat_map(|c| c.bytes.clone()).collect::<Vec<u8>>(), [42]);
}

/// ACME's !align: on until the address ANDed with the first value is the second, with NOPs unless told otherwise
/// (as ACME 0.97 assembled it).
#[test]
fn acme_aligns_by_and_and_equal() {
    let a = m6502("*=$1001\n !byte 1\n !align 3, 0\n !byte 2\n !align 7, 5\n !byte 3\n !align 15, 0, $11\n !byte 4\n");
    assert!(a.ok(), "{:?}", a.problems);
    let mut want = vec![1, 0xEA, 0xEA, 2, 3];
    want.extend([0x11; 10]);
    want.push(4);
    assert_eq!(a.image(0x1001, 0x1011), want);
}

/// Every 68010, 68020 and 68030 integer instruction in its addressing modes (and what the CPU32 has of them), as
/// vasm 1.9 assembles them with -no-opt, choosing the processor with MACHINE as the source goes.
#[test]
fn every_68020_instruction_is_assembled_as_vasm_does() {
    let source = include_str!("testdata/m68k_020_all.asm");
    let assembly = assemble(&m68k::M68k, Path::new("test.asm"), source, &no_files);
    assert!(assembly.ok(), "{:?}", assembly.problems.iter().take(5).map(|p| (p.line + 1, &p.message)).collect::<Vec<_>>());
    let expected = include_bytes!("testdata/m68k_020_all.bin");
    for placed in &assembly.lines {
        let at = (placed.address - 0x1000) as usize;
        assert_eq!(Some(&placed.bytes[..]), expected.get(at..at + placed.bytes.len()), "line {}: {}", placed.line + 1, source.lines().nth(placed.line).unwrap_or(""));
    }
    assert_eq!(assembly.image(0x1000, 0x1000 + expected.len() as i64), expected);
}

/// Each instruction and addressing mode is refused on a processor that hasn't got it, naming the one that has.
#[test]
fn each_processor_refuses_what_it_has_not_got_naming_the_one_that_has() {
    let try_on = |machine: &str, line: &str| {
        let source = format!("\t{machine}\n\torg $1000\nx:\t{line}\n");
        let assembly = assemble(&m68k::M68k, Path::new("t.s"), &source, &no_files);
        assembly.problems.first().map(|p| p.message.clone())
    };
    for (machine, line, needs) in [
        ("mc68000", "movec vbr,d0", "68010"),
        ("machine 68008", "rtd #4", "68010"),
        ("machine 68000", "move.w ccr,d0", "68010"),
        ("machine 68000", "bkpt #1", "68010"),
        ("machine 68000", "moves.l (a0),d0", "68010"),
        ("machine 68010", "bfextu d0{1:2},d1", "68020"),
        ("machine 68010", "move.l ([a0]),d0", "68020"),
        ("machine 68010", "move.l (a0,d1.w*4),d0", "68020"),
        ("machine 68010", "extb.l d0", "68020"),
        ("machine 68010", "bra.l x", "68020"),
        ("machine 68000", "mulu.l d0,d1", "68020"),
        ("machine 68010", "divsl.l d0,d1:d2", "68020"),
        ("machine 68010", "link.l a6,#-8", "68020"),
        ("machine 68010", "trapeq", "68020"),
        ("machine 68010", "cas.w d0,d1,(a0)", "68020"),
        ("machine 68010", "tst.w a0", "68020"),
        ("machine 68010", "cmpi.w #1,x(pc)", "68020"),
        ("machine 68010", "movec cacr,d0", "68020"),
        ("machine cpu32", "bfextu d0{1:2},d1", "CPU32"),
        ("machine cpu32", "move.l ([a0]),d0", "CPU32"),
        ("machine cpu32", "cas2.w d0:d1,d2:d3,(a0):(a1)", "CPU32"),
        ("machine cpu32", "chk.l d0,d1", "CPU32"),
        ("machine 68030", "callm #1,(a0)", "68020"),
        ("machine 68040", "rtm d0", "68020"),
        ("machine 68040", "movec caar,d0", "68020"),
        ("machine 68060", "movec msp,d0", "68040"),
    ] {
        let message = try_on(machine, line).unwrap_or_else(|| panic!("{line} should be refused after {machine}"));
        assert!(message.contains(needs), "{line} after {machine}: {message}");
    }
    for (machine, line) in [
        ("machine 68010", "movec vbr,d0"),
        ("machine cpu32", "move.l (8,a0,d1.l*4),d0"),
        ("machine cpu32", "chk2.w (a0),d1"),
        ("machine 68040", "move.l ([a0]),d0"),
        ("machine 68060", "bfextu d0{1:2},d1"),
        ("opt p=68030", "cas.l d0,d1,(a0)"),
        ("mc68020", "callm #1,(a0)"),
        ("machine mc68010", "rtd #4"),
    ] {
        assert_eq!(try_on(machine, line), None, "{line} after {machine}");
    }
    // An unknown processor is a problem, and so is a 68000's TST.B of an address register on any processor.
    assert!(try_on("machine 68021", "nop").is_some_and(|m| m.contains("MACHINE takes")));
    assert!(try_on("machine 68020", "tst.b a0").is_some());
    // JMP and JSR aren't widened to the full extension word, as vasm doesn't widen them.
    assert!(try_on("machine 68020", "jmp x+$10000(pc)").is_some());
}

/// The processor chosen in the settings comes in through the options, and MACHINE in the source wins over it.
#[test]
fn the_settings_processor_is_used_until_the_source_chooses() {
    let options = Options { settings: vec![(m68k::SETTING, 68020)], ..Default::default() };
    let on = |source: &str| assemble_with(&m68k::M68k, Path::new("t.s"), source, &no_files, 0, &options);
    assert!(on("\textb.l d0\n").ok());
    assert!(!on("\tmachine 68000\n\textb.l d0\n").ok());
    let assembly = on("\tnop\n\tmachine 68008\n\tnop\n");
    assert_eq!(assembly.settings, [(0, m68k::SETTING, 68020), (1, m68k::SETTING, 68008)]);
}
