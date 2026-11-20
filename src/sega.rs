//! Sega's consoles and their neighbours: the Mega Drive (Genesis), the Master System and Game Gear, the SG-1000,
//! and the ColecoVision. Their cartridges' headers are written and their checksums fixed, as the consoles (or
//! their BIOSes) check them, and each has a starter program.
//!
//! The headers are as SMS Power's and Plutiedev's documents describe them, checked against cartridges in MAME.

/// Where the Mega Drive's header is, and its parts.
const MD_HEADER: usize = 0x100;
const MD_CHECKSUM: usize = 0x18E;
const MD_ROM_END: usize = 0x1A4;

/// Whether bytes are a Mega Drive cartridge: its header, from $100, starts with SEGA (after a space, on some).
pub fn is_mega_drive(bytes: &[u8]) -> bool {
    bytes.len() >= 0x200 && (bytes[MD_HEADER..].starts_with(b"SEGA") || bytes[MD_HEADER..].starts_with(b" SEGA"))
}

/// A Mega Drive cartridge's checksum: the sum of its words from $200 to its end. The BIOS doesn't check it, but
/// many games do (and show a red screen when it's wrong), and emulators warn about it.
pub fn mega_drive_checksum(bytes: &[u8]) -> u16 {
    bytes.get(0x200..).unwrap_or_default().chunks(2).fold(0u16, |sum, word| sum.wrapping_add(u16::from_be_bytes([word[0], *word.get(1).unwrap_or(&0)])))
}

/// A Mega Drive cartridge made ready to run: an even length, its header's ROM end set to its last byte and its
/// checksum fixed.
pub fn fix_mega_drive(mut bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !is_mega_drive(&bytes) {
        return Err(crate::i18n::tr("A Mega Drive cartridge has its header at $100, starting with \"SEGA MEGA DRIVE \" or \"SEGA GENESIS    \", or the console won't run it.").into());
    }
    if bytes.len() % 2 == 1 {
        bytes.push(0);
    }
    let end = (bytes.len() - 1) as u32;
    bytes[MD_ROM_END..MD_ROM_END + 4].copy_from_slice(&end.to_be_bytes());
    let sum = mega_drive_checksum(&bytes);
    bytes[MD_CHECKSUM..MD_CHECKSUM + 2].copy_from_slice(&sum.to_be_bytes());
    Ok(bytes)
}

/// Where the Master System's and Game Gear's header goes: the export BIOS looks at $1FF0, $3FF0 and $7FF0, and
/// $7FF0 is where every cartridge of 32K or more has it.
const SMS_HEADER: usize = 0x7FF0;

/// The Master System's or Game Gear's header, if bytes have one: where it is, and its region code (3 and 4 for
/// the Master System, 5 to 7 for the Game Gear).
pub fn sms_header(bytes: &[u8]) -> Option<(usize, u8)> {
    [0x7FF0, 0x3FF0, 0x1FF0].into_iter().find(|at| bytes.get(*at..*at + 8) == Some(b"TMR SEGA")).map(|at| (at, bytes[at + 15] >> 4))
}

/// The header's size code for a cartridge of this many bytes, and how many bytes the checksum covers.
fn sms_size_code(len: usize) -> (u8, usize) {
    match len {
        0..=0x8000 => (0xC, 0x8000),
        0x8001..=0x10000 => (0xE, 0x10000),
        0x10001..=0x20000 => (0xF, 0x20000),
        0x20001..=0x40000 => (0x0, 0x40000),
        0x40001..=0x80000 => (0x1, 0x80000),
        _ => (0x2, 0x100000),
    }
}

/// The checksum a Master System cartridge's header holds: the sum of its bytes, but for the header's own 16, up
/// to the size its header gives.
pub fn sms_checksum(bytes: &[u8], covered: usize) -> u16 {
    bytes.iter().take(covered).enumerate().filter(|(at, _)| !(SMS_HEADER..SMS_HEADER + 16).contains(at)).fold(0u16, |sum, (_, b)| sum.wrapping_add(u16::from(*b)))
}

/// A Master System or Game Gear cartridge made ready to run: filled out to a size the console knows (32K at
/// least), with the header at $7FF0 written ("TMR SEGA", which the export Master System's BIOS won't run a
/// cartridge without, its region and size) and its checksum fixed. A header the program wrote itself keeps its
/// product code and version.
pub fn fix_sms(mut bytes: Vec<u8>, game_gear: bool) -> Result<Vec<u8>, String> {
    if bytes.len() > 0x100000 {
        return Err(crate::i18n::tf!("A Master System cartridge holds 1MB at most, and this is {} bytes.", bytes.len()));
    }
    let had = sms_header(&bytes).filter(|(at, _)| *at == SMS_HEADER).is_some();
    if !had && bytes.get(SMS_HEADER..(SMS_HEADER + 16).min(bytes.len())).is_some_and(|header| header.iter().any(|b| *b != 0 && *b != 0xFF)) {
        return Err(crate::i18n::tr("The program has code or data at $7FF0 to $7FFF, where the cartridge's header goes: move it, or leave those 16 bytes free.").into());
    }
    let (code, size) = sms_size_code(bytes.len());
    bytes.resize(size, 0xFF);
    let header = &mut bytes[SMS_HEADER..SMS_HEADER + 16];
    if !had {
        header.copy_from_slice(&[0; 16]);
        header[..8].copy_from_slice(b"TMR SEGA");
    }
    // The region: an export Master System, or a Game Gear sold everywhere.
    let region = if game_gear { 0x7 } else { 0x4 };
    header[15] = region << 4 | code;
    let sum = sms_checksum(&bytes, size);
    bytes[SMS_HEADER + 10..SMS_HEADER + 12].copy_from_slice(&sum.to_le_bytes());
    Ok(bytes)
}

