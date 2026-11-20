use super::*;
use crate::asm::m6809::M6809;

/// lwasm's table of cycles, an independent count to check this one against (see the file's head).
const LWASM: &str = include_str!("../asm/testdata/m6809_cycles.txt");

fn assemble(source: &str) -> Assembly {
    let assembly = crate::asm::assemble(&M6809, std::path::Path::new("t.asm"), source, &|_| Err("no".into()));
    assert!(assembly.problems.is_empty(), "{source}: {:?}", assembly.problems);
    assembly
}

/// One instruction's bytes, assembled at $3000.
fn bytes(source: &str) -> Vec<u8> {
    assemble(&format!("\torg $3000\n\t{source}\n")).lines.iter().flat_map(|l| l.bytes.clone()).collect()
}

#[test]
fn every_opcode_takes_what_lwasm_counts() {
    let mut checked = 0;
    for line in LWASM.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let code = u16::from_str_radix(fields[0], 16).unwrap();
        let want: (u8, u8) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
        let Ok(at) = BASE.binary_search_by_key(&code, |e| e.0) else {
            panic!("${code:04X} is in lwasm's table and not here");
        };
        let have = (BASE[at].1, BASE[at].2);
        match code {
            // SYNC: lwasm gives 2 and 1 as an estimate; Motorola's manual and Atkinson give at least 4 (3 native),
            // the least before an interrupt can end it.
            0x13 => assert_eq!(have, (4, 3)),
            // RTI: lwasm gives native mode's most; here the base is the least (after a FIRQ), the most added apart.
            0x3B => assert_eq!(have, (6, 6)),
            _ => assert_eq!(have, want, "${code:04X} {}", fields.get(3).unwrap_or(&"")),
        }
        checked += 1;
    }
    // lwasm's table leaves out LDB direct, which Motorola's map gives as 4 (Atkinson: 3 native).
    assert_eq!(BASE[BASE.binary_search_by_key(&0xD6, |e| e.0).unwrap()], (0xD6, 4, 3));
    assert_eq!(checked + 1, BASE.len(), "every opcode here is in lwasm's table but LDB direct");
}

#[test]
fn every_instruction_the_disassembler_knows_has_its_cycles() {
    // Operand bytes of $12 make a valid postbyte (a 5-bit offset), register pair (X,Y) and stack mask for each.
    let decodes = |code: u16| {
        let mut bytes = if code > 0xFF { code.to_be_bytes().to_vec() } else { vec![code as u8] };
        bytes.extend([0x12; 5]);
        let d = crate::disasm::m6809(&|i| bytes.get(i).copied().unwrap_or(0), 0x3000, true);
        !d.text.starts_with("fcb")
    };
    for page in [0u16, 0x1000, 0x1100] {
        for op in 0..=0xFFu16 {
            let code = page | op;
            if page == 0 && matches!(op, 0x10 | 0x11) {
                continue;
            }
            let known = BASE.binary_search_by_key(&code, |e| e.0).is_ok();
            assert_eq!(decodes(code), known, "${code:04X}");
        }
    }
}

