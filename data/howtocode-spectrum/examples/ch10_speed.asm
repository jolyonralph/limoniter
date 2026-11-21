; ch10_speed.asm: three ways to fill 1,024 bytes, timed by the border. Each frame the border turns red while
; LDIR fills them (21 T-states a byte), green while an unrolled run of LD (HL),A and INC L does (11), and blue
; while PUSH does (5.5: two bytes in 11 T-states), so the height of each band is the time it took: about 96, 50
; and 25 scan lines. The value PUSH writes is put into its own LD DE instruction by the program as it runs
; (self-modifying code). Chapter 10, Speed. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

BUFFER          equ     $C000           ; 1,024 bytes clear of the screen, so never held up by the ULA

Init:
        ld      a,%00000111             ; white on black
        call    ClearScreen
        ld      hl,Lines
.line:
        ld      b,(hl)
        inc     b
        ret     z
        dec     b
        inc     hl
        ld      c,1
        call    PrintAt
        inc     hl
        jr      .line

Frame:
        ld      bc,400                  ; wait about 10,000 T-states, to line 45 or so, so the bands are in sight
.wait:
        dec     bc
        ld      a,b
        or      c
        jr      nz,.wait
        ld      a,(Frames)              ; the value to fill with: a different one each frame
        ld      (Value),a
        ; LDIR: the first byte set, then copied along. 21 T-states for each of the other 1,023.
        ld      a,2
        out     ($FE),a
        ld      hl,BUFFER
        ld      de,BUFFER+1
        ld      bc,1023
        ld      a,(Value)
        ld      (hl),a
        ldir
        ; Unrolled: 256 LD (HL),A and INC L for each 256-byte page, 11 T-states a byte, with no loop to count.
        ld      a,4
        out     ($FE),a
        ld      hl,BUFFER
        ld      a,(Value)
        ld      b,4
.page:
        rept    256
        ld      (hl),a
        inc     l
        endr
        inc     h
        dec     b
        jp      nz,.page
        ; PUSH: the stack pointed at the end of the buffer, and the same register pair pushed 512 times. The
        ; interrupts are off meanwhile: one would push its return address into the buffer.
        ld      a,1
        out     ($FE),a
        ld      a,(Value)
        ld      (PushValue+1),a         ; the value into the LD DE below: its low byte
        ld      (PushValue+2),a         ; and its high
        di
        ld      (SavedSp),sp
        ld      sp,BUFFER+1024
PushValue:
        ld      de,0                    ; the 0 is replaced before this runs
        rept    512
        push    de
        endr
        ld      sp,(SavedSp)
        ei
        xor     a
        out     ($FE),a
        ret

Interrupt:
        ret

Value:          db      0
SavedSp:        dw      0
Lines:          db      2,"1,024 bytes filled:",0
                db      4,"red    LDIR          21,504 T",0
                db      5,"green  LD (HL),A     11,264 T",0
                db      6,"blue   PUSH           5,632 T",0
                db      8,"(and the bands' heights: a",0
                db      9,"scan line is 224 T-states)",0
                db      $FF