/// Whether bytes are a ColecoVision cartridge: it starts (at $8000) with AA 55, which shows the BIOS's title
/// screen first, or 55 AA, which goes straight to the game.
pub fn is_coleco(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xAA, 0x55]) || bytes.starts_with(&[0x55, 0xAA])
}

/// A ColecoVision cartridge, as it goes in the slot at $8000: filled out to a whole 8K.
pub fn fix_coleco(mut bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !is_coleco(&bytes) {
        return Err(crate::i18n::tr("A ColecoVision cartridge starts at $8000 with DB $55,$AA (or $AA,$55 to show the BIOS's title screen), or the console won't start it.").into());
    }
    if bytes.len() > 0x8000 {
        return Err(crate::i18n::tf!("A ColecoVision cartridge holds 32K, and this is {} bytes.", bytes.len()));
    }
    bytes.resize(bytes.len().div_ceil(0x2000) * 0x2000, 0xFF);
    Ok(bytes)
}

/// An SG-1000 cartridge, from address 0: filled out to a whole 8K, and to 16K at least, as MAME (0.242) takes a
/// cartridge of 8K that isn't in its list for a different kind and won't start it. The console doesn't mind.
pub fn fix_sg1000(mut bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.len() > 0xC000 {
        return Err(crate::i18n::tf!("An SG-1000 cartridge holds 48K, and this is {} bytes.", bytes.len()));
    }
    bytes.resize(bytes.len().div_ceil(0x2000).max(2) * 0x2000, 0xFF);
    Ok(bytes)
}

/// Which console Z80 source says it's for, near its top: ; machine: sms (or gamegear, sg1000 or coleco).
pub fn machine_z80<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<crate::running::Machine> {
    use crate::running::Machine;
    said_machine(lines).and_then(|value| match value.as_str() {
        "sms" | "mastersystem" | "segamastersystem" | "markiii" => Some(Machine::MasterSystem),
        "gg" | "gamegear" | "segagamegear" => Some(Machine::GameGear),
        "sg1000" | "sg-1000" | "sc3000" | "sc-3000" => Some(Machine::Sg1000),
        "coleco" | "colecovision" => Some(Machine::ColecoVision),
        _ => None,
    })
}

/// Whether 68000 source says it's for the Mega Drive: ; machine: megadrive (or genesis).
pub fn says_mega_drive<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    said_machine(lines).is_some_and(|value| matches!(value.as_str(), "megadrive" | "genesis" | "segamegadrive" | "segagenesis" | "md"))
}

/// What a `; machine:` comment near the top says, in lower case without spaces.
fn said_machine<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<String> {
    lines.into_iter().take(50).find_map(|line| {
        let line = line.trim_start();
        let comment = line.strip_prefix(';').or_else(|| line.strip_prefix('*')).or_else(|| line.strip_prefix("//"))?;
        let comment = comment.trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase();
        Some(comment.strip_prefix("machine")?.trim_start().strip_prefix(':')?.trim().replace(' ', ""))
    })
}

/// The Z80 program the Mega Drive starter puts in the Z80's memory, as its bytes (the starter is 68000 source,
/// so it can't hold Z80 instructions); the test checks they're what this assembles to.
#[cfg(test)]
const MD_Z80_SOURCE: &str = "        org 0
        di
        ld hl,$7F11         ; the sound chip (PSG), as the Z80 sees it
        ld (hl),$8E         ; tone 0: the low 4 bits of its period
        ld (hl),$0F         ; and the high 6: 254, which is 440Hz
        ld (hl),$94         ; tone 0's volume: fairly loud
        ld b,2
wait:   ld de,0
inner:  dec de
        ld a,d
        or e
        jr nz,inner
        djnz wait
        ld (hl),$9F         ; silent again
        halt
";

/// A Mega Drive program: the 68000's vectors and the cartridge's header, the VDP set up, a box of tiles drawn
/// with its colour changing every few frames, and a short note played by the Z80 through the sound chip.
pub const MEGA_DRIVE_STARTER: &str = r#"; machine: megadrive
; A Sega Mega Drive (Genesis) cartridge. F9 sets its header's ROM end and checksum, and runs it in BlastEm or
; MAME. It sets up the VDP (the video chip), draws a box of tiles whose colour changes as it runs, and has the Z80
; play a note through the sound chip, as Mega Drive games leave their sound to the Z80.

VDP_DATA        equ $C00000         ; the VDP's data port
VDP_CTRL        equ $C00004         ; its control port: registers set, and where the next data goes
VERSION         equ $A10001         ; the console's version and region
TMSS            equ $A14000         ; the lock that later consoles put on the VDP
Z80_RAM         equ $A00000         ; the Z80's 8K, as the 68000 sees it
Z80_BUSREQ      equ $A11100
Z80_RESET       equ $A11200
Frames          equ $FF0000         ; in RAM: counted by the vertical blank interrupt

        org 0