#[test]
fn indexed_modes_add_what_the_tables_say() {
    use Mode::{Emulation as E, Native as N};
    // Motorola's table F-2 (the 6809, and a 6309 in emulation mode), then Atkinson's for native mode and the 6309's
    // own modes: postbyte (with X), cycles added in emulation mode, in native mode.
    let table: &[(&str, u8, u32, u32)] = &[
        (",x", 0x84, 0, 0),
        ("5-bit n,x", 0x05, 1, 1),
        ("8-bit n,x", 0x88, 1, 1),
        ("16-bit n,x", 0x89, 4, 3),
        ("a,x", 0x86, 1, 1),
        ("b,x", 0x85, 1, 1),
        ("d,x", 0x8B, 4, 2),
        (",x+", 0x80, 2, 1),
        (",x++", 0x81, 3, 2),
        (",-x", 0x82, 2, 1),
        (",--x", 0x83, 3, 2),
        ("8-bit n,pcr", 0x8C, 1, 1),
        ("16-bit n,pcr", 0x8D, 5, 3),
        ("[,x]", 0x94, 3, 3),
        ("[8-bit n,x]", 0x98, 4, 4),
        ("[16-bit n,x]", 0x99, 7, 6),
        ("[a,x]", 0x96, 4, 4),
        ("[d,x]", 0x9B, 7, 5),
        ("[,x++]", 0x91, 6, 5),
        ("[,--x]", 0x93, 6, 5),
        ("[8-bit n,pcr]", 0x9C, 4, 4),
        ("[16-bit n,pcr]", 0x9D, 8, 6),
        ("[address]", 0x9F, 5, 4),
        ("e,x", 0x87, 1, 1),
        ("f,x", 0x8A, 1, 1),
        ("w,x", 0x8E, 1, 1),
        ("[w,x]", 0x9E, 4, 4),
        (",w", 0x8F, 0, 0),
        ("n,w", 0xAF, 2, 2),
        (",w++", 0xCF, 1, 1),
        (",--w", 0xEF, 1, 1),
        ("[,w]", 0x90, 3, 3),
        ("[n,w]", 0xB0, 5, 5),
        ("[,w++]", 0xD0, 4, 4),
        ("[,--w]", 0xF0, 4, 4),
    ];
    for (form, post, emulation, native) in table {
        assert_eq!(indexed_extra(*post, E), Some(*emulation), "{form}");
        assert_eq!(indexed_extra(*post, N), Some(*native), "{form}");
        // The same from Y, U and S.
        if post & 0x80 != 0 && !matches!(post, 0x8F | 0xAF | 0xCF | 0xEF | 0x90 | 0xB0 | 0xD0 | 0xF0 | 0x9F) {
            for r in 1..4u8 {
                assert_eq!(indexed_extra(post | r << 5, E), Some(*emulation), "{form} from register {r}");
            }
        }
    }
    // [,X+] and [,-X] can't be indirect, and $BF means nothing.
    assert_eq!(indexed_extra(0xB2, E), None);
    assert_eq!(indexed_extra(0xBF, N), None);
}

