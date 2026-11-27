; How To Code: the ZX Spectrum - startup.asm, the startup every example is built on. CC0.
;
; It takes the machine from BASIC, runs your program at 50 frames a second in interrupt mode 2, and gives the
; machine back to BASIC (with a 0 OK) when SPACE is pressed, or after AUTOEXIT frames. Include it first, after
; DEVICE, and define three routines:
;
;   Init        called once, with interrupts off, before the first frame
;   Frame       called once a frame, in the main loop, after the interrupt
;   Interrupt   called from the interrupt itself, at a fixed time after the frame starts (for beam racing)
;
; Each may change any register but SP and I. Memory, as the startup leaves it:
;
;   $4000-$5AFF the screen (bank 5)          $8000-$BBFF your program (bank 2, never contended)
;   $5B00-$7FFF BASIC, its loader, and free   $BC00-$BDFF the stack
;   $BE00-$BF00 the IM 2 vector table        $BFBF       a JP to the interrupt handler
;   $C000-$FFFF free on a 48K; on a 128K, whichever bank is paged in (bank 0 at first)
;
; It works on the 48K, the 128K and +2, the +2A and +3, and the Pentagon. Is128 says whether 128K paging works,
; and LineTime is the T-states of a scan line it found (224 on a 48K, 228 on a 128K or +2A/+3; the Pentagon is a
; 128 with 224, which this can't tell). Chapter 4, Startup and interrupts, explains every line.

        ifndef AUTOEXIT
AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)
        endif

        org     $8000

IM2_TABLE       equ     $BE00           ; 257 bytes, all IM2_JUMP_HI, so any byte on the bus finds the jump
IM2_JUMP        equ     $BFBF
STACK_TOP       equ     $BE00
BANKM           equ     $5B5C           ; the 128K ROM's copy of what was last written to port $7FFD
BORDCR          equ     $5C48           ; BASIC's border colour, in bits 3 to 5

Start:
        di
        ld      (BasicSp),sp            ; BASIC's stack, and its H'L', which BASIC needs back
        exx
        ld      (BasicHl),hl
        exx
        ld      sp,STACK_TOP
        call    FindMachine
        ; The vector table: 257 bytes of $BF, so whatever byte is on the data bus when the interrupt is taken,
        ; the Z80 reads $BFBF from the table and jumps there.
        ld      hl,IM2_TABLE
        ld      de,IM2_TABLE+1
        ld      bc,256
        ld      (hl),IM2_JUMP/256
        ldir
        ld      a,$C3                   ; JP IntHandler
        ld      (IM2_JUMP),a
        ld      hl,IntHandler
        ld      (IM2_JUMP+1),hl
        ld      a,IM2_TABLE/256
        ld      i,a
        im      2
        call    Init
        ei
MainLoop:
        halt                            ; wait for the next frame's interrupt
        call    Frame
        ld      a,$7F                   ; the half-row SPACE, SYM SHIFT, M, N, B
        in      a,($FE)
        rra                             ; SPACE is bit 0, 0 when it's pressed
        jr      nc,Exit
        if AUTOEXIT
        ld      hl,(Frames)
        ld      de,AUTOEXIT
        or      a
        sbc     hl,de
        jr      nc,Exit
        endif
        jr      MainLoop

; Back to BASIC as it was: the 128K's paging, the ROM's interrupt mode, IY and H'L' for BASIC, its stack and
; its border.
Exit:
        di
        ld      a,(Is128)
        or      a
        jr      z,.paged
        ld      a,(BANKM)
        ld      bc,$7FFD
        out     (c),a
.paged:
        ld      a,$3F
        ld      i,a
        im      1
        ld      iy,$5C3A
        exx
        ld      hl,(BasicHl)
        exx
        ld      a,(BORDCR)
        rrca
        rrca
        rrca
        and     7
        out     ($FE),a
        ld      sp,(BasicSp)
        ei
        ret

; The interrupt: every register saved, the example's Interrupt called, and the frame counted.
IntHandler:
        push    af
        push    bc
        push    de
        push    hl
        ex      af,af'
        exx
        push    af
        push    bc
        push    de
        push    hl
        push    ix
        push    iy
        call    Interrupt
        ld      hl,(Frames)
        inc     hl
        ld      (Frames),hl
        pop     iy
        pop     ix
        pop     hl
        pop     de
        pop     bc
        pop     af
        exx
        ex      af,af'
        pop     hl
        pop     de
        pop     bc
        pop     af
        ei
        reti

; DEBUG "text": prints a line of text in Limoniter's Output panel, named for the machine ("Spectrum: score 120"), while
; the program runs in Limoniter's own Spectrum. It writes the text to port $BB, which Limoniter's Spectrum
; listens to and a real Spectrum (48K, 128K, +2 or +3) ignores, so it does nothing there, and no harm. All
; registers are kept. The text follows the CALL, as data, so it costs the length of the text and five bytes:
; take the DEBUG lines out of a release.
        macro   DEBUG text
        call    DebugPrint
        db      text, 10, 0
        endm

