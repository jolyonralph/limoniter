//! The instruction reference: for the processor of the source in front, an instruction's forms, each with its
//! encoding and (for the Z80 and 6502) its cycles, as Limoniter's own assemblers make them, with a short note of
//! what it does and which flags it changes. The forms are found by asking the assembler to assemble each shape of
//! operand the processor has, twice with different numbers: the bytes that change are the operand's.
//!
//! The notes were written for Limoniter from the makers' manuals: Zilog's Z80 CPU User Manual, MOS's MCS6500
//! Microcomputer Family Programming Manual, Motorola's MC6809 data sheet and M68000 Programmer's Reference Manual.

use crate::assemblers::Cpu;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// What an instruction does: the flags it changes, its timing where the forms don't give it, and a sentence or two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct About {
    pub flags: &'static str,
    pub cycles: &'static str,
    pub text: &'static str,
}

/// One way of writing an instruction, and what it becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub form: String,
    /// The bytes in hex, with nn for each byte that comes from the operand.
    pub encoding: String,
    pub cycles: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub cpu: Cpu,
    pub mnemonic: String,
    pub about: Option<About>,
    pub forms: Vec<Form>,
}

/// The flags each processor has, in the order the notes give them.
pub fn flag_names(cpu: Cpu) -> &'static [&'static str] {
    match cpu {
        Cpu::Z80 => &["S", "Z", "H", "P/V", "N", "C"],
        Cpu::M6502 => &["N", "V", "D", "I", "Z", "C"],
        Cpu::M6809 => &["E", "F", "H", "I", "N", "Z", "V", "C"],
        Cpu::M68k => &["X", "N", "Z", "V", "C"],
        // The Game Boy's and the SNES sound processor's: forms and encodings are shown, but not notes yet.
        Cpu::X86 | Cpu::Arm | Cpu::Sm83 | Cpu::Spc700 => &[],
    }
}

/// What each mark under a flag means, in the language chosen.
pub fn flag_meaning(mark: char) -> &'static str {
    match mark {
        '*' => crate::i18n::tr("set by the result"),
        '0' => crate::i18n::tr("cleared"),
        '1' => crate::i18n::tr("set"),
        '-' => crate::i18n::tr("not changed"),
        '?' => crate::i18n::tr("left undefined"),
        'V' => crate::i18n::tr("overflow"),
        'P' => crate::i18n::tr("parity"),
        _ => "",
    }
}

// Each table: the mnemonics a note is for, the flags, the timing (if the forms don't show it) and the note.

