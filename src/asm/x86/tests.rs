use super::*;
use iced_x86::{Decoder, DecoderOptions, Formatter, NasmFormatter};
use std::path::Path;

/// Assembles source (at ORG 0) with Limoniter's x86 assembler.
fn assemble(source: &str) -> crate::asm::Assembly {
    crate::asm::assemble(&X86, Path::new("test.asm"), source, &|p| Err(format!("no {}", p.display())))
}

fn bytes(source: &str) -> Vec<u8> {
    let assembly = assemble(source);
    assert!(assembly.ok(), "{source}: {:?}", assembly.problems);
    assembly.image(assembly.lowest().unwrap_or(0), assembly.highest().unwrap_or(0))
}

/// Each line assembled alone, decoded back by iced-x86 and written as NASM writes it: the same instruction.
fn round_trip(bits: u32, lines: &[&str]) {
    let mut formatter = NasmFormatter::new();
    formatter.options_mut().set_hex_prefix("0x");
    formatter.options_mut().set_hex_suffix("");
    formatter.options_mut().set_uppercase_hex(false);
    formatter.options_mut().set_space_after_operand_separator(false);
    formatter.options_mut().set_branch_leading_zeros(false);
    formatter.options_mut().set_leading_zeros(false);
    formatter.options_mut().set_small_hex_numbers_in_decimal(false);
    formatter.options_mut().set_show_branch_size(false);
    let mut wrong = Vec::new();
    for written in lines {
        // "source => how iced writes it", where the two say the same differently.
        let (line, expected) = written.split_once(" => ").unwrap_or((written, written));
        let source = format!("bits {bits}\n{line}\n");
        let made = bytes(&source);
        let mut decoder = Decoder::with_ip(bits, &made, 0, DecoderOptions::NONE);
        let instruction = decoder.decode();
        let mut text = String::new();
        formatter.format(&instruction, &mut text);
        if instruction.len() != made.len() || text != expected {
            wrong.push(format!("{line:40} -> {:02X?} -> {text} ({} of {} bytes)", made, instruction.len(), made.len()));
        }
    }
    assert!(wrong.is_empty(), "{} of {} wrong:\n{}", wrong.len(), lines.len(), wrong.join("\n"));
}

#[test]
fn the_8086_s_instructions_come_back_as_they_were_written() {
    round_trip(
        16,
        &[
            "mov ax,bx", "mov al,ah", "mov cx,[bx]", "mov [bx+si],dx", "mov [bp+di+0x10],al", "mov ax,[0x1234]", "mov [0x1234],al", "mov bx,[bp]", "mov si,[bx+0x1234]",
            "mov ax,0x1234", "mov cl,0x12", "mov byte [di],0x5", "mov word [bx+0x2],0x1234", "mov ds,ax", "mov ax,es", "mov word [bx],ss => mov [bx],ss", "mov es,[si]",
            "add ax,bx", "add al,0x12", "add ax,0x1234", "add bx,0x5", "add word [bx],0x7f", "add byte [si],0x80", "or cx,dx", "adc dl,[bx+di]", "sbb [bp+si+0x40],cx",
            "and al,0xf", "sub sp,0x100", "xor ax,ax", "cmp word [0x100],0x0", "cmp al,0x41",
            "inc ax", "dec di", "inc byte [bx]", "dec word [bp+0x4]", "neg ax", "not byte [si]", "mul bx", "imul cl", "div word [bx]", "idiv bl",
            "shl ax,1 => shl ax,0x1", "shr bl,cl", "rol word [bx],1 => rol word [bx],0x1", "sar dx,cl", "rcl al,1 => rcl al,0x1", "rcr si,cl",
            "push ax", "pop bx", "push es", "push cs", "pop ds", "push word [bx]", "pop word [si+0x2]",
            "xchg ax,bx => xchg bx,ax", "xchg cx,dx => xchg dx,cx", "xchg al,ah => xchg ah,al", "xchg [bx],ax => xchg ax,[bx]",
            "test al,0x1", "test ax,0x8000", "test bx,cx", "test byte [bx],0x80",
            "lea si,[bx+di+0x100]", "lds si,[bx]", "les di,[0x200]",
            "int 0x21", "int3", "into", "iret", "ret", "ret 0x4", "retf", "retf 0x2",
            "in al,0x60", "in ax,dx", "out 0x20,al", "out dx,ax",
            "movsb", "movsw", "cmpsb", "stosw", "lodsb", "scasw",
            "nop", "hlt", "cli", "sti", "cld", "std", "clc", "stc", "cmc", "cbw", "cwd", "pushf", "popf", "sahf", "lahf", "daa", "das", "aaa", "aas", "aam", "aad", "xlatb", "wait",
            "mov word [es:di],ax => mov [es:di],ax", "mov al,[cs:bx]", "mov ax,[ss:bp+0x2] => mov ax,[bp+0x2]",
        ],
    );
}