; The 68000's vectors: the stack, where it starts, then its exceptions and interrupts.
        dc.l $00FFFE00              ; the stack, near the top of RAM
        dc.l Start
        dcb.l 22,Ignore             ; bus error, address error, illegal instruction and the rest
        dc.l Ignore                 ; spurious interrupt
        dc.l Ignore,Ignore,Ignore   ; levels 1 to 3 (2 is the joypad port's)
        dc.l Ignore                 ; level 4: the horizontal blank
        dc.l Ignore                 ; level 5
        dc.l VBlank                 ; level 6: the vertical blank, once a frame
        dc.l Ignore                 ; level 7
        dcb.l 32,Ignore             ; TRAPs and the rest

; The header, at $100. F9 fills in its checksum and ROM end.
        dc.b "SEGA MEGA DRIVE "     ; the console checks for SEGA here
        dc.b "(C)LIMO 2026.NOV"     ; who, and when
        dc.b "LIMONITER STARTER                               "     ; its name in Japan
        dc.b "LIMONITER STARTER                               "     ; and everywhere else
        dc.b "GM 00000000-00"       ; a game, its product number and version
        dc.w 0                      ; the checksum
        dc.b "J               "     ; what it uses: J, a three-button pad
        dc.l 0,0                    ; where the ROM starts and ends
        dc.l $FF0000,$FFFFFF        ; where RAM starts and ends
        dc.b "            "         ; no battery-backed RAM
        dc.b "            "         ; no modem
        dc.b "                                        "     ; notes
        dc.b "JUE             "     ; for Japan, the USA and Europe

Start:
        move.w  #$2700,sr           ; no interrupts while it's set up
        ; The TMSS: every console from the VA7 board on keeps the VDP locked until SEGA is written here.
        move.b  VERSION,d0
        andi.b  #$0F,d0
        beq.s   .unlocked
        move.l  #$53454741,TMSS     ; "SEGA"
.unlocked:
        lea     VDP_CTRL,a0
        lea     VDP_DATA,a1
        tst.w   (a0)                ; reading the control port forgets a half-written command
        ; The VDP's registers, display off while the screen's made.
        lea     VdpRegisters(pc),a2
        moveq   #(VdpRegistersEnd-VdpRegisters)/2-1,d0
.registers:
        move.w  (a2)+,(a0)
        dbra    d0,.registers
        ; All of video memory cleared, then the colours, and the vertical scroll.
        moveq   #0,d1
        move.l  #$40000000,(a0)     ; write video memory, from 0
        move.w  #$7FFF,d0
.vram:  move.w  d1,(a1)             ; MOVE, not CLR: on a 68000, CLR reads the port first
        dbra    d0,.vram
        move.l  #$40000010,(a0)     ; write the vertical scroll memory, from 0
        moveq   #39,d0
.vsram: move.w  d1,(a1)
        dbra    d0,.vsram
        move.l  #$C0000000,(a0)     ; write the colours, from 0
        lea     Palette(pc),a2
        moveq   #15,d0
.cram:  move.w  (a2)+,(a1)
        dbra    d0,.cram
        ; Tile 1, at $0020: a square with a border.
        move.l  #$40200000,(a0)
        lea     Tile(pc),a2
        moveq   #7,d0
.tile:  move.l  (a2)+,(a1)
        dbra    d0,.tile
        ; A box of tile 1 on plane A (at $C000, 64 tiles a row): 8 rows of 20, in the middle of the screen.
        move.w  #$C000+(10*64+10)*2,d1
        moveq   #7,d2
.row:   bsr.s   SetVramWrite
        moveq   #19,d0
.column:
        move.w  #1,(a1)             ; tile 1, colours 0 to 15, in front
        dbra    d0,.column
        add.w   #64*2,d1
        dbra    d2,.row
        bsr.s   StartZ80
        move.w  #$8164,(a0)         ; the display on, with the vertical blank interrupt
        move.w  #$2500,sr           ; and interrupts: level 6 and up
.loop:
        move.w  Frames,d0
.wait:  cmp.w   Frames,d0           ; wait for the next frame
        beq.s   .wait
        ; Colour 1 changes every eight frames.
        lsr.w   #3,d0
        andi.w  #7,d0
        add.w   d0,d0
        move.l  #$C0020000,(a0)     ; write the colours, from colour 1
        move.w  Colours(pc,d0.w),(a1)
        bra.s   .loop

Colours:
        dc.w $000E,$00EE,$00E0,$0EE0,$0E00,$0E0E,$0EEE,$0888

; Video memory is written from where d1 says: the VDP takes the address in two halves, the top two bits last.
SetVramWrite:
        moveq   #0,d3
        move.w  d1,d3
        andi.w  #$3FFF,d3
        swap    d3
        move.w  d1,d4
        rol.w   #2,d4
        andi.w  #3,d4
        or.w    d4,d3
        ori.l   #$40000000,d3
        move.l  d3,(a0)
        rts

; The Z80 is given its program: its bus taken from it, the program copied in, then it's reset and let go.
StartZ80:
        move.w  #$0100,Z80_BUSREQ   ; ask for the Z80's bus
        move.w  #$0100,Z80_RESET    ; out of reset, or the bus is never handed over
.bus:   btst    #0,Z80_BUSREQ       ; 0 once it's the 68000's
        bne.s   .bus
        lea     Z80Program(pc),a2
        lea     Z80_RAM,a3
        move.w  #Z80ProgramEnd-Z80Program-1,d0
.copy:  move.b  (a2)+,(a3)+
        dbra    d0,.copy
        move.w  #0,Z80_RESET        ; reset it, so it starts from 0
        moveq   #31,d0
.reset: dbra    d0,.reset           ; long enough for the reset to take
        move.w  #0,Z80_BUSREQ       ; its bus back
        move.w  #$0100,Z80_RESET    ; and away it goes
        rts

VBlank:
        addq.w  #1,Frames
Ignore:
        rte

; Each word is $8000, the register's number times $100, and its value.
VdpRegisters:
        dc.w $8004                  ; 0: no horizontal interrupt
        dc.w $8104                  ; 1: display off for now; Mega Drive mode
        dc.w $8230                  ; 2: plane A's tiles at $C000
        dc.w $8334                  ; 3: the window's at $D000
        dc.w $8407                  ; 4: plane B's at $E000
        dc.w $8578                  ; 5: the sprites' table at $F000
        dc.w $8600
        dc.w $8700                  ; 7: the background is colour 0
        dc.w $8800,$8900,$8A00
        dc.w $8B00                  ; 11: the whole screen scrolls as one
        dc.w $8C81                  ; 12: 320 pixels wide
        dc.w $8D3F                  ; 13: the horizontal scroll table at $FC00
        dc.w $8E00
        dc.w $8F02                  ; 15: each write moves on 2 bytes
        dc.w $9001                  ; 16: planes of 64 by 32 tiles
        dc.w $9100,$9200            ; no window
VdpRegistersEnd:

; Colours are 0000 BBB0 GGG0 RRR0.
Palette:
        dc.w $0400,$0EEE,$0EEE,$0000,$0000,$0000,$0000,$0000
        dc.w $0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000

; Four bits a pixel, eight pixels a long.
Tile:
        dc.l $22222222,$21111112,$21111112,$21111112
        dc.l $21111112,$21111112,$21111112,$22222222

; The Z80's program (its source is in Limoniter's tests): a note on the sound chip for about a second.
Z80Program:
        dc.b $F3,$21,$11,$7F,$36,$8E,$36,$0F,$36,$94,$06,$02,$11,$00,$00,$1B
        dc.b $7A,$B3,$20,$FB,$10,$F6,$36,$9F,$76