const Z80: &[(&str, &str, &str, &str)] = &[
    ("adc", "***V0*", "", "Adds the operand and the carry flag to A, or a register pair and the carry to HL. Used for sums wider than one register."),
    ("add", "***V0*", "", "Adds the operand to A, or a register pair to HL, IX or IY. ADD HL, ADD IX and ADD IY change only H, N and C."),
    ("and", "**1P00", "", "ANDs A with the operand, bit by bit, leaving the result in A."),
    ("bit", "?*1?0-", "", "Tests one bit (0 to 7) of a register or of memory: Z is set if the bit is 0. Nothing is changed but the flags."),
    ("call", "------", "", "Pushes the address of the next instruction and jumps to a subroutine. With a condition (NZ, Z, NC, C, PO, PE, P, M) it calls only if the condition holds."),
    ("ccf", "--*-0*", "", "Complements the carry flag; H takes the carry's old value."),
    ("cp", "***V1*", "", "Compares A with the operand by subtracting it, setting the flags as SUB would but leaving A alone. Z means equal; C means A was the smaller, unsigned."),
    ("cpd", "****1-", "", "Compares A with (HL), then decrements HL and BC. Z is set on a match; P/V is cleared when BC reaches 0."),
    ("cpdr", "****1-", "", "Repeats CPD until A matches (HL) or BC reaches 0: a search backwards through memory."),
    ("cpi", "****1-", "", "Compares A with (HL), then increments HL and decrements BC. Z is set on a match; P/V is cleared when BC reaches 0."),
    ("cpir", "****1-", "", "Repeats CPI until A matches (HL) or BC reaches 0: a search forwards through memory."),
    ("cpl", "--1-1-", "", "Inverts every bit of A (the ones' complement)."),
    ("daa", "***P-*", "", "Adjusts A after a BCD addition or subtraction (going by N, H and C), so it holds two decimal digits again."),
    ("dec", "***V1-", "", "Subtracts 1 from a register or from memory, or from a register pair. A pair's DEC changes no flags."),
    ("di", "------", "", "Disables maskable interrupts (clears IFF1 and IFF2)."),
    ("djnz", "------", "", "Decrements B and jumps relatively (-126 to +129 bytes from the instruction) unless B has reached 0: the Z80's loop counter."),
    ("ei", "------", "", "Enables maskable interrupts, after the instruction that follows it, so EI then RET returns before one can come in."),
    ("ex", "------", "", "Exchanges two registers' contents: DE with HL, AF with AF', or the word at the top of the stack with HL, IX or IY. EX AF,AF' brings the other set of flags in."),
    ("exx", "------", "", "Exchanges BC, DE and HL with their shadows BC', DE' and HL'."),
    ("halt", "------", "", "Stops until an interrupt or a reset, running NOPs meanwhile so memory is still refreshed."),
    ("im", "------", "", "Sets the interrupt mode: 0 (the device puts an instruction on the bus), 1 (an RST $38) or 2 (a call through a table at I*256 plus the device's byte)."),
    ("in", "**0P0-", "", "Reads a byte from an I/O port. IN A,(n) puts A on the top half of the address and changes no flags; IN r,(C) uses BC as the port and sets S, Z and P/V by the byte."),
    ("inc", "***V0-", "", "Adds 1 to a register or to memory, or to a register pair. A pair's INC changes no flags."),
    ("ind", "?*??1-", "", "Reads port C into (HL), then decrements HL and B. Z is set when B reaches 0."),
    ("indr", "?1??1-", "", "Repeats IND until B is 0."),
    ("ini", "?*??1-", "", "Reads port C into (HL), then increments HL and decrements B. Z is set when B reaches 0."),
    ("inir", "?1??1-", "", "Repeats INI until B is 0."),
    ("jp", "------", "", "Jumps to an address, or to the one in HL, IX or IY (JP (HL)). With a condition, only if it holds."),
    ("jr", "------", "", "Jumps relatively, -126 to +129 bytes from the instruction, in two bytes. With NZ, Z, NC or C, only if the condition holds."),
    ("ld", "------", "", "Copies the second operand into the first: a register, a register pair, memory or a number. LD A,I and LD A,R set S and Z, and copy IFF2 to P/V."),
    ("ldd", "--0*0-", "", "Copies (HL) to (DE), then decrements HL, DE and BC. P/V is cleared when BC reaches 0."),
    ("lddr", "--000-", "", "Repeats LDD until BC is 0: a block copy backwards, for blocks that overlap upwards."),
    ("ldi", "--0*0-", "", "Copies (HL) to (DE), then increments HL and DE and decrements BC. P/V is cleared when BC reaches 0."),
    ("ldir", "--000-", "", "Repeats LDI until BC is 0: a block copy forwards. 21 T-states a byte, 16 for the last."),
    ("neg", "***V1*", "", "Negates A: A becomes 0 minus A (the two's complement)."),
    ("nop", "------", "", "Does nothing for 4 T-states."),
    ("or", "**0P00", "", "ORs A with the operand, bit by bit, leaving the result in A."),
    ("otdr", "?1??1-", "", "Repeats OUTD until B is 0."),
    ("otir", "?1??1-", "", "Repeats OUTI until B is 0."),
    ("out", "------", "", "Writes a byte to an I/O port. OUT (n),A puts A on the top half of the address; OUT (C),r uses BC as the port."),
    ("outd", "?*??1-", "", "Decrements B, then writes (HL) to port C and decrements HL. Z is set when B reaches 0."),
    ("outi", "?*??1-", "", "Decrements B, then writes (HL) to port C and increments HL. Z is set when B reaches 0."),
    ("pop", "------", "", "Takes a register pair from the stack, and SP goes up by 2. POP AF loads the flags too."),
    ("push", "------", "", "Puts a register pair on the stack, and SP goes down by 2."),
    ("res", "------", "", "Clears one bit (0 to 7) of a register or of memory."),
    ("ret", "------", "", "Returns from a subroutine, taking the address from the stack. With a condition, only if it holds."),
    ("reti", "------", "", "Returns from a maskable interrupt. Zilog's own peripherals watch for it to know their interrupt is over."),
    ("retn", "------", "", "Returns from a non-maskable interrupt, copying IFF2 back to IFF1."),
    ("rl", "**0P0*", "", "Rotates left through the carry: bit 7 goes to C, and C into bit 0."),
    ("rla", "--0-0*", "", "Rotates A left through the carry: a quicker RL A that leaves S, Z and P/V alone."),
    ("rlc", "**0P0*", "", "Rotates left: bit 7 goes to both C and bit 0."),
    ("rlca", "--0-0*", "", "Rotates A left, bit 7 going to both C and bit 0: a quicker RLC A."),
    ("rld", "**0P0-", "", "Rotates the three low nibbles of A and (HL) left: (HL)'s high nibble goes into A's low, A's low into (HL)'s low, and (HL)'s low into its high. For BCD digits."),
    ("rr", "**0P0*", "", "Rotates right through the carry: bit 0 goes to C, and C into bit 7."),
    ("rra", "--0-0*", "", "Rotates A right through the carry: a quicker RR A that leaves S, Z and P/V alone."),
    ("rrc", "**0P0*", "", "Rotates right: bit 0 goes to both C and bit 7."),
    ("rrca", "--0-0*", "", "Rotates A right, bit 0 going to both C and bit 7: a quicker RRC A."),
    ("rrd", "**0P0-", "", "Rotates the three low nibbles of A and (HL) right: A's low nibble goes into (HL)'s high, (HL)'s high into its low, and (HL)'s low into A's. For BCD digits."),
    ("rst", "------", "", "Calls one of eight fixed addresses, $00, $08 and so on up to $38, in one byte."),
    ("sbc", "***V1*", "", "Subtracts the operand and the carry from A, or a register pair and the carry from HL."),
    ("scf", "--0-01", "", "Sets the carry flag."),
    ("set", "------", "", "Sets one bit (0 to 7) of a register or of memory."),
    ("sla", "**0P0*", "", "Shifts left: bit 7 goes to C and 0 into bit 0, which doubles the value."),
    ("sll sli", "**0P0*", "", "Undocumented, though every Z80 has it: shifts left, bit 7 going to C and 1 into bit 0."),
    ("sra", "**0P0*", "", "Shifts right, keeping bit 7 (the sign): halves a signed value. Bit 0 goes to C."),
    ("srl", "**0P0*", "", "Shifts right, 0 going into bit 7: halves an unsigned value. Bit 0 goes to C."),
    ("sub", "***V1*", "", "Subtracts the operand from A."),
    ("xor", "**0P00", "", "Exclusive-ORs A with the operand, bit by bit. XOR A is the quick way to clear A."),
    ("ldix", "------", "", "Spectrum Next: as LDI, but a byte the same as A isn't copied, so A can be a transparent colour."),
    ("ldirx", "------", "", "Spectrum Next: repeats LDIX until BC is 0."),
    ("lddx", "------", "", "Spectrum Next: as LDD (though DE goes up), but a byte the same as A isn't copied."),
    ("lddrx", "------", "", "Spectrum Next: repeats LDDX until BC is 0."),
    ("ldpirx", "------", "", "Spectrum Next: copies a pattern of 8 bytes at HL (aligned to 8) to DE, skipping bytes the same as A, until BC is 0."),
    ("ldws", "------", "", "Spectrum Next: copies (HL) to (DE), then increments L and D: down a column of a layer."),
    ("outinb", "------", "", "Spectrum Next: writes (HL) to port BC and increments HL, leaving B alone."),
    ("mul", "------", "", "Spectrum Next: multiplies D by E, both unsigned, leaving the 16-bit product in DE."),
    ("swapnib", "------", "", "Spectrum Next: swaps A's two nibbles."),
    ("mirror", "------", "", "Spectrum Next: reverses the order of A's bits."),
    ("nextreg", "------", "", "Spectrum Next: writes a number, or A, to one of the Next's registers."),
    ("pixeldn", "------", "", "Spectrum Next: moves HL, an address in the Spectrum's screen, to the pixel line below."),
    ("pixelad", "------", "", "Spectrum Next: puts the screen address of the pixel at column E, line D into HL."),
    ("setae", "------", "", "Spectrum Next: puts the mask of the pixel at column E (its low 3 bits) into A."),
    ("test", "**1P00", "", "Spectrum Next: ANDs A with a number to set the flags, leaving A alone."),
    ("bsla", "------", "", "Spectrum Next: shifts DE left by B places (B's low 5 bits)."),
    ("bsra", "------", "", "Spectrum Next: shifts DE right by B places, keeping the sign."),
    ("bsrl", "------", "", "Spectrum Next: shifts DE right by B places, 0s coming in."),
    ("bsrf", "------", "", "Spectrum Next: shifts DE right by B places, 1s coming in."),
    ("brlc", "------", "", "Spectrum Next: rotates DE left by B places (B's low 4 bits)."),
];