#[test]
fn the_186_and_286_s_instructions_come_back_as_they_were_written() {
    round_trip(16, &["push 0x5", "push 0x1234", "imul ax,bx,0x5", "imul cx,[bx],0x1234", "shl ax,0x4", "ror byte [bx],0x3", "pusha", "popa", "enter 0x10,0x0", "leave", "bound ax,[bx]", "insb", "outsw"]);
    round_trip(16, &["lgdt [0x500]", "lidt [bx]", "sgdt [si]", "smsw ax", "lmsw ax", "lldt ax", "ltr bx", "str cx", "verr ax", "verw word [bx] => verw [bx]", "lar ax,bx", "lsl cx,[si]", "arpl ax,bx", "clts"]);
}

#[test]
fn the_386_s_instructions_come_back_as_they_were_written() {
    round_trip(
        32,
        &[
            "mov eax,ebx", "mov eax,[ebx]", "mov [esp],eax", "mov ecx,[ebp]", "mov edx,[eax+ecx*4]", "mov esi,[ebx+edi*2+0x10]", "mov edi,[ecx*8+0x1000]", "mov eax,[0x12345678]", "mov [ebp-0x4],ebx",
            "mov ax,[esi]", "mov al,[edi+0x7f]", "mov eax,0x12345678", "add eax,0x1", "add ebx,0x12345", "sub esp,0x20", "and dword [eax],0xff",
            "movzx eax,byte [ebx]", "movzx ecx,dx", "movsx eax,al", "movsx edx,word [esi]",
            "bt eax,ebx", "bts dword [ecx],0x5", "btr eax,0x1f", "btc [edx],eax", "bsf eax,ebx", "bsr ecx,[esi]",
            "shld eax,ebx,0x4", "shrd edx,eax,cl", "sete al", "setne byte [ebx] => setne [ebx]", "setg cl",
            "push eax", "pop esi", "push fs", "pop gs", "push 0x12345678", "imul eax,ebx", "imul ecx,[esi]",
            "mov cr0,eax", "mov eax,cr0", "mov dr7,ebx", "mov fs,ax => mov fs,eax", "mov eax,[fs:ebx]",
            "lss esp,[eax]", "lfs ebx,[ecx]", "cdq", "cwde", "pushad => pusha", "popad => popa", "pushfd => pushf", "iretd => iret", "movsd", "stosd",
            "bswap eax", "xadd [ebx],ecx", "cmpxchg ecx,edx", "invlpg [eax]", "cpuid",
        ],
    );
    // In 16-bit code, 32-bit registers and addresses take the size prefixes.
    round_trip(16, &["mov eax,ebx", "mov eax,[ebx]", "mov ax,[ecx+edx*2]", "push eax", "movzx eax,al", "add eax,0x10"]);
}

#[test]
fn nasm_s_own_choices_of_encoding_are_kept() {
    // The accumulator's short forms, a sign-extended byte where it fits, [bp] with a zero byte, XCHG with AX.
    assert_eq!(bytes("add al,5"), [0x04, 0x05]);
    assert_eq!(bytes("add ax,5"), [0x83, 0xC0, 0x05]);
    assert_eq!(bytes("add ax,1000"), [0x05, 0xE8, 0x03]);
    assert_eq!(bytes("mov ax,[0x1234]"), [0xA1, 0x34, 0x12]);
    assert_eq!(bytes("mov ax,[bp]"), [0x8B, 0x46, 0x00]);
    assert_eq!(bytes("xchg bx,ax"), [0x93]);
    assert_eq!(bytes("int 3"), [0xCD, 0x03]);
    assert_eq!(bytes("shl ax,1"), [0xD1, 0xE0]);
    assert_eq!(bytes("push 5"), [0x6A, 0x05]);
    assert_eq!(bytes("rep movsb"), [0xF3, 0xA4]);
    assert_eq!(bytes("lock inc word [bx]"), [0xF0, 0xFF, 0x07]);
    assert_eq!(bytes("bits 32\nmov eax,[esp]"), [0x8B, 0x04, 0x24]);
    assert_eq!(bytes("bits 32\nmov eax,[eax*2]"), [0x8B, 0x04, 0x00]);
    assert_eq!(bytes("bits 32\nmov eax,[ebp]"), [0x8B, 0x45, 0x00]);
    assert_eq!(bytes("mov eax,[es:ebx]"), [0x26, 0x66, 0x67, 0x8B, 0x03]);
}

