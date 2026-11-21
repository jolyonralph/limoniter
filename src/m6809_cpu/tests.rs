use super::*;
use crate::asm::m6809::M6809;

/// 64K of RAM, the program in it from Limoniter's own assembler.
struct Ram(Vec<u8>);

impl Bus for Ram {
    fn read(&mut self, address: u16) -> u8 {
        self.0[usize::from(address)]
    }

    fn write(&mut self, address: u16, value: u8) {
        self.0[usize::from(address)] = value;
    }
}

fn assemble(source: &str) -> crate::asm::Assembly {
    let assembly = crate::asm::assemble(&M6809, std::path::Path::new("t.asm"), source, &|_| Err("no".into()));
    assert!(assembly.problems.is_empty(), "{source}: {:?}", assembly.problems);
    assembly
}

/// Assembles source at $1000 (with `done bra done` at its end) and runs it until it gets there.
fn run(body: &str) -> (Cpu, Ram) {
    let source = format!("\torg $1000\n{body}\ndone\tbra done\n");
    let assembly = assemble(&source);
    let mut ram = Ram(assembly.image(0, 0x1_0000));
    ram.0[0xFFFE] = 0x10;
    ram.0[0xFFFF] = 0x00;
    let done = assembly.symbols["done"] as u16;
    let mut cpu = Cpu::default();
    cpu.reset(&mut ram);
    cpu.s = 0x8000;
    cpu.u = 0x7000;
    for _ in 0..100_000 {
        if cpu.pc == done {
            return (cpu, ram);
        }
        cpu.step(&mut ram, Lines::default());
        assert!(cpu.illegal.is_none(), "{source}: illegal at {:04X}", cpu.pc);
    }
    panic!("{source}: never got to the end");
}

/// H, I, N, Z, V and C, as letters.
fn flags(cpu: &Cpu) -> String {
    cpu.flags()[2..].to_string()
}

#[test]
fn eight_bit_arithmetic_sets_the_flags_motorola_gives() {
    // (instruction, A before, the operand, A after, HINZVC after, I left as the reset set it)
    let cases = [
        ("adda", 0x7F, 0x01, 0x80, "HIN-V-"),
        ("adda", 0xFF, 0x01, 0x00, "HI-Z-C"),
        ("adda", 0x12, 0x34, 0x46, "-I----"),
        ("suba", 0x00, 0x01, 0xFF, "-IN--C"),
        ("suba", 0x80, 0x01, 0x7F, "-I--V-"),
        ("cmpa", 0x05, 0x05, 0x05, "-I-Z--"),
        ("cmpa", 0x05, 0x06, 0x05, "-IN--C"),
        ("anda", 0xF0, 0x0F, 0x00, "-I-Z--"),
        ("ora", 0x80, 0x01, 0x81, "-IN---"),
        ("eora", 0xFF, 0x0F, 0xF0, "-IN---"),
        ("bita", 0x80, 0x80, 0x80, "-IN---"),
    ];
    for (op, a, value, after, want) in cases {
        let (cpu, _) = run(&format!("\tandcc #$D0\n\tlda #${a:02X}\n\t{op} #${value:02X}"));
        assert_eq!(cpu.a, after, "{op} {a:02X},{value:02X}");
        assert_eq!(flags(&cpu), want, "{op} {a:02X},{value:02X}");
    }
    // With the carry: ADCA and SBCA.
    let (cpu, _) = run("\tlda #1\n\torcc #1\n\tadca #1");
    assert_eq!(cpu.a, 3);
    let (cpu, _) = run("\tlda #5\n\torcc #1\n\tsbca #2");
    assert_eq!(cpu.a, 2);
}

