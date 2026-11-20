//! The Vectrex: its cartridges' header, the names of its BIOS routines and its hardware's addresses (as the
//! VECTREX.I that Vectrex programmers include names them), and a starter cartridge.
//!
//! A cartridge starts at address 0 with a header the BIOS reads before it runs anything: the copyright line, "g GCE"
//! and a year, ended by $80 (the BIOS won't start a cartridge without it); where its tune is; then the title, one or
//! more lines each of a height, a width, a place (y, x) and the text, ended by $80; and a 0 to end the header. The
//! program starts on the byte after.

/// The BIOS's routines and its RAM's and VIA's places, by name.
pub const BIOS: &[(&str, u16)] = &[
    ("Cold_Start", 0xF000),
    ("Warm_Start", 0xF06C),
    ("Init_VIA", 0xF14C),
    ("Init_OS_RAM", 0xF164),
    ("Init_OS", 0xF18B),
    ("Wait_Recal", 0xF192),
    ("Set_Refresh", 0xF1A2),
    ("DP_to_D0", 0xF1AA),
    ("DP_to_C8", 0xF1AF),
    ("Read_Btns_Mask", 0xF1B4),
    ("Read_Btns", 0xF1BA),
    ("Joy_Analog", 0xF1F5),
    ("Joy_Digital", 0xF1F8),
    ("Sound_Byte", 0xF256),
    ("Sound_Byte_x", 0xF259),
    ("Sound_Byte_raw", 0xF25B),
    ("Clear_Sound", 0xF272),
    ("Sound_Bytes", 0xF27D),
    ("Sound_Bytes_x", 0xF284),
    ("Do_Sound", 0xF289),
    ("Do_Sound_x", 0xF28C),
    ("Intensity_1F", 0xF29D),
    ("Intensity_3F", 0xF2A1),
    ("Intensity_5F", 0xF2A5),
    ("Intensity_7F", 0xF2A9),
    ("Intensity_a", 0xF2AB),
    ("Dot_ix_b", 0xF2BE),
    ("Dot_ix", 0xF2C1),
    ("Dot_d", 0xF2C3),
    ("Dot_here", 0xF2C5),
    ("Dot_List", 0xF2D5),
    ("Dot_List_Reset", 0xF2DE),
    ("Recalibrate", 0xF2E6),
    ("Moveto_x_7F", 0xF2F2),
    ("Moveto_d_7F", 0xF2FC),
    ("Moveto_ix_FF", 0xF308),
    ("Moveto_ix_7F", 0xF30C),
    ("Moveto_ix_b", 0xF30E),
    ("Moveto_ix", 0xF310),
    ("Moveto_d", 0xF312),
    ("Reset0Ref_D0", 0xF34A),
    ("Check0Ref", 0xF34F),
    ("Reset0Ref", 0xF354),
    ("Reset_Pen", 0xF35B),
    ("Reset0Int", 0xF36B),
    ("Print_Str_hwyx", 0xF373),
    ("Print_Str_yx", 0xF378),
    ("Print_Str_d", 0xF37A),
    ("Print_List_hw", 0xF385),
    ("Print_List", 0xF38A),
    ("Print_List_chk", 0xF38C),
    ("Print_Ships_x", 0xF391),
    ("Print_Ships", 0xF393),
    ("Mov_Draw_VLc_a", 0xF3AD),
    ("Mov_Draw_VL_b", 0xF3B1),
    ("Mov_Draw_VLcs", 0xF3B5),
    ("Mov_Draw_VL_ab", 0xF3B7),
    ("Mov_Draw_VL_a", 0xF3B9),
    ("Mov_Draw_VL", 0xF3BC),
    ("Mov_Draw_VL_d", 0xF3BE),
    ("Draw_VLc", 0xF3CE),
    ("Draw_VL_b", 0xF3D2),
    ("Draw_VLcs", 0xF3D6),
    ("Draw_VL_ab", 0xF3D8),
    ("Draw_VL_a", 0xF3DA),
    ("Draw_VL", 0xF3DD),
    ("Draw_Line_d", 0xF3DF),
    ("Draw_VLp_FF", 0xF404),
    ("Draw_VLp_7F", 0xF408),
    ("Draw_VLp_scale", 0xF40C),
    ("Draw_VLp_b", 0xF40E),
    ("Draw_VLp", 0xF410),
    ("Draw_Pat_VL_a", 0xF434),
    ("Draw_Pat_VL", 0xF437),
    ("Draw_Pat_VL_d", 0xF439),
    ("Draw_VL_mode", 0xF46E),
    ("Print_Str", 0xF495),
    ("Random_3", 0xF511),
    ("Random", 0xF517),
    ("Init_Music_Buf", 0xF533),
    ("Clear_x_b", 0xF53F),
    ("Clear_C8_RAM", 0xF542),
    ("Clear_x_256", 0xF545),
    ("Clear_x_d", 0xF548),
    ("Clear_x_b_80", 0xF550),
    ("Clear_x_b_a", 0xF552),
    ("Dec_3_Counters", 0xF55A),
    ("Dec_6_Counters", 0xF55E),
    ("Dec_Counters", 0xF563),
    ("Delay_3", 0xF56D),
    ("Delay_2", 0xF571),
    ("Delay_1", 0xF575),
    ("Delay_0", 0xF579),
    ("Delay_b", 0xF57A),
    ("Delay_RTS", 0xF57D),
    ("Bitmask_a", 0xF57E),
    ("Abs_a_b", 0xF584),
    ("Abs_b", 0xF58B),
    ("Rise_Run_Angle", 0xF593),
    ("Rot_VL_ab", 0xF610),
    ("Rot_VL", 0xF616),
    ("Rot_VL_Mode", 0xF61F),
    ("Rot_VL_M_dft", 0xF62B),
    ("Move_Mem_a_1", 0xF67F),
    ("Move_Mem_a", 0xF683),
    ("Init_Music_chk", 0xF687),
    ("Init_Music", 0xF68D),
    ("Init_Music_x", 0xF692),
    ("Select_Game", 0xF7A9),
    ("Clear_Score", 0xF84F),
    ("Add_Score_a", 0xF85E),
    ("Add_Score_d", 0xF87C),
    ("Strip_Zeros", 0xF8B7),
    ("Compare_Score", 0xF8C7),
    ("New_High_Score", 0xF8D8),
    ("Obj_Will_Hit_u", 0xF8E5),
    ("Obj_Will_Hit", 0xF8F3),
    ("Obj_Hit", 0xF8FF),
    ("Explosion_Snd", 0xF92E),
    ("Draw_Grid_VL", 0xFF9F),
    // The BIOS's tunes, for a cartridge's header.
    ("music1", 0xFD0D),
    ("music2", 0xFD1D),
    ("music3", 0xFD81),
    ("music4", 0xFDD3),
    ("music5", 0xFE38),
    ("music6", 0xFE76),
    ("music7", 0xFEC6),
    ("music8", 0xFEF8),
    ("music9", 0xFF26),
    ("musica", 0xFF44),
    ("musicb", 0xFF62),
    ("musicc", 0xFF7A),
    ("musicd", 0xFF8F),
    // The BIOS's own RAM.
    ("Vec_Snd_Shadow", 0xC800),
    ("Vec_Btn_State", 0xC80F),
    ("Vec_Prev_Btns", 0xC810),
    ("Vec_Buttons", 0xC811),
    ("Vec_Joy_Resltn", 0xC81A),
    ("Vec_Joy_1_X", 0xC81B),
    ("Vec_Joy_1_Y", 0xC81C),
    ("Vec_Joy_2_X", 0xC81D),
    ("Vec_Joy_2_Y", 0xC81E),
    ("Vec_Joy_Mux", 0xC81F),
    ("Vec_Loop_Count", 0xC825),
    ("Vec_Brightness", 0xC827),
    ("Vec_Text_HW", 0xC82A),
    ("Vec_Text_Height", 0xC82A),
    ("Vec_Text_Width", 0xC82B),
    ("Vec_Music_Flag", 0xC856),
    // The 6522 VIA.
    ("VIA_port_b", 0xD000),
    ("VIA_port_a", 0xD001),
    ("VIA_DDR_b", 0xD002),
    ("VIA_DDR_a", 0xD003),
    ("VIA_t1_cnt_lo", 0xD004),
    ("VIA_t1_cnt_hi", 0xD005),
    ("VIA_t1_lch_lo", 0xD006),
    ("VIA_t1_lch_hi", 0xD007),
    ("VIA_t2_lo", 0xD008),
    ("VIA_t2_hi", 0xD009),
    ("VIA_shift_reg", 0xD00A),
    ("VIA_aux_cntl", 0xD00B),
    ("VIA_cntl", 0xD00C),
    ("VIA_int_flags", 0xD00D),
    ("VIA_int_enable", 0xD00E),
    ("VIA_port_a_nohs", 0xD00F),
];