const M6502: &[(&str, &str, &str, &str)] = &[
    ("adc", "**--**", "", "Adds memory and the carry to A. Clear the carry first (CLC) for a plain add. With D set, it adds in BCD."),
    ("and", "*---*-", "", "ANDs A with memory, bit by bit."),
    ("asl", "*---**", "", "Shifts A or memory left: bit 7 goes to C and 0 into bit 0, which doubles it."),
    ("bcc", "------", "", "Branches if the carry is clear (after a compare: less than, unsigned). 2 cycles, 3 if taken, 4 if taken into another page."),
    ("bcs", "------", "", "Branches if the carry is set (after a compare: greater or equal, unsigned)."),
    ("beq", "------", "", "Branches if Z is set: the result was 0, or a compare found them equal."),
    ("bmi", "------", "", "Branches if N is set: the result's bit 7 is 1."),
    ("bne", "------", "", "Branches if Z is clear: the result wasn't 0, or a compare found them different."),
    ("bpl", "------", "", "Branches if N is clear: the result's bit 7 is 0."),
    ("bvc", "------", "", "Branches if the overflow flag is clear."),
    ("bvs", "------", "", "Branches if the overflow flag is set."),
    ("bit", "**--*-", "", "Tests memory: N and V take its bits 7 and 6, and Z is set if A AND memory is 0. A is left alone."),
    ("brk", "---1--", "", "A software interrupt: pushes the address after its second byte and the flags (with B set), sets I and jumps through the vector at $FFFE."),
    ("clc", "-----0", "", "Clears the carry, as you do before ADC."),
    ("cld", "--0---", "", "Clears decimal mode, so ADC and SBC work in binary."),
    ("cli", "---0--", "", "Clears the interrupt mask, letting IRQs in."),
    ("clv", "-0----", "", "Clears the overflow flag."),
    ("cmp", "*---**", "", "Compares A with memory by subtracting, setting N, Z and C but leaving A alone. C set means A is greater or equal, unsigned."),
    ("cpx", "*---**", "", "Compares X with memory, as CMP does A."),
    ("cpy", "*---**", "", "Compares Y with memory, as CMP does A."),
    ("dec", "*---*-", "", "Subtracts 1 from memory."),
    ("dex", "*---*-", "", "Subtracts 1 from X."),
    ("dey", "*---*-", "", "Subtracts 1 from Y."),
    ("eor", "*---*-", "", "Exclusive-ORs A with memory, bit by bit."),
    ("inc", "*---*-", "", "Adds 1 to memory."),
    ("inx", "*---*-", "", "Adds 1 to X."),
    ("iny", "*---*-", "", "Adds 1 to Y."),
    ("jmp", "------", "", "Jumps to an address, or to the address held at one (JMP (addr)). The NMOS 6502's indirect JMP doesn't carry into the next page: JMP ($10FF) reads $10FF and $1000."),
    ("jsr", "------", "", "Pushes the address of its own last byte and jumps to a subroutine; RTS adds 1 coming back."),
    ("lda", "*---*-", "", "Loads A from memory or a number."),
    ("ldx", "*---*-", "", "Loads X from memory or a number."),
    ("ldy", "*---*-", "", "Loads Y from memory or a number."),
    ("lsr", "0---**", "", "Shifts A or memory right: bit 0 goes to C and 0 into bit 7, which halves it."),
    ("nop", "------", "", "Does nothing for 2 cycles. The other forms are undocumented NOPs that read memory."),
    ("ora", "*---*-", "", "ORs A with memory, bit by bit."),
    ("pha", "------", "", "Pushes A onto the stack, in page 1."),
    ("php", "------", "", "Pushes the flags onto the stack, with B and bit 5 set."),
    ("pla", "*---*-", "", "Pulls A from the stack."),
    ("plp", "******", "", "Pulls the flags from the stack."),
    ("rol", "*---**", "", "Rotates A or memory left through the carry: bit 7 to C, and C into bit 0."),
    ("ror", "*---**", "", "Rotates A or memory right through the carry: bit 0 to C, and C into bit 7."),
    ("rti", "******", "", "Returns from an interrupt: pulls the flags, then the address."),
    ("rts", "------", "", "Returns from a subroutine: pulls the address and adds 1."),
    ("sbc", "**--**", "", "Subtracts memory and the borrow (the carry clear) from A. Set the carry first (SEC) for a plain subtraction. With D set, in BCD."),
    ("sec", "-----1", "", "Sets the carry, as you do before SBC."),
    ("sed", "--1---", "", "Sets decimal mode: ADC and SBC work in BCD."),
    ("sei", "---1--", "", "Sets the interrupt mask, keeping IRQs out (NMIs still come)."),
    ("sta", "------", "", "Stores A in memory."),
    ("stx", "------", "", "Stores X in memory."),
    ("sty", "------", "", "Stores Y in memory."),
    ("tax", "*---*-", "", "Copies A to X."),
    ("tay", "*---*-", "", "Copies A to Y."),
    ("tsx", "*---*-", "", "Copies the stack pointer to X."),
    ("txa", "*---*-", "", "Copies X to A."),
    ("txs", "------", "", "Copies X to the stack pointer."),
    ("tya", "*---*-", "", "Copies Y to A."),
    ("slo aso", "*---**", "", "Undocumented: shifts memory left (ASL), then ORs A with it."),
    ("rla", "*---**", "", "Undocumented: rotates memory left (ROL), then ANDs A with it."),
    ("sre lse", "*---**", "", "Undocumented: shifts memory right (LSR), then exclusive-ORs A with it."),
    ("rra", "**--**", "", "Undocumented: rotates memory right (ROR), then adds it to A with the carry (ADC)."),
    ("sax axs aax", "------", "", "Undocumented: stores A AND X in memory."),
    ("lax", "*---*-", "", "Undocumented: loads A and X from memory together."),
    ("dcp dcm", "*---**", "", "Undocumented: decrements memory, then compares A with it."),
    ("isc isb ins", "**--**", "", "Undocumented: increments memory, then subtracts it from A with the borrow (SBC)."),
    ("anc", "*---**", "", "Undocumented: ANDs A with a number, then copies bit 7 to the carry."),
    ("alr asr", "*---**", "", "Undocumented: ANDs A with a number, then shifts A right (LSR)."),
    ("arr", "**--**", "", "Undocumented: ANDs A with a number, then rotates A right; C and V come out oddly, from bits 6 and 5."),
    ("sbx axs", "*---**", "", "Undocumented: X becomes (A AND X) minus a number, with no borrow; C as a compare sets it."),
    ("lxa", "*---*-", "", "Undocumented and unstable: A and X become (A OR a constant that varies from chip to chip) AND the number."),
    ("ane xaa", "*---*-", "", "Undocumented and unstable: A becomes (A OR a varying constant) AND X AND the number."),
    ("las", "*---*-", "", "Undocumented: A, X and the stack pointer all become memory AND the stack pointer."),
    ("tas shs", "------", "", "Undocumented and unstable: the stack pointer becomes A AND X, and memory gets that AND the address's high byte plus 1."),
    ("sha ahx", "------", "", "Undocumented and unstable: stores A AND X AND the address's high byte plus 1."),
    ("shx sxa", "------", "", "Undocumented and unstable: stores X AND the address's high byte plus 1."),
    ("shy sya", "------", "", "Undocumented and unstable: stores Y AND the address's high byte plus 1."),
    ("jam kil", "------", "", "Undocumented: stops the processor until it's reset."),
];