Z80ProgramEnd:
        even
"#;

/// A Master System program: its interrupts, the VDP in its own mode (mode 4) with a box of tiles whose colour
/// changes every few frames. F9 adds the header.
pub const MASTER_SYSTEM_STARTER: &str = r#"; machine: sms
; A Sega Master System cartridge. F9 writes its header (TMR SEGA, which the export BIOS needs) and checksum, and
; runs it in Emulicious, Gearsystem or MAME. It sets up the VDP and draws a box of tiles whose colour changes.

VDP_DATA        equ $BE
VDP_CONTROL     equ $BF
Frames          equ $C000           ; in RAM: counted by the frame interrupt

        org 0
        di
        im 1                        ; interrupts to $38
        ld sp,$DFF0
        jp Main

        org $38
; The frame interrupt: reading the VDP's status says it's been seen.
Interrupt:
        push af
        in a,(VDP_CONTROL)
        ld a,(Frames)
        inc a
        ld (Frames),a
        pop af
        ei
        reti

        org $66
; The pause button.
Pause:  retn

Main:
        ld hl,VdpRegisters          ; the VDP's registers: each value, then $80 and the register's number
        ld b,VdpRegistersEnd-VdpRegisters
        ld c,VDP_CONTROL
        otir
        ; All 16K of video memory cleared.
        xor a
        out (VDP_CONTROL),a
        ld a,$40                    ; $40: to write video memory
        out (VDP_CONTROL),a
        ld bc,$4000
ClearVram:
        xor a
        out (VDP_DATA),a
        dec bc
        ld a,b
        or c
        jr nz,ClearVram
        ; The colours: 00BBGGRR, 16 for the tiles and 16 for the sprites.
        xor a
        out (VDP_CONTROL),a
        ld a,$C0                    ; $C0: to write the colours
        out (VDP_CONTROL),a
        ld hl,Palette
        ld b,32
        ld c,VDP_DATA
        otir
        ; Tile 1, at $0020: four bytes a row, one from each bit plane.
        ld a,$20
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld hl,Tile
        ld b,32
        otir
        ; A box of tile 1 in the name table (at $3800, 32 tiles a row, two bytes each): 8 rows of 16.
        ld hl,$3800+(8*32+8)*2
        ld d,8
Row:    ld a,l
        out (VDP_CONTROL),a
        ld a,h
        or $40
        out (VDP_CONTROL),a
        ld b,16
Column: ld a,1                      ; tile 1
        out (VDP_DATA),a
        xor a                       ; its colours 0 to 15, not flipped
        out (VDP_DATA),a
        djnz Column
        ld bc,32*2
        add hl,bc
        dec d
        jr nz,Row
        ld a,$E0                    ; the display on, with the frame interrupt
        out (VDP_CONTROL),a
        ld a,$81
        out (VDP_CONTROL),a
        ei