/// The name of the BIOS routine (or place) at an address, the first given for it.
pub fn name_at(address: u16) -> Option<&'static str> {
    BIOS.iter().find(|(_, a)| *a == address).map(|(n, _)| *n)
}

/// Whether bytes are a Vectrex cartridge: they start with the copyright line the BIOS checks.
pub fn is_cartridge(bytes: &[u8]) -> bool {
    bytes.starts_with(b"g GCE")
}

/// What a cartridge's header says: its copyright line, its tune's address, its title's lines, and where the
/// program starts, after the header. None if it isn't one.
pub fn header(bytes: &[u8]) -> Option<(String, u16, Vec<String>, usize)> {
    if !is_cartridge(bytes) {
        return None;
    }
    let text_until_80 = |from: usize| -> Option<(String, usize)> {
        let end = from + bytes.get(from..)?.iter().position(|b| *b == 0x80)?;
        Some((bytes[from..end].iter().map(|b| *b as char).collect(), end + 1))
    };
    let (copyright, at) = text_until_80(0)?;
    let music = u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]);
    let mut at = at + 2;
    let mut title = Vec::new();
    // Each line: height, width, y, x, then the text up to $80; a 0 where a line would start ends the header.
    while *bytes.get(at)? != 0 {
        let (line, next) = text_until_80(at + 4)?;
        title.push(line);
        at = next;
        if title.len() > 8 {
            return None;
        }
    }
    Some((copyright, music, title, at + 1))
}