#[test]
fn memory_instructions_shift_rotate_and_count() {
    // (instruction, A before, A after, NZVC after)
    let cases = [
        ("nega", 0x01, 0xFF, "N--C"),
        ("nega", 0x80, 0x80, "N-VC"),
        ("nega", 0x00, 0x00, "-Z--"),
        ("coma", 0x0F, 0xF0, "N--C"),
        ("lsra", 0x81, 0x40, "---C"),
        ("rora", 0x01, 0x00, "-Z-C"),
        ("asra", 0x81, 0xC0, "N--C"),
        ("asla", 0x40, 0x80, "N-V-"),
        ("asla", 0x80, 0x00, "-ZVC"),
        ("rola", 0x80, 0x00, "-ZVC"),
        ("deca", 0x80, 0x7F, "--V-"),
        ("deca", 0x01, 0x00, "-Z--"),
        ("inca", 0x7F, 0x80, "N-V-"),
        ("inca", 0xFF, 0x00, "-Z--"),
        ("tsta", 0x80, 0x80, "N---"),
        ("clra", 0x55, 0x00, "-Z--"),
    ];
    for (op, a, after, want) in cases {
        let (cpu, _) = run(&format!("\tandcc #$F0\n\tlda #${a:02X}\n\t{op}"));
        assert_eq!(cpu.a, after, "{op} {a:02X}");
        assert_eq!(&flags(&cpu)[2..], want, "{op} {a:02X}");
    }
    // ROL and ROR take the carry in.
    let (cpu, _) = run("\tlda #$00\n\torcc #1\n\trola");
    assert_eq!(cpu.a, 1);
    let (cpu, _) = run("\tlda #$00\n\torcc #1\n\trora");
    assert_eq!(cpu.a, 0x80);
    // On memory, direct and extended: INC, DEC, NEG, COM and CLR in place.
    let (_, ram) = run("\tlda #$10\n\tsta $2000\n\tinc $2000\n\tinc $2000\n\tdec $2000\n\tlda #$01\n\tsta <$40\n\tneg <$40\n\tlda #$AA\n\tsta $2001\n\tcom $2001\n\tclr $2002");
    assert_eq!((ram.0[0x2000], ram.0[0x40], ram.0[0x2001], ram.0[0x2002]), (0x11, 0xFF, 0x55, 0x00));
}

#[test]
fn sixteen_bit_arithmetic_and_the_other_inherent_instructions() {
    let (cpu, _) = run("\tandcc #0\n\tldd #$7FFF\n\taddd #1");
    assert_eq!((cpu.d(), &flags(&cpu)[2..]), (0x8000, "N-V-"));
    let (cpu, _) = run("\tandcc #0\n\tldd #$0000\n\tsubd #1");
    assert_eq!((cpu.d(), &flags(&cpu)[2..]), (0xFFFF, "N--C"));
    let (cpu, _) = run("\tandcc #0\n\tldx #$1234\n\tcmpx #$1234");
    assert_eq!(&flags(&cpu)[2..], "-Z--");
    let (cpu, _) = run("\tldy #$0001\n\tcmpy #$0002\n\ttfr cc,a");
    assert_eq!(cpu.a & (N | C), N | C);
    let (cpu, _) = run("\tldu #$8000\n\tcmpu #$8000\n\ttfr cc,a\n\tlds #$10\n\tcmps #$20\n\ttfr cc,b");
    assert_eq!((cpu.a & Z, cpu.b & C), (Z, C));
    // MUL: C is bit 7 of the low byte.
    let (cpu, _) = run("\tlda #$FF\n\tldb #$FF\n\tmul");
    assert_eq!((cpu.d(), cpu.cc & (Z | C)), (0xFE01, 0));
    let (cpu, _) = run("\tlda #$0C\n\tldb #$0B\n\tmul");
    assert_eq!((cpu.d(), cpu.cc & C), (0x0084, C));
    // SEX, ABX, DAA.
    let (cpu, _) = run("\tldb #$80\n\tsex");
    assert_eq!((cpu.d(), cpu.cc & N), (0xFF80, N));
    let (cpu, _) = run("\tldx #$10F0\n\tldb #$20\n\tabx");
    assert_eq!(cpu.x, 0x1110);
    let (cpu, _) = run("\tlda #$19\n\tadda #$28\n\tdaa");
    assert_eq!(cpu.a, 0x47, "19 and 28 make 47 in BCD");
    let (cpu, _) = run("\tlda #$99\n\tadda #$01\n\tdaa");
    assert_eq!((cpu.a, cpu.cc & C), (0x00, C), "99 and 1 make 100");
    // TFR and EXG, between registers of either size.
    let (cpu, _) = run("\tldx #$1234\n\ttfr x,y\n\tlda #1\n\tldb #2\n\texg a,b\n\tlda #$C8\n\ttfr a,dp\n\texg x,u");
    assert_eq!((cpu.y, cpu.a, cpu.b, cpu.dp, cpu.u), (0x1234, 0xC8, 1, 0xC8, 0x1234));
    // LEA: LEAX and LEAY set Z; LEAS and LEAU don't touch the flags.
    let (cpu, _) = run("\tldx #5\n\tleax -5,x");
    assert_eq!((cpu.x, cpu.cc & Z), (0, Z));
    let (cpu, _) = run("\tldy #$100\n\tleay $80,y\n\tleau 2,y\n\tleas -1,s");
    assert_eq!((cpu.y, cpu.u, cpu.s), (0x180, 0x182, 0x7FFF));
}