Loop:   halt                        ; wait for the frame interrupt
        ; Colour 1 changes every eight frames.
        ld a,(Frames)
        rrca
        rrca
        rrca
        and 7
        ld e,a
        ld d,0
        ld hl,Colours
        add hl,de
        ld a,1
        out (VDP_CONTROL),a
        ld a,$C0
        out (VDP_CONTROL),a
        ld a,(hl)
        out (VDP_DATA),a
        jr Loop

Colours:
        db $03,$0F,$0C,$3C,$30,$33,$3F,$15

VdpRegisters:
        db $04,$80                  ; 0: mode 4, the Master System's own
        db $00,$81                  ; 1: display off for now
        db $FF,$82                  ; 2: the name table at $3800
        db $FF,$83,$FF,$84
        db $FF,$85                  ; 5: the sprites' table at $3F00
        db $FB,$86                  ; 6: the sprites' tiles from $0000
        db $00,$87                  ; 7: the border is colour 0 of the sprites'
        db $00,$88,$00,$89          ; no scrolling
        db $FF,$8A                  ; 10: no line interrupt
VdpRegistersEnd:

Palette:
        db $10,$3F,$3F,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00
        db $10,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00,$00

Tile:
        db $00,$FF,$00,$00          ; colour 2 all along
        db $7E,$81,$00,$00          ; colour 2, six of colour 1, colour 2
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $00,$FF,$00,$00
"#;

