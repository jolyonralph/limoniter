use super::*;
use std::path::Path;

fn assemble(source: &str) -> crate::asm::Assembly {
    crate::asm::assemble(&Arm, Path::new("test.s"), source, &|p| Err(format!("no {}", p.display())))
}

/// The words a line (or lines) makes.
fn words(source: &str) -> Vec<u32> {
    let assembly = assemble(source);
    assert!(assembly.ok(), "{source}: {:?}", assembly.problems);
    let bytes = assembly.image(assembly.lowest().unwrap_or(0), assembly.highest().unwrap_or(0));
    bytes.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
}

#[test]
fn instructions_make_the_words_the_arm_manual_gives() {
    for (line, word) in [
        (" mov r0,#1", 0xE3A0_0001u32),
        (" mov r0,#-1", 0xE3E0_0000),
        (" add r0,r0,#-4", 0xE240_0004),
        (" cmp r0,#10", 0xE350_000A),
        (" cmp r1,#-1", 0xE371_0001),
        (" and r2,r2,#&FFFFFF00", 0xE3C2_20FF),
        (" mov r0,#&FF000000", 0xE3A0_04FF),
        (" addeqs r1,r2,r3,lsl #2", 0x0092_1103),
        (" addseq r1,r2,r3,lsl #2", 0x0092_1103),
        (" movs pc,lr", 0xE1B0_F00E),
        (" teqp pc,#0", 0xE33F_F000),
        (" orr r0,r1,r2,ror r3", 0xE181_0372),
        (" mov r0,r1,rrx", 0xE1A0_0061),
        (" mov r0,r1,lsr #32", 0xE1A0_0021),
        (" ldr r0,[r1,#4]", 0xE591_0004),
        (" ldr r0,[r1]", 0xE591_0000),
        (" str r0,[r1],#4", 0xE481_0004),
        (" ldrb r2,[r3,-r4]!", 0xE773_2004),
        (" ldr r0,[r1,r2,lsl #2]", 0xE791_0102),
        (" strbt r0,[r1],#1", 0xE4E1_0001),
        (" ldreqb r0,[r1]", 0x05D1_0000),
        (" stmfd r13!,{r0-r3,r14}", 0xE92D_400F),
        (" ldmfd sp!,{r0-r3,pc}^", 0xE8FD_800F),
        (" ldmia r0,{r1,r2}", 0xE890_0006),
        (" mul r0,r1,r2", 0xE000_0291),
        (" mla r0,r1,r2,r3", 0xE020_3291),
        (" swp r0,r1,[r2]", 0xE102_0091),
        (" swpb r0,r1,[r2]", 0xE142_0091),
        (" swi \"OS_WriteC\"", 0xEF00_0000),
        (" swi XOS_Write0", 0xEF02_0002),
        (" swi \"OS_WriteI+65\"", 0xEF00_0141),
        (" swi &400C0", 0xEF04_00C0),
        (" mcr p15,0,r0,c2,c0,0", 0xEE02_0F10),
        (" nop", 0xE1A0_0000),
    ] {
        assert_eq!(words(line), [word], "{line}: {:08X}", words(line)[0]);
    }
}

#[test]
fn branches_adr_and_labels_reach_where_they_go() {
    let source = "        org &8000\nstart   b next\n        bl start\nnext    adr r0,message\n        ldr r1,value\n        swi \"OS_Write0\"\n        bne start\nmessage dcb \"Hi\",0\n        align\nvalue   dcd &12345678\n";
    let made = words(source);
    assert_eq!(made[0], 0xEA00_0000, "B to the word after the next: PC+8");
    assert_eq!(made[1], 0xEBFF_FFFD, "BL back to start");
    // message is at &8018: from &8008+8 that's 8 on.
    assert_eq!(made[2], 0xE28F_0008);
    // value is at &801C, from &800C+8 is 8 on.
    assert_eq!(made[3], 0xE59F_1008);
    assert_eq!(made[5], 0x1AFF_FFF9);
    assert_eq!(made[6] & 0x00FF_FFFF, 0x0000_6948, "Hi and its 0");
    assert_eq!(made[7], 0x1234_5678);
}

#[test]
fn what_an_instruction_can_t_hold_is_said() {
    for (line, said) in [(" mov r0,#&101", "can't hold"), (" ldr r0,[r1,#5000]", "4095"), (" teq r0", "operand"), (" ror r0,#3", "isn't")] {
        let assembly = assemble(line);
        assert!(assembly.problems.iter().any(|p| p.message.contains(said)), "{line}: {:?}", assembly.problems);
    }
    let assembly = assemble(" cpu arm2\n swp r0,r1,[r2]\n");
    assert!(assembly.problems.iter().any(|p| p.message.contains("ARM3")), "{:?}", assembly.problems);
    // MUL with its destination its first source warns, as the ARM2 gets it wrong.
    let assembly = assemble(" mul r0,r0,r1\n");
    assert!(!assembly.warnings.is_empty());
}