/// Instructions as written, with their cycles on a 6809 (or a 6309 in emulation mode) and in native mode, from
/// Motorola's MC6809-MC6809E Programming Manual (appendix F) for the 6809's and Atkinson's 6809 and 6309
/// Programmer's Reference for the 6309's and for native mode: base cycles, plus the indexed extra.
#[test]
fn instructions_take_what_the_manuals_give() {
    let cases: &[(&str, &str, &str)] = &[
        ("lda #1", "2", "2"),
        ("lda <$20", "4", "3"),
        ("lda $1234", "5", "4"),
        ("lda ,x", "4", "4"),
        ("lda 5,x", "5", "5"),
        ("lda 100,y", "5", "5"),
        ("lda 1000,u", "8", "7"),
        ("lda d,s", "8", "6"),
        ("lda ,x+", "6", "5"),
        ("lda ,x++", "7", "6"),
        ("lda [,x]", "7", "7"),
        ("lda [,x++]", "10", "9"),
        ("lda [$1234]", "9", "8"),
        ("leax 1,x", "5", "5"),
        ("leax 1000,x", "8", "7"),
        ("ldd #1", "3", "3"),
        ("ldd <$20", "5", "4"),
        ("std $1234", "6", "5"),
        ("ldy #1", "4", "4"),
        ("ldx ,y", "5", "5"),
        ("cmpx #1", "4", "3"),
        ("cmpy $1234", "8", "6"),
        ("addd <$20", "6", "4"),
        ("jsr $1234", "8", "7"),
        ("jsr ,x", "7", "6"),
        ("jmp <$20", "3", "2"),
        ("inc <$20", "6", "5"),
        ("inc $1234", "7", "6"),
        ("tst <$20", "6", "4"),
        ("clra", "2", "1"),
        ("nop", "2", "1"),
        ("mul", "11", "10"),
        ("abx", "3", "1"),
        ("rts", "5", "4"),
        ("exg a,b", "8", "5"),
        ("tfr x,y", "6", "4"),
        ("orcc #$50", "3", "2"),
        ("swi", "19", "21"),
        ("swi2", "20", "22"),
        ("bra *", "3", "3"),
        ("bne *", "3", "3"),
        ("bsr *", "7", "6"),
        ("lbra *", "5", "4"),
        ("lbsr *", "9", "7"),
        // A conditional long branch: 6 when taken on a 6809, 5 either way in native mode. LBRN never is.
        ("lbne *", "5/6", "5"),
        ("lbrn *", "5", "5"),
        // One more for each byte pushed or pulled.
        ("pshs a,b,x", "9", "8"),
        ("puls cc,a,b,dp,x,y,u,pc", "17", "16"),
        ("rti", "6/15", "6/17"),
        ("sync", "\u{2265}4", "\u{2265}3"),
        ("cwai #$ef", "\u{2265}22", "\u{2265}20"),
        // The 6309's own.
        ("ldq #1", "5", "5"),
        ("ldq <$20", "8", "7"),
        ("ldq $1234", "9", "8"),
        ("ldq ,x", "8", "8"),
        ("ldw #1", "4", "4"),
        ("lde #1", "3", "3"),
        ("stf $1234", "6", "5"),
        ("addr a,b", "4", "4"),
        ("tfm x+,y+", "6+3n", "6+3n"),
        ("tfm x,y+", "6+3n", "6+3n"),
        ("divd #3", "25", "25"),
        ("divd <$20", "27", "26"),
        ("divq #1", "34", "34"),
        ("divq $1234", "37", "36"),
        ("muld #2", "28/30", "28/30"),
        ("muld <$20", "30/32", "29/31"),
        ("sexw", "4", "4"),
        ("pshsw", "6", "6"),
        ("incd", "3", "2"),
        ("clrw", "3", "2"),
        ("aim #1,<$20", "6", "6"),
        ("aim #1,2,x", "8", "8"),
        ("tim #1,>$1234", "7", "7"),
        ("band a,0,7,$40", "7", "6"),
        ("stbt cc,3,1,$80", "8", "7"),
        ("ldmd #1", "5", "5"),
        ("bitmd #1", "4", "4"),
        ("lda ,w", "4", "4"),
        ("lda 300,w", "6", "6"),
        ("ldd [,w++]", "9", "9"),
        ("lda e,x", "5", "5"),
        ("lda [w,x]", "8", "8"),
    ];
    for (source, emulation, native) in cases {
        let made = bytes(source);
        assert!(is_instruction(&made), "{source}: {made:02X?}");
        assert_eq!(cycles(&made, Mode::Emulation).map(Timing::text).as_deref(), Some(*emulation), "{source} on a 6809");
        assert_eq!(cycles(&made, Mode::Native).map(Timing::text).as_deref(), Some(*native), "{source} in native mode");
    }
    // The instruction reference shows both.
    assert_eq!(both(&bytes("lda <$20")).as_deref(), Some("4 (3 native)"));
    assert_eq!(both(&bytes("lda #1")).as_deref(), Some("2"));
    // Bytes that aren't one instruction aren't timed.
    assert!(!is_instruction(&[0x86]));
    assert!(!is_instruction(&[0x12, 0x12]));
    assert!(!is_instruction(&[0x01]), "the 6309's OIM needs its value and address");
    assert_eq!(cycles(&[0x15], Mode::Emulation), None, "$15 is nothing on either");
}