#[test]
fn every_indexed_mode_reaches_the_right_byte() {
    // A table at $1800, X pointing at its fifth byte.
    let table = "\torg $1800\ntable\tfcb $10,$11,$12,$13,$14,$15,$16,$17\n\tfdb table+2\n";
    let cases: [(&str, u8, i32); 14] = [
        ("lda ,x", 0x14, 0),
        ("lda 2,x", 0x16, 0),
        ("lda -3,x", 0x11, 0),
        ("lda 7-4,x", 0x17, 0),
        ("ldb #-2\n\tlda b,x", 0x12, 0),
        ("lda #-4\n\tlda a,x", 0x10, 0),
        ("ldd #1\n\tlda d,x", 0x15, 0),
        ("lda ,x+", 0x14, 1),
        ("lda ,x++", 0x14, 2),
        ("lda ,-x", 0x13, -1),
        ("lda ,--x", 0x12, -2),
        ("lda table+1,pcr", 0x11, 0),
        ("lda [4,x]", 0x12, 0),
        ("lda [table+8]", 0x12, 0),
    ];
    for (code, want, moved) in cases {
        let (cpu, _) = run(&format!("\tldx #$1804\n\t{code}\n\tjmp done\n{table}"));
        assert_eq!(cpu.a, want, "{code}");
        assert_eq!(i32::from(cpu.x) - 0x1804, moved, "{code}: X afterwards");
    }
    // A 16-bit offset, an 8-bit one from Y, and the auto-increments from U.
    let (cpu, _) = run(&format!("\tldx #$1804-1000\n\tlda 1000,x\n\tldy #table\n\tldb 100-94,y\n\tldu #table\n\tldu ,u++\n\tjmp done\n{table}"));
    assert_eq!((cpu.a, cpu.b, cpu.u), (0x14, 0x16, 0x1011));
}

#[test]
fn the_stacks_push_and_pull_in_motorola_order() {
    let (cpu, ram) = run("\tlds #$3000\n\tlda #$11\n\tldb #$22\n\tldx #$3344\n\tandcc #0\n\tpshs x,b,a,cc\n\tstd $2000\n\tclra\n\tclrb\n\tldx #0\n\tpuls cc,a,b,x");
    assert_eq!(cpu.s, 0x3000);
    assert_eq!(&ram.0[0x2FFB..0x3000], &[0x00, 0x11, 0x22, 0x33, 0x44], "CC lowest, then A, B, X");
    assert_eq!((cpu.a, cpu.b, cpu.x), (0x11, 0x22, 0x3344));
    // PSHU and PULU, with S among them.
    let (cpu, ram) = run("\tldu #$4000\n\tldy #$5566\n\tlds #$7777\n\tpshu s,y\n\tldy #0\n\tpulu y\n\tleau 2,u");
    assert_eq!((cpu.u, cpu.y), (0x4000, 0x5566));
    assert_eq!(&ram.0[0x3FFC..0x4000], &[0x55, 0x66, 0x77, 0x77], "Y lower than S");
    // Calls and returns, PULS PC among them.
    let (cpu, _) = run("\tlds #$3000\n\tjsr sub\n\tlda #1\n\tbra done\nsub\tldb #2\n\tlbsr sub2\n\trts\nsub2\tldx #3\n\tbsr sub3\n\tpuls pc\nsub3\tldy #4\n\trts");
    assert_eq!((cpu.a, cpu.b, cpu.x, cpu.y, cpu.s), (1, 2, 3, 4, 0x3000));
}