const M6809: &[(&str, &str, &str, &str)] = &[
    ("abx", "--------", "3", "Adds B, unsigned, to X."),
    ("adca adcb", "--*-****", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Adds memory and the carry to A or B."),
    ("adda addb", "--*-****", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Adds memory to A or B."),
    ("addd", "----****", "4 immediate, 6 direct, 6+ indexed, 7 extended", "Adds a 16-bit value to D."),
    ("anda andb", "----**0-", "2 immediate, 4 direct, 4+ indexed, 5 extended", "ANDs A or B with memory."),
    ("andcc", "********", "3", "ANDs the condition codes with a number: clears flags, such as the interrupt masks (ANDCC #$AF lets IRQ and FIRQ in)."),
    ("asl lsl asla aslb lsla lslb", "--?-****", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Shifts left: bit 7 goes to C and 0 into bit 0."),
    ("asr asra asrb", "--?-**-*", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Shifts right, keeping bit 7 (the sign); bit 0 goes to C."),
    ("bcc bhs bcs blo beq bne bmi bpl bvc bvs bge bgt ble blt bhi bls bra brn", "--------", "3", "Branches, -128 to +127 bytes, if the condition holds: BHI, BHS, BLO and BLS after comparing unsigned numbers, BGT, BGE, BLT and BLE signed ones."),
    ("lbcc lbhs lbcs lblo lbeq lbne lbmi lbpl lbvc lbvs lbge lbgt lble lblt lbhi lbls lbrn", "--------", "5, or 6 if taken", "A long branch, anywhere in the 64K, if the condition holds."),
    ("lbra", "--------", "5", "A long branch, anywhere in the 64K."),
    ("bita bitb", "----**0-", "2 immediate, 4 direct, 4+ indexed, 5 extended", "ANDs A or B with memory to set the flags, leaving the register alone."),
    ("bsr", "--------", "7", "Pushes the return address and branches to a subroutine, -128 to +127 bytes away."),
    ("lbsr", "--------", "9", "Pushes the return address and branches to a subroutine anywhere."),
    ("clr clra clrb", "----0100", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Clears a register or memory to 0."),
    ("cmpa cmpb", "--?-****", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Compares A or B with memory by subtracting, setting the flags only."),
    ("cmpx", "----****", "4 immediate, 6 direct, 6+ indexed, 7 extended", "Compares X with a 16-bit value."),
    ("cmpd cmpy cmpu cmps", "----****", "5 immediate, 7 direct, 7+ indexed, 8 extended", "Compares a 16-bit register with a 16-bit value."),
    ("com coma comb", "----**01", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Inverts every bit (the ones' complement); C is set."),
    ("cwai", "********", "20 or more", "ANDs the condition codes with a number, pushes every register and waits for an interrupt."),
    ("daa", "----**?*", "2", "Adjusts A after adding BCD numbers, so it holds two decimal digits again."),
    ("dec deca decb", "----***-", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Subtracts 1. The carry isn't changed, so it can count a loop through a sum."),
    ("eora eorb", "----**0-", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Exclusive-ORs A or B with memory."),
    ("exg", "--------", "8", "Exchanges two registers of the same size (exchanging with CC sets the flags)."),
    ("inc inca incb", "----***-", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Adds 1. The carry isn't changed."),
    ("jmp", "--------", "3 direct, 3+ indexed, 4 extended", "Jumps to an address."),
    ("jsr", "--------", "7 direct, 7+ indexed, 8 extended", "Pushes the return address and jumps to a subroutine."),
    ("lda ldb", "----**0-", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Loads A or B from memory or a number."),
    ("ldd ldx ldu", "----**0-", "3 immediate, 5 direct, 5+ indexed, 6 extended", "Loads a 16-bit register."),
    ("ldy lds", "----**0-", "4 immediate, 6 direct, 6+ indexed, 7 extended", "Loads a 16-bit register. Loading S also lets NMIs in."),
    ("leax leay", "-----*--", "4+", "Loads X or Y with the address an indexed operand works out, without reading it: LEAX 1,X adds 1. Z is set if it's 0."),
    ("leas leau", "--------", "4+", "Loads S or U with the address an indexed operand works out, without reading it: LEAS -2,S makes room on the stack."),
    ("lsr lsra lsrb", "----0*-*", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Shifts right, 0 going into bit 7; bit 0 goes to C."),
    ("mul", "-----*-*", "11", "Multiplies A by B, unsigned, into D. C takes bit 7 of B, for rounding."),
    ("neg nega negb", "--?-****", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Negates: the value becomes 0 minus itself."),
    ("nop", "--------", "2", "Does nothing for 2 cycles."),
    ("ora orb", "----**0-", "2 immediate, 4 direct, 4+ indexed, 5 extended", "ORs A or B with memory."),
    ("orcc", "********", "3", "ORs the condition codes with a number: sets flags, such as the interrupt masks (ORCC #$50 keeps IRQ and FIRQ out)."),
    ("pshs pshu", "--------", "5, and 1 a byte pushed", "Pushes the registers listed onto the S or U stack, in a fixed order whatever the list's."),
    ("puls pulu", "--------", "5, and 1 a byte pulled", "Pulls the registers listed from the S or U stack. PULS PC returns, and pulling CC sets the flags."),
    ("rol rola rolb", "----****", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Rotates left through the carry."),
    ("ror rora rorb", "----**-*", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Rotates right through the carry."),
    ("rti", "********", "6, or 15 after an IRQ or NMI", "Returns from an interrupt: pulls CC and, if its E flag says the whole state was saved, every register; then PC."),
    ("rts", "--------", "5", "Returns from a subroutine."),
    ("sbca sbcb", "--?-****", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Subtracts memory and the carry from A or B."),
    ("sex", "----**--", "2", "Sign-extends B into D: A becomes $FF if B is negative, 0 if not."),
    ("sta stb", "----**0-", "4 direct, 4+ indexed, 5 extended", "Stores A or B in memory."),
    ("std stx stu", "----**0-", "5 direct, 5+ indexed, 6 extended", "Stores a 16-bit register in memory."),
    ("sty sts", "----**0-", "6 direct, 6+ indexed, 7 extended", "Stores a 16-bit register in memory."),
    ("suba subb", "--?-****", "2 immediate, 4 direct, 4+ indexed, 5 extended", "Subtracts memory from A or B."),
    ("subd", "----****", "4 immediate, 6 direct, 6+ indexed, 7 extended", "Subtracts a 16-bit value from D."),
    ("swi", "11-1----", "19", "A software interrupt: pushes every register, masks IRQ and FIRQ and jumps through the vector at $FFFA."),
    ("swi2", "1-------", "20", "A software interrupt through $FFF4, leaving the masks alone: used by OS-9 for system calls."),
    ("swi3", "1-------", "20", "A software interrupt through $FFF2, leaving the masks alone."),
    ("sync", "--------", "4 or more", "Waits for an interrupt; if it's masked, carries on with the next instruction when it comes."),
    ("tfr", "--------", "6", "Copies one register into another of the same size (into CC sets the flags)."),
    ("tst tsta tstb", "----**0-", "2 on A or B; 6 direct, 6+ indexed, 7 extended", "Sets N and Z by a value, leaving it alone."),
];

const M68K: &[(&str, &str, &str, &str)] = &[
    ("abcd", "*?*?*", "6 registers, 18 memory", "Adds two BCD bytes and X. Z is only cleared, so set it before a run of them."),
    ("add", "*****", "Dn: 4 (.B/.W), 6 (.L, 8 from a register or number) plus the address's time; to memory 8 (.L 12) plus", "Adds the source to the destination; one of them must be a data register."),
    ("adda", "-----", "8 (.W), 6 (.L, 8 from a register or number) plus the address's time", "Adds to an address register. The flags aren't changed, and a word is sign-extended first."),
    ("addi", "*****", "Dn: 8 (.L 16); memory 12 (.L 20) plus the address's time", "Adds a number to the destination."),
    ("addq", "*****", "Dn: 4 (.L 8); An: 8; memory 8 (.L 12) plus the address's time", "Adds 1 to 8 to the destination. To an address register it changes no flags."),
    ("addx", "*****", "Dn: 4 (.L 8); -(An): 18 (.L 30)", "Adds the source and X to the destination, for sums wider than 32 bits. Z is only cleared."),
    ("and", "-**00", "as ADD", "ANDs the source with the destination."),
    ("andi", "-**00", "as ADDI; to CCR or SR 20", "ANDs a number with the destination, or with CCR or SR to clear flags."),
    ("asl", "*****", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Shifts left: the top bit goes to C and X. V is set if the sign changes on the way."),
    ("asr", "*****", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Shifts right, keeping the sign: the bottom bit goes to C and X."),
    ("bra", "-----", "10", "Branches always: .S within -128 to +127 bytes, .W within 32K."),
    ("bhi bls bcc bhs bcs blo bne beq bvc bvs bpl bmi bge blt bgt ble", "-----", "taken 10; not taken 8 (.S), 12 (.W)", "Branches if the condition holds: BHI, BHS, BLO and BLS after comparing unsigned numbers, BGT, BGE, BLT and BLE signed ones."),
    ("bsr", "-----", "18", "Pushes the return address and branches to a subroutine."),
    ("bchg", "--*--", "Dn: 8 (12 by number); memory 8 (12) plus the address's time", "Tests a bit (Z set if it was 0), then flips it. In a data register the bit is 0 to 31; in memory, 0 to 7."),
    ("bclr", "--*--", "Dn: 10 (14 by number); memory 8 (12) plus the address's time", "Tests a bit (Z set if it was 0), then clears it."),
    ("bset", "--*--", "Dn: 8 (12 by number); memory 8 (12) plus the address's time", "Tests a bit (Z set if it was 0), then sets it."),
    ("btst", "--*--", "Dn: 6 (10 by number); memory 4 (8) plus the address's time", "Tests a bit: Z is set if it's 0."),
    ("chk", "-*???", "10 if it doesn't trap", "Traps (through vector 6) if a data register is below 0 or above the bound given."),
    ("clr", "-0100", "Dn: 4 (.L 6); memory 8 (.L 12) plus the address's time", "Clears the destination to 0. On a 68000 it reads memory before writing it."),
    ("cmp", "-****", "4 (.L 6) plus the address's time", "Compares a data register with the source by subtracting, setting the flags only."),
    ("cmpa", "-****", "6 plus the address's time", "Compares an address register with the source (a word is sign-extended)."),
    ("cmpi", "-****", "Dn: 8 (.L 14); memory 8 (.L 12) plus the address's time", "Compares the destination with a number."),
    ("cmpm", "-****", "12 (.L 20)", "Compares two places in memory, (Ay)+ with (Ax)+: for comparing strings."),
    ("dbra dbf dbt dbhi dbls dbcc dbhs dbcs dblo dbne dbeq dbvc dbvs dbpl dbmi dbge dblt dbgt dble", "-----", "10 looping; 14 when the count runs out; 12 when the condition holds", "Unless the condition holds, decrements the low word of a data register and branches back unless it's now -1. DBRA (DBF) has no condition: a loop of n+1 times."),
    ("divs", "-***0", "up to 158", "Divides a 32-bit data register by a signed word: the quotient goes in the low word, the remainder in the high. V is set if the quotient won't fit."),
    ("divu", "-***0", "up to 140", "Divides a 32-bit data register by an unsigned word: the quotient goes in the low word, the remainder in the high. V is set if the quotient won't fit."),
    ("eor", "-**00", "Dn: 4 (.L 8); memory 8 (.L 12) plus the address's time", "Exclusive-ORs a data register into the destination."),
    ("eori", "-**00", "as ADDI; to CCR or SR 20", "Exclusive-ORs a number into the destination, or into CCR or SR to flip flags."),
    ("exg", "-----", "6", "Exchanges two registers' 32 bits."),
    ("ext", "-**00", "4", "Sign-extends a data register: a byte to a word (.W), or a word to a long (.L)."),
    ("illegal", "-----", "34", "Traps through the illegal instruction vector, on purpose."),
    ("jmp", "-----", "(An) 8, d(An) 10, abs.W 10, abs.L 12, d(PC) 10", "Jumps to an address."),
    ("jsr", "-----", "(An) 16, d(An) 18, abs.W 18, abs.L 20, d(PC) 18", "Pushes the return address and jumps to a subroutine."),
    ("lea", "-----", "(An) 4, d(An) 8, d(An,Xn) 12, abs.W 8, abs.L 12, d(PC) 8", "Loads an address register with the address an operand works out, without reading it."),
    ("link", "-----", "16", "Pushes an address register, copies SP into it, and adds a (negative) number to SP: a stack frame for local variables."),
    ("lsl", "***0*", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Shifts left: the top bit goes to C and X, 0 into the bottom."),
    ("lsr", "***0*", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Shifts right, 0 going into the top bit: the bottom bit goes to C and X."),
    ("move", "-**00", "4 register to register, plus the times of both addresses; to CCR 12, to or from SR 12 or 6", "Copies the source to the destination."),
    ("movea", "-----", "4 plus the address's time", "Copies to an address register. A word is sign-extended, and the flags aren't changed."),
    ("movem", "-----", "to memory 8 plus 4 a register (8 for .L); from memory 12 plus 4 (8)", "Copies a list of registers to or from memory: MOVEM.L D0-D7/A0-A6,-(SP) saves them all."),
    ("moveq", "-**00", "4", "Loads a data register with a number from -128 to 127, sign-extended to 32 bits, in one word."),
    ("muls", "-**00", "up to 70 (38 plus 2 a change between neighbouring bits)", "Multiplies a data register's low word by a signed word into all 32 bits."),
    ("mulu", "-**00", "up to 70 (38 plus 2 a 1 bit)", "Multiplies a data register's low word by an unsigned word into all 32 bits."),
    ("nbcd", "*?*?*", "Dn: 6; memory 8 plus the address's time", "Negates a BCD byte (0 minus it and X)."),
    ("neg", "*****", "Dn: 4 (.L 6); memory 8 (.L 12) plus the address's time", "Negates: the value becomes 0 minus itself."),
    ("negx", "*****", "Dn: 4 (.L 6); memory 8 (.L 12) plus the address's time", "Negates with X, for values wider than 32 bits."),
    ("nop", "-----", "4", "Does nothing for 4 cycles."),
    ("not", "-**00", "Dn: 4 (.L 6); memory 8 (.L 12) plus the address's time", "Inverts every bit."),
    ("or", "-**00", "as ADD", "ORs the source with the destination."),
    ("ori", "-**00", "as ADDI; to CCR or SR 20", "ORs a number into the destination, or into CCR or SR to set flags."),
    ("pea", "-----", "(An) 12, d(An) 16, d(An,Xn) 20, abs.W 16, abs.L 20, d(PC) 16", "Pushes the address an operand works out."),
    ("reset", "-----", "132", "Supervisor only: pulls the RESET line to reset the devices outside, not the processor."),
    ("rol", "-**0*", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Rotates left: the top bit goes to the bottom and to C."),
    ("ror", "-**0*", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Rotates right: the bottom bit goes to the top and to C."),
    ("roxl", "***0*", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Rotates left through X: the top bit goes to X and C, and X into the bottom."),
    ("roxr", "***0*", "register: 6 (.L 8) plus 2 a place; memory 8 plus the address's time", "Rotates right through X: the bottom bit goes to X and C, and X into the top."),
    ("rte", "*****", "20", "Supervisor only: returns from an exception, pulling SR and then PC."),
    ("rtr", "*****", "20", "Returns, pulling CCR and then PC."),
    ("rts", "-----", "16", "Returns from a subroutine."),
    ("sbcd", "*?*?*", "6 registers, 18 memory", "Subtracts a BCD byte and X. Z is only cleared."),
    ("st sf shi sls scc shs scs slo sne seq svc svs spl smi sge slt sgt sle", "-----", "Dn: 4 if false, 6 if true; memory 8 plus the address's time", "Sets a byte to $FF if the condition holds, or to 0 if not."),
    ("stop", "*****", "4", "Supervisor only: loads SR with a number and waits for an interrupt."),
    ("sub", "*****", "as ADD", "Subtracts the source from the destination."),
    ("suba", "-----", "as ADDA", "Subtracts from an address register, without changing the flags."),
    ("subi", "*****", "as ADDI", "Subtracts a number from the destination."),
    ("subq", "*****", "as ADDQ", "Subtracts 1 to 8 from the destination. From an address register it changes no flags."),
    ("subx", "*****", "as ADDX", "Subtracts the source and X from the destination, for values wider than 32 bits. Z is only cleared."),
    ("swap", "-**00", "4", "Swaps a data register's two words."),
    ("tas", "-**00", "Dn: 4; memory 10 plus the address's time", "Tests a byte and sets its top bit, in one indivisible read and write. (The Amiga's and the Mega Drive's memory don't allow it.)"),
    ("trap", "-----", "34", "Calls the system through one of 16 vectors, TRAP #0 to #15, in supervisor mode."),
    ("trapv", "-----", "4 if it doesn't trap", "Traps (through vector 7) if V is set."),
    ("tst", "-**00", "4 plus the address's time", "Sets N and Z by a value, leaving it alone; V and C are cleared."),
    ("unlk", "-----", "12", "Undoes LINK: SP becomes the address register, which is then pulled from the stack."),
];

fn table(cpu: Cpu) -> &'static [(&'static str, &'static str, &'static str, &'static str)] {
    match cpu {
        Cpu::Z80 => Z80,
        Cpu::M6502 => M6502,
        Cpu::M6809 => M6809,
        Cpu::M68k => M68K,
        Cpu::X86 | Cpu::Arm | Cpu::Sm83 | Cpu::Spc700 => &[],
    }
}

/// The 68000's sizes come off the end of a mnemonic, and its Bcc and DBcc come under their families.
fn base_mnemonic(cpu: Cpu, mnemonic: &str) -> String {
    let lower = mnemonic.to_ascii_lowercase();
    match cpu {
        Cpu::M68k => lower.split('.').next().unwrap_or(&lower).to_string(),
        _ => lower,
    }
}

/// What an instruction does, if there's a note for it.
pub fn about(cpu: Cpu, mnemonic: &str) -> Option<About> {
    let base = base_mnemonic(cpu, mnemonic);
    table(cpu).iter().find(|(names, ..)| names.split_whitespace().any(|n| n == base)).map(|(_, flags, cycles, text)| About { flags, cycles, text })
}

/// Every mnemonic the processor's assembler knows, for the list to choose from.
pub fn mnemonics(cpu: Cpu) -> Vec<String> {
    let isa = cpu.isa();
    let mut out: Vec<String> = crate::code_intel::CodeIntel::for_cpu(cpu).words.iter().filter(|w| isa.is_instruction(w)).cloned().collect();
    for (names, ..) in table(cpu) {
        for name in names.split_whitespace() {
            if isa.is_instruction(name) && !out.iter().any(|o| o == name) {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The instruction on a line of source: its first word that's one of the processor's instructions, after any
/// label. A comment or a string isn't looked in.
pub fn instruction_at(cpu: Cpu, line: &str) -> Option<String> {
    let isa = cpu.isa();
    let code = line.split(';').next().unwrap_or("");
    let code = if matches!(cpu, Cpu::M68k | Cpu::M6809) { code.split('*').next().unwrap_or("") } else { code };
    for word in code.split(|c: char| c.is_whitespace() || c == ':').filter(|w| !w.is_empty()).take(3) {
        let lower = word.to_ascii_lowercase();
        if isa.is_instruction(&lower) {
            return Some(lower);
        }
    }
    None
}

// ---------- Forms ----------

/// The shapes of operand tried, by processor. In each, {n} is a byte, {w} a word, {l} a long word, {d} a small
/// displacement, {e} a place to branch to, {z} a zero-page address, {p} the 6809's direct page and {a} a full
/// address.
fn templates(cpu: Cpu) -> &'static [&'static str] {
    match cpu {
        Cpu::Z80 => &[
            "", "a", "b", "(hl)", "(ix+{d})", "(iy+{d})", "{n}", "a,b", "a,{n}", "a,(hl)", "a,(ix+{d})", "b,a", "b,{n}", "(hl),a", "(hl),{n}", "(ix+{d}),a", "(ix+{d}),{n}", "a,({w})", "({w}),a", "a,(bc)", "a,(de)", "(bc),a", "(de),a", "a,i", "a,r", "i,a", "r,a",
            "bc,{w}", "de,{w}", "hl,{w}", "sp,{w}", "ix,{w}", "iy,{w}", "hl,({w})", "({w}),hl", "bc,({w})", "({w}),bc", "ix,({w})", "({w}),ix", "sp,hl", "sp,ix", "hl,bc", "hl,de", "hl,hl", "hl,sp", "ix,bc", "a,ixh", "ixh,{n}",
            "bc", "de", "hl", "af", "ix", "iy", "af,af'", "de,hl", "(sp),hl", "(sp),ix", "{w}", "nz,{w}", "z,{w}", "nc,{w}", "c,{w}", "po,{w}", "pe,{w}", "p,{w}", "m,{w}", "(ix)",
            "{e}", "nz,{e}", "z,{e}", "nc,{e}", "c,{e}", "nz", "z", "nc", "c", "po", "pe", "p", "m", "0", "1", "2", "$38", "0,a", "7,b", "7,(hl)", "0,(ix+{d})", "a,({n})", "({n}),a", "a,(c)", "b,(c)", "(c),a", "(c),b", "(c),0", "d,e", "de,b", "de,a", "{n},a", "{n},{n}",
        ],
        Cpu::M6502 => &["", "a", "#{n}", "{z}", "{z},x", "{z},y", "{a}", "{a},x", "{a},y", "({a})", "({z},x)", "({z}),y", "{e}"],
        Cpu::M6809 => &[
            "", "#{n}", "#{w}", "<{p}", "{a}", ",x", "{d},x", "{n},x", "{w},x", ",x+", ",x++", ",-x", ",--x", "a,x", "b,x", "d,x", "{d},y", ",s", "{d},s", "{w},pcr", "[,x]", "[{w},x]", "[{a}]", "{e}", "a,b", "x,y", "d,x", "a,b,x,y", "cc,a,b,dp,x,y,u,pc", "d,y",
        ],
        Cpu::M68k => &[
            "", "d0", "a0", "(a0)", "(a0)+", "-(a0)", "{w}(a0)", "{d}(a0,d1.w)", "{a}.w", "{a}", "{w}(pc)", "#{i}", "d0,d1", "d0,a1", "a0,d1", "a0,a1", "(a0),d1", "(a0)+,d1", "-(a0),d1", "{w}(a0),d1", "{d}(a0,d2.w),d1", "{a}.w,d1", "{a},d1", "{w}(pc),d1",
            "#{i},d1", "#{i},a1", "d1,(a0)", "d1,(a0)+", "d1,-(a0)", "d1,{w}(a0)", "d1,{a}", "#{i},(a0)", "#1,d1", "#1,(a0)", "(a0),a1", "{w}(a0),a1", "{a},a1", "{w}(pc),a1",
            "(a0)+,(a1)+", "-(a0),-(a1)", "d0,{e}", "{e}", "d0-d7/a0-a6,-(sp)", "(sp)+,d0-d7/a0-a6", "#{n},ccr", "#{w},sr", "sr,d0", "usp,a0", "a0,usp", "a6,#{w}", "a6", "#{t}",
            // The 68010's and 68020's own, shown when the 68000 hasn't the instruction.
            "ccr,d0", "vbr,d0", "d0,vbr", "(a0),d1:d2", "d0,d1:d2", "d0{4:8}", "(a0){4:8}", "(a0){4:8},d1", "d1,(a0){4:8}", "d0,d1,(a0)", "d0:d1,d2:d3,(a0):(a1)", "d0,d1,#{w}", "-(a0),-(a1),#{w}",
        ],
        Cpu::X86 => &[
            "", "al", "ax", "eax", "bx", "al,bl", "ax,bx", "eax,ebx", "al,{n}", "ax,{w}", "eax,{l}", "bx,{w}", "byte [bx]", "word [bx]", "dword [bx]", "[bx],al", "[bx],ax", "al,[bx]", "ax,[bx]", "byte [bx],{n}", "word [bx],{w}", "ax,[{w}]", "[{w}],ax",
            "{e}", "{n}", "al,{n}", "{n},al", "al,dx", "dx,al", "ax,dx", "al,1", "al,cl", "ax,1", "ax,cl", "ax,{n}", "ds", "es", "ds,ax", "ax,ds", "ax,bx,{n}", "ax,word [bx]", "{w},{n}", "si",
        ],
        Cpu::Arm => &[
            "r0,r1,r2", "r0,r1,#{n}", "r0,r1,r2,lsl #2", "r0,r1", "r0,#{n}", "r0,r1,lsl #2", "r0,[r1]", "r0,[r1,#{n}]", "r0,[r1,r2]", "r0,[r1],#{n}", "r0!,{r1-r3}", "r0,{r1-r3}", "sp!,{r0-r3,lr}", "{e}", "#{n}", "r0,r1,r2,r3", "r0,r1,[r2]",
            "", "p15,0,r0,c1,c0,0", "p1,c0,[r1]", "r0,{e}",
        ],
        // The Game Boy's, in RGBDS's brackets.
        Cpu::Sm83 => &[
            "", "a", "b", "[hl]", "{n}", "a,b", "a,{n}", "a,[hl]", "b,a", "b,{n}", "[hl],a", "[hl],{n}", "a,[{w}]", "[{w}],a", "a,[bc]", "a,[de]", "[bc],a", "[de],a", "a,[hl+]", "[hl+],a", "a,[hl-]", "[hl-],a", "a,[c]", "[c],a",
            "[{w}],sp", "bc,{w}", "de,{w}", "hl,{w}", "sp,{w}", "sp,hl", "hl,sp+{d}", "hl,bc", "sp,{d}", "bc", "de", "hl", "af", "{w}", "nz,{w}", "z,{w}", "nc,{w}", "c,{w}", "{e}", "nz,{e}", "z,{e}", "nc,{e}", "c,{e}", "nz", "z", "nc", "c", "0", "7,b", "7,[hl]", "$38",
        ],
        // The SNES's sound processor, in Sony's syntax.
        Cpu::Spc700 => &[
            "", "a", "x", "y", "a,#{n}", "a,{z}", "a,{z}+x", "a,{a}", "a,{a}+x", "a,{a}+y", "a,(x)", "a,({z}+x)", "a,({z})+y", "{z},a", "{a},a", "x,#{n}", "y,#{n}", "x,{z}", "y,{z}", "{z},#{n}", "{z},{z}", "ya,{z}", "{z},ya", "{a}", "{e}", "{z}", "{z}.1", "{z}.1,{e}",
        ],
    }
}

/// The numbers a placeholder becomes: the first and second tries, a wider one to see whether a narrow form was
/// only the wide one with a small number, and how it's shown.
fn placeholder(cpu: Cpu, name: &str) -> (&'static str, &'static str, Option<&'static str>, &'static str) {
    let hex = cpu != Cpu::X86;
    match name {
        "n" => if hex { ("$12", "$34", Some("$1234"), "n") } else { ("0x12", "0x34", Some("0x1234"), "n") },
        "w" => if hex { ("$1234", "$5678", Some("$12345678"), "nn") } else { ("0x1234", "0x5678", Some("0x12345678"), "nn") },
        "l" => if hex { ("$12345678", "$56789ABC", None, "nnnn") } else { ("0x12345678", "0x56789ABC", None, "nnnn") },
        "d" => ("5", "7", None, "d"),
        "t" => ("1", "2", None, "n"),
        "e" => if hex { ("$1010", "$1020", Some("$7000"), "label") } else { ("0x1010", "0x1020", Some("0x7000"), "label") },
        "z" => ("$12", "$34", Some("$1234"), "zp"),
        // The 6809's direct page, forced by <, which cuts a wider number down rather than refusing it.
        "p" => ("$12", "$34", None, "dp"),
        "a" => if cpu == Cpu::M68k { ("$12345678", "$56789ABC", None, "addr") } else if hex { ("$1234", "$5678", None, "addr") } else { ("0x1234", "0x5678", None, "addr") },
        _ => ("0", "0", None, ""),
    }
}

/// A template with its placeholders filled: by the first numbers, the second, the wider ones, and as it's shown.
/// `size` is a 68000 instruction's size (b, w or l), which its immediate number {i} is made to fit.
fn fill(cpu: Cpu, template: &str, size: Option<char>) -> (String, String, Option<String>, String) {
    let (mut first, mut second, mut wide, mut shown) = (String::new(), String::new(), String::new(), String::new());
    let mut has_wide = false;
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let name = &rest[open + 1..open + close];
        // {r1-r3} and the like are ARM register lists, not placeholders.
        if name.len() != 1 {
            for out in [&mut first, &mut second, &mut wide, &mut shown] {
                out.push_str(&rest[..open + close + 1]);
            }
            rest = &rest[open + close + 1..];
            continue;
        }
        // The 68000's assembler cuts a number down to the size rather than refusing it, so it's made to fit.
        let name = match (name, size) {
            ("i", Some('l')) => "l",
            ("i", Some('w')) => "w",
            ("i", _) => "n",
            (other, _) => other,
        };
        let (a, b, w, s) = placeholder(cpu, name);
        let w = if template.contains("{i}") { None } else { w };
        let before = &rest[..open];
        first.push_str(before);
        first.push_str(a);
        second.push_str(before);
        second.push_str(b);
        wide.push_str(before);
        wide.push_str(w.unwrap_or(a));
        has_wide |= w.is_some();
        shown.push_str(&before.to_uppercase());
        shown.push_str(s);
        rest = &rest[open + close + 1..];
    }
    first.push_str(rest);
    second.push_str(rest);
    wide.push_str(rest);
    shown.push_str(&rest.to_uppercase());
    (first, second, has_wide.then_some(wide), shown)
}

const ORIGIN: i64 = 0x1000;

/// An instruction's bytes, assembled at ORIGIN on its own; None if the assembler won't take it as it is.
fn assemble_one(cpu: Cpu, mnemonic: &str, operands: &str) -> Option<Vec<u8>> {
    let origin = if cpu == Cpu::X86 { "0x1000" } else { "$1000" };
    // A 68010's or 68020's own instruction is assembled for the 68020 (see `forms`).
    let machine = if cpu == Cpu::M68k && LATER_68K.with(std::cell::Cell::get) { "  machine 68020\n" } else { "" };
    let text = format!("{machine}  org {origin}\n  {mnemonic} {operands}\n");
    let read = |_: &Path| -> Result<Vec<u8>, String> { Err(String::new()) };
    let assembly = crate::asm::assemble(cpu.isa(), Path::new("reference.asm"), &text, &read);
    if !assembly.problems.is_empty() || !assembly.warnings.is_empty() {
        return None;
    }
    let line = if machine.is_empty() { 1 } else { 2 };
    let bytes: Vec<u8> = assembly.lines.iter().filter(|p| p.line == line).flat_map(|p| p.bytes.clone()).collect();
    (!bytes.is_empty()).then_some(bytes)
}

thread_local! {
    /// Whether 680x0 forms are being assembled for the 68020, for an instruction the 68000 hasn't got.
    static LATER_68K: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The forms an instruction has, with its bytes: a 680x0 instruction the 68000 hasn't got (MOVEC, BFEXTU) in its
/// 68020 forms, with no cycles, since those processors' times can't be given as one figure.
fn forms(cpu: Cpu, mnemonic: &str) -> Vec<Form> {
    let plain = forms_here(cpu, mnemonic);
    if !plain.is_empty() || cpu != Cpu::M68k {
        return plain;
    }
    LATER_68K.with(|later| later.set(true));
    let later = forms_here(cpu, mnemonic).into_iter().map(|f| Form { cycles: None, ..f }).collect();
    LATER_68K.with(|later| later.set(false));
    later
}

fn forms_here(cpu: Cpu, mnemonic: &str) -> Vec<Form> {
    let isa = cpu.isa();
    // A 68000 instruction with sizes is shown in each; one without, as it is.
    let mut sizes: Vec<String> = Vec::new();
    if cpu == Cpu::M68k && !mnemonic.contains('.') {
        // An instruction whose size changes nothing (MOVEQ) is shown without one: it has sizes only if some form
        // is made differently in two of them.
        let named: Vec<String> = [".b", ".w", ".l", ".s"].iter().map(|s| format!("{mnemonic}{s}")).filter(|m| isa.is_instruction(m)).collect();
        let sized = templates(cpu).iter().any(|t| {
            let made: Vec<Option<Vec<u8>>> = named.iter().map(|m| assemble_one(cpu, m, &fill(cpu, t, m.chars().last()).0)).collect();
            let made: Vec<&Vec<u8>> = made.iter().flatten().collect();
            made.windows(2).any(|pair| pair[0] != pair[1])
        });
        if sized {
            sizes = named;
        }
    }
    if sizes.is_empty() {
        sizes.push(mnemonic.to_string());
    }
    let mut out: Vec<Form> = Vec::new();
    for name in &sizes {
        for template in templates(cpu) {
            // A register list is MOVEM's alone, though the assembler takes MOVE with one as MOVEM.
            if template.contains("d0-d7") && !name.starts_with("movem") {
                continue;
            }
            let size = name.rsplit_once('.').and_then(|(_, s)| s.chars().next());
            let (first, second, wide, shown) = fill(cpu, template, size);
            let Some(a) = assemble_one(cpu, name, &first) else { continue };
            let Some(b) = assemble_one(cpu, name, &second) else { continue };
            if a.len() != b.len() {
                continue;
            }
            // A narrow number taken as a wide one: this is the wide form again.
            if let Some(wide) = wide
                && assemble_one(cpu, name, &wide).is_some_and(|w| w.len() == a.len())
            {
                continue;
            }
            let encoding: Vec<String> = a.iter().zip(&b).map(|(x, y)| if x == y { format!("{x:02X}") } else { "nn".to_string() }).collect();
            let encoding = encoding.join(" ");
            // Another way of writing a form already listed.
            if out.iter().any(|f| f.encoding == encoding) {
                continue;
            }
            let cycles = isa.cycles_at(&a, ORIGIN);
            let form = format!("{} {}", name.to_uppercase(), shown).trim().to_string();
            out.push(Form { form, encoding, cycles });
            if out.len() >= 100 {
                return out;
            }
        }
    }
    out
}

/// An instruction's reference, worked out once and kept.
pub fn reference(cpu: Cpu, mnemonic: &str) -> Option<Arc<Reference>> {
    static KEPT: Mutex<Option<HashMap<(Cpu, String), Arc<Reference>>>> = Mutex::new(None);
    let mnemonic = mnemonic.to_ascii_lowercase();
    if !cpu.isa().is_instruction(&mnemonic) {
        return None;
    }
    let key = (cpu, mnemonic.clone());
    if let Some(found) = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashMap::new).get(&key) {
        return Some(found.clone());
    }
    let made = Arc::new(Reference { cpu, about: about(cpu, &mnemonic), forms: forms(cpu, &mnemonic), mnemonic });
    KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(key, made.clone());
    Some(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form<'a>(reference: &'a Reference, shown: &str) -> &'a Form {
        reference.forms.iter().find(|f| f.form == shown).unwrap_or_else(|| panic!("no {shown} in {:?}", reference.forms.iter().map(|f| &f.form).collect::<Vec<_>>()))
    }

    #[test]
    fn z80_forms_come_from_the_assembler_with_their_cycles() {
        let ld = reference(Cpu::Z80, "ld").unwrap();
        assert_eq!(form(&ld, "LD A,B").encoding, "78");
        assert_eq!(form(&ld, "LD A,n").encoding, "3E nn");
        assert_eq!(form(&ld, "LD A,n").cycles.as_deref(), Some("7"));
        assert_eq!(form(&ld, "LD HL,nn").encoding, "21 nn nn");
        assert_eq!(form(&ld, "LD A,(IX+d)").encoding, "DD 7E nn");
        assert_eq!(form(&ld, "LD A,(IX+d)").cycles.as_deref(), Some("19"));
        assert!(ld.about.unwrap().text.contains("Copies"));
        let jp = reference(Cpu::Z80, "jp").unwrap();
        assert_eq!(form(&jp, "JP nn").encoding, "C3 nn nn");
        assert!(!jp.forms.iter().any(|f| f.form == "JP n"), "a byte taken for an address isn't a form of its own");
        let jr = reference(Cpu::Z80, "jr").unwrap();
        assert_eq!(form(&jr, "JR label").encoding, "18 nn");
        assert_eq!(form(&jr, "JR NZ,label").encoding, "20 nn");
        assert_eq!(about(Cpu::Z80, "ADD").unwrap().flags, "***V0*");
    }

    #[test]
    fn the_6502s_modes_and_cycles() {
        let lda = reference(Cpu::M6502, "lda").unwrap();
        let shown: Vec<(&str, &str)> = lda.forms.iter().map(|f| (f.form.as_str(), f.encoding.as_str())).collect();
        assert_eq!(shown, [("LDA #n", "A9 nn"), ("LDA zp", "A5 nn"), ("LDA zp,X", "B5 nn"), ("LDA addr", "AD nn nn"), ("LDA addr,X", "BD nn nn"), ("LDA addr,Y", "B9 nn nn"), ("LDA (zp,X)", "A1 nn"), ("LDA (zp),Y", "B1 nn")]);
        assert_eq!(form(&lda, "LDA #n").cycles.as_deref(), Some("2"));
        let jsr = reference(Cpu::M6502, "jsr").unwrap();
        assert_eq!(jsr.forms.len(), 1, "{:?}", jsr.forms);
        assert_eq!(form(&jsr, "JSR addr").cycles.as_deref(), Some("6"));
        let bne = reference(Cpu::M6502, "bne").unwrap();
        assert_eq!(form(&bne, "BNE label").encoding, "D0 nn");
        assert_eq!(about(Cpu::M6502, "lsr").unwrap().flags, "0---**");
    }

    #[test]
    fn the_6809_and_68000_have_their_forms_and_notes() {
        let lda = reference(Cpu::M6809, "lda").unwrap();
        assert_eq!(form(&lda, "LDA #n").encoding, "86 nn");
        assert_eq!(form(&lda, "LDA <dp").encoding, "96 nn");
        assert_eq!(form(&lda, "LDA addr").encoding, "B6 nn nn");
        assert!(lda.about.unwrap().cycles.contains("2 immediate"));
        let moveq = reference(Cpu::M68k, "moveq").unwrap();
        assert_eq!(form(&moveq, "MOVEQ #n,D1").encoding, "72 nn");
        assert!(moveq.forms.iter().all(|f| !f.form.contains('.')), "MOVEQ has no sizes");
        let mv = reference(Cpu::M68k, "move").unwrap();
        assert_eq!(form(&mv, "MOVE.W #nn,D1").encoding, "32 3C nn nn");
        assert_eq!(form(&mv, "MOVE.L #nnnn,D1").encoding, "22 3C nn nn nn nn");
        assert_eq!(form(&mv, "MOVE.B #n,D1").encoding, "12 3C 00 nn");
        assert!(!mv.forms.iter().any(|f| f.form == "MOVE.L #nn,D1" || f.form.starts_with("MOVE ")), "{:?}", mv.forms);
        assert_eq!(about(Cpu::M68k, "move.l").unwrap().flags, "-**00");
        assert_eq!(about(Cpu::M68k, "dbra").unwrap().flags, "-----");
    }

    #[test]
    fn x86_and_arm_show_their_forms() {
        let mov = reference(Cpu::X86, "mov").unwrap();
        assert_eq!(form(&mov, "MOV AX,nn").encoding, "B8 nn nn");
        let add = reference(Cpu::Arm, "add").unwrap();
        assert!(add.forms.iter().any(|f| f.form == "ADD R0,R1,R2"), "{:?}", add.forms);
        assert!(reference(Cpu::Z80, "notaninstruction").is_none());
    }

    #[test]
    fn the_instruction_on_a_line_is_found_after_its_label() {
        assert_eq!(instruction_at(Cpu::Z80, "loop:  ld a,(hl) ; ld b,c"), Some("ld".into()));
        assert_eq!(instruction_at(Cpu::Z80, "; djnz"), None);
        assert_eq!(instruction_at(Cpu::M68k, "start move.l d0,d1"), Some("move.l".into()));
        assert_eq!(instruction_at(Cpu::M6502, "        lda #$00"), Some("lda".into()));
    }

    #[test]
    fn every_note_has_a_flag_for_each_flag_and_names_real_instructions() {
        for cpu in [Cpu::Z80, Cpu::M6502, Cpu::M6809, Cpu::M68k] {
            let isa = cpu.isa();
            for (names, flags, _, text) in table(cpu) {
                assert_eq!(flags.chars().count(), flag_names(cpu).len(), "{names}");
                assert!(flags.chars().all(|c| !flag_meaning(c).is_empty()), "{names}: {flags}");
                assert!(!text.is_empty());
                for name in names.split_whitespace() {
                    assert!(isa.is_instruction(name) || cpu == Cpu::M68k && isa.is_instruction(&format!("{name}.w")), "{} has no {name}", cpu.name());
                }
            }
            // And every instruction of the documented set has a note.
            let missing: Vec<String> = mnemonics(cpu).into_iter().filter(|m| about(cpu, m).is_none()).collect();
            if cpu == Cpu::M6502 {
                assert!(missing.is_empty(), "{missing:?}");
            }
        }
    }
}