/// Names the BIOS's routines a disassembled cartridge calls and jumps to (JSR, JMP and the like, to an address
/// written out), with an EQU for each at the top.
pub fn name_bios_calls(source: &str) -> String {
    let mut used: Vec<(&str, u16)> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for line in source.lines() {
        let mut out = line.to_string();
        for pattern in [" >$", " $"] {
            if let Some(at) = out.find(pattern) {
                let digits: String = out[at + pattern.len()..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                let word = out[..at].split_whitespace().last().unwrap_or("").to_ascii_lowercase();
                if digits.len() == 4
                    && matches!(word.as_str(), "jsr" | "jmp" | "lda" | "ldb" | "sta" | "stb" | "ldd" | "std" | "ldx" | "ldu" | "ldy")
                    && let Ok(address) = u16::from_str_radix(&digits, 16)
                    && address >= 0xC800
                    && let Some(name) = name_at(address)
                {
                    out = format!("{}{}{}", &out[..at + 1], name, &out[at + pattern.len() + 4..]);
                    if !used.iter().any(|(n, _)| *n == name) {
                        used.push((name, address));
                    }
                    break;
                }
            }
        }
        lines.push(out);
    }
    used.sort_by_key(|(_, a)| *a);
    let mut head: Vec<String> = vec!["; The Vectrex BIOS's routines this calls, as VECTREX.I names them.".to_string()];
    head.extend(used.iter().map(|(name, address)| format!("{name:<16}equ ${address:04X}")));
    head.push(String::new());
    if used.is_empty() {
        return source.to_string();
    }
    format!("{}\n{}\n", head.join("\n"), lines.join("\n"))
}

/// The starter cartridge: the header, then a loop that waits for the frame, sets the brightness and draws a
/// square and a line of text, as Vectrex programs do.
pub const STARTER: &str = r#"; machine:vectrex
; A Vectrex cartridge. F9 runs it in MAME (with the Vectrex's ROM) or ParaJVE.
; The BIOS's routines, as VECTREX.I names them.
Wait_Recal      equ $F192
Intensity_5F    equ $F2A5
Intensity_7F    equ $F2A9
Reset0Ref       equ $F354
Moveto_d        equ $F312
Draw_VLc        equ $F3CE
Print_Str_d     equ $F37A
music1          equ $FD0D

        org 0
; The header: the copyright line the BIOS looks for, the tune played at the start, and the title.
        fcc "g GCE 1982"
        fcb $80
        fdb music1
        fcb $F8,$50,$20,-$40    ; height, width, y, x
        fcc "LIMONITER"
        fcb $80
        fcb 0                   ; the header ends

main:
        jsr Wait_Recal          ; once a frame: wait, and put the beam back in the middle
        jsr Intensity_5F
        ldd #$5080              ; y, x
        ldu #message
        jsr Print_Str_d
        jsr Intensity_7F
        jsr Reset0Ref
        ldd #$E0E0              ; to the square's corner
        jsr Moveto_d
        ldx #square
        jsr Draw_VLc
        bra main

message:
        fcc "HELLO VECTREX"
        fcb $80

; A vector list: how many lines, less one, then each as y, x.
square:
        fcb 3
        fcb $40,$00
        fcb $00,$40
        fcb -$40,$00
        fcb $00,-$40
"#;

/// The Dragon 32 starter: a program that LOADM (or F9, in XRoar) runs, printing with the ROM's character output.
pub const DRAGON_STARTER: &str = r#"; machine:dragon32
; A Dragon 32 program. F9 runs it in XRoar; Tools, Dragon and Vectrex, Dragon DOS disk puts it on a disk.
; To run it from a Dragon DOS disk, with RUN typed for it, add a comment saying run from: disk.
OUTCHR  equ $A002               ; the ROM's vector to print the character in A

        org $3000
start:
        ldx #message
next:
        lda ,x+
        beq done
        jsr [OUTCHR]
        bra next
done:
        rts

message:
        fcc "HELLO FROM THE DRAGON"
        fcb 13,0

        end start
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starter_assembles_to_a_cartridge_the_bios_takes() {
        let assembly = crate::asm::assemble(&crate::asm::m6809::M6809, std::path::Path::new("v.asm"), STARTER, &|_| Err("no".into()));
        assert!(assembly.problems.is_empty(), "{:?}", assembly.problems);
        let image = assembly.image(0, assembly.highest().unwrap());
        let (copyright, music, title, code) = header(&image).unwrap();
        assert_eq!((copyright.as_str(), music, title.as_slice()), ("g GCE 1982", 0xFD0D, &["LIMONITER".to_string()][..]));
        // The code starts after the header: JSR Wait_Recal.
        assert_eq!(&image[code..code + 3], &[0xBD, 0xF1, 0x92]);
        // Known for what it is, as a file.
        let run = crate::running::Runnable::from_file("v.vec", image).unwrap();
        assert_eq!(run.machine, crate::running::Machine::Vectrex);
    }

    #[test]
    fn the_dragon_starter_assembles() {
        let assembly = crate::asm::assemble(&crate::asm::m6809::M6809, std::path::Path::new("d.asm"), DRAGON_STARTER, &|_| Err("no".into()));
        assert!(assembly.problems.is_empty(), "{:?}", assembly.problems);
        assert_eq!(assembly.start(), Some(0x3000));
    }

    #[test]
    fn bios_calls_are_named_in_a_disassembly() {
        let named = name_bios_calls("main    jsr >$F192\n        ldd #$5080\n        jsr $F37A\n        jsr >$1234\n");
        assert!(named.contains("Wait_Recal      equ $F192"), "{named}");
        assert!(named.contains("jsr Wait_Recal") && named.contains("jsr Print_Str_d") && named.contains("jsr >$1234"), "{named}");
    }
}