#[test]
fn branches_take_their_conditions() {
    // Each condition with the flags set or not: (branch, CC, taken).
    let cases = [
        ("bhi", 0, true), ("bhi", C, false), ("bls", Z, true), ("bcc", 0, true), ("bcs", C, true), ("bne", Z, false),
        ("beq", Z, true), ("bvc", V, false), ("bvs", V, true), ("bpl", N, false), ("bmi", N, true), ("bge", N | V, true),
        ("bge", N, false), ("blt", V, true), ("bgt", 0, true), ("bgt", Z, false), ("ble", N, true), ("brn", 0, false),
    ];
    for (branch, cc, taken) in cases {
        for long in [false, true] {
            let op = if long { format!("l{branch}") } else { branch.to_string() };
            let (cpu, _) = run(&format!("\tclrb\n\tandcc #0\n\torcc #{cc}\n\t{op} yes\n\tbra done\nyes\tldb #1"));
            assert_eq!(cpu.b == 1, taken, "{op} with CC {cc:02X}");
        }
    }
}

/// The cycles of one instruction, run once.
fn cycles_of(code: &str, setup: impl Fn(&mut Cpu)) -> u32 {
    let assembly = assemble(&format!("\torg $1000\n\t{code}\n"));
    let mut ram = Ram(assembly.image(0, 0x1_0000));
    let mut cpu = Cpu { pc: 0x1000, s: 0x8000, x: 0x2000, ..Cpu::default() };
    setup(&mut cpu);
    cpu.step(&mut ram, Lines::default())
}

#[test]
fn instructions_take_motorolas_cycles() {
    // From the MC6809 data sheet's tables, with its indexed modes' extra cycles.
    for (code, want) in [("lda #1", 2), ("lda ,x", 4), ("lda 5,x", 5), ("lda 100,x", 5), ("lda 1000,x", 8), ("lda [,x]", 7), ("lda ,x++", 7), ("jsr $1234", 8), ("pshs a,b,x", 9), ("puls a,b,x,y,u,pc", 15), ("mul", 11), ("nop", 2), ("ldd #1", 3), ("bra *", 3), ("swi", 19)] {
        assert_eq!(cycles_of(code, |_| {}), want, "{code}");
    }
    // A long conditional branch takes one more when it's taken.
    assert_eq!(cycles_of("lbeq $2000", |cpu| cpu.cc = 0), 5);
    assert_eq!(cycles_of("lbeq $2000", |cpu| cpu.cc = Z), 6);
}

/// A program with interrupt handlers, its vectors set and the processor reset.
fn with_handlers(source: &str) -> (Cpu, Ram, crate::asm::Assembly) {
    let assembly = assemble(source);
    let mut ram = Ram(assembly.image(0, 0x1_0000));
    let symbol = |name: &str| assembly.symbols.get(name).map(|a| *a as u16);
    for (vector, name) in [(0xFFFE, "start"), (0xFFF8, "irq"), (0xFFF6, "firq")] {
        if let Some(at) = symbol(name) {
            ram.0[vector..vector + 2].copy_from_slice(&at.to_be_bytes());
        }
    }
    let mut cpu = Cpu::default();
    cpu.reset(&mut ram);
    (cpu, ram, assembly)
}