#[test]
fn jumps_are_short_where_they_reach_and_near_where_they_don_t() {
    let source = "org 0x100\nstart: jmp next\n times 10 nop\nnext: jz start\n jmp far_away\n times 200 nop\nfar_away: loop far_away\n call start\n jmp 0x1234:0x5678\n";
    let made = bytes(source);
    assert_eq!(&made[..2], &[0xEB, 0x0A], "a short JMP over 10 NOPs");
    assert_eq!(&made[12..14], &[0x74, 0xF2], "JZ back to start, short");
    assert_eq!(made[14], 0xE9, "a near JMP over 200 bytes");
    assert_eq!(u16::from_le_bytes([made[15], made[16]]), 200);
    let after = 17 + 200;
    assert_eq!(made[after], 0xE2, "LOOP");
    assert_eq!(made[after + 2], 0xE8, "CALL is near");
    assert_eq!(&made[after + 5..], &[0xEA, 0x78, 0x56, 0x34, 0x12]);
    // A conditional jump too far for a byte: near on a 386, refused on an 8086.
    let far = "start: jz there\n times 300 nop\nthere: ret\n";
    assert_eq!(&bytes(far)[..2], &[0x0F, 0x84]);
    let assembly = assemble(&format!("cpu 8086\n{far}"));
    assert!(assembly.problems.iter().any(|p| p.message.contains("near conditional jumps need a 386")), "{:?}", assembly.problems);
}

#[test]
fn a_boot_sector_fills_to_510_bytes_and_ends_55aa_and_too_big_says_so() {
    let source = "bits 16\norg 0x7c00\nstart: mov si,message\n jmp $\nmessage: db 'Hello',0\n times 510-($-$$) db 0\n dw 0xaa55\n";
    let made = bytes(source);
    assert_eq!(made.len(), 512);
    assert_eq!(&made[..3], &[0xBE, 0x05, 0x7C], "MOV SI,message at 0x7C05");
    assert_eq!(&made[3..5], &[0xEB, 0xFE], "JMP $");
    assert_eq!(&made[510..], &[0x55, 0xAA]);
    let big = "org 0x7c00\n times 600 nop\n times 510-($-$$) db 0\n dw 0xaa55\n";
    let assembly = assemble(big);
    assert!(assembly.problems.iter().any(|p| p.message.contains("90 bytes too long")), "{:?}", assembly.problems);
}

#[test]
fn an_instruction_newer_than_the_cpu_says_so_and_nasm_s_directives_work() {
    let assembly = assemble("cpu 8086\npush 5\n");
    assert!(assembly.problems.iter().any(|p| p.message.contains("needs a 186")), "{:?}", assembly.problems);
    let assembly = assemble("cpu 286\nmov eax,ebx\n");
    assert!(assembly.problems.iter().any(|p| p.message.contains("needs a 386")), "{:?}", assembly.problems);
    // [BITS 32] in brackets, RESB, DQ, ALIGN with NOPs, local labels, a .COM program's ORG.
    let made = bytes("[bits 32]\nnop\nalign 4\nresb 2\ndq 1\n");
    assert_eq!(made, [0x90, 0x90, 0x90, 0x90, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    let made = bytes("org 100h\nmain:\n.again: dec cx\n jnz .again\n mov ah,4ch\n int 21h\n");
    assert_eq!(made, [0x49, 0x75, 0xFD, 0xB4, 0x4C, 0xCD, 0x21]);
    // No size, and wrong ones, are said.
    assert!(!assemble("inc [bx]\n").ok());
    assert!(!assemble("mov ax,bl\n").ok());
    assert!(!assemble("mov ax,[bx+cx]\n").ok());
}

#[test]
fn the_x87_pentium_mmx_and_sse_s_instructions_come_back_as_they_were_written() {
    round_trip(16, &["fld dword [bx]", "fld qword [bx+si]", "fld tword [0x100]", "fild word [bx]", "fistp qword [di]", "fadd st,st(1) => fadd st1", "fsqrt", "fxch st(1) => fxch", "fstp st(2) => fstp st2", "fldpi", "fninit", "fnstsw ax"]);
    round_trip(32, &["rdtsc", "cmpxchg8b [ebx]", "cmovne eax,ebx", "cmovg ecx,[esi]", "movq mm0,[esi]", "paddb mm0,mm1", "emms", "movaps xmm0,[eax]", "addps xmm0,xmm1", "xorps xmm2,xmm2", "pxor xmm1,[ebx+0x10]", "sysenter", "ud2"]);
    // NASM's shorter forms of the x87's: FADD ST1 is FADD ST0,ST1, and FADDP alone pops into ST1.
    assert_eq!(bytes("fadd st1"), bytes("fadd st0,st1"));
    assert_eq!(bytes("faddp"), [0xDE, 0xC1]);
    // A memory operand whose size could be any of several says so.
    let assembly = assemble("fld [bx]\n");
    assert!(assembly.problems.iter().any(|p| p.message.contains("DWORD, QWORD, TWORD")), "{:?}", assembly.problems);
    // And an instruction newer than the CPU chosen.
    let assembly = assemble("cpu 486\nbits 32\ncmovne eax,ebx\n");
    assert!(assembly.problems.iter().any(|p| p.message.contains("needs a Pentium Pro")), "{:?}", assembly.problems);
    let assembly = assemble("cpu 386\nbits 32\nrdtsc\n");
    assert!(assembly.problems.iter().any(|p| p.message.contains("needs a Pentium")), "{:?}", assembly.problems);
}