#[test]
fn the_mode_follows_pragma_and_ldmd() {
    let a = assemble("\torg $3000\n\tlda <$20\n\tpragma 6309\n\tlda <$20\n\tpragma 6809\n\tlda <$20\n\tpragma 6309\n\tldmd #0\n\tlda <$20\n\tldmd #1\n\tlda <$20\n\tldmd #3\n\tlda <$20\n");
    let modes = modes(&a);
    let at = |line: usize| a.lines.iter().position(|p| p.line == line).map(|i| modes[i]).unwrap();
    use Mode::{Emulation as E, Native as N};
    // Nothing said: a 6809. PRAGMA 6309: native, as lwasm counts it. PRAGMA 6809: a 6809 again.
    assert_eq!((at(1), at(3), at(5)), (E, N, E));
    // LDMD itself runs in the mode before it; what follows in the one it sets (bit 0; bit 1 is the FIRQ mode).
    assert_eq!((at(7), at(8), at(9), at(10), at(12)), (N, E, E, N, N));
    let (_, mode, timing) = line(&a, 0, 10).unwrap();
    assert_eq!((mode, timing.text()), (N, "3".to_string()));
    assert_eq!(words(timing, mode), "3 cycles in native mode");
    let (_, mode, timing) = line(&a, 0, 1).unwrap();
    assert_eq!(words(timing, mode), "4 cycles");
    assert!(more(&bytes("lda <$20"), E).contains("3 in a 6309's native mode"));
    assert!(more(&bytes("lbne *"), E).contains("one more when taken"));
    assert!(more(&bytes("tfm x+,y+"), N).contains("3 more for each byte"));
    // A table of bytes isn't timed, even one that starts like an instruction.
    let data = assemble("\torg $3000\n\tfcb $86,1,2\n");
    assert!(line(&data, 0, 1).is_none());
}

#[test]
fn the_option_shows_each_lines_cycles_in_its_mode() {
    let source = "\torg $3000\n\tldx #$400\n\tldmd #1\nloop\tsta ,x+\n\tlbne loop\n\tfcb 1,2\n\trts\n";
    let a = assemble(source);
    let intel = crate::code_intel::CodeIntel { assembly: Some(std::sync::Arc::new(a)), version: 7, cpu: Some(crate::assemblers::Cpu::M6809), ..Default::default() };
    let was = crate::m6502_timing::shown();
    crate::m6502_timing::show(true);
    let notes = crate::m6502_timing::line_notes(&intel, 7, 0, 10);
    // LDX # and LDMD in the 6809's timing; after LDMD #1, native mode's.
    assert_eq!(notes.get(&1).map(String::as_str), Some("3"));
    assert_eq!(notes.get(&2).map(String::as_str), Some("5"));
    assert_eq!(notes.get(&3).map(String::as_str), Some("5"));
    assert_eq!(notes.get(&4).map(String::as_str), Some("5"));
    assert_eq!(notes.get(&5), None, "data isn't timed");
    assert_eq!(notes.get(&6).map(String::as_str), Some("4"));
    // Not while the text has changed since, nor with the option off.
    assert!(crate::m6502_timing::line_notes(&intel, 8, 0, 10).is_empty());
    crate::m6502_timing::show(false);
    assert!(crate::m6502_timing::line_notes(&intel, 7, 0, 10).is_empty());
    crate::m6502_timing::show(was);
}

#[test]
fn a_block_adds_up_in_each_lines_mode() {
    let a = assemble("\torg $3000\n\tlda #1\n\tldmd #1\n\tlda <$20\n\ttfm x+,y+\n\tlbne *\n");
    // LDA # 2, LDMD 5, then in native mode LDA direct 3, TFM 6+3n, LBNE 5.
    let (sum, count, native) = block(&a, 0, 1, 5);
    assert_eq!((count, native), (5, false));
    assert_eq!(sum.range(), "21+3n");
    let (sum, count, native) = block(&a, 0, 3, 3);
    assert_eq!((sum.range(), count, native), ("3".to_string(), 1, true));
    let a = assemble("\torg $3000\n\tlbne *\n\tlbeq *\n");
    assert_eq!(block(&a, 0, 1, 2).0.range(), "10\u{2013}12");
}

#[test]
fn pointing_at_an_instruction_says_its_cycles_in_its_mode() {
    let a = assemble("\torg $3000\n\tpragma 6309\n\tlda <$20\n");
    let intel = crate::code_intel::CodeIntel { assembly: Some(std::sync::Arc::new(a)), cpu: Some(crate::assemblers::Cpu::M6809), words: std::sync::Arc::new(vec!["lda".into()]), ..Default::default() };
    let said = intel.describe(2, "\tlda <$20", 2).unwrap();
    assert!(said.starts_with("$3000: 96 20 (2 bytes, 3 cycles in native mode)"), "{said}");
    assert!(said.contains("4 on a 6809, or a 6309 in emulation mode"), "{said}");
}