DebugPrint:
        ex      (sp),hl                 ; HL is the text, which follows the CALL; the stack keeps the old HL
        push    af
.next:  ld      a,(hl)
        inc     hl
        out     ($BB),a                 ; the debug console port, ending with the 0 after the new line
        or      a
        jr      nz,.next
        pop     af
        ex      (sp),hl                 ; the old HL back, and the return address is the end of the text
        ret

; Is128 and LineTime. A 128K is one whose RAM at $C000 changes when another bank is paged in: a mark written
; there, a different bank paged, a second mark written, and the first bank paged back. On a 48K both marks go
; to the same byte (nothing answers port $7FFD).
FindMachine:
        ld      hl,$C000
        ld      d,(hl)                  ; what was there, to put back
        ld      (hl),0
        ld      a,(BANKM)
        xor     1                       ; another bank, keeping the ROM and screen as they are
        ld      bc,$7FFD
        out     (c),a
        ld      (hl),1
        ld      a,(BANKM)
        out     (c),a
        ld      a,(hl)                  ; 0 if the second mark went elsewhere
        ld      (hl),d
        xor     1
        ld      (Is128),a
        ld      hl,224
        jr      z,.found
        ld      hl,228
.found:
        ld      (LineTime),hl
        ret

; ClearScreen: the pixels cleared, and every attribute made A.
ClearScreen:
        ld      hl,$4000
        ld      de,$4001
        ld      bc,6144
        ld      (hl),0
        push    af
        ldir
        pop     af
        ld      (hl),a                  ; HL is $5800, the attributes
        ld      bc,767
        ldir
        ret

; PixelAddress: B = y (0 to 191), C = x (0 to 255); returns HL, the byte holding that pixel, and A, the pixel's
; bit in it. The screen's lines are in an odd order (chapter 5): y is 010 y7 y6 y2 y1 y0 in the high byte, and
; y5 y4 y3 x7 x6 x5 x4 x3 in the low.
PixelAddress:
        ld      a,b
        and     7
        or      $40
        ld      h,a                     ; 010 00 y2 y1 y0
        ld      a,b
        rra
        rra
        rra
        and     $18
        or      h
        ld      h,a                     ; 010 y7 y6 y2 y1 y0
        ld      a,b
        rla
        rla
        and     $E0
        ld      l,a                     ; y5 y4 y3 0 0 0 0 0
        ld      a,c
        rra
        rra
        rra
        and     $1F
        or      l
        ld      l,a
        ld      a,c
        and     7
        ld      b,a
        ld      a,$80
        ret     z
.shift:
        rrca
        djnz    .shift
        ret

; PrintAt: the text at HL (ending with a 0) printed at character row B (0 to 23) and column C (0 to 31), in the
; ROM's own font, which is at $3D00 for the characters 32 to 127. Attributes are left as they are.
PrintAt:
        ld      a,(hl)
        or      a
        ret     z
        push    hl
        push    bc
        call    PrintChar
        pop     bc
        pop     hl
        inc     hl
        inc     c
        jr      PrintAt

; PrintChar: the character A at row B, column C.
PrintChar:
        ld      l,a
        ld      h,0
        add     hl,hl
        add     hl,hl
        add     hl,hl
        ld      de,$3C00                ; $3D00 less 32 characters of 8 bytes
        add     hl,de
        ex      de,hl                   ; DE: the character's eight bytes
        ld      a,b                     ; a character row starts at 010 r4 r3 000, r2 r1 r0 c4..c0
        and     $18
        or      $40
        ld      h,a
        ld      a,b
        rrca
        rrca
        rrca
        and     $E0
        or      c
        ld      l,a
        ld      b,8
.line:
        ld      a,(de)
        ld      (hl),a
        inc     de
        inc     h                       ; the next pixel line of a character is 256 bytes on
        djnz    .line
        ret

; PrintNumber: HL printed as five digits at row B, column C.
PrintNumber:
        push    bc
        ld      de,NumberText
        ld      bc,-10000
        call    .digit
        ld      bc,-1000
        call    .digit
        ld      bc,-100
        call    .digit
        ld      bc,-10
        call    .digit
        ld      a,l
        add     a,'0'
        ld      (de),a
        pop     bc
        ld      hl,NumberText
        jr      PrintAt
.digit:
        ld      a,'0'-1
.count:
        inc     a
        add     hl,bc
        jr      c,.count
        sbc     hl,bc
        ld      (de),a
        inc     de
        ret

NumberText:     db      "00000",0
Frames:         dw      0               ; frames since the start, counted by the interrupt
Is128:          db      0               ; 1 if 128K paging works
LineTime:       dw      224             ; T-states in a scan line
BasicSp:        dw      0
BasicHl:        dw      0