/// A Game Gear program: the Master System's, with the Game Gear's colours (12 bits, two bytes each) and the box
/// inside its smaller screen.
pub const GAME_GEAR_STARTER: &str = r#"; machine: gamegear
; A Sega Game Gear cartridge. F9 writes its header (TMR SEGA, with the Game Gear's region) and checksum, and runs
; it in Emulicious, Gearsystem or MAME. Its screen is the middle 160 by 144 pixels of the Master System's 256 by
; 192: columns 6 to 25 of the name table, and rows 3 to 20.

VDP_DATA        equ $BE
VDP_CONTROL     equ $BF
Frames          equ $C000           ; in RAM: counted by the frame interrupt

        org 0
        di
        im 1                        ; interrupts to $38
        ld sp,$DFF0
        jp Main

        org $38
; The frame interrupt: reading the VDP's status says it's been seen.
Interrupt:
        push af
        in a,(VDP_CONTROL)
        ld a,(Frames)
        inc a
        ld (Frames),a
        pop af
        ei
        reti

        org $66
Nmi:    retn                        ; (the Game Gear's Start button is read at port 0 instead)

Main:
        ld hl,VdpRegisters
        ld b,VdpRegistersEnd-VdpRegisters
        ld c,VDP_CONTROL
        otir
        ; All 16K of video memory cleared.
        xor a
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld bc,$4000
ClearVram:
        xor a
        out (VDP_DATA),a
        dec bc
        ld a,b
        or c
        jr nz,ClearVram
        ; The colours: two bytes each, GGGGRRRR then 0000BBBB, 32 of them.
        xor a
        out (VDP_CONTROL),a
        ld a,$C0
        out (VDP_CONTROL),a
        ld hl,Palette
        ld b,64
        ld c,VDP_DATA
        otir
        ; Tile 1, at $0020.
        ld a,$20
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld hl,Tile
        ld b,32
        otir
        ; A box of tile 1: 6 rows of 12, inside the Game Gear's screen.
        ld hl,$3800+(9*32+10)*2
        ld d,6
Row:    ld a,l
        out (VDP_CONTROL),a
        ld a,h
        or $40
        out (VDP_CONTROL),a
        ld b,12
Column: ld a,1
        out (VDP_DATA),a
        xor a
        out (VDP_DATA),a
        djnz Column
        ld bc,32*2
        add hl,bc
        dec d
        jr nz,Row
        ld a,$E0                    ; the display on, with the frame interrupt
        out (VDP_CONTROL),a
        ld a,$81
        out (VDP_CONTROL),a
        ei
Loop:   halt
        ; Colour 1 changes every eight frames.
        ld a,(Frames)
        rrca
        rrca
        and 7*2
        ld e,a
        ld d,0
        ld hl,Colours
        add hl,de
        ld a,2                      ; colour 1 is at byte 2
        out (VDP_CONTROL),a
        ld a,$C0
        out (VDP_CONTROL),a
        ld a,(hl)
        out (VDP_DATA),a
        inc hl
        ld a,(hl)
        out (VDP_DATA),a
        jr Loop

Colours:
        dw $000F,$00FF,$00F0,$0FF0,$0F00,$0F0F,$0FFF,$0888

VdpRegisters:
        db $04,$80,$00,$81,$FF,$82,$FF,$83,$FF,$84,$FF,$85,$FB,$86,$00,$87,$00,$88,$00,$89,$FF,$8A
VdpRegistersEnd:

Palette:
        dw $0400,$0FFF,$0FFF,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000
        dw $0400,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000,$0000

Tile:
        db $00,$FF,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $7E,$81,$00,$00
        db $00,$FF,$00,$00
"#;

/// An SG-1000 program: the TMS9918's Graphics I mode, a box of characters whose colour changes, all done in the
/// frame interrupt.
pub const SG1000_STARTER: &str = r#"; machine: sg1000
; A Sega SG-1000 cartridge. F9 runs it in Emulicious, Gearsystem or MAME. It sets up the video chip (a TI
; TMS9918A, as in the MSX and the ColecoVision) in Graphics I mode, and draws a box of characters whose colour
; changes. All the video chip's work after the start is done in the frame interrupt: the chip has one address for
; reading and writing, and an interrupt that used it while the main program was half-way through setting it would
; send the main program's bytes somewhere else.

VDP_DATA        equ $BE
VDP_CONTROL     equ $BF
Frames          equ $C000           ; in RAM (1K, at $C000)

        org 0
        di
        im 1
        ld sp,$C400
        jp Main

        org $38
; The frame interrupt: reading the status says it's been seen. Then colours 0 to 7's entry in the colour table
; is changed every eight frames.
Interrupt:
        push af
        push hl
        in a,(VDP_CONTROL)
        ld hl,Frames
        inc (hl)
        xor a                       ; the colour table, at $2000
        out (VDP_CONTROL),a
        ld a,$20+$40
        out (VDP_CONTROL),a
        ld a,(hl)
        and $78
        add a,a                     ; the foreground colour in the top four bits, the background clear
        add a,$20
        out (VDP_DATA),a
        pop hl
        pop af
        ei
        reti

        org $66
Pause:  retn                        ; the pause button

Main:
        ld hl,VdpRegisters
        ld b,VdpRegistersEnd-VdpRegisters
        ld c,VDP_CONTROL
        otir
        ; All 16K of video memory cleared.
        xor a
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld bc,$4000
ClearVram:
        xor a
        out (VDP_DATA),a
        dec bc
        ld a,b
        or c
        jr nz,ClearVram
        ; Character 1's pattern, at $0008: a square outline.
        ld a,$08
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld hl,Pattern
        ld b,8
        ld c,VDP_DATA
        otir
        ; A box of character 1 in the name table (at $1800, 32 a row): 8 rows of 16.
        ld hl,$1800+8*32+8
        ld d,8
Row:    ld a,l
        out (VDP_CONTROL),a
        ld a,h
        or $40
        out (VDP_CONTROL),a
        ld b,16
        ld a,1
Column: out (VDP_DATA),a
        djnz Column
        ld bc,32
        add hl,bc
        dec d
        jr nz,Row
        ld a,$E0                    ; 16K, the display on, the frame interrupt on, Graphics I
        out (VDP_CONTROL),a
        ld a,$81
        out (VDP_CONTROL),a
        ei
Loop:   jr Loop                     ; everything else happens in the interrupt

VdpRegisters:
        db $00,$80                  ; 0: Graphics I
        db $A0,$81                  ; 1: 16K, display off for now
        db $06,$82                  ; 2: the name table at $1800
        db $80,$83                  ; 3: the colour table at $2000
        db $00,$84                  ; 4: the patterns at $0000
        db $36,$85                  ; 5: the sprites' table at $1B00
        db $07,$86                  ; 6: the sprites' patterns at $3800
        db $04,$87                  ; 7: a dark blue backdrop
VdpRegistersEnd:

Pattern:
        db $FF,$81,$81,$99,$99,$81,$81,$FF
"#;

/// A ColecoVision program: the cartridge's header at $8000, the same TMS9918A as the SG-1000 (but its frame
/// interrupt is the Z80's NMI), and a box of characters whose colour changes.
pub const COLECO_STARTER: &str = r#"; machine: coleco
; A ColecoVision cartridge. F9 runs it in Emulicious, Gearcoleco or MAME (each needs the ColecoVision's BIOS).
; The cartridge starts at $8000 with its header. The video chip is a TI TMS9918A, as in the SG-1000 and the MSX;
; its frame interrupt is the Z80's NMI, which can't be switched off, so all the video chip's work after the start
; is done there: the chip has one address for reading and writing, and an NMI that used it while the main
; program was half-way through setting it would send the main program's bytes somewhere else.

VDP_DATA        equ $BE
VDP_CONTROL     equ $BF
Frames          equ $7000           ; in RAM (1K, at $7000)
Ready           equ $7001           ; set once the screen's made, so the NMI leaves the chip alone till then

        org $8000
        db $55,$AA                  ; straight to the game ($AA,$55 shows the BIOS's title screen first)
        dw 0                        ; the BIOS's sprite tables, work area and controller map:
        dw 0                        ; this program doesn't use the BIOS's routines
        dw 0
        dw 0
        dw Start                    ; where it starts
        jp Nothing                  ; RST $08 to $30
        jp Nothing
        jp Nothing
        jp Nothing
        jp Nothing
        jp Nothing
        jp Nothing                  ; the maskable interrupt (from the expansion port)
        jp Nmi                      ; the NMI: the video chip's frame interrupt

Nothing:
        ret

Start:
        ld sp,$7400
        xor a
        ld (Ready),a
        ld hl,VdpRegisters
        ld b,VdpRegistersEnd-VdpRegisters
        ld c,VDP_CONTROL
        otir
        ; All 16K of video memory cleared.
        xor a
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld bc,$4000
ClearVram:
        xor a
        out (VDP_DATA),a
        dec bc
        ld a,b
        or c
        jr nz,ClearVram
        ; Character 1's pattern, at $0008.
        ld a,$08
        out (VDP_CONTROL),a
        ld a,$40
        out (VDP_CONTROL),a
        ld hl,Pattern
        ld b,8
        ld c,VDP_DATA
        otir
        ; A box of character 1 in the name table (at $1800, 32 a row): 8 rows of 16.
        ld hl,$1800+8*32+8
        ld d,8
Row:    ld a,l
        out (VDP_CONTROL),a
        ld a,h
        or $40
        out (VDP_CONTROL),a
        ld b,16
        ld a,1
Column: out (VDP_DATA),a
        djnz Column
        ld bc,32
        add hl,bc
        dec d
        jr nz,Row
        ld a,1
        ld (Ready),a
        ld a,$E0                    ; 16K, the display on, the frame interrupt on, Graphics I
        out (VDP_CONTROL),a
        ld a,$81
        out (VDP_CONTROL),a
Loop:   jr Loop                     ; everything else happens in the NMI

; The frame interrupt: reading the status says it's been seen (or there's never another). Then colours 0 to 7's
; entry in the colour table is changed every eight frames.
Nmi:
        push af
        push hl
        in a,(VDP_CONTROL)
        ld a,(Ready)
        or a
        jr z,NmiDone
        ld hl,Frames
        inc (hl)
        xor a                       ; the colour table, at $2000
        out (VDP_CONTROL),a
        ld a,$20+$40
        out (VDP_CONTROL),a
        ld a,(hl)
        and $78
        add a,a
        add a,$20
        out (VDP_DATA),a
NmiDone:
        pop hl
        pop af
        retn

VdpRegisters:
        db $00,$80                  ; 0: Graphics I
        db $A0,$81                  ; 1: 16K, display off for now
        db $06,$82                  ; 2: the name table at $1800
        db $80,$83                  ; 3: the colour table at $2000
        db $00,$84                  ; 4: the patterns at $0000
        db $36,$85                  ; 5: the sprites' table at $1B00
        db $07,$86                  ; 6: the sprites' patterns at $3800
        db $04,$87                  ; 7: a dark blue backdrop
VdpRegistersEnd:

Pattern:
        db $FF,$81,$81,$99,$99,$81,$81,$FF
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::running::{Format, Machine, Runnable};

    fn assemble(cpu: crate::assemblers::Cpu, source: &str) -> crate::asm::Assembly {
        let assembly = crate::assemblers::assemble_own_at(cpu, std::path::Path::new("t.asm"), source, 0);
        assert!(assembly.problems.is_empty(), "{:?}", assembly.problems);
        assembly
    }

    fn image(assembly: &crate::asm::Assembly) -> Vec<u8> {
        assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap())
    }

    #[test]
    fn the_mega_drive_starter_is_a_cartridge_with_its_checksum() {
        let bytes = fix_mega_drive(image(&assemble(crate::assemblers::Cpu::M68k, MEGA_DRIVE_STARTER))).unwrap();
        assert!(is_mega_drive(&bytes));
        // The header's parts are where the console and emulators look for them.
        assert_eq!(&bytes[0x100..0x110], b"SEGA MEGA DRIVE ");
        assert_eq!(&bytes[0x180..0x18E], b"GM 00000000-00");
        assert_eq!(&bytes[0x1A8..0x1B0], &[0, 0xFF, 0, 0, 0, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&bytes[0x1F0..0x1F3], b"JUE");
        assert_eq!(u32::from_be_bytes(bytes[0x1A4..0x1A8].try_into().unwrap()) as usize, bytes.len() - 1);
        // The checksum, worked out here another way: every word after the header.
        let mut sum = 0u32;
        for at in (0x200..bytes.len()).step_by(2) {
            sum += u32::from(bytes[at]) << 8 | u32::from(bytes[at + 1]);
        }
        assert_eq!(u16::from_be_bytes([bytes[0x18E], bytes[0x18F]]), sum as u16);
        assert_ne!(sum, 0);
        // The vectors: the stack, then the start just after the header.
        assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()), 0x200);
        let run = Runnable::from_file("t.md", bytes).unwrap();
        assert_eq!((run.machine, run.format), (Machine::MegaDrive, Format::MdRom));
    }

    #[test]
    fn the_mega_drive_starters_z80_program_is_what_its_source_assembles_to() {
        let z80 = image(&assemble(crate::assemblers::Cpu::Z80, MD_Z80_SOURCE));
        let listed: Vec<u8> = MEGA_DRIVE_STARTER
            .split("Z80Program:")
            .nth(1)
            .unwrap()
            .lines()
            .take_while(|line| !line.starts_with("Z80ProgramEnd"))
            .filter_map(|line| line.trim().strip_prefix("dc.b"))
            .flat_map(|bytes| bytes.split(',').map(|b| u8::from_str_radix(b.trim().trim_start_matches('$'), 16).unwrap()).collect::<Vec<_>>())
            .collect();
        assert_eq!(listed, z80);
    }

    #[test]
    fn master_system_and_game_gear_headers_are_written_and_checked() {
        let sms = fix_sms(image(&assemble(crate::assemblers::Cpu::Z80, MASTER_SYSTEM_STARTER)), false).unwrap();
        assert_eq!(sms.len(), 0x8000);
        assert_eq!(&sms[0x7FF0..0x7FF8], b"TMR SEGA");
        assert_eq!(sms[0x7FFF], 0x4C, "an export Master System's, 32K");
        let sum: u32 = sms[..0x7FF0].iter().map(|b| u32::from(*b)).sum();
        assert_eq!(u16::from_le_bytes([sms[0x7FFA], sms[0x7FFB]]), sum as u16);
        assert_eq!(sms_header(&sms), Some((0x7FF0, 4)));
        let run = Runnable::from_file("t.sms", sms).unwrap();
        assert_eq!(run.machine, Machine::MasterSystem);

        let gg = fix_sms(image(&assemble(crate::assemblers::Cpu::Z80, GAME_GEAR_STARTER)), true).unwrap();
        assert_eq!(gg[0x7FFF], 0x7C, "a Game Gear's, sold everywhere, 32K");
        assert_eq!(Runnable::from_file("t.gg", gg).unwrap().machine, Machine::GameGear);

        // Bigger cartridges: the checksum covers the bytes after the header too, up to the size given.
        let mut big = vec![1u8; 0x9000];
        big[0x7FF0..0x8000].fill(0);
        let big = fix_sms(big, false).unwrap();
        assert_eq!((big.len(), big[0x7FFF] & 15), (0x10000, 0xE));
        let sum: u32 = big.iter().enumerate().filter(|(at, _)| !(0x7FF0..0x8000).contains(at)).map(|(_, b)| u32::from(*b)).sum();
        assert_eq!(u16::from_le_bytes([big[0x7FFA], big[0x7FFB]]), sum as u16);
        // A header of its own keeps its product code; code where the header goes is refused.
        let mut own = vec![0u8; 0x8000];
        own[0x7FF0..0x7FF8].copy_from_slice(b"TMR SEGA");
        own[0x7FFC] = 0x26;
        assert_eq!(fix_sms(own, false).unwrap()[0x7FFC], 0x26);
        let mut clash = vec![0u8; 0x8000];
        clash[0x7FF4] = 0xC9;
        assert!(fix_sms(clash, false).is_err());
    }

    #[test]
    fn coleco_and_sg1000_cartridges_are_made() {
        let assembly = assemble(crate::assemblers::Cpu::Z80, COLECO_STARTER);
        assert_eq!(assembly.lowest(), Some(0x8000));
        let coleco = fix_coleco(image(&assembly)).unwrap();
        assert_eq!((coleco.len(), &coleco[..2]), (0x2000, &[0x55, 0xAA][..]));
        // Where it starts, and the NMI's jump at $8021.
        let start = usize::from(u16::from_le_bytes([coleco[10], coleco[11]]));
        assert_eq!(coleco[start - 0x8000..start - 0x8000 + 3], [0x31, 0x00, 0x74], "LD SP,$7400");
        assert_eq!(coleco[0x21], 0xC3, "JP at the NMI's place");
        assert_eq!(Runnable::from_file("t.col", coleco).unwrap().machine, Machine::ColecoVision);
        assert!(fix_coleco(vec![1, 2, 3]).is_err());

        let sg = fix_sg1000(image(&assemble(crate::assemblers::Cpu::Z80, SG1000_STARTER))).unwrap();
        assert_eq!(sg.len(), 0x4000);
        assert_eq!(sg[0x38], 0xF5, "PUSH AF, the interrupt at $38");
        assert_eq!(Runnable::from_file("t.sg", sg).unwrap().machine, Machine::Sg1000);
    }

    /// Writes each starter's cartridge to target/consoles, made as F9 makes it, to run in an emulator.
    #[test]
    #[ignore]
    fn write_the_starters() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/consoles");
        std::fs::create_dir_all(&out).unwrap();
        for (cpu, source) in [(crate::assemblers::Cpu::M68k, MEGA_DRIVE_STARTER), (crate::assemblers::Cpu::Z80, MASTER_SYSTEM_STARTER), (crate::assemblers::Cpu::Z80, GAME_GEAR_STARTER), (crate::assemblers::Cpu::Z80, SG1000_STARTER), (crate::assemblers::Cpu::Z80, COLECO_STARTER)] {
            let assembled = crate::assemblers::assemble_own_to_run(cpu, std::path::Path::new("starter.asm"), source);
            let Ok(crate::app::asm_host::Made::Run(run)) = crate::app::asm_host::made_from(cpu, &assembled, "starter", Default::default()) else { panic!("not made") };
            std::fs::write(out.join(&run.name), run.bytes.as_slice()).unwrap();
        }
    }

    #[test]
    fn the_source_says_which_console() {
        assert_eq!(machine_z80(["; machine: sms"]), Some(Machine::MasterSystem));
        assert_eq!(machine_z80(["; Machine: Game Gear"]), Some(Machine::GameGear));
        assert_eq!(machine_z80(["; machine: sg-1000"]), Some(Machine::Sg1000));
        assert_eq!(machine_z80(["; machine: ColecoVision"]), Some(Machine::ColecoVision));
        assert_eq!(machine_z80(["; machine: spectrum"]), None);
        assert!(says_mega_drive(["* machine: genesis"]));
        assert!(!says_mega_drive(["; machine: amiga"]));
    }
}