#[test]
fn interrupts_stack_what_they_should_and_return() {
    let (mut cpu, mut ram, assembly) = with_handlers("\torg $1000\nstart\tlds #$3000\n\tandcc #$AF\nloop\tinca\n\tbra loop\nirq\tinc $2000\n\trti\nfirq\tinc $2001\n\trti\n");
    let symbol = |name: &str| assembly.symbols[name] as u16;
    let none = Lines::default();
    // Masked at reset: nothing happens.
    cpu.step(&mut ram, Lines { irq: true, firq: true, ..none });
    assert_eq!(cpu.s, 0x3000, "LDS ran, not an interrupt");
    for _ in 0..6 {
        cpu.step(&mut ram, none);
    }
    let (a, x) = (cpu.a, cpu.x);
    // An IRQ stacks all twelve bytes, with E set, and masks IRQ.
    assert_eq!(cpu.step(&mut ram, Lines { irq: true, ..none }), 19);
    assert_eq!((cpu.pc, cpu.s, cpu.cc & (E | I)), (symbol("irq"), 0x3000 - 12, E | I));
    cpu.step(&mut ram, Lines { irq: true, ..none });
    assert_eq!(cpu.step(&mut ram, none), 15, "RTI unstacking everything");
    assert_eq!((cpu.s, cpu.a, cpu.x, ram.0[0x2000]), (0x3000, a, x, 1));
    assert_eq!(cpu.cc & I, 0, "IRQ unmasked again");
    // A FIRQ stacks only PC and CC, with E clear.
    assert_eq!(cpu.step(&mut ram, Lines { firq: true, ..none }), 10);
    assert_eq!((cpu.pc, cpu.s, cpu.cc & (E | F | I)), (symbol("firq"), 0x3000 - 3, F | I));
    cpu.step(&mut ram, none);
    assert_eq!(cpu.step(&mut ram, none), 6, "RTI after a FIRQ");
    assert_eq!((cpu.s, ram.0[0x2001]), (0x3000, 1));
}

#[test]
fn cwai_and_sync_wait_for_an_interrupt() {
    let (mut cpu, mut ram, assembly) = with_handlers("\torg $1000\nstart\tlds #$3000\n\tcwai #$EF\n\tlda #$42\n\tsync\n\tldb #$24\nhere\tbra here\nirq\tinc $2000\n\trti\n");
    let symbol = |name: &str| assembly.symbols[name] as u16;
    let none = Lines::default();
    cpu.step(&mut ram, none);
    cpu.step(&mut ram, none);
    assert_eq!((cpu.wait, cpu.s), (Wait::Cwai, 0x3000 - 12), "everything stacked, waiting");
    for _ in 0..10 {
        assert_eq!(cpu.step(&mut ram, none), 1);
    }
    cpu.step(&mut ram, Lines { irq: true, ..none });
    assert_eq!((cpu.pc, cpu.s), (symbol("irq"), 0x3000 - 12), "straight to the handler, nothing stacked again");
    cpu.step(&mut ram, none);
    cpu.step(&mut ram, none);
    assert_eq!((cpu.a, ram.0[0x2000]), (0, 1), "back after CWAI");
    cpu.step(&mut ram, none);
    assert_eq!(cpu.a, 0x42);
    // SYNC with IRQ masked: the line carries it on to the next instruction, without the handler.
    cpu.cc |= I;
    cpu.step(&mut ram, none);
    assert_eq!(cpu.wait, Wait::Sync);
    cpu.step(&mut ram, none);
    assert_eq!(cpu.wait, Wait::Sync);
    cpu.step(&mut ram, Lines { irq: true, ..none });
    cpu.step(&mut ram, Lines { irq: true, ..none });
    assert_eq!((cpu.b, ram.0[0x2000]), (0x24, 1));
}

