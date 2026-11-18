//! Starting points for the PC, in NASM's syntax: a DOS .COM program and a boot sector. F9 assembles either and
//! runs it in DOSBox (or boots the floppy in QEMU).

/// A DOS .COM program that says hello and waits for a key.
pub const COM: &str = "; A DOS .COM program, for the 8086 and every PC since. F9 runs it in DOSBox.
        cpu 8086
        org 100h                ; DOS loads a .COM at 100h

start:  mov dx,message
        mov ah,9                ; DOS: print the text up to a $
        int 21h
        mov ah,0                ; the BIOS: wait for a key
        int 16h
        mov ax,4c00h            ; DOS: back to the prompt
        int 21h

message db 'Hello from Limoniter',13,10,'$'
";

/// A boot sector: the BIOS loads it at 7C00h and runs it, with nothing else on the disk.
pub const BOOT: &str = "; A PC boot sector: the BIOS loads these 512 bytes at 7C00h and jumps to them.
; F9 puts it on a floppy that boots, in DOSBox or QEMU.
        bits 16
        cpu 8086
        org 7c00h

start:  xor ax,ax
        mov ds,ax               ; the message is addressed from segment 0
        mov si,message
.next:  lodsb
        or al,al                ; the end of the message
        jz .done
        mov ah,0eh              ; the BIOS: write a character
        int 10h
        jmp .next
.done:  hlt
        jmp .done

message db 'Booted by Limoniter',13,10,0

        times 510-($-$$) db 0   ; the rest of the sector
        dw 0aa55h               ; the mark that it boots
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starters_assemble_into_a_com_program_and_a_boot_sector() {
        let isa = crate::asm::x86::X86;
        let com = crate::asm::assemble(&isa, std::path::Path::new("hello.asm"), COM, &|_| Err(String::new()));
        assert!(com.ok(), "{:?}", com.problems);
        assert_eq!(com.lowest(), Some(0x100));
        let boot = crate::asm::assemble(&isa, std::path::Path::new("boot.asm"), BOOT, &|_| Err(String::new()));
        assert!(boot.ok(), "{:?}", boot.problems);
        let bytes = boot.image(0x7C00, boot.highest().unwrap());
        assert_eq!(bytes.len(), 512);
        assert_eq!(&bytes[510..], &[0x55, 0xAA]);
    }
}
