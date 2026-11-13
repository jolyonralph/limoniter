use super::*;
use std::path::Path;

fn no_files(path: &Path) -> Result<Vec<u8>, String> {
    Err(format!("{} isn't there", path.display()))
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
    assert!(lines.iter().any(|(l, m)| *l == 6 && m.contains("doesn't fit in a byte")), "{lines:?}");
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
            _ => Err("no".into()),
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