#[test]
fn software_interrupts_go_through_their_vectors() {
    for (op, vector, masks) in [("swi", 0xFFFA, true), ("swi2", 0xFFF4, false), ("swi3", 0xFFF2, false)] {
        let assembly = assemble(&format!("\torg $1000\n\tlds #$3000\n\t{op}\n"));
        let mut ram = Ram(assembly.image(0, 0x1_0000));
        ram.0[vector..vector + 2].copy_from_slice(&[0x50, 0x00]);
        let mut cpu = Cpu { pc: 0x1000, ..Cpu::default() };
        cpu.step(&mut ram, Lines::default());
        cpu.step(&mut ram, Lines::default());
        assert_eq!((cpu.pc, cpu.s, cpu.cc & E != 0, cpu.cc & (I | F) == I | F), (0x5000, 0x3000 - 12, true, masks), "{op}");
    }
}

#[test]
fn every_instruction_the_disassembler_knows_runs_and_nothing_else_does() {
    // The undocumented aliases of the memory instructions, which the chip runs and the disassembler doesn't name.
    let alias = |page: u16, op: u16| page == 0 && matches!(op >> 4, 0 | 4 | 5 | 6 | 7) && (matches!(op & 0xF, 0x1 | 0x2 | 0x5 | 0xB) || matches!(op, 0x4E | 0x5E));
    for page in [0u16, 0x10, 0x11] {
        for op in 0..=0xFFu16 {
            if page == 0 && matches!(op, 0x10 | 0x11) {
                continue;
            }
            let mut bytes: Vec<u8> = if page == 0 { vec![op as u8] } else { vec![page as u8, op as u8] };
            // Operands that make a valid postbyte, register pair and stack mask.
            bytes.extend([0x12; 5]);
            let decoded = crate::disasm::m6809(&|i| bytes.get(i).copied().unwrap_or(0), 0x1000, false);
            let known = !decoded.text.starts_with("fcb");
            let mut ram = Ram(vec![0; 0x1_0000]);
            ram.0[0x1000..0x1000 + bytes.len()].copy_from_slice(&bytes);
            let mut cpu = Cpu { pc: 0x1000, s: 0x8000, u: 0x7000, ..Cpu::default() };
            cpu.step(&mut ram, Lines::default());
            let runs = cpu.illegal.is_none();
            if alias(page, op) {
                assert!(runs, "${page:02X}{op:02X} runs, as on the chip");
                continue;
            }
            assert_eq!(runs, known, "${page:02X}{op:02X} ({})", decoded.text);
            // What doesn't jump or call ends where the disassembler says its instruction does.
            let word = decoded.text.split_whitespace().next().unwrap_or("");
            let branch = word.strip_prefix('l').unwrap_or(word);
            let flow = matches!(branch, "jsr" | "jmp" | "rts" | "rti" | "swi" | "swi2" | "swi3" | "bsr" | "bra" | "brn" | "bhi" | "bls" | "bcc" | "bhs" | "bcs" | "blo" | "bne" | "beq" | "bvc" | "bvs" | "bpl" | "bmi" | "bge" | "blt" | "bgt" | "ble" | "tfr" | "exg");
            if runs && !flow {
                assert_eq!(usize::from(cpu.pc.wrapping_sub(0x1000)), decoded.length, "${page:02X}{op:02X} {}: its length", decoded.text);
            }
        }
    }
}

#[test]
fn a_program_runs_as_written() {
    // Fibonacci's numbers to $2000, 16 bits each, and then copied to $2100 with the auto-incrementing modes.
    let (_, ram) = run(
        "\tldx #$2000\n\tldd #1\n\tstd ,x++\n\tstd ,x++\n\tldb #10\n\tstb $1FFF\nnext\tldd -4,x\n\taddd -2,x\n\tstd ,x++\n\tdec $1FFF\n\tbne next\n\tldx #$2000\n\tldu #$2100\ncopy\tldd ,x++\n\tstd ,u++\n\tcmpx #$2018\n\tblo copy",
    );
    let numbers: Vec<u16> = ram.0[0x2100..0x2118].chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect();
    assert_eq!(numbers, vec![1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144]);
}
